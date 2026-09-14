//! Replaces the old two-level generic CompositeMarketDataProvider nesting
//! (Upstox -> (Yahoo -> AlphaVantage), fixed at compile time) with a
//! provider whose order is read from settings at call time — because
//! Rust generics are resolved at compile time, they can't represent "the
//! user reorders this at runtime from Settings," which is exactly what
//! was asked for. This holds named trait-object providers instead and
//! iterates them in whatever order the current setting says.

use async_trait::async_trait;
use pm_domain::analytics::DailyBar;
use std::sync::Arc;

use super::{MarketDataError, MarketDataProvider, Quote};
use crate::sqlite::SqliteAppSettings;

pub const MARKET_DATA_PRIORITY_SETTING: &str = "market_data_priority_order";
/// Used when no priority has been explicitly set — same order as the
/// original hardcoded chain, so behavior is unchanged until the user
/// actually opens Settings and reorders something.
pub const DEFAULT_PRIORITY_ORDER: &str = "upstox,yahoo,alpha_vantage";

pub struct PrioritizedMarketDataProvider {
    providers: Vec<(String, Arc<dyn MarketDataProvider>)>,
    settings: Arc<SqliteAppSettings>,
}

impl PrioritizedMarketDataProvider {
    pub fn new(providers: Vec<(String, Arc<dyn MarketDataProvider>)>, settings: Arc<SqliteAppSettings>) -> Self {
        Self { providers, settings }
    }

    /// Reads the current priority order from settings and returns
    /// providers in that sequence. Any name in the setting that doesn't
    /// match a registered provider is ignored (rather than erroring) —
    /// and any registered provider NOT mentioned in the setting is
    /// appended at the end, so a provider added in a future update still
    /// gets tried even if the user's saved order predates it.
    async fn ordered_providers(&self) -> Vec<&Arc<dyn MarketDataProvider>> {
        let order_setting = self
            .settings
            .get(MARKET_DATA_PRIORITY_SETTING)
            .await
            .ok()
            .flatten()
            .unwrap_or_else(|| DEFAULT_PRIORITY_ORDER.to_string());

        let requested_order: Vec<&str> = order_setting.split(',').map(|s| s.trim()).filter(|s| !s.is_empty()).collect();

        let mut ordered = Vec::with_capacity(self.providers.len());
        for name in &requested_order {
            if let Some((_, provider)) = self.providers.iter().find(|(n, _)| n == name) {
                ordered.push(provider);
            }
        }
        for (name, provider) in &self.providers {
            if !requested_order.contains(&name.as_str()) {
                ordered.push(provider);
            }
        }
        ordered
    }
}

#[async_trait]
impl MarketDataProvider for PrioritizedMarketDataProvider {
    async fn fetch_quote(&self, symbol: &str, exchange: &str) -> Result<Quote, MarketDataError> {
        let ordered = self.ordered_providers().await;
        let mut errors = Vec::new();
        for provider in ordered {
            match provider.fetch_quote(symbol, exchange).await {
                Ok(quote) => return Ok(quote),
                Err(e) => errors.push(e.to_string()),
            }
        }
        Err(MarketDataError::RequestFailed(format!("all providers failed for {symbol}: {}", errors.join("; "))))
    }

    async fn fetch_daily_history_1y(&self, symbol: &str, exchange: &str) -> Result<Vec<DailyBar>, MarketDataError> {
        let ordered = self.ordered_providers().await;
        let mut errors = Vec::new();
        for provider in ordered {
            match provider.fetch_daily_history_1y(symbol, exchange).await {
                Ok(bars) => return Ok(bars),
                Err(e) => errors.push(e.to_string()),
            }
        }
        Err(MarketDataError::RequestFailed(format!("all providers failed for {symbol}: {}", errors.join("; "))))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sqlite::SqlitePool;

    struct FakeProvider {
        should_fail: bool,
        call_order: Arc<std::sync::Mutex<Vec<String>>>,
        name: String,
        price: rust_decimal::Decimal,
    }

    #[async_trait]
    impl MarketDataProvider for FakeProvider {
        async fn fetch_quote(&self, _symbol: &str, _exchange: &str) -> Result<Quote, MarketDataError> {
            self.call_order.lock().unwrap().push(self.name.clone());
            if self.should_fail {
                Err(MarketDataError::RequestFailed(format!("{} simulated failure", self.name)))
            } else {
                Ok(Quote { price: self.price, day_high: None, day_low: None, week52_high: None, week52_low: None, volume: None })
            }
        }
        async fn fetch_daily_history_1y(&self, _symbol: &str, _exchange: &str) -> Result<Vec<DailyBar>, MarketDataError> {
            Ok(vec![])
        }
    }

    fn fake(name: &str, should_fail: bool, price: i64, call_order: Arc<std::sync::Mutex<Vec<String>>>) -> Arc<dyn MarketDataProvider> {
        Arc::new(FakeProvider { should_fail, call_order, name: name.to_string(), price: rust_decimal::Decimal::from(price) })
    }

    #[tokio::test]
    async fn tries_providers_in_the_configured_order() {
        let pool = SqlitePool::open_in_memory().unwrap();
        let settings = Arc::new(SqliteAppSettings::new(pool));
        settings.set(MARKET_DATA_PRIORITY_SETTING, "b,a").await.unwrap();

        let call_order = Arc::new(std::sync::Mutex::new(Vec::new()));
        let provider = PrioritizedMarketDataProvider::new(
            vec![
                ("a".to_string(), fake("a", false, 100, call_order.clone())),
                ("b".to_string(), fake("b", true, 200, call_order.clone())),
            ],
            settings,
        );

        let quote = provider.fetch_quote("X", "NSE").await.unwrap();
        assert_eq!(quote.price, rust_decimal::Decimal::from(100));
        assert_eq!(*call_order.lock().unwrap(), vec!["b".to_string(), "a".to_string()]);
    }

    #[tokio::test]
    async fn defaults_to_upstox_yahoo_alpha_vantage_when_unset() {
        let pool = SqlitePool::open_in_memory().unwrap();
        let settings = Arc::new(SqliteAppSettings::new(pool));
        let call_order = Arc::new(std::sync::Mutex::new(Vec::new()));
        let provider = PrioritizedMarketDataProvider::new(
            vec![
                ("yahoo".to_string(), fake("yahoo", true, 100, call_order.clone())),
                ("upstox".to_string(), fake("upstox", true, 200, call_order.clone())),
                ("alpha_vantage".to_string(), fake("alpha_vantage", true, 300, call_order.clone())),
            ],
            settings,
        );

        let _ = provider.fetch_quote("X", "NSE").await;
        assert_eq!(*call_order.lock().unwrap(), vec!["upstox".to_string(), "yahoo".to_string(), "alpha_vantage".to_string()]);
    }

    #[tokio::test]
    async fn unrecognized_names_in_setting_are_ignored_not_erroring() {
        let pool = SqlitePool::open_in_memory().unwrap();
        let settings = Arc::new(SqliteAppSettings::new(pool));
        settings.set(MARKET_DATA_PRIORITY_SETTING, "typo_name,a").await.unwrap();
        let call_order = Arc::new(std::sync::Mutex::new(Vec::new()));
        let provider = PrioritizedMarketDataProvider::new(vec![("a".to_string(), fake("a", false, 100, call_order.clone()))], settings);

        let quote = provider.fetch_quote("X", "NSE").await.unwrap();
        assert_eq!(quote.price, rust_decimal::Decimal::from(100));
    }

    #[tokio::test]
    async fn provider_missing_from_saved_order_still_gets_tried_appended_at_end() {
        let pool = SqlitePool::open_in_memory().unwrap();
        let settings = Arc::new(SqliteAppSettings::new(pool));
        settings.set(MARKET_DATA_PRIORITY_SETTING, "a").await.unwrap(); // "b" not mentioned
        let call_order = Arc::new(std::sync::Mutex::new(Vec::new()));
        let provider = PrioritizedMarketDataProvider::new(
            vec![
                ("a".to_string(), fake("a", true, 100, call_order.clone())),
                ("b".to_string(), fake("b", false, 200, call_order.clone())),
            ],
            settings,
        );

        let quote = provider.fetch_quote("X", "NSE").await.unwrap();
        assert_eq!(quote.price, rust_decimal::Decimal::from(200));
        assert_eq!(*call_order.lock().unwrap(), vec!["a".to_string(), "b".to_string()]);
    }

    #[tokio::test]
    async fn all_providers_failing_returns_a_combined_error() {
        let pool = SqlitePool::open_in_memory().unwrap();
        let settings = Arc::new(SqliteAppSettings::new(pool));
        let call_order = Arc::new(std::sync::Mutex::new(Vec::new()));
        let provider = PrioritizedMarketDataProvider::new(vec![("a".to_string(), fake("a", true, 100, call_order.clone()))], settings);

        let result = provider.fetch_quote("X", "NSE").await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("a simulated failure"));
    }
}

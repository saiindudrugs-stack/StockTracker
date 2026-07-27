//! Tries the primary provider first; on ANY error, falls back to the
//! secondary (if one is configured). This is the whole reason
//! MarketDataProvider's fetch methods take raw (symbol, exchange) instead
//! of a pre-mapped ticker string — each provider maps to its own format
//! internally, so this wrapper never needs to know either provider's
//! ticker conventions, just which one to try first.

use async_trait::async_trait;
use pm_domain::analytics::DailyBar;

use super::{MarketDataError, MarketDataProvider, Quote};

pub struct CompositeMarketDataProvider<P: MarketDataProvider, S: MarketDataProvider> {
    primary: P,
    fallback: Option<S>,
}

impl<P: MarketDataProvider, S: MarketDataProvider> CompositeMarketDataProvider<P, S> {
    pub fn new(primary: P, fallback: Option<S>) -> Self {
        Self { primary, fallback }
    }
}

#[async_trait]
impl<P: MarketDataProvider, S: MarketDataProvider> MarketDataProvider for CompositeMarketDataProvider<P, S> {
    async fn fetch_quote(&self, symbol: &str, exchange: &str) -> Result<Quote, MarketDataError> {
        match self.primary.fetch_quote(symbol, exchange).await {
            Ok(quote) => Ok(quote),
            Err(primary_err) => match &self.fallback {
                Some(fallback) => fallback.fetch_quote(symbol, exchange).await.map_err(|fallback_err| {
                    MarketDataError::RequestFailed(format!(
                        "primary failed ({primary_err}) and fallback also failed ({fallback_err})"
                    ))
                }),
                None => Err(primary_err),
            },
        }
    }

    async fn fetch_daily_history_1y(&self, symbol: &str, exchange: &str) -> Result<Vec<DailyBar>, MarketDataError> {
        match self.primary.fetch_daily_history_1y(symbol, exchange).await {
            Ok(bars) => Ok(bars),
            Err(primary_err) => match &self.fallback {
                Some(fallback) => fallback.fetch_daily_history_1y(symbol, exchange).await.map_err(|fallback_err| {
                    MarketDataError::RequestFailed(format!(
                        "primary failed ({primary_err}) and fallback also failed ({fallback_err})"
                    ))
                }),
                None => Err(primary_err),
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    struct FakeProvider {
        should_fail: bool,
        call_count: Arc<AtomicUsize>,
        price: rust_decimal::Decimal,
    }

    #[async_trait]
    impl MarketDataProvider for FakeProvider {
        async fn fetch_quote(&self, _symbol: &str, _exchange: &str) -> Result<Quote, MarketDataError> {
            self.call_count.fetch_add(1, Ordering::SeqCst);
            if self.should_fail {
                Err(MarketDataError::RequestFailed("simulated failure".to_string()))
            } else {
                Ok(Quote { price: self.price, day_high: None, day_low: None, week52_high: None, week52_low: None, volume: None })
            }
        }
        async fn fetch_daily_history_1y(&self, _symbol: &str, _exchange: &str) -> Result<Vec<DailyBar>, MarketDataError> {
            self.call_count.fetch_add(1, Ordering::SeqCst);
            if self.should_fail {
                Err(MarketDataError::RequestFailed("simulated failure".to_string()))
            } else {
                Ok(vec![])
            }
        }
    }

    #[tokio::test]
    async fn uses_primary_when_it_succeeds_never_touching_fallback() {
        let primary_calls = Arc::new(AtomicUsize::new(0));
        let fallback_calls = Arc::new(AtomicUsize::new(0));
        let composite = CompositeMarketDataProvider::new(
            FakeProvider { should_fail: false, call_count: primary_calls.clone(), price: rust_decimal_macros::dec!(100) },
            Some(FakeProvider { should_fail: false, call_count: fallback_calls.clone(), price: rust_decimal_macros::dec!(200) }),
        );

        let quote = composite.fetch_quote("X", "NSE").await.unwrap();
        assert_eq!(quote.price, rust_decimal_macros::dec!(100));
        assert_eq!(primary_calls.load(Ordering::SeqCst), 1);
        assert_eq!(fallback_calls.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn falls_back_when_primary_fails() {
        let primary_calls = Arc::new(AtomicUsize::new(0));
        let fallback_calls = Arc::new(AtomicUsize::new(0));
        let composite = CompositeMarketDataProvider::new(
            FakeProvider { should_fail: true, call_count: primary_calls.clone(), price: rust_decimal_macros::dec!(100) },
            Some(FakeProvider { should_fail: false, call_count: fallback_calls.clone(), price: rust_decimal_macros::dec!(200) }),
        );

        let quote = composite.fetch_quote("X", "NSE").await.unwrap();
        assert_eq!(quote.price, rust_decimal_macros::dec!(200));
        assert_eq!(primary_calls.load(Ordering::SeqCst), 1);
        assert_eq!(fallback_calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn returns_primary_error_when_no_fallback_configured() {
        let primary_calls = Arc::new(AtomicUsize::new(0));
        let composite: CompositeMarketDataProvider<FakeProvider, FakeProvider> = CompositeMarketDataProvider::new(
            FakeProvider { should_fail: true, call_count: primary_calls.clone(), price: rust_decimal_macros::dec!(100) },
            None,
        );

        let result = composite.fetch_quote("X", "NSE").await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn returns_combined_error_when_both_fail() {
        let primary_calls = Arc::new(AtomicUsize::new(0));
        let fallback_calls = Arc::new(AtomicUsize::new(0));
        let composite = CompositeMarketDataProvider::new(
            FakeProvider { should_fail: true, call_count: primary_calls.clone(), price: rust_decimal_macros::dec!(100) },
            Some(FakeProvider { should_fail: true, call_count: fallback_calls.clone(), price: rust_decimal_macros::dec!(200) }),
        );

        let result = composite.fetch_quote("X", "NSE").await;
        assert!(result.is_err());
        assert_eq!(primary_calls.load(Ordering::SeqCst), 1);
        assert_eq!(fallback_calls.load(Ordering::SeqCst), 1);
    }
}

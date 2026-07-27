//! Alpha Vantage — used as a FALLBACK, not primary. Unlike Yahoo Finance,
//! this is an official, documented API (NASDAQ's own licensed US data
//! provider), but its free tier has a very low request ceiling (as low as
//! single-digit calls per minute on some accounts) — too thin to be the
//! primary source for an app that might refresh a dozen holdings at once.
//! Yahoo stays primary for volume; this exists for when Yahoo's unofficial
//! endpoint has an outage or gets rate-limited.
//!
//! Live-verified for all three markets this app supports, by the user
//! directly testing GLOBAL_QUOTE with their own API key (not from
//! documentation alone): RELIANCE.BSE (India), AAPL (US), TSCO.LON (UK)
//! all returned real data. The NSE colon-prefix format ("NSE:SYMBOL",
//! documented in Alpha Vantage's own cookbook) was NOT tested — only the
//! .BSE suffix was — so NSE-exchange instruments are mapped to Alpha
//! Vantage's .BSE format as a practical fallback (same company, adjacent
//! Indian exchange, nearly identical price) rather than trusting an
//! unverified NSE-specific format.

use async_trait::async_trait;
use chrono::NaiveDate;
use reqwest::Client;
use rust_decimal::Decimal;
use std::collections::HashMap;
use std::str::FromStr;
use std::sync::Arc;

use super::{DailyBar, MarketDataError, MarketDataProvider, Quote};
use crate::sqlite::SqliteAppSettings;

pub const ALPHA_VANTAGE_API_KEY_SETTING: &str = "alpha_vantage_api_key";

pub struct AlphaVantageProvider {
    http: Client,
    settings: Arc<SqliteAppSettings>,
}

impl AlphaVantageProvider {
    /// Reads the key from local settings storage on every call rather than
    /// baking a fixed key in at construction — lets Settings -> save a new
    /// key take effect immediately, no app restart needed.
    pub fn new(settings: Arc<SqliteAppSettings>) -> Self {
        Self { http: Client::new(), settings }
    }

    /// See the module-level doc comment for exactly what's been verified
    /// here vs. what's a reasonable-but-untested extrapolation.
    fn to_alpha_vantage_symbol(symbol: &str, exchange: &str) -> String {
        match exchange.to_uppercase().as_str() {
            "BSE" => format!("{symbol}.BSE"),
            // Not independently verified — NSE listings are mapped to the
            // *confirmed-working* .BSE format for the same company rather
            // than the documented-but-untested "NSE:SYMBOL" form.
            "NSE" => format!("{symbol}.BSE"),
            "LSE" => format!("{symbol}.LON"),
            _ => symbol.to_string(),
        }
    }
}

#[async_trait]
impl MarketDataProvider for AlphaVantageProvider {
    async fn fetch_quote(&self, symbol: &str, exchange: &str) -> Result<Quote, MarketDataError> {
        let api_key = self
            .settings
            .get(ALPHA_VANTAGE_API_KEY_SETTING)
            .await
            .map_err(|e| MarketDataError::RequestFailed(e.to_string()))?
            .filter(|k| !k.trim().is_empty())
            .ok_or_else(|| MarketDataError::RequestFailed("no Alpha Vantage API key configured in Settings".to_string()))?;

        let av_symbol = Self::to_alpha_vantage_symbol(symbol, exchange);
        let url = format!(
            "https://www.alphavantage.co/query?function=GLOBAL_QUOTE&symbol={av_symbol}&apikey={api_key}"
        );
        let response = self
            .http
            .get(&url)
            .send()
            .await
            .map_err(|e| MarketDataError::RequestFailed(e.to_string()))?;

        let body: GlobalQuoteResponse = response.json().await.map_err(|e| {
            MarketDataError::UnexpectedResponse(format!("couldn't parse Alpha Vantage response for {av_symbol}: {e}"))
        })?;

        let q = body
            .global_quote
            .filter(|q| !q.price.is_empty())
            .ok_or_else(|| MarketDataError::NoData(format!("{av_symbol}: empty response — bad symbol, rate limit, or invalid key")))?;

        let price = Decimal::from_str(&q.price)
            .map_err(|_| MarketDataError::UnexpectedResponse(format!("bad price for {av_symbol}")))?;

        Ok(Quote {
            price,
            day_high: Decimal::from_str(&q.high).ok(),
            day_low: Decimal::from_str(&q.low).ok(),
            // Not available from GLOBAL_QUOTE (would need the separate
            // OVERVIEW endpoint) — acceptable gap for a fallback path.
            week52_high: None,
            week52_low: None,
            volume: q.volume.parse().ok(),
        })
    }

    async fn fetch_daily_history_1y(&self, symbol: &str, exchange: &str) -> Result<Vec<DailyBar>, MarketDataError> {
        let api_key = self
            .settings
            .get(ALPHA_VANTAGE_API_KEY_SETTING)
            .await
            .map_err(|e| MarketDataError::RequestFailed(e.to_string()))?
            .filter(|k| !k.trim().is_empty())
            .ok_or_else(|| MarketDataError::RequestFailed("no Alpha Vantage API key configured in Settings".to_string()))?;

        let av_symbol = Self::to_alpha_vantage_symbol(symbol, exchange);
        let url = format!(
            "https://www.alphavantage.co/query?function=TIME_SERIES_DAILY&symbol={av_symbol}&outputsize=full&apikey={api_key}"
        );
        let response = self
            .http
            .get(&url)
            .send()
            .await
            .map_err(|e| MarketDataError::RequestFailed(e.to_string()))?;

        let body: DailySeriesResponse = response.json().await.map_err(|e| {
            MarketDataError::UnexpectedResponse(format!("couldn't parse Alpha Vantage history for {av_symbol}: {e}"))
        })?;

        let series = body
            .time_series
            .ok_or_else(|| MarketDataError::NoData(format!("{av_symbol}: no time series — bad symbol, rate limit, or invalid key")))?;

        let cutoff = chrono::Utc::now().date_naive() - chrono::Duration::days(366);
        let mut bars: Vec<DailyBar> = series
            .into_iter()
            .filter_map(|(date_str, bar)| {
                let date = NaiveDate::parse_from_str(&date_str, "%Y-%m-%d").ok()?;
                if date < cutoff {
                    return None;
                }
                Some(DailyBar {
                    date,
                    open: bar.open.parse().ok()?,
                    high: bar.high.parse().ok()?,
                    low: bar.low.parse().ok()?,
                    close: bar.close.parse().ok()?,
                    volume: bar.volume.parse().ok()?,
                })
            })
            .collect();
        bars.sort_by_key(|b| b.date);
        Ok(bars)
    }
}

#[derive(serde::Deserialize)]
struct GlobalQuoteResponse {
    #[serde(rename = "Global Quote")]
    global_quote: Option<GlobalQuote>,
}

#[derive(serde::Deserialize)]
struct GlobalQuote {
    #[serde(rename = "03. high")]
    high: String,
    #[serde(rename = "04. low")]
    low: String,
    #[serde(rename = "05. price")]
    price: String,
    #[serde(rename = "06. volume")]
    volume: String,
}

#[derive(serde::Deserialize)]
struct DailySeriesResponse {
    #[serde(rename = "Time Series (Daily)")]
    time_series: Option<HashMap<String, DailyBarRaw>>,
}

#[derive(serde::Deserialize)]
struct DailyBarRaw {
    #[serde(rename = "1. open")]
    open: String,
    #[serde(rename = "2. high")]
    high: String,
    #[serde(rename = "3. low")]
    low: String,
    #[serde(rename = "4. close")]
    close: String,
    #[serde(rename = "5. volume")]
    volume: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bse_and_nse_both_map_to_the_verified_bse_suffix() {
        assert_eq!(AlphaVantageProvider::to_alpha_vantage_symbol("RELIANCE", "BSE"), "RELIANCE.BSE");
        assert_eq!(AlphaVantageProvider::to_alpha_vantage_symbol("RELIANCE", "NSE"), "RELIANCE.BSE");
    }

    #[test]
    fn lse_maps_to_dot_lon_suffix() {
        assert_eq!(AlphaVantageProvider::to_alpha_vantage_symbol("TSCO", "LSE"), "TSCO.LON");
    }

    #[test]
    fn us_exchanges_pass_through_unsuffixed() {
        assert_eq!(AlphaVantageProvider::to_alpha_vantage_symbol("AAPL", "NASDAQ"), "AAPL");
        assert_eq!(AlphaVantageProvider::to_alpha_vantage_symbol("AAPL", "NYSE"), "AAPL");
    }

    #[test]
    fn parses_a_real_shaped_global_quote_response() {
        let sample = r#"{"Global Quote": {"01. symbol": "AAPL", "02. open": "210.00", "03. high": "212.50", "04. low": "209.10", "05. price": "211.34", "06. volume": "45000000", "07. latest trading day": "2026-07-24", "08. previous close": "209.80", "09. change": "1.54", "10. change percent": "0.7340%"}}"#;
        let parsed: GlobalQuoteResponse = serde_json::from_str(sample).unwrap();
        let q = parsed.global_quote.unwrap();
        assert_eq!(q.price, "211.34");
        assert_eq!(q.high, "212.50");
    }

    #[test]
    fn empty_object_response_is_treated_as_no_data_not_a_panic() {
        let sample = r#"{}"#;
        let parsed: GlobalQuoteResponse = serde_json::from_str(sample).unwrap();
        assert!(parsed.global_quote.is_none());
    }

    #[test]
    fn parses_a_real_shaped_daily_series_response() {
        let sample = r#"{"Time Series (Daily)": {"2026-07-24": {"1. open": "210.00", "2. high": "212.50", "3. low": "209.10", "4. close": "211.34", "5. volume": "45000000"}}}"#;
        let parsed: DailySeriesResponse = serde_json::from_str(sample).unwrap();
        let series = parsed.time_series.unwrap();
        assert_eq!(series.len(), 1);
        assert_eq!(series["2026-07-24"].close, "211.34");
    }
}

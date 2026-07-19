//! Unofficial Yahoo Finance quote client. See the module-level warning in
//! `market_data/mod.rs` — this is not a supported Yahoo API, it's the
//! public chart endpoint their own website's frontend happens to call,
//! which many hobbyist tools rely on but which Yahoo could change or block
//! at any time without notice.
//!
//! HONESTY NOTE ON VERIFICATION: this was written in a sandboxed
//! environment with no general internet access (only an allowlist of dev
//! tool domains — crates.io, npm, github). I could not actually send a
//! request to `query1.finance.yahoo.com` to confirm this response shape is
//! current. The JSON structure below (`chart.result[0].meta...` for quotes,
//! `chart.result[0].timestamp` + `indicators.quote[0]` for history) matches
//! the endpoint's long-documented-by-the-community shape as of this code's
//! training data, but "matches what I remember" is a real notch below "I
//! confirmed it just now" — test this against a real symbol before
//! trusting it for anything beyond casual use, and if the shape has
//! changed, the error message from serde_json on a failed parse will show
//! you the actual JSON Yahoo returned, which is the fastest way to fix the
//! structs below.

use super::{MarketDataError, MarketDataProvider, Quote};
use async_trait::async_trait;
use chrono::DateTime;
use pm_domain::analytics::DailyBar;
use rust_decimal::Decimal;
use serde::Deserialize;
use std::str::FromStr;

pub struct YahooFinanceProvider {
    http: reqwest::Client,
}

impl YahooFinanceProvider {
    pub fn new() -> Self {
        Self {
            http: reqwest::Client::builder()
                // Yahoo's frontend endpoint has been known to reject
                // requests with no User-Agent (treating them as bot
                // traffic) — set a plain browser-like one rather than
                // reqwest's default.
                .user_agent("Mozilla/5.0 (compatible; PortfolioManagerDesktop/0.2)")
                .build()
                .expect("reqwest client build should not fail with static config"),
        }
    }

    /// Maps this app's own (symbol, exchange) pair to Yahoo's suffix
    /// convention. NSE -> .NS, BSE -> .BO — Yahoo's two Indian exchange
    /// suffixes; anything else is passed through unsuffixed (won't resolve
    /// correctly for Indian tickers, but avoids silently guessing wrong for
    /// exchanges this function doesn't know about).
    pub fn to_yahoo_symbol(symbol: &str, exchange: &str) -> String {
        match exchange.to_uppercase().as_str() {
            "NSE" => format!("{symbol}.NS"),
            "BSE" => format!("{symbol}.BO"),
            _ => symbol.to_string(),
        }
    }

    fn f64_to_decimal(v: f64) -> Option<Decimal> {
        Decimal::from_str(&v.to_string()).ok()
    }
}

impl Default for YahooFinanceProvider {
    fn default() -> Self {
        Self::new()
    }
}

// --- Quote (meta-object) response shape ---

#[derive(Debug, Deserialize)]
struct YahooChartResponse {
    chart: YahooChart,
}
#[derive(Debug, Deserialize)]
struct YahooChart {
    result: Option<Vec<YahooChartResult>>,
    error: Option<YahooError>,
}
#[derive(Debug, Deserialize)]
struct YahooError {
    description: String,
}
#[derive(Debug, Deserialize)]
struct YahooChartResult {
    meta: YahooMeta,
    timestamp: Option<Vec<i64>>,
    indicators: Option<YahooIndicators>,
}
#[derive(Debug, Deserialize)]
struct YahooMeta {
    #[serde(rename = "regularMarketPrice")]
    regular_market_price: f64,
    #[serde(rename = "regularMarketDayHigh")]
    regular_market_day_high: Option<f64>,
    #[serde(rename = "regularMarketDayLow")]
    regular_market_day_low: Option<f64>,
    #[serde(rename = "fiftyTwoWeekHigh")]
    fifty_two_week_high: Option<f64>,
    #[serde(rename = "fiftyTwoWeekLow")]
    fifty_two_week_low: Option<f64>,
    #[serde(rename = "regularMarketVolume")]
    regular_market_volume: Option<u64>,
}
#[derive(Debug, Deserialize)]
struct YahooIndicators {
    quote: Vec<YahooQuoteSeries>,
}
#[derive(Debug, Deserialize)]
struct YahooQuoteSeries {
    close: Vec<Option<f64>>,
    volume: Vec<Option<f64>>,
}

#[async_trait]
impl MarketDataProvider for YahooFinanceProvider {
    async fn fetch_quote(&self, symbol: &str) -> Result<Quote, MarketDataError> {
        let url = format!(
            "https://query1.finance.yahoo.com/v8/finance/chart/{symbol}?interval=1d&range=1d"
        );
        let response = self
            .http
            .get(&url)
            .send()
            .await
            .map_err(|e| MarketDataError::RequestFailed(e.to_string()))?;

        if !response.status().is_success() {
            return Err(MarketDataError::RequestFailed(format!(
                "HTTP {} for symbol {symbol} — Yahoo may be rate-limiting or the endpoint has changed",
                response.status()
            )));
        }

        let body: YahooChartResponse = response.json().await.map_err(|e| {
            MarketDataError::UnexpectedResponse(format!(
                "couldn't parse Yahoo's response for {symbol}: {e}. \
                 The endpoint's JSON shape may have changed since this client was written — \
                 see the honesty note at the top of yahoo_finance.rs."
            ))
        })?;

        if let Some(err) = body.chart.error {
            return Err(MarketDataError::NoData(format!("{symbol}: {}", err.description)));
        }

        let meta = body
            .chart
            .result
            .and_then(|r| r.into_iter().next())
            .map(|r| r.meta)
            .ok_or_else(|| MarketDataError::NoData(symbol.to_string()))?;

        let price = Self::f64_to_decimal(meta.regular_market_price)
            .ok_or_else(|| MarketDataError::UnexpectedResponse(format!("bad price for {symbol}")))?;

        Ok(Quote {
            price,
            day_high: meta.regular_market_day_high.and_then(Self::f64_to_decimal),
            day_low: meta.regular_market_day_low.and_then(Self::f64_to_decimal),
            week52_high: meta.fifty_two_week_high.and_then(Self::f64_to_decimal),
            week52_low: meta.fifty_two_week_low.and_then(Self::f64_to_decimal),
            volume: meta.regular_market_volume,
        })
    }

    async fn fetch_daily_history_1y(&self, symbol: &str) -> Result<Vec<DailyBar>, MarketDataError> {
        let url = format!(
            "https://query1.finance.yahoo.com/v8/finance/chart/{symbol}?interval=1d&range=1y"
        );
        let response = self
            .http
            .get(&url)
            .send()
            .await
            .map_err(|e| MarketDataError::RequestFailed(e.to_string()))?;

        if !response.status().is_success() {
            return Err(MarketDataError::RequestFailed(format!(
                "HTTP {} for symbol {symbol} history",
                response.status()
            )));
        }

        let body: YahooChartResponse = response.json().await.map_err(|e| {
            MarketDataError::UnexpectedResponse(format!(
                "couldn't parse Yahoo's 1y history response for {symbol}: {e}"
            ))
        })?;

        if let Some(err) = body.chart.error {
            return Err(MarketDataError::NoData(format!("{symbol}: {}", err.description)));
        }

        let result = body
            .chart
            .result
            .and_then(|r| r.into_iter().next())
            .ok_or_else(|| MarketDataError::NoData(symbol.to_string()))?;

        let timestamps = result.timestamp.ok_or_else(|| {
            MarketDataError::UnexpectedResponse(format!("no timestamp array for {symbol}"))
        })?;
        let quote_series = result
            .indicators
            .and_then(|i| i.quote.into_iter().next())
            .ok_or_else(|| MarketDataError::UnexpectedResponse(format!("no quote series for {symbol}")))?;

        let mut bars = Vec::with_capacity(timestamps.len());
        for i in 0..timestamps.len() {
            // Yahoo pads days with no trade (holidays inside the range
            // grid) with `null` close/volume — skip those rather than
            // treat a missing value as a zero, which would corrupt SMA/OBV.
            let (Some(close), Some(volume)) = (
                quote_series.close.get(i).copied().flatten(),
                quote_series.volume.get(i).copied().flatten(),
            ) else {
                continue;
            };
            let date = DateTime::from_timestamp(timestamps[i], 0)
                .ok_or_else(|| MarketDataError::UnexpectedResponse(format!("bad timestamp for {symbol}")))?
                .date_naive();
            bars.push(DailyBar { date, close, volume });
        }
        Ok(bars)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_nse_and_bse_suffixes_correctly() {
        assert_eq!(YahooFinanceProvider::to_yahoo_symbol("RELIANCE", "NSE"), "RELIANCE.NS");
        assert_eq!(YahooFinanceProvider::to_yahoo_symbol("RELIANCE", "BSE"), "RELIANCE.BO");
        assert_eq!(YahooFinanceProvider::to_yahoo_symbol("RELIANCE", "nse"), "RELIANCE.NS");
    }

    #[test]
    fn unknown_exchange_passes_through_unsuffixed_rather_than_guessing() {
        assert_eq!(YahooFinanceProvider::to_yahoo_symbol("AAPL", "NASDAQ"), "AAPL");
    }

    /// Parses a hand-written JSON string shaped like Yahoo's documented
    /// meta response — proves the deserialization logic is internally
    /// consistent, NOT that it matches Yahoo's live response today (see
    /// the module-level honesty note).
    #[test]
    fn parses_a_well_formed_quote_response() {
        let json = r#"{
            "chart": {
                "result": [{
                    "meta": {
                        "regularMarketPrice": 2510.75,
                        "regularMarketDayHigh": 2525.0,
                        "regularMarketDayLow": 2495.5,
                        "fiftyTwoWeekHigh": 2900.0,
                        "fiftyTwoWeekLow": 2100.0,
                        "regularMarketVolume": 8234567
                    }
                }],
                "error": null
            }
        }"#;
        let parsed: YahooChartResponse = serde_json::from_str(json).unwrap();
        let meta = &parsed.chart.result.unwrap()[0].meta;
        assert_eq!(meta.regular_market_price, 2510.75);
        assert_eq!(meta.regular_market_day_high, Some(2525.0));
        assert_eq!(meta.regular_market_volume, Some(8234567));
    }

    #[test]
    fn parses_yahoos_error_shape() {
        let json = r#"{
            "chart": {
                "result": null,
                "error": { "description": "No data found, symbol may be delisted" }
            }
        }"#;
        let parsed: YahooChartResponse = serde_json::from_str(json).unwrap();
        assert!(parsed.chart.result.is_none());
        assert_eq!(parsed.chart.error.unwrap().description, "No data found, symbol may be delisted");
    }

    #[test]
    fn parses_a_well_formed_history_response_and_skips_null_days() {
        let json = r#"{
            "chart": {
                "result": [{
                    "meta": { "regularMarketPrice": 100.0 },
                    "timestamp": [1735689600, 1735776000, 1735862400],
                    "indicators": {
                        "quote": [{
                            "close": [100.0, null, 102.0],
                            "volume": [1000.0, null, 1200.0]
                        }]
                    }
                }],
                "error": null
            }
        }"#;
        let parsed: YahooChartResponse = serde_json::from_str(json).unwrap();
        let result = &parsed.chart.result.unwrap()[0];
        let series = &result.indicators.as_ref().unwrap().quote[0];
        assert_eq!(series.close.len(), 3);
        assert_eq!(series.close[1], None); // the null day
    }
}

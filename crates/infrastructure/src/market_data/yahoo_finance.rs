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
//! current. The JSON structure below (`chart.result[0].meta.regularMarketPrice`)
//! matches the endpoint's long-documented-by-the-community shape as of
//! this code's training data, but "matches what I remember" is a real
//! notch below "I confirmed it just now" — test this against a real
//! symbol before trusting it for anything beyond casual use, and if the
//! shape has changed, the error message from `serde_json` on a failed
//! parse will show you the actual JSON Yahoo returned, which is the
//! fastest way to fix the struct below.

use super::{MarketDataError, MarketDataProvider};
use async_trait::async_trait;
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
}

impl Default for YahooFinanceProvider {
    fn default() -> Self {
        Self::new()
    }
}

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
}
#[derive(Debug, Deserialize)]
struct YahooMeta {
    #[serde(rename = "regularMarketPrice")]
    regular_market_price: f64,
}

#[async_trait]
impl MarketDataProvider for YahooFinanceProvider {
    async fn fetch_latest_price(&self, symbol: &str) -> Result<Decimal, MarketDataError> {
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

        let price = body
            .chart
            .result
            .and_then(|r| r.into_iter().next())
            .map(|r| r.meta.regular_market_price)
            .ok_or_else(|| MarketDataError::NoData(symbol.to_string()))?;

        Decimal::from_str(&price.to_string())
            .map_err(|e| MarketDataError::UnexpectedResponse(format!("price {price} not a valid decimal: {e}")))
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

    /// This test parses a hand-written JSON string shaped like Yahoo's
    /// documented response — it proves the deserialization logic is
    /// internally consistent, NOT that it matches Yahoo's live response
    /// today (see the module-level honesty note; that requires a real
    /// network call this sandbox can't make).
    #[test]
    fn parses_a_well_formed_chart_response() {
        let json = r#"{
            "chart": {
                "result": [{
                    "meta": { "regularMarketPrice": 2510.75 }
                }],
                "error": null
            }
        }"#;
        let parsed: YahooChartResponse = serde_json::from_str(json).unwrap();
        assert_eq!(parsed.chart.result.unwrap()[0].meta.regular_market_price, 2510.75);
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
}

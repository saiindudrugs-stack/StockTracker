//! Alpha Vantage — deliberately NOT a MarketDataProvider (no fetch_quote,
//! not part of the priority-ordered quote chain at all). Its free tier's
//! real limit is 25 requests/day TOTAL, which makes it fundamentally
//! unsuitable as a live-price source or as a general fallback — it was
//! removed from that role entirely. What it's kept for: `OVERVIEW`
//! returns two fields (market cap, dividend yield) that neither Upstox's
//! Fundamentals API nor Yahoo reliably provide, and this app's own News
//! screen wants at least market cap shown. Given the 25/day budget, the
//! caller is expected to cache the result for a real stretch of time
//! (this app's own caching layer enforces once-an-hour, not this module —
//! see SqliteAlphaVantageOverviewCache) rather than calling this on every
//! fundamentals view.

use reqwest::Client;
use serde::Deserialize;
use std::sync::Arc;

use super::MarketDataError;
use crate::sqlite::SqliteAppSettings;

pub const ALPHA_VANTAGE_API_KEY_SETTING: &str = "alpha_vantage_api_key";

pub struct AlphaVantageProvider {
    http: Client,
    settings: Arc<SqliteAppSettings>,
}

#[derive(Debug, Clone, Default)]
pub struct AlphaVantageOverview {
    pub market_cap: Option<f64>,
    pub dividend_yield: Option<f64>,
}

impl AlphaVantageProvider {
    pub fn new(settings: Arc<SqliteAppSettings>) -> Self {
        Self { http: Client::new(), settings }
    }

    /// Same symbol-mapping convention Alpha Vantage's own docs describe:
    /// bare symbol for US, `EXCHANGE:SYMBOL` for everything else — kept
    /// from the original implementation since this part was already
    /// live-verified by the user (RELIANCE.BSE, AAPL, TSCO.LON all
    /// confirmed working earlier in this project).
    fn to_alpha_vantage_symbol(symbol: &str, exchange: &str) -> String {
        match exchange.to_uppercase().as_str() {
            "NSE" | "BSE" => format!("{symbol}.BSE"),
            "LSE" => format!("{symbol}.LON"),
            _ => symbol.to_string(),
        }
    }

    /// Only market_cap and dividend_yield are extracted — every other
    /// OVERVIEW field (PE, sector, description, etc.) is intentionally
    /// left unused now that Upstox's Fundamentals API covers those, to
    /// keep this module's footprint matching its narrow, current role.
    pub async fn fetch_overview(&self, symbol: &str, exchange: &str) -> Result<AlphaVantageOverview, MarketDataError> {
        let api_key = self
            .settings
            .get(ALPHA_VANTAGE_API_KEY_SETTING)
            .await
            .map_err(|e| MarketDataError::RequestFailed(e.to_string()))?
            .filter(|k| !k.trim().is_empty())
            .ok_or_else(|| MarketDataError::RequestFailed("no Alpha Vantage API key configured in Settings".to_string()))?;

        let av_symbol = Self::to_alpha_vantage_symbol(symbol, exchange);
        let url = format!("https://www.alphavantage.co/query?function=OVERVIEW&symbol={av_symbol}&apikey={api_key}");
        let response = self.http.get(&url).send().await.map_err(|e| MarketDataError::RequestFailed(e.to_string()))?;
        let body: RawOverview = response
            .json()
            .await
            .map_err(|e| MarketDataError::UnexpectedResponse(format!("couldn't parse Alpha Vantage overview for {av_symbol}: {e}")))?;

        // An unrecognized symbol, or a rate-limited request, comes back
        // as `{}` on this endpoint rather than an HTTP error — Symbol
        // being None is how that's detected, same as the original
        // implementation this replaces.
        if body.symbol.is_none() {
            return Err(MarketDataError::NoData(format!("{av_symbol}: empty overview — bad symbol, rate limit, or invalid key")));
        }

        Ok(AlphaVantageOverview {
            market_cap: body.market_capitalization.and_then(|s| s.parse::<f64>().ok()),
            dividend_yield: body.dividend_yield.and_then(|s| s.parse::<f64>().ok()),
        })
    }
}

#[derive(Deserialize)]
struct RawOverview {
    #[serde(rename = "Symbol")]
    symbol: Option<String>,
    #[serde(rename = "MarketCapitalization")]
    market_capitalization: Option<String>,
    #[serde(rename = "DividendYield")]
    dividend_yield: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_a_real_shaped_overview_response() {
        let sample = r#"{"Symbol": "IBM", "MarketCapitalization": "150000000000", "DividendYield": "0.045"}"#;
        let parsed: RawOverview = serde_json::from_str(sample).unwrap();
        assert_eq!(parsed.symbol.as_deref(), Some("IBM"));
        assert_eq!(parsed.market_capitalization.as_deref(), Some("150000000000"));
        assert_eq!(parsed.dividend_yield.as_deref(), Some("0.045"));
    }

    #[test]
    fn empty_overview_object_has_no_symbol_signaling_bad_lookup_or_rate_limit() {
        let sample = r#"{}"#;
        let parsed: RawOverview = serde_json::from_str(sample).unwrap();
        assert!(parsed.symbol.is_none());
    }

    #[test]
    fn maps_nse_and_bse_to_the_bse_suffix() {
        assert_eq!(AlphaVantageProvider::to_alpha_vantage_symbol("RELIANCE", "NSE"), "RELIANCE.BSE");
        assert_eq!(AlphaVantageProvider::to_alpha_vantage_symbol("RELIANCE", "BSE"), "RELIANCE.BSE");
    }

    #[test]
    fn maps_lse_to_the_lon_suffix() {
        assert_eq!(AlphaVantageProvider::to_alpha_vantage_symbol("TSCO", "LSE"), "TSCO.LON");
    }

    #[test]
    fn leaves_us_symbols_bare() {
        assert_eq!(AlphaVantageProvider::to_alpha_vantage_symbol("AAPL", "NASDAQ"), "AAPL");
    }
}

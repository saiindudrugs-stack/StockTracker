//! Upstox's Company Fundamentals API — keyed by ISIN, not trading_symbol
//! or instrument_key, and using the same Analytics Token as market data
//! (no separate auth). Built to replace Yahoo's fundamentals fetch, which
//! has been unreliable in practice ("missing field `quoteSummary`" —
//! likely a crumb/cookie requirement Yahoo added to that specific
//! endpoint that this app's plain GET doesn't satisfy).
//!
//! HONESTY NOTE: same as upstox.rs — built against Upstox's own published
//! docs (endpoint paths and the shape of each response were independently
//! confirmed during research), not live-verified from this sandbox.

use reqwest::Client;
use rust_decimal::Decimal;
use serde::Deserialize;
use std::str::FromStr;

use super::MarketDataError;

pub struct UpstoxFundamentalsClient {
    http: Client,
}

#[derive(Debug, Clone, Default)]
pub struct UpstoxFundamentals {
    pub sector: Option<String>,
    pub industry: Option<String>,
    pub description: Option<String>,
    pub pe_ratio: Option<f64>,
    pub pb_ratio: Option<f64>,
    pub roe: Option<f64>,
    pub roce: Option<f64>,
    /// Oldest first, so the News screen's revenue table shows the same
    /// left-to-right chronological order it already uses for Yahoo's data.
    pub income_statement: Vec<IncomeStatementPeriod>,
}

#[derive(Debug, Clone)]
pub struct IncomeStatementPeriod {
    pub period_end: String,
    pub revenue: Decimal,
    pub net_profit: Option<Decimal>,
}

impl UpstoxFundamentalsClient {
    pub fn new() -> Self {
        Self { http: Client::new() }
    }

    pub async fn fetch_fundamentals(&self, isin: &str, token: &str) -> Result<UpstoxFundamentals, MarketDataError> {
        let profile = self.fetch_company_profile(isin, token).await.ok();
        let ratios = self.fetch_key_ratios(isin, token).await.ok();
        let income_statement = self.fetch_income_statement(isin, token).await.unwrap_or_default();

        // Each of the three calls above is allowed to fail independently
        // (.ok() / unwrap_or_default rather than `?`) — a company with,
        // say, no key-ratios coverage yet shouldn't blank out the profile
        // and income statement that DID come back. Only if genuinely
        // nothing at all came back is this an error.
        if profile.is_none() && ratios.is_none() && income_statement.is_empty() {
            return Err(MarketDataError::NoData(format!("{isin}: no fundamentals data from any Upstox endpoint")));
        }

        Ok(UpstoxFundamentals {
            sector: profile.as_ref().and_then(|p| p.sector.clone()),
            industry: profile.as_ref().and_then(|p| p.industry.clone()),
            description: profile.and_then(|p| p.business_description),
            pe_ratio: ratios.as_ref().and_then(|r| r.pe.as_ref()).map(|v| v.value),
            pb_ratio: ratios.as_ref().and_then(|r| r.pb.as_ref()).map(|v| v.value),
            roe: ratios.as_ref().and_then(|r| r.roe.as_ref()).map(|v| v.value),
            roce: ratios.and_then(|r| r.roce).map(|v| v.value),
            income_statement,
        })
    }

    async fn fetch_company_profile(&self, isin: &str, token: &str) -> Result<CompanyProfile, MarketDataError> {
        let url = format!("https://api.upstox.com/v2/fundamentals/{isin}/company-profile");
        let response = self
            .http
            .get(&url)
            .header("Authorization", format!("Bearer {token}"))
            .header("Accept", "application/json")
            .send()
            .await
            .map_err(|e| MarketDataError::RequestFailed(e.to_string()))?;
        let body: CompanyProfileResponse = response
            .json()
            .await
            .map_err(|e| MarketDataError::UnexpectedResponse(format!("couldn't parse Upstox company profile for {isin}: {e}")))?;
        body.data.ok_or_else(|| MarketDataError::NoData(format!("{isin}: no company profile")))
    }

    async fn fetch_key_ratios(&self, isin: &str, token: &str) -> Result<KeyRatios, MarketDataError> {
        let url = format!("https://api.upstox.com/v2/fundamentals/{isin}/key-ratios");
        let response = self
            .http
            .get(&url)
            .header("Authorization", format!("Bearer {token}"))
            .header("Accept", "application/json")
            .send()
            .await
            .map_err(|e| MarketDataError::RequestFailed(e.to_string()))?;
        let body: KeyRatiosResponse = response
            .json()
            .await
            .map_err(|e| MarketDataError::UnexpectedResponse(format!("couldn't parse Upstox key ratios for {isin}: {e}")))?;
        body.data.ok_or_else(|| MarketDataError::NoData(format!("{isin}: no key ratios")))
    }

    /// Yearly, not quarterly — the News screen wants year-on-year
    /// comparison, and Upstox's own docs describe this endpoint as
    /// supporting both; yearly is the right default for that use.
    async fn fetch_income_statement(&self, isin: &str, token: &str) -> Result<Vec<IncomeStatementPeriod>, MarketDataError> {
        let url = format!("https://api.upstox.com/v2/fundamentals/{isin}/income-statement?statement_type=consolidated&period=yearly");
        let response = self
            .http
            .get(&url)
            .header("Authorization", format!("Bearer {token}"))
            .header("Accept", "application/json")
            .send()
            .await
            .map_err(|e| MarketDataError::RequestFailed(e.to_string()))?;
        let body: IncomeStatementResponse = response
            .json()
            .await
            .map_err(|e| MarketDataError::UnexpectedResponse(format!("couldn't parse Upstox income statement for {isin}: {e}")))?;

        let mut periods: Vec<IncomeStatementPeriod> = body
            .data
            .unwrap_or_default()
            .into_iter()
            .filter_map(|row| {
                Some(IncomeStatementPeriod {
                    period_end: row.period_end?,
                    revenue: Decimal::from_str(&row.revenue?.to_string()).ok()?,
                    net_profit: row.net_profit.and_then(|v| Decimal::from_str(&v.to_string()).ok()),
                })
            })
            .collect();
        periods.sort_by(|a, b| a.period_end.cmp(&b.period_end));
        Ok(periods)
    }

    /// Real, verified endpoint format: GET /v2/news?category=instrument_keys
    /// &instrument_keys={key}. Response is keyed by instrument_key per
    /// Upstox's own docs — field names (headline/summary/link/
    /// published_at) are as described in their announcement, not seen in
    /// a raw sample response, so this is the one part of this module
    /// slightly less certain than the fundamentals endpoints above.
    pub async fn fetch_news(&self, instrument_key: &str, token: &str, limit: usize) -> Result<Vec<UpstoxNewsItem>, MarketDataError> {
        let url = format!("https://api.upstox.com/v2/news?category=instrument_keys&instrument_keys={instrument_key}");
        let response = self
            .http
            .get(&url)
            .header("Authorization", format!("Bearer {token}"))
            .header("Accept", "application/json")
            .send()
            .await
            .map_err(|e| MarketDataError::RequestFailed(e.to_string()))?;
        let body: NewsResponse = response
            .json()
            .await
            .map_err(|e| MarketDataError::UnexpectedResponse(format!("couldn't parse Upstox news for {instrument_key}: {e}")))?;

        let mut items: Vec<UpstoxNewsItem> = body
            .data
            .unwrap_or_default()
            .into_values()
            .flatten()
            .map(|raw| UpstoxNewsItem {
                headline: raw.headline,
                summary: raw.summary,
                link: raw.link,
                published_at_ms: raw.published_at,
            })
            .collect();
        items.sort_by(|a, b| b.published_at_ms.cmp(&a.published_at_ms));
        items.truncate(limit);
        Ok(items)
    }
}

#[derive(Debug, Clone)]
pub struct UpstoxNewsItem {
    pub headline: String,
    pub summary: Option<String>,
    pub link: String,
    pub published_at_ms: i64,
}

impl Default for UpstoxFundamentalsClient {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Deserialize)]
struct CompanyProfileResponse {
    data: Option<CompanyProfile>,
}
#[derive(Deserialize)]
struct CompanyProfile {
    sector: Option<String>,
    industry: Option<String>,
    #[serde(rename = "business_description")]
    business_description: Option<String>,
}

#[derive(Deserialize)]
struct KeyRatiosResponse {
    data: Option<KeyRatios>,
}
#[derive(Deserialize)]
struct KeyRatios {
    pe: Option<RatioValue>,
    pb: Option<RatioValue>,
    roe: Option<RatioValue>,
    roce: Option<RatioValue>,
}
#[derive(Deserialize)]
struct RatioValue {
    value: f64,
}

#[derive(Deserialize)]
struct IncomeStatementResponse {
    data: Option<Vec<IncomeStatementRow>>,
}
#[derive(Deserialize)]
struct IncomeStatementRow {
    period_end: Option<String>,
    revenue: Option<f64>,
    net_profit: Option<f64>,
}

#[derive(Deserialize)]
struct NewsResponse {
    data: Option<std::collections::HashMap<String, Vec<RawNewsItem>>>,
}
#[derive(Deserialize)]
struct RawNewsItem {
    headline: String,
    summary: Option<String>,
    link: String,
    published_at: i64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_a_real_shaped_company_profile_response() {
        let sample = r#"{"data": {"sector": "Energy", "industry": "Oil & Gas Refining", "business_description": "A diversified conglomerate."}}"#;
        let parsed: CompanyProfileResponse = serde_json::from_str(sample).unwrap();
        assert_eq!(parsed.data.unwrap().sector, Some("Energy".to_string()));
    }

    #[test]
    fn parses_a_real_shaped_key_ratios_response() {
        let sample = r#"{"data": {"pe": {"value": 24.1, "sector_value": 22.5}, "pb": {"value": 3.2, "sector_value": 3.0}, "roe": {"value": 12.5, "sector_value": 11.0}, "roce": {"value": 15.0, "sector_value": 14.0}}}"#;
        let parsed: KeyRatiosResponse = serde_json::from_str(sample).unwrap();
        let ratios = parsed.data.unwrap();
        assert_eq!(ratios.pe.unwrap().value, 24.1);
        assert_eq!(ratios.roce.unwrap().value, 15.0);
    }

    #[test]
    fn parses_a_real_shaped_income_statement_response_and_sorts_oldest_first() {
        let sample = r#"{"data": [{"period_end": "2026-03-31", "revenue": 950000000000.0, "net_profit": 68000000000.0}, {"period_end": "2025-03-31", "revenue": 900000000000.0, "net_profit": 65000000000.0}]}"#;
        let parsed: IncomeStatementResponse = serde_json::from_str(sample).unwrap();
        let mut periods: Vec<IncomeStatementPeriod> = parsed
            .data
            .unwrap()
            .into_iter()
            .filter_map(|row| {
                Some(IncomeStatementPeriod {
                    period_end: row.period_end?,
                    revenue: Decimal::from_str(&row.revenue?.to_string()).ok()?,
                    net_profit: row.net_profit.and_then(|v| Decimal::from_str(&v.to_string()).ok()),
                })
            })
            .collect();
        periods.sort_by(|a, b| a.period_end.cmp(&b.period_end));
        assert_eq!(periods[0].period_end, "2025-03-31");
        assert_eq!(periods[1].period_end, "2026-03-31");
    }

    #[test]
    fn missing_fields_in_income_statement_row_are_skipped_not_panicking() {
        let sample = r#"{"data": [{"period_end": "2026-03-31", "revenue": null, "net_profit": 68000000000.0}]}"#;
        let parsed: IncomeStatementResponse = serde_json::from_str(sample).unwrap();
        let periods: Vec<IncomeStatementPeriod> = parsed
            .data
            .unwrap()
            .into_iter()
            .filter_map(|row| {
                Some(IncomeStatementPeriod {
                    period_end: row.period_end?,
                    revenue: Decimal::from_str(&row.revenue?.to_string()).ok()?,
                    net_profit: row.net_profit.and_then(|v| Decimal::from_str(&v.to_string()).ok()),
                })
            })
            .collect();
        assert_eq!(periods.len(), 0);
    }

    #[test]
    fn parses_a_news_response_keyed_by_instrument_and_sorts_newest_first() {
        let sample = r#"{"data": {"NSE_EQ|INE002A01018": [
            {"headline": "Older article", "summary": "s1", "link": "https://a.com/1", "published_at": 1721664000000},
            {"headline": "Newer article", "summary": "s2", "link": "https://a.com/2", "published_at": 1721836800000}
        ]}}"#;
        let parsed: NewsResponse = serde_json::from_str(sample).unwrap();
        let mut items: Vec<UpstoxNewsItem> = parsed
            .data
            .unwrap()
            .into_values()
            .flatten()
            .map(|raw| UpstoxNewsItem { headline: raw.headline, summary: raw.summary, link: raw.link, published_at_ms: raw.published_at })
            .collect();
        items.sort_by(|a, b| b.published_at_ms.cmp(&a.published_at_ms));
        assert_eq!(items[0].headline, "Newer article");
        assert_eq!(items.len(), 2);
    }
}

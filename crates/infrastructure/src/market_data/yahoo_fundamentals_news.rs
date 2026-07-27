//! Company fundamentals, revenue history, and news — a different pair of
//! Yahoo endpoints from the quote/chart ones in yahoo_finance.rs
//! (quoteSummary and search, not v8/finance/chart), so kept in their own
//! file even though it's the same YahooFinanceProvider type.
//!
//! HONESTY NOTE, matching the one at the top of yahoo_finance.rs: this is
//! written against Yahoo's documented/commonly-referenced JSON shape for
//! these two endpoints, but — like the rest of this integration — has NOT
//! been verified against a live response (this sandbox has no general
//! internet access to Yahoo). The first real test is whichever machine
//! actually runs this build. If the News & Fundamentals screen shows
//! nothing or errors immediately, this shape having drifted from Yahoo's
//! actual current response is the first thing to suspect.
//!
//! Regulatory prioritization is a plain keyword match over ordinary news
//! headlines, NOT a real separate regulatory-filings feed — there's no
//! verified free BSE/NSE announcements API (see the research discussion:
//! only paid vendors and unofficial community scrapers exist), so this is
//! the honest, achievable version: surface likely-regulatory items first
//! within the same general news list, clearly labeled as such, not a
//! guarantee of catching every real filing.

use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use serde::Deserialize;

use super::yahoo_finance::YahooFinanceProvider;
use super::MarketDataError;

#[derive(Debug, Clone)]
pub struct Fundamentals {
    pub sector: Option<String>,
    pub industry: Option<String>,
    pub description: Option<String>,
    pub market_cap: Option<Decimal>,
    pub pe_ratio: Option<Decimal>,
    pub dividend_yield: Option<Decimal>,
    pub week52_high: Option<Decimal>,
    pub week52_low: Option<Decimal>,
    /// Oldest first, one entry per reporting period Yahoo returns (usually
    /// the last 4 annual periods).
    pub revenue_by_period: Vec<RevenuePeriod>,
}

#[derive(Debug, Clone)]
pub struct RevenuePeriod {
    pub period_end: String,
    pub revenue: Decimal,
    pub net_income: Option<Decimal>,
}

#[derive(Debug, Clone)]
pub struct NewsItem {
    pub title: String,
    pub publisher: String,
    pub link: String,
    pub published_at: DateTime<Utc>,
    /// Plain keyword match over the headline — see the module-level
    /// honesty note. Not a verified regulatory-filings source.
    pub is_regulatory: bool,
}

/// Headline keywords that suggest a regulatory/corporate-disclosure item
/// rather than ordinary market commentary — deliberately conservative
/// (favors missing a real filing over mislabeling routine news as one).
const REGULATORY_KEYWORDS: &[&str] = &[
    "board meeting",
    "board approves",
    "agm",
    "annual general meeting",
    "dividend declared",
    "interim dividend",
    "shareholding pattern",
    "sebi",
    "disclosure",
    "corporate announcement",
    "rights issue",
    "bonus issue",
    "stock split",
    "buyback",
    "credit rating",
    "sec filing",
    "10-q",
    "10-k",
    "8-k",
    "regulatory filing",
];

fn looks_regulatory(title: &str) -> bool {
    let lower = title.to_lowercase();
    REGULATORY_KEYWORDS.iter().any(|kw| lower.contains(kw))
}

// ---- quoteSummary (fundamentals + revenue) ----

#[derive(Deserialize)]
struct QuoteSummaryResponse {
    #[serde(rename = "quoteSummary")]
    quote_summary: QuoteSummaryWrapper,
}

#[derive(Deserialize)]
struct QuoteSummaryWrapper {
    result: Option<Vec<QuoteSummaryResult>>,
    error: Option<YahooApiError>,
}

#[derive(Deserialize)]
struct YahooApiError {
    description: String,
}

#[derive(Deserialize)]
struct QuoteSummaryResult {
    #[serde(rename = "assetProfile")]
    asset_profile: Option<AssetProfile>,
    #[serde(rename = "summaryDetail")]
    summary_detail: Option<SummaryDetail>,
    #[serde(rename = "incomeStatementHistory")]
    income_statement_history: Option<IncomeStatementHistory>,
}

#[derive(Deserialize)]
struct AssetProfile {
    sector: Option<String>,
    industry: Option<String>,
    #[serde(rename = "longBusinessSummary")]
    long_business_summary: Option<String>,
}

#[derive(Deserialize)]
struct SummaryDetail {
    #[serde(rename = "marketCap")]
    market_cap: Option<RawValue>,
    #[serde(rename = "trailingPE")]
    trailing_pe: Option<RawValue>,
    #[serde(rename = "dividendYield")]
    dividend_yield: Option<RawValue>,
    #[serde(rename = "fiftyTwoWeekHigh")]
    fifty_two_week_high: Option<RawValue>,
    #[serde(rename = "fiftyTwoWeekLow")]
    fifty_two_week_low: Option<RawValue>,
}

#[derive(Deserialize)]
struct IncomeStatementHistory {
    #[serde(rename = "incomeStatementHistory")]
    income_statement_history: Vec<IncomeStatementPeriod>,
}

#[derive(Deserialize)]
struct IncomeStatementPeriod {
    #[serde(rename = "endDate")]
    end_date: Option<RawFmtValue>,
    #[serde(rename = "totalRevenue")]
    total_revenue: Option<RawValue>,
    #[serde(rename = "netIncome")]
    net_income: Option<RawValue>,
}

/// Yahoo wraps most numeric fields as `{"raw": 123.45, "fmt": "123.45"}`
/// rather than a bare number — this mirrors that shape.
#[derive(Deserialize)]
struct RawValue {
    raw: Option<f64>,
}

#[derive(Deserialize)]
struct RawFmtValue {
    fmt: Option<String>,
}

fn raw_to_decimal(v: &Option<RawValue>) -> Option<Decimal> {
    v.as_ref()?.raw.and_then(|f| Decimal::try_from(f).ok())
}

// ---- search (news) ----

#[derive(Deserialize)]
struct SearchResponse {
    news: Option<Vec<NewsRaw>>,
}

#[derive(Deserialize)]
struct NewsRaw {
    title: String,
    publisher: String,
    link: String,
    #[serde(rename = "providerPublishTime")]
    provider_publish_time: i64,
}

impl YahooFinanceProvider {
    pub async fn fetch_fundamentals(&self, symbol: &str, exchange: &str) -> Result<Fundamentals, MarketDataError> {
        let yahoo_symbol = Self::to_yahoo_symbol(symbol, exchange);
        let url = format!(
            "https://query1.finance.yahoo.com/v10/finance/quoteSummary/{yahoo_symbol}?modules=assetProfile,summaryDetail,incomeStatementHistory"
        );
        let response = self
            .http_client()
            .get(&url)
            .send()
            .await
            .map_err(|e| MarketDataError::RequestFailed(e.to_string()))?;

        let body: QuoteSummaryResponse = response.json().await.map_err(|e| {
            MarketDataError::UnexpectedResponse(format!(
                "couldn't parse Yahoo fundamentals for {yahoo_symbol}: {e} — the endpoint's shape may have drifted from what this was written against (see the honesty note at the top of this file)"
            ))
        })?;

        if let Some(err) = body.quote_summary.error {
            return Err(MarketDataError::NoData(format!("{yahoo_symbol}: {}", err.description)));
        }
        let result = body
            .quote_summary
            .result
            .and_then(|r| r.into_iter().next())
            .ok_or_else(|| MarketDataError::NoData(yahoo_symbol.clone()))?;

        let revenue_by_period = result
            .income_statement_history
            .map(|h| {
                h.income_statement_history
                    .into_iter()
                    .filter_map(|p| {
                        let revenue = raw_to_decimal(&p.total_revenue)?;
                        Some(RevenuePeriod {
                            period_end: p.end_date.and_then(|d| d.fmt).unwrap_or_default(),
                            revenue,
                            net_income: raw_to_decimal(&p.net_income),
                        })
                    })
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        // Yahoo returns most-recent-first; flip to oldest-first so a
        // revenue chart reads left-to-right chronologically.
        let mut revenue_by_period = revenue_by_period;
        revenue_by_period.reverse();

        Ok(Fundamentals {
            sector: result.asset_profile.as_ref().and_then(|p| p.sector.clone()),
            industry: result.asset_profile.as_ref().and_then(|p| p.industry.clone()),
            description: result.asset_profile.and_then(|p| p.long_business_summary),
            market_cap: result.summary_detail.as_ref().and_then(|s| raw_to_decimal(&s.market_cap)),
            pe_ratio: result.summary_detail.as_ref().and_then(|s| raw_to_decimal(&s.trailing_pe)),
            dividend_yield: result.summary_detail.as_ref().and_then(|s| raw_to_decimal(&s.dividend_yield)),
            week52_high: result.summary_detail.as_ref().and_then(|s| raw_to_decimal(&s.fifty_two_week_high)),
            week52_low: result.summary_detail.as_ref().and_then(|s| raw_to_decimal(&s.fifty_two_week_low)),
            revenue_by_period,
        })
    }

    pub async fn fetch_news(&self, symbol: &str, exchange: &str) -> Result<Vec<NewsItem>, MarketDataError> {
        let yahoo_symbol = Self::to_yahoo_symbol(symbol, exchange);
        let url = format!("https://query1.finance.yahoo.com/v1/finance/search?q={yahoo_symbol}&newsCount=15");
        let response = self
            .http_client()
            .get(&url)
            .send()
            .await
            .map_err(|e| MarketDataError::RequestFailed(e.to_string()))?;

        let body: SearchResponse = response.json().await.map_err(|e| {
            MarketDataError::UnexpectedResponse(format!("couldn't parse Yahoo news search for {yahoo_symbol}: {e}"))
        })?;

        let mut items: Vec<NewsItem> = body
            .news
            .unwrap_or_default()
            .into_iter()
            .filter_map(|n| {
                Some(NewsItem {
                    is_regulatory: looks_regulatory(&n.title),
                    title: n.title,
                    publisher: n.publisher,
                    link: n.link,
                    published_at: DateTime::from_timestamp(n.provider_publish_time, 0)?,
                })
            })
            .collect();

        // Regulatory items first (newest first within each group), then
        // everything else, newest first — then cap at 5 per the "top 5,
        // priority to regulatory submissions" requirement.
        items.sort_by(|a, b| match (a.is_regulatory, b.is_regulatory) {
            (true, false) => std::cmp::Ordering::Less,
            (false, true) => std::cmp::Ordering::Greater,
            _ => b.published_at.cmp(&a.published_at),
        });
        items.truncate(5);
        Ok(items)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn regulatory_keywords_are_matched_case_insensitively() {
        assert!(looks_regulatory("Board approves Q1 results, declares interim dividend"));
        assert!(looks_regulatory("NSE DISCLOSURE: change in promoter shareholding"));
        assert!(looks_regulatory("Company files 10-Q with SEC"));
        assert!(!looks_regulatory("Analyst maintains buy rating on strong earnings"));
        assert!(!looks_regulatory("Retail arm eyes expansion into tier-2 cities"));
    }

    #[test]
    fn sorts_regulatory_items_first_then_by_recency_and_caps_at_five() {
        let make = |title: &str, ts: i64| NewsItem {
            title: title.to_string(),
            publisher: "Test".to_string(),
            link: "http://example.com".to_string(),
            published_at: DateTime::from_timestamp(ts, 0).unwrap(),
            is_regulatory: looks_regulatory(title),
        };
        let mut items = vec![
            make("Analyst note: buy rating", 300),
            make("Board approves dividend", 100),
            make("Retail expansion news", 400),
            make("SEBI disclosure filed", 200),
            make("Old regulatory filing", 50),
            make("Newest general news", 500),
        ];
        items.sort_by(|a, b| match (a.is_regulatory, b.is_regulatory) {
            (true, false) => std::cmp::Ordering::Less,
            (false, true) => std::cmp::Ordering::Greater,
            _ => b.published_at.cmp(&a.published_at),
        });
        items.truncate(5);

        assert_eq!(items.len(), 5);
        assert!(items[0].is_regulatory);
        assert!(items[1].is_regulatory);
        assert!(items[2].is_regulatory);
        // Among the 3 regulatory items, newest (200) should come first.
        assert_eq!(items[0].title, "SEBI disclosure filed");
    }

    #[test]
    fn raw_value_extracts_the_inner_float_as_decimal() {
        let v = Some(RawValue { raw: Some(123.45) });
        assert_eq!(raw_to_decimal(&v), Decimal::try_from(123.45).ok());
    }

    #[test]
    fn raw_value_handles_missing_raw_field_gracefully() {
        let v = Some(RawValue { raw: None });
        assert_eq!(raw_to_decimal(&v), None);
        assert_eq!(raw_to_decimal(&None), None);
    }

    #[test]
    fn parses_a_documented_shaped_quote_summary_response() {
        let sample = r#"{
            "quoteSummary": {
                "result": [{
                    "assetProfile": {"sector": "Technology", "industry": "Software", "longBusinessSummary": "A company."},
                    "summaryDetail": {"marketCap": {"raw": 1000000.0}, "trailingPE": {"raw": 25.5}, "dividendYield": {"raw": 0.01}, "fiftyTwoWeekHigh": {"raw": 200.0}, "fiftyTwoWeekLow": {"raw": 100.0}},
                    "incomeStatementHistory": {"incomeStatementHistory": [
                        {"endDate": {"fmt": "2025-03-31"}, "totalRevenue": {"raw": 500000.0}, "netIncome": {"raw": 50000.0}},
                        {"endDate": {"fmt": "2024-03-31"}, "totalRevenue": {"raw": 450000.0}, "netIncome": {"raw": 40000.0}}
                    ]}
                }],
                "error": null
            }
        }"#;
        let parsed: QuoteSummaryResponse = serde_json::from_str(sample).unwrap();
        let result = parsed.quote_summary.result.unwrap().into_iter().next().unwrap();
        assert_eq!(result.asset_profile.unwrap().sector.as_deref(), Some("Technology"));
        assert_eq!(raw_to_decimal(&result.summary_detail.unwrap().market_cap), Decimal::try_from(1000000.0).ok());
        assert_eq!(result.income_statement_history.unwrap().income_statement_history.len(), 2);
    }

    #[test]
    fn parses_a_documented_shaped_news_search_response() {
        let sample = r#"{"news": [{"title": "Test headline", "publisher": "Reuters", "link": "http://example.com/a", "providerPublishTime": 1721606400}]}"#;
        let parsed: SearchResponse = serde_json::from_str(sample).unwrap();
        let news = parsed.news.unwrap();
        assert_eq!(news.len(), 1);
        assert_eq!(news[0].title, "Test headline");
    }
}

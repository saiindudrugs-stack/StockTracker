//! BSE's own website (bseindia.com) serves announcements from an
//! internal API, similar in spirit to NSE's but with real differences:
//! BSE identifies instruments by a numeric "scripcode", not a trading
//! symbol, and the requests seen in real, working implementations don't
//! show the same cookie-warm-up dance NSE requires — Origin/Referer
//! headers alone appear sufficient (confirmed via a real, working code
//! sample using exactly this header set against `api.bseindia.com`).
//!
//! CONFIDENCE NOTE, stated plainly — this is the least-certain endpoint
//! in this whole app: the scripcode-search endpoint
//! (`getQouteSearch.aspx`) is confirmed from a real, working code sample.
//! The actual announcements endpoint path is constructed from a strong
//! but not byte-confirmed pattern (`api.bseindia.com/BseIndiaAPI/api/
//! {EndpointName}/w`, the same convention as BSE's confirmed
//! `ListofScripData` endpoint) combined with the documented paginated
//! `{"Table": [...], "Table1": [{"ROWCNT": n}]}` response shape a real
//! Python library built against this same data returns. If the path is
//! wrong, this fails loudly (a parse or 404 error), not silently.

use reqwest::Client;
use serde::Deserialize;

use crate::market_data::nse_announcements::RegulatoryAnnouncement;
use crate::market_data::MarketDataError;

const USER_AGENT: &str = "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/124.0 Safari/537.36";

pub struct BseAnnouncementsClient {
    http: Client,
}

impl BseAnnouncementsClient {
    pub fn new() -> Self {
        Self { http: Client::new() }
    }

    async fn resolve_scrip_code(&self, symbol: &str) -> Result<String, MarketDataError> {
        let url = format!("https://api.bseindia.com/Msource/1D/getQouteSearch.aspx?Type=EQ&text={symbol}&flag=site");
        let response = self
            .http
            .get(&url)
            .header("User-Agent", USER_AGENT)
            .header("Accept", "application/json, text/plain, */*")
            .header("Origin", "https://www.bseindia.com")
            .header("Referer", "https://www.bseindia.com/")
            // These four are present in a real, confirmed-working code
            // sample hitting this exact host — added after a real
            // failure ("expected value at line 1 column 1", meaning the
            // body wasn't valid JSON at all) suggested the request
            // without them wasn't being treated as a genuine browser
            // request by BSE's server.
            .header("sec-ch-ua-mobile", "?0")
            .header("sec-fetch-site", "same-site")
            .header("sec-fetch-mode", "cors")
            .header("sec-fetch-dest", "empty")
            .send()
            .await
            .map_err(|e| MarketDataError::RequestFailed(format!("BSE scrip code lookup failed: {e}")))?;

        let status = response.status();
        let text = response.text().await.map_err(|e| MarketDataError::RequestFailed(e.to_string()))?;
        parse_scrip_code_from_response(&text, symbol, status)
    }

    pub async fn fetch_announcements(&self, symbol: &str) -> Result<Vec<RegulatoryAnnouncement>, MarketDataError> {
        let scrip_code = self.resolve_scrip_code(symbol).await?;

        let url = format!("https://api.bseindia.com/BseIndiaAPI/api/AnnSubCategoryGetData/w?pageno=1&strCat=-1&strPrevDate=&strScrip={scrip_code}&strSearch=P&strToDate=&strType=C&subcategory=-1");
        let response = self
            .http
            .get(&url)
            .header("User-Agent", USER_AGENT)
            .header("Accept", "application/json, text/plain, */*")
            .header("Origin", "https://www.bseindia.com")
            .header("Referer", "https://www.bseindia.com/corporates/ann.html")
            .send()
            .await
            .map_err(|e| MarketDataError::RequestFailed(format!("BSE announcements request failed: {e}")))?;

        let body: BseAnnouncementsResponse = response
            .json()
            .await
            .map_err(|e| MarketDataError::UnexpectedResponse(format!("couldn't parse BSE announcements for {symbol}: {e}")))?;

        let mut items: Vec<RegulatoryAnnouncement> = body
            .table
            .into_iter()
            .map(|r| RegulatoryAnnouncement {
                symbol: symbol.to_string(),
                subject: r.news_sub.or(r.headline).unwrap_or_else(|| "Announcement".to_string()),
                broadcast_date: r.news_dt.or(r.dissem_dt).unwrap_or_default(),
                attachment_url: r.attachment_name,
            })
            .collect();
        // Page 1 of BSE's own pagination is usually already a reasonable
        // size, but this caps it defensively regardless — same 20-item
        // limit as NSE, for consistency. BSE's NEWS_DT format is ISO-like
        // (e.g. "2026-09-15T18:30:00"), which — unlike NSE's "DD-Mon-
        // YYYY" format — sorts correctly as a plain string, so no
        // separate date-parsing step is needed here.
        items.sort_by(|a, b| b.broadcast_date.cmp(&a.broadcast_date));
        items.truncate(20);
        Ok(items)
    }
}

impl Default for BseAnnouncementsClient {
    fn default() -> Self {
        Self::new()
    }
}

/// The scrip-code search response shape wasn't confirmed byte-for-byte
/// either — this handles both a plain JSON array and a JSON object
/// wrapping the array, since real BSE endpoints have been observed doing
/// either depending on the specific one. Whichever shape doesn't match
/// just means an empty result here, not a panic.
/// Confirmed via a real failure's diagnostic output: this endpoint
/// doesn't return JSON at all — it returns an HTML autocomplete/search-
/// suggestion fragment, e.g.:
///   <li class='quotemenu quotemenuselect'><a id='...' href='/stock-
///   share-price/reliance-industries-ltd/reliance/500325/'><strong>
///   RELIANCE</strong> I...
/// The scrip code is the last numeric path segment in that href
/// (`500325` above). This replaces the original JSON-parsing attempt,
/// which was a reasonable guess before there was a real response to
/// check it against — now there is.
fn parse_scrip_code_from_response(text: &str, symbol: &str, status: reqwest::StatusCode) -> Result<String, MarketDataError> {
    if let Some(code) = extract_scrip_code_from_html(text) {
        return Ok(code);
    }

    // Fallback: in case a different environment/time genuinely gets a
    // JSON response instead (some BSE endpoints do respond in either
    // shape depending on headers), still try parsing that too, rather
    // than assuming HTML is the only possible shape forever.
    #[derive(Deserialize)]
    struct ScripRow {
        #[serde(rename = "scrip_cd", alias = "Scrip_Cd", alias = "SCRIP_CD")]
        scrip_cd: Option<serde_json::Value>,
    }
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum ScripResponse {
        Array(Vec<ScripRow>),
        Wrapped { #[serde(rename = "Table")] table: Vec<ScripRow> },
    }
    if let Ok(parsed) = serde_json::from_str::<ScripResponse>(text) {
        let rows = match parsed {
            ScripResponse::Array(rows) => rows,
            ScripResponse::Wrapped { table } => table,
        };
        if let Some(code) = rows.into_iter().find_map(|r| r.scrip_cd) {
            return Ok(code.to_string().trim_matches('"').to_string());
        }
    }

    let snippet: String = text.chars().take(200).collect();
    Err(MarketDataError::UnexpectedResponse(format!(
        "couldn't find a BSE scrip code for {symbol} in either HTML or JSON shape (HTTP {status}, body started with: {snippet:?})"
    )))
}

/// Looks for `href='.../{slug}/{digits}/'` (or double-quoted) and returns
/// the digit segment — BSE's own search-suggestion markup encodes the
/// scrip code as the final path segment of the stock's detail-page URL.
fn extract_scrip_code_from_html(html: &str) -> Option<String> {
    for quote in ['\'', '"'] {
        let needle = format!("href={quote}");
        let mut search_from = 0;
        while let Some(start) = html[search_from..].find(&needle) {
            let href_start = search_from + start + needle.len();
            let Some(end_offset) = html[href_start..].find(quote) else { break };
            let href = &html[href_start..href_start + end_offset];
            let last_segment = href.trim_end_matches('/').rsplit('/').next().unwrap_or("");
            if !last_segment.is_empty() && last_segment.chars().all(|c| c.is_ascii_digit()) {
                return Some(last_segment.to_string());
            }
            search_from = href_start + end_offset;
        }
    }
    None
}

#[derive(Deserialize)]
struct BseAnnouncementsResponse {
    #[serde(rename = "Table", default)]
    table: Vec<RawBseAnnouncement>,
}

#[derive(Deserialize)]
struct RawBseAnnouncement {
    #[serde(rename = "NEWSSUB")]
    news_sub: Option<String>,
    #[serde(rename = "HEADLINE")]
    headline: Option<String>,
    #[serde(rename = "NEWS_DT")]
    news_dt: Option<String>,
    #[serde(rename = "DissemDT")]
    dissem_dt: Option<String>,
    #[serde(rename = "ATTACHMENTNAME")]
    attachment_name: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_a_plain_array_scrip_code_response() {
        let sample = r#"[{"scrip_cd": 532540, "Scrip_Name": "TCS"}]"#;
        let code = parse_scrip_code_from_response(sample, "TCS", reqwest::StatusCode::OK).unwrap();
        assert_eq!(code, "532540");
    }

    #[test]
    fn parses_a_table_wrapped_scrip_code_response() {
        let sample = r#"{"Table": [{"scrip_cd": 500325}]}"#;
        let code = parse_scrip_code_from_response(sample, "RELIANCE", reqwest::StatusCode::OK).unwrap();
        assert_eq!(code, "500325");
    }

    #[test]
    fn extracts_scrip_code_from_the_real_html_response_captured_live() {
        // Verbatim (truncated) from a real failure's diagnostic output —
        // BSE's actual response shape, not a guess.
        let html = "<li class='quotemenu quotemenuselect'><a id='/stock-share-price/reliance-industries-ltd/reliance-industries-ltd/reliance/500325/' href='/stock-share-price/reliance-industries-ltd/reliance/500325/'><strong>RELIANCE</strong> I";
        let code = parse_scrip_code_from_response(html, "RELIANCE", reqwest::StatusCode::OK).unwrap();
        assert_eq!(code, "500325");
    }

    #[test]
    fn extracts_scrip_code_with_double_quoted_href() {
        let html = r#"<a href="/stock-share-price/tata-consultancy/tcs/532540/">TCS</a>"#;
        let code = parse_scrip_code_from_response(html, "TCS", reqwest::StatusCode::OK).unwrap();
        assert_eq!(code, "532540");
    }

    #[test]
    fn no_matching_scrip_code_is_a_clear_error_not_a_panic() {
        let sample = r#"[]"#;
        let result = parse_scrip_code_from_response(sample, "NOPE", reqwest::StatusCode::OK);
        assert!(result.is_err());
    }

    #[test]
    fn invalid_json_produces_a_diagnostic_error_with_status_and_body_snippet() {
        let sample = "";
        let result = parse_scrip_code_from_response(sample, "RELIANCE", reqwest::StatusCode::FORBIDDEN);
        let err = result.unwrap_err().to_string();
        assert!(err.contains("403"), "error should include the HTTP status: {err}");
    }

    #[test]
    fn parses_a_plausibly_shaped_bse_announcement_row() {
        let sample = r#"{"Table": [{"NEWSSUB": "Board Meeting Outcome", "NEWS_DT": "2026-09-15T18:30:00"}]}"#;
        let parsed: BseAnnouncementsResponse = serde_json::from_str(sample).unwrap();
        assert_eq!(parsed.table[0].news_sub.as_deref(), Some("Board Meeting Outcome"));
    }

    #[test]
    fn empty_table_parses_to_no_announcements_not_an_error() {
        let sample = r#"{"Table": []}"#;
        let parsed: BseAnnouncementsResponse = serde_json::from_str(sample).unwrap();
        assert_eq!(parsed.table.len(), 0);
    }

    #[test]
    fn missing_table_key_defaults_to_empty_not_a_parse_failure() {
        let sample = r#"{}"#;
        let parsed: BseAnnouncementsResponse = serde_json::from_str(sample).unwrap();
        assert_eq!(parsed.table.len(), 0);
    }
}

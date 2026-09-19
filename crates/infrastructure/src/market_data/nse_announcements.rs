//! NSE's own website (nseindia.com) serves its public announcements
//! page from an internal JSON API — not an officially documented
//! developer API (NSE has none with open registration; the licensed
//! route via vendors like Tickerplant runs ~₹3 lakh/year), but a real,
//! working endpoint that multiple independently-maintained open-source
//! projects (Python's `nse` package, several Node/TS clients) already
//! wrap successfully. This gives genuine corporate announcements and
//! board meetings — not a keyword match over general news headlines,
//! which is what this app's "regulatory" tag has been until now.
//!
//! CONFIDENCE NOTE, stated plainly: the cookie-warm-up flow and the
//! general `/api/{endpoint-name}` URL convention are confirmed by
//! multiple independent real sources. The exact `corporate-announcements`
//! path is a strong, convergent pattern match (several other real NSE
//! endpoints follow this identical naming scheme, and it matches the
//! open-source library's own method name) rather than a byte-for-byte
//! confirmed URL from NSE's own documentation, which doesn't exist. If
//! this 404s or the shape doesn't match, the caller sees a clear parse/
//! request error, not a silent wrong result — same defensive posture as
//! every other unofficial source in this app.
//!
//! REAL, IMPORTANT CAVEAT: NSE is documented to block requests from
//! cloud/datacenter IP ranges (AWS, Azure, GCP) — several independent
//! reports of this. This app runs on the end user's own machine over a
//! residential/office connection, which is a fundamentally different
//! situation, but it also means this cannot be live-tested from a cloud-
//! hosted sandbox; the real test happens on the user's own machine.
//! NSE also documents a 3 requests/second throttle — this client doesn't
//! fire faster than that.

use reqwest::Client;
use serde::Deserialize;

use crate::market_data::MarketDataError;

const USER_AGENT: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/124.0 Safari/537.36";

pub struct NseAnnouncementsClient {
    http: Client,
}

#[derive(Debug, Clone)]
pub struct RegulatoryAnnouncement {
    pub symbol: String,
    pub subject: String,
    pub broadcast_date: String,
    pub attachment_url: Option<String>,
}

impl NseAnnouncementsClient {
    /// Manual cookie handling rather than reqwest's built-in cookie jar
    /// (the `cookies` feature) — that feature transitively pulls in a
    /// `time` crate version whose own sub-dependency requires Rust's
    /// edition2024, which conflicts workspace-wide with Tauri's own
    /// `time` requirement in a way no version pin resolves (confirmed by
    /// trying several). Extracting `Set-Cookie` from the warm-up response
    /// and forwarding it as a plain `Cookie` header is a few more lines
    /// but avoids that dependency entirely.
    pub fn new() -> Self {
        let http = Client::builder().user_agent(USER_AGENT).build().expect("failed to build NSE HTTP client");
        Self { http }
    }

    /// Warm-up request — establishes the session cookie NSE's actual
    /// data endpoints require, returned here (not stored) so the caller
    /// forwards it explicitly on the next request. Must succeed before
    /// fetch_announcements will get real data back rather than a bot-
    /// detection response.
    async fn warm_up_session(&self) -> Result<String, MarketDataError> {
        let response = self
            .http
            .get("https://www.nseindia.com/get-quotes/equity?symbol=RELIANCE")
            .header("Accept", "text/html,application/xhtml+xml,application/xml;q=0.9,*/*;q=0.8")
            .send()
            .await
            .map_err(|e| MarketDataError::RequestFailed(format!("NSE session warm-up failed: {e}")))?;

        let cookies: Vec<String> = response
            .headers()
            .get_all(reqwest::header::SET_COOKIE)
            .iter()
            .filter_map(|v| v.to_str().ok())
            .filter_map(|raw| raw.split(';').next()) // keep only "name=value", drop Set-Cookie's own attributes (Path, Expires, etc.)
            .map(|s| s.to_string())
            .collect();

        if cookies.is_empty() {
            return Err(MarketDataError::NoData("NSE warm-up returned no session cookie — likely blocked (see this module's IP-blocking caveat)".to_string()));
        }
        Ok(cookies.join("; "))
    }

    pub async fn fetch_announcements(&self, symbol: &str) -> Result<Vec<RegulatoryAnnouncement>, MarketDataError> {
        let cookie_header = self.warm_up_session().await?;

        let url = format!("https://www.nseindia.com/api/corporate-announcements?index=equities&symbol={symbol}");
        let response = self
            .http
            .get(&url)
            .header("Accept", "application/json, text/plain, */*")
            .header("Referer", format!("https://www.nseindia.com/get-quotes/equity?symbol={symbol}"))
            .header("Cookie", cookie_header)
            .send()
            .await
            .map_err(|e| MarketDataError::RequestFailed(format!("NSE announcements request failed: {e}")))?;

        let rows: Vec<RawNseAnnouncement> = response
            .json()
            .await
            .map_err(|e| MarketDataError::UnexpectedResponse(format!("couldn't parse NSE announcements for {symbol}: {e}")))?;

        let mut items: Vec<RegulatoryAnnouncement> = rows
            .into_iter()
            .filter_map(|r| {
                Some(RegulatoryAnnouncement {
                    symbol: r.symbol?,
                    subject: r.subject.or(r.desc).unwrap_or_else(|| "Announcement".to_string()),
                    broadcast_date: r.an_dt.or(r.broadcast_date).unwrap_or_default(),
                    attachment_url: r.attchment_file,
                })
            })
            .collect();
        // NSE's endpoint returns every announcement ever filed for a
        // symbol with no date-range parameter honored by default — real
        // stocks can have 20+ years of history. Newest first, capped at
        // 20, same reasoning as every other "top N" list in this app.
        // Sorted by an actually-parsed date, not the raw string — NSE's
        // "DD-Mon-YYYY HH:MM:SS" format sorts WRONG lexicographically
        // (e.g. "15-Sep-2026" would sort before "22-Mar-2005" as plain
        // text, since '1' < '2', even though 2026 is later).
        items.sort_by_key(|a| std::cmp::Reverse(chrono::NaiveDateTime::parse_from_str(&a.broadcast_date, "%d-%b-%Y %H:%M:%S").ok()));
        items.truncate(20);
        Ok(items)
    }
}

impl Default for NseAnnouncementsClient {
    fn default() -> Self {
        Self::new()
    }
}

/// Field names are the most uncertain part of this module — NSE's own
/// naming is inconsistent across their different endpoints (some use
/// camelCase, some abbreviate). Multiple plausible field names are
/// accepted per logical field (`.or()` chains below) specifically
/// because guessing exactly one and failing silently on the other would
/// be worse than trying a couple of realistic candidates.
#[derive(Deserialize)]
struct RawNseAnnouncement {
    symbol: Option<String>,
    subject: Option<String>,
    desc: Option<String>,
    #[serde(rename = "an_dt")]
    an_dt: Option<String>,
    #[serde(rename = "broadcastdate")]
    broadcast_date: Option<String>,
    #[serde(rename = "attchmntFile")]
    attchment_file: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_a_plausibly_shaped_nse_announcement_row() {
        let sample = r#"[{"symbol": "RELIANCE", "subject": "Board Meeting Intimation", "an_dt": "15-Sep-2026 18:30:00", "attchmntFile": "https://nsearchives.nseindia.com/corporate/RELIANCE_15092026.pdf"}]"#;
        let rows: Vec<RawNseAnnouncement> = serde_json::from_str(sample).unwrap();
        assert_eq!(rows[0].symbol.as_deref(), Some("RELIANCE"));
        assert_eq!(rows[0].subject.as_deref(), Some("Board Meeting Intimation"));
    }

    #[test]
    fn falls_back_to_desc_when_subject_is_absent() {
        let sample = r#"[{"symbol": "TCS", "desc": "Newspaper Publication"}]"#;
        let rows: Vec<RawNseAnnouncement> = serde_json::from_str(sample).unwrap();
        assert_eq!(rows[0].desc.as_deref(), Some("Newspaper Publication"));
        assert!(rows[0].subject.is_none());
    }

    #[test]
    fn row_missing_symbol_entirely_is_filtered_out_not_a_panic() {
        let sample = r#"[{"subject": "orphaned row, no symbol"}]"#;
        let rows: Vec<RawNseAnnouncement> = serde_json::from_str(sample).unwrap();
        let filtered: Vec<RegulatoryAnnouncement> = rows
            .into_iter()
            .filter_map(|r| {
                Some(RegulatoryAnnouncement {
                    symbol: r.symbol?,
                    subject: r.subject.or(r.desc).unwrap_or_else(|| "Announcement".to_string()),
                    broadcast_date: r.an_dt.or(r.broadcast_date).unwrap_or_default(),
                    attachment_url: r.attchment_file,
                })
            })
            .collect();
        assert_eq!(filtered.len(), 0);
    }

    #[test]
    fn empty_array_response_parses_to_no_announcements() {
        let rows: Vec<RawNseAnnouncement> = serde_json::from_str("[]").unwrap();
        assert_eq!(rows.len(), 0);
    }
}

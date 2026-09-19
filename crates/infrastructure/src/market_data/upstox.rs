//! Upstox — a genuine broker-grade data source, unlike Yahoo's unofficial
//! scrape. Requires an actual Upstox trading account and an "Analytics
//! Token" (free, generated once via a button click on Upstox's Developer
//! Apps page, valid 1 year — NOT a daily OAuth login).
//!
//! HONESTY NOTE: built against Upstox's own published API documentation
//! (endpoint URLs and shapes independently confirmed across multiple
//! sources during research), NOT live-verified from this sandbox — no
//! Upstox account/token was available to test with while writing this.
//! The first real test is a real token and a real symbol.

use async_trait::async_trait;
use chrono::NaiveDate;
use flate2::read::GzDecoder;
use reqwest::Client;
use rust_decimal::Decimal;
use serde::Deserialize;
use std::io::Read;
use std::str::FromStr;
use std::sync::Arc;

use super::{DailyBar, MarketDataError, MarketDataProvider, Quote};
use crate::sqlite::{SqliteAppSettings, SqliteUpstoxInstrumentCache, UpstoxInstrumentRow};

pub const UPSTOX_ANALYTICS_TOKEN_SETTING: &str = "upstox_analytics_token";

const INSTRUMENT_FILE_URLS: &[(&str, &str)] = &[
    ("NSE", "https://assets.upstox.com/market-quote/instruments/exchange/NSE.json.gz"),
    ("BSE", "https://assets.upstox.com/market-quote/instruments/exchange/BSE.json.gz"),
];

pub struct UpstoxProvider {
    http: Client,
    settings: Arc<SqliteAppSettings>,
    instruments: Arc<SqliteUpstoxInstrumentCache>,
}

impl UpstoxProvider {
    pub fn new(settings: Arc<SqliteAppSettings>, instruments: Arc<SqliteUpstoxInstrumentCache>) -> Self {
        Self { http: Client::new(), settings, instruments }
    }

    async fn token(&self) -> Result<String, MarketDataError> {
        self.settings
            .get(UPSTOX_ANALYTICS_TOKEN_SETTING)
            .await
            .map_err(|e| MarketDataError::RequestFailed(e.to_string()))?
            .filter(|t| !t.trim().is_empty())
            .ok_or_else(|| MarketDataError::RequestFailed("no Upstox Analytics Token configured in Settings".to_string()))
    }

    /// Downloads and decompresses both NSE and BSE instrument files,
    /// wholesale-replacing the local cache. Returns the count actually
    /// cached (equities only — this file also lists F&O, indices, etc.).
    pub async fn refresh_instrument_cache(&self) -> Result<usize, MarketDataError> {
        let mut all_rows: Vec<UpstoxInstrumentRow> = Vec::new();

        for (exchange, url) in INSTRUMENT_FILE_URLS {
            let response = self.http.get(*url).send().await.map_err(|e| MarketDataError::RequestFailed(e.to_string()))?;
            let bytes = response.bytes().await.map_err(|e| MarketDataError::RequestFailed(e.to_string()))?;

            let json_text = decode_instrument_file_bytes(&bytes, exchange)?;

            let rows: Vec<RawInstrumentRow> = serde_json::from_str(&json_text)
                .map_err(|e| MarketDataError::UnexpectedResponse(format!("couldn't parse {exchange} instrument file: {e}")))?;

            let total_rows = rows.len();
            let mut kept_for_exchange = 0usize;
            for row in &rows {
                if row.instrument_type.as_deref() != Some("EQ") {
                    continue;
                }
                let Some(symbol) = row.trading_symbol.clone() else { continue };
                all_rows.push(UpstoxInstrumentRow {
                    trading_symbol: symbol,
                    exchange: exchange.to_string(),
                    instrument_key: row.instrument_key.clone(),
                    isin: row.isin.clone(),
                });
                kept_for_exchange += 1;
            }

            // Real file, real rows, but the "EQ" filter matched nothing —
            // that's a red flag the filter itself is wrong (a field name
            // or value assumption that's since drifted), not that this
            // exchange genuinely has zero equities. Failing loudly here
            // with the actual instrument_type values seen turns the next
            // report into a direct fix instead of the same "not found in
            // cache" symptom recurring with no new information.
            if total_rows > 0 && kept_for_exchange == 0 {
                let mut sample_types: Vec<String> = rows.iter().filter_map(|r| r.instrument_type.clone()).collect();
                sample_types.sort();
                sample_types.dedup();
                sample_types.truncate(10);
                return Err(MarketDataError::UnexpectedResponse(format!(
                    "{exchange} instrument file parsed {total_rows} rows but none matched instrument_type \"EQ\" — the actual values seen were: {sample_types:?}"
                )));
            }
        }

        let count = all_rows.len();
        self.instruments.replace_all(all_rows).await.map_err(|e| MarketDataError::RequestFailed(e.to_string()))?;
        Ok(count)
    }

    /// Real, officially documented endpoint (Upstox's own "Instrument
    /// Search API," announced March 2026) — confirmed via a real curl
    /// example in their docs, returning {status, data: [...], meta_data}.
    /// This replaces reliance on the bulk .gz instrument files for the
    /// critical path: a real live test showed that file download failing
    /// ("expected value at line 1 column 1" — an empty or non-JSON
    /// response), and Upstox's own docs explicitly position this search
    /// endpoint as the better fit for "look up a few instruments," which
    /// is exactly this app's use case, rather than "the full universe of
    /// instruments" the bulk files are for.
    async fn search_instrument(&self, symbol: &str, exchange: &str) -> Result<UpstoxSearchResult, MarketDataError> {
        let token = self.token().await?;
        let url = format!("https://api.upstox.com/v2/instruments/search?query={symbol}&exchanges={exchange}&segments=EQ&records=5");
        let response = self
            .http
            .get(&url)
            .header("Authorization", format!("Bearer {token}"))
            .header("Accept", "application/json")
            .send()
            .await
            .map_err(|e| MarketDataError::RequestFailed(e.to_string()))?;

        let body: SearchResponse = response
            .json()
            .await
            .map_err(|e| MarketDataError::UnexpectedResponse(format!("couldn't parse Upstox instrument search for {symbol}: {e}")))?;

        // Prefer an exact trading_symbol match over the first result —
        // free-text search can return close-but-not-exact matches (e.g.
        // searching "TCS" could plausibly also surface a similarly-named
        // instrument); an exact match is worth preferring when present.
        let results = body.data.unwrap_or_default();
        results
            .iter()
            .find(|r| r.trading_symbol.as_deref().map(|s| s.eq_ignore_ascii_case(symbol)).unwrap_or(false))
            .or_else(|| results.first())
            .map(|r| UpstoxSearchResult { instrument_key: r.instrument_key.clone(), isin: r.isin.clone() })
            .ok_or_else(|| MarketDataError::NoData(format!("{symbol}: no match from Upstox instrument search")))
    }

    /// Public so callers needing instrument_key resolution outside a
    /// quote fetch (e.g. resolving symbols before starting a live stream)
    /// get the SAME on-demand search-and-cache behavior, rather than a
    /// second, cache-only lookup that silently requires the old manual
    /// "Refresh Instrument List" step this method no longer needs.
    pub async fn resolve_instrument_key(&self, symbol: &str, exchange: &str) -> Result<String, MarketDataError> {
        let primary = exchange.to_uppercase();
        if let Some(key) = self
            .instruments
            .get_instrument_key(symbol, &primary)
            .await
            .map_err(|e| MarketDataError::RequestFailed(e.to_string()))?
        {
            return Ok(key);
        }
        for fallback in ["NSE", "BSE"] {
            if fallback == primary {
                continue;
            }
            if let Some(key) = self
                .instruments
                .get_instrument_key(symbol, fallback)
                .await
                .map_err(|e| MarketDataError::RequestFailed(e.to_string()))?
            {
                return Ok(key);
            }
        }

        // Cache miss on both exchanges — search on demand and cache the
        // result, rather than requiring a separate manual "refresh" step
        // that depended on the now-unreliable bulk file download.
        // upsert_one, not replace_all — the latter wholesale-replaces the
        // entire cache, which would wipe every other cached instrument
        // just to add this one.
        let result = self.search_instrument(symbol, &primary).await?;
        let _ = self.instruments.upsert_one(symbol, &primary, &result.instrument_key, result.isin.as_deref()).await;
        Ok(result.instrument_key)
    }

    /// Public so main.rs's fundamentals command can resolve an ISIN
    /// without going through fetch_quote/fetch_daily_history_1y.
    pub async fn resolve_isin(&self, symbol: &str, exchange: &str) -> Result<String, MarketDataError> {
        let primary = exchange.to_uppercase();
        if let Some(Some(isin)) = self
            .instruments
            .get_isin(symbol, &primary)
            .await
            .map_err(|e| MarketDataError::RequestFailed(e.to_string()))?
            .map(Some)
        {
            return Ok(isin);
        }
        for fallback in ["NSE", "BSE"] {
            if fallback == primary {
                continue;
            }
            if let Some(Some(isin)) = self
                .instruments
                .get_isin(symbol, fallback)
                .await
                .map_err(|e| MarketDataError::RequestFailed(e.to_string()))?
                .map(Some)
            {
                return Ok(isin);
            }
        }

        // Same on-demand search-and-cache fallback as resolve_instrument_key.
        let result = self.search_instrument(symbol, &primary).await?;
        let isin = result.isin.clone().ok_or_else(|| MarketDataError::NoData(format!("{symbol}: Upstox search returned no ISIN")))?;
        let _ = self.instruments.upsert_one(symbol, &primary, &result.instrument_key, Some(&isin)).await;
        Ok(isin)
    }

    pub fn http_client(&self) -> &Client {
        &self.http
    }
}

/// Real failure this fixes: "invalid gzip header". reqwest's own `gzip`
/// feature (enabled for this exact file) transparently decompresses the
/// response whenever the server sends `Content-Encoding: gzip` —
/// independent of whether the file itself is a .gz file. If Upstox's CDN
/// sets that header (common for object-storage/CDN-served static assets,
/// even ones whose filename already ends in .gz), reqwest hands back
/// already-decompressed JSON bytes, and running those back through
/// GzDecoder fails since they're no longer actually gzip-formatted.
/// Checking the real gzip magic bytes (0x1f 0x8b) up front handles either
/// case correctly instead of assuming one. Extracted as a standalone
/// function so both code paths are directly testable without needing a
/// live HTTP response.
fn decode_instrument_file_bytes(bytes: &[u8], exchange: &str) -> Result<String, MarketDataError> {
    if bytes.len() >= 2 && bytes[0] == 0x1f && bytes[1] == 0x8b {
        let mut decoder = GzDecoder::new(bytes);
        let mut text = String::new();
        decoder
            .read_to_string(&mut text)
            .map_err(|e| MarketDataError::UnexpectedResponse(format!("couldn't decompress {exchange} instrument file: {e}")))?;
        Ok(text)
    } else {
        String::from_utf8(bytes.to_vec())
            .map_err(|e| MarketDataError::UnexpectedResponse(format!("{exchange} instrument file wasn't gzip or valid UTF-8 text: {e}")))
    }
}

#[async_trait]
impl MarketDataProvider for UpstoxProvider {
    async fn fetch_quote(&self, symbol: &str, exchange: &str) -> Result<Quote, MarketDataError> {
        let token = self.token().await?;
        let instrument_key = self.resolve_instrument_key(symbol, exchange).await?;
        let url = format!("https://api.upstox.com/v2/market-quote/quotes?instrument_key={instrument_key}");

        let response = self
            .http
            .get(&url)
            .header("Authorization", format!("Bearer {token}"))
            .header("Accept", "application/json")
            .send()
            .await
            .map_err(|e| MarketDataError::RequestFailed(e.to_string()))?;

        let body: QuoteResponse = response
            .json()
            .await
            .map_err(|e| MarketDataError::UnexpectedResponse(format!("couldn't parse Upstox quote for {symbol}: {e}")))?;

        let quote = body
            .data
            .into_values()
            .next()
            .ok_or_else(|| MarketDataError::NoData(format!("{symbol}: empty response — check token or symbol")))?;

        Ok(Quote {
            price: Decimal::from_str(&quote.last_price.to_string())
                .map_err(|_| MarketDataError::UnexpectedResponse(format!("bad price for {symbol}")))?,
            day_high: quote.ohlc.as_ref().map(|o| o.high).and_then(|v| Decimal::from_str(&v.to_string()).ok()),
            day_low: quote.ohlc.as_ref().map(|o| o.low).and_then(|v| Decimal::from_str(&v.to_string()).ok()),
            week52_high: None,
            week52_low: None,
            volume: Some(quote.volume as u64),
            previous_close: quote.ohlc.as_ref().and_then(|o| o.close).and_then(|v| Decimal::from_str(&v.to_string()).ok()),
        })
    }

    async fn fetch_daily_history_1y(&self, symbol: &str, exchange: &str) -> Result<Vec<DailyBar>, MarketDataError> {
        let token = self.token().await?;
        let instrument_key = self.resolve_instrument_key(symbol, exchange).await?;
        let to_date = chrono::Utc::now().date_naive();
        let from_date = to_date - chrono::Duration::days(366);
        let url = format!(
            "https://api.upstox.com/v2/historical-candle/{instrument_key}/day/{to_date}/{from_date}",
            to_date = to_date.format("%Y-%m-%d"),
            from_date = from_date.format("%Y-%m-%d"),
        );

        let response = self
            .http
            .get(&url)
            .header("Authorization", format!("Bearer {token}"))
            .header("Accept", "application/json")
            .send()
            .await
            .map_err(|e| MarketDataError::RequestFailed(e.to_string()))?;

        let body: HistoricalCandleResponse = response
            .json()
            .await
            .map_err(|e| MarketDataError::UnexpectedResponse(format!("couldn't parse Upstox history for {symbol}: {e}")))?;

        let mut bars: Vec<DailyBar> = body
            .data
            .candles
            .into_iter()
            .filter_map(|candle| {
                let timestamp_str = candle.first()?.as_str()?;
                let date = NaiveDate::parse_from_str(&timestamp_str[..10], "%Y-%m-%d").ok()?;
                Some(DailyBar {
                    date,
                    open: candle.get(1)?.as_f64()?,
                    high: candle.get(2)?.as_f64()?,
                    low: candle.get(3)?.as_f64()?,
                    close: candle.get(4)?.as_f64()?,
                    volume: candle.get(5)?.as_f64()?,
                })
            })
            .collect();
        bars.sort_by_key(|b| b.date);
        Ok(bars)
    }
}

struct UpstoxSearchResult {
    instrument_key: String,
    isin: Option<String>,
}

#[derive(Deserialize)]
struct SearchResponse {
    data: Option<Vec<SearchResultRow>>,
}
#[derive(Deserialize)]
struct SearchResultRow {
    #[serde(rename = "trading_symbol")]
    trading_symbol: Option<String>,
    instrument_key: String,
    isin: Option<String>,
}

#[derive(Deserialize)]
struct RawInstrumentRow {
    trading_symbol: Option<String>,
    instrument_key: String,
    instrument_type: Option<String>,
    isin: Option<String>,
}

#[derive(Deserialize)]
struct QuoteResponse {
    data: std::collections::HashMap<String, UpstoxQuote>,
}
#[derive(Deserialize)]
struct UpstoxQuote {
    last_price: f64,
    volume: i64,
    ohlc: Option<Ohlc>,
}
#[derive(Deserialize)]
struct Ohlc {
    high: f64,
    low: f64,
    /// Previous trading day's close — confirmed present in Upstox's own
    /// documented response shape (their /market-quote/quotes endpoint's
    /// ohlc block includes open/high/low/close, same as their /ohlc
    /// endpoint's prev_ohlc block). Was never read before, which is why
    /// Prev Close/Day Chg% showed blank for any symbol without locally-
    /// stored price history yet — this feeds that as a live fallback.
    close: Option<f64>,
}

#[derive(Deserialize)]
struct HistoricalCandleResponse {
    data: CandleData,
}
#[derive(Deserialize)]
struct CandleData {
    candles: Vec<Vec<serde_json::Value>>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn parses_a_real_shaped_search_response() {
        let sample = r#"{"status": "success", "data": [{"trading_symbol": "RELIANCE", "instrument_key": "NSE_EQ|INE002A01018", "isin": "INE002A01018"}], "meta_data": {"page": {"count": 1}}}"#;
        let parsed: SearchResponse = serde_json::from_str(sample).unwrap();
        let rows = parsed.data.unwrap();
        assert_eq!(rows[0].trading_symbol.as_deref(), Some("RELIANCE"));
        assert_eq!(rows[0].instrument_key, "NSE_EQ|INE002A01018");
    }

    #[test]
    fn empty_data_array_parses_to_no_results_not_an_error() {
        let sample = r#"{"status": "success", "data": []}"#;
        let parsed: SearchResponse = serde_json::from_str(sample).unwrap();
        assert_eq!(parsed.data.unwrap().len(), 0);
    }

    #[test]
    fn prefers_exact_trading_symbol_match_over_first_result() {
        let rows = vec![
            SearchResultRow { trading_symbol: Some("RELIANCEPOWER".to_string()), instrument_key: "NSE_EQ|WRONG".to_string(), isin: None },
            SearchResultRow { trading_symbol: Some("RELIANCE".to_string()), instrument_key: "NSE_EQ|CORRECT".to_string(), isin: Some("INE002A01018".to_string()) },
        ];
        let found = rows
            .iter()
            .find(|r| r.trading_symbol.as_deref().map(|s| s.eq_ignore_ascii_case("RELIANCE")).unwrap_or(false))
            .or_else(|| rows.first());
        assert_eq!(found.unwrap().instrument_key, "NSE_EQ|CORRECT");
    }

    #[test]
    fn decodes_real_gzip_bytes_correctly() {
        let original = r#"[{"trading_symbol": "RELIANCE"}]"#;
        let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        encoder.write_all(original.as_bytes()).unwrap();
        let gzipped = encoder.finish().unwrap();

        let decoded = decode_instrument_file_bytes(&gzipped, "NSE").unwrap();
        assert_eq!(decoded, original);
    }

    #[test]
    fn falls_back_to_plain_text_when_bytes_are_not_actually_gzip() {
        // Reproduces the real failure: reqwest's own gzip feature already
        // decompressed the transport encoding, leaving plain JSON bytes
        // that don't start with the gzip magic number.
        let plain_json = r#"[{"trading_symbol": "RELIANCE"}]"#;
        let decoded = decode_instrument_file_bytes(plain_json.as_bytes(), "NSE").unwrap();
        assert_eq!(decoded, plain_json);
    }

    #[test]
    fn neither_gzip_nor_valid_utf8_is_a_clear_error_not_a_panic() {
        let garbage = vec![0xff, 0xfe, 0x00, 0x01];
        let result = decode_instrument_file_bytes(&garbage, "NSE");
        assert!(result.is_err());
    }

    #[test]
    fn empty_bytes_is_treated_as_empty_text_not_a_panic() {
        let decoded = decode_instrument_file_bytes(&[], "NSE").unwrap();
        assert_eq!(decoded, "");
    }

    #[test]
    fn parses_a_real_shaped_quote_response() {
        let sample = r#"{"status": "success", "data": {"NSE_EQ:RELIANCE": {"last_price": 1288.6, "volume": 10836226, "ohlc": {"open": 1290.0, "high": 1304.6, "low": 1280.7, "close": 1270.0}}}}"#;
        let parsed: QuoteResponse = serde_json::from_str(sample).unwrap();
        let quote = parsed.data.into_values().next().unwrap();
        assert_eq!(quote.last_price, 1288.6);
        assert_eq!(quote.ohlc.as_ref().unwrap().high, 1304.6);
        // Real gap this closes: `close` was already present in this exact
        // test's sample JSON before the Ohlc struct had a field for it —
        // serde silently ignores unknown JSON keys, so this test passed
        // the whole time without ever verifying `close` was captured,
        // which is exactly how the missing-Prev-Close bug went unnoticed.
        assert_eq!(quote.ohlc.unwrap().close, Some(1270.0));
    }

    #[test]
    fn parses_a_real_shaped_historical_candle_response() {
        let sample = r#"{"status": "success", "data": {"candles": [["2026-07-24T00:00:00+05:30", 1290.0, 1304.6, 1280.7, 1288.6, 10836226, 0]]}}"#;
        let parsed: HistoricalCandleResponse = serde_json::from_str(sample).unwrap();
        assert_eq!(parsed.data.candles.len(), 1);
        assert_eq!(parsed.data.candles[0][4].as_f64(), Some(1288.6));
    }

    #[test]
    fn instrument_row_carries_isin_alongside_instrument_key() {
        let sample = r#"[{"trading_symbol": "RELIANCE", "instrument_key": "NSE_EQ|INE002A01018", "instrument_type": "EQ", "isin": "INE002A01018"}, {"trading_symbol": "NIFTY25000CE", "instrument_key": "NSE_FO|123", "instrument_type": "CE", "isin": null}]"#;
        let rows: Vec<RawInstrumentRow> = serde_json::from_str(sample).unwrap();
        let equities: Vec<_> = rows.iter().filter(|r| r.instrument_type.as_deref() == Some("EQ")).collect();
        assert_eq!(equities.len(), 1);
        assert_eq!(equities[0].isin.as_deref(), Some("INE002A01018"));
    }
}

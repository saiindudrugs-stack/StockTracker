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

            let mut decoder = GzDecoder::new(&bytes[..]);
            let mut json_text = String::new();
            decoder
                .read_to_string(&mut json_text)
                .map_err(|e| MarketDataError::UnexpectedResponse(format!("couldn't decompress {exchange} instrument file: {e}")))?;

            let rows: Vec<RawInstrumentRow> = serde_json::from_str(&json_text)
                .map_err(|e| MarketDataError::UnexpectedResponse(format!("couldn't parse {exchange} instrument file: {e}")))?;

            for row in rows {
                if row.instrument_type.as_deref() != Some("EQ") {
                    continue;
                }
                let Some(symbol) = row.trading_symbol else { continue };
                all_rows.push(UpstoxInstrumentRow {
                    trading_symbol: symbol,
                    exchange: exchange.to_string(),
                    instrument_key: row.instrument_key,
                    isin: row.isin,
                });
            }
        }

        let count = all_rows.len();
        self.instruments.replace_all(all_rows).await.map_err(|e| MarketDataError::RequestFailed(e.to_string()))?;
        Ok(count)
    }

    async fn resolve_instrument_key(&self, symbol: &str, exchange: &str) -> Result<String, MarketDataError> {
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
        Err(MarketDataError::NoData(format!(
            "{symbol} not found in Upstox instrument cache — try Refresh Instrument List in Settings first"
        )))
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
        Err(MarketDataError::NoData(format!("{symbol}: no ISIN in Upstox instrument cache")))
    }

    pub fn http_client(&self) -> &Client {
        &self.http
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

    #[test]
    fn parses_a_real_shaped_quote_response() {
        let sample = r#"{"status": "success", "data": {"NSE_EQ:RELIANCE": {"last_price": 1288.6, "volume": 10836226, "ohlc": {"open": 1290.0, "high": 1304.6, "low": 1280.7, "close": 1270.0}}}}"#;
        let parsed: QuoteResponse = serde_json::from_str(sample).unwrap();
        let quote = parsed.data.into_values().next().unwrap();
        assert_eq!(quote.last_price, 1288.6);
        assert_eq!(quote.ohlc.unwrap().high, 1304.6);
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

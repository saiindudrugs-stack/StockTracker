//! Live market data — separate from the Broker Adapter Framework
//! (`brokers/`) deliberately: fetching a public quote needs no
//! authentication and no broker account, so it's its own trait rather than
//! bolted onto `BrokerAdapter`.
//!
//! IMPORTANT — read before relying on this in anything beyond casual
//! personal use: `YahooFinanceProvider` calls an **unofficial, undocumented**
//! Yahoo Finance endpoint. Yahoo doesn't publish or support it — it can
//! change shape, get rate-limited, or disappear without notice, and this
//! was chosen specifically because it's free and needs no API key, not
//! because it's reliable. The user explicitly chose this over a paid
//! Zerodha Kite Connect subscription, trading reliability for zero cost —
//! that trade-off is deliberate, not hidden.

pub mod yahoo_finance;
pub mod yahoo_fundamentals_news;
pub mod alpha_vantage;
pub mod amfi;
pub mod composite;

use async_trait::async_trait;
use pm_domain::analytics::DailyBar;
use rust_decimal::Decimal;

#[derive(Debug, thiserror::Error)]
pub enum MarketDataError {
    #[error("request failed: {0}")]
    RequestFailed(String),
    #[error("unexpected response shape: {0}")]
    UnexpectedResponse(String),
    #[error("no price data returned for symbol {0}")]
    NoData(String),
}

/// A single point-in-time snapshot — everything the Holdings/Watchlist
/// columnar display needs from one request. Fields are `Option` where
/// Yahoo's own meta object can omit them (e.g. a newly-listed instrument
/// might not have a full 52-week range yet) — better to show "—" in the UI
/// than fabricate a number.
#[derive(Debug, Clone)]
pub struct Quote {
    pub price: Decimal,
    pub day_high: Option<Decimal>,
    pub day_low: Option<Decimal>,
    pub week52_high: Option<Decimal>,
    pub week52_low: Option<Decimal>,
    pub volume: Option<u64>,
}

#[async_trait]
pub trait MarketDataProvider: Send + Sync {
    /// `symbol` and `exchange` are the app's own raw values (Instrument.
    /// symbol, Instrument.exchange) — each provider maps these to its own
    /// ticker format internally (Yahoo wants "RELIANCE.NS", Alpha Vantage
    /// wants "NSE:RELIANCE" or "RELIANCE.BSE"). This used to be the
    /// caller's job, with main.rs pre-mapping to Yahoo's format before
    /// calling — that broke down the moment a second provider needed a
    /// *different* format for the same instrument, which is exactly what
    /// adding Alpha Vantage as a fallback requires.
    async fn fetch_quote(&self, symbol: &str, exchange: &str) -> Result<Quote, MarketDataError>;

    /// Up to a year of daily closes + volume, oldest first — the heavier
    /// call, only needed for market-phase classification (SMA-200 needs
    /// 200+ daily bars). Deliberately a separate method from fetch_quote
    /// so callers can choose when to pay for it rather than it being
    /// bundled into every routine price refresh.
    async fn fetch_daily_history_1y(&self, symbol: &str, exchange: &str) -> Result<Vec<DailyBar>, MarketDataError>;
}

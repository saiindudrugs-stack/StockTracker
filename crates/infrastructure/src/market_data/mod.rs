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

use async_trait::async_trait;
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

#[async_trait]
pub trait MarketDataProvider: Send + Sync {
    /// `symbol` here is the *provider's* ticker format (e.g. Yahoo wants
    /// "RELIANCE.NS" for NSE, "RELIANCE.BO" for BSE) — mapping from the
    /// app's own Instrument.symbol + exchange to that format is the
    /// caller's job (see `to_yahoo_symbol` in yahoo_finance.rs), not this
    /// trait's, so a future second provider isn't forced into Yahoo's
    /// suffix convention.
    async fn fetch_latest_price(&self, symbol: &str) -> Result<Decimal, MarketDataError>;
}

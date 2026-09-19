//! Broker Adapter Framework (HLD Section 3.2, SRS 2.2.2): one common trait,
//! one module per broker. The Portfolio Engine and Live Feed Manager depend
//! only on `BrokerAdapter` — adding Upstox/FYERS/etc. per the confirmed
//! rollout order (SRS 2.2.2) means writing a new module that implements this
//! trait, never touching the engine or the domain layer.

pub mod zerodha;

use async_trait::async_trait;
use chrono::NaiveDate;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

#[derive(Debug, thiserror::Error)]
pub enum BrokerError {
    #[error("authentication failed: {0}")]
    AuthFailed(String),
    #[error("broker API request failed: {0}")]
    RequestFailed(String),
    #[error("broker returned an unexpected response shape: {0}")]
    UnexpectedResponse(String),
    #[error("not authenticated — call authenticate() first")]
    NotAuthenticated,
}

/// One row of what a broker's holdings API returns — deliberately broker-
/// agnostic; each adapter is responsible for mapping its own API's response
/// shape into this. Downstream code (sync use-case) never sees Zerodha's
/// JSON directly.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BrokerHolding {
    pub isin: String,
    pub symbol: String,
    pub exchange: String,
    pub quantity: Decimal,
    pub avg_cost: Decimal,
    pub last_traded_price: Decimal,
}

/// One row of a broker's intraday/MIS position — kept distinct from
/// BrokerHolding per SRS 2.2.1 ("intraday positions ... distinct from the
/// long-term holdings view").
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BrokerIntradayPosition {
    pub isin: String,
    pub symbol: String,
    pub exchange: String,
    pub quantity: Decimal,
    pub entry_price: Decimal,
    pub last_traded_price: Decimal,
    pub product_type: String, // e.g. "MIS", "CNC" — broker-specific string, not modeled further in v1
}

#[derive(Debug, Clone)]
pub struct AuthCredentials {
    pub api_key: String,
    /// Broker-specific: Zerodha uses a request_token exchanged for an
    /// access_token; other brokers vary. Kept as an opaque string here so
    /// the trait doesn't leak any one broker's auth flow shape.
    pub secret_or_token: String,
}

/// The common interface every broker module implements (HLD Section 3.2).
/// Credentials themselves are never persisted by an adapter — that's the
/// OS-keychain's job (HLD Section 8); adapters only ever hold a live session
/// token in memory for their own lifetime.
#[async_trait]
pub trait BrokerAdapter: Send + Sync {
    fn broker_code(&self) -> &'static str;

    async fn authenticate(&mut self, credentials: AuthCredentials) -> Result<(), BrokerError>;

    async fn fetch_holdings(&self) -> Result<Vec<BrokerHolding>, BrokerError>;

    async fn fetch_intraday_positions(&self) -> Result<Vec<BrokerIntradayPosition>, BrokerError>;

    /// Historical daily bars for backfill (SRS 2.2.2 "CSV/Excel import
    /// adapter ... for historical backfill" covers the no-API case; this
    /// covers brokers that do expose historical data over their API).
    async fn fetch_historical_daily(
        &self,
        isin: &str,
        from: NaiveDate,
        to: NaiveDate,
    ) -> Result<Vec<(NaiveDate, Decimal)>, BrokerError>;
}

/// Registry so the application layer can look up "the adapter for broker X"
/// without a giant match statement — this is the seam where the configurable
/// broker-priority config file (SRS 2.2.2, Section 10) plugs in: the config
/// says *which* adapters to instantiate and in what order to roll them out,
/// this registry is just the runtime lookup, indifferent to that order.
pub struct BrokerRegistry {
    adapters: std::collections::HashMap<&'static str, std::sync::Arc<tokio::sync::Mutex<dyn BrokerAdapter>>>,
}

impl Default for BrokerRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl BrokerRegistry {
    pub fn new() -> Self {
        Self {
            adapters: std::collections::HashMap::new(),
        }
    }

    pub fn register(&mut self, adapter: std::sync::Arc<tokio::sync::Mutex<dyn BrokerAdapter>>) {
        // Broker code is read synchronously via try_lock since registration
        // happens at startup, well before any broker session is in use.
        let code = adapter
            .try_lock()
            .expect("adapter must not be locked during registration")
            .broker_code();
        self.adapters.insert(code, adapter);
    }

    pub fn get(&self, broker_code: &str) -> Option<std::sync::Arc<tokio::sync::Mutex<dyn BrokerAdapter>>> {
        self.adapters.get(broker_code).cloned()
    }
}

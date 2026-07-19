//! Repository *traits* — defined in the domain layer, implemented in
//! infrastructure (HLD Section 3.1: "Infrastructure ... Implements
//! domain-defined interfaces (Repository Pattern)"). The domain and
//! application layers depend only on these traits, never on rusqlite/duckdb
//! directly, so the storage engine can be swapped without touching business
//! logic — this is the mechanism behind the "Extensibility" NFR.

use crate::entities::{Holding, Instrument, Portfolio, Transaction};
use async_trait::async_trait;
use chrono::NaiveDate;
use rust_decimal::Decimal;
use uuid::Uuid;

#[derive(Debug, thiserror::Error)]
pub enum RepositoryError {
    #[error("storage error: {0}")]
    Storage(String),
    #[error("not found: {0}")]
    NotFound(String),
}

/// Portfolios themselves (HLD Section 5.1 `portfolio` table). Added when the
/// single-user personal tool grew a real second requirement: a family with
/// several individual accounts, each needing its own isolated holdings list
/// — exactly the "may productize later" seam the domain layer was built
/// with from the start (every Transaction/Holding already carries a
/// portfolio_id). This trait is what turns that latent scoping into an
/// actual "create a portfolio, list portfolios, switch between them" feature.
#[async_trait]
pub trait PortfolioRepository: Send + Sync {
    async fn create(&self, portfolio: &Portfolio) -> Result<(), RepositoryError>;
    async fn list_all(&self) -> Result<Vec<Portfolio>, RepositoryError>;
    async fn get(&self, id: Uuid) -> Result<Portfolio, RepositoryError>;
}

/// The append-only transaction ledger (SQLite+SQLCipher in production, per
/// HLD Section 5.1).
#[async_trait]
pub trait TransactionRepository: Send + Sync {
    async fn record(&self, txn: &Transaction) -> Result<(), RepositoryError>;
    async fn list_for_portfolio(
        &self,
        portfolio_id: Uuid,
    ) -> Result<Vec<Transaction>, RepositoryError>;
    async fn list_for_instrument(
        &self,
        portfolio_id: Uuid,
        instrument_id: Uuid,
    ) -> Result<Vec<Transaction>, RepositoryError>;
}

/// Reference data — shared across portfolios (HLD Section 5.1 `instrument`).
#[async_trait]
pub trait InstrumentRepository: Send + Sync {
    async fn upsert(&self, instrument: &Instrument) -> Result<(), RepositoryError>;
    async fn get(&self, id: Uuid) -> Result<Instrument, RepositoryError>;
    async fn find_by_isin(&self, isin: &str) -> Result<Option<Instrument>, RepositoryError>;
    /// Every instrument in the shared reference-data table. Added once
    /// users could add their own tickers (rather than only the two demo
    /// instruments) — at that point "list everything" stopped being an
    /// unused trait method and became the only way to populate a ticker
    /// picker.
    async fn list_all(&self) -> Result<Vec<Instrument>, RepositoryError>;
}

/// Derived, rebuildable holding cache (HLD Section 5.1 `holding_snapshot`).
/// This is NOT the source of truth — it's a read-optimized cache the
/// Portfolio Engine rebuilds by folding TransactionRepository entries through
/// Holding::apply. Implementations may cache aggressively since it's always
/// re-derivable.
#[async_trait]
pub trait HoldingRepository: Send + Sync {
    async fn upsert_snapshot(&self, holding: &Holding, as_of: NaiveDate) -> Result<(), RepositoryError>;
    async fn get_snapshot(
        &self,
        portfolio_id: Uuid,
        instrument_id: Uuid,
    ) -> Result<Option<Holding>, RepositoryError>;
    async fn list_for_portfolio(&self, portfolio_id: Uuid) -> Result<Vec<Holding>, RepositoryError>;
}

/// Analytical time-series store — DuckDB in production (HLD Section 5.2).
/// Deliberately separate from the transactional repositories above: this
/// data is public market data, unencrypted, and rebuilt/backfilled
/// independently of the ledger.
#[async_trait]
pub trait PriceRepository: Send + Sync {
    async fn upsert_daily_bar(
        &self,
        instrument_id: Uuid,
        date: NaiveDate,
        close: Decimal,
    ) -> Result<(), RepositoryError>;
    async fn latest_price(&self, instrument_id: Uuid) -> Result<Option<Decimal>, RepositoryError>;
    async fn daily_series(
        &self,
        instrument_id: Uuid,
        from: NaiveDate,
        to: NaiveDate,
    ) -> Result<Vec<(NaiveDate, Decimal)>, RepositoryError>;
}

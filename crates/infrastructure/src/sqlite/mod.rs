//! Connection + schema for the transactional store (HLD Section 5.1).
//!
//! SIMPLIFICATION NOTE: this uses plain rusqlite (no SQLCipher encryption)
//! for this first working slice. The HLD calls for SQLCipher at rest
//! (Section 4, 8) — wiring that in is a matter of swapping the `bundled`
//! rusqlite feature for `bundled-sqlcipher` and threading an encryption key
//! through `open_encrypted()` below; nothing above this module (use-cases,
//! domain) needs to change, since they only ever see the repository traits.
//! Called out explicitly rather than silently shipped unencrypted.

use rusqlite::Connection;
use std::sync::{Arc, Mutex};

pub mod holding_repository;
pub mod instrument_repository;
pub mod price_repository;
pub mod transaction_repository;

pub use holding_repository::SqliteHoldingRepository;
pub use instrument_repository::SqliteInstrumentRepository;
pub use price_repository::SqlitePriceRepository;
pub use transaction_repository::SqliteTransactionRepository;

/// Shared handle to the connection. `rusqlite::Connection` isn't `Sync`, so
/// every repository wraps blocking calls in `spawn_blocking` against a clone
/// of this `Arc<Mutex<..>>` — see `with_conn` below.
#[derive(Clone)]
pub struct SqlitePool(pub Arc<Mutex<Connection>>);

impl SqlitePool {
    pub fn open(path: &str) -> rusqlite::Result<Self> {
        let conn = Connection::open(path)?;
        conn.pragma_update(None, "journal_mode", "WAL")?; // crash-safety, HLD Section 8
        conn.pragma_update(None, "foreign_keys", "ON")?;
        let pool = Self(Arc::new(Mutex::new(conn)));
        pool.run_migrations()?;
        Ok(pool)
    }

    pub fn open_in_memory() -> rusqlite::Result<Self> {
        let conn = Connection::open_in_memory()?;
        let pool = Self(Arc::new(Mutex::new(conn)));
        pool.run_migrations()?;
        Ok(pool)
    }

    fn run_migrations(&self) -> rusqlite::Result<()> {
        let conn = self.0.lock().expect("sqlite mutex poisoned");
        conn.execute_batch(SCHEMA)
    }

    /// Runs a blocking rusqlite closure on the blocking thread pool so it
    /// never stalls the async runtime. This is the one seam every
    /// repository method in this module goes through.
    pub async fn with_conn<T, F>(&self, f: F) -> Result<T, pm_domain::repositories::RepositoryError>
    where
        F: FnOnce(&Connection) -> rusqlite::Result<T> + Send + 'static,
        T: Send + 'static,
    {
        let pool = self.0.clone();
        tokio::task::spawn_blocking(move || {
            let conn = pool.lock().expect("sqlite mutex poisoned");
            f(&conn)
        })
        .await
        .map_err(|e| pm_domain::repositories::RepositoryError::Storage(format!("task join error: {e}")))?
        .map_err(|e| pm_domain::repositories::RepositoryError::Storage(e.to_string()))
    }
}

/// Schema per HLD Section 5.1. `price_history` is included here too as the
/// SqlitePriceRepository's simplification note explains — DuckDB per the
/// HLD is a drop-in swap behind the same `PriceRepository` trait.
const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS instrument (
    id TEXT PRIMARY KEY,
    isin TEXT NOT NULL UNIQUE,
    symbol TEXT NOT NULL,
    asset_class TEXT NOT NULL,
    exchange TEXT NOT NULL,
    sector TEXT
);

CREATE TABLE IF NOT EXISTS "transaction" (
    id TEXT PRIMARY KEY,
    portfolio_id TEXT NOT NULL,
    instrument_id TEXT NOT NULL,
    transaction_type TEXT NOT NULL,
    quantity TEXT NOT NULL,
    price TEXT NOT NULL,
    fees TEXT NOT NULL,
    trade_date TEXT NOT NULL,
    broker_ref TEXT,
    recorded_at TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_txn_portfolio ON "transaction"(portfolio_id);
CREATE INDEX IF NOT EXISTS idx_txn_portfolio_instrument ON "transaction"(portfolio_id, instrument_id);

CREATE TABLE IF NOT EXISTS holding_snapshot (
    portfolio_id TEXT NOT NULL,
    instrument_id TEXT NOT NULL,
    as_of_date TEXT NOT NULL,
    quantity TEXT NOT NULL,
    avg_cost TEXT NOT NULL,
    realized_pnl TEXT NOT NULL,
    PRIMARY KEY (portfolio_id, instrument_id)
);

CREATE TABLE IF NOT EXISTS price_history (
    instrument_id TEXT NOT NULL,
    date TEXT NOT NULL,
    close TEXT NOT NULL,
    PRIMARY KEY (instrument_id, date)
);
"#;

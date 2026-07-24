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

pub mod alert_rule_repository;
pub mod holding_repository;
pub mod instrument_repository;
pub mod portfolio_repository;
pub mod price_repository;
pub mod transaction_repository;

pub use alert_rule_repository::SqliteAlertRuleRepository;
pub use holding_repository::SqliteHoldingRepository;
pub use instrument_repository::SqliteInstrumentRepository;
pub use portfolio_repository::SqlitePortfolioRepository;
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
        conn.execute_batch(SCHEMA)?;

        // CREATE TABLE IF NOT EXISTS only helps fresh databases — an
        // existing price_history table from before OHLC/volume were added
        // needs these columns bolted on explicitly. SQLite's ALTER TABLE
        // has no "ADD COLUMN IF NOT EXISTS", so each is wrapped to ignore
        // the specific "duplicate column name" error rather than swallow
        // every possible failure, so a genuinely different problem still
        // surfaces.
        for stmt in [
            "ALTER TABLE price_history ADD COLUMN open TEXT",
            "ALTER TABLE price_history ADD COLUMN high TEXT",
            "ALTER TABLE price_history ADD COLUMN low TEXT",
            "ALTER TABLE price_history ADD COLUMN volume INTEGER",
        ] {
            if let Err(e) = conn.execute(stmt, []) {
                let already_exists = matches!(&e, rusqlite::Error::SqliteFailure(_, Some(msg)) if msg.contains("duplicate column name"));
                if !already_exists {
                    return Err(e);
                }
            }
        }
        Ok(())
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

    /// Deletes every row from every table — used for the Settings screen's
    /// "Reset All Data" action. Deliberately does NOT drop/recreate tables
    /// (schema stays intact, just empty) so the app doesn't need to
    /// re-run migrations afterward. This exists because reinstalling the
    /// app does NOT clear this database — app data directories
    /// (~/Library/Application Support/... on Mac, %APPDATA%\... on
    /// Windows) persist independently of the installed application, which
    /// is standard OS behavior, not a bug — uninstalling/reinstalling an
    /// app is not expected to silently delete a user's data.
    pub async fn reset_all(&self) -> Result<(), pm_domain::repositories::RepositoryError> {
        self.with_conn(|conn| {
            conn.execute_batch(
                r#"
                DELETE FROM "transaction";
                DELETE FROM holding_snapshot;
                DELETE FROM price_history;
                DELETE FROM alert_rule;
                DELETE FROM instrument;
                DELETE FROM portfolio;
                "#,
            )
        })
        .await
    }
}

/// Schema per HLD Section 5.1. `price_history` is included here too as the
/// SqlitePriceRepository's simplification note explains — DuckDB per the
/// HLD is a drop-in swap behind the same `PriceRepository` trait.
const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS portfolio (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    base_currency TEXT NOT NULL DEFAULT 'inr',
    goal_tag TEXT,
    created_at TEXT NOT NULL DEFAULT (datetime('now'))
);

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
    open TEXT,
    high TEXT,
    low TEXT,
    close TEXT NOT NULL,
    volume INTEGER,
    PRIMARY KEY (instrument_id, date)
);

CREATE TABLE IF NOT EXISTS alert_rule (
    id TEXT PRIMARY KEY,
    portfolio_id TEXT NOT NULL,
    instrument_id TEXT NOT NULL,
    condition TEXT NOT NULL,
    threshold_price TEXT NOT NULL,
    triggered INTEGER NOT NULL DEFAULT 0,
    created_at TEXT NOT NULL DEFAULT (datetime('now'))
);
CREATE INDEX IF NOT EXISTS idx_alert_rule_portfolio ON alert_rule(portfolio_id);
"#;

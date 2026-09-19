//! Disposable Kite instrument master cache — see the schema comment in
//! sqlite/mod.rs. Same lifecycle pattern as SqliteUpstoxInstrumentCache:
//! wholesale-replaced from Kite's own instruments CSV dump, not something
//! this app derives or persists opinions about.

use super::SqlitePool;
use pm_domain::repositories::RepositoryError;
use rusqlite::params;

pub struct SqliteKiteInstrumentCache {
    pool: SqlitePool,
}

impl SqliteKiteInstrumentCache {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    pub async fn replace_all(&self, rows: Vec<(String, String, u32)>) -> Result<(), RepositoryError> {
        self.pool
            .with_conn(move |conn| {
                let tx = conn.unchecked_transaction()?;
                tx.execute("DELETE FROM kite_instrument_cache", [])?;
                {
                    let mut stmt = tx.prepare(
                        "INSERT INTO kite_instrument_cache (trading_symbol, exchange, instrument_token) VALUES (?1, ?2, ?3) \
                         ON CONFLICT(trading_symbol, exchange) DO UPDATE SET instrument_token = excluded.instrument_token",
                    )?;
                    for (symbol, exchange, token) in &rows {
                        stmt.execute(params![symbol, exchange, token])?;
                    }
                }
                tx.commit()?;
                Ok(())
            })
            .await
    }

    pub async fn count(&self) -> Result<i64, RepositoryError> {
        self.pool
            .with_conn(|conn| conn.query_row("SELECT COUNT(*) FROM kite_instrument_cache", [], |row| row.get(0)))
            .await
    }

    pub async fn get_instrument_token(&self, trading_symbol: &str, exchange: &str) -> Result<Option<u32>, RepositoryError> {
        let trading_symbol = trading_symbol.to_uppercase();
        let exchange = exchange.to_uppercase();
        self.pool
            .with_conn(move |conn| {
                conn.query_row(
                    "SELECT instrument_token FROM kite_instrument_cache WHERE trading_symbol = ?1 AND exchange = ?2",
                    params![trading_symbol, exchange],
                    |row| row.get(0),
                )
                .map(Some)
                .or_else(|e| if matches!(e, rusqlite::Error::QueryReturnedNoRows) { Ok(None) } else { Err(e) })
            })
            .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn replace_all_then_lookup_round_trips() {
        let pool = SqlitePool::open_in_memory().unwrap();
        let cache = SqliteKiteInstrumentCache::new(pool);
        cache.replace_all(vec![("INFY".to_string(), "NSE".to_string(), 408065)]).await.unwrap();

        assert_eq!(cache.count().await.unwrap(), 1);
        assert_eq!(cache.get_instrument_token("INFY", "NSE").await.unwrap(), Some(408065));
        assert_eq!(cache.get_instrument_token("infy", "nse").await.unwrap(), Some(408065));
    }

    #[tokio::test]
    async fn replace_all_wholesale_replaces_not_accumulates() {
        let pool = SqlitePool::open_in_memory().unwrap();
        let cache = SqliteKiteInstrumentCache::new(pool);
        cache.replace_all(vec![("OLD".to_string(), "NSE".to_string(), 1)]).await.unwrap();
        cache.replace_all(vec![("NEW".to_string(), "NSE".to_string(), 2)]).await.unwrap();

        assert_eq!(cache.count().await.unwrap(), 1);
        assert!(cache.get_instrument_token("OLD", "NSE").await.unwrap().is_none());
    }

    #[tokio::test]
    async fn unknown_symbol_returns_none() {
        let pool = SqlitePool::open_in_memory().unwrap();
        let cache = SqliteKiteInstrumentCache::new(pool);
        assert!(cache.get_instrument_token("NOPE", "NSE").await.unwrap().is_none());
    }
}

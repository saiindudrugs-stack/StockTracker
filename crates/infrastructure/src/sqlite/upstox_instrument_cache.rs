//! Disposable Upstox instrument master cache — see the schema comment in
//! sqlite/mod.rs. Serves two different lookups from the same cached data:
//! instrument_key (for quotes/historical candles) and ISIN (for the
//! separate Fundamentals API, which doesn't accept instrument_key at all).

use super::SqlitePool;
use pm_domain::repositories::RepositoryError;
use rusqlite::params;

pub struct SqliteUpstoxInstrumentCache {
    pool: SqlitePool,
}

/// One row from Upstox's instrument master file, ready to cache.
pub struct UpstoxInstrumentRow {
    pub trading_symbol: String,
    pub exchange: String,
    pub instrument_key: String,
    pub isin: Option<String>,
}

impl SqliteUpstoxInstrumentCache {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    pub async fn replace_all(&self, rows: Vec<UpstoxInstrumentRow>) -> Result<(), RepositoryError> {
        self.pool
            .with_conn(move |conn| {
                let tx = conn.unchecked_transaction()?;
                tx.execute("DELETE FROM upstox_instrument_cache", [])?;
                {
                    let mut stmt = tx.prepare(
                        "INSERT INTO upstox_instrument_cache (trading_symbol, exchange, instrument_key, isin) \
                         VALUES (?1, ?2, ?3, ?4) \
                         ON CONFLICT(trading_symbol, exchange) DO UPDATE SET instrument_key = excluded.instrument_key, isin = excluded.isin",
                    )?;
                    for row in &rows {
                        stmt.execute(params![row.trading_symbol, row.exchange, row.instrument_key, row.isin])?;
                    }
                }
                tx.commit()?;
                Ok(())
            })
            .await
    }

    pub async fn count(&self) -> Result<i64, RepositoryError> {
        self.pool
            .with_conn(|conn| conn.query_row("SELECT COUNT(*) FROM upstox_instrument_cache", [], |row| row.get(0)))
            .await
    }

    pub async fn get_instrument_key(&self, trading_symbol: &str, exchange: &str) -> Result<Option<String>, RepositoryError> {
        let trading_symbol = trading_symbol.to_uppercase();
        let exchange = exchange.to_uppercase();
        self.pool
            .with_conn(move |conn| {
                conn.query_row(
                    "SELECT instrument_key FROM upstox_instrument_cache WHERE trading_symbol = ?1 AND exchange = ?2",
                    params![trading_symbol, exchange],
                    |row| row.get(0),
                )
                .map(Some)
                .or_else(|e| if matches!(e, rusqlite::Error::QueryReturnedNoRows) { Ok(None) } else { Err(e) })
            })
            .await
    }

    pub async fn get_isin(&self, trading_symbol: &str, exchange: &str) -> Result<Option<String>, RepositoryError> {
        let trading_symbol = trading_symbol.to_uppercase();
        let exchange = exchange.to_uppercase();
        self.pool
            .with_conn(move |conn| {
                conn.query_row(
                    "SELECT isin FROM upstox_instrument_cache WHERE trading_symbol = ?1 AND exchange = ?2",
                    params![trading_symbol, exchange],
                    |row| row.get::<_, Option<String>>(0),
                )
                .or_else(|e| if matches!(e, rusqlite::Error::QueryReturnedNoRows) { Ok(None) } else { Err(e) })
            })
            .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(symbol: &str, exchange: &str, key: &str, isin: Option<&str>) -> UpstoxInstrumentRow {
        UpstoxInstrumentRow {
            trading_symbol: symbol.to_string(),
            exchange: exchange.to_string(),
            instrument_key: key.to_string(),
            isin: isin.map(|s| s.to_string()),
        }
    }

    #[tokio::test]
    async fn replace_all_then_lookup_round_trips_both_fields() {
        let pool = SqlitePool::open_in_memory().unwrap();
        let cache = SqliteUpstoxInstrumentCache::new(pool);
        cache
            .replace_all(vec![row("RELIANCE", "NSE", "NSE_EQ|INE002A01018", Some("INE002A01018"))])
            .await
            .unwrap();

        assert_eq!(cache.count().await.unwrap(), 1);
        assert_eq!(cache.get_instrument_key("RELIANCE", "NSE").await.unwrap(), Some("NSE_EQ|INE002A01018".to_string()));
        assert_eq!(cache.get_isin("RELIANCE", "NSE").await.unwrap(), Some("INE002A01018".to_string()));
        assert_eq!(cache.get_instrument_key("reliance", "nse").await.unwrap(), Some("NSE_EQ|INE002A01018".to_string()));
    }

    #[tokio::test]
    async fn replace_all_wholesale_replaces_not_accumulates() {
        let pool = SqlitePool::open_in_memory().unwrap();
        let cache = SqliteUpstoxInstrumentCache::new(pool);
        cache.replace_all(vec![row("OLD", "NSE", "NSE_EQ|OLD", None)]).await.unwrap();
        cache.replace_all(vec![row("NEW", "NSE", "NSE_EQ|NEW", None)]).await.unwrap();

        assert_eq!(cache.count().await.unwrap(), 1);
        assert!(cache.get_instrument_key("OLD", "NSE").await.unwrap().is_none());
    }

    #[tokio::test]
    async fn unknown_symbol_returns_none_for_both_lookups() {
        let pool = SqlitePool::open_in_memory().unwrap();
        let cache = SqliteUpstoxInstrumentCache::new(pool);
        assert!(cache.get_instrument_key("NOPE", "NSE").await.unwrap().is_none());
        assert!(cache.get_isin("NOPE", "NSE").await.unwrap().is_none());
    }

    #[tokio::test]
    async fn missing_isin_is_stored_as_null_not_an_error() {
        let pool = SqlitePool::open_in_memory().unwrap();
        let cache = SqliteUpstoxInstrumentCache::new(pool);
        cache.replace_all(vec![row("XYZ", "NSE", "NSE_EQ|XYZ", None)]).await.unwrap();
        assert_eq!(cache.get_instrument_key("XYZ", "NSE").await.unwrap(), Some("NSE_EQ|XYZ".to_string()));
        assert_eq!(cache.get_isin("XYZ", "NSE").await.unwrap(), None);
    }

    #[tokio::test]
    async fn same_symbol_different_exchange_are_distinct_rows() {
        let pool = SqlitePool::open_in_memory().unwrap();
        let cache = SqliteUpstoxInstrumentCache::new(pool);
        cache
            .replace_all(vec![
                row("RELIANCE", "NSE", "NSE_EQ|A", Some("INE002A01018")),
                row("RELIANCE", "BSE", "BSE_EQ|B", Some("INE002A01018")),
            ])
            .await
            .unwrap();
        assert_eq!(cache.get_instrument_key("RELIANCE", "NSE").await.unwrap(), Some("NSE_EQ|A".to_string()));
        assert_eq!(cache.get_instrument_key("RELIANCE", "BSE").await.unwrap(), Some("BSE_EQ|B".to_string()));
    }
}

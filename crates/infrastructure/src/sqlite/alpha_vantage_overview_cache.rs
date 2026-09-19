//! Real TTL cache, not the disposable wholesale-replace pattern the
//! instrument caches use — see the schema comment in sqlite/mod.rs for
//! why: Alpha Vantage's 25-requests/day budget means this data must be
//! served stale (up to an hour old) rather than fetched fresh every time
//! the News screen loads.

use super::SqlitePool;
use pm_domain::repositories::RepositoryError;
use rusqlite::params;

pub struct SqliteAlphaVantageOverviewCache {
    pool: SqlitePool,
}

#[derive(Debug, Clone)]
pub struct CachedOverview {
    pub market_cap: Option<f64>,
    pub dividend_yield: Option<f64>,
    pub fetched_at_unix: i64,
}

impl SqliteAlphaVantageOverviewCache {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    pub async fn get(&self, symbol: &str) -> Result<Option<CachedOverview>, RepositoryError> {
        let symbol = symbol.to_uppercase();
        self.pool
            .with_conn(move |conn| {
                conn.query_row(
                    "SELECT market_cap, dividend_yield, fetched_at FROM alpha_vantage_overview_cache WHERE symbol = ?1",
                    params![symbol],
                    |row| {
                        let market_cap: Option<String> = row.get(0)?;
                        let dividend_yield: Option<String> = row.get(1)?;
                        let fetched_at_unix: i64 = row.get(2)?;
                        Ok(CachedOverview {
                            market_cap: market_cap.and_then(|s| s.parse().ok()),
                            dividend_yield: dividend_yield.and_then(|s| s.parse().ok()),
                            fetched_at_unix,
                        })
                    },
                )
                .map(Some)
                .or_else(|e| if matches!(e, rusqlite::Error::QueryReturnedNoRows) { Ok(None) } else { Err(e) })
            })
            .await
    }

    pub async fn upsert(&self, symbol: &str, market_cap: Option<f64>, dividend_yield: Option<f64>, fetched_at_unix: i64) -> Result<(), RepositoryError> {
        let symbol = symbol.to_uppercase();
        let market_cap = market_cap.map(|v| v.to_string());
        let dividend_yield = dividend_yield.map(|v| v.to_string());
        self.pool
            .with_conn(move |conn| {
                conn.execute(
                    "INSERT INTO alpha_vantage_overview_cache (symbol, market_cap, dividend_yield, fetched_at) \
                     VALUES (?1, ?2, ?3, ?4) \
                     ON CONFLICT(symbol) DO UPDATE SET market_cap = excluded.market_cap, dividend_yield = excluded.dividend_yield, fetched_at = excluded.fetched_at",
                    params![symbol, market_cap, dividend_yield, fetched_at_unix],
                )?;
                Ok(())
            })
            .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn upsert_then_get_round_trips() {
        let pool = SqlitePool::open_in_memory().unwrap();
        let cache = SqliteAlphaVantageOverviewCache::new(pool);
        cache.upsert("RELIANCE", Some(1.5e12), Some(0.045), 1_700_000_000).await.unwrap();

        let cached = cache.get("RELIANCE").await.unwrap().unwrap();
        assert_eq!(cached.market_cap, Some(1.5e12));
        assert_eq!(cached.dividend_yield, Some(0.045));
        assert_eq!(cached.fetched_at_unix, 1_700_000_000);
    }

    #[tokio::test]
    async fn get_is_case_insensitive_on_symbol() {
        let pool = SqlitePool::open_in_memory().unwrap();
        let cache = SqliteAlphaVantageOverviewCache::new(pool);
        cache.upsert("RELIANCE", Some(1.0), None, 100).await.unwrap();
        assert!(cache.get("reliance").await.unwrap().is_some());
    }

    #[tokio::test]
    async fn upsert_updates_in_place_not_accumulating_rows() {
        let pool = SqlitePool::open_in_memory().unwrap();
        let cache = SqliteAlphaVantageOverviewCache::new(pool);
        cache.upsert("RELIANCE", Some(1.0), None, 100).await.unwrap();
        cache.upsert("RELIANCE", Some(2.0), Some(0.05), 200).await.unwrap();

        let cached = cache.get("RELIANCE").await.unwrap().unwrap();
        assert_eq!(cached.market_cap, Some(2.0));
        assert_eq!(cached.dividend_yield, Some(0.05));
        assert_eq!(cached.fetched_at_unix, 200);
    }

    #[tokio::test]
    async fn unknown_symbol_returns_none() {
        let pool = SqlitePool::open_in_memory().unwrap();
        let cache = SqliteAlphaVantageOverviewCache::new(pool);
        assert!(cache.get("NOPE").await.unwrap().is_none());
    }

    #[tokio::test]
    async fn missing_dividend_yield_stores_as_null_not_an_error() {
        let pool = SqlitePool::open_in_memory().unwrap();
        let cache = SqliteAlphaVantageOverviewCache::new(pool);
        cache.upsert("XYZ", Some(1.0), None, 100).await.unwrap();
        let cached = cache.get("XYZ").await.unwrap().unwrap();
        assert_eq!(cached.dividend_yield, None);
    }
}

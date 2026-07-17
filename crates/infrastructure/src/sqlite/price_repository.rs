//! SIMPLIFICATION NOTE (see also sqlite/mod.rs): the confirmed HLD (Section
//! 5.2) puts price_history in DuckDB, not SQLite — columnar storage genuinely
//! matters once intraday_bar data is flowing (Section 5.2 retention note:
//! 60 trading days of 1-minute bars). This SqlitePriceRepository exists so
//! the Portfolio Engine / Live Feed Manager slice has a real, working
//! `PriceRepository` to compile and test against *today*. Swapping it for a
//! DuckDB-backed implementation later touches only this file — the trait
//! (pm_domain::repositories::PriceRepository) and every use-case that
//! depends on it stay exactly as they are, since that's the entire point of
//! defining the trait in the domain layer (HLD Section 3.1).

use super::SqlitePool;
use async_trait::async_trait;
use chrono::NaiveDate;
use pm_domain::repositories::{PriceRepository, RepositoryError};
use rust_decimal::Decimal;
use rusqlite::params;
use std::str::FromStr;
use uuid::Uuid;

pub struct SqlitePriceRepository {
    pool: SqlitePool,
}

impl SqlitePriceRepository {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl PriceRepository for SqlitePriceRepository {
    async fn upsert_daily_bar(&self, instrument_id: Uuid, date: NaiveDate, close: Decimal) -> Result<(), RepositoryError> {
        self.pool
            .with_conn(move |conn| {
                conn.execute(
                    r#"INSERT INTO price_history (instrument_id, date, close) VALUES (?1, ?2, ?3)
                       ON CONFLICT(instrument_id, date) DO UPDATE SET close = excluded.close"#,
                    params![instrument_id.to_string(), date.format("%Y-%m-%d").to_string(), close.to_string()],
                )?;
                Ok(())
            })
            .await
    }

    async fn latest_price(&self, instrument_id: Uuid) -> Result<Option<Decimal>, RepositoryError> {
        let raw: Option<String> = self
            .pool
            .with_conn(move |conn| {
                conn.query_row(
                    "SELECT close FROM price_history WHERE instrument_id = ?1 ORDER BY date DESC LIMIT 1",
                    params![instrument_id.to_string()],
                    |row| row.get(0),
                )
                .map(Some)
                .or_else(|e| if matches!(e, rusqlite::Error::QueryReturnedNoRows) { Ok(None) } else { Err(e) })
            })
            .await?;
        raw.map(|s| Decimal::from_str(&s).map_err(|e| RepositoryError::Storage(format!("corrupt price in DB: {e}"))))
            .transpose()
    }

    async fn daily_series(&self, instrument_id: Uuid, from: NaiveDate, to: NaiveDate) -> Result<Vec<(NaiveDate, Decimal)>, RepositoryError> {
        let rows: Vec<(String, String)> = self
            .pool
            .with_conn(move |conn| {
                let mut stmt = conn.prepare(
                    "SELECT date, close FROM price_history WHERE instrument_id = ?1 AND date BETWEEN ?2 AND ?3 ORDER BY date ASC",
                )?;
                let rows = stmt
                    .query_map(
                        params![instrument_id.to_string(), from.format("%Y-%m-%d").to_string(), to.format("%Y-%m-%d").to_string()],
                        |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
                    )?
                    .collect::<rusqlite::Result<Vec<_>>>()?;
                Ok(rows)
            })
            .await?;

        rows.into_iter()
            .map(|(d, c)| {
                let date = NaiveDate::parse_from_str(&d, "%Y-%m-%d")
                    .map_err(|e| RepositoryError::Storage(format!("corrupt date in DB: {e}")))?;
                let close = Decimal::from_str(&c).map_err(|e| RepositoryError::Storage(format!("corrupt price in DB: {e}")))?;
                Ok((date, close))
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    #[tokio::test]
    async fn latest_price_returns_most_recent_bar() {
        let pool = SqlitePool::open_in_memory().unwrap();
        let repo = SqlitePriceRepository::new(pool);
        let instrument_id = Uuid::new_v4();

        repo.upsert_daily_bar(instrument_id, NaiveDate::from_ymd_opt(2026, 1, 1).unwrap(), dec!(100)).await.unwrap();
        repo.upsert_daily_bar(instrument_id, NaiveDate::from_ymd_opt(2026, 1, 2).unwrap(), dec!(105)).await.unwrap();

        assert_eq!(repo.latest_price(instrument_id).await.unwrap(), Some(dec!(105)));
    }

    #[tokio::test]
    async fn daily_series_respects_date_range() {
        let pool = SqlitePool::open_in_memory().unwrap();
        let repo = SqlitePriceRepository::new(pool);
        let instrument_id = Uuid::new_v4();

        for (day, price) in [(1, 100), (2, 102), (3, 98), (4, 110)] {
            repo.upsert_daily_bar(instrument_id, NaiveDate::from_ymd_opt(2026, 1, day).unwrap(), Decimal::from(price)).await.unwrap();
        }

        let series = repo
            .daily_series(instrument_id, NaiveDate::from_ymd_opt(2026, 1, 2).unwrap(), NaiveDate::from_ymd_opt(2026, 1, 3).unwrap())
            .await
            .unwrap();
        assert_eq!(series.len(), 2);
        assert_eq!(series[0].1, dec!(102));
        assert_eq!(series[1].1, dec!(98));
    }
}

use super::SqlitePool;
use async_trait::async_trait;
use chrono::NaiveDate;
use pm_domain::entities::Holding;
use pm_domain::repositories::{HoldingRepository, RepositoryError};
use rust_decimal::Decimal;
use rusqlite::params;
use std::str::FromStr;
use uuid::Uuid;

pub struct SqliteHoldingRepository {
    pool: SqlitePool,
}

impl SqliteHoldingRepository {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }
}

type HoldingRow = (String, String, String, String, String);

fn row_to_holding_parts(row: &rusqlite::Row) -> rusqlite::Result<HoldingRow> {
    Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?))
}

fn parse_holding(row: HoldingRow) -> Result<Holding, RepositoryError> {
    let (portfolio_id, instrument_id, quantity, avg_cost, realized_pnl) = row;
    let parse_err = |ctx: &str, e: String| RepositoryError::Storage(format!("corrupt {ctx} in DB: {e}"));
    Ok(Holding {
        portfolio_id: Uuid::parse_str(&portfolio_id).map_err(|e| parse_err("portfolio_id", e.to_string()))?,
        instrument_id: Uuid::parse_str(&instrument_id).map_err(|e| parse_err("instrument_id", e.to_string()))?,
        quantity: Decimal::from_str(&quantity).map_err(|e| parse_err("quantity", e.to_string()))?,
        avg_cost: Decimal::from_str(&avg_cost).map_err(|e| parse_err("avg_cost", e.to_string()))?,
        realized_pnl: Decimal::from_str(&realized_pnl).map_err(|e| parse_err("realized_pnl", e.to_string()))?,
    })
}

#[async_trait]
impl HoldingRepository for SqliteHoldingRepository {
    async fn upsert_snapshot(&self, holding: &Holding, as_of: NaiveDate) -> Result<(), RepositoryError> {
        let holding = holding.clone();
        self.pool
            .with_conn(move |conn| {
                conn.execute(
                    r#"INSERT INTO holding_snapshot (portfolio_id, instrument_id, as_of_date, quantity, avg_cost, realized_pnl)
                       VALUES (?1, ?2, ?3, ?4, ?5, ?6)
                       ON CONFLICT(portfolio_id, instrument_id) DO UPDATE SET
                         as_of_date = excluded.as_of_date, quantity = excluded.quantity,
                         avg_cost = excluded.avg_cost, realized_pnl = excluded.realized_pnl"#,
                    params![
                        holding.portfolio_id.to_string(),
                        holding.instrument_id.to_string(),
                        as_of.format("%Y-%m-%d").to_string(),
                        holding.quantity.to_string(),
                        holding.avg_cost.to_string(),
                        holding.realized_pnl.to_string(),
                    ],
                )?;
                Ok(())
            })
            .await
    }

    async fn get_snapshot(&self, portfolio_id: Uuid, instrument_id: Uuid) -> Result<Option<Holding>, RepositoryError> {
        let row = self
            .pool
            .with_conn(move |conn| {
                conn.query_row(
                    "SELECT portfolio_id, instrument_id, quantity, avg_cost, realized_pnl FROM holding_snapshot WHERE portfolio_id = ?1 AND instrument_id = ?2",
                    params![portfolio_id.to_string(), instrument_id.to_string()],
                    row_to_holding_parts,
                )
                .map(Some)
                .or_else(|e| if matches!(e, rusqlite::Error::QueryReturnedNoRows) { Ok(None) } else { Err(e) })
            })
            .await?;
        row.map(parse_holding).transpose()
    }

    async fn list_for_portfolio(&self, portfolio_id: Uuid) -> Result<Vec<Holding>, RepositoryError> {
        let rows = self
            .pool
            .with_conn(move |conn| {
                let mut stmt = conn.prepare(
                    "SELECT portfolio_id, instrument_id, quantity, avg_cost, realized_pnl FROM holding_snapshot WHERE portfolio_id = ?1",
                )?;
                let rows = stmt
                    .query_map(params![portfolio_id.to_string()], row_to_holding_parts)?
                    .collect::<rusqlite::Result<Vec<_>>>()?;
                Ok(rows)
            })
            .await?;
        rows.into_iter().map(parse_holding).collect()
    }

    async fn delete_snapshot(&self, portfolio_id: Uuid, instrument_id: Uuid) -> Result<(), RepositoryError> {
        self.pool
            .with_conn(move |conn| {
                conn.execute(
                    "DELETE FROM holding_snapshot WHERE portfolio_id = ?1 AND instrument_id = ?2",
                    params![portfolio_id.to_string(), instrument_id.to_string()],
                )?;
                Ok(())
            })
            .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    #[tokio::test]
    async fn upsert_then_get_round_trips() {
        let pool = SqlitePool::open_in_memory().unwrap();
        let repo = SqliteHoldingRepository::new(pool);
        let holding = Holding {
            portfolio_id: Uuid::new_v4(),
            instrument_id: Uuid::new_v4(),
            quantity: dec!(15),
            avg_cost: dec!(102.50),
            realized_pnl: dec!(30),
        };

        repo.upsert_snapshot(&holding, NaiveDate::from_ymd_opt(2026, 1, 1).unwrap()).await.unwrap();
        let fetched = repo.get_snapshot(holding.portfolio_id, holding.instrument_id).await.unwrap();
        assert_eq!(fetched, Some(holding));
    }

    #[tokio::test]
    async fn get_snapshot_returns_none_when_absent() {
        let pool = SqlitePool::open_in_memory().unwrap();
        let repo = SqliteHoldingRepository::new(pool);
        let result = repo.get_snapshot(Uuid::new_v4(), Uuid::new_v4()).await.unwrap();
        assert_eq!(result, None);
    }

    #[tokio::test]
    async fn upsert_overwrites_existing_snapshot() {
        let pool = SqlitePool::open_in_memory().unwrap();
        let repo = SqliteHoldingRepository::new(pool);
        let portfolio_id = Uuid::new_v4();
        let instrument_id = Uuid::new_v4();
        let date = NaiveDate::from_ymd_opt(2026, 1, 1).unwrap();

        repo.upsert_snapshot(&Holding { portfolio_id, instrument_id, quantity: dec!(10), avg_cost: dec!(100), realized_pnl: dec!(0) }, date).await.unwrap();
        repo.upsert_snapshot(&Holding { portfolio_id, instrument_id, quantity: dec!(20), avg_cost: dec!(105), realized_pnl: dec!(50) }, date).await.unwrap();

        let fetched = repo.get_snapshot(portfolio_id, instrument_id).await.unwrap().unwrap();
        assert_eq!(fetched.quantity, dec!(20));
        assert_eq!(fetched.realized_pnl, dec!(50));
    }

    #[tokio::test]
    async fn delete_snapshot_removes_it() {
        let pool = SqlitePool::open_in_memory().unwrap();
        let repo = SqliteHoldingRepository::new(pool);
        let portfolio_id = Uuid::new_v4();
        let instrument_id = Uuid::new_v4();
        let date = NaiveDate::from_ymd_opt(2026, 1, 1).unwrap();

        repo.upsert_snapshot(&Holding { portfolio_id, instrument_id, quantity: dec!(10), avg_cost: dec!(100), realized_pnl: dec!(0) }, date).await.unwrap();
        repo.delete_snapshot(portfolio_id, instrument_id).await.unwrap();

        assert_eq!(repo.get_snapshot(portfolio_id, instrument_id).await.unwrap(), None);
    }
}

use super::SqlitePool;
use async_trait::async_trait;
use chrono::{DateTime, NaiveDate, Utc};
use pm_domain::entities::{Transaction, TransactionType};
use pm_domain::repositories::{RepositoryError, TransactionRepository};
use pm_domain::value_objects::Money;
use rust_decimal::Decimal;
use rusqlite::params;
use std::str::FromStr;
use uuid::Uuid;

pub struct SqliteTransactionRepository {
    pool: SqlitePool,
}

impl SqliteTransactionRepository {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }
}

fn txn_type_to_str(t: TransactionType) -> &'static str {
    match t {
        TransactionType::Buy => "buy",
        TransactionType::Sell => "sell",
        TransactionType::Bonus => "bonus",
        TransactionType::Split => "split",
        TransactionType::Dividend => "dividend",
        TransactionType::SipInstallment => "sip_installment",
    }
}

fn str_to_txn_type(s: &str) -> Result<TransactionType, RepositoryError> {
    Ok(match s {
        "buy" => TransactionType::Buy,
        "sell" => TransactionType::Sell,
        "bonus" => TransactionType::Bonus,
        "split" => TransactionType::Split,
        "dividend" => TransactionType::Dividend,
        "sip_installment" => TransactionType::SipInstallment,
        other => return Err(RepositoryError::Storage(format!("unknown transaction_type '{other}' in DB"))),
    })
}

fn row_to_transaction(row: &rusqlite::Row) -> rusqlite::Result<(String, String, String, String, String, String, String, String, Option<String>, String)> {
    Ok((
        row.get(0)?, // id
        row.get(1)?, // portfolio_id
        row.get(2)?, // instrument_id
        row.get(3)?, // transaction_type
        row.get(4)?, // quantity
        row.get(5)?, // price
        row.get(6)?, // fees
        row.get(7)?, // trade_date
        row.get(8)?, // broker_ref
        row.get(9)?, // recorded_at
    ))
}

fn parse_transaction(
    id: String, portfolio_id: String, instrument_id: String, transaction_type: String,
    quantity: String, price: String, fees: String, trade_date: String,
    broker_ref: Option<String>, recorded_at: String,
) -> Result<Transaction, RepositoryError> {
    let parse_err = |ctx: &str, e: String| RepositoryError::Storage(format!("corrupt {ctx} in DB: {e}"));
    Ok(Transaction {
        id: Uuid::parse_str(&id).map_err(|e| parse_err("id", e.to_string()))?,
        portfolio_id: Uuid::parse_str(&portfolio_id).map_err(|e| parse_err("portfolio_id", e.to_string()))?,
        instrument_id: Uuid::parse_str(&instrument_id).map_err(|e| parse_err("instrument_id", e.to_string()))?,
        transaction_type: str_to_txn_type(&transaction_type)?,
        quantity: Decimal::from_str(&quantity).map_err(|e| parse_err("quantity", e.to_string()))?,
        price: Money::inr(Decimal::from_str(&price).map_err(|e| parse_err("price", e.to_string()))?),
        fees: Money::inr(Decimal::from_str(&fees).map_err(|e| parse_err("fees", e.to_string()))?),
        trade_date: NaiveDate::parse_from_str(&trade_date, "%Y-%m-%d").map_err(|e| parse_err("trade_date", e.to_string()))?,
        broker_ref,
        recorded_at: DateTime::parse_from_rfc3339(&recorded_at)
            .map_err(|e| parse_err("recorded_at", e.to_string()))?
            .with_timezone(&Utc),
    })
}

#[async_trait]
impl TransactionRepository for SqliteTransactionRepository {
    async fn record(&self, txn: &Transaction) -> Result<(), RepositoryError> {
        let txn = txn.clone();
        self.pool
            .with_conn(move |conn| {
                conn.execute(
                    r#"INSERT INTO "transaction"
                        (id, portfolio_id, instrument_id, transaction_type, quantity, price, fees, trade_date, broker_ref, recorded_at)
                       VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)"#,
                    params![
                        txn.id.to_string(),
                        txn.portfolio_id.to_string(),
                        txn.instrument_id.to_string(),
                        txn_type_to_str(txn.transaction_type),
                        txn.quantity.to_string(),
                        txn.price.amount().to_string(),
                        txn.fees.amount().to_string(),
                        txn.trade_date.format("%Y-%m-%d").to_string(),
                        txn.broker_ref,
                        txn.recorded_at.to_rfc3339(),
                    ],
                )?;
                Ok(())
            })
            .await
    }

    async fn list_for_portfolio(&self, portfolio_id: Uuid) -> Result<Vec<Transaction>, RepositoryError> {
        let rows = self
            .pool
            .with_conn(move |conn| {
                let mut stmt = conn.prepare(
                    r#"SELECT id, portfolio_id, instrument_id, transaction_type, quantity, price, fees, trade_date, broker_ref, recorded_at
                       FROM "transaction" WHERE portfolio_id = ?1"#,
                )?;
                let rows = stmt
                    .query_map(params![portfolio_id.to_string()], row_to_transaction)?
                    .collect::<rusqlite::Result<Vec<_>>>()?;
                Ok(rows)
            })
            .await?;

        rows.into_iter()
            .map(|(id, p, i, t, q, pr, f, d, b, r)| parse_transaction(id, p, i, t, q, pr, f, d, b, r))
            .collect()
    }

    async fn list_for_instrument(&self, portfolio_id: Uuid, instrument_id: Uuid) -> Result<Vec<Transaction>, RepositoryError> {
        let rows = self
            .pool
            .with_conn(move |conn| {
                let mut stmt = conn.prepare(
                    r#"SELECT id, portfolio_id, instrument_id, transaction_type, quantity, price, fees, trade_date, broker_ref, recorded_at
                       FROM "transaction" WHERE portfolio_id = ?1 AND instrument_id = ?2"#,
                )?;
                let rows = stmt
                    .query_map(params![portfolio_id.to_string(), instrument_id.to_string()], row_to_transaction)?
                    .collect::<rusqlite::Result<Vec<_>>>()?;
                Ok(rows)
            })
            .await?;

        rows.into_iter()
            .map(|(id, p, i, t, q, pr, f, d, b, r)| parse_transaction(id, p, i, t, q, pr, f, d, b, r))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pm_domain::value_objects::Money;
    use rust_decimal_macros::dec;

    fn sample(portfolio_id: Uuid, instrument_id: Uuid) -> Transaction {
        Transaction {
            id: Uuid::new_v4(),
            portfolio_id,
            instrument_id,
            transaction_type: TransactionType::Buy,
            quantity: dec!(10),
            price: Money::inr(dec!(100.50)),
            fees: Money::inr(dec!(20)),
            trade_date: NaiveDate::from_ymd_opt(2026, 1, 1).unwrap(),
            broker_ref: Some("KITE-123".to_string()),
            recorded_at: Utc::now(),
        }
    }

    #[tokio::test]
    async fn round_trips_a_transaction_through_sqlite() {
        let pool = SqlitePool::open_in_memory().unwrap();
        let repo = SqliteTransactionRepository::new(pool);
        let portfolio_id = Uuid::new_v4();
        let instrument_id = Uuid::new_v4();
        let txn = sample(portfolio_id, instrument_id);

        repo.record(&txn).await.unwrap();
        let fetched = repo.list_for_portfolio(portfolio_id).await.unwrap();

        assert_eq!(fetched.len(), 1);
        assert_eq!(fetched[0].quantity, dec!(10));
        assert_eq!(fetched[0].price.amount(), dec!(100.50));
        assert_eq!(fetched[0].broker_ref.as_deref(), Some("KITE-123"));
    }

    #[tokio::test]
    async fn filters_by_instrument() {
        let pool = SqlitePool::open_in_memory().unwrap();
        let repo = SqliteTransactionRepository::new(pool);
        let portfolio_id = Uuid::new_v4();
        let instrument_a = Uuid::new_v4();
        let instrument_b = Uuid::new_v4();

        repo.record(&sample(portfolio_id, instrument_a)).await.unwrap();
        repo.record(&sample(portfolio_id, instrument_b)).await.unwrap();

        let for_a = repo.list_for_instrument(portfolio_id, instrument_a).await.unwrap();
        assert_eq!(for_a.len(), 1);
        assert_eq!(for_a[0].instrument_id, instrument_a);
    }
}

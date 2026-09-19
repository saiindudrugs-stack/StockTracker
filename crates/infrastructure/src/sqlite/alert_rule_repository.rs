use super::SqlitePool;
use async_trait::async_trait;
use pm_domain::entities::{AlertCondition, AlertRule};
use pm_domain::repositories::{AlertRuleRepository, RepositoryError};
use rust_decimal::Decimal;
use rusqlite::params;
use std::str::FromStr;
use uuid::Uuid;

pub struct SqliteAlertRuleRepository {
    pool: SqlitePool,
}

impl SqliteAlertRuleRepository {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }
}

fn condition_to_str(c: AlertCondition) -> &'static str {
    match c {
        AlertCondition::StopLoss => "stop_loss",
        AlertCondition::Target => "target",
    }
}

fn str_to_condition(s: &str) -> Result<AlertCondition, RepositoryError> {
    match s {
        "stop_loss" => Ok(AlertCondition::StopLoss),
        "target" => Ok(AlertCondition::Target),
        other => Err(RepositoryError::Storage(format!("unknown alert condition '{other}' in DB"))),
    }
}

type AlertRuleRow = (String, String, String, String, String, i64);

fn row_to_parts(row: &rusqlite::Row) -> rusqlite::Result<AlertRuleRow> {
    Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?, row.get(5)?))
}

fn parse_alert_rule(row: AlertRuleRow) -> Result<AlertRule, RepositoryError> {
    let (id, portfolio_id, instrument_id, condition, threshold_price, triggered) = row;
    let parse_err = |ctx: &str, e: String| RepositoryError::Storage(format!("corrupt {ctx} in DB: {e}"));
    Ok(AlertRule {
        id: Uuid::parse_str(&id).map_err(|e| parse_err("id", e.to_string()))?,
        portfolio_id: Uuid::parse_str(&portfolio_id).map_err(|e| parse_err("portfolio_id", e.to_string()))?,
        instrument_id: Uuid::parse_str(&instrument_id).map_err(|e| parse_err("instrument_id", e.to_string()))?,
        condition: str_to_condition(&condition)?,
        threshold_price: Decimal::from_str(&threshold_price).map_err(|e| parse_err("threshold_price", e.to_string()))?,
        triggered: triggered != 0,
    })
}

#[async_trait]
impl AlertRuleRepository for SqliteAlertRuleRepository {
    async fn create(&self, rule: &AlertRule) -> Result<(), RepositoryError> {
        let rule = rule.clone();
        self.pool
            .with_conn(move |conn| {
                conn.execute(
                    "INSERT INTO alert_rule (id, portfolio_id, instrument_id, condition, threshold_price, triggered) \
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                    params![
                        rule.id.to_string(),
                        rule.portfolio_id.to_string(),
                        rule.instrument_id.to_string(),
                        condition_to_str(rule.condition),
                        rule.threshold_price.to_string(),
                        rule.triggered as i64,
                    ],
                )?;
                Ok(())
            })
            .await
    }

    async fn list_for_portfolio(&self, portfolio_id: Uuid) -> Result<Vec<AlertRule>, RepositoryError> {
        let rows = self
            .pool
            .with_conn(move |conn| {
                let mut stmt = conn.prepare(
                    "SELECT id, portfolio_id, instrument_id, condition, threshold_price, triggered \
                     FROM alert_rule WHERE portfolio_id = ?1 ORDER BY created_at ASC",
                )?;
                let rows = stmt
                    .query_map(params![portfolio_id.to_string()], row_to_parts)?
                    .collect::<rusqlite::Result<Vec<_>>>()?;
                Ok(rows)
            })
            .await?;
        rows.into_iter().map(parse_alert_rule).collect()
    }

    async fn mark_triggered(&self, id: Uuid) -> Result<(), RepositoryError> {
        self.pool
            .with_conn(move |conn| {
                conn.execute("UPDATE alert_rule SET triggered = 1 WHERE id = ?1", params![id.to_string()])?;
                Ok(())
            })
            .await
    }

    async fn delete(&self, id: Uuid) -> Result<(), RepositoryError> {
        self.pool
            .with_conn(move |conn| {
                conn.execute("DELETE FROM alert_rule WHERE id = ?1", params![id.to_string()])?;
                Ok(())
            })
            .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample(portfolio_id: Uuid, instrument_id: Uuid) -> AlertRule {
        AlertRule {
            id: Uuid::new_v4(),
            portfolio_id,
            instrument_id,
            condition: AlertCondition::StopLoss,
            threshold_price: Decimal::from_str("100.50").unwrap(),
            triggered: false,
        }
    }

    #[tokio::test]
    async fn create_then_list_round_trips() {
        let pool = SqlitePool::open_in_memory().unwrap();
        let repo = SqliteAlertRuleRepository::new(pool);
        let portfolio_id = Uuid::new_v4();
        let instrument_id = Uuid::new_v4();
        let rule = sample(portfolio_id, instrument_id);

        repo.create(&rule).await.unwrap();
        let list = repo.list_for_portfolio(portfolio_id).await.unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0], rule);
    }

    #[tokio::test]
    async fn mark_triggered_flips_the_flag() {
        let pool = SqlitePool::open_in_memory().unwrap();
        let repo = SqliteAlertRuleRepository::new(pool);
        let portfolio_id = Uuid::new_v4();
        let rule = sample(portfolio_id, Uuid::new_v4());

        repo.create(&rule).await.unwrap();
        repo.mark_triggered(rule.id).await.unwrap();

        let list = repo.list_for_portfolio(portfolio_id).await.unwrap();
        assert!(list[0].triggered);
    }

    #[tokio::test]
    async fn delete_removes_the_rule() {
        let pool = SqlitePool::open_in_memory().unwrap();
        let repo = SqliteAlertRuleRepository::new(pool);
        let portfolio_id = Uuid::new_v4();
        let rule = sample(portfolio_id, Uuid::new_v4());

        repo.create(&rule).await.unwrap();
        repo.delete(rule.id).await.unwrap();

        assert_eq!(repo.list_for_portfolio(portfolio_id).await.unwrap().len(), 0);
    }

    #[tokio::test]
    async fn list_for_portfolio_only_returns_that_portfolios_rules() {
        let pool = SqlitePool::open_in_memory().unwrap();
        let repo = SqliteAlertRuleRepository::new(pool);
        let portfolio_a = Uuid::new_v4();
        let portfolio_b = Uuid::new_v4();

        repo.create(&sample(portfolio_a, Uuid::new_v4())).await.unwrap();
        repo.create(&sample(portfolio_b, Uuid::new_v4())).await.unwrap();

        assert_eq!(repo.list_for_portfolio(portfolio_a).await.unwrap().len(), 1);
        assert_eq!(repo.list_for_portfolio(portfolio_b).await.unwrap().len(), 1);
    }
}

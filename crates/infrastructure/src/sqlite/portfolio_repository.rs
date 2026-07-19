use super::SqlitePool;
use async_trait::async_trait;
use pm_domain::entities::Portfolio;
use pm_domain::repositories::{PortfolioRepository, RepositoryError};
use pm_domain::value_objects::Currency;
use rusqlite::params;
use uuid::Uuid;

pub struct SqlitePortfolioRepository {
    pool: SqlitePool,
}

impl SqlitePortfolioRepository {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }
}

fn currency_to_str(c: Currency) -> &'static str {
    match c {
        Currency::Inr => "inr",
    }
}

fn str_to_currency(s: &str) -> Result<Currency, RepositoryError> {
    match s {
        "inr" => Ok(Currency::Inr),
        other => Err(RepositoryError::Storage(format!("unknown currency '{other}' in DB"))),
    }
}

type PortfolioRow = (String, String, String, Option<String>);

fn row_to_portfolio_parts(row: &rusqlite::Row) -> rusqlite::Result<PortfolioRow> {
    Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))
}

fn parse_portfolio(row: PortfolioRow) -> Result<Portfolio, RepositoryError> {
    let (id, name, base_currency, goal_tag) = row;
    Ok(Portfolio {
        id: Uuid::parse_str(&id).map_err(|e| RepositoryError::Storage(format!("corrupt id in DB: {e}")))?,
        name,
        base_currency: str_to_currency(&base_currency)?,
        goal_tag,
    })
}

#[async_trait]
impl PortfolioRepository for SqlitePortfolioRepository {
    async fn create(&self, portfolio: &Portfolio) -> Result<(), RepositoryError> {
        let portfolio = portfolio.clone();
        self.pool
            .with_conn(move |conn| {
                conn.execute(
                    "INSERT INTO portfolio (id, name, base_currency, goal_tag) VALUES (?1, ?2, ?3, ?4)",
                    params![
                        portfolio.id.to_string(),
                        portfolio.name,
                        currency_to_str(portfolio.base_currency),
                        portfolio.goal_tag,
                    ],
                )?;
                Ok(())
            })
            .await
    }

    async fn list_all(&self) -> Result<Vec<Portfolio>, RepositoryError> {
        let rows = self
            .pool
            .with_conn(|conn| {
                let mut stmt = conn.prepare(
                    "SELECT id, name, base_currency, goal_tag FROM portfolio ORDER BY created_at ASC",
                )?;
                let rows = stmt
                    .query_map([], row_to_portfolio_parts)?
                    .collect::<rusqlite::Result<Vec<_>>>()?;
                Ok(rows)
            })
            .await?;
        rows.into_iter().map(parse_portfolio).collect()
    }

    async fn get(&self, id: Uuid) -> Result<Portfolio, RepositoryError> {
        let row = self
            .pool
            .with_conn(move |conn| {
                conn.query_row(
                    "SELECT id, name, base_currency, goal_tag FROM portfolio WHERE id = ?1",
                    params![id.to_string()],
                    row_to_portfolio_parts,
                )
            })
            .await
            .map_err(|_| RepositoryError::NotFound(format!("portfolio {id}")))?;
        parse_portfolio(row)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample(name: &str) -> Portfolio {
        Portfolio {
            id: Uuid::new_v4(),
            name: name.to_string(),
            base_currency: Currency::Inr,
            goal_tag: None,
        }
    }

    #[tokio::test]
    async fn create_then_get_round_trips() {
        let pool = SqlitePool::open_in_memory().unwrap();
        let repo = SqlitePortfolioRepository::new(pool);
        let portfolio = sample("Dad's Portfolio");

        repo.create(&portfolio).await.unwrap();
        let fetched = repo.get(portfolio.id).await.unwrap();
        assert_eq!(fetched, portfolio);
    }

    #[tokio::test]
    async fn list_all_returns_every_created_portfolio_in_creation_order() {
        let pool = SqlitePool::open_in_memory().unwrap();
        let repo = SqlitePortfolioRepository::new(pool);

        let p1 = sample("Dad");
        let p2 = sample("Mom");
        let p3 = sample("Kid 1");
        repo.create(&p1).await.unwrap();
        repo.create(&p2).await.unwrap();
        repo.create(&p3).await.unwrap();

        let all = repo.list_all().await.unwrap();
        assert_eq!(all.len(), 3);
        assert_eq!(all[0].name, "Dad");
        assert_eq!(all[1].name, "Mom");
        assert_eq!(all[2].name, "Kid 1");
    }

    #[tokio::test]
    async fn get_unknown_portfolio_returns_not_found() {
        let pool = SqlitePool::open_in_memory().unwrap();
        let repo = SqlitePortfolioRepository::new(pool);
        let result = repo.get(Uuid::new_v4()).await;
        assert!(matches!(result, Err(RepositoryError::NotFound(_))));
    }
}

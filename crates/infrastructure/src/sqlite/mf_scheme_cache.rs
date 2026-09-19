//! Disposable AMFI scheme master-list cache — see the schema comment in
//! sqlite/mod.rs. This is NOT behind a domain repository trait, unlike
//! everything else in this crate: nothing else needs to swap this
//! implementation, and it holds no user data at all (only public scheme
//! metadata), so it doesn't need the same swappability contract as
//! transactional/price data does.

use super::SqlitePool;
use chrono::NaiveDate;
use pm_domain::repositories::RepositoryError;
use rust_decimal::Decimal;
use rusqlite::params;
use std::str::FromStr;

use crate::market_data::amfi::AmfiSchemeRow;

pub struct SqliteMfSchemeCache {
    pool: SqlitePool,
}

impl SqliteMfSchemeCache {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    /// Wholesale replace — every call clears the entire cache and reloads
    /// it from the freshly-fetched AMFI file, per the "clear cache, then
    /// load the latest" behavior explicitly requested. This is safe
    /// precisely because nothing permanent lives in this table — every row
    /// here is rebuildable from the next AMFI fetch.
    pub async fn replace_all(&self, rows: Vec<AmfiSchemeRow>) -> Result<(), RepositoryError> {
        self.pool
            .with_conn(move |conn| {
                let tx = conn.unchecked_transaction()?;
                tx.execute("DELETE FROM mf_scheme_cache", [])?;
                {
                    let mut stmt = tx.prepare(
                        "INSERT INTO mf_scheme_cache
                            (scheme_code, scheme_name, category, amc_name, isin_growth, isin_div_reinvest, nav, nav_date)
                         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                    )?;
                    for row in &rows {
                        stmt.execute(params![
                            row.scheme_code,
                            row.scheme_name,
                            row.category,
                            row.amc_name,
                            row.isin_growth,
                            row.isin_div_reinvest,
                            row.nav.to_string(),
                            row.nav_date.format("%Y-%m-%d").to_string(),
                        ])?;
                    }
                }
                tx.commit()?;
                Ok(())
            })
            .await
    }

    pub async fn count(&self) -> Result<i64, RepositoryError> {
        self.pool
            .with_conn(|conn| conn.query_row("SELECT COUNT(*) FROM mf_scheme_cache", [], |row| row.get(0)))
            .await
    }

    /// Case-insensitive substring search over scheme name — the backbone
    /// of the "type to search" fund picker. Capped at `limit` so a broad
    /// query (e.g. "fund") doesn't return thousands of rows to the UI.
    pub async fn search_by_name(&self, query: &str, limit: u32) -> Result<Vec<AmfiSchemeRow>, RepositoryError> {
        let pattern = format!("%{}%", query.replace('%', "").replace('_', ""));
        let rows: Vec<RawRow> = self
            .pool
            .with_conn(move |conn| {
                let mut stmt = conn.prepare(
                    "SELECT scheme_code, scheme_name, category, amc_name, isin_growth, isin_div_reinvest, nav, nav_date
                     FROM mf_scheme_cache WHERE scheme_name LIKE ?1 COLLATE NOCASE
                     ORDER BY scheme_name ASC LIMIT ?2",
                )?;
                let rows = stmt
                    .query_map(params![pattern, limit], row_to_raw)?
                    .collect::<rusqlite::Result<Vec<_>>>()?;
                Ok(rows)
            })
            .await?;
        rows.into_iter().map(parse_row).collect()
    }

    pub async fn get_by_code(&self, scheme_code: &str) -> Result<Option<AmfiSchemeRow>, RepositoryError> {
        let scheme_code = scheme_code.to_string();
        let row: Option<RawRow> = self
            .pool
            .with_conn(move |conn| {
                conn.query_row(
                    "SELECT scheme_code, scheme_name, category, amc_name, isin_growth, isin_div_reinvest, nav, nav_date
                     FROM mf_scheme_cache WHERE scheme_code = ?1",
                    params![scheme_code],
                    row_to_raw,
                )
                .map(Some)
                .or_else(|e| if matches!(e, rusqlite::Error::QueryReturnedNoRows) { Ok(None) } else { Err(e) })
            })
            .await?;
        row.map(parse_row).transpose()
    }
}

type RawRow = (String, String, String, String, Option<String>, Option<String>, String, String);

fn row_to_raw(row: &rusqlite::Row) -> rusqlite::Result<RawRow> {
    Ok((
        row.get(0)?,
        row.get(1)?,
        row.get(2)?,
        row.get(3)?,
        row.get(4)?,
        row.get(5)?,
        row.get(6)?,
        row.get(7)?,
    ))
}

fn parse_row(row: RawRow) -> Result<AmfiSchemeRow, RepositoryError> {
    let (scheme_code, scheme_name, category, amc_name, isin_growth, isin_div_reinvest, nav, nav_date) = row;
    let parse_err = |ctx: &str, e: String| RepositoryError::Storage(format!("corrupt {ctx} in DB: {e}"));
    Ok(AmfiSchemeRow {
        scheme_code,
        isin_growth,
        isin_div_reinvest,
        scheme_name,
        nav: Decimal::from_str(&nav).map_err(|e| parse_err("nav", e.to_string()))?,
        nav_date: NaiveDate::parse_from_str(&nav_date, "%Y-%m-%d").map_err(|e| parse_err("nav_date", e.to_string()))?,
        category,
        amc_name,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_row(code: &str, name: &str) -> AmfiSchemeRow {
        AmfiSchemeRow {
            scheme_code: code.to_string(),
            isin_growth: Some("INF209KA12Z1".to_string()),
            isin_div_reinvest: None,
            scheme_name: name.to_string(),
            nav: Decimal::from_str("106.5961").unwrap(),
            nav_date: NaiveDate::from_ymd_opt(2026, 7, 24).unwrap(),
            category: "Debt Scheme - Banking and PSU Fund".to_string(),
            amc_name: "Aditya Birla Sun Life Mutual Fund".to_string(),
        }
    }

    #[tokio::test]
    async fn replace_all_then_search_round_trips() {
        let pool = SqlitePool::open_in_memory().unwrap();
        let cache = SqliteMfSchemeCache::new(pool);

        cache
            .replace_all(vec![
                sample_row("119551", "Aditya Birla Sun Life Banking & PSU Debt Fund - Direct - Growth"),
                sample_row("119552", "HDFC Corporate Bond Fund - Direct - Growth"),
            ])
            .await
            .unwrap();

        assert_eq!(cache.count().await.unwrap(), 2);

        let results = cache.search_by_name("banking", 10).await.unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].scheme_code, "119551");
    }

    #[tokio::test]
    async fn replace_all_wholesale_replaces_not_accumulates() {
        let pool = SqlitePool::open_in_memory().unwrap();
        let cache = SqliteMfSchemeCache::new(pool);

        cache.replace_all(vec![sample_row("111", "Old Fund")]).await.unwrap();
        assert_eq!(cache.count().await.unwrap(), 1);

        // A second refresh with completely different rows must not leave
        // "Old Fund" behind — this is the exact "clear cache daily" behavior
        // the user asked for.
        cache.replace_all(vec![sample_row("222", "New Fund")]).await.unwrap();
        assert_eq!(cache.count().await.unwrap(), 1);
        assert!(cache.get_by_code("111").await.unwrap().is_none());
        assert!(cache.get_by_code("222").await.unwrap().is_some());
    }

    #[tokio::test]
    async fn search_is_case_insensitive() {
        let pool = SqlitePool::open_in_memory().unwrap();
        let cache = SqliteMfSchemeCache::new(pool);
        cache.replace_all(vec![sample_row("119551", "HDFC Corporate Bond Fund")]).await.unwrap();

        assert_eq!(cache.search_by_name("hdfc", 10).await.unwrap().len(), 1);
        assert_eq!(cache.search_by_name("HDFC", 10).await.unwrap().len(), 1);
        assert_eq!(cache.search_by_name("CoRpOrAtE", 10).await.unwrap().len(), 1);
    }

    #[tokio::test]
    async fn get_by_code_returns_none_for_unknown_code() {
        let pool = SqlitePool::open_in_memory().unwrap();
        let cache = SqliteMfSchemeCache::new(pool);
        assert!(cache.get_by_code("999999").await.unwrap().is_none());
    }
}

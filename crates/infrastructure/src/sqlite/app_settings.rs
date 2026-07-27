//! Local-only config storage — not behind a domain repository trait,
//! same reasoning as mf_scheme_cache: nothing else needs to swap this
//! implementation, and it holds no financial/transactional data.

use super::SqlitePool;
use pm_domain::repositories::RepositoryError;
use rusqlite::params;

pub struct SqliteAppSettings {
    pool: SqlitePool,
}

impl SqliteAppSettings {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    pub async fn get(&self, key: &str) -> Result<Option<String>, RepositoryError> {
        let key = key.to_string();
        self.pool
            .with_conn(move |conn| {
                conn.query_row("SELECT value FROM app_settings WHERE key = ?1", params![key], |row| row.get(0))
                    .map(Some)
                    .or_else(|e| if matches!(e, rusqlite::Error::QueryReturnedNoRows) { Ok(None) } else { Err(e) })
            })
            .await
    }

    pub async fn set(&self, key: &str, value: &str) -> Result<(), RepositoryError> {
        let key = key.to_string();
        let value = value.to_string();
        self.pool
            .with_conn(move |conn| {
                conn.execute(
                    "INSERT INTO app_settings (key, value) VALUES (?1, ?2) \
                     ON CONFLICT(key) DO UPDATE SET value = excluded.value",
                    params![key, value],
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
    async fn get_returns_none_for_unset_key() {
        let pool = SqlitePool::open_in_memory().unwrap();
        let settings = SqliteAppSettings::new(pool);
        assert_eq!(settings.get("nonexistent").await.unwrap(), None);
    }

    #[tokio::test]
    async fn set_then_get_round_trips() {
        let pool = SqlitePool::open_in_memory().unwrap();
        let settings = SqliteAppSettings::new(pool);
        settings.set("alpha_vantage_api_key", "ABC123").await.unwrap();
        assert_eq!(settings.get("alpha_vantage_api_key").await.unwrap(), Some("ABC123".to_string()));
    }

    #[tokio::test]
    async fn set_overwrites_existing_value() {
        let pool = SqlitePool::open_in_memory().unwrap();
        let settings = SqliteAppSettings::new(pool);
        settings.set("key", "first").await.unwrap();
        settings.set("key", "second").await.unwrap();
        assert_eq!(settings.get("key").await.unwrap(), Some("second".to_string()));
    }
}

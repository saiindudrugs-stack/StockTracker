use super::SqlitePool;
use async_trait::async_trait;
use pm_domain::entities::{AssetClass, Instrument};
use pm_domain::repositories::{InstrumentRepository, RepositoryError};
use pm_domain::value_objects::Isin;
use rusqlite::{params, OptionalExtension};
use uuid::Uuid;

pub struct SqliteInstrumentRepository {
    pool: SqlitePool,
}

impl SqliteInstrumentRepository {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }
}

fn asset_class_to_str(a: AssetClass) -> &'static str {
    match a {
        AssetClass::Equity => "equity",
        AssetClass::MutualFund => "mutual_fund",
        AssetClass::Etf => "etf",
        AssetClass::SovereignGoldBond => "sgb",
        AssetClass::Bond => "bond",
        AssetClass::ReitInvit => "reit_invit",
    }
}

fn str_to_asset_class(s: &str) -> Result<AssetClass, RepositoryError> {
    Ok(match s {
        "equity" => AssetClass::Equity,
        "mutual_fund" => AssetClass::MutualFund,
        "etf" => AssetClass::Etf,
        "sgb" => AssetClass::SovereignGoldBond,
        "bond" => AssetClass::Bond,
        "reit_invit" => AssetClass::ReitInvit,
        other => return Err(RepositoryError::Storage(format!("unknown asset_class '{other}' in DB"))),
    })
}

type InstrumentRow = (String, String, String, String, String, Option<String>);

fn row_to_instrument(row: &rusqlite::Row) -> rusqlite::Result<InstrumentRow> {
    Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?, row.get(5)?))
}

fn parse_instrument(row: InstrumentRow) -> Result<Instrument, RepositoryError> {
    let (id, isin, symbol, asset_class, exchange, sector) = row;
    let parse_err = |ctx: &str, e: String| RepositoryError::Storage(format!("corrupt {ctx} in DB: {e}"));
    Ok(Instrument {
        id: Uuid::parse_str(&id).map_err(|e| parse_err("id", e.to_string()))?,
        isin: Isin::parse(&isin).map_err(|e| parse_err("isin", e.to_string()))?,
        symbol,
        asset_class: str_to_asset_class(&asset_class)?,
        exchange,
        sector,
    })
}

#[async_trait]
impl InstrumentRepository for SqliteInstrumentRepository {
    async fn upsert(&self, instrument: &Instrument) -> Result<(), RepositoryError> {
        let instrument = instrument.clone();
        self.pool
            .with_conn(move |conn| {
                conn.execute(
                    r#"INSERT INTO instrument (id, isin, symbol, asset_class, exchange, sector)
                       VALUES (?1, ?2, ?3, ?4, ?5, ?6)
                       ON CONFLICT(id) DO UPDATE SET
                         isin = excluded.isin, symbol = excluded.symbol,
                         asset_class = excluded.asset_class, exchange = excluded.exchange,
                         sector = excluded.sector"#,
                    params![
                        instrument.id.to_string(),
                        instrument.isin.as_str(),
                        instrument.symbol,
                        asset_class_to_str(instrument.asset_class),
                        instrument.exchange,
                        instrument.sector,
                    ],
                )?;
                Ok(())
            })
            .await
    }

    async fn get(&self, id: Uuid) -> Result<Instrument, RepositoryError> {
        let row = self
            .pool
            .with_conn(move |conn| {
                conn.query_row(
                    "SELECT id, isin, symbol, asset_class, exchange, sector FROM instrument WHERE id = ?1",
                    params![id.to_string()],
                    row_to_instrument,
                )
            })
            .await
            .map_err(|_| RepositoryError::NotFound(format!("instrument {id}")))?;
        parse_instrument(row)
    }

    async fn find_by_isin(&self, isin: &str) -> Result<Option<Instrument>, RepositoryError> {
        let isin = isin.to_string();
        let row = self
            .pool
            .with_conn(move |conn| {
                conn.query_row(
                    "SELECT id, isin, symbol, asset_class, exchange, sector FROM instrument WHERE isin = ?1",
                    params![isin],
                    row_to_instrument,
                )
                .optional()
            })
            .await?;
        row.map(parse_instrument).transpose()
    }

    async fn list_all(&self) -> Result<Vec<Instrument>, RepositoryError> {
        let rows = self
            .pool
            .with_conn(|conn| {
                let mut stmt = conn.prepare(
                    "SELECT id, isin, symbol, asset_class, exchange, sector FROM instrument ORDER BY symbol ASC",
                )?;
                let rows = stmt
                    .query_map([], row_to_instrument)?
                    .collect::<rusqlite::Result<Vec<_>>>()?;
                Ok(rows)
            })
            .await?;
        rows.into_iter().map(parse_instrument).collect()
    }
}

impl SqliteInstrumentRepository {
    /// Not part of the `InstrumentRepository` domain trait — this is a UI/
    /// demo-convenience lookup (the Tauri commands take a human-typed
    /// symbol, not an ISIN or UUID). Kept as an inherent method here rather
    /// than added to the domain trait, since "look up by symbol" isn't a
    /// requirement any use-case actually needs yet.
    pub async fn find_by_symbol(&self, symbol: &str) -> Result<Option<Instrument>, RepositoryError> {
        let symbol = symbol.to_string();
        let row = self
            .pool
            .with_conn(move |conn| {
                conn.query_row(
                    "SELECT id, isin, symbol, asset_class, exchange, sector FROM instrument WHERE symbol = ?1",
                    params![symbol],
                    row_to_instrument,
                )
                .optional()
            })
            .await?;
        row.map(parse_instrument).transpose()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Instrument {
        Instrument {
            id: Uuid::new_v4(),
            isin: Isin::parse("INE002A01018").unwrap(),
            symbol: "RELIANCE".to_string(),
            asset_class: AssetClass::Equity,
            exchange: "NSE".to_string(),
            sector: Some("Energy".to_string()),
        }
    }

    #[tokio::test]
    async fn upsert_then_get_round_trips() {
        let pool = SqlitePool::open_in_memory().unwrap();
        let repo = SqliteInstrumentRepository::new(pool);
        let instrument = sample();

        repo.upsert(&instrument).await.unwrap();
        let fetched = repo.get(instrument.id).await.unwrap();
        assert_eq!(fetched, instrument);

        let by_isin = repo.find_by_isin("INE002A01018").await.unwrap();
        assert_eq!(by_isin, Some(instrument));
    }

    #[tokio::test]
    async fn upsert_is_idempotent_and_updates_fields() {
        let pool = SqlitePool::open_in_memory().unwrap();
        let repo = SqliteInstrumentRepository::new(pool);
        let mut instrument = sample();

        repo.upsert(&instrument).await.unwrap();
        instrument.sector = Some("Renamed".to_string());
        repo.upsert(&instrument).await.unwrap();

        let fetched = repo.get(instrument.id).await.unwrap();
        assert_eq!(fetched.sector.as_deref(), Some("Renamed"));
    }

    #[tokio::test]
    async fn list_all_returns_every_instrument_sorted_by_symbol() {
        let pool = SqlitePool::open_in_memory().unwrap();
        let repo = SqliteInstrumentRepository::new(pool);

        let mut tcs = sample();
        tcs.symbol = "TCS".to_string();
        tcs.isin = Isin::parse("INE467B01029").unwrap();
        let mut reliance = sample();
        reliance.symbol = "RELIANCE".to_string();

        repo.upsert(&tcs).await.unwrap();
        repo.upsert(&reliance).await.unwrap();

        let all = repo.list_all().await.unwrap();
        assert_eq!(all.len(), 2);
        assert_eq!(all[0].symbol, "RELIANCE"); // alphabetical, not insertion order
        assert_eq!(all[1].symbol, "TCS");
    }
}

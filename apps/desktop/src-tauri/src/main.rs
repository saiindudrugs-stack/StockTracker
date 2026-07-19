//! Tauri shell: the thinnest possible layer between the UI and the engine
//! built in Volume II, Slice 1. Every command below just deserializes IPC
//! arguments, calls a use-case (or repository) from the engine crates, and
//! serializes the result — no business logic lives here (HLD Section 3.1:
//! Presentation layer depends on Application only).
//!
//! MULTI-PORTFOLIO NOTE: every portfolio-scoped command below takes an
//! explicit `portfolio_id: String` argument from the frontend rather than a
//! hardcoded demo id — this is what actually turns "a family with several
//! individual accounts" into a real feature rather than one shared bucket.
//! Instruments and prices are NOT portfolio-scoped (shared reference data,
//! per HLD Section 5.1) — the same RELIANCE instrument row is looked up by
//! every portfolio's holdings.

use pm_application::use_cases::{
    ComputeXirrUseCase, DashboardSummary, DashboardSummaryUseCase, RecordTransactionUseCase,
};
use pm_domain::entities::{AssetClass, Holding, Instrument, Portfolio, Transaction, TransactionType};
use pm_domain::repositories::{
    HoldingRepository, InstrumentRepository, PortfolioRepository, PriceRepository, TransactionRepository,
};
use pm_domain::value_objects::{Currency, Isin, Money};
use pm_infrastructure::sqlite::{
    SqliteHoldingRepository, SqliteInstrumentRepository, SqlitePool, SqlitePortfolioRepository,
    SqlitePriceRepository, SqliteTransactionRepository,
};
use rust_decimal::Decimal;
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::str::FromStr;
use std::sync::Arc;
use tauri::{Manager, State};
use uuid::Uuid;

struct AppState {
    portfolios: Arc<SqlitePortfolioRepository>,
    transactions: Arc<SqliteTransactionRepository>,
    holdings: Arc<SqliteHoldingRepository>,
    instruments: Arc<SqliteInstrumentRepository>,
    prices: Arc<SqlitePriceRepository>,
}

#[derive(Serialize)]
struct PortfolioView {
    id: String,
    name: String,
}

#[derive(Serialize)]
struct HoldingView {
    symbol: String,
    sector: Option<String>,
    quantity: String,
    avg_cost: String,
    last_price: Option<String>,
    market_value: Option<String>,
    unrealized_pnl: Option<String>,
}

#[derive(Serialize)]
struct InstrumentView {
    symbol: String,
    sector: Option<String>,
}

#[derive(Serialize)]
struct PriceHistoryPoint {
    date: String,
    close: String,
}

fn parse_portfolio_id(raw: &str) -> Result<Uuid, String> {
    Uuid::parse_str(raw).map_err(|_| format!("invalid portfolio id '{raw}'"))
}

/// Deterministic, non-cryptographic placeholder ISIN for a user-added
/// ticker that doesn't come with a real one (SRS 2.2.2's CSV/manual-entry
/// path never specified an ISIN source). Prefixed "ZZ" — not a real ISIN
/// country code — so it's visibly a placeholder if it ever surfaces in a
/// report, rather than silently looking like a genuine identifier.
fn placeholder_isin(symbol: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(symbol.as_bytes());
    let digest = hasher.finalize();
    let hex: String = digest.iter().map(|b| format!("{b:02X}")).collect();
    format!("ZZ{}", &hex[..10])
}

#[tauri::command]
async fn list_portfolios(state: State<'_, AppState>) -> Result<Vec<PortfolioView>, String> {
    let all = state.portfolios.list_all().await.map_err(|e| e.to_string())?;
    Ok(all.into_iter().map(|p| PortfolioView { id: p.id.to_string(), name: p.name }).collect())
}

#[tauri::command]
async fn create_portfolio(state: State<'_, AppState>, name: String) -> Result<PortfolioView, String> {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        return Err("portfolio name can't be empty".to_string());
    }
    let portfolio = Portfolio {
        id: Uuid::new_v4(),
        name: trimmed.to_string(),
        base_currency: Currency::Inr,
        goal_tag: None,
    };
    state.portfolios.create(&portfolio).await.map_err(|e| e.to_string())?;
    Ok(PortfolioView { id: portfolio.id.to_string(), name: portfolio.name })
}

#[tauri::command]
async fn list_instruments(state: State<'_, AppState>) -> Result<Vec<InstrumentView>, String> {
    let all = state.instruments.list_all().await.map_err(|e| e.to_string())?;
    Ok(all.into_iter().map(|i| InstrumentView { symbol: i.symbol, sector: i.sector }).collect())
}

/// Adds a new ticker the user wants to track. No broker/exchange validation
/// happens here (SRS's Broker Adapter Framework isn't wired to this command
/// yet) — this just registers the symbol as reference data so it can be
/// bought/tracked. Exchange defaults to NSE and sector is left blank; both
/// are editable-in-spirit but there's no edit command yet, only add.
#[tauri::command]
async fn add_instrument(state: State<'_, AppState>, symbol: String) -> Result<InstrumentView, String> {
    let symbol = symbol.trim().to_uppercase();
    if symbol.is_empty() {
        return Err("symbol can't be empty".to_string());
    }
    if let Some(existing) = state.instruments.find_by_symbol(&symbol).await.map_err(|e| e.to_string())? {
        return Ok(InstrumentView { symbol: existing.symbol, sector: existing.sector });
    }
    let instrument = Instrument {
        id: Uuid::new_v4(),
        isin: Isin::parse(&placeholder_isin(&symbol)).map_err(|e| e.to_string())?,
        symbol: symbol.clone(),
        asset_class: AssetClass::Equity,
        exchange: "NSE".to_string(),
        sector: None,
    };
    state.instruments.upsert(&instrument).await.map_err(|e| e.to_string())?;
    Ok(InstrumentView { symbol: instrument.symbol, sector: instrument.sector })
}

#[tauri::command]
async fn get_price_history(state: State<'_, AppState>, symbol: String) -> Result<Vec<PriceHistoryPoint>, String> {
    let instrument = state
        .instruments
        .find_by_symbol(&symbol)
        .await
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("unknown symbol '{symbol}'"))?;

    let to = chrono::Utc::now().date_naive();
    let from = to - chrono::Duration::days(60);
    let series = state
        .prices
        .daily_series(instrument.id, from, to)
        .await
        .map_err(|e| e.to_string())?;

    Ok(series
        .into_iter()
        .map(|(date, close)| PriceHistoryPoint { date: date.format("%Y-%m-%d").to_string(), close: close.to_string() })
        .collect())
}

#[tauri::command]
async fn get_dashboard_summary(state: State<'_, AppState>, portfolio_id: String) -> Result<DashboardSummary, String> {
    let portfolio_id = parse_portfolio_id(&portfolio_id)?;
    let use_case = DashboardSummaryUseCase::new(state.holdings.clone(), state.prices.clone());
    use_case.execute(portfolio_id).await.map_err(|e| e.to_string())
}

#[tauri::command]
async fn list_holdings(state: State<'_, AppState>, portfolio_id: String) -> Result<Vec<HoldingView>, String> {
    let portfolio_id = parse_portfolio_id(&portfolio_id)?;
    let holdings: Vec<Holding> = state
        .holdings
        .list_for_portfolio(portfolio_id)
        .await
        .map_err(|e| e.to_string())?;

    let mut views = Vec::with_capacity(holdings.len());
    for h in holdings {
        let instrument = state
            .instruments
            .get(h.instrument_id)
            .await
            .map_err(|e| e.to_string())?;
        let ltp = state
            .prices
            .latest_price(h.instrument_id)
            .await
            .map_err(|e| e.to_string())?;
        views.push(HoldingView {
            symbol: instrument.symbol,
            sector: instrument.sector,
            quantity: h.quantity.to_string(),
            avg_cost: h.avg_cost.to_string(),
            last_price: ltp.map(|p| p.to_string()),
            market_value: ltp.map(|p| h.market_value(p).to_string()),
            unrealized_pnl: ltp.map(|p| h.unrealized_pnl(p).to_string()),
        });
    }
    Ok(views)
}

#[tauri::command]
async fn record_buy(
    state: State<'_, AppState>,
    portfolio_id: String,
    symbol: String,
    quantity: String,
    price: String,
) -> Result<(), String> {
    let portfolio_id = parse_portfolio_id(&portfolio_id)?;
    let instrument = state
        .instruments
        .find_by_symbol(&symbol)
        .await
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("unknown symbol '{symbol}' — add it as a ticker first"))?;

    let txn = Transaction {
        id: Uuid::new_v4(),
        portfolio_id,
        instrument_id: instrument.id,
        transaction_type: TransactionType::Buy,
        quantity: Decimal::from_str(&quantity).map_err(|e| e.to_string())?,
        price: Money::inr(Decimal::from_str(&price).map_err(|e| e.to_string())?),
        fees: Money::inr(Decimal::from_str("20").unwrap()),
        trade_date: chrono::Utc::now().date_naive(),
        broker_ref: None,
        recorded_at: chrono::Utc::now(),
    };

    let use_case = RecordTransactionUseCase::new(state.transactions.clone(), state.holdings.clone());
    use_case.execute(txn).await.map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
async fn compute_xirr_for_symbol(state: State<'_, AppState>, portfolio_id: String, symbol: String) -> Result<f64, String> {
    let portfolio_id = parse_portfolio_id(&portfolio_id)?;
    let instrument = state
        .instruments
        .find_by_symbol(&symbol)
        .await
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("unknown symbol '{symbol}'"))?;
    let use_case = ComputeXirrUseCase::new(state.transactions.clone(), state.prices.clone());
    use_case
        .execute_for_instrument(portfolio_id, instrument.id)
        .await
        .map_err(|e| e.to_string())
}

/// Deterministic pseudo-random walk (simple LCG, fixed seed) — no external
/// crate needed, and deterministic so every fresh install shows the same
/// demo chart rather than a different random one each run, which would make
/// screenshots/bug reports inconsistent between machines.
async fn seed_price_history(
    state: &AppState,
    instrument_id: Uuid,
    start_price: Decimal,
    seed: u64,
) -> Result<(), String> {
    let mut rng_state = seed;
    let mut next_step = || -> Decimal {
        rng_state ^= rng_state << 13;
        rng_state ^= rng_state >> 7;
        rng_state ^= rng_state << 17;
        let bucket = (rng_state % 301) as i64 - 150;
        Decimal::from(bucket) / Decimal::from(10000)
    };

    let today = chrono::Utc::now().date_naive();
    let mut price = start_price;
    let mut day_prices = Vec::with_capacity(60);
    for i in (0..60).rev() {
        let date = today - chrono::Duration::days(i);
        let pct_move = next_step();
        price = (price * (Decimal::ONE + pct_move)).round_dp(2);
        if price <= Decimal::ZERO {
            price = start_price;
        }
        day_prices.push((date, price));
    }

    for (date, close) in day_prices {
        state.prices.upsert_daily_bar(instrument_id, date, close).await.map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// Seeds one demo portfolio ("My Portfolio") with two demo instruments and
/// one buy each, only on first-ever launch (no portfolios exist yet) — so
/// re-launching, or any portfolio the user creates afterward, doesn't get
/// duplicate or unwanted demo data. A family's other 3 accounts are meant
/// to be created for real via "+ Add portfolio" in the UI, not guessed at
/// here with invented names.
async fn seed_demo_data_if_first_launch(state: &AppState) -> Result<(), String> {
    let existing_portfolios = state.portfolios.list_all().await.map_err(|e| e.to_string())?;
    if !existing_portfolios.is_empty() {
        return Ok(());
    }

    let demo_portfolio = Portfolio {
        id: Uuid::new_v4(),
        name: "My Portfolio".to_string(),
        base_currency: Currency::Inr,
        goal_tag: None,
    };
    state.portfolios.create(&demo_portfolio).await.map_err(|e| e.to_string())?;

    let reliance = Instrument {
        id: Uuid::new_v4(),
        isin: Isin::parse("INE002A01018").unwrap(),
        symbol: "RELIANCE".to_string(),
        asset_class: AssetClass::Equity,
        exchange: "NSE".to_string(),
        sector: Some("Energy".to_string()),
    };
    let tcs = Instrument {
        id: Uuid::new_v4(),
        isin: Isin::parse("INE467B01029").unwrap(),
        symbol: "TCS".to_string(),
        asset_class: AssetClass::Equity,
        exchange: "NSE".to_string(),
        sector: Some("IT".to_string()),
    };

    state.instruments.upsert(&reliance).await.map_err(|e| e.to_string())?;
    state.instruments.upsert(&tcs).await.map_err(|e| e.to_string())?;

    seed_price_history(state, reliance.id, Decimal::from_str("2450.00").unwrap(), 0x5EED_0001).await?;
    seed_price_history(state, tcs.id, Decimal::from_str("3950.00").unwrap(), 0x5EED_0002).await?;

    let use_case = RecordTransactionUseCase::new(state.transactions.clone(), state.holdings.clone());
    use_case
        .execute(Transaction {
            id: Uuid::new_v4(),
            portfolio_id: demo_portfolio.id,
            instrument_id: reliance.id,
            transaction_type: TransactionType::Buy,
            quantity: Decimal::from(10),
            price: Money::inr(Decimal::from_str("2450.50").unwrap()),
            fees: Money::inr(Decimal::from(20)),
            trade_date: chrono::Utc::now().date_naive(),
            broker_ref: None,
            recorded_at: chrono::Utc::now(),
        })
        .await
        .map_err(|e| e.to_string())?;

    use_case
        .execute(Transaction {
            id: Uuid::new_v4(),
            portfolio_id: demo_portfolio.id,
            instrument_id: tcs.id,
            transaction_type: TransactionType::Buy,
            quantity: Decimal::from(5),
            price: Money::inr(Decimal::from_str("3980.00").unwrap()),
            fees: Money::inr(Decimal::from(20)),
            trade_date: chrono::Utc::now().date_naive(),
            broker_ref: None,
            recorded_at: chrono::Utc::now(),
        })
        .await
        .map_err(|e| e.to_string())?;

    Ok(())
}

fn main() {
    tauri::Builder::default()
        .setup(|app| {
            let app_dir = app
                .path()
                .app_data_dir()
                .expect("app data dir must be resolvable");
            std::fs::create_dir_all(&app_dir).ok();
            let db_path = app_dir.join("portfolio.db");

            // SIMPLIFICATION (see crates/infrastructure README note): plain
            // SQLite, not yet SQLCipher — flagged, not hidden.
            let pool = SqlitePool::open(db_path.to_str().unwrap()).expect("failed to open local database");

            let state = AppState {
                portfolios: Arc::new(SqlitePortfolioRepository::new(pool.clone())),
                transactions: Arc::new(SqliteTransactionRepository::new(pool.clone())),
                holdings: Arc::new(SqliteHoldingRepository::new(pool.clone())),
                instruments: Arc::new(SqliteInstrumentRepository::new(pool.clone())),
                prices: Arc::new(SqlitePriceRepository::new(pool)),
            };

            tauri::async_runtime::block_on(seed_demo_data_if_first_launch(&state))
                .expect("demo data seeding failed");

            app.manage(state);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            list_portfolios,
            create_portfolio,
            get_dashboard_summary,
            list_holdings,
            list_instruments,
            add_instrument,
            get_price_history,
            record_buy,
            compute_xirr_for_symbol
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

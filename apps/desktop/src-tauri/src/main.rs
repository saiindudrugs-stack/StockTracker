//! Tauri shell: the thinnest possible layer between the UI and the engine
//! built in Volume II, Slice 1. Every command below just deserializes IPC
//! arguments, calls a use-case from `pm-application`, and serializes the
//! result — no business logic lives here (HLD Section 3.1: Presentation
//! layer depends on Application only).
//!
//! DEMO DATA NOTE: this seeds one demo portfolio with a couple of
//! instruments and transactions on first launch, purely so the dashboard
//! has something to show. There is no broker sync wired into the UI yet —
//! that's the next slice (calling `ZerodhaAdapter` from a Tauri command and
//! piping results through `RecordTransactionUseCase`).

use pm_application::use_cases::{
    ComputeXirrUseCase, DashboardSummary, DashboardSummaryUseCase, RecordTransactionUseCase,
};
use pm_domain::entities::{AssetClass, Holding, Instrument, Transaction, TransactionType};
use pm_domain::value_objects::{Isin, Money};
use pm_domain::repositories::{HoldingRepository, InstrumentRepository, PriceRepository, TransactionRepository};
use pm_infrastructure::sqlite::{
    SqliteHoldingRepository, SqliteInstrumentRepository, SqlitePool, SqlitePriceRepository,
    SqliteTransactionRepository,
};
use rust_decimal::Decimal;
use serde::Serialize;
use std::str::FromStr;
use std::sync::Arc;
use tauri::{Manager, State};
use uuid::Uuid;

/// Fixed demo portfolio id so every launch looks at the same seeded data
/// (a real build would read the "last opened portfolio" from settings —
/// out of scope for this slice).
const DEMO_PORTFOLIO_ID: Uuid = Uuid::from_u128(0x0000_0000_0000_0000_0000_0000_0000_0001);

struct AppState {
    transactions: Arc<SqliteTransactionRepository>,
    holdings: Arc<SqliteHoldingRepository>,
    instruments: Arc<SqliteInstrumentRepository>,
    prices: Arc<SqlitePriceRepository>,
}

#[derive(Serialize)]
struct HoldingView {
    symbol: String,
    quantity: String,
    avg_cost: String,
    last_price: Option<String>,
    market_value: Option<String>,
    unrealized_pnl: Option<String>,
}

#[tauri::command]
async fn get_dashboard_summary(state: State<'_, AppState>) -> Result<DashboardSummary, String> {
    let use_case = DashboardSummaryUseCase::new(state.holdings.clone(), state.prices.clone());
    use_case
        .execute(DEMO_PORTFOLIO_ID)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn list_holdings(state: State<'_, AppState>) -> Result<Vec<HoldingView>, String> {
    let holdings: Vec<Holding> = state
        .holdings
        .list_for_portfolio(DEMO_PORTFOLIO_ID)
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
    symbol: String,
    quantity: String,
    price: String,
) -> Result<(), String> {
    let instrument = state
        .instruments
        .find_by_symbol(&symbol)
        .await
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("unknown symbol '{symbol}' — only demo-seeded instruments exist in this slice"))?;

    let txn = Transaction {
        id: Uuid::new_v4(),
        portfolio_id: DEMO_PORTFOLIO_ID,
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
async fn compute_xirr_for_symbol(state: State<'_, AppState>, symbol: String) -> Result<f64, String> {
    let instrument = state
        .instruments
        .find_by_symbol(&symbol)
        .await
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("unknown symbol '{symbol}'"))?;
    let use_case = ComputeXirrUseCase::new(state.transactions.clone(), state.prices.clone());
    use_case
        .execute_for_instrument(DEMO_PORTFOLIO_ID, instrument.id)
        .await
        .map_err(|e| e.to_string())
}

/// Seeds two demo instruments + one buy each + a price, only if the demo
/// portfolio has no holdings yet — so re-launching doesn't duplicate data.
async fn seed_demo_data_if_empty(state: &AppState) -> Result<(), String> {
    let existing = state
        .holdings
        .list_for_portfolio(DEMO_PORTFOLIO_ID)
        .await
        .map_err(|e| e.to_string())?;
    if !existing.is_empty() {
        return Ok(());
    }

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

    state
        .prices
        .upsert_daily_bar(reliance.id, chrono::Utc::now().date_naive(), Decimal::from_str("2510.00").unwrap())
        .await
        .map_err(|e| e.to_string())?;
    state
        .prices
        .upsert_daily_bar(tcs.id, chrono::Utc::now().date_naive(), Decimal::from_str("4120.00").unwrap())
        .await
        .map_err(|e| e.to_string())?;

    let use_case = RecordTransactionUseCase::new(state.transactions.clone(), state.holdings.clone());
    use_case
        .execute(Transaction {
            id: Uuid::new_v4(),
            portfolio_id: DEMO_PORTFOLIO_ID,
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
            portfolio_id: DEMO_PORTFOLIO_ID,
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
                transactions: Arc::new(SqliteTransactionRepository::new(pool.clone())),
                holdings: Arc::new(SqliteHoldingRepository::new(pool.clone())),
                instruments: Arc::new(SqliteInstrumentRepository::new(pool.clone())),
                prices: Arc::new(SqlitePriceRepository::new(pool)),
            };

            tauri::async_runtime::block_on(seed_demo_data_if_empty(&state))
                .expect("demo data seeding failed");

            app.manage(state);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_dashboard_summary,
            list_holdings,
            record_buy,
            compute_xirr_for_symbol
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

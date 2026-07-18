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

#[tauri::command]
async fn list_instruments(state: State<'_, AppState>) -> Result<Vec<InstrumentView>, String> {
    // v1 shortcut: the demo only ever seeds RELIANCE and TCS, and there's no
    // "list all instruments" method on the repository trait yet (it wasn't
    // a requirement any use-case needed — see the same reasoning as
    // find_by_symbol in SqliteInstrumentRepository). Hardcoding the two
    // known demo symbols here is honest about that gap rather than adding
    // an unused-elsewhere trait method just to avoid it.
    let mut views = Vec::new();
    for symbol in ["RELIANCE", "TCS"] {
        if let Some(instrument) = state.instruments.find_by_symbol(symbol).await.map_err(|e| e.to_string())? {
            views.push(InstrumentView { symbol: instrument.symbol, sector: instrument.sector });
        }
    }
    Ok(views)
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
        // xorshift-style LCG step — good enough for demo data, not
        // cryptographic, never used for anything security-sensitive.
        rng_state ^= rng_state << 13;
        rng_state ^= rng_state >> 7;
        rng_state ^= rng_state << 17;
        // Map to a small daily percentage move in roughly [-1.5%, +1.5%].
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
            price = start_price; // guard against an implausible walk to zero
        }
        day_prices.push((date, price));
    }

    for (date, close) in day_prices {
        state
            .prices
            .upsert_daily_bar(instrument_id, date, close)
            .await
            .map_err(|e| e.to_string())?;
    }
    Ok(())
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

    // 60 days of daily closes via a small deterministic pseudo-random walk —
    // no external crate needed, and deterministic (fixed seed) so every
    // fresh install shows the same demo chart rather than a different
    // random one each time, which would make screenshots/bug reports
    // inconsistent between runs.
    seed_price_history(state, reliance.id, Decimal::from_str("2450.00").unwrap(), 0x5EED_0001).await?;
    seed_price_history(state, tcs.id, Decimal::from_str("3950.00").unwrap(), 0x5EED_0002).await?;

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
            list_instruments,
            get_price_history,
            record_buy,
            compute_xirr_for_symbol
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

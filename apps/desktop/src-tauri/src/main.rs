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
use pm_infrastructure::market_data::{yahoo_finance::YahooFinanceProvider, MarketDataProvider};
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
    pool: SqlitePool,
    portfolios: Arc<SqlitePortfolioRepository>,
    transactions: Arc<SqliteTransactionRepository>,
    holdings: Arc<SqliteHoldingRepository>,
    instruments: Arc<SqliteInstrumentRepository>,
    prices: Arc<SqlitePriceRepository>,
    market_data: Arc<YahooFinanceProvider>,
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
    /// Change vs. the previous trading day's close, as a fraction (e.g.
    /// 0.021 = +2.1%) — None when there isn't at least one prior day of
    /// price history yet (e.g. a ticker added and priced for the first
    /// time today has nothing to compare against).
    day_change_pct: Option<f64>,
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

#[derive(Serialize)]
struct BackfillResult {
    symbol: String,
    days_backfilled: usize,
}

/// Downloads a full year of daily closes from Yahoo Finance and persists
/// them into the local price_history store — this is what makes the Chart
/// screen actually show a real year of history instead of either nothing
/// (a freshly-added ticker) or the synthetic demo random-walk (the two
/// seeded instruments). Called automatically by the frontend right after
/// a ticker is added, and available as a manual re-run too (e.g. to
/// replace RELIANCE/TCS's synthetic seed data with the real thing).
///
/// Deliberately a separate command from add_instrument rather than baked
/// into it — instrument creation should succeed even if this network call
/// fails or the user is offline, and coupling them would make adding a
/// ticker silently depend on Yahoo being reachable.
#[tauri::command]
async fn backfill_history(state: State<'_, AppState>, symbol: String) -> Result<BackfillResult, String> {
    let instrument = state
        .instruments
        .find_by_symbol(&symbol)
        .await
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("unknown symbol '{symbol}'"))?;
    let yahoo_symbol = YahooFinanceProvider::to_yahoo_symbol(&instrument.symbol, &instrument.exchange);
    let bars = state
        .market_data
        .fetch_daily_history_1y(&yahoo_symbol)
        .await
        .map_err(|e| e.to_string())?;

    for bar in &bars {
        state
            .prices
            .upsert_daily_bar(instrument.id, bar.date, Decimal::from_str(&bar.close.to_string()).map_err(|e| e.to_string())?)
            .await
            .map_err(|e| e.to_string())?;
    }

    Ok(BackfillResult { symbol: instrument.symbol, days_backfilled: bars.len() })
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

        // Day change: compare today's latest price against the most recent
        // *prior* trading day's close in price_history. Looking back 10
        // calendar days (not just "yesterday") covers weekends/holidays
        // where the previous trading day isn't literally yesterday.
        let day_change_pct = if let Some(current) = ltp {
            let today = chrono::Utc::now().date_naive();
            let window_start = today - chrono::Duration::days(10);
            let series = state
                .prices
                .daily_series(h.instrument_id, window_start, today)
                .await
                .map_err(|e| e.to_string())?;
            // series is ordered by date ascending; the previous close is
            // the last entry strictly before today, if one exists.
            series
                .iter()
                .rev()
                .find(|(date, _)| *date < today)
                .and_then(|(_, prev_close)| {
                    if prev_close.is_zero() {
                        None
                    } else {
                        // round_dp(6): same lesson as the avg_cost bug —
                        // an un-rounded Decimal division can produce a
                        // 20+ digit repeating decimal; 6 dp is far more
                        // precision than a percentage display needs.
                        let pct = ((current - *prev_close) / *prev_close).round_dp(6);
                        pct.to_string().parse::<f64>().ok()
                    }
                })
        } else {
            None
        };

        views.push(HoldingView {
            symbol: instrument.symbol,
            sector: instrument.sector,
            quantity: h.quantity.to_string(),
            avg_cost: h.avg_cost.to_string(),
            last_price: ltp.map(|p| p.to_string()),
            market_value: ltp.map(|p| h.market_value(p).to_string()),
            unrealized_pnl: ltp.map(|p| h.unrealized_pnl(p).to_string()),
            day_change_pct,
        });
    }
    Ok(views)
}

#[derive(Serialize)]
struct RefreshPricesResult {
    updated: Vec<String>,
    failed: Vec<RefreshFailure>,
}

#[derive(Serialize)]
struct RefreshFailure {
    symbol: String,
    reason: String,
}

/// Pulls a fresh price for every instrument currently held in this
/// portfolio via the (unofficial, unsupported — see market_data/mod.rs)
/// Yahoo Finance endpoint. Deliberately continues past individual failures
/// rather than aborting the whole refresh — one delisted or mistyped
/// symbol shouldn't block updating the rest of the portfolio. Both the
/// successes and failures are reported back so the UI can show exactly
/// what did and didn't update, rather than a single opaque pass/fail.
#[tauri::command]
async fn refresh_prices(state: State<'_, AppState>, portfolio_id: String) -> Result<RefreshPricesResult, String> {
    let portfolio_id = parse_portfolio_id(&portfolio_id)?;
    let holdings = state
        .holdings
        .list_for_portfolio(portfolio_id)
        .await
        .map_err(|e| e.to_string())?;

    let mut updated = Vec::new();
    let mut failed = Vec::new();
    let today = chrono::Utc::now().date_naive();

    for h in holdings {
        let instrument = match state.instruments.get(h.instrument_id).await {
            Ok(i) => i,
            Err(e) => {
                failed.push(RefreshFailure { symbol: h.instrument_id.to_string(), reason: e.to_string() });
                continue;
            }
        };
        let yahoo_symbol = YahooFinanceProvider::to_yahoo_symbol(&instrument.symbol, &instrument.exchange);

        match state.market_data.fetch_quote(&yahoo_symbol).await {
            Ok(quote) => {
                if let Err(e) = state.prices.upsert_daily_bar(h.instrument_id, today, quote.price).await {
                    failed.push(RefreshFailure { symbol: instrument.symbol, reason: e.to_string() });
                } else {
                    updated.push(instrument.symbol);
                }
            }
            Err(e) => {
                failed.push(RefreshFailure { symbol: instrument.symbol, reason: e.to_string() });
            }
        }
    }

    Ok(RefreshPricesResult { updated, failed })
}

#[derive(Serialize)]
struct MarketSnapshotView {
    symbol: String,
    price: String,
    day_high: Option<String>,
    day_low: Option<String>,
    week52_high: Option<String>,
    week52_low: Option<String>,
    volume: Option<u64>,
}

/// Live quote for any tracked instrument, regardless of whether it's
/// actually held in any portfolio — this is what makes it possible to add
/// a ticker and watch it before ever buying (no portfolio_id needed here at
/// all, deliberately, since watching isn't owning).
#[tauri::command]
async fn get_market_snapshot(state: State<'_, AppState>, symbol: String) -> Result<MarketSnapshotView, String> {
    let instrument = state
        .instruments
        .find_by_symbol(&symbol)
        .await
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("unknown symbol '{symbol}'"))?;
    let yahoo_symbol = YahooFinanceProvider::to_yahoo_symbol(&instrument.symbol, &instrument.exchange);
    let quote = state.market_data.fetch_quote(&yahoo_symbol).await.map_err(|e| e.to_string())?;

    Ok(MarketSnapshotView {
        symbol: instrument.symbol,
        price: quote.price.to_string(),
        day_high: quote.day_high.map(|d| d.to_string()),
        day_low: quote.day_low.map(|d| d.to_string()),
        week52_high: quote.week52_high.map(|d| d.to_string()),
        week52_low: quote.week52_low.map(|d| d.to_string()),
        volume: quote.volume,
    })
}

#[derive(Serialize)]
struct TechnicalAnalysisView {
    phase: String,
    latest_close: f64,
    sma_10: Option<f64>,
    sma_20: Option<f64>,
    sma_50: Option<f64>,
    rsi_14: Option<f64>,
    annualized_return_pct: Option<f64>,
    annualized_volatility_pct: Option<f64>,
    /// Historical VaR at 95% confidence, as a fraction (e.g. -0.045 = a
    /// possible 4.5% one-day loss) — see the honesty/methodology note in
    /// crates/domain/src/analytics/portfolio_stats.rs.
    historical_var_95_pct: Option<f64>,
    /// Buy/Sell/Hold from the Fibonacci-retracement confluence check (see
    /// crates/domain/src/analytics/signal.rs for the full methodology and
    /// honesty note). This is a rule-based technical heuristic, not
    /// financial advice — `recommendation_reasons` lists exactly why it
    /// fired so it's auditable, never a black box.
    recommendation: Option<String>,
    recommendation_reasons: Vec<String>,
    nearest_fib_label: Option<String>,
    nearest_fib_price: Option<f64>,
}

/// One combined technical read on a stock: market phase, moving averages,
/// RSI, and risk/return stats — all computed from a single fetched year of
/// daily history rather than one call per statistic, since that history
/// fetch is the expensive part (this is why it's a manual per-row action,
/// not part of auto-refresh — see get_market_snapshot for the cheap
/// same-day quote used there instead).
#[tauri::command]
async fn analyze_market_phase(state: State<'_, AppState>, symbol: String) -> Result<TechnicalAnalysisView, String> {
    let instrument = state
        .instruments
        .find_by_symbol(&symbol)
        .await
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("unknown symbol '{symbol}'"))?;
    let yahoo_symbol = YahooFinanceProvider::to_yahoo_symbol(&instrument.symbol, &instrument.exchange);
    let bars = state
        .market_data
        .fetch_daily_history_1y(&yahoo_symbol)
        .await
        .map_err(|e| e.to_string())?;

    let phase = pm_domain::analytics::classify_market_phase(&bars);
    let closes: Vec<f64> = bars.iter().map(|b| b.close).collect();
    let latest_close = closes.last().copied().unwrap_or(0.0);

    let last_of = |series: Vec<Option<f64>>| series.last().copied().flatten();
    let sma_10 = last_of(pm_domain::analytics::sma_series(&closes, 10));
    let sma_20 = last_of(pm_domain::analytics::sma_series(&closes, 20));
    let sma_50 = last_of(pm_domain::analytics::sma_series(&closes, 50));
    let rsi_14 = last_of(pm_domain::analytics::rsi(&closes, 14));

    let returns = pm_domain::analytics::daily_returns(&closes);
    let (annualized_return_pct, annualized_volatility_pct, historical_var_95_pct) = if returns.len() >= 2 {
        (
            Some(pm_domain::analytics::annualized_return(&returns) * 100.0),
            Some(pm_domain::analytics::annualized_volatility(&returns) * 100.0),
            pm_domain::analytics::historical_var(&returns, 0.95).map(|v| v * 100.0),
        )
    } else {
        (None, None, None)
    };

    let signal = pm_domain::analytics::generate_signal(&bars, phase, rsi_14);
    let (recommendation, recommendation_reasons, nearest_fib_label, nearest_fib_price) = match signal {
        Some(s) => (
            Some(s.recommendation.label().to_string()),
            s.reasons,
            s.nearest_fib_level.as_ref().map(|l| l.label.to_string()),
            s.nearest_fib_level.as_ref().map(|l| l.price),
        ),
        None => (None, vec!["Not enough history yet for a reliable read (needs 50+ trading days)".to_string()], None, None),
    };

    Ok(TechnicalAnalysisView {
        phase: phase.label().to_string(),
        latest_close,
        sma_10,
        sma_20,
        sma_50,
        rsi_14,
        annualized_return_pct,
        annualized_volatility_pct,
        historical_var_95_pct,
        recommendation,
        recommendation_reasons,
        nearest_fib_label,
        nearest_fib_price,
    })
}

#[derive(Serialize)]
struct StockRiskReturn {
    symbol: String,
    annualized_return_pct: f64,
    annualized_volatility_pct: f64,
    /// Plain-language quadrant label matching the reference article's own
    /// framing ("High Risk Low Return" etc.) — computed by comparing each
    /// stock against the *median* return/volatility of the other held
    /// stocks being analyzed together, so the label is relative to this
    /// portfolio, not some universal fixed threshold that wouldn't mean
    /// much on its own.
    risk_label: String,
}

#[derive(Serialize)]
struct CorrelationPair {
    symbol_a: String,
    symbol_b: String,
    correlation: f64,
}

#[derive(Serialize)]
struct PortfolioAnalysisView {
    stocks: Vec<StockRiskReturn>,
    correlations: Vec<CorrelationPair>,
    /// Symbols where a 1-year history fetch failed (delisted, rate-limited,
    /// etc.) — excluded from the stats above rather than silently dropped
    /// with no explanation.
    skipped: Vec<RefreshFailure>,
}

fn median(mut values: Vec<f64>) -> f64 {
    if values.is_empty() {
        return 0.0;
    }
    values.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let mid = values.len() / 2;
    if values.len() % 2 == 0 {
        (values[mid - 1] + values[mid]) / 2.0
    } else {
        values[mid]
    }
}

/// Portfolio-level "understand my holdings" analysis, directly modeled on
/// the reference article's risk/return comparison and correlation matrix
/// sections: for every held stock, a year of daily history is fetched
/// (same heavier call as analyze_market_phase — this is a deliberate,
/// on-demand action, not something that runs automatically), and from that
/// this computes annualized return/volatility per stock plus the pairwise
/// Pearson correlation of daily returns across all of them.
#[tauri::command]
async fn get_portfolio_analysis(state: State<'_, AppState>, portfolio_id: String) -> Result<PortfolioAnalysisView, String> {
    let portfolio_id = parse_portfolio_id(&portfolio_id)?;
    let holdings = state.holdings.list_for_portfolio(portfolio_id).await.map_err(|e| e.to_string())?;

    let mut symbol_returns: Vec<(String, Vec<f64>)> = Vec::new();
    let mut skipped = Vec::new();

    for h in holdings {
        let instrument = match state.instruments.get(h.instrument_id).await {
            Ok(i) => i,
            Err(e) => {
                skipped.push(RefreshFailure { symbol: h.instrument_id.to_string(), reason: e.to_string() });
                continue;
            }
        };
        let yahoo_symbol = YahooFinanceProvider::to_yahoo_symbol(&instrument.symbol, &instrument.exchange);
        match state.market_data.fetch_daily_history_1y(&yahoo_symbol).await {
            Ok(bars) => {
                let closes: Vec<f64> = bars.iter().map(|b| b.close).collect();
                let returns = pm_domain::analytics::daily_returns(&closes);
                if returns.len() >= 2 {
                    symbol_returns.push((instrument.symbol, returns));
                } else {
                    skipped.push(RefreshFailure { symbol: instrument.symbol, reason: "not enough history yet".to_string() });
                }
            }
            Err(e) => skipped.push(RefreshFailure { symbol: instrument.symbol, reason: e.to_string() }),
        }
    }

    let ann_returns: Vec<f64> = symbol_returns.iter().map(|(_, r)| pm_domain::analytics::annualized_return(r) * 100.0).collect();
    let ann_vols: Vec<f64> = symbol_returns.iter().map(|(_, r)| pm_domain::analytics::annualized_volatility(r) * 100.0).collect();
    let median_return = median(ann_returns.clone());
    let median_vol = median(ann_vols.clone());

    let stocks: Vec<StockRiskReturn> = symbol_returns
        .iter()
        .enumerate()
        .map(|(i, (symbol, _))| {
            let ret = ann_returns[i];
            let vol = ann_vols[i];
            let risk_word = if vol > median_vol { "High Risk" } else { "Low Risk" };
            let return_word = if ret > median_return { "High Return" } else { "Low Return" };
            StockRiskReturn {
                symbol: symbol.clone(),
                annualized_return_pct: ret,
                annualized_volatility_pct: vol,
                risk_label: format!("{risk_word}, {return_word}"),
            }
        })
        .collect();

    let mut correlations = Vec::new();
    for i in 0..symbol_returns.len() {
        for j in (i + 1)..symbol_returns.len() {
            if let Some(corr) = pm_domain::analytics::pearson_correlation(&symbol_returns[i].1, &symbol_returns[j].1) {
                correlations.push(CorrelationPair {
                    symbol_a: symbol_returns[i].0.clone(),
                    symbol_b: symbol_returns[j].0.clone(),
                    correlation: corr,
                });
            }
        }
    }

    Ok(PortfolioAnalysisView { stocks, correlations, skipped })
}

/// Wipes every portfolio, holding, transaction, instrument, and cached
/// price from the local database — irreversible, and there's no
/// confirmation dialog on the Rust side, so the frontend MUST confirm with
/// the user before calling this (see SettingsScreen.tsx's "Danger Zone").
/// This exists specifically because reinstalling the app does not clear
/// local data — see the doc comment on SqlitePool::reset_all for why.
#[tauri::command]
async fn reset_all_data(state: State<'_, AppState>) -> Result<(), String> {
    state.pool.reset_all().await.map_err(|e| e.to_string())
}

/// Removes one stock's row from Holdings for one portfolio — deletes all
/// of that instrument's transactions in this portfolio plus the cached
/// snapshot, so it stops showing up in list_holdings. This does NOT delete
/// the instrument itself from the shared reference table (see
/// remove_from_watchlist for that) — the same ticker can still be tracked
/// on the Watchlist screen or held in a different family portfolio.
///
/// This is a deliberate test/cleanup escape hatch, not a normal correction
/// mechanism — see the doc comment on TransactionRepository::
/// delete_for_instrument for why a real trading mistake should still be
/// fixed with an offsetting transaction, not a delete.
#[tauri::command]
async fn remove_holding(state: State<'_, AppState>, portfolio_id: String, symbol: String) -> Result<(), String> {
    let portfolio_id = parse_portfolio_id(&portfolio_id)?;
    let instrument = state
        .instruments
        .find_by_symbol(&symbol)
        .await
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("unknown symbol '{symbol}'"))?;

    state
        .transactions
        .delete_for_instrument(portfolio_id, instrument.id)
        .await
        .map_err(|e| e.to_string())?;
    state
        .holdings
        .delete_snapshot(portfolio_id, instrument.id)
        .await
        .map_err(|e| e.to_string())?;
    Ok(())
}

/// Removes a ticker from tracking entirely (Watchlist "Remove"). Since
/// instruments are shared reference data (HLD Section 5.1) — the same row
/// backs every portfolio's holdings and the Watchlist and Chart screens —
/// this is only safe when NOTHING currently holds a non-zero quantity of
/// it anywhere. Checked here by looking at every portfolio's holdings
/// before deleting, rather than trusting the caller; a family-scale number
/// of portfolios makes that loop cheap enough not to need a smarter query.
#[tauri::command]
async fn remove_from_watchlist(state: State<'_, AppState>, symbol: String) -> Result<(), String> {
    let instrument = state
        .instruments
        .find_by_symbol(&symbol)
        .await
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("unknown symbol '{symbol}'"))?;

    let portfolios = state.portfolios.list_all().await.map_err(|e| e.to_string())?;
    for portfolio in &portfolios {
        if let Some(holding) = state
            .holdings
            .get_snapshot(portfolio.id, instrument.id)
            .await
            .map_err(|e| e.to_string())?
        {
            if !holding.quantity.is_zero() {
                return Err(format!(
                    "Can't remove {symbol} — still held ({} shares) in portfolio '{}'. Remove it from Holdings there first.",
                    holding.quantity, portfolio.name
                ));
            }
        }
    }

    state.instruments.delete(instrument.id).await.map_err(|e| e.to_string())
}

/// Shared by record_buy and record_sell — both are "look up the instrument,
/// build a Transaction, run it through RecordTransactionUseCase" with only
/// the TransactionType differing. Kept as one function rather than two
/// near-identical copies after several bugs earlier in this project came
/// from exactly that kind of duplication drifting apart.
async fn record_transaction_of_type(
    state: &State<'_, AppState>,
    portfolio_id: String,
    symbol: String,
    quantity: String,
    price: String,
    transaction_type: TransactionType,
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
        transaction_type,
        quantity: Decimal::from_str(&quantity).map_err(|e| e.to_string())?,
        price: Money::inr(Decimal::from_str(&price).map_err(|e| e.to_string())?),
        fees: Money::inr(Decimal::from_str("20").unwrap()),
        trade_date: chrono::Utc::now().date_naive(),
        broker_ref: None,
        recorded_at: chrono::Utc::now(),
    };

    let use_case = RecordTransactionUseCase::new(state.transactions.clone(), state.holdings.clone());
    // A sell that overdraws the position is rejected here — before it ever
    // reaches the ledger — by RecordTransactionUseCase's own validate-then-
    // persist ordering (see the bug fix noted in the README under
    // "A real bug I found and fixed").
    use_case.execute(txn).await.map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
async fn record_buy(
    state: State<'_, AppState>,
    portfolio_id: String,
    symbol: String,
    quantity: String,
    price: String,
) -> Result<(), String> {
    record_transaction_of_type(&state, portfolio_id, symbol, quantity, price, TransactionType::Buy).await
}

#[tauri::command]
async fn record_sell(
    state: State<'_, AppState>,
    portfolio_id: String,
    symbol: String,
    quantity: String,
    price: String,
) -> Result<(), String> {
    record_transaction_of_type(&state, portfolio_id, symbol, quantity, price, TransactionType::Sell).await
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
                pool: pool.clone(),
                portfolios: Arc::new(SqlitePortfolioRepository::new(pool.clone())),
                transactions: Arc::new(SqliteTransactionRepository::new(pool.clone())),
                holdings: Arc::new(SqliteHoldingRepository::new(pool.clone())),
                instruments: Arc::new(SqliteInstrumentRepository::new(pool.clone())),
                prices: Arc::new(SqlitePriceRepository::new(pool)),
                market_data: Arc::new(YahooFinanceProvider::new()),
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
            backfill_history,
            get_price_history,
            record_buy,
            record_sell,
            compute_xirr_for_symbol,
            refresh_prices,
            get_market_snapshot,
            analyze_market_phase,
            get_portfolio_analysis,
            remove_holding,
            remove_from_watchlist,
            reset_all_data
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

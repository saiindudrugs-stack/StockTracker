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

use chrono::NaiveDate;
use pm_application::use_cases::{
    ComputeXirrUseCase, DashboardSummary, DashboardSummaryUseCase, RecordTransactionUseCase,
};
use pm_domain::entities::{AlertCondition, AlertRule, AssetClass, Holding, Instrument, Portfolio, Transaction, TransactionType};
use pm_domain::repositories::{
    AlertRuleRepository, HoldingRepository, InstrumentRepository, PortfolioRepository, PriceRepository,
    TransactionRepository,
};
use pm_domain::value_objects::{Currency, Isin, Money};
use pm_infrastructure::market_data::{
    alpha_vantage::{AlphaVantageProvider, ALPHA_VANTAGE_API_KEY_SETTING},
    amfi::{AmfiProvider, MutualFundDataSource},
    composite::CompositeMarketDataProvider,
    yahoo_finance::YahooFinanceProvider,
    MarketDataProvider,
};
use pm_infrastructure::sqlite::{
    SqliteAlertRuleRepository, SqliteAppSettings, SqliteHoldingRepository, SqliteInstrumentRepository,
    SqliteMfSchemeCache, SqlitePool, SqlitePortfolioRepository, SqlitePriceRepository, SqliteTransactionRepository,
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
    alert_rules: Arc<SqliteAlertRuleRepository>,
    market_data: Arc<CompositeMarketDataProvider<YahooFinanceProvider, AlphaVantageProvider>>,
    /// Separate from `market_data` above deliberately: fetch_fundamentals
    /// and fetch_news are inherent methods on YahooFinanceProvider itself
    /// (News & Fundamentals is Yahoo-only for now, no fallback provider
    /// for these two), not part of the MarketDataProvider trait the
    /// composite wraps — so they need a concrete instance, not the trait
    /// object. Cheap to have a second one; YahooFinanceProvider::new()
    /// just builds an HTTP client, no shared state to duplicate.
    yahoo_direct: Arc<YahooFinanceProvider>,
    mf_scheme_cache: Arc<SqliteMfSchemeCache>,
    mf_data_source: Arc<AmfiProvider>,
    app_settings: Arc<SqliteAppSettings>,
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
    /// Which market this instrument trades on (NSE/BSE/NASDAQ/NYSE/LSE/…)
    /// — the frontend uses this to show the right currency symbol per row,
    /// since a single portfolio can now hold instruments from multiple
    /// countries (the market/country selector added later than the
    /// original single-market design).
    exchange: String,
    quantity: String,
    avg_cost: String,
    last_price: Option<String>,
    /// Previous trading day's close — shown as its own column so the
    /// day-change % has a visible, checkable basis rather than being a
    /// number you just have to trust.
    previous_close: Option<String>,
    market_value: Option<String>,
    unrealized_pnl: Option<String>,
    /// Change vs. the previous trading day's close, as a fraction (e.g.
    /// 0.021 = +2.1%) — None when there isn't at least one prior day of
    /// price history yet (e.g. a ticker added and priced for the first
    /// time today has nothing to compare against).
    day_change_pct: Option<f64>,
    /// Today's move in rupees: quantity * (current price - previous
    /// close). Different from unrealized_pnl (which is since you bought
    /// it) and different from day_change_pct (which is a percentage, not
    /// an amount) — this is "how much did today specifically move my
    /// money by," the number a trader actually watching the day cares
    /// about most.
    day_gain_loss: Option<String>,
    /// Point-to-point return since the earliest Buy of this stock in this
    /// portfolio — unlike XIRR, ignores cashflow timing (a single lump
    /// entry vs. several buys), so the two numbers answering different
    /// questions can legitimately disagree.
    cagr_pct: Option<f64>,
    /// What the invested amount would be worth today at a flat 9.5%
    /// simple-interest rate over the same holding period — a plain
    /// benchmark line, not a claim about any real fixed-income product.
    simple_interest_value_at_9_5_pct: Option<String>,
    years_held: Option<f64>,
}

#[derive(Serialize)]
struct InstrumentView {
    symbol: String,
    sector: Option<String>,
    exchange: String,
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

/// Rounds an f64 price (from Yahoo's JSON, via DailyBar) to 2 decimal
/// places before it becomes a Decimal or a display string — the same fix
/// applied to YahooFinanceProvider::f64_to_decimal, needed again here since
/// OHLC bars take a separate path (fetch_daily_history_1y) from the plain
/// quote (fetch_quote). Without this, a raw f64 tail like
/// 761.4500122070313 propagates straight into stored prices and the
/// candlestick chart. A stock price has no meaningful precision beyond the
/// paisa, whatever produced the extra digits upstream.
fn round_price(v: f64) -> Decimal {
    Decimal::from_str(&v.to_string()).unwrap_or(Decimal::ZERO).round_dp(2)
}

/// Groups an exchange into the country label used for the Dashboard's
/// per-market summary — matches the same three markets the country
/// selector (MARKETS in CountrySelector.tsx) offers, plus a few extra
/// exchanges the Yahoo suffix mapping already understands, so a holding
/// never silently falls into an unlabeled bucket.
fn country_label_for_exchange(exchange: &str) -> &'static str {
    match exchange.to_uppercase().as_str() {
        "NSE" | "BSE" | "AMFI" => "India",
        "NASDAQ" | "NYSE" => "United States",
        "LSE" => "United Kingdom",
        "TSX" => "Canada",
        "ASX" => "Australia",
        "HKEX" => "Hong Kong",
        _ => "Other",
    }
}

/// "Today" per NSE/BSE's own clock (IST, UTC+5:30), not the server's UTC
/// clock. Plain `chrono::Utc::now().date_naive()` is wrong for roughly 5.5
/// hours every day (IST midnight to 5:30 AM) — UTC's calendar date is still
/// "yesterday" during that window, which would shift every "today vs.
/// previous close" comparison by a day for anyone using this app during
/// those hours India-side. Every place in this file that means "today" in
/// the sense of "the current trading day" should use this, not
/// chrono::Utc::now().date_naive() directly.
fn ist_today() -> chrono::NaiveDate {
    (chrono::Utc::now() + chrono::Duration::hours(5) + chrono::Duration::minutes(30)).date_naive()
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

/// Deletes a portfolio AND everything scoped to it — every transaction,
/// holding snapshot, and alert rule for it — since a portfolio with
/// dangling holding/transaction rows pointing at a deleted portfolio_id
/// would corrupt every other command that lists by portfolio. This is a
/// real, permanent data loss (not a soft-delete), which is why the UI
/// gates it behind the same two-click ConfirmButton pattern used for
/// row-level removal elsewhere, not a single click.
///
/// Deliberately does NOT touch the `instrument` table — instruments are
/// shared across portfolios and Watchlist (HLD Section 5.1), so deleting
/// one family member's portfolio must never remove a ticker another
/// portfolio still holds or that's just being watched.
#[tauri::command]
async fn delete_portfolio(state: State<'_, AppState>, portfolio_id: String) -> Result<(), String> {
    let portfolio_id = parse_portfolio_id(&portfolio_id)?;

    let holdings = state.holdings.list_for_portfolio(portfolio_id).await.map_err(|e| e.to_string())?;
    for h in &holdings {
        state.transactions.delete_for_instrument(portfolio_id, h.instrument_id).await.map_err(|e| e.to_string())?;
        state.holdings.delete_snapshot(portfolio_id, h.instrument_id).await.map_err(|e| e.to_string())?;
    }

    let alerts = state.alert_rules.list_for_portfolio(portfolio_id).await.map_err(|e| e.to_string())?;
    for a in &alerts {
        state.alert_rules.delete(a.id).await.map_err(|e| e.to_string())?;
    }

    state.portfolios.delete(portfolio_id).await.map_err(|e| e.to_string())
}

#[tauri::command]
async fn list_instruments(state: State<'_, AppState>) -> Result<Vec<InstrumentView>, String> {
    let all = state.instruments.list_all().await.map_err(|e| e.to_string())?;
    Ok(all.into_iter().map(|i| InstrumentView { symbol: i.symbol, sector: i.sector, exchange: i.exchange }).collect())
}

/// Equities only (no mutual funds) — the left-pane ticker list on the News
/// & Fundamentals screen, since revenue/fundamentals/news don't apply to a
/// mutual fund unit the same way they do a company.
#[tauri::command]
async fn list_equity_instruments(state: State<'_, AppState>) -> Result<Vec<InstrumentView>, String> {
    let equities = state.instruments.list_by_asset_class(AssetClass::Equity).await.map_err(|e| e.to_string())?;
    Ok(equities.into_iter().map(|i| InstrumentView { symbol: i.symbol, sector: i.sector, exchange: i.exchange }).collect())
}

#[derive(Serialize)]
struct RevenuePeriodView {
    period_end: String,
    revenue: String,
    net_income: Option<String>,
}

#[derive(Serialize)]
struct FundamentalsView {
    sector: Option<String>,
    industry: Option<String>,
    description: Option<String>,
    market_cap: Option<String>,
    pe_ratio: Option<String>,
    dividend_yield: Option<String>,
    week52_high: Option<String>,
    week52_low: Option<String>,
    revenue_by_period: Vec<RevenuePeriodView>,
}

/// Portfolio-agnostic, like get_market_snapshot — fundamentals are a
/// property of the company, not of any one portfolio's holding of it.
#[tauri::command]
async fn get_fundamentals(state: State<'_, AppState>, symbol: String) -> Result<FundamentalsView, String> {
    let instrument = state
        .instruments
        .find_by_symbol(&symbol)
        .await
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("unknown symbol '{symbol}'"))?;
    let f = state
        .yahoo_direct
        .fetch_fundamentals(&instrument.symbol, &instrument.exchange)
        .await
        .map_err(|e| e.to_string())?;

    Ok(FundamentalsView {
        sector: f.sector,
        industry: f.industry,
        description: f.description,
        market_cap: f.market_cap.map(|d| d.to_string()),
        pe_ratio: f.pe_ratio.map(|d| d.to_string()),
        dividend_yield: f.dividend_yield.map(|d| d.to_string()),
        week52_high: f.week52_high.map(|d| d.to_string()),
        week52_low: f.week52_low.map(|d| d.to_string()),
        revenue_by_period: f
            .revenue_by_period
            .into_iter()
            .map(|p| RevenuePeriodView { period_end: p.period_end, revenue: p.revenue.to_string(), net_income: p.net_income.map(|d| d.to_string()) })
            .collect(),
    })
}

#[derive(Serialize)]
struct NewsItemView {
    title: String,
    publisher: String,
    link: String,
    published_at: String,
    is_regulatory: bool,
}

/// Top 5, regulatory-flagged items first — see the honesty note at the
/// top of yahoo_fundamentals_news.rs for exactly what "regulatory" means
/// here (a keyword match over headlines, not a verified separate filings
/// feed).
#[tauri::command]
async fn get_stock_news(state: State<'_, AppState>, symbol: String) -> Result<Vec<NewsItemView>, String> {
    let instrument = state
        .instruments
        .find_by_symbol(&symbol)
        .await
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("unknown symbol '{symbol}'"))?;
    let news = state
        .yahoo_direct
        .fetch_news(&instrument.symbol, &instrument.exchange)
        .await
        .map_err(|e| e.to_string())?;

    Ok(news
        .into_iter()
        .map(|n| NewsItemView {
            title: n.title,
            publisher: n.publisher,
            link: n.link,
            published_at: n.published_at.format("%Y-%m-%d %H:%M UTC").to_string(),
            is_regulatory: n.is_regulatory,
        })
        .collect())
}

/// Adds a new ticker the user wants to track. No broker/exchange validation
/// happens here (SRS's Broker Adapter Framework isn't wired to this command
/// yet) — this just registers the symbol as reference data so it can be
/// bought/tracked. Exchange defaults to NSE and sector is left blank; both
/// are editable-in-spirit but there's no edit command yet, only add.
#[tauri::command]
/// Shared by add_instrument and the CSV importer — "find this symbol, or
/// register it fresh with the given exchange" is identical either way.
async fn ensure_instrument_tracked(state: &State<'_, AppState>, symbol: &str, exchange: &str) -> Result<Instrument, String> {
    let symbol = symbol.trim().to_uppercase();
    if symbol.is_empty() {
        return Err("symbol can't be empty".to_string());
    }
    let requested_exchange = exchange.trim().to_uppercase();
    if let Some(mut existing) = state.instruments.find_by_symbol(&symbol).await.map_err(|e| e.to_string())? {
        // A real bug this fixes: previously, an already-registered symbol
        // was returned as-is no matter what exchange was now being
        // requested — so switching the country/market selector and
        // re-adding a ticker silently did nothing if it happened to
        // already exist (e.g. added once under the wrong market by
        // mistake). Now the stored exchange is corrected in place when a
        // different one is explicitly given, updating the SAME
        // instrument_id — so existing holdings/transactions for it stay
        // intact, they just start resolving against the right market.
        if !requested_exchange.is_empty() && existing.exchange != requested_exchange {
            existing.exchange = requested_exchange;
            state.instruments.upsert(&existing).await.map_err(|e| e.to_string())?;
        }
        return Ok(existing);
    }
    let instrument = Instrument {
        id: Uuid::new_v4(),
        isin: Isin::parse(&placeholder_isin(&symbol)).map_err(|e| e.to_string())?,
        symbol: symbol.clone(),
        asset_class: AssetClass::Equity,
        exchange: requested_exchange,
        sector: None,
        display_name: None,
    };
    state.instruments.upsert(&instrument).await.map_err(|e| e.to_string())?;
    Ok(instrument)
}

#[tauri::command]
async fn add_instrument(state: State<'_, AppState>, symbol: String, exchange: Option<String>) -> Result<InstrumentView, String> {
    // Defaults to NSE when the frontend doesn't pass one — keeps existing
    // callers working unchanged. The country/market selector passes the
    // selected market's exchange explicitly.
    let instrument = ensure_instrument_tracked(&state, &symbol, exchange.as_deref().unwrap_or("NSE")).await?;
    Ok(InstrumentView { symbol: instrument.symbol, sector: instrument.sector, exchange: instrument.exchange })
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
/// Shared by backfill_history (the command) and demo seeding — fetches a
/// real year of Yahoo history and stores it. Factored out so the demo
/// RELIANCE/TCS instruments get real data at seed time instead of the old
/// synthetic random-walk seed, which was the actual root cause of a real
/// bug: comparing a live real quote against a fake synthetic "previous
/// close" produced nonsensical day-change percentages (a user-reported
/// -47% for RELIANCE that doesn't reflect anything that actually happened
/// in the market).
async fn backfill_instrument_history(state: &AppState, instrument: &Instrument) -> Result<usize, String> {
    let bars = state
        .market_data
        .fetch_daily_history_1y(&instrument.symbol, &instrument.exchange)
        .await
        .map_err(|e| e.to_string())?;

    for bar in &bars {
        state
            .prices
            .upsert_ohlc_bar(
                instrument.id,
                pm_domain::repositories::OhlcBar {
                    date: bar.date,
                    open: round_price(bar.open),
                    high: round_price(bar.high),
                    low: round_price(bar.low),
                    close: round_price(bar.close),
                    volume: Some(bar.volume as i64),
                },
            )
            .await
            .map_err(|e| e.to_string())?;
    }
    Ok(bars.len())
}

#[tauri::command]
async fn backfill_history(state: State<'_, AppState>, symbol: String) -> Result<BackfillResult, String> {
    let instrument = state
        .instruments
        .find_by_symbol(&symbol)
        .await
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("unknown symbol '{symbol}'"))?;
    let days_backfilled = backfill_instrument_history(&state, &instrument).await?;

    Ok(BackfillResult { symbol: instrument.symbol, days_backfilled })
}

#[derive(Serialize)]
struct ImportRowResult {
    row_number: usize,
    symbol: String,
    status: String, // "Imported" or an error message
}

#[derive(Serialize)]
struct ImportCsvResult {
    imported: usize,
    failed: usize,
    rows: Vec<ImportRowResult>,
}

/// Parses "Symbol,Quantity,BuyPrice,BuyDate,Exchange" CSV rows and records
/// each as a Buy transaction in the given portfolio. Hand-rolled parsing
/// rather than pulling in the `csv` crate — the expected fields (tickers,
/// numbers, ISO dates) never legitimately contain a comma or a quote, so a
/// plain split(',') is honest about what this actually handles rather than
/// implying full RFC 4180 support (quoted fields, embedded commas) it
/// doesn't have.
///
/// BuyDate is optional: per the user's own instruction, a blank date
/// defaults to exactly one year before today, since older holdings often
/// don't have an exactly-known purchase date on hand. Exchange is also
/// optional and defaults to NSE.
///
/// Every row is attempted independently — one bad row (a typo'd number, an
/// invalid date) doesn't abort the rest of the file. The per-row result
/// list is the whole point: silently skipping a row would be worse than a
/// slow file, but so would one bad row blocking 50 good ones.
/// Exports every column shown on the Holdings screen (plus XIRR, computed
/// separately since it isn't part of HoldingView) as CSV text. Reuses
/// list_holdings directly rather than re-querying everything a second way,
/// so the exported numbers are guaranteed to match what's on screen.
#[tauri::command]
async fn export_holdings_csv(state: State<'_, AppState>, portfolio_id: String, si_rate_pct: Option<f64>) -> Result<String, String> {
    let holdings = list_holdings(state.clone(), portfolio_id.clone(), si_rate_pct).await?;
    let pid = parse_portfolio_id(&portfolio_id)?;

    let mut out = String::from(
        "Symbol,Sector,Quantity,AvgCost,PreviousClose,LTP,DayChangePct,MarketValue,UnrealizedPnl,CAGRPct,SimpleInterestValue,YearsHeld,XIRRPct\n",
    );
    for h in &holdings {
        let xirr = match state.instruments.find_by_symbol(&h.symbol).await {
            Ok(Some(instrument)) => {
                let use_case = ComputeXirrUseCase::new(state.transactions.clone(), state.prices.clone());
                use_case
                    .execute_for_instrument(pid, instrument.id)
                    .await
                    .ok()
                    .map(|r| format!("{:.2}", r * 100.0))
                    .unwrap_or_default()
            }
            _ => String::new(),
        };
        // Sector/symbol are the only fields that could theoretically contain
        // a comma; wrapping every field in quotes would be safer in general,
        // but this app's own tickers/sectors are plain words in practice —
        // same honesty-over-completeness call as the CSV importer above.
        out.push_str(&format!(
            "{},{},{},{},{},{},{},{},{},{},{},{},{}\n",
            h.symbol,
            h.sector.clone().unwrap_or_default(),
            h.quantity,
            h.avg_cost,
            h.previous_close.clone().unwrap_or_default(),
            h.last_price.clone().unwrap_or_default(),
            h.day_change_pct.map(|p| format!("{:.4}", p * 100.0)).unwrap_or_default(),
            h.market_value.clone().unwrap_or_default(),
            h.unrealized_pnl.clone().unwrap_or_default(),
            h.cagr_pct.map(|p| format!("{p:.2}")).unwrap_or_default(),
            h.simple_interest_value_at_9_5_pct.clone().unwrap_or_default(),
            h.years_held.map(|y| format!("{y:.2}")).unwrap_or_default(),
            xirr,
        ));
    }
    Ok(out)
}

#[tauri::command]
async fn import_holdings_csv(state: State<'_, AppState>, portfolio_id: String, csv_content: String) -> Result<ImportCsvResult, String> {
    let portfolio_id = parse_portfolio_id(&portfolio_id)?;
    let default_buy_date = ist_today() - chrono::Duration::days(365);

    let mut rows_out = Vec::new();
    let mut imported = 0usize;
    let mut failed = 0usize;

    let lines: Vec<&str> = csv_content.lines().collect();
    // Skip a header row if the first cell looks like "Symbol" rather than
    // an actual ticker — tolerant of the template being re-uploaded as-is
    // versus a header-less export from a spreadsheet.
    let data_lines = if lines.first().map(|l| l.to_uppercase().starts_with("SYMBOL")).unwrap_or(false) {
        &lines[1..]
    } else {
        &lines[..]
    };

    for (i, line) in data_lines.iter().enumerate() {
        let row_number = i + 2; // +1 for 1-indexing, +1 for the header row
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let cols: Vec<&str> = line.split(',').map(|c| c.trim()).collect();
        if cols.len() < 3 {
            failed += 1;
            rows_out.push(ImportRowResult { row_number, symbol: line.to_string(), status: "expected at least Symbol,Quantity,BuyPrice".to_string() });
            continue;
        }

        let symbol = cols[0].to_uppercase();

        let outcome: Result<(), String> = async {
            let quantity = Decimal::from_str(cols[1]).map_err(|e| format!("bad quantity: {e}"))?;
            let buy_price = Decimal::from_str(cols[2]).map_err(|e| format!("bad buy price: {e}"))?;
            let buy_date = match cols.get(3).map(|s| s.trim()).filter(|s| !s.is_empty()) {
                Some(s) => NaiveDate::parse_from_str(s, "%Y-%m-%d").map_err(|e| format!("bad date '{s}': {e}"))?,
                None => default_buy_date,
            };
            let exchange = cols.get(4).map(|s| s.trim()).filter(|s| !s.is_empty()).unwrap_or("NSE").to_uppercase();

            let instrument = match state.instruments.find_by_symbol(&symbol).await.map_err(|e| e.to_string())? {
                Some(existing) => existing,
                None => {
                    let instrument = Instrument {
                        id: Uuid::new_v4(),
                        isin: Isin::parse(&placeholder_isin(&symbol)).map_err(|e| e.to_string())?,
                        symbol: symbol.clone(),
                        asset_class: AssetClass::Equity,
                        exchange,
                        sector: None,
                        display_name: None,
                    };
                    state.instruments.upsert(&instrument).await.map_err(|e| e.to_string())?;
                    // Best-effort: a newly-imported ticker otherwise has no
                    // chart/Watchlist data until someone remembers to click
                    // "Backfill Real 1Y History" manually. A failed backfill
                    // (rate limit, an unrecognized symbol on Yahoo's side)
                    // does NOT fail the import itself — the holding is still
                    // real and correct even if its chart is empty for now.
                    if let Ok(bars) = state.market_data.fetch_daily_history_1y(&instrument.symbol, &instrument.exchange).await {
                        for bar in &bars {
                            let ohlc = pm_domain::repositories::OhlcBar {
                                date: bar.date,
                                open: round_price(bar.open),
                                high: round_price(bar.high),
                                low: round_price(bar.low),
                                close: round_price(bar.close),
                                volume: Some(bar.volume as i64),
                            };
                            let _ = state.prices.upsert_ohlc_bar(instrument.id, ohlc).await;
                        }
                    }
                    instrument
                }
            };

            let txn = Transaction {
                id: Uuid::new_v4(),
                portfolio_id,
                instrument_id: instrument.id,
                transaction_type: TransactionType::Buy,
                quantity,
                price: Money::inr(buy_price),
                fees: Money::inr(Decimal::ZERO), // historical import — real per-trade fees aren't known
                trade_date: buy_date,
                broker_ref: None,
                recorded_at: chrono::Utc::now(),
            };
            let use_case = RecordTransactionUseCase::new(state.transactions.clone(), state.holdings.clone());
            use_case.execute(txn).await.map_err(|e| e.to_string())?;
            Ok(())
        }
        .await;

        match outcome {
            Ok(()) => {
                imported += 1;
                rows_out.push(ImportRowResult { row_number, symbol, status: "Imported".to_string() });
            }
            Err(e) => {
                failed += 1;
                rows_out.push(ImportRowResult { row_number, symbol, status: e });
            }
        }
    }

    Ok(ImportCsvResult { imported, failed, rows: rows_out })
}

#[tauri::command]
async fn get_price_history(state: State<'_, AppState>, symbol: String) -> Result<Vec<PriceHistoryPoint>, String> {
    let instrument = state
        .instruments
        .find_by_symbol(&symbol)
        .await
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("unknown symbol '{symbol}'"))?;

    let to = ist_today();
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

#[derive(Serialize)]
struct CandleView {
    date: String,
    open: String,
    high: String,
    low: String,
    close: String,
    volume: Option<i64>,
}

/// OHLC series for candlestick charting — a full year, matching what
/// backfill_history actually downloads, rather than the 60-day window
/// get_price_history uses for the simpler line-chart path.
#[tauri::command]
async fn get_ohlc_history(state: State<'_, AppState>, symbol: String) -> Result<Vec<CandleView>, String> {
    let instrument = state
        .instruments
        .find_by_symbol(&symbol)
        .await
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("unknown symbol '{symbol}'"))?;

    let to = ist_today();
    let from = to - chrono::Duration::days(365);
    let series = state.prices.ohlc_series(instrument.id, from, to).await.map_err(|e| e.to_string())?;

    Ok(series
        .into_iter()
        .map(|bar| CandleView {
            date: bar.date.format("%Y-%m-%d").to_string(),
            open: bar.open.to_string(),
            high: bar.high.to_string(),
            low: bar.low.to_string(),
            close: bar.close.to_string(),
            volume: bar.volume,
        })
        .collect())
}

#[tauri::command]
async fn get_dashboard_summary(state: State<'_, AppState>, portfolio_id: String) -> Result<DashboardSummary, String> {
    let portfolio_id = parse_portfolio_id(&portfolio_id)?;
    let use_case = DashboardSummaryUseCase::new(state.holdings.clone(), state.prices.clone());
    use_case.execute(portfolio_id).await.map_err(|e| e.to_string())
}

#[derive(Serialize)]
struct MarketSummaryView {
    country: String,
    currency_symbol: String,
    net_worth: String,
    unrealized_pnl: String,
    realized_pnl: String,
    holding_count: usize,
}

/// Per-country breakdown, not a currency-converted blend — see the doc
/// comment on why: get_dashboard_summary's single "net worth" figure sums
/// every holding's raw numeric value regardless of currency the moment a
/// portfolio holds instruments from more than one market, which is
/// mathematically meaningless (adding rupees and dollars as if they were
/// the same unit) without an FX conversion this app doesn't do. Each
/// country group here stays internally consistent in its own currency
/// instead — real numbers you can trust, rather than one blended number
/// that looks precise but isn't meaningful.
#[tauri::command]
async fn get_dashboard_by_market(state: State<'_, AppState>, portfolio_id: String) -> Result<Vec<MarketSummaryView>, String> {
    let portfolio_id = parse_portfolio_id(&portfolio_id)?;
    let holdings = state.holdings.list_for_portfolio(portfolio_id).await.map_err(|e| e.to_string())?;

    // country -> (net_worth, unrealized_pnl, realized_pnl, holding_count, currency_symbol)
    let mut groups: std::collections::HashMap<&'static str, (Decimal, Decimal, Decimal, usize, &'static str)> =
        std::collections::HashMap::new();

    for h in &holdings {
        let instrument = state.instruments.get(h.instrument_id).await.map_err(|e| e.to_string())?;
        // Mutual funds live on their own screen with their own currency
        // story (always India/AMFI today) — excluded here the same way
        // list_holdings excludes them from the equity Holdings table.
        if instrument.asset_class == AssetClass::MutualFund {
            continue;
        }
        let country = country_label_for_exchange(&instrument.exchange);
        let currency = match country {
            "India" => "₹",
            "United States" => "$",
            "United Kingdom" => "£",
            "Canada" => "C$",
            "Australia" => "A$",
            "Hong Kong" => "HK$",
            _ => "",
        };
        let entry = groups.entry(country).or_insert((Decimal::ZERO, Decimal::ZERO, Decimal::ZERO, 0, currency));
        entry.2 += h.realized_pnl;
        entry.3 += 1;
        if let Ok(Some(ltp)) = state.prices.latest_price(h.instrument_id).await {
            entry.0 += h.market_value(ltp);
            entry.1 += h.unrealized_pnl(ltp);
        }
    }

    let mut views: Vec<MarketSummaryView> = groups
        .into_iter()
        .map(|(country, (net_worth, unrealized, realized, count, currency))| MarketSummaryView {
            country: country.to_string(),
            currency_symbol: currency.to_string(),
            net_worth: net_worth.to_string(),
            unrealized_pnl: unrealized.to_string(),
            realized_pnl: realized.to_string(),
            holding_count: count,
        })
        .collect();
    views.sort_by(|a, b| a.country.cmp(&b.country));
    Ok(views)
}

struct HoldingMetrics {
    ltp: Option<Decimal>,
    previous_close: Option<Decimal>,
    day_change_pct: Option<f64>,
    day_gain_loss: Option<String>,
    cagr_pct: Option<f64>,
    simple_interest_value: Option<String>,
    years_held: Option<f64>,
    market_value: Option<String>,
    unrealized_pnl: Option<String>,
}

/// Shared by list_holdings (equities) and list_mutual_funds — both need
/// "current price vs. previous close, years held since first buy, CAGR,
/// and the simple-interest benchmark" for a holding, and this was written
/// once for equities first; factored out here rather than copy-pasted a
/// second time for funds, given how many of this project's earlier bugs
/// came from exactly that kind of near-duplicate drift.
async fn compute_holding_metrics(
    state: &State<'_, AppState>,
    portfolio_id: Uuid,
    h: &Holding,
    si_rate: f64,
) -> Result<HoldingMetrics, String> {
    let ltp = state.prices.latest_price(h.instrument_id).await.map_err(|e| e.to_string())?;
    let previous_close = find_previous_close(state, h.instrument_id).await?;
    let day_change_pct = match (ltp, previous_close) {
        (Some(current), Some(prev)) if !prev.is_zero() => {
            let pct = ((current - prev) / prev).round_dp(6);
            pct.to_string().parse::<f64>().ok()
        }
        _ => None,
    };
    let day_gain_loss = match (ltp, previous_close) {
        (Some(current), Some(prev)) => Some((h.quantity * (current - prev)).round_dp(2).to_string()),
        _ => None,
    };

    let ledger = state
        .transactions
        .list_for_instrument(portfolio_id, h.instrument_id)
        .await
        .map_err(|e| e.to_string())?;
    let earliest_buy = ledger
        .iter()
        .filter(|t| matches!(t.transaction_type, TransactionType::Buy | TransactionType::SipInstallment))
        .map(|t| t.trade_date)
        .min();

    let today = ist_today();
    let years_held = earliest_buy.map(|d| (today - d).num_days() as f64 / 365.25).filter(|y| *y > 0.0);

    let invested_value = h.avg_cost.to_string().parse::<f64>().unwrap_or(0.0) * h.quantity.to_string().parse::<f64>().unwrap_or(0.0);
    let cagr_pct = match (years_held, ltp) {
        (Some(years), Some(price)) => {
            let final_value = h.market_value(price).to_string().parse::<f64>().unwrap_or(0.0);
            pm_domain::analytics::cagr(invested_value, final_value, years).map(|r| r * 100.0)
        }
        _ => None,
    };
    let simple_interest_value = years_held
        .map(|years| pm_domain::analytics::simple_interest_value(invested_value, si_rate, years))
        .map(|v| format!("{v:.2}"));

    Ok(HoldingMetrics {
        ltp,
        previous_close,
        day_change_pct,
        day_gain_loss,
        cagr_pct,
        simple_interest_value,
        years_held,
        market_value: ltp.map(|p| h.market_value(p).to_string()),
        unrealized_pnl: ltp.map(|p| h.unrealized_pnl(p).to_string()),
    })
}

#[tauri::command]
async fn list_holdings(state: State<'_, AppState>, portfolio_id: String, si_rate_pct: Option<f64>) -> Result<Vec<HoldingView>, String> {
    // Defaults to 9.5% (the original hardcoded value) when the frontend
    // doesn't pass one — keeps existing behavior for anyone not using the
    // new configurable input.
    let si_rate = si_rate_pct.unwrap_or(9.5) / 100.0;
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
        // Mutual funds live on their own screen — see list_mutual_funds —
        // so they're deliberately excluded here rather than shown mixed in
        // with equities, per the explicit "don't mix them" requirement.
        if instrument.asset_class == AssetClass::MutualFund {
            continue;
        }

        let m = compute_holding_metrics(&state, portfolio_id, &h, si_rate).await?;

        views.push(HoldingView {
            symbol: instrument.symbol,
            sector: instrument.sector,
            exchange: instrument.exchange,
            quantity: h.quantity.to_string(),
            avg_cost: h.avg_cost.to_string(),
            last_price: m.ltp.map(|p| p.to_string()),
            previous_close: m.previous_close.map(|p| p.to_string()),
            market_value: m.market_value,
            unrealized_pnl: m.unrealized_pnl,
            day_change_pct: m.day_change_pct,
            day_gain_loss: m.day_gain_loss,
            cagr_pct: m.cagr_pct,
            simple_interest_value_at_9_5_pct: m.simple_interest_value,
            years_held: m.years_held,
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
    let today = ist_today();

    for h in holdings {
        let instrument = match state.instruments.get(h.instrument_id).await {
            Ok(i) => i,
            Err(e) => {
                failed.push(RefreshFailure { symbol: h.instrument_id.to_string(), reason: e.to_string() });
                continue;
            }
        };

        match state.market_data.fetch_quote(&instrument.symbol, &instrument.exchange).await {
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

/// Shared by list_holdings and get_market_snapshot — both need "% change
/// vs. the last trading day's close" for the same instrument, and this was
/// written inline in list_holdings first; factored out here rather than
/// copy-pasted a second time, given how many of this project's earlier
/// bugs came from exactly that kind of drift between near-duplicate logic.
/// The most recent trading day's close strictly before today — the shared
/// lookup behind both the "Previous Day Close" column and the day-change %
/// derived from it, so the two numbers can never quietly disagree with
/// each other.
async fn find_previous_close(
    state: &State<'_, AppState>,
    instrument_id: Uuid,
) -> Result<Option<Decimal>, String> {
    let today = ist_today();
    let window_start = today - chrono::Duration::days(10);
    let series = state
        .prices
        .daily_series(instrument_id, window_start, today)
        .await
        .map_err(|e| e.to_string())?;
    Ok(series.iter().rev().find(|(date, _)| *date < today).map(|(_, close)| *close))
}

#[derive(Serialize)]
struct MarketSnapshotView {
    symbol: String,
    /// Same reasoning as HoldingView.exchange — lets the frontend show the
    /// right currency symbol per row now that Watchlist can hold tickers
    /// from multiple countries.
    exchange: String,
    price: String,
    previous_close: Option<String>,
    day_high: Option<String>,
    day_low: Option<String>,
    week52_high: Option<String>,
    week52_low: Option<String>,
    volume: Option<u64>,
    day_change_pct: Option<f64>,
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
    let quote = state.market_data.fetch_quote(&instrument.symbol, &instrument.exchange).await.map_err(|e| e.to_string())?;
    let previous_close = find_previous_close(&state, instrument.id).await?;
    let day_change_pct = previous_close.and_then(|prev| {
        if prev.is_zero() {
            None
        } else {
            let pct = ((quote.price - prev) / prev).round_dp(6);
            pct.to_string().parse::<f64>().ok()
        }
    });

    Ok(MarketSnapshotView {
        symbol: instrument.symbol,
        exchange: instrument.exchange,
        price: quote.price.to_string(),
        previous_close: previous_close.map(|p| p.to_string()),
        day_high: quote.day_high.map(|d| d.to_string()),
        day_low: quote.day_low.map(|d| d.to_string()),
        week52_high: quote.week52_high.map(|d| d.to_string()),
        week52_low: quote.week52_low.map(|d| d.to_string()),
        volume: quote.volume,
        day_change_pct,
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
    let bars = state
        .market_data
        .fetch_daily_history_1y(&instrument.symbol, &instrument.exchange)
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
        match state.market_data.fetch_daily_history_1y(&instrument.symbol, &instrument.exchange).await {
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

/// Saves the Alpha Vantage fallback API key to local settings storage —
/// never committed to GitHub, never synced anywhere. Takes effect
/// immediately, no restart needed, since AlphaVantageProvider reads this
/// setting fresh on every call rather than caching it at startup.
#[tauri::command]
async fn save_alpha_vantage_key(state: State<'_, AppState>, api_key: String) -> Result<(), String> {
    state
        .app_settings
        .set(ALPHA_VANTAGE_API_KEY_SETTING, api_key.trim())
        .await
        .map_err(|e| e.to_string())
}

/// Returns whether a key is currently saved — deliberately does NOT return
/// the key itself back to the frontend once saved, so it isn't sitting in
/// the webview's JS state/memory longer than the one moment it's typed in.
#[tauri::command]
async fn has_alpha_vantage_key(state: State<'_, AppState>) -> Result<bool, String> {
    Ok(state
        .app_settings
        .get(ALPHA_VANTAGE_API_KEY_SETTING)
        .await
        .map_err(|e| e.to_string())?
        .map(|k| !k.trim().is_empty())
        .unwrap_or(false))
}

#[derive(Serialize)]
struct AlertRuleView {
    id: String,
    symbol: String,
    condition: String, // "stop_loss" | "target"
    threshold_price: String,
    triggered: bool,
    /// Live check against the current price — computed fresh on every
    /// list_alert_rules call rather than relying on a background poller,
    /// since this app has no persistent background process (it only runs
    /// while the window is open). `triggered` in the DB is the durable
    /// "this has fired at least once" record; `is_triggered_now` is
    /// today's read against the latest cached price.
    is_triggered_now: bool,
    /// Within 2% of the threshold but not yet crossed it — a softer,
    /// distinct "heads up" state from is_triggered_now, meant for a gentler
    /// pulse animation rather than the full alert blink.
    is_nearing: bool,
    current_price: Option<String>,
}

/// How close counts as "nearing" a stop-loss/target — 2% of the threshold
/// price. A named constant rather than a magic number, since this exact
/// figure is a judgment call worth being able to find and tune later.
const ALERT_NEARING_THRESHOLD_PCT: f64 = 0.02;

#[tauri::command]
async fn create_alert_rule(
    state: State<'_, AppState>,
    portfolio_id: String,
    symbol: String,
    condition: String,
    threshold_price: String,
) -> Result<(), String> {
    let portfolio_id = parse_portfolio_id(&portfolio_id)?;
    let instrument = state
        .instruments
        .find_by_symbol(&symbol)
        .await
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("unknown symbol '{symbol}'"))?;
    let condition = match condition.as_str() {
        "stop_loss" => AlertCondition::StopLoss,
        "target" => AlertCondition::Target,
        other => return Err(format!("unknown alert condition '{other}' — expected 'stop_loss' or 'target'")),
    };
    let rule = AlertRule {
        id: Uuid::new_v4(),
        portfolio_id,
        instrument_id: instrument.id,
        condition,
        threshold_price: Decimal::from_str(&threshold_price).map_err(|e| e.to_string())?,
        triggered: false,
    };
    state.alert_rules.create(&rule).await.map_err(|e| e.to_string())
}

/// Lists every alert rule for a portfolio, each with a *live* trigger check
/// against the latest cached price — this is what the Dashboard's alerts
/// panel and the flashing-row mechanism on Holdings/Watchlist both read
/// from. A rule already marked triggered in the DB stays marked (see the
/// doc comment on AlertRule::triggered) even if the live price has since
/// moved back across the threshold — dismissing it is an explicit action
/// (delete_alert_rule), not something a price bounce should do silently.
#[tauri::command]
async fn list_alert_rules(state: State<'_, AppState>, portfolio_id: String) -> Result<Vec<AlertRuleView>, String> {
    let portfolio_id = parse_portfolio_id(&portfolio_id)?;
    let rules = state.alert_rules.list_for_portfolio(portfolio_id).await.map_err(|e| e.to_string())?;

    let mut views = Vec::with_capacity(rules.len());
    for rule in rules {
        let instrument = state.instruments.get(rule.instrument_id).await.map_err(|e| e.to_string())?;
        let current_price = state.prices.latest_price(rule.instrument_id).await.map_err(|e| e.to_string())?;

        let is_triggered_now = current_price
            .map(|price| match rule.condition {
                AlertCondition::StopLoss => price <= rule.threshold_price,
                AlertCondition::Target => price >= rule.threshold_price,
            })
            .unwrap_or(false);

        // "Nearing" only makes sense before the alert has actually fired —
        // once triggered, it's not a warning anymore, it's the real thing.
        let is_nearing = !is_triggered_now
            && current_price
                .map(|price| {
                    if rule.threshold_price.is_zero() {
                        return false;
                    }
                    let distance = ((price - rule.threshold_price) / rule.threshold_price).abs();
                    let distance_f64 = distance.round_dp(6).to_string().parse::<f64>().unwrap_or(f64::MAX);
                    let approaching = match rule.condition {
                        // Only "nearing" if moving toward the threshold from
                        // the safe side — a stop-loss at 100 with price at
                        // 500 and falling isn't "nearing" yet even if some
                        // future 2% window would eventually say so; only the
                        // current distance matters, direction doesn't need
                        // separate tracking since price is checked live each
                        // call.
                        AlertCondition::StopLoss => price > rule.threshold_price,
                        AlertCondition::Target => price < rule.threshold_price,
                    };
                    approaching && distance_f64 <= ALERT_NEARING_THRESHOLD_PCT
                })
                .unwrap_or(false);

        // Durably record the first time this fires — see the trait doc
        // comment on mark_triggered for why this is one-way.
        if is_triggered_now && !rule.triggered {
            state.alert_rules.mark_triggered(rule.id).await.map_err(|e| e.to_string())?;
        }

        views.push(AlertRuleView {
            id: rule.id.to_string(),
            symbol: instrument.symbol,
            condition: match rule.condition {
                AlertCondition::StopLoss => "stop_loss".to_string(),
                AlertCondition::Target => "target".to_string(),
            },
            threshold_price: rule.threshold_price.to_string(),
            triggered: rule.triggered || is_triggered_now,
            is_triggered_now,
            is_nearing,
            current_price: current_price.map(|p| p.to_string()),
        });
    }
    Ok(views)
}

#[tauri::command]
async fn delete_alert_rule(state: State<'_, AppState>, id: String) -> Result<(), String> {
    let id = Uuid::parse_str(&id).map_err(|e| e.to_string())?;
    state.alert_rules.delete(id).await.map_err(|e| e.to_string())
}

#[derive(Serialize)]
struct MfRefreshResult {
    scheme_count: i64,
}

/// Wholesale-replaces the disposable AMFI scheme cache with today's file —
/// "clear the cache, then load the latest," exactly as requested. This is
/// the ~30,000+ row master list used only for the search-to-add picker; it
/// is never consulted for a fund the user already holds (that NAV history
/// lives permanently in price_history, untouched by this refresh).
#[tauri::command]
async fn refresh_mf_scheme_cache(state: State<'_, AppState>) -> Result<MfRefreshResult, String> {
    let rows = state.mf_data_source.fetch_all_schemes().await.map_err(|e| e.to_string())?;
    let count = rows.len() as i64;
    state.mf_scheme_cache.replace_all(rows).await.map_err(|e| e.to_string())?;
    Ok(MfRefreshResult { scheme_count: count })
}

#[derive(Serialize)]
struct MfSchemeSearchResultView {
    scheme_code: String,
    scheme_name: String,
    category: String,
    amc_name: String,
    nav: String,
    nav_date: String,
}

/// Type-to-search over the cached scheme master list — the picker that
/// replaces manually typing an AMFI Scheme Code. Returns at most 25
/// matches so a broad query doesn't flood the UI.
#[tauri::command]
async fn search_mf_schemes(state: State<'_, AppState>, query: String) -> Result<Vec<MfSchemeSearchResultView>, String> {
    if query.trim().len() < 2 {
        return Ok(Vec::new()); // avoid a near-unfiltered scan on a 1-character query
    }
    let rows = state.mf_scheme_cache.search_by_name(&query, 25).await.map_err(|e| e.to_string())?;
    Ok(rows
        .into_iter()
        .map(|r| MfSchemeSearchResultView {
            scheme_code: r.scheme_code,
            scheme_name: r.scheme_name,
            category: r.category,
            amc_name: r.amc_name,
            nav: r.nav.to_string(),
            nav_date: r.nav_date.format("%Y-%m-%d").to_string(),
        })
        .collect())
}

/// Registers a fund the user picked from the search results as a trackable
/// instrument (mirrors add_instrument for equities) and seeds its first
/// NAV data point into the *permanent* price_history — never the
/// disposable cache — so a chart/return calc has something to start from
/// immediately, without waiting for the next daily refresh.
#[tauri::command]
async fn add_mutual_fund(state: State<'_, AppState>, scheme_code: String) -> Result<InstrumentView, String> {
    if let Some(existing) = state.instruments.find_by_symbol(&scheme_code).await.map_err(|e| e.to_string())? {
        return Ok(InstrumentView { symbol: existing.symbol, sector: existing.sector, exchange: existing.exchange });
    }
    let scheme = state
        .mf_scheme_cache
        .get_by_code(&scheme_code)
        .await
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("scheme code '{scheme_code}' not found — try Refresh Fund List first"))?;

    let isin = scheme
        .isin_growth
        .clone()
        .or_else(|| scheme.isin_div_reinvest.clone())
        .unwrap_or_else(|| placeholder_isin(&scheme_code));
    let instrument = Instrument {
        id: Uuid::new_v4(),
        isin: Isin::parse(&isin).map_err(|e| e.to_string())?,
        symbol: scheme_code.clone(),
        asset_class: AssetClass::MutualFund,
        exchange: "AMFI".to_string(),
        sector: Some(scheme.category.clone()),
        display_name: Some(scheme.scheme_name.clone()),
    };
    state.instruments.upsert(&instrument).await.map_err(|e| e.to_string())?;
    state
        .prices
        .upsert_daily_bar(instrument.id, scheme.nav_date, scheme.nav)
        .await
        .map_err(|e| e.to_string())?;

    Ok(InstrumentView { symbol: instrument.symbol, sector: instrument.sector, exchange: instrument.exchange })
}

#[derive(Serialize)]
struct MfHoldingView {
    scheme_code: String,
    scheme_name: String,
    category: Option<String>,
    units: String,
    avg_nav: String,
    current_nav: Option<String>,
    previous_nav: Option<String>,
    nav_change_pct: Option<f64>,
    market_value: Option<String>,
    unrealized_pnl: Option<String>,
    cagr_pct: Option<f64>,
    simple_interest_value: Option<String>,
    years_held: Option<f64>,
}

/// Mutual funds, listed entirely separately from list_holdings — this is
/// the only place MutualFund-asset-class holdings ever appear, per the
/// explicit "don't mix them" requirement.
#[tauri::command]
async fn list_mutual_funds(state: State<'_, AppState>, portfolio_id: String, si_rate_pct: Option<f64>) -> Result<Vec<MfHoldingView>, String> {
    let si_rate = si_rate_pct.unwrap_or(9.5) / 100.0;
    let portfolio_id = parse_portfolio_id(&portfolio_id)?;
    let holdings = state.holdings.list_for_portfolio(portfolio_id).await.map_err(|e| e.to_string())?;

    let mut views = Vec::new();
    for h in holdings {
        let instrument = state.instruments.get(h.instrument_id).await.map_err(|e| e.to_string())?;
        if instrument.asset_class != AssetClass::MutualFund {
            continue;
        }
        let m = compute_holding_metrics(&state, portfolio_id, &h, si_rate).await?;
        views.push(MfHoldingView {
            scheme_code: instrument.symbol,
            scheme_name: instrument.display_name.unwrap_or_default(),
            category: instrument.sector,
            units: h.quantity.to_string(),
            avg_nav: h.avg_cost.to_string(),
            current_nav: m.ltp.map(|p| p.to_string()),
            previous_nav: m.previous_close.map(|p| p.to_string()),
            nav_change_pct: m.day_change_pct,
            market_value: m.market_value,
            unrealized_pnl: m.unrealized_pnl,
            cagr_pct: m.cagr_pct,
            simple_interest_value: m.simple_interest_value,
            years_held: m.years_held,
        });
    }
    Ok(views)
}

/// Exports every column shown on the Mutual Funds screen as CSV text, same
/// "reuse the list command directly" guarantee as export_holdings_csv.
#[tauri::command]
async fn export_mf_csv(state: State<'_, AppState>, portfolio_id: String, si_rate_pct: Option<f64>) -> Result<String, String> {
    let funds = list_mutual_funds(state.clone(), portfolio_id, si_rate_pct).await?;
    let mut out = String::from(
        "SchemeCode,SchemeName,Category,Units,AvgNAV,PreviousNAV,CurrentNAV,NAVChangePct,MarketValue,UnrealizedPnl,CAGRPct,SimpleInterestValue,YearsHeld\n",
    );
    for f in &funds {
        out.push_str(&format!(
            "{},{},{},{},{},{},{},{},{},{},{},{},{}\n",
            f.scheme_code,
            f.scheme_name,
            f.category.clone().unwrap_or_default(),
            f.units,
            f.avg_nav,
            f.previous_nav.clone().unwrap_or_default(),
            f.current_nav.clone().unwrap_or_default(),
            f.nav_change_pct.map(|p| format!("{:.4}", p * 100.0)).unwrap_or_default(),
            f.market_value.clone().unwrap_or_default(),
            f.unrealized_pnl.clone().unwrap_or_default(),
            f.cagr_pct.map(|p| format!("{p:.2}")).unwrap_or_default(),
            f.simple_interest_value.clone().unwrap_or_default(),
            f.years_held.map(|y| format!("{y:.2}")).unwrap_or_default(),
        ));
    }
    Ok(out)
}

/// Parses "SchemeCode,Units,BuyNAV,BuyDate" rows — the AMFI Scheme Code is
/// required (not the fund name, for the same reason add_mutual_fund and
/// search_mf_schemes exist: fund names collide constantly across
/// Direct/Regular and Growth/IDCW variants). A scheme not present in the
/// cache fails that row with a clear "run Refresh Fund List first" message
/// rather than guessing.
#[tauri::command]
async fn import_mf_csv(state: State<'_, AppState>, portfolio_id: String, csv_content: String) -> Result<ImportCsvResult, String> {
    let portfolio_id_uuid = parse_portfolio_id(&portfolio_id)?;
    let default_buy_date = ist_today() - chrono::Duration::days(365);

    let mut rows_out = Vec::new();
    let mut imported = 0usize;
    let mut failed = 0usize;

    let lines: Vec<&str> = csv_content.lines().collect();
    let data_lines = if lines.first().map(|l| l.to_uppercase().starts_with("SCHEMECODE")).unwrap_or(false) {
        &lines[1..]
    } else {
        &lines[..]
    };

    for (i, line) in data_lines.iter().enumerate() {
        let row_number = i + 2;
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let cols: Vec<&str> = line.split(',').map(|c| c.trim()).collect();
        if cols.len() < 3 {
            failed += 1;
            rows_out.push(ImportRowResult { row_number, symbol: line.to_string(), status: "expected at least SchemeCode,Units,BuyNAV".to_string() });
            continue;
        }
        let scheme_code = cols[0].to_string();

        let outcome: Result<(), String> = async {
            let units = Decimal::from_str(cols[1]).map_err(|e| format!("bad units: {e}"))?;
            let buy_nav = Decimal::from_str(cols[2]).map_err(|e| format!("bad NAV: {e}"))?;
            let buy_date = match cols.get(3).map(|s| s.trim()).filter(|s| !s.is_empty()) {
                Some(s) => NaiveDate::parse_from_str(s, "%Y-%m-%d").map_err(|e| format!("bad date '{s}': {e}"))?,
                None => default_buy_date,
            };

            let instrument = match state.instruments.find_by_symbol(&scheme_code).await.map_err(|e| e.to_string())? {
                Some(existing) => existing,
                None => {
                    let view = add_mutual_fund(state.clone(), scheme_code.clone()).await?;
                    state
                        .instruments
                        .find_by_symbol(&view.symbol)
                        .await
                        .map_err(|e| e.to_string())?
                        .ok_or_else(|| "fund was just added but couldn't be re-fetched".to_string())?
                }
            };

            let txn = Transaction {
                id: Uuid::new_v4(),
                portfolio_id: portfolio_id_uuid,
                instrument_id: instrument.id,
                transaction_type: TransactionType::Buy,
                quantity: units,
                price: Money::inr(buy_nav),
                fees: Money::inr(Decimal::ZERO),
                trade_date: buy_date,
                broker_ref: None,
                recorded_at: chrono::Utc::now(),
            };
            let use_case = RecordTransactionUseCase::new(state.transactions.clone(), state.holdings.clone());
            use_case.execute(txn).await.map_err(|e| e.to_string())?;
            Ok(())
        }
        .await;

        match outcome {
            Ok(()) => {
                imported += 1;
                rows_out.push(ImportRowResult { row_number, symbol: scheme_code, status: "Imported".to_string() });
            }
            Err(e) => {
                failed += 1;
                rows_out.push(ImportRowResult { row_number, symbol: scheme_code, status: e });
            }
        }
    }
    Ok(ImportCsvResult { imported, failed, rows: rows_out })
}

/// Refreshes NAV for every fund actually held in this portfolio, from the
/// (already-refreshed) scheme cache into permanent price_history — mirrors
/// refresh_prices for equities. Does NOT refresh the scheme cache itself;
/// call refresh_mf_scheme_cache first (the UI does this as one "Refresh"
/// action, but they're separate commands since one is a network fetch of
/// the whole market and the other is just copying cached values forward).
#[tauri::command]
async fn refresh_mf_nav(state: State<'_, AppState>, portfolio_id: String) -> Result<RefreshPricesResult, String> {
    let portfolio_id = parse_portfolio_id(&portfolio_id)?;
    let holdings = state.holdings.list_for_portfolio(portfolio_id).await.map_err(|e| e.to_string())?;

    let mut updated = Vec::new();
    let mut failed = Vec::new();
    for h in holdings {
        let instrument = match state.instruments.get(h.instrument_id).await {
            Ok(i) if i.asset_class == AssetClass::MutualFund => i,
            Ok(_) => continue, // not a fund, skip silently — this command only touches MFs
            Err(e) => {
                failed.push(RefreshFailure { symbol: h.instrument_id.to_string(), reason: e.to_string() });
                continue;
            }
        };
        match state.mf_scheme_cache.get_by_code(&instrument.symbol).await {
            Ok(Some(scheme)) => {
                if let Err(e) = state.prices.upsert_daily_bar(instrument.id, scheme.nav_date, scheme.nav).await {
                    failed.push(RefreshFailure { symbol: instrument.symbol, reason: e.to_string() });
                } else {
                    updated.push(instrument.symbol);
                }
            }
            Ok(None) => failed.push(RefreshFailure {
                symbol: instrument.symbol,
                reason: "not in scheme cache — try Refresh Fund List first".to_string(),
            }),
            Err(e) => failed.push(RefreshFailure { symbol: instrument.symbol, reason: e.to_string() }),
        }
    }
    Ok(RefreshPricesResult { updated, failed })
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
        trade_date: ist_today(),
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

/// Whole-portfolio XIRR for the Dashboard (SRS 2.2.3) — combines every
/// transaction across every held instrument into one cashflow series. See
/// the doc comment on ComputeXirrUseCase::execute_for_portfolio for the one
/// simplification worth knowing about (an unpriced holding contributes
/// zero to the final mark-to-market cashflow rather than failing outright).
#[tauri::command]
async fn compute_portfolio_xirr(state: State<'_, AppState>, portfolio_id: String) -> Result<f64, String> {
    let portfolio_id = parse_portfolio_id(&portfolio_id)?;
    let use_case = ComputeXirrUseCase::new(state.transactions.clone(), state.prices.clone());
    use_case.execute_for_portfolio(portfolio_id).await.map_err(|e| e.to_string())
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
        display_name: None,
    };
    let tcs = Instrument {
        id: Uuid::new_v4(),
        isin: Isin::parse("INE467B01029").unwrap(),
        symbol: "TCS".to_string(),
        asset_class: AssetClass::Equity,
        exchange: "NSE".to_string(),
        sector: Some("IT".to_string()),
        display_name: None,
    };

    state.instruments.upsert(&reliance).await.map_err(|e| e.to_string())?;
    state.instruments.upsert(&tcs).await.map_err(|e| e.to_string())?;

    // Real Yahoo data, not the old synthetic random-walk seed — see the
    // doc comment on backfill_instrument_history for why that mattered.
    // Best-effort: if this fails (offline first launch, rate limit), the
    // demo instruments just start with no price history, same as any
    // manually-added ticker whose backfill hasn't run yet — not a reason
    // to fail app startup.
    let _ = backfill_instrument_history(state, &reliance).await;
    let _ = backfill_instrument_history(state, &tcs).await;

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
            trade_date: ist_today(),
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
            trade_date: ist_today(),
            broker_ref: None,
            recorded_at: chrono::Utc::now(),
        })
        .await
        .map_err(|e| e.to_string())?;

    Ok(())
}

fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_process::init())
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

            let app_settings_repo = Arc::new(SqliteAppSettings::new(pool.clone()));
            let market_data_provider = Arc::new(CompositeMarketDataProvider::new(
                YahooFinanceProvider::new(),
                Some(AlphaVantageProvider::new(app_settings_repo.clone())),
            ));

            let state = AppState {
                pool: pool.clone(),
                portfolios: Arc::new(SqlitePortfolioRepository::new(pool.clone())),
                transactions: Arc::new(SqliteTransactionRepository::new(pool.clone())),
                holdings: Arc::new(SqliteHoldingRepository::new(pool.clone())),
                instruments: Arc::new(SqliteInstrumentRepository::new(pool.clone())),
                prices: Arc::new(SqlitePriceRepository::new(pool.clone())),
                alert_rules: Arc::new(SqliteAlertRuleRepository::new(pool.clone())),
                market_data: market_data_provider,
                yahoo_direct: Arc::new(YahooFinanceProvider::new()),
                mf_scheme_cache: Arc::new(SqliteMfSchemeCache::new(pool)),
                mf_data_source: Arc::new(AmfiProvider::new()),
                app_settings: app_settings_repo,
            };

            tauri::async_runtime::block_on(seed_demo_data_if_first_launch(&state))
                .expect("demo data seeding failed");

            app.manage(state);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            list_portfolios,
            create_portfolio,
            delete_portfolio,
            get_dashboard_summary,
            get_dashboard_by_market,
            list_holdings,
            list_instruments,
            list_equity_instruments,
            get_fundamentals,
            get_stock_news,
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
            create_alert_rule,
            list_alert_rules,
            delete_alert_rule,
            import_holdings_csv,
            export_holdings_csv,
            get_ohlc_history,
            compute_portfolio_xirr,
            reset_all_data,
            save_alpha_vantage_key,
            has_alpha_vantage_key,
            refresh_mf_scheme_cache,
            search_mf_schemes,
            add_mutual_fund,
            list_mutual_funds,
            refresh_mf_nav,
            import_mf_csv,
            export_mf_csv
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

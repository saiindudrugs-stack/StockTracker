// These mirror the #[derive(Serialize)] structs in src-tauri/src/main.rs
// exactly. If a Rust field is renamed, this file goes out of sync silently
// (Tauri's IPC has no shared schema check) — worth revisiting with a
// codegen step (e.g. specta) once the command surface stops changing every
// few days.

export interface PortfolioView {
  id: string;
  name: string;
}

export interface DashboardSummary {
  net_worth: string;
  overall_unrealized_pnl: string;
  overall_realized_pnl: string;
  holdings_priced: number;
  holdings_missing_price: number;
}

export interface HoldingView {
  symbol: string;
  sector: string | null;
  quantity: string;
  avg_cost: string;
  last_price: string | null;
  previous_close: string | null;
  market_value: string | null;
  unrealized_pnl: string | null;
  day_change_pct: number | null;
  // CAGR: plain point-to-point return since the earliest Buy of this stock
  // in this portfolio, annualized — different from XIRR (money-weighted,
  // accounts for timing of each cashflow). null when there's no Buy date
  // yet to measure from.
  cagr_pct: number | null;
  // What the invested capital would be worth today at a flat 9.5% simple
  // interest (no compounding) over the same holding period — a benchmark
  // to compare against, not a real investment return.
  simple_interest_value_at_9_5_pct: string | null;
  years_held: number | null;
}

export interface InstrumentView {
  symbol: string;
  sector: string | null;
}

export interface PriceHistoryPoint {
  date: string;
  close: string;
}

export interface CandleView {
  date: string;
  open: string;
  high: string;
  low: string;
  close: string;
  volume: number | null;
}

export interface RefreshFailure {
  symbol: string;
  reason: string;
}

export interface RefreshPricesResult {
  updated: string[];
  failed: RefreshFailure[];
}

export interface MarketSnapshotView {
  symbol: string;
  price: string;
  previous_close: string | null;
  day_high: string | null;
  day_low: string | null;
  week52_high: string | null;
  week52_low: string | null;
  volume: number | null;
  day_change_pct: number | null;
}

export interface TechnicalAnalysisView {
  phase: string;
  latest_close: number;
  sma_10: number | null;
  sma_20: number | null;
  sma_50: number | null;
  rsi_14: number | null;
  annualized_return_pct: number | null;
  annualized_volatility_pct: number | null;
  historical_var_95_pct: number | null;
  // Fibonacci-retracement confluence signal — a rules-based technical-
  // analysis heuristic, not financial advice. See the honesty note in
  // crates/domain/src/analytics/signal.rs.
  recommendation: string | null;
  recommendation_reasons: string[];
  nearest_fib_label: string | null;
  nearest_fib_price: number | null;
}

export interface StockRiskReturn {
  symbol: string;
  annualized_return_pct: number;
  annualized_volatility_pct: number;
  risk_label: string;
}

export interface CorrelationPair {
  symbol_a: string;
  symbol_b: string;
  correlation: number;
}

export interface PortfolioAnalysisView {
  stocks: StockRiskReturn[];
  correlations: CorrelationPair[];
  skipped: RefreshFailure[];
}

export interface AlertRuleView {
  id: string;
  symbol: string;
  condition: "stop_loss" | "target";
  threshold_price: string;
  triggered: boolean;
  is_triggered_now: boolean;
  current_price: string | null;
}

export type ScreenId = "dashboard" | "holdings" | "watchlist" | "analysis" | "chart" | "settings";

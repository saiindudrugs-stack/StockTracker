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
  market_value: string | null;
  unrealized_pnl: string | null;
  day_change_pct: number | null;
}

export interface InstrumentView {
  symbol: string;
  sector: string | null;
}

export interface PriceHistoryPoint {
  date: string;
  close: string;
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
  day_high: string | null;
  day_low: string | null;
  week52_high: string | null;
  week52_low: string | null;
  volume: number | null;
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

export type ScreenId = "dashboard" | "holdings" | "watchlist" | "analysis" | "chart" | "settings";

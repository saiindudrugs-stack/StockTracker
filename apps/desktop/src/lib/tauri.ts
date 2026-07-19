import { invoke } from "@tauri-apps/api/core";
import type {
  DashboardSummary,
  HoldingView,
  InstrumentView,
  MarketSnapshotView,
  PortfolioAnalysisView,
  PortfolioView,
  PriceHistoryPoint,
  RefreshPricesResult,
  TechnicalAnalysisView,
} from "./types";

// One function per backend command, typed — callers never touch the raw
// `invoke` string-based API directly. Keeping this list in sync with the
// #[tauri::command] functions in main.rs and the invoke_handler![] list is
// manual for now (see the note in types.ts).
export const api = {
  listPortfolios: () => invoke<PortfolioView[]>("list_portfolios"),
  createPortfolio: (name: string) => invoke<PortfolioView>("create_portfolio", { name }),

  getDashboardSummary: (portfolioId: string) =>
    invoke<DashboardSummary>("get_dashboard_summary", { portfolioId }),
  listHoldings: (portfolioId: string) => invoke<HoldingView[]>("list_holdings", { portfolioId }),
  recordBuy: (portfolioId: string, symbol: string, quantity: string, price: string) =>
    invoke<void>("record_buy", { portfolioId, symbol, quantity, price }),
  recordSell: (portfolioId: string, symbol: string, quantity: string, price: string) =>
    invoke<void>("record_sell", { portfolioId, symbol, quantity, price }),
  computeXirrForSymbol: (portfolioId: string, symbol: string) =>
    invoke<number>("compute_xirr_for_symbol", { portfolioId, symbol }),
  // Unofficial Yahoo Finance pull — see the honesty note in
  // crates/infrastructure/src/market_data/mod.rs. Can fail per-symbol
  // without failing the whole refresh; check `.failed` on the result.
  refreshPrices: (portfolioId: string) =>
    invoke<RefreshPricesResult>("refresh_prices", { portfolioId }),

  // Instruments and prices are shared reference data, not portfolio-scoped.
  listInstruments: () => invoke<InstrumentView[]>("list_instruments"),
  addInstrument: (symbol: string) => invoke<InstrumentView>("add_instrument", { symbol }),
  getPriceHistory: (symbol: string) => invoke<PriceHistoryPoint[]>("get_price_history", { symbol }),

  // Works for ANY tracked instrument, held or not — this is what makes a
  // watchlist (tracking before buying) possible without a portfolio_id.
  getMarketSnapshot: (symbol: string) => invoke<MarketSnapshotView>("get_market_snapshot", { symbol }),
  // Heavier call (needs a year of daily history) — trigger on demand, not
  // on every auto-refresh tick.
  analyzeMarketPhase: (symbol: string) => invoke<TechnicalAnalysisView>("analyze_market_phase", { symbol }),
  // Same heavier-call caveat as above, run once per held stock — a
  // deliberate "run my analysis" action, not automatic.
  getPortfolioAnalysis: (portfolioId: string) =>
    invoke<PortfolioAnalysisView>("get_portfolio_analysis", { portfolioId }),
};

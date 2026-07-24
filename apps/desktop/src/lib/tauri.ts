import { invoke } from "@tauri-apps/api/core";
import type {
  CandleView,
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
  listHoldings: (portfolioId: string, siRatePct?: number) =>
    invoke<HoldingView[]>("list_holdings", { portfolioId, siRatePct }),
  recordBuy: (portfolioId: string, symbol: string, quantity: string, price: string) =>
    invoke<void>("record_buy", { portfolioId, symbol, quantity, price }),
  recordSell: (portfolioId: string, symbol: string, quantity: string, price: string) =>
    invoke<void>("record_sell", { portfolioId, symbol, quantity, price }),
  // csvContent is the raw file text, read client-side via FileReader — no
  // file-path plumbing needed since Tauri commands take plain strings.
  importHoldingsCsv: (portfolioId: string, csvContent: string) =>
    invoke<{ imported: number; failed: number; rows: { row_number: number; symbol: string; status: string }[] }>(
      "import_holdings_csv",
      { portfolioId, csvContent }
    ),
  // Returns raw CSV text — the frontend turns it into a downloadable file
  // via a Blob, same trick already used for the CSV template download.
  exportHoldingsCsv: (portfolioId: string, siRatePct?: number) =>
    invoke<string>("export_holdings_csv", { portfolioId, siRatePct }),
  computeXirrForSymbol: (portfolioId: string, symbol: string) =>
    invoke<number>("compute_xirr_for_symbol", { portfolioId, symbol }),
  computePortfolioXirr: (portfolioId: string) => invoke<number>("compute_portfolio_xirr", { portfolioId }),
  // Unofficial Yahoo Finance pull — see the honesty note in
  // crates/infrastructure/src/market_data/mod.rs. Can fail per-symbol
  // without failing the whole refresh; check `.failed` on the result.
  refreshPrices: (portfolioId: string) =>
    invoke<RefreshPricesResult>("refresh_prices", { portfolioId }),

  // Instruments and prices are shared reference data, not portfolio-scoped.
  listInstruments: () => invoke<InstrumentView[]>("list_instruments"),
  addInstrument: (symbol: string) => invoke<InstrumentView>("add_instrument", { symbol }),
  // Downloads a real year of Yahoo daily history into local storage —
  // needed because a freshly-added ticker (or the two synthetic-seeded
  // demo instruments) otherwise has little to no real chart data.
  backfillHistory: (symbol: string) => invoke<{ symbol: string; days_backfilled: number }>("backfill_history", { symbol }),
  getPriceHistory: (symbol: string) => invoke<PriceHistoryPoint[]>("get_price_history", { symbol }),
  getOhlcHistory: (symbol: string) => invoke<CandleView[]>("get_ohlc_history", { symbol }),

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

  // Danger zone — wipes every portfolio, holding, transaction, and cached
  // price. Backed by reset_all_data in main.rs.
  resetAllData: () => invoke<void>("reset_all_data"),

  // Row-level removal, deliberately scoped differently:
  // removeHolding only clears this portfolio's position (transactions +
  // snapshot) for that stock — the ticker itself stays tracked elsewhere.
  // removeFromWatchlist deletes the shared instrument entirely, and the
  // backend rejects it if any portfolio still holds a non-zero quantity.
  removeHolding: (portfolioId: string, symbol: string) => invoke<void>("remove_holding", { portfolioId, symbol }),
  removeFromWatchlist: (symbol: string) => invoke<void>("remove_from_watchlist", { symbol }),
};

import { invoke } from "@tauri-apps/api/core";
import type { DashboardSummary, HoldingView, InstrumentView, PortfolioView, PriceHistoryPoint } from "./types";

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
  computeXirrForSymbol: (portfolioId: string, symbol: string) =>
    invoke<number>("compute_xirr_for_symbol", { portfolioId, symbol }),

  // Instruments and prices are shared reference data, not portfolio-scoped.
  listInstruments: () => invoke<InstrumentView[]>("list_instruments"),
  addInstrument: (symbol: string) => invoke<InstrumentView>("add_instrument", { symbol }),
  getPriceHistory: (symbol: string) => invoke<PriceHistoryPoint[]>("get_price_history", { symbol }),
};

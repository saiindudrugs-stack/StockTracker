import { invoke } from "@tauri-apps/api/core";
import type { DashboardSummary, HoldingView, InstrumentView, PriceHistoryPoint } from "./types";

// One function per backend command, typed — callers never touch the raw
// `invoke` string-based API directly. Keeping this list in sync with the
// #[tauri::command] functions in main.rs and the invoke_handler![] list is
// manual for now (see the note in types.ts).
export const api = {
  getDashboardSummary: () => invoke<DashboardSummary>("get_dashboard_summary"),
  listHoldings: () => invoke<HoldingView[]>("list_holdings"),
  listInstruments: () => invoke<InstrumentView[]>("list_instruments"),
  getPriceHistory: (symbol: string) => invoke<PriceHistoryPoint[]>("get_price_history", { symbol }),
  recordBuy: (symbol: string, quantity: string, price: string) =>
    invoke<void>("record_buy", { symbol, quantity, price }),
  computeXirrForSymbol: (symbol: string) => invoke<number>("compute_xirr_for_symbol", { symbol }),
};

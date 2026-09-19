import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import type {
  AlertRuleView,
  CandleView,
  DashboardSummary,
  HoldingView,
  InstrumentView,
  MarketSnapshotView,
  MarketSummaryView,
  PortfolioSummaryRow,
  TaxSummaryView,
  TaxLossHarvestingView,
  MfHoldingView,
  MfSchemeSearchResultView,
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
  // Deletes the portfolio AND everything scoped to it (transactions,
  // holdings, alert rules) — real, permanent data loss. See the doc
  // comment on delete_portfolio in main.rs for why this is safe re:
  // shared instruments (it never touches those).
  deletePortfolio: (portfolioId: string) => invoke<void>("delete_portfolio", { portfolioId }),

  getDashboardSummary: (portfolioId: string) =>
    invoke<DashboardSummary>("get_dashboard_summary", { portfolioId }),
  getDashboardByMarket: (portfolioId: string) =>
    invoke<MarketSummaryView[]>("get_dashboard_by_market", { portfolioId }),
  getAllPortfoliosSummary: () => invoke<PortfolioSummaryRow[]>("get_all_portfolios_summary"),
  getAllHeldSymbols: () => invoke<string[]>("get_all_held_symbols"),
  getTaxSummary: (portfolioId: string) => invoke<TaxSummaryView>("get_tax_summary", { portfolioId }),
  getTaxLossHarvestingCandidates: (portfolioId: string) =>
    invoke<TaxLossHarvestingView>("get_tax_loss_harvesting_candidates", { portfolioId }),
  saveTargetAllocation: (allocationJson: string) => invoke<void>("save_target_allocation", { allocationJson }),
  getTargetAllocation: () => invoke<string>("get_target_allocation"),
  listHoldings: (portfolioId: string, siRatePct?: number) =>
    invoke<HoldingView[]>("list_holdings", { portfolioId, siRatePct }),
  recordBuy: (portfolioId: string, symbol: string, quantity: string, price: string) =>
    invoke<void>("record_buy", { portfolioId, symbol, quantity, price }),
  recordSell: (portfolioId: string, symbol: string, quantity: string, price: string) =>
    invoke<void>("record_sell", { portfolioId, symbol, quantity, price }),
  // sharesHeld/dividendPerShare, not quantity/price — matches the Rust
  // command's own parameter names, since a dividend's two numbers mean
  // something different from a Buy/Sell's.
  recordDividend: (portfolioId: string, symbol: string, sharesHeld: string, dividendPerShare: string) =>
    invoke<void>("record_dividend", { portfolioId, symbol, sharesHeld, dividendPerShare }),
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
  addInstrument: (symbol: string, exchange?: string) => invoke<InstrumentView>("add_instrument", { symbol, exchange }),
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
  removeNonIndianInstruments: () => invoke<{ removed: string[]; kept: string[] }>("remove_non_indian_instruments"),

  // Stop-loss / target alerter. condition is "stop_loss" (fires at or
  // below threshold) or "target" (fires at or above). Trigger status is
  // recomputed live every time listAlertRules is called — see the doc
  // comment on list_alert_rules in main.rs.
  createAlertRule: (portfolioId: string, symbol: string, condition: "stop_loss" | "target", thresholdPrice: string) =>
    invoke<void>("create_alert_rule", { portfolioId, symbol, condition, thresholdPrice }),
  listAlertRules: (portfolioId: string) => invoke<AlertRuleView[]>("list_alert_rules", { portfolioId }),
  deleteAlertRule: (id: string) => invoke<void>("delete_alert_rule", { id }),

  // Mutual funds — a fully separate data path from equities. The scheme
  // cache (searchMfSchemes reads from it) is disposable and wholesale-
  // replaced by refreshMfSchemeCache; it holds no data about what you
  // actually own. What you own lives in the same portfolio/holding tables
  // as equities, just filtered to asset_class = mutual_fund server-side.
  refreshMfSchemeCache: () => invoke<{ scheme_count: number }>("refresh_mf_scheme_cache"),
  searchMfSchemes: (query: string) => invoke<MfSchemeSearchResultView[]>("search_mf_schemes", { query }),
  addMutualFund: (schemeCode: string) => invoke<InstrumentView>("add_mutual_fund", { schemeCode }),
  listMutualFunds: (portfolioId: string, siRatePct?: number) =>
    invoke<MfHoldingView[]>("list_mutual_funds", { portfolioId, siRatePct }),
  refreshMfNav: (portfolioId: string) => invoke<RefreshPricesResult>("refresh_mf_nav", { portfolioId }),
  importMfCsv: (portfolioId: string, csvContent: string) =>
    invoke<{ imported: number; failed: number; rows: { row_number: number; symbol: string; status: string }[] }>(
      "import_mf_csv",
      { portfolioId, csvContent }
    ),
  exportMfCsv: (portfolioId: string, siRatePct?: number) => invoke<string>("export_mf_csv", { portfolioId, siRatePct }),

  saveUpstoxToken: (token: string) => invoke<void>("save_upstox_token", { token }),
  hasUpstoxToken: () => invoke<boolean>("has_upstox_token"),
  refreshUpstoxInstrumentCache: () => invoke<{ instrument_count: number }>("refresh_upstox_instrument_cache"),

  saveMarketDataPriority: (order: string) => invoke<void>("save_market_data_priority", { order }),
  getMarketDataPriority: () => invoke<string>("get_market_data_priority_order"),

  saveFlashThreshold: (thresholdPct: number) => invoke<void>("save_flash_threshold", { thresholdPct }),
  getFlashThreshold: () => invoke<number>("get_flash_threshold"),

  saveFontScale: (scalePct: number) => invoke<void>("save_font_scale", { scalePct }),
  getFontScale: () => invoke<number>("get_font_scale"),

  // Real connection tests — a saved key proves nothing about whether it's
  // actually valid, so these make one small live call per source rather
  // than just checking "is something saved." Explicit only (a button
  // click), never automatic.
  testMarketDataConnection: (provider: string) => invoke<string>("test_market_data_connection", { provider }),
  testAnnouncementsConnection: (provider: string) => invoke<string>("test_announcements_connection", { provider }),

  // Live streaming — opt-in, explicit start/stop. Resolves each symbol's
  // Upstox instrument_key (needs the instrument cache refreshed first)
  // and returns how many actually resolved, so the caller can tell the
  // user if some symbols were skipped. Ticks arrive via the
  // "live-price-tick" window event, not a return value — listen for that
  // separately (see subscribeLivePriceTicks below).
  startLivePriceStream: (symbols: string[]) => invoke<number>("start_live_price_stream", { symbols }),
  stopLivePriceStream: () => invoke<void>("stop_live_price_stream"),

  // Zerodha — a real daily OAuth flow, not a long-lived token like
  // Upstox. connectZerodha() opens the system browser and waits for the
  // login redirect via a listener scoped to that one call only (see the
  // Rust module doc comment on kite_auth.rs for the exact lifecycle
  // guarantee — it never runs unless this is called, and never outlives
  // the call).
  saveZerodhaCredentials: (apiKey: string, apiSecret: string) => invoke<void>("save_zerodha_credentials", { apiKey, apiSecret }),
  hasZerodhaCredentials: () => invoke<boolean>("has_zerodha_credentials"),
  hasValidZerodhaSession: () => invoke<boolean>("has_valid_zerodha_session"),
  connectZerodha: () => invoke<string>("connect_zerodha"),
  refreshKiteInstrumentCache: () => invoke<{ instrument_count: number }>("refresh_kite_instrument_cache"),
  startZerodhaLiveStream: (symbols: string[]) => invoke<number>("start_zerodha_live_stream", { symbols }),
  testAiProviderConnection: (provider: string) => invoke<string>("test_ai_provider_connection", { provider }),

  // AI portfolio insights — anthropic/openai/gemini, whichever the user
  // configures and explicitly picks. Every call is a deliberate, visible
  // action (see the "Get Insights" button flow), never automatic.
  saveAiProviderKey: (provider: string, key: string) => invoke<void>("save_ai_provider_key", { provider, key }),
  hasAiProviderKey: (provider: string) => invoke<boolean>("has_ai_provider_key", { provider }),
  saveAiProviderModel: (provider: string, model: string) => invoke<void>("save_ai_provider_model", { provider, model }),
  getAiProviderModel: (provider: string) => invoke<string>("get_ai_provider_model", { provider }),
  generatePortfolioInsights: (portfolioId: string, provider: string) =>
    invoke<string>("generate_portfolio_insights", { portfolioId, provider }),

  // News & Fundamentals — equities only (no mutual funds). Fundamentals
  // and news are properties of the company, not any one portfolio, same
  // reasoning as getMarketSnapshot.
  listEquityInstruments: () => invoke<InstrumentView[]>("list_equity_instruments"),
};

export interface LivePriceTick {
  symbol: string;
  price: number;
}

/// Subscribes to the "live-price-tick" backend event (emitted once per
/// tick while a stream started via api.startLivePriceStream is running).
/// Returns an unsubscribe function — call it on unmount, same pattern as
/// a plain useEffect cleanup, so a screen navigated away from doesn't
/// keep reacting to ticks after it's gone.
export function subscribeLivePriceTicks(onTick: (tick: LivePriceTick) => void): () => void {
  let unlisten: (() => void) | null = null;
  let cancelled = false;
  listen<LivePriceTick>("live-price-tick", (event) => onTick(event.payload)).then((fn) => {
    if (cancelled) {
      fn();
    } else {
      unlisten = fn;
    }
  });
  return () => {
    cancelled = true;
    if (unlisten) unlisten();
  };
}

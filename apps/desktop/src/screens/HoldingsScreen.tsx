import { useEffect, useRef, useState } from "react";
import { api } from "../lib/tauri";
import type { HoldingView, InstrumentView } from "../lib/types";
import { colors, panelStyle } from "../lib/theme";

type Tab = "long_term" | "intraday";
type TxnType = "buy" | "sell";

const AUTO_REFRESH_MS = 30_000;

function pnlColor(value: number): string {
  if (value > 0) return colors.success;
  if (value < 0) return colors.danger;
  return colors.textMuted;
}

function parseNumeric(s: string | null): number {
  if (s === null) return 0;
  const n = parseFloat(s);
  return Number.isFinite(n) ? n : 0;
}

export function HoldingsScreen({ portfolioId }: { portfolioId: string }) {
  const [tab, setTab] = useState<Tab>("long_term");
  const [holdings, setHoldings] = useState<HoldingView[]>([]);
  const [instruments, setInstruments] = useState<InstrumentView[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [txnType, setTxnType] = useState<TxnType>("buy");
  const [symbol, setSymbol] = useState<string>("");
  const [qty, setQty] = useState("5");
  const [price, setPrice] = useState("");
  const [newTicker, setNewTicker] = useState("");
  const [xirrResult, setXirrResult] = useState<string | null>(null);
  const [refreshing, setRefreshing] = useState(false);
  const [refreshMsg, setRefreshMsg] = useState<string | null>(null);
  const [autoRefresh, setAutoRefresh] = useState(false);
  const intervalRef = useRef<ReturnType<typeof setInterval> | null>(null);

  async function refreshHoldings() {
    try {
      setHoldings(await api.listHoldings(portfolioId));
      setError(null);
    } catch (e) {
      setError(String(e));
    }
  }

  async function refreshInstruments() {
    try {
      const list = await api.listInstruments();
      setInstruments(list);
      if (!symbol && list.length > 0) setSymbol(list[0].symbol);
    } catch (e) {
      setError(String(e));
    }
  }

  useEffect(() => {
    refreshHoldings();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [portfolioId]);

  useEffect(() => {
    refreshInstruments();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  async function handleRefreshPrices() {
    setRefreshing(true);
    setRefreshMsg(null);
    try {
      const result = await api.refreshPrices(portfolioId);
      await refreshHoldings();
      const parts: string[] = [];
      if (result.updated.length > 0) parts.push(`Updated: ${result.updated.join(", ")}`);
      if (result.failed.length > 0) {
        parts.push(`Failed: ${result.failed.map((f) => `${f.symbol} (${f.reason})`).join("; ")}`);
      }
      setRefreshMsg(parts.length > 0 ? parts.join(" — ") : "No holdings to refresh.");
    } catch (e) {
      setRefreshMsg(String(e));
    } finally {
      setRefreshing(false);
    }
  }

  // Auto-refresh: opt-in only, and deliberately not the default — hitting
  // an unofficial, unsupported endpoint every 30s is a materially different
  // risk (rate-limiting, IP blocks) than a manual click now and then. The
  // interval is cleared on unmount and whenever the toggle or portfolio
  // changes, so switching family portfolios never leaves a stray timer
  // hammering prices for a screen that's no longer visible.
  useEffect(() => {
    if (autoRefresh) {
      intervalRef.current = setInterval(() => {
        handleRefreshPrices();
      }, AUTO_REFRESH_MS);
    }
    return () => {
      if (intervalRef.current) clearInterval(intervalRef.current);
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [autoRefresh, portfolioId]);

  async function handleAddTicker() {
    const trimmed = newTicker.trim();
    if (!trimmed) return;
    try {
      const added = await api.addInstrument(trimmed);
      setNewTicker("");
      await refreshInstruments();
      setSymbol(added.symbol);
      setError(null);
    } catch (e) {
      setError(String(e));
    }
  }

  async function handleRecordTransaction() {
    if (!symbol || !price) {
      setError("Pick a ticker and enter a price first.");
      return;
    }
    try {
      if (txnType === "buy") {
        await api.recordBuy(portfolioId, symbol, qty, price);
      } else {
        await api.recordSell(portfolioId, symbol, qty, price);
      }
      await refreshHoldings();
      setError(null);
    } catch (e) {
      // A sell that overdraws the position surfaces here as a plain error
      // string from the Rust side (RecordTransactionUseCase rejects it
      // before it ever reaches the ledger) — not a silent no-op.
      setError(String(e));
    }
  }

  async function handleXirr(sym: string) {
    try {
      const rate = await api.computeXirrForSymbol(portfolioId, sym);
      setXirrResult(`${sym}: ${(rate * 100).toFixed(2)}% XIRR`);
    } catch (e) {
      setXirrResult(`${sym}: ${String(e)}`);
    }
  }

  return (
    <div style={{ padding: 24 }}>
      <h1 style={{ fontSize: 20, color: colors.navy, marginBottom: 4 }}>Holdings</h1>
      <p style={{ fontSize: 13, color: colors.textMuted, marginTop: 0 }}>
        Split into two tabs deliberately, per the wireframe — intraday and long-term positions
        settle differently (same-day close vs. tax-lot tracking) and shouldn't be confused with
        each other. This tab's holdings belong only to the portfolio selected above.
      </p>

      <div style={{ display: "flex", gap: 8, marginBottom: 16 }}>
        <button
          onClick={() => setTab("long_term")}
          style={{
            fontSize: 12,
            padding: "5px 12px",
            borderRadius: 6,
            border: `1px solid ${tab === "long_term" ? colors.accent : colors.border}`,
            background: tab === "long_term" ? colors.surface : "transparent",
            color: tab === "long_term" ? colors.accent : colors.textMuted,
            cursor: "pointer",
          }}
        >
          Long-term
        </button>
        <button
          onClick={() => setTab("intraday")}
          style={{
            fontSize: 12,
            padding: "5px 12px",
            borderRadius: 6,
            border: `1px solid ${tab === "intraday" ? colors.accent : colors.border}`,
            background: tab === "intraday" ? colors.surface : "transparent",
            color: tab === "intraday" ? colors.accent : colors.textMuted,
            cursor: "pointer",
          }}
        >
          Intraday (today)
        </button>
      </div>

      {error && <p style={{ color: colors.danger }}>{error}</p>}

      {tab === "long_term" ? (
        <>
          <div style={{ display: "flex", alignItems: "center", gap: 10, marginBottom: 4, flexWrap: "wrap" }}>
            <button onClick={handleRefreshPrices} disabled={refreshing}>
              {refreshing ? "Refreshing…" : "Refresh Prices"}
            </button>
            <label style={{ fontSize: 12, display: "flex", alignItems: "center", gap: 5, cursor: "pointer" }}>
              <input type="checkbox" checked={autoRefresh} onChange={(e) => setAutoRefresh(e.target.checked)} />
              Auto-refresh every 30s
            </label>
          </div>
          <p style={{ fontSize: 11, color: colors.textMuted, marginTop: 0, marginBottom: 10 }}>
            Pulls from an unofficial Yahoo Finance endpoint — free, but unsupported by Yahoo and
            could break or get rate-limited without notice, especially with auto-refresh left on.
            Not a real-time feed.
          </p>
          {refreshMsg && (
            <p style={{ fontSize: 12, marginBottom: 12, color: refreshMsg.startsWith("Failed") ? colors.danger : colors.textMuted }}>
              {refreshMsg}
            </p>
          )}

          <table style={{ borderCollapse: "collapse", width: "100%", fontSize: 13 }}>
            <thead>
              <tr style={{ textAlign: "left", borderBottom: `1px solid ${colors.border}` }}>
                <th style={{ padding: "6px 8px 6px 0" }}>Symbol</th>
                <th>Qty</th>
                <th>Avg cost</th>
                <th>LTP</th>
                <th>Day chg %</th>
                <th>Mkt value</th>
                <th>Unreal. P/L</th>
                <th></th>
              </tr>
            </thead>
            <tbody>
              {holdings.map((h) => {
                const pnl = parseNumeric(h.unrealized_pnl);
                return (
                  <tr key={h.symbol} style={{ borderBottom: "1px solid #eee" }}>
                    <td style={{ padding: "6px 8px 6px 0" }}>{h.symbol}</td>
                    <td>{h.quantity}</td>
                    <td>{h.avg_cost}</td>
                    <td>{h.last_price ?? "—"}</td>
                    <td style={{ color: h.day_change_pct != null ? pnlColor(h.day_change_pct) : colors.textMuted }}>
                      {h.day_change_pct != null ? `${(h.day_change_pct * 100).toFixed(2)}%` : "—"}
                    </td>
                    <td>{h.market_value ?? "—"}</td>
                    <td style={{ color: h.unrealized_pnl != null ? pnlColor(pnl) : colors.textMuted, fontWeight: 500 }}>
                      {h.unrealized_pnl ?? "—"}
                    </td>
                    <td>
                      <button onClick={() => handleXirr(h.symbol)} style={{ fontSize: 11 }}>
                        XIRR
                      </button>
                    </td>
                  </tr>
                );
              })}
              {holdings.length === 0 && (
                <tr>
                  <td colSpan={8} style={{ padding: "12px 0", color: colors.textMuted, fontSize: 12 }}>
                    No holdings in this portfolio yet — record a buy below.
                  </td>
                </tr>
              )}
            </tbody>
          </table>
          {xirrResult && <p style={{ fontSize: 13, marginTop: 8 }}>{xirrResult}</p>}

          <h2 style={{ fontSize: 15, marginTop: 28, color: colors.navy }}>Add a new ticker</h2>
          <p style={{ fontSize: 12, color: colors.textMuted, marginTop: 0 }}>
            Registers the symbol as trackable (NSE, equity, no real ISIN — this slice doesn't
            validate against an exchange or broker yet). It'll then show up in the Buy/Sell form below.
          </p>
          <div style={{ display: "flex", gap: 8, alignItems: "center", marginBottom: 20 }}>
            <input
              value={newTicker}
              onChange={(e) => setNewTicker(e.target.value.toUpperCase())}
              onKeyDown={(e) => e.key === "Enter" && handleAddTicker()}
              placeholder="e.g. HDFCBANK"
              style={{ width: 140 }}
            />
            <button onClick={handleAddTicker}>Add Ticker</button>
          </div>

          <h2 style={{ fontSize: 15, color: colors.navy, marginBottom: 8 }}>Record a Transaction</h2>
          <div style={{ display: "flex", gap: 6, marginBottom: 8 }}>
            <button
              onClick={() => setTxnType("buy")}
              style={{
                fontSize: 12,
                padding: "4px 14px",
                borderRadius: 6,
                border: `1px solid ${txnType === "buy" ? colors.success : colors.border}`,
                background: txnType === "buy" ? "#DFF3E3" : "transparent",
                color: txnType === "buy" ? colors.success : colors.textMuted,
                cursor: "pointer",
                fontWeight: txnType === "buy" ? 600 : 400,
              }}
            >
              Buy
            </button>
            <button
              onClick={() => setTxnType("sell")}
              style={{
                fontSize: 12,
                padding: "4px 14px",
                borderRadius: 6,
                border: `1px solid ${txnType === "sell" ? colors.danger : colors.border}`,
                background: txnType === "sell" ? "#FBE4E2" : "transparent",
                color: txnType === "sell" ? colors.danger : colors.textMuted,
                cursor: "pointer",
                fontWeight: txnType === "sell" ? 600 : 400,
              }}
            >
              Sell
            </button>
          </div>
          <div style={{ display: "flex", gap: 8, alignItems: "center" }}>
            <select value={symbol} onChange={(e) => setSymbol(e.target.value)}>
              {instruments.length === 0 && <option value="">No tickers yet — add one above</option>}
              {instruments.map((inst) => (
                <option key={inst.symbol} value={inst.symbol}>
                  {inst.symbol}
                </option>
              ))}
            </select>
            <input value={qty} onChange={(e) => setQty(e.target.value)} placeholder="Quantity" style={{ width: 80 }} />
            <input
              value={price}
              onChange={(e) => setPrice(e.target.value)}
              placeholder="Price"
              style={{ width: 100 }}
            />
            <button
              onClick={handleRecordTransaction}
              style={{
                background: txnType === "buy" ? colors.success : colors.danger,
                color: "white",
                border: "none",
                borderRadius: 4,
                padding: "6px 14px",
                cursor: "pointer",
              }}
            >
              Record {txnType === "buy" ? "Buy" : "Sell"}
            </button>
          </div>
          {txnType === "sell" && (
            <p style={{ fontSize: 11, color: colors.textMuted, marginTop: 6 }}>
              Selling more than you currently hold is rejected — it never reaches the ledger, so
              there's nothing to undo if you mistype a quantity.
            </p>
          )}
        </>
      ) : (
        <div style={panelStyle}>
          <p style={{ fontSize: 13, color: colors.textMuted, margin: 0 }}>
            No intraday positions — this tab needs a live broker connection. The Zerodha adapter's
            fetch_intraday_positions() is implemented in Rust (see
            crates/infrastructure/src/brokers/zerodha.rs) but isn't reachable from the
            UI yet in this slice.
          </p>
        </div>
      )}
    </div>
  );
}

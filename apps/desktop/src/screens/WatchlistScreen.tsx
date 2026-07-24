import { useEffect, useMemo, useRef, useState } from "react";
import { api } from "../lib/tauri";
import type { InstrumentView, MarketSnapshotView, TechnicalAnalysisView } from "../lib/types";
import { colors, phaseColor, recommendationColor, dayChangeRowTint, zebraRowTint, flashAnimation, pnlColor, fmtMoney } from "../lib/theme";
import { ConfirmButton } from "../components/ConfirmButton";

const AUTO_REFRESH_MS = 30_000;

interface Row {
  snapshot: MarketSnapshotView | null;
  analysis: TechnicalAnalysisView | null;
  loadingSnapshot: boolean;
  loadingAnalysis: boolean;
  expanded: boolean;
  error: string | null;
}

function rsiColor(rsi: number | null): string {
  if (rsi == null) return colors.textMuted;
  if (rsi >= 70) return colors.danger; // overbought
  if (rsi <= 30) return colors.success; // oversold — conventionally a "cheap" reading, not necessarily "good"
  return colors.textMuted;
}

function fmtPct(v: number | null): string {
  return v == null ? "—" : `${v.toFixed(2)}%`;
}
function fmtNum(v: number | null): string {
  return v == null ? "—" : v.toFixed(2);
}

export function WatchlistScreen() {
  const [instruments, setInstruments] = useState<InstrumentView[]>([]);
  const [rows, setRows] = useState<Record<string, Row>>({});
  const [newTicker, setNewTicker] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [autoRefresh, setAutoRefresh] = useState(false);
  const intervalRef = useRef<ReturnType<typeof setInterval> | null>(null);

  async function refreshInstruments() {
    try {
      const list = await api.listInstruments();
      setInstruments(list);
    } catch (e) {
      setError(String(e));
    }
  }

  useEffect(() => {
    refreshInstruments();
  }, []);

  const emptyRow: Row = {
    snapshot: null,
    analysis: null,
    loadingSnapshot: false,
    loadingAnalysis: false,
    expanded: false,
    error: null,
  };

  function patchRow(symbol: string, patch: Partial<Row>) {
    setRows((prev) => ({
      ...prev,
      [symbol]: {
        ...emptyRow,
        ...prev[symbol],
        ...patch,
      },
    }));
  }

  async function refreshSnapshot(symbol: string) {
    patchRow(symbol, { loadingSnapshot: true, error: null });
    try {
      const snapshot = await api.getMarketSnapshot(symbol);
      patchRow(symbol, { snapshot, loadingSnapshot: false });
    } catch (e) {
      patchRow(symbol, { loadingSnapshot: false, error: String(e) });
    }
  }

  async function refreshAllSnapshots() {
    for (const inst of instruments) {
      await refreshSnapshot(inst.symbol);
    }
  }

  async function analyzeStock(symbol: string) {
    patchRow(symbol, { loadingAnalysis: true, expanded: true });
    try {
      const analysis = await api.analyzeMarketPhase(symbol);
      patchRow(symbol, { analysis, loadingAnalysis: false });
    } catch (e) {
      patchRow(symbol, { loadingAnalysis: false, error: String(e) });
    }
  }

  async function handleRemove(symbol: string) {
    try {
      await api.removeFromWatchlist(symbol);
      await refreshInstruments();
      setRows((prev) => {
        const next = { ...prev };
        delete next[symbol];
        return next;
      });
      setError(null);
    } catch (e) {
      // Backend rejects removal if any portfolio still holds a non-zero
      // quantity — surfaces here as a plain, specific error rather than
      // a silent no-op or a generic failure.
      setError(String(e));
    }
  }

  async function handleAddTicker() {
    const trimmed = newTicker.trim();
    if (!trimmed) return;
    try {
      const added = await api.addInstrument(trimmed);
      setNewTicker("");
      await refreshInstruments();
      await refreshSnapshot(added.symbol);
      setError(null);
      // Fire-and-forget: a fresh ticker has no chart history yet without
      // this. Not awaited into the main flow — a slow/failed backfill
      // shouldn't block the ticker from being added and shown.
      api.backfillHistory(added.symbol).catch((e) => setError(`Backfill for ${added.symbol} failed: ${e}`));
    } catch (e) {
      setError(String(e));
    }
  }

  // Same opt-in-only pattern as Holdings: auto-refresh is a real rate-limit
  // risk against an unofficial endpoint, so it stays off by default and
  // only ever pulls the cheap snapshot, never the heavier technical analysis.
  useEffect(() => {
    if (autoRefresh) {
      intervalRef.current = setInterval(() => {
        refreshAllSnapshots();
      }, AUTO_REFRESH_MS);
    }
    return () => {
      if (intervalRef.current) clearInterval(intervalRef.current);
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [autoRefresh, instruments.length]);

  type SortKey = "symbol" | "previous_close" | "price" | "day_change_pct" | "day_high" | "day_low" | "week52_high" | "week52_low" | "volume" | "rsi" | "phase" | "signal";
  const [sortKey, setSortKey] = useState<SortKey>("volume");
  const [sortDir, setSortDir] = useState<1 | -1>(-1); // -1 = descending, matches the previous fixed "highest volume first" default

  function sortValue(inst: InstrumentView, key: SortKey): number | string {
    const snap = rows[inst.symbol]?.snapshot;
    const analysis = rows[inst.symbol]?.analysis;
    switch (key) {
      case "symbol":
        return inst.symbol;
      case "previous_close":
        return snap?.previous_close != null ? parseFloat(snap.previous_close) : -Infinity;
      case "price":
        return snap?.price != null ? parseFloat(snap.price) : -Infinity;
      case "day_change_pct":
        return snap?.day_change_pct ?? -Infinity;
      case "day_high":
        return snap?.day_high != null ? parseFloat(snap.day_high) : -Infinity;
      case "day_low":
        return snap?.day_low != null ? parseFloat(snap.day_low) : -Infinity;
      case "week52_high":
        return snap?.week52_high != null ? parseFloat(snap.week52_high) : -Infinity;
      case "week52_low":
        return snap?.week52_low != null ? parseFloat(snap.week52_low) : -Infinity;
      case "volume":
        return snap?.volume ?? -1;
      case "rsi":
        return analysis?.rsi_14 ?? -Infinity;
      case "phase":
        return analysis?.phase ?? "";
      case "signal":
        return analysis?.recommendation ?? "";
    }
  }

  function handleSortClick(key: SortKey) {
    if (key === sortKey) {
      setSortDir((d) => (d === 1 ? -1 : 1) as 1 | -1);
    } else {
      setSortKey(key);
      setSortDir(-1);
    }
  }

  function sortIndicator(key: SortKey): string {
    if (key !== sortKey) return "";
    return sortDir === 1 ? " ▲" : " ▼";
  }

  const sortedInstruments = [...instruments].sort((a, b) => {
    const va = sortValue(a, sortKey);
    const vb = sortValue(b, sortKey);
    if (typeof va === "string" || typeof vb === "string") {
      const cmp = String(va).localeCompare(String(vb));
      return sortDir === -1 ? -cmp : cmp;
    }
    return sortDir === -1 ? vb - va : va - vb;
  });

  return (
    <div style={{ padding: 24 }}>
      <h1 style={{ fontSize: 20, color: colors.navy, marginBottom: 4 }}>Watchlist</h1>
      <p style={{ fontSize: 13, color: colors.textMuted, marginTop: 0 }}>
        Track any ticker before you've bought it — adding one here doesn't require a portfolio or
        a buy transaction. Once you're ready, use the Buy form on the Holdings screen for whichever
        family portfolio should own it. Sorted by today's volume, highest first.
      </p>

      <div style={{ display: "flex", gap: 8, alignItems: "center", marginBottom: 10 }}>
        <input
          value={newTicker}
          onChange={(e) => setNewTicker(e.target.value.toUpperCase())}
          onKeyDown={(e) => e.key === "Enter" && handleAddTicker()}
          placeholder="e.g. HDFCBANK"
          style={{ width: 140 }}
        />
        <button onClick={handleAddTicker}>Add to Watchlist</button>
        <button onClick={refreshAllSnapshots}>Refresh All Quotes</button>
        <label style={{ fontSize: 12, display: "flex", alignItems: "center", gap: 5, cursor: "pointer" }}>
          <input type="checkbox" checked={autoRefresh} onChange={(e) => setAutoRefresh(e.target.checked)} />
          Auto-refresh every 30s
        </label>
      </div>
      <p style={{ fontSize: 11, color: colors.textMuted, marginTop: 0, marginBottom: 12 }}>
        Quotes pull from an unofficial Yahoo Finance endpoint — free, but unsupported and could
        break or get rate-limited, especially with auto-refresh on. "Analyze" pulls a full year of
        history per stock (phase, moving averages, RSI, risk/return), so it's a manual per-row
        action, not part of auto-refresh.
      </p>

      {error && <p style={{ color: colors.danger }}>{error}</p>}

      <table style={{ borderCollapse: "collapse", width: "100%", fontSize: 13 }}>
        <thead>
          <tr style={{ textAlign: "left", borderBottom: `1px solid ${colors.border}` }}>
            <th style={{ padding: "6px 8px 6px 0", cursor: "pointer" }} onClick={() => handleSortClick("symbol")}>
              Symbol{sortIndicator("symbol")}
            </th>
            <th style={{ cursor: "pointer" }} onClick={() => handleSortClick("previous_close")}>
              Prev Close{sortIndicator("previous_close")}
            </th>
            <th style={{ cursor: "pointer" }} onClick={() => handleSortClick("price")}>
              Price{sortIndicator("price")}
            </th>
            <th style={{ cursor: "pointer" }} onClick={() => handleSortClick("day_change_pct")}>
              Day chg %{sortIndicator("day_change_pct")}
            </th>
            <th style={{ cursor: "pointer" }} onClick={() => handleSortClick("day_high")}>
              Day High{sortIndicator("day_high")}
            </th>
            <th style={{ cursor: "pointer" }} onClick={() => handleSortClick("day_low")}>
              Day Low{sortIndicator("day_low")}
            </th>
            <th style={{ cursor: "pointer" }} onClick={() => handleSortClick("week52_high")}>
              52W High{sortIndicator("week52_high")}
            </th>
            <th style={{ cursor: "pointer" }} onClick={() => handleSortClick("week52_low")}>
              52W Low{sortIndicator("week52_low")}
            </th>
            <th style={{ cursor: "pointer" }} onClick={() => handleSortClick("volume")}>
              Volume{sortIndicator("volume")}
            </th>
            <th style={{ cursor: "pointer" }} onClick={() => handleSortClick("rsi")}>
              RSI(14){sortIndicator("rsi")}
            </th>
            <th style={{ cursor: "pointer" }} onClick={() => handleSortClick("phase")}>
              Phase{sortIndicator("phase")}
            </th>
            <th style={{ cursor: "pointer" }} onClick={() => handleSortClick("signal")}>
              Signal{sortIndicator("signal")}
            </th>
            <th></th>
          </tr>
        </thead>
        <tbody>
          {sortedInstruments.map((inst, index) => {
            const row = rows[inst.symbol];
            const dayChange = row?.snapshot?.day_change_pct ?? null;
            const tint = dayChangeRowTint(dayChange) ?? zebraRowTint(index);
            const flash = flashAnimation(dayChange);
            return (
              <>
                <tr
                  key={inst.symbol}
                  style={{
                    borderBottom: "1px solid #eee",
                    cursor: "pointer",
                    backgroundColor: flash ? undefined : tint,
                    animation: flash ? `${flash} 1.4s ease-in-out infinite` : undefined,
                  }}
                  onClick={() => patchRow(inst.symbol, { expanded: !row?.expanded })}
                >
                  <td style={{ padding: "6px 8px 6px 0", fontWeight: flash ? 700 : 400 }}>{inst.symbol}</td>
                  <td>{fmtMoney(row?.snapshot?.previous_close)}</td>
                  <td>{row?.snapshot?.price ? fmtMoney(row.snapshot.price) : (row?.loadingSnapshot ? "…" : "—")}</td>
                  <td style={{ color: dayChange != null ? pnlColor(dayChange) : colors.textMuted, fontWeight: 600 }}>
                    {dayChange != null ? `${(dayChange * 100).toFixed(2)}%` : "—"}
                  </td>
                  <td>{fmtMoney(row?.snapshot?.day_high)}</td>
                  <td>{fmtMoney(row?.snapshot?.day_low)}</td>
                  <td>{fmtMoney(row?.snapshot?.week52_high)}</td>
                  <td>{fmtMoney(row?.snapshot?.week52_low)}</td>
                  <td>{row?.snapshot?.volume?.toLocaleString() ?? "—"}</td>
                  <td style={{ color: rsiColor(row?.analysis?.rsi_14 ?? null), fontWeight: 600 }}>
                    {row?.analysis ? fmtNum(row.analysis.rsi_14) : "—"}
                  </td>
                  <td>
                    {row?.analysis ? (
                      <span style={{ color: phaseColor(row.analysis.phase), fontWeight: 600 }}>{row.analysis.phase}</span>
                    ) : (
                      <button
                        onClick={(e) => {
                          e.stopPropagation();
                          analyzeStock(inst.symbol);
                        }}
                        disabled={row?.loadingAnalysis}
                        style={{ fontSize: 11 }}
                      >
                        {row?.loadingAnalysis ? "Analyzing…" : "Analyze"}
                      </button>
                    )}
                  </td>
                  <td>
                    {row?.analysis?.recommendation ? (
                      <span style={{ color: recommendationColor(row.analysis.recommendation), fontWeight: 700 }}>
                        {row.analysis.recommendation}
                      </span>
                    ) : row?.analysis ? (
                      <span style={{ color: colors.textMuted }}>—</span>
                    ) : null}
                  </td>
                  <td>
                    <ConfirmButton
                      label="Remove"
                      confirmLabel="Yes, stop tracking"
                      onConfirm={() => handleRemove(inst.symbol)}
                    />
                  </td>
                </tr>
                {row?.expanded && row.analysis && (
                  <tr key={`${inst.symbol}-detail`} style={{ background: colors.surface }}>
                    <td colSpan={13} style={{ padding: "8px 12px", fontSize: 12 }}>
                      <div style={{ display: "flex", gap: 24, flexWrap: "wrap" }}>
                        <span>SMA(10): <strong>{fmtNum(row.analysis.sma_10)}</strong></span>
                        <span>SMA(20): <strong>{fmtNum(row.analysis.sma_20)}</strong></span>
                        <span>SMA(50): <strong>{fmtNum(row.analysis.sma_50)}</strong></span>
                        <span>Ann. return: <strong>{fmtPct(row.analysis.annualized_return_pct)}</strong></span>
                        <span>Ann. volatility: <strong>{fmtPct(row.analysis.annualized_volatility_pct)}</strong></span>
                        <span>
                          Historical VaR (95%):{" "}
                          <strong style={{ color: colors.danger }}>{fmtPct(row.analysis.historical_var_95_pct)}</strong>
                        </span>
                      </div>
                      <p style={{ color: colors.textMuted, marginTop: 6, marginBottom: 12 }}>
                        VaR reads as: 95% confident a single day's loss won't exceed this — a rough
                        risk gauge from the last year of daily moves, not a guarantee.
                      </p>

                      {row.analysis.nearest_fib_label && (
                        <p style={{ margin: "0 0 6px" }}>
                          Nearest Fibonacci level: <strong>{row.analysis.nearest_fib_label}</strong> at{" "}
                          <strong>{fmtNum(row.analysis.nearest_fib_price)}</strong> (swing high/low from the
                          last year)
                        </p>
                      )}
                      <p style={{ margin: "0 0 4px", fontWeight: 600 }}>
                        Signal:{" "}
                        <span style={{ color: recommendationColor(row.analysis.recommendation) }}>
                          {row.analysis.recommendation ?? "Hold"}
                        </span>
                      </p>
                      <ul style={{ margin: "0 0 6px", paddingLeft: 18 }}>
                        {row.analysis.recommendation_reasons.map((reason, i) => (
                          <li key={i}>{reason}</li>
                        ))}
                      </ul>
                      <p style={{ color: colors.textMuted, fontStyle: "italic", margin: 0 }}>
                        Rules-based technical-analysis heuristic (Fibonacci retracement + trend +
                        RSI + candlestick confluence) — not financial advice, not backtested, and
                        no strategy here is foolproof. Treat this as "what a textbook confluence
                        check found," not "what to do."
                      </p>
                    </td>
                  </tr>
                )}
              </>
            );
          })}
          {instruments.length === 0 && (
            <tr>
              <td colSpan={13} style={{ padding: "12px 0", color: colors.textMuted, fontSize: 12 }}>
                No tickers yet — add one above to start tracking it.
              </td>
            </tr>
          )}
        </tbody>
      </table>
    </div>
  );
}

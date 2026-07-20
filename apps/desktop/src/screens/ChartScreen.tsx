import { useEffect, useState } from "react";
import { api } from "../lib/tauri";
import type { InstrumentView, PriceHistoryPoint } from "../lib/types";
import { colors, panelStyle } from "../lib/theme";

const WIDTH = 760;
const HEIGHT = 260;
const PADDING = 32;

function LineChart({ points }: { points: PriceHistoryPoint[] }) {
  if (points.length < 2) {
    return (
      <p style={{ fontSize: 12, color: colors.textMuted }}>
        Not enough price history to draw a chart yet.
      </p>
    );
  }

  const closes = points.map((p) => parseFloat(p.close));
  const min = Math.min(...closes);
  const max = Math.max(...closes);
  const range = max - min || 1;

  const xStep = (WIDTH - PADDING * 2) / (points.length - 1);
  const coords = closes.map((c, i) => {
    const x = PADDING + i * xStep;
    const y = PADDING + (HEIGHT - PADDING * 2) * (1 - (c - min) / range);
    return [x, y] as const;
  });
  const path = coords.map(([x, y], i) => `${i === 0 ? "M" : "L"}${x.toFixed(1)},${y.toFixed(1)}`).join(" ");

  return (
    <svg viewBox={`0 0 ${WIDTH} ${HEIGHT}`} width="100%" height={HEIGHT}>
      <line x1={PADDING} y1={HEIGHT - PADDING} x2={WIDTH - PADDING} y2={HEIGHT - PADDING} stroke={colors.border} />
      <line x1={PADDING} y1={PADDING} x2={PADDING} y2={HEIGHT - PADDING} stroke={colors.border} />
      <text x={4} y={PADDING + 4} fontSize={10} fill={colors.textMuted}>
        {max.toFixed(0)}
      </text>
      <text x={4} y={HEIGHT - PADDING + 4} fontSize={10} fill={colors.textMuted}>
        {min.toFixed(0)}
      </text>
      <path d={path} fill="none" stroke={colors.accent} strokeWidth={2} />
      {coords.length > 0 && (
        <circle cx={coords[coords.length - 1][0]} cy={coords[coords.length - 1][1]} r={3} fill={colors.navy} />
      )}
    </svg>
  );
}

export function ChartScreen() {
  const [instruments, setInstruments] = useState<InstrumentView[]>([]);
  const [symbol, setSymbol] = useState<string | null>(null);
  const [history, setHistory] = useState<PriceHistoryPoint[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [backfilling, setBackfilling] = useState(false);
  const [backfillMsg, setBackfillMsg] = useState<string | null>(null);

  useEffect(() => {
    api
      .listInstruments()
      .then((list) => {
        setInstruments(list);
        if (list.length > 0) setSymbol(list[0].symbol);
      })
      .catch((e) => setError(String(e)));
  }, []);

  async function loadHistory() {
    if (!symbol) return;
    try {
      setHistory(await api.getPriceHistory(symbol));
    } catch (e) {
      setError(String(e));
    }
  }

  useEffect(() => {
    loadHistory();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [symbol]);

  async function handleBackfill() {
    if (!symbol) return;
    setBackfilling(true);
    setBackfillMsg(null);
    try {
      const result = await api.backfillHistory(symbol);
      setBackfillMsg(`Downloaded ${result.days_backfilled} real trading days for ${symbol}.`);
      await loadHistory();
    } catch (e) {
      setBackfillMsg(`Backfill failed: ${String(e)}`);
    } finally {
      setBackfilling(false);
    }
  }

  return (
    <div style={{ padding: 24 }}>
      <h1 style={{ fontSize: 20, color: colors.navy, marginBottom: 4 }}>Chart</h1>
      <p style={{ fontSize: 13, color: colors.textMuted, marginTop: 0 }}>
        A daily-close line chart. New tickers auto-backfill a real year of Yahoo Finance history
        the moment they're added; the two original demo instruments (RELIANCE, TCS) still carry
        synthetic seed data until you backfill them here. Candlesticks/Heikin Ashi/Renko and
        overlaid indicators aren't built into this chart yet — see the Watchlist screen's
        "Analyze" for SMA/RSI numbers on the same underlying data.
      </p>

      <div style={{ display: "flex", gap: 6, marginBottom: 8, alignItems: "center", flexWrap: "wrap" }}>
        {instruments.map((inst) => (
          <button
            key={inst.symbol}
            onClick={() => setSymbol(inst.symbol)}
            style={{
              fontSize: 12,
              padding: "5px 12px",
              borderRadius: 6,
              border: `1px solid ${symbol === inst.symbol ? colors.accent : colors.border}`,
              background: symbol === inst.symbol ? colors.surface : "transparent",
              color: symbol === inst.symbol ? colors.accent : colors.textMuted,
              cursor: "pointer",
            }}
          >
            {inst.symbol}
          </button>
        ))}
        <button onClick={handleBackfill} disabled={!symbol || backfilling}>
          {backfilling ? "Downloading…" : "Backfill Real 1Y History"}
        </button>
      </div>
      {backfillMsg && <p style={{ fontSize: 12, color: colors.textMuted, marginBottom: 8 }}>{backfillMsg}</p>}

      {error && <p style={{ color: colors.danger }}>{error}</p>}

      <div style={panelStyle}>
        <LineChart points={history} />
      </div>
    </div>
  );
}

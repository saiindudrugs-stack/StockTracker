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

  useEffect(() => {
    api
      .listInstruments()
      .then((list) => {
        setInstruments(list);
        if (list.length > 0) setSymbol(list[0].symbol);
      })
      .catch((e) => setError(String(e)));
  }, []);

  useEffect(() => {
    if (!symbol) return;
    api.getPriceHistory(symbol).then(setHistory).catch((e) => setError(String(e)));
  }, [symbol]);

  return (
    <div style={{ padding: 24 }}>
      <h1 style={{ fontSize: 20, color: colors.navy, marginBottom: 4 }}>Chart</h1>
      <p style={{ fontSize: 13, color: colors.textMuted, marginTop: 0 }}>
        A daily-close line chart over the seeded 60-day history — real data flowing through
        DuckDB's SQLite stand-in (see the infrastructure README), not mocked in the frontend.
        Candlesticks/Heikin Ashi/Renko and indicator overlays (SRS 2.2.4) aren't built yet; this
        proves the data pipeline works before investing in a fuller charting engine.
      </p>

      <div style={{ display: "flex", gap: 6, marginBottom: 12 }}>
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
      </div>

      {error && <p style={{ color: colors.danger }}>{error}</p>}

      <div style={panelStyle}>
        <LineChart points={history} />
      </div>
    </div>
  );
}

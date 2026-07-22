import { useEffect, useMemo, useState } from "react";
import { api } from "../lib/tauri";
import type { CandleView, InstrumentView } from "../lib/types";
import { colors, panelStyle } from "../lib/theme";

const WIDTH = 800;
const HEIGHT = 340;
const PADDING_LEFT = 56;
const PADDING_RIGHT = 16;
const PADDING_TOP = 16;
const PADDING_BOTTOM = 36;
const PLOT_WIDTH = WIDTH - PADDING_LEFT - PADDING_RIGHT;
const PLOT_HEIGHT = HEIGHT - PADDING_TOP - PADDING_BOTTOM;

function CandlestickChart({ candles }: { candles: CandleView[] }) {
  const [hoverIndex, setHoverIndex] = useState<number | null>(null);

  const parsed = useMemo(
    () =>
      candles.map((c) => ({
        date: c.date,
        open: parseFloat(c.open),
        high: parseFloat(c.high),
        low: parseFloat(c.low),
        close: parseFloat(c.close),
        volume: c.volume,
      })),
    [candles]
  );

  if (parsed.length < 2) {
    return (
      <p style={{ fontSize: 12, color: colors.textMuted }}>
        Not enough OHLC history to draw candles yet — try "Backfill Real 1Y History" above.
      </p>
    );
  }

  const min = Math.min(...parsed.map((p) => p.low));
  const max = Math.max(...parsed.map((p) => p.high));
  const range = max - min || 1;
  const priceToY = (price: number) => PADDING_TOP + PLOT_HEIGHT * (1 - (price - min) / range);

  const slotWidth = PLOT_WIDTH / parsed.length;
  const bodyWidth = Math.max(2, Math.min(10, slotWidth * 0.6));
  const xForIndex = (i: number) => PADDING_LEFT + slotWidth * i + slotWidth / 2;

  // A handful of evenly-spaced date labels on the X axis rather than one
  // per candle (a year of daily candles would otherwise produce an
  // unreadable wall of overlapping text).
  const labelCount = Math.min(7, parsed.length);
  const labelIndices = Array.from({ length: labelCount }, (_, i) =>
    Math.round((i * (parsed.length - 1)) / Math.max(1, labelCount - 1))
  );

  // Y-axis gridlines: min, mid, max.
  const yTicks = [min, min + range / 2, max];

  function handleMouseMove(e: React.MouseEvent<SVGSVGElement>) {
    const rect = e.currentTarget.getBoundingClientRect();
    const scaleX = WIDTH / rect.width;
    const svgX = (e.clientX - rect.left) * scaleX;
    const idx = Math.round((svgX - PADDING_LEFT - slotWidth / 2) / slotWidth);
    setHoverIndex(idx >= 0 && idx < parsed.length ? idx : null);
  }

  const hovered = hoverIndex != null ? parsed[hoverIndex] : null;

  return (
    <div>
      <svg
        viewBox={`0 0 ${WIDTH} ${HEIGHT}`}
        width="100%"
        height={HEIGHT}
        onMouseMove={handleMouseMove}
        onMouseLeave={() => setHoverIndex(null)}
        style={{ cursor: "crosshair" }}
      >
        {/* Y-axis gridlines + labels */}
        {yTicks.map((price, i) => (
          <g key={i}>
            <line
              x1={PADDING_LEFT}
              y1={priceToY(price)}
              x2={WIDTH - PADDING_RIGHT}
              y2={priceToY(price)}
              stroke="#eee"
              strokeDasharray={i === 1 ? "3,3" : undefined}
            />
            <text x={4} y={priceToY(price) + 4} fontSize={10} fill={colors.textMuted}>
              {price.toFixed(price > 1000 ? 0 : 2)}
            </text>
          </g>
        ))}

        {/* X-axis date labels */}
        {labelIndices.map((idx) => (
          <text
            key={idx}
            x={xForIndex(idx)}
            y={HEIGHT - PADDING_BOTTOM + 16}
            fontSize={10}
            fill={colors.textMuted}
            textAnchor="middle"
          >
            {parsed[idx].date.slice(5)}
          </text>
        ))}
        <line
          x1={PADDING_LEFT}
          y1={HEIGHT - PADDING_BOTTOM}
          x2={WIDTH - PADDING_RIGHT}
          y2={HEIGHT - PADDING_BOTTOM}
          stroke={colors.border}
        />

        {/* Candles */}
        {parsed.map((c, i) => {
          const x = xForIndex(i);
          const isUp = c.close >= c.open;
          const color = isUp ? colors.success : colors.danger;
          const bodyTop = priceToY(Math.max(c.open, c.close));
          const bodyBottom = priceToY(Math.min(c.open, c.close));
          return (
            <g key={c.date}>
              <line x1={x} y1={priceToY(c.high)} x2={x} y2={priceToY(c.low)} stroke={color} strokeWidth={1} />
              <rect
                x={x - bodyWidth / 2}
                y={bodyTop}
                width={bodyWidth}
                height={Math.max(1, bodyBottom - bodyTop)}
                fill={color}
              />
            </g>
          );
        })}

        {/* Hover crosshair */}
        {hoverIndex != null && (
          <line
            x1={xForIndex(hoverIndex)}
            y1={PADDING_TOP}
            x2={xForIndex(hoverIndex)}
            y2={HEIGHT - PADDING_BOTTOM}
            stroke={colors.navy}
            strokeWidth={1}
            strokeDasharray="2,2"
          />
        )}
      </svg>

      {/* Tooltip: date + OHLCV at the cursor, as requested — shown below
          the chart rather than as a floating SVG box, so it never gets
          clipped at the chart's edges. */}
      <div style={{ fontSize: 12, minHeight: 20, color: colors.textMuted }}>
        {hovered ? (
          <span>
            <strong style={{ color: colors.navy }}>{hovered.date}</strong>
            {"  "}O: {hovered.open.toFixed(2)} H: {hovered.high.toFixed(2)} L: {hovered.low.toFixed(2)} C:{" "}
            <strong style={{ color: hovered.close >= hovered.open ? colors.success : colors.danger }}>
              {hovered.close.toFixed(2)}
            </strong>
            {hovered.volume != null ? `  Vol: ${hovered.volume.toLocaleString()}` : ""}
          </span>
        ) : (
          "Hover over the chart to see a candle's exact date and OHLC values."
        )}
      </div>
    </div>
  );
}

export function ChartScreen() {
  const [instruments, setInstruments] = useState<InstrumentView[]>([]);
  const [symbol, setSymbol] = useState<string | null>(null);
  const [candles, setCandles] = useState<CandleView[]>([]);
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
      setCandles(await api.getOhlcHistory(symbol));
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
        Real candlesticks (green = up day, red = down day) over up to a year of daily OHLC —
        actual open/high/low/close, not a close-only approximation. Hover anywhere on the chart
        for the exact date and values under your cursor. New tickers auto-backfill this history
        the moment they're added; the two original demo instruments still carry synthetic data
        until you backfill them here.
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
        <CandlestickChart candles={candles} />
      </div>
    </div>
  );
}

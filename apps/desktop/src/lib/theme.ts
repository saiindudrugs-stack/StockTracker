import type { CSSProperties } from "react";

export const colors = {
  navy: "#1F3864",
  accent: "#2E74B5",
  surface: "#F2F2F2",
  border: "#DDD",
  textMuted: "#666",
  success: "#1E7A34",
  danger: "#B3261E",
};

export const cardStyle: CSSProperties = {
  background: colors.surface,
  borderRadius: 8,
  padding: "12px 16px",
};

export const panelStyle: CSSProperties = {
  border: `1px dashed ${colors.border}`,
  borderRadius: 8,
  padding: 16,
};

export function recommendationColor(rec: string | null): string {
  switch (rec) {
    case "Buy":
      return colors.success;
    case "Sell":
      return colors.danger;
    case "Hold":
      return colors.textMuted;
    default:
      return colors.textMuted;
  }
}
/// Green for positive, red for negative, muted for exactly zero — the one
/// color rule used across P/L, day-change %, and CAGR everywhere in this
/// app. Was duplicated locally in HoldingsScreen before Watchlist also
/// needed it; promoted here rather than copy-pasted a second time.
export function pnlColor(value: number): string {
  if (value > 0) return colors.success;
  if (value < 0) return colors.danger;
  return colors.textMuted;
}

export function phaseColor(phase: string): string {
  switch (phase) {
    case "Markup":
      return colors.success;
    case "Accumulation":
      return "#3D8B5F"; // softer green — building, not yet confirmed uptrend
    case "Markdown":
      return colors.danger;
    case "Distribution":
      return "#C77B4A"; // amber-orange — softer warning, not yet confirmed downtrend
    default:
      return colors.textMuted; // Insufficient data
  }
}

// Any single-day move at or beyond this magnitude is flagged as a genuine
// "look at this now" alert (flashing), not just a colored number — the
// threshold the user asked for.
export const BIG_MOVE_THRESHOLD = 0.035;

/// Subtle full-row background tint scaled by the day's move — a quick
/// glance at the row shading should tell you which stocks moved today
/// without needing to read the percentage column at all. Deliberately
/// light (low alpha) so text stays readable and zebra striping can still
/// show through on unchanged rows.
export function dayChangeRowTint(pct: number | null): string | undefined {
  if (pct == null || pct === 0) return undefined;
  const intensity = Math.min(Math.abs(pct) / 0.05, 1); // saturates at a 5% move
  if (pct > 0) return `rgba(30, 122, 52, ${0.06 + intensity * 0.12})`;
  return `rgba(179, 38, 30, ${0.06 + intensity * 0.12})`;
}

/// Every-other-row shading for plain readability on rows with no notable
/// move — applied only when dayChangeRowTint returns nothing, so it never
/// competes with the move-based tint above.
export function zebraRowTint(index: number): string {
  return index % 2 === 0 ? "transparent" : "rgba(0, 0, 0, 0.02)";
}

/// Whether a row's ticker should flash — beyond the threshold in either
/// direction. Returns the animation name to use, or undefined for no flash.
export function flashAnimation(pct: number | null): string | undefined {
  if (pct == null) return undefined;
  if (pct <= -BIG_MOVE_THRESHOLD) return "flash-amber";
  if (pct >= BIG_MOVE_THRESHOLD) return "flash-green";
  return undefined;
}

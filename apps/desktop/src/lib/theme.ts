import type { CSSProperties } from "react";

/// Formats a numeric string (as returned by every backend command — prices
/// and money are transmitted as strings, not JS numbers, to avoid float
/// precision surprises) with thousands separators and exactly 2 decimal
/// places. This is what was missing from Market Value / Unrealized P/L —
/// they were rendered as raw strings straight from the backend with no
/// formatting at all.
export function fmtMoney(value: string | number | null | undefined): string {
  if (value == null) return "—";
  const n = typeof value === "string" ? parseFloat(value) : value;
  if (!Number.isFinite(n)) return "—";
  return n.toLocaleString("en-IN", { minimumFractionDigits: 2, maximumFractionDigits: 2 });
}

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
  // Whisper-subtle gradient, not a flat block — softens the repeated
  // "gray box" look across Dashboard's stacked summary cards without
  // touching any of the actual color-coded text (P&L red/green) inside
  // them, which is where the real signal lives.
  background: `linear-gradient(180deg, #F5F5F5 0%, ${colors.surface} 100%)`,
  borderRadius: 8,
  padding: "12px 16px",
};

export const panelStyle: CSSProperties = {
  border: `1px dashed ${colors.border}`,
  borderRadius: 8,
  padding: 16,
};

/// Solid navy header band for data tables (Holdings, Watchlist, Mutual
/// Funds, Analysis) — matches the wireframes shown before these screens
/// were built; the plain-white/thin-border header the tables originally
/// shipped with never carried that treatment over. Apply tableHeaderRow to
/// the <tr> and tableHeaderCell to each <th> inside it; the first and last
/// <th> in a row also need firstHeaderCell / lastHeaderCell merged in (via
/// spread) so the rounded corners land on the outer edges instead of
/// every cell.
export const tableHeaderRow: CSSProperties = {
  // A subtle gradient rather than a flat block — same navy, softer to
  // look at repeated across a page that already has multiple tables
  // (Holdings alone has 14 columns), without changing what the color
  // itself signals (this is still "header row," nothing color-coded
  // here competes with the actual P&L red/green used elsewhere).
  background: `linear-gradient(180deg, ${colors.navy} 0%, #16294A 100%)`,
  textAlign: "left",
};

export const tableHeaderCell: CSSProperties = {
  color: "#E6F1FB",
  padding: "8px 10px",
  fontWeight: 500,
};

export const firstHeaderCell: CSSProperties = {
  borderTopLeftRadius: 8,
  borderBottomLeftRadius: 8,
  paddingLeft: 14,
};

export const lastHeaderCell: CSSProperties = {
  borderTopRightRadius: 8,
  borderBottomRightRadius: 8,
  paddingRight: 14,
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
export function currencySymbolForExchange(exchange: string | null | undefined): string {
  switch ((exchange ?? "").toUpperCase()) {
    case "NASDAQ":
    case "NYSE":
      return "$";
    case "LSE":
      return "£";
    case "TSX":
      return "C$";
    case "ASX":
      return "A$";
    case "HKEX":
      return "HK$";
    // NSE, BSE, AMFI, and anything unrecognized default to ₹ — this app
    // started India-only, so ₹ stays the sensible fallback rather than a
    // bare "?" for any exchange this mapping doesn't yet know about.
    default:
      return "₹";
  }
}

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

/// Recommended auto-refresh interval for a whole-watchlist pass — one
/// quote request per tracked symbol, not a single batched call — based on
/// each source's REAL documented rate limit, not a guess:
/// - Upstox: 25 req/sec, 250/min, 1000/30min (official docs) — generous.
///   A watchlist of dozens of symbols refreshing every few seconds stays
///   comfortably inside that.
/// - Yahoo: no documented limit (unofficial, scrapes their site) — kept
///   deliberately cautious regardless, since "no limit" isn't the same
///   as "safe to hammer."
export function recommendedRefreshSeconds(source: string, symbolCount: number): { seconds: number; reason: string } {
  switch (source) {
    case "upstox":
      return { seconds: 5, reason: "Upstox allows 25 requests/second — a wide margin even for a large watchlist." };
    default:
      return { seconds: 20, reason: "Yahoo has no documented rate limit, but it's an unofficial, scraped endpoint — kept cautious regardless." };
  }
}

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
  return index % 2 === 0 ? "transparent" : "#F3F5F8";
}

/// Whether a row's ticker should flash — beyond the threshold in either
/// direction. Returns the animation name to use, or undefined for no flash.
export function flashAnimation(pct: number | null, thresholdPct: number = BIG_MOVE_THRESHOLD): string | undefined {
  if (pct == null) return undefined;
  if (pct <= -thresholdPct) return "flash-amber";
  if (pct >= thresholdPct) return "flash-green";
  return undefined;
}

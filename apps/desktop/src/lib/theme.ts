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

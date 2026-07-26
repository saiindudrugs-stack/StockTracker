import type { ScreenId } from "../lib/types";
import { colors } from "../lib/theme";

const ITEMS: { id: ScreenId; label: string; glyph: string }[] = [
  { id: "dashboard", label: "Dashboard", glyph: "\u2302" },
  { id: "holdings", label: "Holdings", glyph: "\u2261" },
  { id: "mutual-funds", label: "Mutual Funds", glyph: "\u20B9" },
  { id: "watchlist", label: "Watchlist", glyph: "\u2606" },
  { id: "analysis", label: "Analysis", glyph: "\u03A3" },
  { id: "chart", label: "Chart", glyph: "\u2197" },
  { id: "settings", label: "Settings", glyph: "\u2699" },
];

/// Horizontal, sitting directly under PortfolioTabs — both rows live at the
/// top now, rather than one at the top and one down the left edge, per
/// explicit layout feedback that looking both top and left was
/// distracting. Same active-state treatment as PortfolioTabs (a solid
/// accent underline), so the two rows read as one consistent system
/// instead of two different UI languages.
export function NavBar({ active, onSelect }: { active: ScreenId; onSelect: (id: ScreenId) => void }) {
  return (
    <div
      style={{
        display: "flex",
        alignItems: "center",
        gap: 2,
        padding: "0 16px",
        borderBottom: `1px solid ${colors.border}`,
        flexShrink: 0,
      }}
    >
      {ITEMS.map((item) => {
        const isActive = item.id === active;
        return (
          <button
            key={item.id}
            onClick={() => onSelect(item.id)}
            title={item.label}
            style={{
              display: "flex",
              alignItems: "center",
              gap: 6,
              padding: "8px 12px",
              background: "transparent",
              border: "none",
              borderBottom: `2px solid ${isActive ? colors.accent : "transparent"}`,
              cursor: "pointer",
              color: isActive ? colors.accent : colors.textMuted,
              fontWeight: isActive ? 600 : 400,
              fontSize: 12,
            }}
          >
            <span style={{ fontSize: 14 }}>{item.glyph}</span>
            <span>{item.label}</span>
          </button>
        );
      })}
    </div>
  );
}

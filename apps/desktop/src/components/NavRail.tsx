import type { ScreenId } from "../lib/types";
import { colors } from "../lib/theme";

const ITEMS: { id: ScreenId; label: string; glyph: string }[] = [
  { id: "dashboard", label: "Dashboard", glyph: "\u2302" },
  { id: "holdings", label: "Holdings", glyph: "\u2261" },
  { id: "chart", label: "Chart", glyph: "\u2197" },
  { id: "settings", label: "Settings", glyph: "\u2699" },
];

export function NavRail({ active, onSelect }: { active: ScreenId; onSelect: (id: ScreenId) => void }) {
  return (
    <div
      style={{
        width: 72,
        display: "flex",
        flexDirection: "column",
        alignItems: "center",
        gap: 4,
        paddingTop: 16,
        borderRight: `1px solid ${colors.border}`,
        height: "100vh",
        boxSizing: "border-box",
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
              width: 56,
              padding: "10px 0",
              display: "flex",
              flexDirection: "column",
              alignItems: "center",
              gap: 2,
              background: isActive ? colors.surface : "transparent",
              border: "none",
              borderRadius: 8,
              cursor: "pointer",
              color: isActive ? colors.accent : colors.textMuted,
            }}
          >
            <span style={{ fontSize: 18 }}>{item.glyph}</span>
            <span style={{ fontSize: 9 }}>{item.label}</span>
          </button>
        );
      })}
    </div>
  );
}

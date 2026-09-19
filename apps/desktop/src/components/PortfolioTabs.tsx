import { useState } from "react";
import type { PortfolioView } from "../lib/types";
import { colors } from "../lib/theme";

/// Deliberately no remove/delete control here at all — that's rare enough
/// (per explicit feedback: "something we don't use often") that it belongs
/// in Settings' "Manage portfolios" section instead, not sitting in the
/// tab bar you look at constantly. Keeps this bar to exactly what it's for:
/// picking which portfolio you're looking at.
export function PortfolioTabs({
  portfolios,
  activeId,
  onSelect,
  onCreate,
}: {
  portfolios: PortfolioView[];
  activeId: string | null;
  onSelect: (id: string) => void;
  onCreate: (name: string) => void;
}) {
  const [adding, setAdding] = useState(false);
  const [newName, setNewName] = useState("");

  function submitNew() {
    const trimmed = newName.trim();
    if (trimmed) onCreate(trimmed);
    setNewName("");
    setAdding(false);
  }

  return (
    <div
      style={{
        display: "flex",
        alignItems: "center",
        gap: 4,
        padding: "10px 16px 8px",
        flexShrink: 0,
      }}
    >
      {portfolios.map((p) => {
        const isActive = p.id === activeId;
        return (
          <button
            key={p.id}
            onClick={() => onSelect(p.id)}
            style={{
              fontSize: 12,
              padding: "6px 10px",
              background: "transparent",
              border: "none",
              borderBottom: `4px solid ${isActive ? colors.accent : "transparent"}`,
              color: isActive ? colors.accent : colors.textMuted,
              fontWeight: isActive ? 600 : 400,
              cursor: "pointer",
            }}
          >
            {p.name}
          </button>
        );
      })}

      {adding ? (
        <span style={{ display: "flex", gap: 4 }}>
          <input
            autoFocus
            value={newName}
            onChange={(e) => setNewName(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === "Enter") submitNew();
              if (e.key === "Escape") setAdding(false);
            }}
            placeholder="e.g. Dad, Mom, Kid 1"
            style={{ fontSize: 12, padding: "4px 8px", width: 140 }}
          />
          <button onClick={submitNew} style={{ fontSize: 11 }}>
            Add
          </button>
        </span>
      ) : (
        <button
          onClick={() => setAdding(true)}
          title="Add a portfolio"
          style={{
            fontSize: 12,
            padding: "5px 10px",
            borderRadius: 6,
            border: `1px dashed ${colors.border}`,
            background: "transparent",
            color: colors.textMuted,
            cursor: "pointer",
          }}
        >
          + Add portfolio
        </button>
      )}
    </div>
  );
}

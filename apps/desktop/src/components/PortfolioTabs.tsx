import { useState } from "react";
import type { PortfolioView } from "../lib/types";
import { colors } from "../lib/theme";
import { ConfirmButton } from "./ConfirmButton";

export function PortfolioTabs({
  portfolios,
  activeId,
  onSelect,
  onCreate,
  onDelete,
}: {
  portfolios: PortfolioView[];
  activeId: string | null;
  onSelect: (id: string) => void;
  onCreate: (name: string) => void;
  onDelete: (id: string) => void;
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
        gap: 6,
        padding: "10px 16px",
        borderBottom: `1px solid ${colors.border}`,
        flexShrink: 0,
      }}
    >
      {portfolios.map((p) => {
        const isActive = p.id === activeId;
        return (
          <span
            key={p.id}
            style={{
              display: "inline-flex",
              alignItems: "center",
              gap: 4,
              borderRadius: 6,
              border: `1px solid ${isActive ? colors.accent : colors.border}`,
              background: isActive ? colors.surface : "transparent",
              paddingRight: 4,
            }}
          >
            <button
              onClick={() => onSelect(p.id)}
              style={{
                fontSize: 12,
                padding: "5px 8px 5px 12px",
                border: "none",
                background: "transparent",
                color: isActive ? colors.accent : colors.textMuted,
                cursor: "pointer",
                fontWeight: isActive ? 600 : 400,
              }}
            >
              {p.name}
            </button>
            <ConfirmButton
              label="×"
              confirmLabel="Delete portfolio?"
              onConfirm={() => onDelete(p.id)}
              style={{ padding: "1px 5px", fontSize: 12 }}
            />
          </span>
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

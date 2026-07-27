import { useState } from "react";
import { colors } from "../lib/theme";

export interface Market {
  code: string;
  label: string;
  defaultExchange: string;
}

/// Only three markets for now, deliberately — matches what's actually
/// been verified against Yahoo's real suffix conventions (see the doc
/// comment on to_yahoo_symbol in yahoo_finance.rs). India's NSE/BSE path
/// has been exercised live all session; US and UK follow Yahoo's
/// documented convention but haven't been proven against a real fetch
/// from this sandbox — worth trying a real ticker from each before
/// trusting it blindly.
export const MARKETS: Market[] = [
  { code: "IN", label: "India (NSE/BSE)", defaultExchange: "NSE" },
  { code: "US", label: "United States (NYSE/NASDAQ)", defaultExchange: "NASDAQ" },
  { code: "UK", label: "United Kingdom (LSE)", defaultExchange: "LSE" },
];

export function CountrySelector({ selected, onSelect }: { selected: Market; onSelect: (m: Market) => void }) {
  const [open, setOpen] = useState(false);

  return (
    <div style={{ position: "relative" }}>
      <button
        onClick={() => setOpen((o) => !o)}
        style={{
          fontSize: 12,
          padding: "6px 12px",
          border: `0.5px solid ${colors.border}`,
          borderRadius: 6,
          background: colors.surface,
          display: "flex",
          alignItems: "center",
          gap: 6,
          cursor: "pointer",
        }}
      >
        {selected.label} <span style={{ fontSize: 10 }}>▾</span>
      </button>
      {open && (
        <div
          style={{
            position: "absolute",
            top: 32,
            right: 0,
            width: 220,
            background: "white",
            border: `0.5px solid ${colors.border}`,
            borderRadius: 8,
            boxShadow: "0 4px 12px rgba(0,0,0,0.08)",
            zIndex: 20,
          }}
        >
          {MARKETS.map((m) => (
            <div
              key={m.code}
              onClick={() => {
                onSelect(m);
                setOpen(false);
              }}
              style={{
                padding: "8px 12px",
                fontSize: 12,
                cursor: "pointer",
                background: m.code === selected.code ? "#E6F1FB" : "transparent",
                color: m.code === selected.code ? colors.accent : colors.textMuted,
                fontWeight: m.code === selected.code ? 600 : 400,
                borderTop: `0.5px solid ${colors.border}`,
              }}
            >
              {m.label}
            </div>
          ))}
        </div>
      )}
    </div>
  );
}

import { useState } from "react";
import { colors, panelStyle } from "../lib/theme";

const DEFAULT_BROKER_ORDER = [
  "Zerodha (live)",
  "Upstox",
  "FYERS",
  "Angel One",
  "Kotak Neo",
  "Dhan",
  "Groww",
  "ICICI Direct",
  "Motilal Oswal",
  "Interactive Brokers",
];

export function SettingsScreen() {
  const [aiEnabled, setAiEnabled] = useState(true);
  const [aiMode, setAiMode] = useState<"local" | "cloud">("local");
  const [brokerOrder, setBrokerOrder] = useState(DEFAULT_BROKER_ORDER);

  function move(index: number, direction: -1 | 1) {
    const next = [...brokerOrder];
    const target = index + direction;
    if (target < 0 || target >= next.length || index === 0) return; // Zerodha (live) pinned at #1
    if (target === 0) return;
    [next[index], next[target]] = [next[target], next[index]];
    setBrokerOrder(next);
  }

  return (
    <div style={{ padding: 24 }}>
      <h1 style={{ fontSize: 20, color: colors.navy, marginBottom: 4 }}>Settings</h1>
      <p style={{ fontSize: 13, color: colors.textMuted, marginTop: 0 }}>
        This screen's controls are real UI state but aren't wired to backend persistence yet in
        this slice — they'll reset on restart. Persisting them is a small follow-up (a settings
        table or JSON file), deliberately left out so this slice's engineering effort went toward
        the Portfolio Engine / Zerodha adapter / Live Feed Manager instead.
      </p>

      <div style={{ ...panelStyle, marginBottom: 16 }}>
        <div style={{ display: "flex", justifyContent: "space-between", alignItems: "center", marginBottom: 12 }}>
          <span style={{ fontSize: 13, fontWeight: 600 }}>AI Assistant</span>
          <button
            onClick={() => setAiEnabled((v) => !v)}
            style={{
              fontSize: 11,
              padding: "3px 12px",
              borderRadius: 10,
              border: "none",
              background: aiEnabled ? "#DFF3E3" : "#F0F0F0",
              color: aiEnabled ? colors.success : colors.textMuted,
              cursor: "pointer",
            }}
          >
            {aiEnabled ? "On" : "Off"}
          </button>
        </div>

        {aiEnabled && (
          <>
            <div
              style={{
                display: "flex",
                justifyContent: "space-between",
                alignItems: "center",
                paddingTop: 10,
                borderTop: `1px solid ${colors.border}`,
                marginBottom: 10,
              }}
            >
              <span style={{ fontSize: 12, color: "#444" }}>Mode</span>
              <div style={{ display: "flex", gap: 12, fontSize: 11 }}>
                <label style={{ cursor: "pointer" }}>
                  <input
                    type="radio"
                    checked={aiMode === "local"}
                    onChange={() => setAiMode("local")}
                    style={{ marginRight: 4 }}
                  />
                  Local only
                </label>
                <label style={{ cursor: "pointer" }}>
                  <input
                    type="radio"
                    checked={aiMode === "cloud"}
                    onChange={() => setAiMode("cloud")}
                    style={{ marginRight: 4 }}
                  />
                  Local + cloud backup
                </label>
              </div>
            </div>

            <div style={{ paddingTop: 10, borderTop: `1px solid ${colors.border}` }}>
              <p style={{ fontSize: 12, color: "#444", margin: "0 0 6px" }}>
                Cloud API key (optional backup) — Anthropic first, OpenAI second
              </p>
              <div style={{ display: "flex", gap: 8 }}>
                <span
                  style={{
                    fontSize: 11,
                    padding: "3px 10px",
                    border: `1px solid ${colors.border}`,
                    borderRadius: 6,
                    background: colors.surface,
                  }}
                >
                  Anthropic — no key set
                </span>
                <span
                  style={{
                    fontSize: 11,
                    padding: "3px 10px",
                    border: `1px dashed ${colors.border}`,
                    borderRadius: 6,
                    color: colors.textMuted,
                  }}
                >
                  OpenAI — add key
                </span>
              </div>
              <p style={{ fontSize: 11, color: colors.textMuted, margin: "8px 0 0" }}>
                A consent screen would show here once, before the first cloud call, stating
                exactly what's sent — not implemented yet, no cloud calls actually happen in this
                slice regardless of this toggle.
              </p>
            </div>
          </>
        )}
      </div>

      <div style={panelStyle}>
        <p style={{ fontSize: 13, fontWeight: 600, margin: "0 0 6px" }}>Broker rollout priority</p>
        <p style={{ fontSize: 11, color: colors.textMuted, margin: "0 0 10px" }}>
          Reorderable list, kept as a simple config rather than a full drag-and-drop UI until more
          than a couple of adapters are actually live.
        </p>
        <div style={{ display: "flex", flexDirection: "column", gap: 4 }}>
          {brokerOrder.map((broker, i) => (
            <div
              key={broker}
              style={{
                display: "flex",
                justifyContent: "space-between",
                alignItems: "center",
                fontSize: 12,
                padding: "5px 10px",
                background: i === 0 ? colors.surface : "transparent",
                borderRadius: 4,
              }}
            >
              <span>
                {i + 1}. {broker}
              </span>
              {i !== 0 && (
                <span style={{ display: "flex", gap: 4 }}>
                  <button onClick={() => move(i, -1)} disabled={i <= 1} style={{ fontSize: 10 }}>
                    ↑
                  </button>
                  <button onClick={() => move(i, 1)} disabled={i === brokerOrder.length - 1} style={{ fontSize: 10 }}>
                    ↓
                  </button>
                </span>
              )}
            </div>
          ))}
        </div>
      </div>
    </div>
  );
}

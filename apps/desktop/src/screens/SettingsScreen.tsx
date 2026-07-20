import { useState } from "react";
import { api } from "../lib/tauri";
import { colors, panelStyle } from "../lib/theme";

export function SettingsScreen() {
  const [aiEnabled, setAiEnabled] = useState(true);
  const [aiMode, setAiMode] = useState<"local" | "cloud">("local");
  const [confirmingReset, setConfirmingReset] = useState(false);
  const [resetting, setResetting] = useState(false);
  const [resetMessage, setResetMessage] = useState<string | null>(null);

  async function handleReset() {
    setResetting(true);
    setResetMessage(null);
    try {
      await api.resetAllData();
      setResetMessage("Done — every portfolio, holding, transaction, and cached price has been cleared. Restart the app to see a clean slate.");
      setConfirmingReset(false);
    } catch (e) {
      setResetMessage(`Reset failed: ${String(e)}`);
    } finally {
      setResetting(false);
    }
  }

  return (
    <div style={{ padding: 24 }}>
      <h1 style={{ fontSize: 20, color: colors.navy, marginBottom: 4 }}>Settings</h1>
      <p style={{ fontSize: 13, color: colors.textMuted, marginTop: 0 }}>
        The AI toggle below is real UI state but isn't wired to backend persistence yet — it'll
        reset on restart.
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

      <div style={{ ...panelStyle, marginBottom: 16 }}>
        <p style={{ fontSize: 13, fontWeight: 600, margin: "0 0 6px" }}>Price data source</p>
        <p style={{ fontSize: 12, color: colors.textMuted, margin: 0 }}>
          Yahoo Finance (unofficial endpoint) — the sole live price source right now. The
          Zerodha/broker rollout plan from earlier in this project is on hold in favor of this
          simpler, no-subscription-required approach; the Zerodha adapter code still exists in
          the Rust engine (crates/infrastructure/src/brokers/zerodha.rs) but nothing in the UI
          calls it anymore.
        </p>
      </div>

      <div style={{ ...panelStyle, borderColor: colors.danger }}>
        <p style={{ fontSize: 13, fontWeight: 600, margin: "0 0 6px", color: colors.danger }}>
          Danger Zone
        </p>
        <p style={{ fontSize: 12, color: colors.textMuted, margin: "0 0 10px" }}>
          Reinstalling the app does NOT clear this data — your portfolios, holdings, and cached
          prices live in a database file in your OS's app-data folder, completely separate from
          the installed application. That's standard, expected behavior on every OS, not a bug.
          Use this button if you want to wipe everything and start clean (e.g. after test data).
        </p>
        {!confirmingReset ? (
          <button onClick={() => setConfirmingReset(true)} style={{ color: colors.danger }}>
            Reset All Data…
          </button>
        ) : (
          <div>
            <p style={{ fontSize: 12, fontWeight: 600, color: colors.danger, margin: "0 0 8px" }}>
              This permanently deletes every portfolio, holding, transaction, and cached price.
              This cannot be undone. Are you sure?
            </p>
            <button onClick={handleReset} disabled={resetting} style={{ color: colors.danger, marginRight: 8 }}>
              {resetting ? "Resetting…" : "Yes, delete everything"}
            </button>
            <button onClick={() => setConfirmingReset(false)} disabled={resetting}>
              Cancel
            </button>
          </div>
        )}
        {resetMessage && <p style={{ fontSize: 12, marginTop: 10 }}>{resetMessage}</p>}
      </div>
    </div>
  );
}

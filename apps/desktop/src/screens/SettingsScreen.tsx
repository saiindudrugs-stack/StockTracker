import { useState } from "react";
import { check } from "@tauri-apps/plugin-updater";
import { relaunch } from "@tauri-apps/plugin-process";
import { api } from "../lib/tauri";
import type { PortfolioView } from "../lib/types";
import { colors, panelStyle } from "../lib/theme";
import { ConfirmButton } from "../components/ConfirmButton";

export function SettingsScreen({
  portfolios,
  onDeletePortfolio,
}: {
  portfolios: PortfolioView[];
  onDeletePortfolio: (id: string) => void;
}) {
  const [updateStatus, setUpdateStatus] = useState<
    "idle" | "checking" | "up-to-date" | "available" | "downloading" | "error"
  >("idle");
  const [updateVersion, setUpdateVersion] = useState<string | null>(null);
  const [updateNotes, setUpdateNotes] = useState<string | null>(null);
  const [updateError, setUpdateError] = useState<string | null>(null);
  // Holds the actual Update object between "found one" and "install it" —
  // check() and downloadAndInstall() are two separate steps so the user
  // sees what's new before committing to the download.
  const [pendingUpdate, setPendingUpdate] = useState<Awaited<ReturnType<typeof check>> | null>(null);

  async function handleCheckForUpdates() {
    setUpdateStatus("checking");
    setUpdateError(null);
    try {
      const update = await check();
      if (update) {
        setPendingUpdate(update);
        setUpdateVersion(update.version);
        setUpdateNotes(update.body ?? null);
        setUpdateStatus("available");
      } else {
        setUpdateStatus("up-to-date");
      }
    } catch (e) {
      setUpdateStatus("error");
      setUpdateError(String(e));
    }
  }

  async function handleInstallUpdate() {
    if (!pendingUpdate) return;
    setUpdateStatus("downloading");
    try {
      await pendingUpdate.downloadAndInstall();
      await relaunch();
    } catch (e) {
      setUpdateStatus("error");
      setUpdateError(String(e));
    }
  }

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
        <p style={{ fontSize: 13, fontWeight: 600, margin: "0 0 6px" }}>
          Software update <span style={{ color: colors.textMuted, fontWeight: 400 }}>(paused)</span>
        </p>
        <p style={{ fontSize: 12, color: colors.textMuted, margin: "0 0 10px" }}>
          Temporarily paused, not removed — the release pipeline hit a persistent signing failure
          ("public key found, but no private key") that held up even after the signing secrets
          were verified correct and confirmed present in the build step, pointing to a bug in the
          signing tool itself rather than anything in this app or its GitHub setup. The button
          below is disabled so it doesn't silently fail — once the signing pipeline is revisited
          and working, this re-enables with no other changes needed.
        </p>
        {updateStatus === "available" && updateVersion ? (
          <div style={{ ...panelStyle, borderColor: colors.accent, marginBottom: 10 }}>
            <p style={{ fontSize: 12, fontWeight: 600, margin: "0 0 4px", color: colors.accent }}>
              Version {updateVersion} is available
            </p>
            {updateNotes && <p style={{ fontSize: 12, color: colors.textMuted, margin: "0 0 8px" }}>{updateNotes}</p>}
            <button onClick={handleInstallUpdate} disabled={updateStatus === ("downloading" as typeof updateStatus)}>
              Install & Restart
            </button>
          </div>
        ) : null}
        <button onClick={handleCheckForUpdates} disabled title="Paused until the signing pipeline is fixed — see note above">
          Check for Updates
        </button>
        {updateStatus === "up-to-date" && (
          <p style={{ fontSize: 12, color: colors.success, marginTop: 8 }}>You're on the latest version.</p>
        )}
        {updateStatus === "error" && updateError && (
          <p style={{ fontSize: 12, color: colors.danger, marginTop: 8 }}>Update check failed: {updateError}</p>
        )}
      </div>

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

      <div style={{ ...panelStyle, marginBottom: 16 }}>
        <p style={{ fontSize: 13, fontWeight: 600, margin: "0 0 6px" }}>Manage portfolios</p>
        <p style={{ fontSize: 12, color: colors.textMuted, margin: "0 0 10px" }}>
          Removing a portfolio deletes every transaction, holding, and alert scoped to it — real,
          permanent data loss. It never touches shared instruments, so it can't affect another
          family member's portfolio or your Watchlist. Kept here rather than in the tab bar since
          this is rare enough not to need a control you see constantly.
        </p>
        <div style={{ display: "flex", flexDirection: "column", gap: 6 }}>
          {portfolios.map((p) => (
            <div key={p.id} style={{ display: "flex", justifyContent: "space-between", alignItems: "center", fontSize: 12 }}>
              <span>{p.name}</span>
              <ConfirmButton label="Remove" confirmLabel="Yes, delete" onConfirm={() => onDeletePortfolio(p.id)} />
            </div>
          ))}
          {portfolios.length === 0 && <p style={{ fontSize: 12, color: colors.textMuted, margin: 0 }}>No portfolios yet.</p>}
        </div>
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

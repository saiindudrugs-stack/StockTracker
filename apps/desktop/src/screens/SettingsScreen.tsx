import { useEffect, useState } from "react";
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

  const [avKeyInput, setAvKeyInput] = useState("");
  const [avKeySaved, setAvKeySaved] = useState<boolean | null>(null);
  const [avSaveMsg, setAvSaveMsg] = useState<string | null>(null);

  useEffect(() => {
    api
      .hasAlphaVantageKey()
      .then(setAvKeySaved)
      .catch(() => setAvKeySaved(false));
  }, []);

  async function handleSaveAlphaVantageKey() {
    if (!avKeyInput.trim()) return;
    try {
      await api.saveAlphaVantageKey(avKeyInput.trim());
      setAvKeySaved(true);
      setAvKeyInput("");
      setAvSaveMsg("Saved. Takes effect immediately — no restart needed.");
    } catch (e) {
      setAvSaveMsg(String(e));
    }
  }

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

  type AiProvider = "anthropic" | "openai" | "gemini";
  const AI_PROVIDERS: AiProvider[] = ["anthropic", "openai", "gemini"];
  const aiDefaultModel: Record<AiProvider, string> = {
    anthropic: "claude-sonnet-5",
    openai: "gpt-4o",
    gemini: "gemini-2.0-flash",
  };
  const [aiKeyInput, setAiKeyInput] = useState<Partial<Record<AiProvider, string>>>({});
  const [aiModelInput, setAiModelInput] = useState<Partial<Record<AiProvider, string>>>({});
  const [aiKeySaved, setAiKeySaved] = useState<Partial<Record<AiProvider, boolean>>>({});
  const [aiSaveMsg, setAiSaveMsg] = useState<Partial<Record<AiProvider, string>>>({});

  useEffect(() => {
    AI_PROVIDERS.forEach((provider) => {
      api
        .hasAiProviderKey(provider)
        .then((saved) => setAiKeySaved((prev) => ({ ...prev, [provider]: saved })))
        .catch(() => setAiKeySaved((prev) => ({ ...prev, [provider]: false })));
    });
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  async function handleSaveAiKey(provider: AiProvider) {
    const value = aiKeyInput[provider];
    if (!value?.trim()) return;
    try {
      await api.saveAiProviderKey(provider, value.trim());
      setAiKeySaved((prev) => ({ ...prev, [provider]: true }));
      setAiKeyInput((prev) => ({ ...prev, [provider]: "" }));
      setAiSaveMsg((prev) => ({ ...prev, [provider]: "Saved. Takes effect immediately." }));
    } catch (e) {
      setAiSaveMsg((prev) => ({ ...prev, [provider]: String(e) }));
    }
  }

  async function handleSaveAiModel(provider: AiProvider) {
    const value = aiModelInput[provider];
    if (!value?.trim()) return;
    try {
      await api.saveAiProviderModel(provider, value.trim());
      setAiModelInput((prev) => ({ ...prev, [provider]: "" }));
      setAiSaveMsg((prev) => ({ ...prev, [provider]: `Model set to "${value.trim()}".` }));
    } catch (e) {
      setAiSaveMsg((prev) => ({ ...prev, [provider]: String(e) }));
    }
  }
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
        <p style={{ fontSize: 13, fontWeight: 600, margin: "0 0 6px" }}>Data sources</p>
        <p style={{ fontSize: 12, color: colors.textMuted, margin: "0 0 10px" }}>
          Yahoo Finance stays primary — this key is only ever used as a fallback when a Yahoo
          request fails. Live-verified for India (BSE), US, and UK. Stored locally in your own
          database only; never committed to GitHub, never synced anywhere.
        </p>
        <p style={{ fontSize: 12, margin: "0 0 8px" }}>
          Alpha Vantage key:{" "}
          {avKeySaved === null ? "checking…" : avKeySaved ? (
            <span style={{ color: colors.success, fontWeight: 600 }}>saved</span>
          ) : (
            <span style={{ color: colors.textMuted }}>not set</span>
          )}
        </p>
        <div style={{ display: "flex", gap: 8, alignItems: "center" }}>
          <input
            type="password"
            value={avKeyInput}
            onChange={(e) => setAvKeyInput(e.target.value)}
            placeholder={avKeySaved ? "Enter a new key to replace it" : "Paste your Alpha Vantage API key"}
            style={{ width: 260 }}
          />
          <button onClick={handleSaveAlphaVantageKey} disabled={!avKeyInput.trim()}>
            Save
          </button>
        </div>
        {avSaveMsg && <p style={{ fontSize: 12, color: colors.textMuted, marginTop: 8 }}>{avSaveMsg}</p>}
      </div>

      <div style={{ ...panelStyle, marginBottom: 16 }}>
        <p style={{ fontSize: 13, fontWeight: 600, margin: "0 0 6px" }}>AI portfolio insights</p>
        <p style={{ fontSize: 12, color: colors.textMuted, margin: "0 0 10px" }}>
          Configure any of the three below, then use "Get AI Insights" on the Dashboard. Every
          analysis is a deliberate action — nothing here is called automatically. Keys are stored
          locally in your own database only; never committed to GitHub, never synced anywhere.
        </p>
        {(["anthropic", "openai", "gemini"] as const).map((provider) => (
          <div key={provider} style={{ marginBottom: 14, paddingBottom: 14, borderBottom: `1px solid ${colors.border}` }}>
            <p style={{ fontSize: 12, fontWeight: 600, margin: "0 0 4px", textTransform: "capitalize" }}>
              {provider}
            </p>
            <p style={{ fontSize: 12, margin: "0 0 6px" }}>
              Key:{" "}
              {aiKeySaved[provider] === undefined ? "checking…" : aiKeySaved[provider] ? (
                <span style={{ color: colors.success, fontWeight: 600 }}>saved</span>
              ) : (
                <span style={{ color: colors.textMuted }}>not set</span>
              )}
            </p>
            <div style={{ display: "flex", gap: 8, alignItems: "center", marginBottom: 6 }}>
              <input
                type="password"
                value={aiKeyInput[provider] ?? ""}
                onChange={(e) => setAiKeyInput((prev) => ({ ...prev, [provider]: e.target.value }))}
                placeholder={aiKeySaved[provider] ? "Enter a new key to replace it" : `Paste your ${provider} API key`}
                style={{ width: 240 }}
              />
              <button onClick={() => handleSaveAiKey(provider)} disabled={!aiKeyInput[provider]?.trim()}>
                Save
              </button>
            </div>
            <div style={{ display: "flex", gap: 8, alignItems: "center" }}>
              <span style={{ fontSize: 11, color: colors.textMuted }}>Model:</span>
              <input
                value={aiModelInput[provider] ?? ""}
                onChange={(e) => setAiModelInput((prev) => ({ ...prev, [provider]: e.target.value }))}
                placeholder={aiDefaultModel[provider]}
                style={{ width: 200, fontSize: 12 }}
              />
              <button onClick={() => handleSaveAiModel(provider)} disabled={!aiModelInput[provider]?.trim()} style={{ fontSize: 11 }}>
                Save model
              </button>
            </div>
            {aiSaveMsg[provider] && <p style={{ fontSize: 11, color: colors.textMuted, marginTop: 6 }}>{aiSaveMsg[provider]}</p>}
          </div>
        ))}
        <p style={{ fontSize: 11, color: colors.textMuted, margin: 0 }}>
          Model names are configurable rather than fixed — provider model availability changes
          over time, and a stale hardcoded model name would be the most likely single point of
          failure here. Defaults shown as placeholders above.
        </p>
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

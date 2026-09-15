import { useEffect, useState } from "react";
import { check } from "@tauri-apps/plugin-updater";
import { relaunch } from "@tauri-apps/plugin-process";
import { api } from "../lib/tauri";
import type { PortfolioView } from "../lib/types";
import { colors, panelStyle } from "../lib/theme";
import { ConfirmButton } from "../components/ConfirmButton";
import { ConnectionIndicator } from "../components/ConnectionIndicator";

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

  const [upstoxTokenInput, setUpstoxTokenInput] = useState("");
  const [upstoxTokenSaved, setUpstoxTokenSaved] = useState<boolean | null>(null);
  const [upstoxSaveMsg, setUpstoxSaveMsg] = useState<string | null>(null);
  const [upstoxRefreshing, setUpstoxRefreshing] = useState(false);
  const [upstoxRefreshMsg, setUpstoxRefreshMsg] = useState<string | null>(null);

  useEffect(() => {
    api
      .hasUpstoxToken()
      .then(setUpstoxTokenSaved)
      .catch(() => setUpstoxTokenSaved(false));
  }, []);

  async function handleSaveUpstoxToken() {
    if (!upstoxTokenInput.trim()) return;
    try {
      await api.saveUpstoxToken(upstoxTokenInput.trim());
      setUpstoxTokenSaved(true);
      setUpstoxTokenInput("");
      setUpstoxSaveMsg("Saved. Takes effect immediately.");
    } catch (e) {
      setUpstoxSaveMsg(String(e));
    }
  }

  async function handleRefreshUpstoxInstruments() {
    setUpstoxRefreshing(true);
    setUpstoxRefreshMsg(null);
    try {
      const result = await api.refreshUpstoxInstrumentCache();
      setUpstoxRefreshMsg(`Cached ${result.instrument_count.toLocaleString()} NSE + BSE equity instruments.`);
    } catch (e) {
      setUpstoxRefreshMsg(String(e));
    } finally {
      setUpstoxRefreshing(false);
    }
  }

  const [priorityOrder, setPriorityOrder] = useState<string[]>(["upstox", "yahoo", "alpha_vantage"]);
  const [priorityMsg, setPriorityMsg] = useState<string | null>(null);

  useEffect(() => {
    api
      .getMarketDataPriority()
      .then((order) => setPriorityOrder(order.split(",").map((s) => s.trim()).filter(Boolean)))
      .catch(() => {});
  }, []);

  async function moveProviderPriority(index: number, direction: -1 | 1) {
    const next = [...priorityOrder];
    const target = index + direction;
    if (target < 0 || target >= next.length) return;
    [next[index], next[target]] = [next[target], next[index]];
    setPriorityOrder(next);
    try {
      await api.saveMarketDataPriority(next.join(","));
      setPriorityMsg("Order saved. Takes effect immediately.");
    } catch (e) {
      setPriorityMsg(String(e));
    }
  }

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

  const [flashThresholdInput, setFlashThresholdInput] = useState("");
  const [flashThresholdMsg, setFlashThresholdMsg] = useState<string | null>(null);

  const [fontScale, setFontScale] = useState(100);
  const [fontScaleMsg, setFontScaleMsg] = useState<string | null>(null);

  useEffect(() => {
    api
      .getFontScale()
      .then(setFontScale)
      .catch(() => {});
  }, []);

  async function handleSaveFontScale(pct: number) {
    // Applied immediately, not just on next launch — document.body isn't
    // part of React's managed tree, so setting it directly here doesn't
    // conflict with anything React itself renders.
    document.body.style.zoom = `${pct}%`;
    setFontScale(pct);
    try {
      await api.saveFontScale(pct);
      setFontScaleMsg(`Set to ${pct}%.`);
    } catch (e) {
      setFontScaleMsg(String(e));
    }
  }

  useEffect(() => {
    api
      .getFlashThreshold()
      .then((pct) => setFlashThresholdInput(pct.toString()))
      .catch(() => {});
  }, []);

  async function handleSaveFlashThreshold() {
    const value = parseFloat(flashThresholdInput);
    if (!Number.isFinite(value) || value <= 0) {
      setFlashThresholdMsg("Enter a positive number.");
      return;
    }
    try {
      await api.saveFlashThreshold(value);
      setFlashThresholdMsg(`Saved — flash now triggers at ±${value}%.`);
    } catch (e) {
      setFlashThresholdMsg(String(e));
    }
  }

  const [zerodhaApiKeyInput, setZerodhaApiKeyInput] = useState("");
  const [zerodhaApiSecretInput, setZerodhaApiSecretInput] = useState("");
  const [zerodhaCredsSaved, setZerodhaCredsSaved] = useState<boolean | null>(null);
  const [zerodhaSessionValid, setZerodhaSessionValid] = useState<boolean | null>(null);
  const [zerodhaConnecting, setZerodhaConnecting] = useState(false);
  const [zerodhaMsg, setZerodhaMsg] = useState<string | null>(null);

  useEffect(() => {
    api.hasZerodhaCredentials().then(setZerodhaCredsSaved).catch(() => setZerodhaCredsSaved(false));
    api.hasValidZerodhaSession().then(setZerodhaSessionValid).catch(() => setZerodhaSessionValid(false));
  }, []);

  async function handleSaveZerodhaCredentials() {
    if (!zerodhaApiKeyInput.trim() || !zerodhaApiSecretInput.trim()) return;
    try {
      await api.saveZerodhaCredentials(zerodhaApiKeyInput.trim(), zerodhaApiSecretInput.trim());
      setZerodhaCredsSaved(true);
      setZerodhaApiKeyInput("");
      setZerodhaApiSecretInput("");
      setZerodhaMsg("Saved.");
    } catch (e) {
      setZerodhaMsg(String(e));
    }
  }

  async function handleConnectZerodha() {
    setZerodhaConnecting(true);
    setZerodhaMsg(null);
    try {
      const result = await api.connectZerodha();
      setZerodhaSessionValid(true);
      setZerodhaMsg(result);
    } catch (e) {
      setZerodhaMsg(String(e));
    } finally {
      setZerodhaConnecting(false);
    }
  }

  const [kiteRefreshing, setKiteRefreshing] = useState(false);
  const [kiteRefreshMsg, setKiteRefreshMsg] = useState<string | null>(null);

  async function handleRefreshKiteInstruments() {
    setKiteRefreshing(true);
    setKiteRefreshMsg(null);
    try {
      const result = await api.refreshKiteInstrumentCache();
      setKiteRefreshMsg(`Cached ${result.instrument_count.toLocaleString()} NSE + BSE equity instruments.`);
    } catch (e) {
      setKiteRefreshMsg(String(e));
    } finally {
      setKiteRefreshing(false);
    }
  }

  const [cleanupRunning, setCleanupRunning] = useState(false);
  const [cleanupMsg, setCleanupMsg] = useState<string | null>(null);

  async function handleCleanupNonIndian() {
    setCleanupRunning(true);
    setCleanupMsg(null);
    try {
      const result = await api.removeNonIndianInstruments();
      const parts = [];
      parts.push(result.removed.length > 0 ? `Removed: ${result.removed.join(", ")}.` : "Nothing to remove.");
      if (result.kept.length > 0) parts.push(`Left alone (still held somewhere): ${result.kept.join(", ")}.`);
      setCleanupMsg(parts.join(" "));
    } catch (e) {
      setCleanupMsg(String(e));
    } finally {
      setCleanupRunning(false);
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
        <p style={{ fontSize: 13, fontWeight: 600, margin: "0 0 6px" }}>Data sources — priority order</p>
        <p style={{ fontSize: 12, color: colors.textMuted, margin: "0 0 10px" }}>
          Tried top to bottom — the next source is only ever used when the one above it fails.
          Reorder with the arrows. Green means a real test call just succeeded, not just that a
          key is saved.
        </p>
        {priorityOrder.map((provider, i) => (
          <div key={provider} style={{ display: "flex", alignItems: "center", gap: 10, padding: "6px 0", borderBottom: i < priorityOrder.length - 1 ? `1px solid ${colors.border}` : undefined }}>
            <span style={{ fontSize: 12, color: colors.textMuted, width: 16 }}>{i + 1}.</span>
            <span style={{ fontSize: 13, fontWeight: 600, textTransform: "capitalize", width: 110 }}>
              {provider === "alpha_vantage" ? "Alpha Vantage" : provider}
            </span>
            <ConnectionIndicator onTest={() => api.testMarketDataConnection(provider)} />
            <span style={{ marginLeft: "auto", display: "flex", gap: 4 }}>
              <button onClick={() => moveProviderPriority(i, -1)} disabled={i === 0} style={{ fontSize: 11, padding: "2px 8px" }}>
                ↑
              </button>
              <button
                onClick={() => moveProviderPriority(i, 1)}
                disabled={i === priorityOrder.length - 1}
                style={{ fontSize: 11, padding: "2px 8px" }}
              >
                ↓
              </button>
            </span>
          </div>
        ))}
        {priorityMsg && <p style={{ fontSize: 11, color: colors.textMuted, marginTop: 8 }}>{priorityMsg}</p>}
      </div>

      <div style={{ ...panelStyle, marginBottom: 16 }}>
        <p style={{ fontSize: 13, fontWeight: 600, margin: "0 0 6px" }}>Upstox</p>
        <p style={{ fontSize: 12, color: colors.textMuted, margin: "0 0 8px" }}>
          Needs an actual Upstox trading account. Generate a free Analytics Token (valid 1 year, no
          daily re-login) from Upstox's Developer Apps page, Analytics tab. India (NSE/BSE) only —
          also powers real Fundamentals and News once connected.
        </p>
        <p style={{ fontSize: 12, margin: "0 0 8px" }}>
          Analytics Token:{" "}
          {upstoxTokenSaved === null ? "checking…" : upstoxTokenSaved ? (
            <span style={{ color: colors.success, fontWeight: 600 }}>saved</span>
          ) : (
            <span style={{ color: colors.textMuted }}>not set</span>
          )}
        </p>
        <div style={{ display: "flex", gap: 8, alignItems: "center", marginBottom: 8 }}>
          <input
            type="password"
            value={upstoxTokenInput}
            onChange={(e) => setUpstoxTokenInput(e.target.value)}
            placeholder={upstoxTokenSaved ? "Enter a new token to replace it" : "Paste your Upstox Analytics Token"}
            style={{ width: 260 }}
          />
          <button onClick={handleSaveUpstoxToken} disabled={!upstoxTokenInput.trim()}>
            Save
          </button>
        </div>
        {upstoxSaveMsg && <p style={{ fontSize: 12, color: colors.textMuted, marginBottom: 8 }}>{upstoxSaveMsg}</p>}
        <div style={{ display: "flex", gap: 8, alignItems: "center" }}>
          <button onClick={handleRefreshUpstoxInstruments} disabled={upstoxRefreshing || !upstoxTokenSaved}>
            {upstoxRefreshing ? "Refreshing…" : "Refresh Instrument List"}
          </button>
          <span style={{ fontSize: 11, color: colors.textMuted }}>
            Optional now — symbols resolve automatically on first use via Upstox's Search API. This
            bulk download is only useful for pre-warming many symbols at once, and depends on a
            separate file Upstox publishes that's been unreliable in testing.
          </span>
        </div>
        {upstoxRefreshMsg && <p style={{ fontSize: 12, color: colors.textMuted, marginTop: 8 }}>{upstoxRefreshMsg}</p>}
      </div>

      <div style={{ ...panelStyle, marginBottom: 16 }}>
        <p style={{ fontSize: 13, fontWeight: 600, margin: "0 0 6px" }}>Yahoo Finance</p>
        <p style={{ fontSize: 12, color: colors.textMuted, margin: "0 0 8px" }}>
          No account or key needed — the original, unofficial-endpoint source this app started
          with.
        </p>
        <ConnectionIndicator onTest={() => api.testMarketDataConnection("yahoo")} />
      </div>

      <div style={{ ...panelStyle, marginBottom: 16 }}>
        <p style={{ fontSize: 13, fontWeight: 600, margin: "0 0 6px" }}>NSE / BSE Announcements</p>
        <p style={{ fontSize: 12, color: colors.textMuted, margin: "0 0 8px" }}>
          No account or key needed — real corporate announcements pulled directly from each
          exchange's own site, replacing the old keyword-guessed "regulatory" tagging. Both are
          unofficial endpoints; NSE specifically blocks requests from cloud/datacenter networks
          (this app runs on your own machine, so that shouldn't affect you, but it's why this
          can't be tested from anywhere except here).
        </p>
        <div style={{ display: "flex", gap: 16, alignItems: "center" }}>
          <span style={{ fontSize: 12, fontWeight: 600 }}>NSE:</span>
          <ConnectionIndicator onTest={() => api.testAnnouncementsConnection("nse")} />
        </div>
        <div style={{ display: "flex", gap: 16, alignItems: "center", marginTop: 8 }}>
          <span style={{ fontSize: 12, fontWeight: 600 }}>BSE:</span>
          <ConnectionIndicator onTest={() => api.testAnnouncementsConnection("bse")} />
        </div>
      </div>

      <div style={{ ...panelStyle, marginBottom: 16 }}>
        <p style={{ fontSize: 13, fontWeight: 600, margin: "0 0 6px" }}>Alpha Vantage</p>
        <p style={{ fontSize: 12, color: colors.textMuted, margin: "0 0 10px" }}>
          Live-verified for India (BSE), US, and UK. Stored locally in your own database only;
          never committed to GitHub, never synced anywhere.
        </p>
        <p style={{ fontSize: 12, margin: "0 0 8px" }}>
          Alpha Vantage key:{" "}
          {avKeySaved === null ? "checking…" : avKeySaved ? (
            <span style={{ color: colors.success, fontWeight: 600 }}>saved</span>
          ) : (
            <span style={{ color: colors.textMuted }}>not set</span>
          )}
        </p>
        <div style={{ display: "flex", gap: 8, alignItems: "center", marginBottom: 8 }}>
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
        {avSaveMsg && <p style={{ fontSize: 12, color: colors.textMuted, marginBottom: 8 }}>{avSaveMsg}</p>}
        <ConnectionIndicator onTest={() => api.testMarketDataConnection("alpha_vantage")} />
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
            {aiSaveMsg[provider] && <p style={{ fontSize: 11, color: colors.textMuted, marginTop: 6, marginBottom: 6 }}>{aiSaveMsg[provider]}</p>}
            <ConnectionIndicator onTest={() => api.testAiProviderConnection(provider)} />
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
        <p style={{ fontSize: 13, fontWeight: 600, margin: "0 0 6px" }}>Clean up non-Indian tickers</p>
        <p style={{ fontSize: 12, color: colors.textMuted, margin: "0 0 10px" }}>
          Removes any tracked ticker not on NSE/BSE — leftovers from before the country/market
          selector was removed. Anything still genuinely held in a portfolio is left alone and
          reported separately, never silently deleted.
        </p>
        <button onClick={handleCleanupNonIndian} disabled={cleanupRunning}>
          {cleanupRunning ? "Cleaning up…" : "Remove non-Indian tickers"}
        </button>
        {cleanupMsg && <p style={{ fontSize: 12, color: colors.textMuted, marginTop: 8 }}>{cleanupMsg}</p>}
      </div>

      <div style={{ ...panelStyle, marginBottom: 16 }}>
        <p style={{ fontSize: 13, fontWeight: 600, margin: "0 0 6px" }}>Display</p>
        <p style={{ fontSize: 12, color: colors.textMuted, margin: "0 0 8px" }}>
          The day-move flash on Holdings/Watchlist rows — how big a move triggers the amber/green
          blink. Was fixed at 3.5%, now yours to set.
        </p>
        <div style={{ display: "flex", gap: 8, alignItems: "center" }}>
          <input
            type="number"
            step="0.1"
            min="0.1"
            value={flashThresholdInput}
            onChange={(e) => setFlashThresholdInput(e.target.value)}
            style={{ width: 70 }}
          />
          <span style={{ fontSize: 12 }}>%</span>
          <button onClick={handleSaveFlashThreshold} disabled={!flashThresholdInput.trim()}>
            Save
          </button>
        </div>
        {flashThresholdMsg && <p style={{ fontSize: 12, color: colors.textMuted, marginTop: 8 }}>{flashThresholdMsg}</p>}

        <p style={{ fontSize: 12, fontWeight: 600, margin: "16px 0 8px" }}>App text size</p>
        <div style={{ display: "flex", gap: 8, alignItems: "center" }}>
          {[85, 100, 115, 130, 150].map((pct) => (
            <button
              key={pct}
              onClick={() => handleSaveFontScale(pct)}
              style={{
                fontSize: 12,
                padding: "4px 10px",
                borderRadius: 6,
                border: `1px solid ${fontScale === pct ? colors.accent : colors.border}`,
                background: fontScale === pct ? "#E6F1FB" : "transparent",
                color: fontScale === pct ? colors.accent : colors.textMuted,
                fontWeight: fontScale === pct ? 600 : 400,
                cursor: "pointer",
              }}
            >
              {pct}%
            </button>
          ))}
        </div>
        {fontScaleMsg && <p style={{ fontSize: 12, color: colors.textMuted, marginTop: 8 }}>{fontScaleMsg}</p>}
      </div>

      <div style={{ ...panelStyle, marginBottom: 16 }}>
        <p style={{ fontSize: 13, fontWeight: 600, margin: "0 0 6px" }}>Zerodha (Kite Connect)</p>
        <p style={{ fontSize: 12, color: colors.textMuted, margin: "0 0 8px" }}>
          Needs a Kite Connect subscription (₹500/month) — separate from a normal Zerodha account.
          Unlike Upstox, there's no year-long token: a fresh login is required every trading day.
          "Connect Zerodha" opens Zerodha's real login page in your browser; nothing runs locally
          until you click it, and it stops listening the moment login completes or after 3 minutes.
        </p>
        <p style={{ fontSize: 12, margin: "0 0 8px" }}>
          Credentials:{" "}
          {zerodhaCredsSaved === null ? "checking…" : zerodhaCredsSaved ? (
            <span style={{ color: colors.success, fontWeight: 600 }}>saved</span>
          ) : (
            <span style={{ color: colors.textMuted }}>not set</span>
          )}
          {"  ·  Today's session: "}
          {zerodhaSessionValid === null ? "checking…" : zerodhaSessionValid ? (
            <span style={{ color: colors.success, fontWeight: 600 }}>connected</span>
          ) : (
            <span style={{ color: colors.textMuted }}>not connected</span>
          )}
        </p>
        <div style={{ display: "flex", gap: 8, alignItems: "center", marginBottom: 8, flexWrap: "wrap" }}>
          <input
            type="password"
            value={zerodhaApiKeyInput}
            onChange={(e) => setZerodhaApiKeyInput(e.target.value)}
            placeholder="API key"
            style={{ width: 160 }}
          />
          <input
            type="password"
            value={zerodhaApiSecretInput}
            onChange={(e) => setZerodhaApiSecretInput(e.target.value)}
            placeholder="API secret"
            style={{ width: 160 }}
          />
          <button onClick={handleSaveZerodhaCredentials} disabled={!zerodhaApiKeyInput.trim() || !zerodhaApiSecretInput.trim()}>
            Save
          </button>
        </div>
        <button onClick={handleConnectZerodha} disabled={!zerodhaCredsSaved || zerodhaConnecting}>
          {zerodhaConnecting ? "Waiting for login…" : "Connect Zerodha"}
        </button>{" "}
        <button onClick={handleRefreshKiteInstruments} disabled={!zerodhaSessionValid || kiteRefreshing}>
          {kiteRefreshing ? "Refreshing…" : "Refresh Instrument List"}
        </button>
        {zerodhaMsg && <p style={{ fontSize: 12, color: colors.textMuted, marginTop: 8 }}>{zerodhaMsg}</p>}
        {kiteRefreshMsg && <p style={{ fontSize: 12, color: colors.textMuted, marginTop: 8 }}>{kiteRefreshMsg}</p>}
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

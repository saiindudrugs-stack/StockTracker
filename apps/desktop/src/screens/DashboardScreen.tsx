import { useEffect, useState } from "react";
import { api, subscribeLivePriceTicks } from "../lib/tauri";
import { LiveStreamControl } from "../components/LiveStreamControl";
import type { AlertRuleView, DashboardSummary, HoldingView, MarketSummaryView, TaxSummaryView, TaxLossHarvestingView, PortfolioSummaryRow } from "../lib/types";
import { cardStyle, colors, panelStyle, pnlColor, fmtMoney, tableHeaderRow, tableHeaderCell, firstHeaderCell, lastHeaderCell, zebraRowTint } from "../lib/theme";

// A few distinct, low-saturation colors for the sector breakdown bars —
// enough for a handful of sectors; this is demo-scale data (2 instruments),
// not a real allocation engine.
const SECTOR_COLORS = ["#2E74B5", "#5B9BD5", "#9DC3E6", "#1F3864", "#7F9EC2"];

type AiTag = "CONCERN" | "POSITIVE" | "QUESTION" | "INFO";

/// Splits the model's response into lines and pulls off a leading
/// [TAG] marker where present — see build_portfolio_prompt in
/// ai_insights.rs for why this is a tag the model writes itself rather
/// than sentiment we try to guess from prose client-side (guessing would
/// be fragile; asking the model to tag its own points is reliable).
/// Untagged lines (blank lines, or if the model didn't follow the format
/// for some line) just render plain — never blocks the response.
function parseAiInsightLines(raw: string): { tag: AiTag | null; text: string }[] {
  const tagPattern = /^\[(CONCERN|POSITIVE|QUESTION|INFO)\]\s*/;
  return raw
    .split("\n")
    .map((line) => line.trim())
    .filter((line) => line.length > 0)
    .map((line) => {
      const match = line.match(tagPattern);
      if (match) {
        return { tag: match[1] as AiTag, text: line.slice(match[0].length) };
      }
      return { tag: null, text: line };
    });
}

/// Deliberately subtle — a thin left border plus a ~8% tint, not a loud
/// background fill. Per research on fintech color usage: red/green (and
/// their neighbors here) should read as signals used sparingly, not as
/// decoration competing with the actual P&L colors used everywhere else
/// in this app.
function aiTagColor(tag: AiTag): string {
  switch (tag) {
    case "CONCERN":
      return "#B45309"; // amber, matches the existing "regulatory" convention on the News screen
    case "POSITIVE":
      return colors.success;
    case "QUESTION":
      return colors.accent;
    case "INFO":
      return colors.textMuted;
  }
}

export function DashboardScreen({ portfolioId, isMyPortfolio }: { portfolioId: string; isMyPortfolio?: boolean }) {
  const [summary, setSummary] = useState<DashboardSummary | null>(null);
  const [holdings, setHoldings] = useState<HoldingView[]>([]);
  const [xirr, setXirr] = useState<number | null>(null);
  const [xirrError, setXirrError] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [alertRules, setAlertRules] = useState<AlertRuleView[]>([]);
  const [marketSummaries, setMarketSummaries] = useState<MarketSummaryView[]>([]);

  const [allPortfolios, setAllPortfolios] = useState<PortfolioSummaryRow[] | null>(null);
  const [allHeldSymbols, setAllHeldSymbols] = useState<string[] | null>(null);

  // The Consolidated table aggregates every portfolio, so a live stream
  // that's actually live for it needs every symbol held anywhere — not
  // just this one portfolio's holdings, which is what starting the
  // stream from here used to be limited to (and could easily be empty,
  // silently making the whole consolidated view non-live).
  useEffect(() => {
    if (!isMyPortfolio) return;
    api.getAllHeldSymbols().then(setAllHeldSymbols).catch(() => {});
  }, [isMyPortfolio]);

  useEffect(() => {
    if (!isMyPortfolio) return;
    api.getAllPortfoliosSummary().then(setAllPortfolios).catch(() => {});
  }, [isMyPortfolio]);

  // Re-fetches the whole consolidated table on a live tick rather than
  // trying to recompute each portfolio's totals incrementally client-side
  // — that would need this screen to also hold every portfolio's full
  // holdings list just to know which quantity a tick's price-change
  // applies to, a much bigger data-fetching burden than just asking the
  // backend for fresh totals. Debounced so a burst of ticks doesn't
  // trigger a re-fetch per tick.
  useEffect(() => {
    if (!isMyPortfolio) return;
    let debounceTimer: ReturnType<typeof setTimeout> | null = null;
    const unsubscribe = subscribeLivePriceTicks(() => {
      if (debounceTimer) clearTimeout(debounceTimer);
      debounceTimer = setTimeout(() => {
        api.getAllPortfoliosSummary().then(setAllPortfolios).catch(() => {});
      }, 1500);
    });
    return () => {
      if (debounceTimer) clearTimeout(debounceTimer);
      unsubscribe();
    };
  }, [isMyPortfolio]);

  const AI_PROVIDERS = ["anthropic", "openai", "gemini"] as const;
  const [availableAiProviders, setAvailableAiProviders] = useState<string[]>([]);

  const [taxSummary, setTaxSummary] = useState<TaxSummaryView | null>(null);
  const [taxSummaryExpanded, setTaxSummaryExpanded] = useState(false);
  const [taxLossView, setTaxLossView] = useState<TaxLossHarvestingView | null>(null);
  const [taxLossExpanded, setTaxLossExpanded] = useState(false);

  const [targetAllocations, setTargetAllocations] = useState<Record<string, number>>({});
  const [editingTargets, setEditingTargets] = useState(false);
  const [targetInputs, setTargetInputs] = useState<Record<string, string>>({});
  const [targetSaveMsg, setTargetSaveMsg] = useState<string | null>(null);
  // How many percentage points off target counts as "worth flagging" —
  // matches the standard "percentage-of-portfolio rebalancing" threshold
  // approach (one of the three well-established institutional rebalancing
  // techniques), not an arbitrary number.
  const DRIFT_THRESHOLD_PCT = 5;

  useEffect(() => {
    api
      .getTargetAllocation()
      .then((json) => {
        try {
          setTargetAllocations(JSON.parse(json));
        } catch {
          setTargetAllocations({});
        }
      })
      .catch(() => {});
  }, []);

  async function handleSaveTargets() {
    const parsed: Record<string, number> = {};
    for (const [sector, value] of Object.entries(targetInputs)) {
      const num = parseFloat(value);
      if (Number.isFinite(num) && num >= 0) parsed[sector] = num;
    }
    try {
      await api.saveTargetAllocation(JSON.stringify(parsed));
      setTargetAllocations(parsed);
      setEditingTargets(false);
      setTargetSaveMsg("Saved.");
    } catch (e) {
      setTargetSaveMsg(String(e));
    }
  }

  useEffect(() => {
    // Independent of the main Promise.all above, same reasoning as
    // marketSummaries — supplementary, shouldn't fail alongside the core
    // dashboard numbers.
    api.getTaxSummary(portfolioId).then(setTaxSummary).catch(() => {});
    api.getTaxLossHarvestingCandidates(portfolioId).then(setTaxLossView).catch(() => {});
  }, [portfolioId]);
  const [selectedAiProvider, setSelectedAiProvider] = useState<string>("");
  const [aiConfirming, setAiConfirming] = useState(false);
  const [aiLoading, setAiLoading] = useState(false);
  const [aiInsights, setAiInsights] = useState<string | null>(null);
  const [aiError, setAiError] = useState<string | null>(null);

  useEffect(() => {
    Promise.all(
      AI_PROVIDERS.map((p) => api.hasAiProviderKey(p).then((saved) => (saved ? p : null)).catch(() => null))
    ).then((results) => {
      const available = results.filter((p): p is (typeof AI_PROVIDERS)[number] => p !== null);
      setAvailableAiProviders(available);
      if (available.length > 0) setSelectedAiProvider(available[0]);
    });
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [portfolioId]);

  async function handleGenerateInsights() {
    setAiConfirming(false);
    setAiLoading(true);
    setAiError(null);
    setAiInsights(null);
    try {
      const result = await api.generatePortfolioInsights(portfolioId, selectedAiProvider);
      setAiInsights(result);
    } catch (e) {
      setAiError(String(e));
    } finally {
      setAiLoading(false);
    }
  }

  function refreshAlerts() {
    api
      .listAlertRules(portfolioId)
      .then(setAlertRules)
      .catch((e) => setError(String(e)));
  }

  async function handleDismissAlert(id: string) {
    try {
      await api.deleteAlertRule(id);
      refreshAlerts();
    } catch (e) {
      setError(String(e));
    }
  }

  useEffect(() => {
    Promise.all([api.getDashboardSummary(portfolioId), api.listHoldings(portfolioId)])
      .then(([s, h]) => {
        setSummary(s);
        setHoldings(h);
      })
      .catch((e) => setError(String(e)));
    refreshAlerts();

    // Independent from the Promise.all above on purpose — this is a
    // supplementary breakdown, not something the rest of the dashboard
    // should fail alongside if it errors.
    api
      .getDashboardByMarket(portfolioId)
      .then(setMarketSummaries)
      .catch(() => setMarketSummaries([]));

    // Kept separate from the Promise.all above: XIRR can legitimately fail
    // to compute (e.g. no priced holdings, or fewer than one inflow/outflow
    // pair) even when the rest of the dashboard is fine — a solver error
    // here shouldn't blank out net worth and P/L too.
    setXirr(null);
    setXirrError(null);
    api
      .computePortfolioXirr(portfolioId)
      .then(setXirr)
      .catch((e) => setXirrError(String(e)));
  }, [portfolioId]);

  // Allocation by sector, computed client-side from market value — there's
  // no dedicated allocation use-case yet (SRS 2.2.3 "Asset Allocation,
  // Sector Allocation" isn't wired up as its own backend command), so this
  // is derived from list_holdings rather than a real analytics engine call.
  const bySector = new Map<string, number>();
  let totalMarketValue = 0;
  for (const h of holdings) {
    const mv = h.market_value ? parseFloat(h.market_value) : 0;
    const sector = h.sector ?? "Unclassified";
    bySector.set(sector, (bySector.get(sector) ?? 0) + mv);
    totalMarketValue += mv;
  }
  const allocation = Array.from(bySector.entries()).sort((a, b) => b[1] - a[1]);

  return (
    <div style={{ padding: 24 }}>
      <h1 style={{ fontSize: 20, color: colors.navy, marginBottom: 4 }}>Dashboard</h1>
      <p style={{ fontSize: 13, color: colors.textMuted, marginTop: 0 }}>
        Real numbers from the SQLite ledger below. Sections marked "not built yet" are honest
        placeholders, not missing data — those backend pieces (alerts, calendar, live intraday
        feed) don't exist yet in this slice.
      </p>

      {error && <p style={{ color: colors.danger }}>{error}</p>}

      {summary && (
        <>
          <p style={{ fontSize: 11, color: colors.textMuted, margin: "0 0 8px" }}>
            Prices shown here are last-refreshed values, not live — use the{" "}
            <strong>Refresh Prices</strong> button on the Holdings screen to pull fresh ones.
          </p>
          <div style={{ display: "flex", gap: 12, margin: "0 0 16px" }}>
          <div style={{ ...cardStyle, minWidth: 160 }}>
            <div style={{ fontSize: 12, color: colors.textMuted }}>Net worth</div>
            <div style={{ fontSize: 18, fontWeight: 600 }}>₹{fmtMoney(summary.net_worth)}</div>
          </div>
          <div style={{ ...cardStyle, minWidth: 160 }}>
            <div style={{ fontSize: 12, color: colors.textMuted }}>Unrealized P/L</div>
            <div
              style={{
                fontSize: 18,
                fontWeight: 600,
                color:
                  parseFloat(summary.overall_unrealized_pnl) > 0
                    ? colors.success
                    : parseFloat(summary.overall_unrealized_pnl) < 0
                    ? colors.danger
                    : undefined,
              }}
            >
              ₹{fmtMoney(summary.overall_unrealized_pnl)}
            </div>
          </div>
          <div style={{ ...cardStyle, minWidth: 160 }}>
            <div style={{ fontSize: 12, color: colors.textMuted }}>Realized P/L</div>
            <div
              style={{
                fontSize: 18,
                fontWeight: 600,
                color:
                  parseFloat(summary.overall_realized_pnl) > 0
                    ? colors.success
                    : parseFloat(summary.overall_realized_pnl) < 0
                    ? colors.danger
                    : undefined,
              }}
            >
              ₹{fmtMoney(summary.overall_realized_pnl)}
            </div>
          </div>
          <div style={{ ...cardStyle, minWidth: 160 }}>
            <div style={{ fontSize: 12, color: colors.textMuted }}>Portfolio XIRR</div>
            {xirr != null ? (
              <div style={{ fontSize: 18, fontWeight: 600, color: xirr >= 0 ? colors.success : colors.danger }}>
                {(xirr * 100).toFixed(2)}%
              </div>
            ) : (
              <div style={{ fontSize: 12, color: colors.textMuted, marginTop: 4 }}>
                {xirrError ? "Not enough data yet" : "…"}
              </div>
            )}
          </div>
        </div>
        </>
      )}

      {isMyPortfolio && (
        <div style={{ marginBottom: 16 }}>
          <LiveStreamControl
            symbols={isMyPortfolio && allHeldSymbols ? allHeldSymbols : holdings.map((h) => h.symbol)}
            label={isMyPortfolio ? "Every portfolio's holdings (for the consolidated view below):" : "This portfolio's holdings:"}
          />
        </div>
      )}

      {isMyPortfolio && allPortfolios && allPortfolios.length > 0 && (
        <div style={{ ...panelStyle, marginBottom: 16 }}>
          <p style={{ fontSize: 12, color: colors.textMuted, margin: "0 0 8px", fontWeight: 600 }}>
            Consolidated — all portfolios
          </p>
          <p style={{ fontSize: 11, color: colors.textMuted, margin: "0 0 10px" }}>
            Every family portfolio side by side. Updates automatically a moment after any live
            price tick, if a live stream is running.
          </p>
          <table className="data-table" style={{ borderCollapse: "collapse", width: "100%", fontSize: 13 }}>
            <thead>
              <tr style={tableHeaderRow}>
                <th style={{ ...tableHeaderCell, ...firstHeaderCell }}>Portfolio</th>
                <th style={tableHeaderCell}>Net worth</th>
                <th style={tableHeaderCell}>Unrealized P/L</th>
                <th style={tableHeaderCell}>Realized P/L</th>
                <th style={tableHeaderCell}>Day's Gain/Loss</th>
                <th style={{ ...tableHeaderCell, ...lastHeaderCell }}>XIRR</th>
              </tr>
            </thead>
            <tbody>
              {allPortfolios.map((p, i) => (
                <tr key={p.portfolio_id} style={{ background: zebraRowTint(i) }}>
                  <td style={{ padding: "6px 8px 6px 0", fontWeight: 600 }}>{p.portfolio_name}</td>
                  <td style={{ padding: "6px 8px" }}>₹{fmtMoney(p.net_worth)}</td>
                  <td style={{ padding: "6px 8px", color: pnlColor(parseFloat(p.unrealized_pnl)) }}>₹{fmtMoney(p.unrealized_pnl)}</td>
                  <td style={{ padding: "6px 8px", color: pnlColor(parseFloat(p.realized_pnl)) }}>₹{fmtMoney(p.realized_pnl)}</td>
                  <td style={{ padding: "6px 8px", color: pnlColor(parseFloat(p.day_gain_loss)) }}>₹{fmtMoney(p.day_gain_loss)}</td>
                  <td style={{ padding: "6px 8px", color: p.xirr_pct != null ? pnlColor(p.xirr_pct) : undefined }}>
                    {p.xirr_pct != null ? `${p.xirr_pct.toFixed(2)}%` : "—"}
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      )}

      <div style={{ display: "grid", gridTemplateColumns: "1.3fr 1fr", gap: 12, marginTop: 8 }}>
        <div style={panelStyle}>
          <div style={{ display: "flex", justifyContent: "space-between", alignItems: "center", marginBottom: 10 }}>
            <p style={{ fontSize: 12, color: colors.textMuted, margin: 0, fontWeight: 600 }}>Sector allocation</p>
            <span
              onClick={() => {
                if (!editingTargets) {
                  const seed: Record<string, string> = {};
                  allocation.forEach(([sector]) => {
                    seed[sector] = (targetAllocations[sector] ?? "").toString();
                  });
                  setTargetInputs(seed);
                }
                setEditingTargets((v) => !v);
              }}
              style={{ fontSize: 11, color: colors.accent, cursor: "pointer" }}
            >
              {editingTargets ? "Cancel" : "Set targets"}
            </span>
          </div>
          {allocation.length === 0 ? (
            <p style={{ fontSize: 12, color: colors.textMuted }}>No priced holdings yet.</p>
          ) : (
            <div style={{ display: "flex", flexDirection: "column", gap: 8 }}>
              {allocation.map(([sector, value], i) => {
                const pct = totalMarketValue > 0 ? (value / totalMarketValue) * 100 : 0;
                const target = targetAllocations[sector];
                const drift = target != null ? pct - target : null;
                const isDrifted = drift != null && Math.abs(drift) > DRIFT_THRESHOLD_PCT;
                return (
                  <div key={sector}>
                    <div style={{ display: "flex", justifyContent: "space-between", fontSize: 12, marginBottom: 3 }}>
                      <span>{sector}</span>
                      {editingTargets ? (
                        <span style={{ display: "flex", alignItems: "center", gap: 4 }}>
                          <span style={{ color: colors.textMuted }}>{pct.toFixed(1)}% → target</span>
                          <input
                            value={targetInputs[sector] ?? ""}
                            onChange={(e) => setTargetInputs((prev) => ({ ...prev, [sector]: e.target.value }))}
                            style={{ width: 50, fontSize: 11 }}
                          />
                          <span style={{ fontSize: 11 }}>%</span>
                        </span>
                      ) : (
                        <span style={{ color: isDrifted ? colors.danger : colors.textMuted, fontWeight: isDrifted ? 600 : 400 }}>
                          {pct.toFixed(1)}%
                          {target != null && ` (target ${target}%${isDrifted ? `, drifted ${drift! > 0 ? "+" : ""}${drift!.toFixed(1)}pt` : ""})`}
                        </span>
                      )}
                    </div>
                    <div style={{ background: "#E5E5E5", borderRadius: 4, height: 8, overflow: "hidden", position: "relative" }}>
                      <div
                        style={{
                          width: `${pct}%`,
                          height: "100%",
                          background: isDrifted ? colors.danger : SECTOR_COLORS[i % SECTOR_COLORS.length],
                        }}
                      />
                      {target != null && (
                        <div style={{ position: "absolute", left: `${target}%`, top: 0, bottom: 0, width: 2, background: colors.navy }} />
                      )}
                    </div>
                  </div>
                );
              })}
              {editingTargets && (
                <div style={{ display: "flex", gap: 8, alignItems: "center", marginTop: 4 }}>
                  <button onClick={handleSaveTargets} style={{ fontSize: 11 }}>
                    Save targets
                  </button>
                  {targetSaveMsg && <span style={{ fontSize: 11, color: colors.textMuted }}>{targetSaveMsg}</span>}
                </div>
              )}
            </div>
          )}
        </div>

        <div style={panelStyle}>
          <p style={{ fontSize: 12, color: colors.textMuted, margin: "0 0 6px", fontWeight: 600 }}>
            Intraday positions
          </p>
          <p style={{ fontSize: 12, color: colors.textMuted, margin: 0 }}>
            Not built yet — requires a live broker connection (Zerodha's `fetch_intraday_positions`
            exists in the Rust adapter but isn't wired to a UI command in this slice).
          </p>
        </div>
      </div>

      {marketSummaries.length > 0 && (
        <div style={{ ...panelStyle, marginTop: 12 }}>
          <p style={{ fontSize: 12, color: colors.textMuted, margin: "0 0 4px", fontWeight: 600 }}>
            By market
          </p>
          <p style={{ fontSize: 11, color: colors.textMuted, margin: "0 0 10px" }}>
            Shown separately per country rather than blended into one number — this portfolio may hold
            instruments priced in different currencies, and summing rupees and dollars together as if
            they were the same unit would be misleading rather than precise. No currency conversion
            happens here.
          </p>
          <div style={{ display: "grid", gridTemplateColumns: `repeat(${marketSummaries.length}, 1fr)`, gap: 10 }}>
            {marketSummaries.map((m) => {
              const unrealized = parseFloat(m.unrealized_pnl);
              const realized = parseFloat(m.realized_pnl);
              return (
                <div key={m.country} style={{ ...cardStyle, padding: 12 }}>
                  <div style={{ fontSize: 12, fontWeight: 600, marginBottom: 6 }}>
                    {m.country}{" "}
                    <span style={{ fontWeight: 400, color: colors.textMuted }}>
                      ({m.holding_count} holding{m.holding_count === 1 ? "" : "s"})
                    </span>
                  </div>
                  <div style={{ fontSize: 11, color: colors.textMuted }}>Net worth</div>
                  <div style={{ fontSize: 15, fontWeight: 500, marginBottom: 6 }}>
                    {m.currency_symbol}
                    {fmtMoney(m.net_worth)}
                  </div>
                  <div style={{ display: "flex", gap: 14 }}>
                    <div>
                      <div style={{ fontSize: 10, color: colors.textMuted }}>Unrealized</div>
                      <div style={{ fontSize: 12, color: pnlColor(unrealized), fontWeight: 500 }}>
                        {m.currency_symbol}
                        {fmtMoney(m.unrealized_pnl)}
                      </div>
                    </div>
                    <div>
                      <div style={{ fontSize: 10, color: colors.textMuted }}>Realized</div>
                      <div style={{ fontSize: 12, color: pnlColor(realized), fontWeight: 500 }}>
                        {m.currency_symbol}
                        {fmtMoney(m.realized_pnl)}
                      </div>
                    </div>
                  </div>
                </div>
              );
            })}
          </div>
        </div>
      )}

      {availableAiProviders.length > 0 && (
        <div style={{ ...panelStyle, marginTop: 12 }}>
          <p style={{ fontSize: 12, color: colors.textMuted, margin: "0 0 4px", fontWeight: 600 }}>
            AI portfolio insights
          </p>
          <p style={{ fontSize: 11, color: colors.textMuted, margin: "0 0 10px" }}>
            AI-generated analysis for informational purposes only — not financial advice. Always
            verify anything here yourself before acting on it.
          </p>

          {availableAiProviders.length > 1 && (
            <div style={{ display: "flex", gap: 10, marginBottom: 10, fontSize: 12 }}>
              {availableAiProviders.map((p) => (
                <label key={p} style={{ cursor: "pointer", textTransform: "capitalize" }}>
                  <input
                    type="radio"
                    checked={selectedAiProvider === p}
                    onChange={() => setSelectedAiProvider(p)}
                    style={{ marginRight: 4 }}
                  />
                  {p}
                </label>
              ))}
            </div>
          )}

          {!aiConfirming && !aiLoading && !aiInsights && (
            <button onClick={() => setAiConfirming(true)}>Get AI Insights</button>
          )}

          {aiConfirming && (
            <div style={{ ...cardStyle, padding: 10 }}>
              <p style={{ fontSize: 12, margin: "0 0 8px" }}>
                This sends your holdings (symbol, sector, quantity, market value, unrealized P/L%),
                sector allocation percentages, and portfolio XIRR to{" "}
                <strong style={{ textTransform: "capitalize" }}>{selectedAiProvider}</strong>'s API.
                No account numbers, no other portfolios, nothing beyond what's listed here.
              </p>
              <div style={{ display: "flex", gap: 8 }}>
                <button onClick={handleGenerateInsights}>Send & Analyze</button>
                <button onClick={() => setAiConfirming(false)}>Cancel</button>
              </div>
            </div>
          )}

          {aiLoading && <p style={{ fontSize: 12, color: colors.textMuted }}>Analyzing…</p>}

          {aiError && <p style={{ fontSize: 12, color: colors.danger }}>{aiError}</p>}

          {aiInsights && (
            <div>
              <div style={{ ...cardStyle, padding: 12 }}>
                {parseAiInsightLines(aiInsights).map((line, i) => (
                  <div
                    key={i}
                    style={{
                      fontSize: 13,
                      lineHeight: 1.6,
                      padding: line.tag ? "4px 10px" : "2px 0",
                      marginBottom: 4,
                      borderLeft: line.tag ? `3px solid ${aiTagColor(line.tag)}` : undefined,
                      background: line.tag ? `${aiTagColor(line.tag)}14` : undefined,
                      borderRadius: line.tag ? "0 4px 4px 0" : undefined,
                    }}
                  >
                    {line.tag && (
                      <span style={{ fontSize: 9, fontWeight: 700, color: aiTagColor(line.tag), marginRight: 6 }}>
                        {line.tag}
                      </span>
                    )}
                    {line.text}
                  </div>
                ))}
              </div>
              <button
                onClick={() => {
                  setAiInsights(null);
                  setAiError(null);
                }}
                style={{ marginTop: 8, fontSize: 12 }}
              >
                Clear
              </button>
            </div>
          )}
        </div>
      )}

      {taxSummary && taxSummary.rows.length > 0 && (
        <div style={{ ...panelStyle, marginTop: 12 }}>
          <div
            onClick={() => setTaxSummaryExpanded((v) => !v)}
            style={{ display: "flex", justifyContent: "space-between", alignItems: "center", cursor: "pointer" }}
          >
            <p style={{ fontSize: 12, color: colors.textMuted, margin: 0, fontWeight: 600 }}>
              Tax summary (STCG/LTCG) — Short-term ₹{fmtMoney(taxSummary.total_short_term)}, Long-term ₹
              {fmtMoney(taxSummary.total_long_term)}
            </p>
            <span style={{ fontSize: 11, color: colors.accent }}>{taxSummaryExpanded ? "Hide" : "Show"} by symbol</span>
          </div>
          {taxSummaryExpanded && (
            <div style={{ marginTop: 10 }}>
              <p style={{ fontSize: 11, color: colors.textMuted, margin: "0 0 8px" }}>
                Computed via FIFO lot matching over your full transaction history — India's 12-month
                equity long-term threshold, applied per purchase lot, not to the position as a whole.
              </p>
              <table style={{ borderCollapse: "collapse", fontSize: 12, width: "100%" }}>
                <thead>
                  <tr style={{ textAlign: "left", borderBottom: `1px solid ${colors.border}` }}>
                    <th style={{ padding: "4px 8px 4px 0" }}>Symbol</th>
                    <th style={{ padding: "4px 8px" }}>Short-term gain</th>
                    <th style={{ padding: "4px 8px" }}>Long-term gain</th>
                  </tr>
                </thead>
                <tbody>
                  {taxSummary.rows.map((r) => (
                    <tr key={r.symbol}>
                      <td style={{ padding: "4px 8px 4px 0" }}>{r.symbol}</td>
                      <td style={{ padding: "4px 8px", color: pnlColor(parseFloat(r.short_term_gain)) }}>
                        ₹{fmtMoney(r.short_term_gain)}
                      </td>
                      <td style={{ padding: "4px 8px", color: pnlColor(parseFloat(r.long_term_gain)) }}>
                        ₹{fmtMoney(r.long_term_gain)}
                      </td>
                    </tr>
                  ))}
                </tbody>
              </table>
            </div>
          )}
        </div>
      )}

      {taxLossView && taxLossView.candidates.length > 0 && (
        <div style={{ ...panelStyle, marginTop: 12 }}>
          <div
            onClick={() => setTaxLossExpanded((v) => !v)}
            style={{ display: "flex", justifyContent: "space-between", alignItems: "center", cursor: "pointer" }}
          >
            <p style={{ fontSize: 12, color: colors.textMuted, margin: 0, fontWeight: 600 }}>
              Tax-loss harvesting — {taxLossView.candidates.length} lot{taxLossView.candidates.length === 1 ? "" : "s"} at
              a loss, ₹{fmtMoney(taxLossView.total_realized_gains_this_fy)} realized gains this FY to offset
            </p>
            <span style={{ fontSize: 11, color: colors.accent }}>{taxLossExpanded ? "Hide" : "Show"} candidates</span>
          </div>
          {taxLossExpanded && (
            <div style={{ marginTop: 10 }}>
              <p style={{ fontSize: 11, color: colors.textMuted, margin: "0 0 8px" }}>
                India has no wash-sale rule — a lot here can be sold to offset the gains above, then
                bought straight back if you still want the position. Not tax advice; confirm with
                whoever files your return.
              </p>
              <table style={{ borderCollapse: "collapse", fontSize: 12, width: "100%" }}>
                <thead>
                  <tr style={{ textAlign: "left", borderBottom: `1px solid ${colors.border}` }}>
                    <th style={{ padding: "4px 8px 4px 0" }}>Symbol</th>
                    <th style={{ padding: "4px 8px" }}>Qty</th>
                    <th style={{ padding: "4px 8px" }}>Cost</th>
                    <th style={{ padding: "4px 8px" }}>Current</th>
                    <th style={{ padding: "4px 8px" }}>Loss</th>
                    <th style={{ padding: "4px 8px" }}>Term</th>
                    <th style={{ padding: "4px 8px" }}>Bought</th>
                  </tr>
                </thead>
                <tbody>
                  {taxLossView.candidates.map((c, i) => (
                    <tr key={i}>
                      <td style={{ padding: "4px 8px 4px 0" }}>{c.symbol}</td>
                      <td style={{ padding: "4px 8px" }}>{c.quantity}</td>
                      <td style={{ padding: "4px 8px" }}>₹{fmtMoney(c.cost_price)}</td>
                      <td style={{ padding: "4px 8px" }}>₹{fmtMoney(c.current_price)}</td>
                      <td style={{ padding: "4px 8px", color: colors.danger }}>₹{fmtMoney(c.unrealized_loss)}</td>
                      <td style={{ padding: "4px 8px" }}>{c.is_long_term ? "Long" : "Short"}</td>
                      <td style={{ padding: "4px 8px" }}>{c.purchase_date}</td>
                    </tr>
                  ))}
                </tbody>
              </table>
            </div>
          )}
        </div>
      )}

      <div style={{ display: "grid", gridTemplateColumns: "1fr 1fr", gap: 12, marginTop: 12 }}>
        <div style={panelStyle}>
          <p style={{ fontSize: 12, color: colors.textMuted, margin: "0 0 6px", fontWeight: 600 }}>
            Alerts
          </p>
          {alertRules.length === 0 ? (
            <p style={{ fontSize: 12, color: colors.textMuted, margin: 0 }}>
              No stop-loss/target alerts set — add one from the Holdings screen.
            </p>
          ) : (
            <div style={{ display: "flex", flexDirection: "column", gap: 6 }}>
              {alertRules.map((a) => {
                // stop_loss uses amber (matches the "falling" convention
                // already established for the ±3.5% day-move flash),
                // target uses green (matches "rising"). Triggered gets the
                // full blink; nearing-but-not-triggered gets the gentler
                // pulse — see the keyframe doc comment in App.tsx.
                const animation = a.is_triggered_now
                  ? a.condition === "stop_loss"
                    ? "flash-amber"
                    : "flash-green"
                  : a.is_nearing
                  ? a.condition === "stop_loss"
                    ? "pulse-amber"
                    : "pulse-green"
                  : undefined;
                return (
                  <div
                    key={a.id}
                    style={{
                      display: "flex",
                      justifyContent: "space-between",
                      alignItems: "center",
                      fontSize: 12,
                      padding: "4px 8px",
                      borderRadius: 4,
                      animation: animation ? `${animation} 1.4s ease-in-out infinite` : undefined,
                    }}
                  >
                    <span style={{ fontWeight: a.is_triggered_now ? 700 : 400 }}>
                      {a.is_triggered_now ? "⚠ " : a.is_nearing ? "近 " : ""}
                      {a.symbol} {a.condition === "stop_loss" ? "≤" : "≥"} ₹{fmtMoney(a.threshold_price)}
                      {a.current_price != null && <span style={{ color: colors.textMuted }}> (now ₹{fmtMoney(a.current_price)})</span>}
                      {a.is_nearing && !a.is_triggered_now && (
                        <span style={{ color: colors.textMuted, fontStyle: "italic" }}> — nearing</span>
                      )}
                    </span>
                    <button onClick={() => handleDismissAlert(a.id)} style={{ fontSize: 11 }}>
                      Dismiss
                    </button>
                  </div>
                );
              })}
            </div>
          )}
        </div>
        <div style={panelStyle}>
          <p style={{ fontSize: 12, color: colors.textMuted, margin: "0 0 6px", fontWeight: 600 }}>
            Calendar
          </p>
          <p style={{ fontSize: 12, color: colors.textMuted, margin: 0 }}>
            Not built yet — no calendar_event table or use-case exists in this slice.
          </p>
        </div>
      </div>
    </div>
  );
}

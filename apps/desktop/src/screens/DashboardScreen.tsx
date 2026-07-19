import { useEffect, useState } from "react";
import { api } from "../lib/tauri";
import type { DashboardSummary, HoldingView } from "../lib/types";
import { cardStyle, colors, panelStyle } from "../lib/theme";

// A few distinct, low-saturation colors for the sector breakdown bars —
// enough for a handful of sectors; this is demo-scale data (2 instruments),
// not a real allocation engine.
const SECTOR_COLORS = ["#2E74B5", "#5B9BD5", "#9DC3E6", "#1F3864", "#7F9EC2"];

export function DashboardScreen({ portfolioId }: { portfolioId: string }) {
  const [summary, setSummary] = useState<DashboardSummary | null>(null);
  const [holdings, setHoldings] = useState<HoldingView[]>([]);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    Promise.all([api.getDashboardSummary(portfolioId), api.listHoldings(portfolioId)])
      .then(([s, h]) => {
        setSummary(s);
        setHoldings(h);
      })
      .catch((e) => setError(String(e)));
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
            <div style={{ fontSize: 18, fontWeight: 600 }}>₹{summary.net_worth}</div>
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
              ₹{summary.overall_unrealized_pnl}
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
              ₹{summary.overall_realized_pnl}
            </div>
          </div>
        </div>
        </>
      )}

      <div style={{ display: "grid", gridTemplateColumns: "1.3fr 1fr", gap: 12, marginTop: 8 }}>
        <div style={panelStyle}>
          <p style={{ fontSize: 12, color: colors.textMuted, margin: "0 0 10px", fontWeight: 600 }}>
            Sector allocation
          </p>
          {allocation.length === 0 ? (
            <p style={{ fontSize: 12, color: colors.textMuted }}>No priced holdings yet.</p>
          ) : (
            <div style={{ display: "flex", flexDirection: "column", gap: 8 }}>
              {allocation.map(([sector, value], i) => {
                const pct = totalMarketValue > 0 ? (value / totalMarketValue) * 100 : 0;
                return (
                  <div key={sector}>
                    <div style={{ display: "flex", justifyContent: "space-between", fontSize: 12, marginBottom: 3 }}>
                      <span>{sector}</span>
                      <span style={{ color: colors.textMuted }}>{pct.toFixed(1)}%</span>
                    </div>
                    <div style={{ background: "#E5E5E5", borderRadius: 4, height: 8, overflow: "hidden" }}>
                      <div
                        style={{
                          width: `${pct}%`,
                          height: "100%",
                          background: SECTOR_COLORS[i % SECTOR_COLORS.length],
                        }}
                      />
                    </div>
                  </div>
                );
              })}
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

      <div style={{ display: "grid", gridTemplateColumns: "1fr 1fr", gap: 12, marginTop: 12 }}>
        <div style={panelStyle}>
          <p style={{ fontSize: 12, color: colors.textMuted, margin: "0 0 6px", fontWeight: 600 }}>
            Notifications
          </p>
          <p style={{ fontSize: 12, color: colors.textMuted, margin: 0 }}>
            Not built yet — the Alert Engine (SRS 2.2.5) doesn't exist in this slice.
          </p>
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

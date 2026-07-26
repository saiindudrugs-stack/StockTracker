import { useEffect, useState } from "react";
import { NavBar } from "./components/NavBar";
import { PortfolioTabs } from "./components/PortfolioTabs";
import { DashboardScreen } from "./screens/DashboardScreen";
import { HoldingsScreen } from "./screens/HoldingsScreen";
import { MutualFundsScreen } from "./screens/MutualFundsScreen";
import { WatchlistScreen } from "./screens/WatchlistScreen";
import { AnalysisScreen } from "./screens/AnalysisScreen";
import { ChartScreen } from "./screens/ChartScreen";
import { SettingsScreen } from "./screens/SettingsScreen";
import { api } from "./lib/tauri";
import type { PortfolioView, ScreenId } from "./lib/types";
import { colors } from "./lib/theme";

export default function App() {
  const [screen, setScreen] = useState<ScreenId>("dashboard");
  const [portfolios, setPortfolios] = useState<PortfolioView[]>([]);
  const [activePortfolioId, setActivePortfolioId] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  async function refreshPortfolios(selectId?: string) {
    try {
      const list = await api.listPortfolios();
      setPortfolios(list);
      if (selectId) {
        setActivePortfolioId(selectId);
      } else if (!activePortfolioId && list.length > 0) {
        setActivePortfolioId(list[0].id);
      }
      setError(null);
    } catch (e) {
      setError(String(e));
    }
  }

  useEffect(() => {
    refreshPortfolios();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  async function handleCreatePortfolio(name: string) {
    try {
      const created = await api.createPortfolio(name);
      await refreshPortfolios(created.id);
    } catch (e) {
      setError(String(e));
    }
  }

  async function handleDeletePortfolio(id: string) {
    try {
      await api.deletePortfolio(id);
      if (activePortfolioId === id) {
        setActivePortfolioId(null); // force refreshPortfolios to pick a new default below
      }
      const list = await api.listPortfolios();
      setPortfolios(list);
      if (activePortfolioId === id) {
        setActivePortfolioId(list.length > 0 ? list[0].id : null);
      }
      setError(null);
    } catch (e) {
      setError(String(e));
    }
  }

  // Watchlist/Chart/Settings are deliberately portfolio-agnostic — tracking
  // a ticker before buying shouldn't require setting up a family portfolio
  // first. Dashboard, Holdings, and Analysis need an active portfolio_id
  // since all three are about *owned* positions.
  const needsPortfolio = screen === "dashboard" || screen === "holdings" || screen === "analysis" || screen === "mutual-funds";

  return (
    <div style={{ fontFamily: "system-ui, sans-serif", display: "flex", flexDirection: "column", height: "100vh" }}>
      {/* Global keyframes for alert animations — a plain <style> tag since
          this app has no CSS module/stylesheet setup, everything else is
          inline styles.
          flash-amber/flash-green: the ±3.5% day-move alert (Holdings and
          Watchlist rows) AND a triggered stop-loss/target alert — full-
          intensity blink, something needs your attention now.
          pulse-amber/pulse-green: a stop-loss/target that's within 2% but
          hasn't fired yet — deliberately gentler (smaller opacity swing),
          a heads-up rather than an alarm. */}
      <style>{`
        @keyframes flash-amber {
          0%, 100% { background-color: rgba(230, 162, 60, 0.15); }
          50% { background-color: rgba(230, 162, 60, 0.55); }
        }
        @keyframes flash-green {
          0%, 100% { background-color: rgba(30, 122, 52, 0.12); }
          50% { background-color: rgba(30, 122, 52, 0.45); }
        }
        @keyframes pulse-amber {
          0%, 100% { background-color: rgba(230, 162, 60, 0.08); }
          50% { background-color: rgba(230, 162, 60, 0.22); }
        }
        @keyframes pulse-green {
          0%, 100% { background-color: rgba(30, 122, 52, 0.06); }
          50% { background-color: rgba(30, 122, 52, 0.18); }
        }
        /* Subtle vertical dividers between columns, for tables that opt in
           via className="data-table" — a shared rule rather than adding a
           borderRight to every single <td>/<th> across every table. */
        .data-table td, .data-table th {
          border-right: 1px solid #EAEAEA;
        }
        .data-table td:last-child, .data-table th:last-child {
          border-right: none;
        }
      `}</style>
      <PortfolioTabs
        portfolios={portfolios}
        activeId={activePortfolioId}
        onSelect={setActivePortfolioId}
        onCreate={handleCreatePortfolio}
      />
      <NavBar active={screen} onSelect={setScreen} />
      <div style={{ flex: 1, overflow: "auto" }}>
        {error && <p style={{ color: colors.danger, padding: "8px 24px 0" }}>{error}</p>}
        {needsPortfolio && !activePortfolioId ? (
          <p style={{ padding: 24, color: colors.textMuted, fontSize: 13 }}>
            No portfolio selected yet — click "+ Add portfolio" above to create one.
          </p>
        ) : (
          <>
            {screen === "dashboard" && activePortfolioId && <DashboardScreen portfolioId={activePortfolioId} />}
            {screen === "holdings" && activePortfolioId && <HoldingsScreen portfolioId={activePortfolioId} />}
            {screen === "mutual-funds" && activePortfolioId && <MutualFundsScreen portfolioId={activePortfolioId} />}
            {screen === "watchlist" && <WatchlistScreen />}
            {screen === "analysis" && activePortfolioId && <AnalysisScreen portfolioId={activePortfolioId} />}
            {screen === "chart" && <ChartScreen />}
            {screen === "settings" && <SettingsScreen portfolios={portfolios} onDeletePortfolio={handleDeletePortfolio} />}
          </>
        )}
      </div>
    </div>
  );
}

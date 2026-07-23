import { useEffect, useState } from "react";
import { NavRail } from "./components/NavRail";
import { PortfolioTabs } from "./components/PortfolioTabs";
import { DashboardScreen } from "./screens/DashboardScreen";
import { HoldingsScreen } from "./screens/HoldingsScreen";
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

  // Watchlist/Chart/Settings are deliberately portfolio-agnostic — tracking
  // a ticker before buying shouldn't require setting up a family portfolio
  // first. Dashboard, Holdings, and Analysis need an active portfolio_id
  // since all three are about *owned* positions.
  const needsPortfolio = screen === "dashboard" || screen === "holdings" || screen === "analysis";

  return (
    <div style={{ fontFamily: "system-ui, sans-serif", display: "flex", height: "100vh" }}>
      {/* Global keyframes for the ±3.5% day-move flash alert (Holdings and
          Watchlist rows) — a plain <style> tag since this app has no CSS
          module/stylesheet setup, everything else is inline styles. Amber
          for a stock falling beyond the threshold, green for rising beyond
          it, per the user's own color choice. */}
      <style>{`
        @keyframes flash-amber {
          0%, 100% { background-color: rgba(230, 162, 60, 0.15); }
          50% { background-color: rgba(230, 162, 60, 0.55); }
        }
        @keyframes flash-green {
          0%, 100% { background-color: rgba(30, 122, 52, 0.12); }
          50% { background-color: rgba(30, 122, 52, 0.45); }
        }
      `}</style>
      <NavRail active={screen} onSelect={setScreen} />
      <div style={{ flex: 1, display: "flex", flexDirection: "column", overflow: "hidden" }}>
        <PortfolioTabs
          portfolios={portfolios}
          activeId={activePortfolioId}
          onSelect={setActivePortfolioId}
          onCreate={handleCreatePortfolio}
        />
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
              {screen === "watchlist" && <WatchlistScreen />}
              {screen === "analysis" && activePortfolioId && <AnalysisScreen portfolioId={activePortfolioId} />}
              {screen === "chart" && <ChartScreen />}
              {screen === "settings" && <SettingsScreen />}
            </>
          )}
        </div>
      </div>
    </div>
  );
}

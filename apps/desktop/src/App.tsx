import { useEffect, useState } from "react";
import { NavRail } from "./components/NavRail";
import { PortfolioTabs } from "./components/PortfolioTabs";
import { DashboardScreen } from "./screens/DashboardScreen";
import { HoldingsScreen } from "./screens/HoldingsScreen";
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

  return (
    <div style={{ fontFamily: "system-ui, sans-serif", display: "flex", height: "100vh" }}>
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
          {!activePortfolioId ? (
            <p style={{ padding: 24, color: colors.textMuted, fontSize: 13 }}>
              No portfolio selected yet — click "+ Add portfolio" above to create one.
            </p>
          ) : (
            <>
              {screen === "dashboard" && <DashboardScreen portfolioId={activePortfolioId} />}
              {screen === "holdings" && <HoldingsScreen portfolioId={activePortfolioId} />}
              {screen === "chart" && <ChartScreen />}
              {screen === "settings" && <SettingsScreen />}
            </>
          )}
        </div>
      </div>
    </div>
  );
}

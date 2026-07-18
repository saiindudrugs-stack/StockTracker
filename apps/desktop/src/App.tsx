import { useState } from "react";
import { NavRail } from "./components/NavRail";
import { DashboardScreen } from "./screens/DashboardScreen";
import { HoldingsScreen } from "./screens/HoldingsScreen";
import { ChartScreen } from "./screens/ChartScreen";
import { SettingsScreen } from "./screens/SettingsScreen";
import type { ScreenId } from "./lib/types";

export default function App() {
  const [screen, setScreen] = useState<ScreenId>("dashboard");

  return (
    <div style={{ fontFamily: "system-ui, sans-serif", display: "flex", height: "100vh" }}>
      <NavRail active={screen} onSelect={setScreen} />
      <div style={{ flex: 1, overflow: "auto" }}>
        {screen === "dashboard" && <DashboardScreen />}
        {screen === "holdings" && <HoldingsScreen />}
        {screen === "chart" && <ChartScreen />}
        {screen === "settings" && <SettingsScreen />}
      </div>
    </div>
  );
}

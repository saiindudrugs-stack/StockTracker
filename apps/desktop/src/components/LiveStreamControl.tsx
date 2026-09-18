import { useState } from "react";
import { api } from "../lib/tauri";
import { colors } from "../lib/theme";

/// Only one live stream runs app-wide at a time (backed by a single
/// background task in the Rust layer) — starting one here stops whatever
/// was running from another screen, same as switching brokers within one
/// screen already does. That's a deliberate, existing constraint, not a
/// bug introduced by reusing this control across screens.
export function LiveStreamControl({ symbols, label }: { symbols: string[]; label?: string }) {
  const [liveStreaming, setLiveStreaming] = useState(false);
  const [broker, setBroker] = useState<"upstox" | "zerodha">("upstox");
  const [msg, setMsg] = useState<string | null>(null);

  async function handleToggle() {
    if (liveStreaming) {
      await api.stopLivePriceStream();
      setLiveStreaming(false);
      setMsg(null);
      return;
    }
    try {
      const resolvedCount = broker === "upstox" ? await api.startLivePriceStream(symbols) : await api.startZerodhaLiveStream(symbols);
      setLiveStreaming(true);
      setMsg(`Streaming ${resolvedCount} of ${symbols.length} symbols live (${broker === "upstox" ? "Upstox" : "Zerodha"}).`);
    } catch (e) {
      setMsg(String(e));
    }
  }

  return (
    <div style={{ display: "flex", alignItems: "center", gap: 8, flexWrap: "wrap" }}>
      {label && <span style={{ fontSize: 12 }}>{label}</span>}
      {!liveStreaming && (
        <select value={broker} onChange={(e) => setBroker(e.target.value as "upstox" | "zerodha")} style={{ fontSize: 12 }}>
          <option value="upstox">Upstox</option>
          <option value="zerodha">Zerodha</option>
        </select>
      )}
      <button
        onClick={handleToggle}
        style={{ fontSize: 12, color: liveStreaming ? colors.success : undefined, fontWeight: liveStreaming ? 600 : 400 }}
      >
        {liveStreaming ? "● Live — Stop" : "Start Live Streaming"}
      </button>
      {msg && <span style={{ fontSize: 11, color: colors.textMuted }}>{msg}</span>}
    </div>
  );
}

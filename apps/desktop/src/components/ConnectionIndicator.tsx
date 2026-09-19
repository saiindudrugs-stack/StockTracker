import { useState } from "react";
import { colors } from "../lib/theme";

type Status = "untested" | "testing" | "connected" | "failed";

/// A saved key proves nothing about whether it's valid or the network
/// path works — this only ever turns green after a real test call
/// actually succeeds (see the Rust doc comment on test_market_data_
/// connection / test_ai_provider_connection for why). Starts gray on
/// every app launch; status isn't persisted, since a connection that
/// worked yesterday isn't a claim about right now.
export function ConnectionIndicator({ onTest }: { onTest: () => Promise<string> }) {
  const [status, setStatus] = useState<Status>("untested");
  const [detail, setDetail] = useState<string | null>(null);

  async function handleTest() {
    setStatus("testing");
    setDetail(null);
    try {
      const result = await onTest();
      setStatus("connected");
      setDetail(result);
    } catch (e) {
      setStatus("failed");
      setDetail(String(e));
    }
  }

  const dotColor = status === "connected" ? colors.success : status === "failed" ? colors.danger : status === "testing" ? "#E6A23C" : "#C4C9D0";

  return (
    <span style={{ display: "inline-flex", alignItems: "center", gap: 6 }}>
      <span
        title={detail ?? (status === "untested" ? "Not tested yet" : undefined)}
        style={{
          display: "inline-block",
          width: 8,
          height: 8,
          borderRadius: "50%",
          background: dotColor,
          flexShrink: 0,
        }}
      />
      <button onClick={handleTest} disabled={status === "testing"} style={{ fontSize: 11, padding: "2px 8px" }}>
        {status === "testing" ? "Testing…" : status === "connected" ? "Retest" : "Test connection"}
      </button>
      {detail && (
        <span style={{ fontSize: 11, color: status === "connected" ? colors.success : colors.danger }}>{detail}</span>
      )}
    </span>
  );
}

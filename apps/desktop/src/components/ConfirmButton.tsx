import { useState } from "react";
import type { CSSProperties } from "react";
import { colors } from "../lib/theme";

/// Deliberately NOT using window.confirm()/window.alert() anywhere in this
/// app: Tauri's webview (WKWebView on macOS) doesn't reliably implement
/// the browser's native synchronous dialog the way a real browser tab
/// does — it can return immediately without ever showing anything, which
/// silently swallows the "are you sure?" step entirely. This component is
/// a plain two-click confirmation built from ordinary React state, so it
/// works identically regardless of the host webview's dialog support.
export function ConfirmButton({
  label,
  confirmLabel,
  onConfirm,
  style,
}: {
  label: string;
  confirmLabel?: string;
  onConfirm: () => void;
  style?: CSSProperties;
}) {
  const [confirming, setConfirming] = useState(false);

  if (confirming) {
    return (
      <span style={{ display: "inline-flex", gap: 4 }}>
        <button
          onClick={(e) => {
            e.stopPropagation();
            setConfirming(false);
            onConfirm();
          }}
          style={{
            fontSize: 11,
            color: "white",
            background: colors.danger,
            border: "none",
            borderRadius: 4,
            padding: "2px 8px",
            cursor: "pointer",
          }}
        >
          {confirmLabel ?? "Confirm"}
        </button>
        <button
          onClick={(e) => {
            e.stopPropagation();
            setConfirming(false);
          }}
          style={{ fontSize: 11, cursor: "pointer" }}
        >
          Cancel
        </button>
      </span>
    );
  }

  return (
    <button
      onClick={(e) => {
        e.stopPropagation();
        setConfirming(true);
      }}
      style={{ fontSize: 11, color: colors.danger, cursor: "pointer", ...style }}
    >
      {label}
    </button>
  );
}

import { useState } from "react";
import { colors } from "../lib/theme";

export function AlertSetter({
  onSave,
}: {
  onSave: (condition: "stop_loss" | "target", thresholdPrice: string) => Promise<void>;
}) {
  const [open, setOpen] = useState(false);
  const [condition, setCondition] = useState<"stop_loss" | "target">("stop_loss");
  const [price, setPrice] = useState("");
  const [saving, setSaving] = useState(false);

  if (!open) {
    return (
      <button onClick={() => setOpen(true)} style={{ fontSize: 11 }}>
        Set Alert
      </button>
    );
  }

  async function handleSave() {
    if (!price.trim()) return;
    setSaving(true);
    try {
      await onSave(condition, price);
      setOpen(false);
      setPrice("");
    } finally {
      setSaving(false);
    }
  }

  return (
    <span style={{ display: "inline-flex", gap: 4, alignItems: "center" }}>
      <select value={condition} onChange={(e) => setCondition(e.target.value as "stop_loss" | "target")} style={{ fontSize: 11 }}>
        <option value="stop_loss">Stop-loss ≤</option>
        <option value="target">Target ≥</option>
      </select>
      <input
        value={price}
        onChange={(e) => setPrice(e.target.value)}
        placeholder="Price"
        style={{ width: 70, fontSize: 11 }}
        onKeyDown={(e) => e.key === "Enter" && handleSave()}
      />
      <button onClick={handleSave} disabled={saving} style={{ fontSize: 11, color: colors.success }}>
        Save
      </button>
      <button onClick={() => setOpen(false)} style={{ fontSize: 11 }}>
        Cancel
      </button>
    </span>
  );
}

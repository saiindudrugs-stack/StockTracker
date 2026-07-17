import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";

interface DashboardSummary {
  net_worth: string;
  overall_unrealized_pnl: string;
  overall_realized_pnl: string;
  holdings_priced: number;
  holdings_missing_price: number;
}

interface HoldingView {
  symbol: string;
  quantity: string;
  avg_cost: string;
  last_price: string | null;
  market_value: string | null;
  unrealized_pnl: string | null;
}

const cardStyle: React.CSSProperties = {
  background: "#F2F2F2",
  borderRadius: 8,
  padding: "12px 16px",
  minWidth: 160,
};

export default function App() {
  const [summary, setSummary] = useState<DashboardSummary | null>(null);
  const [holdings, setHoldings] = useState<HoldingView[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [symbol, setSymbol] = useState("RELIANCE");
  const [qty, setQty] = useState("5");
  const [price, setPrice] = useState("2500");
  const [xirrResult, setXirrResult] = useState<string | null>(null);

  async function refresh() {
    try {
      const [s, h] = await Promise.all([
        invoke<DashboardSummary>("get_dashboard_summary"),
        invoke<HoldingView[]>("list_holdings"),
      ]);
      setSummary(s);
      setHoldings(h);
      setError(null);
    } catch (e) {
      setError(String(e));
    }
  }

  useEffect(() => {
    refresh();
  }, []);

  async function handleBuy() {
    try {
      await invoke("record_buy", { symbol, quantity: qty, price });
      await refresh();
    } catch (e) {
      setError(String(e));
    }
  }

  async function handleXirr(sym: string) {
    try {
      const rate = await invoke<number>("compute_xirr_for_symbol", { symbol: sym });
      setXirrResult(`${sym}: ${(rate * 100).toFixed(2)}% XIRR`);
    } catch (e) {
      setXirrResult(`${sym}: ${String(e)}`);
    }
  }

  return (
    <div style={{ fontFamily: "system-ui, sans-serif", padding: 20, color: "#1F3864" }}>
      <h1 style={{ fontSize: 20 }}>Portfolio Manager — Volume II, Slice 1 demo</h1>
      <p style={{ color: "#666", fontSize: 13 }}>
        Real engine underneath (SQLite ledger, XIRR solver, holding derivation) — this is the
        thinnest possible UI to prove it's wired end to end, not the full dashboard from the
        wireframes.
      </p>

      {error && <p style={{ color: "crimson" }}>{error}</p>}

      {summary && (
        <div style={{ display: "flex", gap: 12, margin: "16px 0" }}>
          <div style={cardStyle}>
            <div style={{ fontSize: 12, color: "#666" }}>Net worth</div>
            <div style={{ fontSize: 18, fontWeight: 600 }}>₹{summary.net_worth}</div>
          </div>
          <div style={cardStyle}>
            <div style={{ fontSize: 12, color: "#666" }}>Unrealized P/L</div>
            <div style={{ fontSize: 18, fontWeight: 600 }}>₹{summary.overall_unrealized_pnl}</div>
          </div>
          <div style={cardStyle}>
            <div style={{ fontSize: 12, color: "#666" }}>Realized P/L</div>
            <div style={{ fontSize: 18, fontWeight: 600 }}>₹{summary.overall_realized_pnl}</div>
          </div>
        </div>
      )}

      <h2 style={{ fontSize: 15, marginTop: 24 }}>Holdings</h2>
      <table style={{ borderCollapse: "collapse", width: "100%", fontSize: 13 }}>
        <thead>
          <tr style={{ textAlign: "left", borderBottom: "1px solid #ccc" }}>
            <th>Symbol</th><th>Qty</th><th>Avg cost</th><th>LTP</th><th>Mkt value</th><th>Unreal. P/L</th><th></th>
          </tr>
        </thead>
        <tbody>
          {holdings.map((h) => (
            <tr key={h.symbol} style={{ borderBottom: "1px solid #eee" }}>
              <td>{h.symbol}</td>
              <td>{h.quantity}</td>
              <td>{h.avg_cost}</td>
              <td>{h.last_price ?? "—"}</td>
              <td>{h.market_value ?? "—"}</td>
              <td>{h.unrealized_pnl ?? "—"}</td>
              <td><button onClick={() => handleXirr(h.symbol)}>XIRR</button></td>
            </tr>
          ))}
        </tbody>
      </table>
      {xirrResult && <p style={{ fontSize: 13 }}>{xirrResult}</p>}

      <h2 style={{ fontSize: 15, marginTop: 24 }}>Record a Buy (demo instruments only)</h2>
      <div style={{ display: "flex", gap: 8, alignItems: "center" }}>
        <select value={symbol} onChange={(e) => setSymbol(e.target.value)}>
          <option value="RELIANCE">RELIANCE</option>
          <option value="TCS">TCS</option>
        </select>
        <input value={qty} onChange={(e) => setQty(e.target.value)} placeholder="Quantity" style={{ width: 80 }} />
        <input value={price} onChange={(e) => setPrice(e.target.value)} placeholder="Price" style={{ width: 100 }} />
        <button onClick={handleBuy}>Record Buy</button>
      </div>
    </div>
  );
}

import { useState } from "react";
import { api } from "../lib/tauri";
import type { PortfolioAnalysisView } from "../lib/types";
import { colors, panelStyle } from "../lib/theme";

function correlationColor(c: number): string {
  // Green for positive correlation (move together), red for negative
  // (move oppositely) — intensity scaled by magnitude. A diversified
  // portfolio generally wants LESS green here, not more; strong positive
  // correlation across everything you hold means less real diversification
  // than the holdings count alone suggests.
  const intensity = Math.min(Math.abs(c), 1);
  if (c > 0) {
    const g = Math.round(200 - intensity * 80);
    return `rgb(${255 - intensity * 120}, ${g + 40}, ${255 - intensity * 160})`;
  }
  const r = Math.round(200 + intensity * 55);
  return `rgb(${r}, ${255 - intensity * 140}, ${255 - intensity * 140})`;
}

export function AnalysisScreen({ portfolioId }: { portfolioId: string }) {
  const [analysis, setAnalysis] = useState<PortfolioAnalysisView | null>(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);

  async function runAnalysis() {
    setLoading(true);
    setError(null);
    try {
      const result = await api.getPortfolioAnalysis(portfolioId);
      setAnalysis(result);
    } catch (e) {
      setError(String(e));
    } finally {
      setLoading(false);
    }
  }

  const symbols = analysis ? Array.from(new Set(analysis.stocks.map((s) => s.symbol))) : [];

  function correlationFor(a: string, b: string): number | null {
    if (!analysis) return null;
    if (a === b) return 1;
    const pair = analysis.correlations.find(
      (c) => (c.symbol_a === a && c.symbol_b === b) || (c.symbol_a === b && c.symbol_b === a)
    );
    return pair ? pair.correlation : null;
  }

  return (
    <div style={{ padding: 24 }}>
      <h1 style={{ fontSize: 20, color: colors.navy, marginBottom: 4 }}>Analysis</h1>
      <p style={{ fontSize: 13, color: colors.textMuted, marginTop: 0 }}>
        Risk/return comparison and a return-correlation matrix across this portfolio's holdings —
        modeled directly on the standard "compare risk vs. return, then check correlation" approach
        from stock-analysis references (e.g. mean/volatility of daily returns, Pearson correlation
        between stocks). This fetches a year of history per held stock, so it's a deliberate action,
        not something that runs automatically.
      </p>

      <button onClick={runAnalysis} disabled={loading} style={{ marginBottom: 16 }}>
        {loading ? "Analyzing portfolio…" : "Run Analysis"}
      </button>

      {error && <p style={{ color: colors.danger }}>{error}</p>}

      {analysis && (
        <>
          {analysis.skipped.length > 0 && (
            <p style={{ fontSize: 12, color: colors.textMuted, marginBottom: 16 }}>
              Skipped (couldn't fetch history): {analysis.skipped.map((s) => `${s.symbol} (${s.reason})`).join("; ")}
            </p>
          )}

          <div style={panelStyle}>
            <p style={{ fontSize: 12, fontWeight: 600, margin: "0 0 10px" }}>Risk vs. Return</p>
            {analysis.stocks.length === 0 ? (
              <p style={{ fontSize: 12, color: colors.textMuted }}>No stocks with enough history yet.</p>
            ) : (
              <table style={{ borderCollapse: "collapse", width: "100%", fontSize: 13 }}>
                <thead>
                  <tr style={{ textAlign: "left", borderBottom: `1px solid ${colors.border}` }}>
                    <th style={{ padding: "4px 8px 4px 0" }}>Symbol</th>
                    <th>Ann. Return</th>
                    <th>Ann. Volatility</th>
                    <th>Read</th>
                  </tr>
                </thead>
                <tbody>
                  {analysis.stocks.map((s) => (
                    <tr key={s.symbol} style={{ borderBottom: "1px solid #eee" }}>
                      <td style={{ padding: "4px 8px 4px 0" }}>{s.symbol}</td>
                      <td style={{ color: s.annualized_return_pct >= 0 ? colors.success : colors.danger }}>
                        {s.annualized_return_pct.toFixed(1)}%
                      </td>
                      <td>{s.annualized_volatility_pct.toFixed(1)}%</td>
                      <td style={{ color: colors.textMuted }}>{s.risk_label}</td>
                    </tr>
                  ))}
                </tbody>
              </table>
            )}
            <p style={{ fontSize: 11, color: colors.textMuted, marginTop: 8 }}>
              "High/Low" here is relative to the median across this portfolio's own holdings, not a
              universal threshold — the read only means something in the context of what else you hold.
            </p>
          </div>

          <div style={{ ...panelStyle, marginTop: 16 }}>
            <p style={{ fontSize: 12, fontWeight: 600, margin: "0 0 10px" }}>
              Return Correlation Matrix
            </p>
            {symbols.length < 2 ? (
              <p style={{ fontSize: 12, color: colors.textMuted }}>
                Need at least 2 stocks with history to compute correlation.
              </p>
            ) : (
              <table style={{ borderCollapse: "collapse", fontSize: 12 }}>
                <thead>
                  <tr>
                    <th style={{ padding: 4 }}></th>
                    {symbols.map((s) => (
                      <th key={s} style={{ padding: 4, textAlign: "center" }}>
                        {s}
                      </th>
                    ))}
                  </tr>
                </thead>
                <tbody>
                  {symbols.map((rowSym) => (
                    <tr key={rowSym}>
                      <th style={{ padding: 4, textAlign: "left" }}>{rowSym}</th>
                      {symbols.map((colSym) => {
                        const c = correlationFor(rowSym, colSym);
                        return (
                          <td
                            key={colSym}
                            style={{
                              padding: "6px 10px",
                              textAlign: "center",
                              background: c != null ? correlationColor(c) : "#f5f5f5",
                            }}
                          >
                            {c != null ? c.toFixed(2) : "—"}
                          </td>
                        );
                      })}
                    </tr>
                  ))}
                </tbody>
              </table>
            )}
            <p style={{ fontSize: 11, color: colors.textMuted, marginTop: 8 }}>
              Close to +1: these stocks tend to move together (less real diversification benefit
              than holding two "different" stocks might suggest). Close to -1: they tend to move
              oppositely. Near 0: little relationship either way.
            </p>
          </div>
        </>
      )}
    </div>
  );
}

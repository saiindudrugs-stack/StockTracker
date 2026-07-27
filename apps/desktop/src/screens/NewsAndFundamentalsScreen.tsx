import { useEffect, useState } from "react";
import { api } from "../lib/tauri";
import type { FundamentalsView, InstrumentView, NewsItemView } from "../lib/types";
import { colors, panelStyle, fmtMoney } from "../lib/theme";

export function NewsAndFundamentalsScreen() {
  const [instruments, setInstruments] = useState<InstrumentView[]>([]);
  const [selected, setSelected] = useState<string | null>(null);
  const [fundamentals, setFundamentals] = useState<FundamentalsView | null>(null);
  const [news, setNews] = useState<NewsItemView[]>([]);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    api
      .listEquityInstruments()
      .then((list) => {
        setInstruments(list);
        if (list.length > 0 && !selected) setSelected(list[0].symbol);
      })
      .catch((e) => setError(String(e)));
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  useEffect(() => {
    if (!selected) return;
    setLoading(true);
    setError(null);
    setFundamentals(null);
    setNews([]);
    Promise.all([api.getFundamentals(selected), api.getStockNews(selected)])
      .then(([f, n]) => {
        setFundamentals(f);
        setNews(n);
      })
      .catch((e) => setError(String(e)))
      .finally(() => setLoading(false));
  }, [selected]);

  return (
    <div style={{ display: "flex", height: "100%" }}>
      <div style={{ width: 180, borderRight: `1px solid ${colors.border}`, overflowY: "auto", flexShrink: 0 }}>
        <p style={{ fontSize: 10, color: colors.textMuted, padding: "10px 10px 4px" }}>Tracked equities</p>
        {instruments.map((inst) => (
          <div
            key={inst.symbol}
            onClick={() => setSelected(inst.symbol)}
            style={{
              padding: "8px 10px",
              fontSize: 12,
              cursor: "pointer",
              background: selected === inst.symbol ? "#E6F1FB" : "transparent",
              color: selected === inst.symbol ? colors.accent : colors.textMuted,
              fontWeight: selected === inst.symbol ? 600 : 400,
              borderLeft: selected === inst.symbol ? `3px solid ${colors.accent}` : "3px solid transparent",
            }}
          >
            {inst.symbol}
          </div>
        ))}
        {instruments.length === 0 && (
          <p style={{ fontSize: 11, color: colors.textMuted, padding: "0 10px" }}>
            No equities tracked yet — add one from Watchlist or Holdings.
          </p>
        )}
      </div>

      <div style={{ flex: 1, padding: "16px 20px", overflowY: "auto" }}>
        {error && <p style={{ color: colors.danger }}>{error}</p>}
        {loading && <p style={{ fontSize: 12, color: colors.textMuted }}>Loading…</p>}

        {!loading && fundamentals && selected && (
          <>
            <p style={{ fontSize: 16, fontWeight: 500, margin: "0 0 2px" }}>{selected}</p>
            <p style={{ fontSize: 11, color: colors.textMuted, margin: "0 0 12px" }}>
              {[fundamentals.sector, fundamentals.industry].filter(Boolean).join(" · ") || "Sector/industry unavailable"}
            </p>

            <div style={{ display: "grid", gridTemplateColumns: "repeat(4, 1fr)", gap: 8, marginBottom: 16 }}>
              <div style={{ ...panelStyle, padding: 10 }}>
                <div style={{ fontSize: 10, color: colors.textMuted }}>Market cap</div>
                <div style={{ fontSize: 13, fontWeight: 500 }}>{fmtMoney(fundamentals.market_cap)}</div>
              </div>
              <div style={{ ...panelStyle, padding: 10 }}>
                <div style={{ fontSize: 10, color: colors.textMuted }}>P/E</div>
                <div style={{ fontSize: 13, fontWeight: 500 }}>{fundamentals.pe_ratio ? parseFloat(fundamentals.pe_ratio).toFixed(2) : "—"}</div>
              </div>
              <div style={{ ...panelStyle, padding: 10 }}>
                <div style={{ fontSize: 10, color: colors.textMuted }}>52W range</div>
                <div style={{ fontSize: 13, fontWeight: 500 }}>
                  {fundamentals.week52_low ? parseFloat(fundamentals.week52_low).toFixed(0) : "—"}–
                  {fundamentals.week52_high ? parseFloat(fundamentals.week52_high).toFixed(0) : "—"}
                </div>
              </div>
              <div style={{ ...panelStyle, padding: 10 }}>
                <div style={{ fontSize: 10, color: colors.textMuted }}>Div yield</div>
                <div style={{ fontSize: 13, fontWeight: 500 }}>
                  {fundamentals.dividend_yield ? `${(parseFloat(fundamentals.dividend_yield) * 100).toFixed(2)}%` : "—"}
                </div>
              </div>
            </div>

            {fundamentals.description && (
              <p style={{ fontSize: 12, color: colors.textMuted, marginBottom: 16, lineHeight: 1.5 }}>
                {fundamentals.description.length > 400 ? `${fundamentals.description.slice(0, 400)}…` : fundamentals.description}
              </p>
            )}

            <p style={{ fontSize: 13, fontWeight: 600, margin: "0 0 8px" }}>Revenue by period</p>
            {fundamentals.revenue_by_period.length === 0 ? (
              <p style={{ fontSize: 12, color: colors.textMuted, marginBottom: 16 }}>Not available for this symbol.</p>
            ) : (
              <table style={{ borderCollapse: "collapse", fontSize: 12, marginBottom: 20 }}>
                <thead>
                  <tr style={{ textAlign: "left", borderBottom: `1px solid ${colors.border}` }}>
                    <th style={{ padding: "4px 12px 4px 0" }}>Period end</th>
                    <th style={{ padding: "4px 12px" }}>Revenue</th>
                    <th style={{ padding: "4px 12px" }}>Net income</th>
                  </tr>
                </thead>
                <tbody>
                  {fundamentals.revenue_by_period.map((p) => (
                    <tr key={p.period_end}>
                      <td style={{ padding: "4px 12px 4px 0" }}>{p.period_end || "—"}</td>
                      <td style={{ padding: "4px 12px" }}>{fmtMoney(p.revenue)}</td>
                      <td style={{ padding: "4px 12px" }}>{p.net_income ? fmtMoney(p.net_income) : "—"}</td>
                    </tr>
                  ))}
                </tbody>
              </table>
            )}

            <p style={{ fontSize: 13, fontWeight: 600, margin: "0 0 8px" }}>News and highlights (top 5)</p>
            <p style={{ fontSize: 11, color: colors.textMuted, margin: "0 0 8px" }}>
              "Regulatory" is a plain keyword match over headlines (board meetings, disclosures, SEBI/SEC
              filings, dividends) — not a verified separate filings feed. Treat it as a helpful sort, not a
              guarantee every real filing is caught.
            </p>
            {news.length === 0 ? (
              <p style={{ fontSize: 12, color: colors.textMuted }}>No news found for this symbol.</p>
            ) : (
              <div style={{ display: "flex", flexDirection: "column", gap: 6 }}>
                {news.map((n, i) => (
                  <a
                    key={i}
                    href={n.link}
                    target="_blank"
                    rel="noreferrer"
                    style={{
                      display: "block",
                      padding: "6px 10px",
                      borderLeft: `3px solid ${n.is_regulatory ? "#854F0B" : colors.border}`,
                      background: n.is_regulatory ? "#FAEEDA" : "transparent",
                      borderRadius: "0 4px 4px 0",
                      textDecoration: "none",
                      color: "inherit",
                    }}
                  >
                    {n.is_regulatory && (
                      <div style={{ fontSize: 9, color: "#633806", fontWeight: 600 }}>REGULATORY</div>
                    )}
                    <div style={{ fontSize: 12 }}>{n.title}</div>
                    <div style={{ fontSize: 10, color: colors.textMuted }}>
                      {n.publisher} · {n.published_at}
                    </div>
                  </a>
                ))}
              </div>
            )}
          </>
        )}
      </div>
    </div>
  );
}

import { useEffect, useState } from "react";
import { api } from "../lib/tauri";
import type { FundamentalsView, InstrumentView, NewsItemView } from "../lib/types";
import { colors, panelStyle, fmtMoney } from "../lib/theme";

export function NewsAndFundamentalsScreen() {
  const [instruments, setInstruments] = useState<InstrumentView[]>([]);
  const [selected, setSelected] = useState<string | null>(null);
  const [newsLimit, setNewsLimit] = useState(5);
  const [fundamentals, setFundamentals] = useState<FundamentalsView | null>(null);
  const [fundamentalsError, setFundamentalsError] = useState<string | null>(null);
  const [news, setNews] = useState<NewsItemView[]>([]);
  const [newsError, setNewsError] = useState<string | null>(null);
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
    setFundamentals(null);
    setFundamentalsError(null);
    setNews([]);
    setNewsError(null);

    // Deliberately two independent calls, not Promise.all — fundamentals
    // and news come from different Yahoo endpoints with different
    // reliability (quoteSummary has been unreliable in practice; the
    // search/news endpoint has not), and Promise.all fails the whole pane
    // the moment either one does. News showing up shouldn't depend on
    // fundamentals succeeding, or vice versa.
    api
      .getFundamentals(selected)
      .then(setFundamentals)
      .catch((e) => setFundamentalsError(String(e)));

    api
      .getStockNews(selected, newsLimit)
      .then(setNews)
      .catch((e) => setNewsError(String(e)))
      .finally(() => setLoading(false));
  }, [selected, newsLimit]);

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

        {!loading && selected && (
          <>
            <p style={{ fontSize: 16, fontWeight: 500, margin: "0 0 2px" }}>{selected}</p>
            <p style={{ fontSize: 11, color: colors.textMuted, margin: "0 0 12px" }}>
              {fundamentals ? [fundamentals.sector, fundamentals.industry].filter(Boolean).join(" · ") || "Sector/industry unavailable" : ""}
            </p>

            {fundamentals ? (
              <>
                <div style={{ display: "grid", gridTemplateColumns: "repeat(5, 1fr)", gap: 8, marginBottom: 16 }}>
                  <div style={{ ...panelStyle, padding: 10 }}>
                    <div style={{ fontSize: 10, color: colors.textMuted }}>Market cap</div>
                    <div style={{ fontSize: 13, fontWeight: 500 }}>{fmtMoney(fundamentals.market_cap)}</div>
                  </div>
                  <div style={{ ...panelStyle, padding: 10 }}>
                    <div style={{ fontSize: 10, color: colors.textMuted }}>Volume (today)</div>
                    <div style={{ fontSize: 13, fontWeight: 500 }}>
                      {fundamentals.volume != null ? fundamentals.volume.toLocaleString() : "—"}
                    </div>
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
              </>
            ) : (
              <p style={{ fontSize: 12, color: colors.textMuted, marginBottom: 16 }}>
                Fundamentals aren't available right now for this symbol — Yahoo's fundamentals endpoint has been
                unreliable in practice (see Settings for the underlying reason). News below is fetched
                independently and isn't affected by this.
                {fundamentalsError && <span style={{ display: "block", marginTop: 4, opacity: 0.7 }}>{fundamentalsError}</span>}
              </p>
            )}

            <div style={{ display: "flex", justifyContent: "space-between", alignItems: "center", margin: "0 0 8px" }}>
              <p style={{ fontSize: 13, fontWeight: 600, margin: 0 }}>News and highlights (top {newsLimit})</p>
              <label style={{ fontSize: 11, color: colors.textMuted, display: "flex", alignItems: "center", gap: 6 }}>
                Show
                <select value={newsLimit} onChange={(e) => setNewsLimit(Number(e.target.value))} style={{ fontSize: 11 }}>
                  <option value={5}>5</option>
                  <option value={10}>10</option>
                  <option value={15}>15</option>
                </select>
              </label>
            </div>
            <p style={{ fontSize: 11, color: colors.textMuted, margin: "0 0 8px" }}>
              Items labeled "NSE/BSE (verified filing)" are real corporate announcements pulled directly
              from the exchange — not guaranteed to succeed every time (both are unofficial endpoints; NSE
              in particular can be unavailable from some networks), but genuine filings when they load, not
              a keyword guess. Other "Regulatory" tags are a plain keyword match over general news
              headlines — a helpful sort, not a verified feed.
            </p>
            {newsError ? (
              <p style={{ fontSize: 12, color: colors.danger }}>Couldn't load news: {newsError}</p>
            ) : news.length === 0 ? (
              <p style={{ fontSize: 12, color: colors.textMuted }}>No news found for this symbol.</p>
            ) : (
              <div style={{ display: "flex", flexDirection: "column", gap: 6 }}>
                {news.map((n, i) => {
                  const content = (
                    <>
                      {n.is_regulatory && (
                        <div style={{ fontSize: 9, color: "#633806", fontWeight: 600 }}>REGULATORY</div>
                      )}
                      <div style={{ fontSize: 12 }}>{n.title}</div>
                      <div style={{ fontSize: 10, color: colors.textMuted }}>
                        {n.publisher} · {n.published_at}
                      </div>
                    </>
                  );
                  const sharedStyle = {
                    display: "block" as const,
                    padding: "6px 10px",
                    borderLeft: `3px solid ${n.is_regulatory ? "#854F0B" : colors.border}`,
                    background: n.is_regulatory ? "#FAEEDA" : "transparent",
                    borderRadius: "0 4px 4px 0" as const,
                  };
                  // Many NSE/BSE filing categories (postal ballots, some
                  // press releases) genuinely carry no PDF attachment —
                  // rendering those as a dead `href=""` link is what read
                  // as "not hyperlinked to any details." A plain,
                  // non-interactive block is the honest rendering when
                  // there's nowhere real to send the click.
                  return n.link ? (
                    <a key={i} href={n.link} target="_blank" rel="noreferrer" style={{ ...sharedStyle, textDecoration: "none", color: "inherit" }}>
                      {content}
                    </a>
                  ) : (
                    <div key={i} style={sharedStyle}>
                      {content}
                    </div>
                  );
                })}
              </div>
            )}
          </>
        )}
      </div>
    </div>
  );
}

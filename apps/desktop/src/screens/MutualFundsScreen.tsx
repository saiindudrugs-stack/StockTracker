import { useEffect, useState } from "react";
import { api } from "../lib/tauri";
import type { MfHoldingView, MfSchemeSearchResultView } from "../lib/types";
import { colors, panelStyle, pnlColor, fmtMoney, tableHeaderRow, tableHeaderCell, firstHeaderCell, lastHeaderCell } from "../lib/theme";
import { ConfirmButton } from "../components/ConfirmButton";

export function MutualFundsScreen({ portfolioId }: { portfolioId: string }) {
  const [funds, setFunds] = useState<MfHoldingView[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [refreshing, setRefreshing] = useState(false);
  const [refreshMsg, setRefreshMsg] = useState<string | null>(null);

  // Search-to-add picker state
  const [query, setQuery] = useState("");
  const [results, setResults] = useState<MfSchemeSearchResultView[]>([]);
  const [selected, setSelected] = useState<MfSchemeSearchResultView | null>(null);
  const [searching, setSearching] = useState(false);
  const [txnType, setTxnType] = useState<"buy" | "sell">("buy");
  const [units, setUnits] = useState("");
  const [nav, setNav] = useState("");

  const [csvFile, setCsvFile] = useState<File | null>(null);
  const [importResult, setImportResult] = useState<{ imported: number; failed: number; rows: { row_number: number; symbol: string; status: string }[] } | null>(null);

  async function refreshFunds() {
    try {
      setFunds(await api.listMutualFunds(portfolioId));
      setError(null);
    } catch (e) {
      setError(String(e));
    }
  }

  useEffect(() => {
    refreshFunds();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [portfolioId]);

  // Debounced type-to-search — waits for a short pause in typing before
  // hitting the cache, rather than searching on every keystroke.
  useEffect(() => {
    if (query.trim().length < 2) {
      setResults([]);
      return;
    }
    setSearching(true);
    const handle = setTimeout(() => {
      api
        .searchMfSchemes(query)
        .then(setResults)
        .catch((e) => setError(String(e)))
        .finally(() => setSearching(false));
    }, 300);
    return () => clearTimeout(handle);
  }, [query]);

  async function handleRefreshAll() {
    setRefreshing(true);
    setRefreshMsg(null);
    try {
      const cacheResult = await api.refreshMfSchemeCache();
      const navResult = await api.refreshMfNav(portfolioId);
      await refreshFunds();
      const parts = [`Scheme list: ${cacheResult.scheme_count.toLocaleString()} schemes loaded.`];
      if (navResult.updated.length > 0) parts.push(`NAV updated: ${navResult.updated.join(", ")}.`);
      if (navResult.failed.length > 0) {
        parts.push(`Failed: ${navResult.failed.map((f) => `${f.symbol} (${f.reason})`).join("; ")}`);
      }
      setRefreshMsg(parts.join(" "));
    } catch (e) {
      setRefreshMsg(String(e));
    } finally {
      setRefreshing(false);
    }
  }

  async function handleSelectScheme(scheme: MfSchemeSearchResultView) {
    setSelected(scheme);
    setQuery(scheme.scheme_name);
    setResults([]);
    setNav(scheme.nav);
    try {
      await api.addMutualFund(scheme.scheme_code);
    } catch (e) {
      setError(String(e));
    }
  }

  async function handleRecordTransaction() {
    if (!selected || !units || !nav) {
      setError("Search for and select a fund, then enter units and NAV.");
      return;
    }
    try {
      if (txnType === "buy") {
        await api.recordBuy(portfolioId, selected.scheme_code, units, nav);
      } else {
        await api.recordSell(portfolioId, selected.scheme_code, units, nav);
      }
      await refreshFunds();
      setError(null);
      setUnits("");
      setSelected(null);
      setQuery("");
    } catch (e) {
      setError(String(e));
    }
  }

  async function handleRemove(scheme_code: string) {
    try {
      await api.removeHolding(portfolioId, scheme_code);
      await refreshFunds();
      setError(null);
    } catch (e) {
      setError(String(e));
    }
  }

  function handleDownloadTemplate() {
    const csv = "SchemeCode,Units,BuyNAV,BuyDate\n119551,250.5,106.20,2025-04-10\n120438,100,2884.93,\n";
    const blob = new Blob([csv], { type: "text/csv" });
    const url = URL.createObjectURL(blob);
    const a = document.createElement("a");
    a.href = url;
    a.download = "Mutual_Fund_Template.csv";
    a.click();
    URL.revokeObjectURL(url);
  }

  async function handleImport() {
    if (!csvFile) return;
    const text = await csvFile.text();
    try {
      const result = await api.importMfCsv(portfolioId, text);
      setImportResult(result);
      await refreshFunds();
      setError(null);
    } catch (e) {
      setError(String(e));
    }
  }

  async function handleExport() {
    try {
      const csv = await api.exportMfCsv(portfolioId);
      const blob = new Blob([csv], { type: "text/csv" });
      const url = URL.createObjectURL(blob);
      const a = document.createElement("a");
      a.href = url;
      a.download = `Mutual_Funds_Export_${portfolioId.slice(0, 8)}.csv`;
      a.click();
      URL.revokeObjectURL(url);
    } catch (e) {
      setError(String(e));
    }
  }

  return (
    <div style={{ padding: 24 }}>
      <h1 style={{ fontSize: 20, color: colors.navy, marginBottom: 4 }}>Mutual funds</h1>
      <p style={{ fontSize: 13, color: colors.textMuted, marginTop: 0 }}>
        Tracked completely separately from equities — NAV instead of LTP, XIRR as the primary
        return metric, and its own CSV format. NAV comes from AMFI's official daily file, not
        Yahoo Finance.
      </p>

      <div style={{ display: "flex", gap: 8, alignItems: "center", marginBottom: 6 }}>
        <button onClick={handleRefreshAll} disabled={refreshing}>
          {refreshing ? "Refreshing…" : "Refresh Fund List + NAV"}
        </button>
      </div>
      {refreshMsg && <p style={{ fontSize: 12, color: colors.textMuted, marginBottom: 12 }}>{refreshMsg}</p>}
      {error && <p style={{ color: colors.danger }}>{error}</p>}

      <table className="data-table" style={{ borderCollapse: "collapse", width: "100%", fontSize: 13, marginBottom: 24 }}>
        <thead>
          <tr style={tableHeaderRow}>
            <th style={{ ...tableHeaderCell, ...firstHeaderCell }}>Scheme</th>
            <th style={tableHeaderCell}>Category</th>
            <th style={tableHeaderCell}>Units</th>
            <th style={tableHeaderCell}>Avg NAV</th>
            <th style={tableHeaderCell}>Current NAV</th>
            <th style={tableHeaderCell}>NAV chg %</th>
            <th style={tableHeaderCell}>Value</th>
            <th style={tableHeaderCell}>Unreal. P/L</th>
            <th style={tableHeaderCell}>CAGR %</th>
            <th style={{ ...tableHeaderCell, ...lastHeaderCell }}></th>
          </tr>
        </thead>
        <tbody>
          {funds.map((f) => (
            <tr key={f.scheme_code} style={{ borderBottom: "1px solid #eee" }}>
              <td style={{ padding: "6px 8px 6px 0" }}>{f.scheme_name}</td>
              <td>
                <span style={{ fontSize: 10, color: "#3C3489", background: "#EEEDFE", borderRadius: 4, padding: "2px 6px" }}>
                  {f.category ?? "—"}
                </span>
              </td>
              <td>{f.units}</td>
              <td>{fmtMoney(f.avg_nav)}</td>
              <td>{fmtMoney(f.current_nav)}</td>
              <td style={{ color: f.nav_change_pct != null ? pnlColor(f.nav_change_pct) : colors.textMuted }}>
                {f.nav_change_pct != null ? `${(f.nav_change_pct * 100).toFixed(2)}%` : "—"}
              </td>
              <td>{fmtMoney(f.market_value)}</td>
              <td style={{ color: f.unrealized_pnl != null ? pnlColor(parseFloat(f.unrealized_pnl)) : colors.textMuted, fontWeight: 500 }}>
                {fmtMoney(f.unrealized_pnl)}
              </td>
              <td style={{ color: f.cagr_pct != null ? pnlColor(f.cagr_pct) : colors.textMuted }}>
                {f.cagr_pct != null ? `${f.cagr_pct.toFixed(2)}%` : "—"}
              </td>
              <td>
                <ConfirmButton label="Remove" confirmLabel="Yes, delete" onConfirm={() => handleRemove(f.scheme_code)} />
              </td>
            </tr>
          ))}
          {funds.length === 0 && (
            <tr>
              <td colSpan={10} style={{ padding: "12px 0", color: colors.textMuted, fontSize: 12 }}>
                No mutual funds in this portfolio yet — search for one below.
              </td>
            </tr>
          )}
        </tbody>
      </table>

      <h2 style={{ fontSize: 15, color: colors.navy, marginBottom: 8 }}>Add / record a transaction</h2>
      <div style={{ position: "relative", marginBottom: 8, maxWidth: 480 }}>
        <input
          value={query}
          onChange={(e) => {
            setQuery(e.target.value);
            setSelected(null);
          }}
          placeholder="Search by scheme name, e.g. HDFC Corporate Bond"
          style={{ width: "100%" }}
        />
        {searching && <span style={{ fontSize: 11, color: colors.textMuted }}>Searching…</span>}
        {results.length > 0 && (
          <div style={{ ...panelStyle, position: "absolute", zIndex: 5, background: "white", maxHeight: 240, overflowY: "auto", width: "100%" }}>
            {results.map((r) => (
              <div
                key={r.scheme_code}
                onClick={() => handleSelectScheme(r)}
                style={{ padding: "6px 4px", fontSize: 12, cursor: "pointer", borderBottom: "1px solid #eee" }}
              >
                <div>{r.scheme_name}</div>
                <div style={{ color: colors.textMuted, fontSize: 10 }}>
                  {r.amc_name} · {r.category} · NAV {r.nav} ({r.nav_date})
                </div>
              </div>
            ))}
          </div>
        )}
      </div>
      {selected && (
        <p style={{ fontSize: 11, color: colors.textMuted, marginTop: 0 }}>
          Selected: <strong>{selected.scheme_name}</strong> (Scheme Code {selected.scheme_code}) — no manual code entry needed.
        </p>
      )}

      <div style={{ display: "flex", gap: 6, marginBottom: 8 }}>
        <button
          onClick={() => setTxnType("buy")}
          style={{
            fontSize: 12,
            padding: "4px 14px",
            borderRadius: 6,
            border: `1px solid ${txnType === "buy" ? colors.success : colors.border}`,
            background: txnType === "buy" ? "#DFF3E3" : "transparent",
            color: txnType === "buy" ? colors.success : colors.textMuted,
          }}
        >
          Buy
        </button>
        <button
          onClick={() => setTxnType("sell")}
          style={{
            fontSize: 12,
            padding: "4px 14px",
            borderRadius: 6,
            border: `1px solid ${txnType === "sell" ? colors.danger : colors.border}`,
            background: txnType === "sell" ? "#FBE4E2" : "transparent",
            color: txnType === "sell" ? colors.danger : colors.textMuted,
          }}
        >
          Sell
        </button>
      </div>
      <div style={{ display: "flex", gap: 8, alignItems: "center", marginBottom: 28 }}>
        <input value={units} onChange={(e) => setUnits(e.target.value)} placeholder="Units" style={{ width: 100 }} />
        <input value={nav} onChange={(e) => setNav(e.target.value)} placeholder="NAV" style={{ width: 100 }} />
        <button
          onClick={handleRecordTransaction}
          style={{ background: txnType === "buy" ? colors.success : colors.danger, color: "white", border: "none", borderRadius: 4, padding: "6px 14px" }}
        >
          Record {txnType === "buy" ? "Buy" : "Sell"}
        </button>
      </div>

      <h2 style={{ fontSize: 15, color: colors.navy, marginBottom: 8 }}>Bulk import / export (CSV)</h2>
      <p style={{ fontSize: 12, color: colors.textMuted, marginTop: 0 }}>
        One row per holding: SchemeCode (required — use the AMFI Scheme Code, not the fund name;
        names collide across Direct/Regular and Growth/IDCW variants), Units, BuyNAV, BuyDate
        (optional — blank defaults to exactly one year ago).
      </p>
      <div style={{ display: "flex", gap: 8, alignItems: "center" }}>
        <button onClick={handleDownloadTemplate}>Download CSV Template</button>
        <input type="file" accept=".csv" onChange={(e) => setCsvFile(e.target.files?.[0] ?? null)} style={{ fontSize: 12 }} />
        <button onClick={handleImport} disabled={!csvFile}>
          Import {csvFile ? `"${csvFile.name}"` : ""}
        </button>
        <button onClick={handleExport}>Export Holdings (CSV)</button>
      </div>
      {importResult && (
        <div style={{ ...panelStyle, marginTop: 12 }}>
          <p style={{ fontSize: 12, color: importResult.failed > 0 ? colors.danger : colors.success, fontWeight: 600, margin: "0 0 6px" }}>
            {importResult.imported} imported, {importResult.failed} failed
          </p>
          <ul style={{ margin: 0, paddingLeft: 18, fontSize: 11, color: colors.textMuted }}>
            {importResult.rows.map((r) => (
              <li key={r.row_number}>
                Row {r.row_number} ({r.symbol}): {r.status}
              </li>
            ))}
          </ul>
        </div>
      )}
    </div>
  );
}

//! Fetches and parses Kite's public instruments CSV dump — verified
//! real format from Kite's own docs: `instrument_token, exchange_token,
//! tradingsymbol, name, last_price, expiry, strike, tick_size, lot_size,
//! instrument_type, segment, exchange`. Per-exchange endpoint
//! (`/instruments/{exchange}`) keeps this to NSE + BSE, matching the
//! same scope as the Upstox instrument cache, rather than pulling every
//! F&O/currency/commodity instrument this app has no use for.
//!
//! HONESTY NOTE: built against Kite's documented CSV shape, not
//! live-tested — no Zerodha access_token was available in this sandbox.

use reqwest::Client;

use crate::market_data::MarketDataError;

pub struct KiteInstrumentRow {
    pub trading_symbol: String,
    pub exchange: String,
    pub instrument_token: u32,
}

pub struct KiteInstrumentFetcher {
    http: Client,
}

impl KiteInstrumentFetcher {
    pub fn new() -> Self {
        Self { http: Client::new() }
    }

    /// Requires a valid access_token — unlike Upstox's instrument file,
    /// Kite's requires the same daily-refreshed session token as
    /// everything else in their API.
    pub async fn fetch_nse_and_bse(&self, api_key: &str, access_token: &str) -> Result<Vec<KiteInstrumentRow>, MarketDataError> {
        let mut all_rows = Vec::new();
        for exchange in ["NSE", "BSE"] {
            let url = format!("https://api.kite.trade/instruments/{exchange}");
            let response = self
                .http
                .get(&url)
                .header("X-Kite-Version", "3")
                .header("Authorization", format!("token {api_key}:{access_token}"))
                .send()
                .await
                .map_err(|e| MarketDataError::RequestFailed(e.to_string()))?;
            let csv_text = response.text().await.map_err(|e| MarketDataError::RequestFailed(e.to_string()))?;
            all_rows.extend(parse_instruments_csv(&csv_text, exchange));
        }
        Ok(all_rows)
    }
}

impl Default for KiteInstrumentFetcher {
    fn default() -> Self {
        Self::new()
    }
}

/// Equities only (instrument_type == "EQ") — this CSV also lists futures,
/// options, and other segments this app doesn't track. Malformed or
/// short rows are skipped rather than failing the whole parse, same
/// resilience pattern as every other CSV/feed parser in this app.
fn parse_instruments_csv(csv_text: &str, exchange: &str) -> Vec<KiteInstrumentRow> {
    let mut rows = Vec::new();
    for (i, line) in csv_text.lines().enumerate() {
        if i == 0 {
            continue; // header row
        }
        let fields: Vec<&str> = line.split(',').collect();
        // instrument_token=0, tradingsymbol=2, instrument_type=9
        if fields.len() < 10 {
            continue;
        }
        if fields[9] != "EQ" {
            continue;
        }
        let Ok(instrument_token) = fields[0].parse::<u32>() else { continue };
        let trading_symbol = fields[2].to_string();
        if trading_symbol.is_empty() {
            continue;
        }
        rows.push(KiteInstrumentRow { trading_symbol, exchange: exchange.to_string(), instrument_token });
    }
    rows
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_a_real_shaped_instruments_csv_keeping_only_equities() {
        let csv = "instrument_token,exchange_token,tradingsymbol,name,last_price,expiry,strike,tick_size,lot_size,instrument_type,segment,exchange\n\
                   408065,1594,INFY,INFOSYS,1500.0,,,0.05,1,EQ,NSE,NSE\n\
                   5720322,22345,NIFTY15DECFUT,,78.0,2015-12-31,,0.05,75,FUT,NFO-FUT,NFO\n";
        let rows = parse_instruments_csv(csv, "NSE");
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].trading_symbol, "INFY");
        assert_eq!(rows[0].instrument_token, 408065);
    }

    #[test]
    fn skips_malformed_rows_without_panicking() {
        let csv = "header,row,here,a,b,c,d,e,f,g,h,i\n\
                   not_a_number,1594,INFY,INFOSYS,1500.0,,,0.05,1,EQ,NSE,NSE\n\
                   408066,1594,,INFOSYS,1500.0,,,0.05,1,EQ,NSE,NSE\n\
                   short,row\n";
        let rows = parse_instruments_csv(csv, "NSE");
        assert_eq!(rows.len(), 0);
    }

    #[test]
    fn tags_every_row_with_the_requested_exchange() {
        let csv = "h\n408065,1594,INFY,INFOSYS,1500.0,,,0.05,1,EQ,NSE,NSE\n";
        let rows = parse_instruments_csv(csv, "BSE");
        assert_eq!(rows[0].exchange, "BSE");
    }
}

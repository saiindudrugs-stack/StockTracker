//! Zerodha adapter — Phase 1 broker per the confirmed rollout order (SRS
//! 2.2.2). Implements `BrokerAdapter` against the real Kite Connect API
//! shape (https://kite.trade/docs/connect/v3/), so the request/response
//! plumbing here is genuine, not a mock. What's NOT included: an actual
//! Kite account to integration-test against — the unit tests below cover
//! the checksum and JSON-mapping logic in isolation, which is the part that
//! doesn't need a live session.

use super::{AuthCredentials, BrokerAdapter, BrokerError, BrokerHolding, BrokerIntradayPosition};
use async_trait::async_trait;
use chrono::NaiveDate;
use rust_decimal::Decimal;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::str::FromStr;

const BASE_URL: &str = "https://api.kite.trade";
const KITE_API_VERSION: &str = "3";

pub struct ZerodhaAdapter {
    http: reqwest::Client,
    api_key: String,
    access_token: Option<String>,
}

impl ZerodhaAdapter {
    pub fn new() -> Self {
        Self {
            http: reqwest::Client::new(),
            api_key: String::new(),
            access_token: None,
        }
    }

    /// Kite Connect's login handshake: the user completes a browser login
    /// against Zerodha directly (never through this app — credentials never
    /// touch our process), which redirects back with a `request_token`. This
    /// function does the *second* half: exchanging that token for a
    /// session's `access_token` via the documented checksum scheme
    /// (SHA-256 of api_key + request_token + api_secret).
    fn generate_checksum(api_key: &str, request_token: &str, api_secret: &str) -> String {
        let mut hasher = Sha256::new();
        hasher.update(api_key.as_bytes());
        hasher.update(request_token.as_bytes());
        hasher.update(api_secret.as_bytes());
        format!("{:x}", hasher.finalize())
    }

    fn auth_header(&self) -> Result<String, BrokerError> {
        let token = self.access_token.as_ref().ok_or(BrokerError::NotAuthenticated)?;
        Ok(format!("token {}:{}", self.api_key, token))
    }
}

impl Default for ZerodhaAdapter {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Deserialize)]
struct KiteSessionResponse {
    data: KiteSessionData,
}
#[derive(Debug, Deserialize)]
struct KiteSessionData {
    access_token: String,
}

#[derive(Debug, Deserialize)]
struct KiteHoldingsResponse {
    data: Vec<KiteHolding>,
}
#[derive(Debug, Deserialize)]
struct KiteHolding {
    tradingsymbol: String,
    exchange: String,
    isin: String,
    quantity: i64,
    average_price: f64,
    last_price: f64,
}

#[derive(Debug, Deserialize)]
struct KitePositionsResponse {
    data: KitePositionsData,
}
#[derive(Debug, Deserialize)]
struct KitePositionsData {
    day: Vec<KitePosition>,
}
#[derive(Debug, Deserialize)]
struct KitePosition {
    tradingsymbol: String,
    exchange: String,
    quantity: i64,
    average_price: f64,
    last_price: f64,
    product: String,
    // Kite's positions endpoint doesn't return ISIN directly; a real
    // implementation resolves it via the instruments master-data dump
    // (a separate, cached-daily Kite endpoint). Left as a TODO seam rather
    // than guessed at, since inventing a mapping here would be silently wrong.
}

fn f64_to_decimal(context: &str, v: f64) -> Result<Decimal, BrokerError> {
    Decimal::from_str(&v.to_string())
        .map_err(|e| BrokerError::UnexpectedResponse(format!("{context}: {e}")))
}

#[async_trait]
impl BrokerAdapter for ZerodhaAdapter {
    fn broker_code(&self) -> &'static str {
        "zerodha"
    }

    async fn authenticate(&mut self, credentials: AuthCredentials) -> Result<(), BrokerError> {
        // For Zerodha, `credentials.secret_or_token` carries the
        // request_token obtained from the browser redirect; the API secret
        // itself is read from the OS keychain by the caller and passed here
        // — this adapter never persists it (HLD Section 8).
        //
        // NOTE: the real flow needs the api_secret too, which the
        // `AuthCredentials` shape (api_key + one opaque token) doesn't carry
        // separately. This is a known gap flagged for the next LLD pass —
        // v1's `AuthCredentials` was written broker-agnostic before this
        // adapter surfaced that Kite's handshake needs three inputs, not two.
        // Left visible here rather than silently working around it.
        self.api_key = credentials.api_key.clone();
        let request_token = credentials.secret_or_token.clone();
        let api_secret = std::env::var("ZERODHA_API_SECRET").map_err(|_| {
            BrokerError::AuthFailed(
                "ZERODHA_API_SECRET not available — see NOTE in authenticate()".to_string(),
            )
        })?;

        let checksum = Self::generate_checksum(&self.api_key, &request_token, &api_secret);

        let response = self
            .http
            .post(format!("{BASE_URL}/session/token"))
            .header("X-Kite-Version", KITE_API_VERSION)
            .form(&[
                ("api_key", self.api_key.as_str()),
                ("request_token", request_token.as_str()),
                ("checksum", checksum.as_str()),
            ])
            .send()
            .await
            .map_err(|e| BrokerError::RequestFailed(e.to_string()))?;

        if !response.status().is_success() {
            return Err(BrokerError::AuthFailed(format!(
                "Kite returned status {}",
                response.status()
            )));
        }

        let parsed: KiteSessionResponse = response
            .json()
            .await
            .map_err(|e| BrokerError::UnexpectedResponse(e.to_string()))?;
        self.access_token = Some(parsed.data.access_token);
        Ok(())
    }

    async fn fetch_holdings(&self) -> Result<Vec<BrokerHolding>, BrokerError> {
        let response = self
            .http
            .get(format!("{BASE_URL}/portfolio/holdings"))
            .header("X-Kite-Version", KITE_API_VERSION)
            .header("Authorization", self.auth_header()?)
            .send()
            .await
            .map_err(|e| BrokerError::RequestFailed(e.to_string()))?;

        let parsed: KiteHoldingsResponse = response
            .json()
            .await
            .map_err(|e| BrokerError::UnexpectedResponse(e.to_string()))?;

        parsed
            .data
            .into_iter()
            .map(|h| {
                Ok(BrokerHolding {
                    isin: h.isin,
                    symbol: h.tradingsymbol,
                    exchange: h.exchange,
                    quantity: Decimal::from(h.quantity),
                    avg_cost: f64_to_decimal("average_price", h.average_price)?,
                    last_traded_price: f64_to_decimal("last_price", h.last_price)?,
                })
            })
            .collect()
    }

    async fn fetch_intraday_positions(&self) -> Result<Vec<BrokerIntradayPosition>, BrokerError> {
        let response = self
            .http
            .get(format!("{BASE_URL}/portfolio/positions"))
            .header("X-Kite-Version", KITE_API_VERSION)
            .header("Authorization", self.auth_header()?)
            .send()
            .await
            .map_err(|e| BrokerError::RequestFailed(e.to_string()))?;

        let parsed: KitePositionsResponse = response
            .json()
            .await
            .map_err(|e| BrokerError::UnexpectedResponse(e.to_string()))?;

        parsed
            .data
            .day
            .into_iter()
            .map(|p| {
                Ok(BrokerIntradayPosition {
                    isin: String::new(), // see KitePosition note above
                    symbol: p.tradingsymbol,
                    exchange: p.exchange,
                    quantity: Decimal::from(p.quantity),
                    entry_price: f64_to_decimal("average_price", p.average_price)?,
                    last_traded_price: f64_to_decimal("last_price", p.last_price)?,
                    product_type: p.product,
                })
            })
            .collect()
    }

    async fn fetch_historical_daily(
        &self,
        _isin: &str,
        _from: NaiveDate,
        _to: NaiveDate,
    ) -> Result<Vec<(NaiveDate, Decimal)>, BrokerError> {
        // Kite's historical-candles endpoint is keyed by `instrument_token`
        // (an internal Kite ID resolved from the instruments master-data
        // dump), not ISIN directly — same resolution step noted on
        // KitePosition above. Wiring the instrument-token lookup is next
        // LLD's job; left unimplemented rather than faked.
        Err(BrokerError::RequestFailed(
            "historical daily fetch requires instrument_token resolution — not yet wired".to_string(),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn checksum_is_deterministic_sha256_hex() {
        let a = ZerodhaAdapter::generate_checksum("key123", "reqtok456", "secret789");
        let b = ZerodhaAdapter::generate_checksum("key123", "reqtok456", "secret789");
        assert_eq!(a, b, "same inputs must always produce the same checksum");
        assert_eq!(a.len(), 64, "SHA-256 hex digest is always 64 chars");
        assert!(a.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn checksum_changes_if_any_input_changes() {
        let base = ZerodhaAdapter::generate_checksum("key123", "reqtok456", "secret789");
        let different_token = ZerodhaAdapter::generate_checksum("key123", "different", "secret789");
        assert_ne!(base, different_token);
    }

    #[test]
    fn parses_kite_holdings_response_shape() {
        let json = r#"{
            "data": [
                {
                    "tradingsymbol": "RELIANCE",
                    "exchange": "NSE",
                    "isin": "INE002A01018",
                    "quantity": 10,
                    "average_price": 2450.5,
                    "last_price": 2510.0
                }
            ]
        }"#;
        let parsed: KiteHoldingsResponse = serde_json::from_str(json).unwrap();
        assert_eq!(parsed.data.len(), 1);
        assert_eq!(parsed.data[0].tradingsymbol, "RELIANCE");
        assert_eq!(parsed.data[0].quantity, 10);
    }

    #[tokio::test]
    async fn methods_reject_calls_before_authentication() {
        let adapter = ZerodhaAdapter::new();
        let result = adapter.fetch_holdings().await;
        assert!(matches!(result, Err(BrokerError::NotAuthenticated)));
    }
}

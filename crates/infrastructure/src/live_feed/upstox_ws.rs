//! Upstox's live WebSocket feed — genuinely real-time (sub-second),
//! unlike the REST polling every other price fetch in this app does.
//! Two-step connect, per Upstox's own docs:
//! 1. GET /v2/feed/market-data-feed/authorize (Bearer: Analytics Token)
//!    → a one-time-use wss:// URL.
//! 2. Connect to that URL, send a JSON subscribe message, receive
//!    binary Protobuf-encoded frames back (decoded via the schema in
//!    proto/market_data_feed_v3.proto).
//!
//! Deliberately NOT reusing live_feed::TickTransport as-is — that trait's
//! send_subscribe takes numeric instrument tokens (Kite's model); Upstox
//! subscribes by string instrument_key inside a JSON message, a different
//! enough shape that forcing a fit would cost more clarity than it saves.
//! live_feed::PriceTick is still reused — that part IS genuinely generic.
//!
//! HONESTY NOTE: built against Upstox's own documented request/response
//! shapes and a verified real .proto schema, not live-tested — no
//! Analytics Token was available in this sandbox to test a real
//! connection against.

use chrono::{TimeZone, Utc};
use futures_util::{SinkExt, StreamExt};
use prost::Message as ProstMessage;
use reqwest::Client;
use rust_decimal::Decimal;
use serde::Serialize;
use std::str::FromStr;
use std::sync::Arc;
use tokio_tungstenite::tungstenite::Message as WsMessage;
use uuid::Uuid;

use crate::live_feed::PriceTick;
use crate::market_data::MarketDataError;
use crate::sqlite::SqliteAppSettings;

pub mod proto {
    include!(concat!(env!("OUT_DIR"), "/com.upstox.marketdatafeederv3udapi.rpc.proto.rs"));
}

use proto::feed::FeedUnion;
use proto::FeedResponse;

#[derive(Serialize)]
struct SubscribeMessage<'a> {
    guid: String,
    method: &'static str,
    data: SubscribeData<'a>,
}
#[derive(Serialize)]
struct SubscribeData<'a> {
    mode: &'static str,
    #[serde(rename = "instrumentKeys")]
    instrument_keys: &'a [String],
}

/// A tick as received off the wire, before resolving instrument_key back
/// to this app's own Instrument UUID — kept separate from PriceTick so
/// the resolution step (a DB lookup) can happen in the caller, not deep
/// inside frame decoding.
pub struct RawUpstoxTick {
    pub instrument_key: String,
    pub ltp: f64,
    pub ltt_ms: i64,
}

pub struct UpstoxLiveFeedClient {
    http: Client,
    settings: Arc<SqliteAppSettings>,
}

impl UpstoxLiveFeedClient {
    pub fn new(settings: Arc<SqliteAppSettings>) -> Self {
        Self { http: Client::new(), settings }
    }

    async fn token(&self) -> Result<String, MarketDataError> {
        self.settings
            .get(crate::market_data::upstox::UPSTOX_ANALYTICS_TOKEN_SETTING)
            .await
            .map_err(|e| MarketDataError::RequestFailed(e.to_string()))?
            .filter(|t| !t.trim().is_empty())
            .ok_or_else(|| MarketDataError::RequestFailed("no Upstox Analytics Token configured in Settings".to_string()))
    }

    /// Step 1 of the two-step connect — exchanges the Analytics Token for
    /// a one-time-use wss:// URL.
    async fn fetch_authorized_ws_url(&self) -> Result<String, MarketDataError> {
        let token = self.token().await?;
        let response = self
            .http
            .get("https://api.upstox.com/v3/feed/market-data-feed/authorize")
            .header("Authorization", format!("Bearer {token}"))
            .header("Accept", "application/json")
            .send()
            .await
            .map_err(|e| MarketDataError::RequestFailed(e.to_string()))?;

        #[derive(serde::Deserialize)]
        struct AuthorizeResponse {
            data: Option<AuthorizeData>,
        }
        #[derive(serde::Deserialize)]
        struct AuthorizeData {
            #[serde(rename = "authorizedRedirectUri", alias = "authorized_redirect_uri")]
            authorized_redirect_uri: String,
        }

        let body: AuthorizeResponse = response
            .json()
            .await
            .map_err(|e| MarketDataError::UnexpectedResponse(format!("couldn't parse Upstox WS authorize response: {e}")))?;

        body.data
            .map(|d| d.authorized_redirect_uri)
            .ok_or_else(|| MarketDataError::NoData("Upstox WS authorize returned no URL — check the Analytics Token".to_string()))
    }

    /// Connects, subscribes to the given instrument_keys in LTPC mode
    /// (price + close only — the lightest mode, sufficient for what this
    /// app displays), and calls `on_tick` for every decoded price update
    /// until the connection closes or errors. Runs until cancelled by the
    /// caller (e.g. dropping the future) — this is the long-lived loop,
    /// not a one-shot call.
    pub async fn stream<F>(&self, instrument_keys: Vec<String>, mut on_tick: F) -> Result<(), MarketDataError>
    where
        F: FnMut(RawUpstoxTick) + Send,
    {
        let ws_url = self.fetch_authorized_ws_url().await?;
        let (mut ws_stream, _) =
            tokio_tungstenite::connect_async(&ws_url).await.map_err(|e| MarketDataError::RequestFailed(format!("Upstox WS connect failed: {e}")))?;

        let subscribe = SubscribeMessage {
            guid: Uuid::new_v4().to_string(),
            method: "sub",
            data: SubscribeData { mode: "ltpc", instrument_keys: &instrument_keys },
        };
        let subscribe_json = serde_json::to_string(&subscribe).map_err(|e| MarketDataError::RequestFailed(e.to_string()))?;
        // Per Upstox's docs: the SUBSCRIBE request itself is sent as a
        // binary frame containing JSON text, not a WebSocket text frame —
        // an easy, real mistake to make if copying patterns from a plain
        // JSON API.
        ws_stream
            .send(WsMessage::Binary(subscribe_json.into_bytes()))
            .await
            .map_err(|e| MarketDataError::RequestFailed(format!("Upstox WS subscribe failed: {e}")))?;

        while let Some(msg) = ws_stream.next().await {
            let msg = msg.map_err(|e| MarketDataError::RequestFailed(format!("Upstox WS read failed: {e}")))?;
            let bytes = match msg {
                WsMessage::Binary(b) => b,
                WsMessage::Close(_) => break,
                _ => continue, // ping/pong/text frames aren't feed data
            };
            let Ok(decoded) = FeedResponse::decode(&bytes[..]) else { continue }; // one malformed frame shouldn't kill the whole stream
            for (instrument_key, feed) in decoded.feeds {
                if let Some(ltpc) = extract_ltpc(&feed) {
                    on_tick(RawUpstoxTick { instrument_key, ltp: ltpc.ltp, ltt_ms: ltpc.ltt });
                }
            }
        }
        Ok(())
    }
}

/// LTPC is nested differently depending on which feed variant came back
/// (plain ltpc, full feed's marketFF/indexFF, or firstLevelWithGreeks) —
/// this pulls it out regardless of which one is present, since this app
/// only ever wants the price, not the full depth/greeks payload.
fn extract_ltpc(feed: &proto::Feed) -> Option<proto::Ltpc> {
    match &feed.feed_union {
        Some(FeedUnion::Ltpc(l)) => Some(l.clone()),
        Some(FeedUnion::FullFeed(ff)) => match &ff.full_feed_union {
            Some(proto::full_feed::FullFeedUnion::MarketFf(m)) => m.ltpc.clone(),
            Some(proto::full_feed::FullFeedUnion::IndexFf(i)) => i.ltpc.clone(),
            None => None,
        },
        Some(FeedUnion::FirstLevelWithGreeks(f)) => f.ltpc.clone(),
        None => None,
    }
}

/// Converts a raw tick (instrument_key still unresolved) into this app's
/// generic PriceTick once the instrument_id lookup succeeds — kept as a
/// free function so it's testable without a live connection.
pub fn to_price_tick(raw: &RawUpstoxTick, instrument_id: Uuid) -> Option<PriceTick> {
    let ltp = Decimal::from_str(&raw.ltp.to_string()).ok()?;
    let timestamp = Utc.timestamp_millis_opt(raw.ltt_ms).single()?;
    Some(PriceTick { instrument_id, ltp, timestamp })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_ltpc_from_the_plain_ltpc_feed_variant() {
        let feed = proto::Feed { feed_union: Some(FeedUnion::Ltpc(proto::Ltpc { ltp: 100.5, ltt: 1000, ltq: 10, cp: 99.0 })), request_mode: 0 };
        let ltpc = extract_ltpc(&feed).unwrap();
        assert_eq!(ltpc.ltp, 100.5);
    }

    #[test]
    fn extracts_ltpc_from_a_market_full_feed() {
        let inner = proto::MarketFullFeed {
            ltpc: Some(proto::Ltpc { ltp: 200.0, ltt: 2000, ltq: 5, cp: 195.0 }),
            ..Default::default()
        };
        let feed = proto::Feed {
            feed_union: Some(FeedUnion::FullFeed(proto::FullFeed { full_feed_union: Some(proto::full_feed::FullFeedUnion::MarketFf(inner)) })),
            request_mode: 1,
        };
        let ltpc = extract_ltpc(&feed).unwrap();
        assert_eq!(ltpc.ltp, 200.0);
    }

    #[test]
    fn extracts_ltpc_from_first_level_with_greeks() {
        let feed = proto::Feed {
            feed_union: Some(FeedUnion::FirstLevelWithGreeks(proto::FirstLevelWithGreeks {
                ltpc: Some(proto::Ltpc { ltp: 300.0, ltt: 3000, ltq: 1, cp: 290.0 }),
                ..Default::default()
            })),
            request_mode: 2,
        };
        let ltpc = extract_ltpc(&feed).unwrap();
        assert_eq!(ltpc.ltp, 300.0);
    }

    #[test]
    fn missing_feed_union_returns_none_not_a_panic() {
        let feed = proto::Feed { feed_union: None, request_mode: 0 };
        assert!(extract_ltpc(&feed).is_none());
    }

    #[test]
    fn to_price_tick_converts_a_raw_tick_given_a_resolved_instrument_id() {
        let raw = RawUpstoxTick { instrument_key: "NSE_EQ|INE002A01018".to_string(), ltp: 1288.6, ltt_ms: 1721664000000 };
        let id = Uuid::new_v4();
        let tick = to_price_tick(&raw, id).unwrap();
        assert_eq!(tick.instrument_id, id);
        assert_eq!(tick.ltp.to_string(), "1288.6");
    }

    #[test]
    fn subscribe_message_serializes_with_the_documented_json_shape() {
        let keys = vec!["NSE_EQ|INE002A01018".to_string()];
        let msg = SubscribeMessage { guid: "abc".to_string(), method: "sub", data: SubscribeData { mode: "ltpc", instrument_keys: &keys } };
        let json = serde_json::to_string(&msg).unwrap();
        assert!(json.contains("\"method\":\"sub\""));
        assert!(json.contains("\"mode\":\"ltpc\""));
        assert!(json.contains("\"instrumentKeys\":[\"NSE_EQ|INE002A01018\"]"));
    }
}

//! Abstracts the WebSocket connection itself behind a trait, so
//! `LiveFeedManager`'s reconnect/backoff loop can be unit-tested against a
//! fake transport that fails on command — exercising real network flakiness
//! in a test would be slow and nondeterministic.

use async_trait::async_trait;
use futures_util::{SinkExt, StreamExt};
use tokio_tungstenite::tungstenite::Message;

#[derive(Debug, thiserror::Error)]
pub enum TransportError {
    #[error("connection failed: {0}")]
    ConnectFailed(String),
    #[error("connection closed")]
    Closed,
    #[error("send failed: {0}")]
    SendFailed(String),
}

#[async_trait]
pub trait TickTransport: Send {
    async fn connect(&mut self) -> Result<(), TransportError>;
    async fn send_subscribe(&mut self, instrument_tokens: &[u32]) -> Result<(), TransportError>;
    /// Returns the next raw text/binary frame as bytes, or an error if the
    /// connection dropped — the caller (manager.rs) is what decides to
    /// reconnect on error, this trait just reports it.
    async fn next_message(&mut self) -> Result<Vec<u8>, TransportError>;
}

/// Real implementation wrapping tokio-tungstenite. Message framing here is
/// broker-agnostic (raw bytes out); decoding those bytes into a `PriceTick`
/// is the `TickDecoder`'s job in manager.rs, since that part IS
/// broker-specific (Kite uses a compact binary tick format, not JSON — the
/// exact byte layout is Kite Connect API documentation the adapter would
/// need to encode a byte-for-byte decoder against, flagged as a follow-up
/// rather than guessed at here).
pub struct WebSocketTransport {
    url: String,
    stream: Option<
        tokio_tungstenite::WebSocketStream<
            tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
        >,
    >,
}

impl WebSocketTransport {
    pub fn new(url: String) -> Self {
        Self { url, stream: None }
    }
}

#[async_trait]
impl TickTransport for WebSocketTransport {
    async fn connect(&mut self) -> Result<(), TransportError> {
        let (stream, _response) = tokio_tungstenite::connect_async(&self.url)
            .await
            .map_err(|e| TransportError::ConnectFailed(e.to_string()))?;
        self.stream = Some(stream);
        Ok(())
    }

    async fn send_subscribe(&mut self, instrument_tokens: &[u32]) -> Result<(), TransportError> {
        let stream = self.stream.as_mut().ok_or(TransportError::Closed)?;
        // Kite's subscribe frame shape: {"a":"subscribe","v":[tokens...]}.
        // Documented broker-specific detail lives here in the transport
        // construction site, not baked into the trait.
        let payload = serde_json::json!({ "a": "subscribe", "v": instrument_tokens });
        stream
            .send(Message::Text(payload.to_string()))
            .await
            .map_err(|e| TransportError::SendFailed(e.to_string()))
    }

    async fn next_message(&mut self) -> Result<Vec<u8>, TransportError> {
        let stream = self.stream.as_mut().ok_or(TransportError::Closed)?;
        loop {
            match stream.next().await {
                Some(Ok(Message::Binary(bytes))) => return Ok(bytes),
                Some(Ok(Message::Text(text))) => return Ok(text.into_bytes()),
                Some(Ok(Message::Close(_))) | None => return Err(TransportError::Closed),
                Some(Ok(_)) => continue, // ping/pong/frame, skip
                Some(Err(e)) => return Err(TransportError::ConnectFailed(e.to_string())),
            }
        }
    }
}

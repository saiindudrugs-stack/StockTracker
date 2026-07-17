//! Live Feed Manager (HLD Section 3.2, 7; SRS 2.2.2 "Auto-reconnect,
//! exponential-backoff retry, and local cache fallback"). Confirmed resolution
//! is 1-minute bars only (Section 11 decision) — no tick-level storage — but
//! the live P/L path still needs every individual tick as it arrives; this
//! module separates those two concerns:
//!
//! - Every tick is broadcast immediately (`PriceTick` events) for live
//!   dashboard P/L (SRS 2.2.1 "tick-level or near-tick P/L updates").
//! - Ticks are also folded into `MinuteBarAggregator`, which only emits a
//!   completed bar once a minute boundary passes — that's what actually gets
//!   persisted to intraday_bar (Section 5.2), keeping storage at the
//!   confirmed 1-minute resolution regardless of how chatty the feed is.

pub mod aggregator;
pub mod reconnect;
pub mod transport;
pub mod manager;

pub use aggregator::{MinuteBar, MinuteBarAggregator};
pub use reconnect::ReconnectPolicy;
pub use transport::{TickTransport, WebSocketTransport};
pub use manager::{LiveFeedManager, PriceTick};

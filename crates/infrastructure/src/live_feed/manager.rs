//! Ties the transport, reconnect policy, and minute-bar aggregator together
//! into the actual run loop (HLD Section 7 "Application Flow" step 3: "Live
//! Feed Manager opens WebSocket subscriptions ... ticks update the
//! in-memory cache and emit events").

use super::aggregator::{MinuteBar, MinuteBarAggregator};
use super::reconnect::ReconnectPolicy;
use super::transport::{TickTransport, TransportError};
use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use std::collections::HashMap;
use tokio::sync::broadcast;
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq)]
pub struct PriceTick {
    pub instrument_id: Uuid,
    pub ltp: Decimal,
    pub timestamp: DateTime<Utc>,
}

/// Broker-specific: turns a raw frame from `TickTransport::next_message`
/// into zero or more ticks. Kite's actual binary tick packet format is
/// documented in Kite Connect's API docs but genuinely broker-specific byte
/// layout — this trait is the seam where that decoder plugs in without
/// `LiveFeedManager` needing to know it exists. The instrument_token->Uuid
/// mapping (the instrument master-data lookup) lives in whatever concrete
/// decoder is passed in, not here.
pub trait TickDecoder: Send {
    fn decode(&self, raw: &[u8]) -> Vec<PriceTick>;
}

#[derive(Debug, Clone)]
pub enum FeedEvent {
    Tick(PriceTick),
    MinuteBarCompleted(MinuteBar),
    Disconnected { attempt: u32 },
    Reconnected,
}

pub struct LiveFeedManager<T: TickTransport, D: TickDecoder> {
    transport: T,
    decoder: D,
    reconnect_policy: ReconnectPolicy,
    aggregator: MinuteBarAggregator,
    events: broadcast::Sender<FeedEvent>,
    /// Local cache fallback (SRS 2.2.2): last known price per instrument, so
    /// the dashboard has *something* to show immediately on reconnect/offline
    /// rather than a blank field (NFR "Offline support").
    last_known_price: HashMap<Uuid, Decimal>,
}

impl<T: TickTransport, D: TickDecoder> LiveFeedManager<T, D> {
    pub fn new(transport: T, decoder: D, reconnect_policy: ReconnectPolicy) -> Self {
        let (tx, _rx) = broadcast::channel(1024);
        Self {
            transport,
            decoder,
            reconnect_policy,
            aggregator: MinuteBarAggregator::new(),
            events: tx,
            last_known_price: HashMap::new(),
        }
    }

    pub fn subscribe_events(&self) -> broadcast::Receiver<FeedEvent> {
        self.events.subscribe()
    }

    pub fn last_known_price(&self, instrument_id: Uuid) -> Option<Decimal> {
        self.last_known_price.get(&instrument_id).copied()
    }

    /// One iteration of connect -> subscribe -> read-loop-until-drop. Split
    /// out from `run_forever` so the reconnect/backoff behavior is testable
    /// without an actual infinite loop or real sleeps (see tests below,
    /// which call this directly against a fake transport).
    pub async fn run_once(&mut self, instrument_tokens: &[u32]) -> Result<(), TransportError> {
        self.transport.connect().await?;
        self.transport.send_subscribe(instrument_tokens).await?;
        self.reconnect_policy.reset();
        let _ = self.events.send(FeedEvent::Reconnected);

        loop {
            let raw = self.transport.next_message().await?;
            for tick in self.decoder.decode(&raw) {
                self.last_known_price.insert(tick.instrument_id, tick.ltp);
                let _ = self.events.send(FeedEvent::Tick(tick.clone()));
                if let Some(completed) = self.aggregator.ingest(tick.instrument_id, tick.ltp, tick.timestamp) {
                    let _ = self.events.send(FeedEvent::MinuteBarCompleted(completed));
                }
            }
        }
    }

    /// The actual production loop: run_once until it errors, then back off
    /// and retry, forever. `sleep_fn` is injected so tests can run this with
    /// zero real delay while still exercising the retry-count/backoff logic.
    pub async fn run_forever<F, Fut>(&mut self, instrument_tokens: &[u32], sleep_fn: F)
    where
        F: Fn(std::time::Duration) -> Fut,
        Fut: std::future::Future<Output = ()>,
    {
        loop {
            if let Err(_e) = self.run_once(instrument_tokens).await {
                let attempt = self.reconnect_policy.attempt_count();
                let _ = self.events.send(FeedEvent::Disconnected { attempt });
                let delay = self.reconnect_policy.next_delay();
                sleep_fn(delay).await;
                // Cooperative yield: if connect/backoff ever resolve without
                // a real suspension point (as a fast-failing feed or a
                // zero-delay backoff can), this loop would otherwise never
                // hand control back to the runtime — starving its own
                // shutdown signal and any timers/timeouts a caller wraps
                // around this future. Cheap insurance against that class of
                // hang in production, not just in tests.
                tokio::task::yield_now().await;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use rust_decimal_macros::dec;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;
    use std::time::Duration;

    /// A transport that connects fine, then yields a fixed number of
    /// messages before erroring — simulates a broker session dropping mid-
    /// stream without touching a real socket.
    struct FakeTransport {
        messages: Vec<Vec<u8>>,
        next_idx: usize,
        connect_calls: Arc<AtomicUsize>,
    }
    #[async_trait]
    impl TickTransport for FakeTransport {
        async fn connect(&mut self) -> Result<(), TransportError> {
            self.connect_calls.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }
        async fn send_subscribe(&mut self, _tokens: &[u32]) -> Result<(), TransportError> {
            Ok(())
        }
        async fn next_message(&mut self) -> Result<Vec<u8>, TransportError> {
            if self.next_idx < self.messages.len() {
                let msg = self.messages[self.next_idx].clone();
                self.next_idx += 1;
                Ok(msg)
            } else {
                Err(TransportError::Closed)
            }
        }
    }

    /// Decodes each fake "message" as a single tick where the byte payload
    /// is just the price encoded as a decimal string — good enough to
    /// exercise the aggregation/broadcast path without real Kite framing.
    struct FakeDecoder {
        instrument_id: Uuid,
    }
    impl TickDecoder for FakeDecoder {
        fn decode(&self, raw: &[u8]) -> Vec<PriceTick> {
            use std::str::FromStr;
            let text = String::from_utf8_lossy(raw);
            let parts: Vec<&str> = text.split('@').collect(); // "price@epoch_seconds"
            let price = Decimal::from_str(parts[0]).unwrap();
            let ts = parts[1].parse::<i64>().unwrap();
            vec![PriceTick {
                instrument_id: self.instrument_id,
                ltp: price,
                timestamp: DateTime::from_timestamp(ts, 0).unwrap(),
            }]
        }
    }

    #[tokio::test]
    async fn run_once_updates_last_known_price_and_broadcasts_ticks() {
        let instrument_id = Uuid::new_v4();
        let transport = FakeTransport {
            messages: vec![b"100.5@1767250500".to_vec(), b"101.0@1767250501".to_vec()],
            next_idx: 0,
            connect_calls: Arc::new(AtomicUsize::new(0)),
        };
        let decoder = FakeDecoder { instrument_id };
        let mut manager = LiveFeedManager::new(transport, decoder, ReconnectPolicy::default_policy());
        let mut events = manager.subscribe_events();

        // run_once errors out once messages are exhausted (FakeTransport
        // returns Closed) — that's expected, we're testing what happened
        // before it closed.
        let _ = manager.run_once(&[123]).await;

        assert_eq!(manager.last_known_price(instrument_id), Some(dec!(101.0)));

        let mut tick_count = 0;
        while let Ok(event) = events.try_recv() {
            if matches!(event, FeedEvent::Tick(_)) {
                tick_count += 1;
            }
        }
        assert_eq!(tick_count, 2);
    }

    #[tokio::test]
    async fn run_forever_backs_off_and_retries_on_disconnect() {
        let instrument_id = Uuid::new_v4();
        let connect_calls = Arc::new(AtomicUsize::new(0));
        let transport = FakeTransport {
            messages: vec![b"100@1767250500".to_vec()],
            next_idx: 0,
            connect_calls: connect_calls.clone(),
        };
        let decoder = FakeDecoder { instrument_id };
        let mut manager = LiveFeedManager::new(transport, decoder, ReconnectPolicy::new(Duration::from_millis(1), Duration::from_millis(10)));

        let sleep_calls = Arc::new(AtomicUsize::new(0));
        let sleep_calls_clone = sleep_calls.clone();

        // Run with a fast, fake sleep for a bounded number of iterations by
        // racing against a timeout — run_forever never returns on its own
        // (that's the point: it's the production "keep retrying" loop).
        let run = manager.run_forever(&[123], move |_d| {
            sleep_calls_clone.fetch_add(1, Ordering::SeqCst);
            async {}
        });
        let _ = tokio::time::timeout(Duration::from_millis(50), run).await;

        // Every retry reconnects (FakeTransport's single message is
        // exhausted after the very first read each time), so both counters
        // climb together — confirms the reconnect loop actually retries
        // rather than giving up after the first disconnect.
        assert!(connect_calls.load(Ordering::SeqCst) > 1, "must reconnect more than once");
        assert!(sleep_calls.load(Ordering::SeqCst) > 1, "must back off between retries");
    }
}

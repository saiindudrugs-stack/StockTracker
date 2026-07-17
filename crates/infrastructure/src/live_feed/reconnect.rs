//! Pure reconnect/backoff calculator — no I/O, no sleeping, so it's cheap to
//! test exhaustively (SRS 2.2.2 "exponential-backoff retry").

use std::time::Duration;

#[derive(Debug, Clone)]
pub struct ReconnectPolicy {
    base: Duration,
    max: Duration,
    attempt: u32,
}

impl ReconnectPolicy {
    pub fn new(base: Duration, max: Duration) -> Self {
        Self { base, max, attempt: 0 }
    }

    /// Standard desktop-app defaults: start at 1s, cap at 60s. Reasonable
    /// for a broker WebSocket reconnect without hammering their servers.
    pub fn default_policy() -> Self {
        Self::new(Duration::from_secs(1), Duration::from_secs(60))
    }

    /// Returns the delay to wait before the *next* attempt, and advances
    /// internal state. Doubles each call, capped at `max`, with the doubling
    /// done in integer millis to avoid float drift over many reconnects.
    pub fn next_delay(&mut self) -> Duration {
        let millis = self.base.as_millis().saturating_mul(1u128 << self.attempt.min(20));
        let capped = millis.min(self.max.as_millis());
        self.attempt = self.attempt.saturating_add(1);
        Duration::from_millis(capped as u64)
    }

    /// Call on a successful reconnect — resets backoff so a brief blip
    /// doesn't leave the next real outage waiting a full minute for the
    /// first retry.
    pub fn reset(&mut self) {
        self.attempt = 0;
    }

    pub fn attempt_count(&self) -> u32 {
        self.attempt
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn doubles_each_attempt_up_to_the_cap() {
        let mut policy = ReconnectPolicy::new(Duration::from_secs(1), Duration::from_secs(60));
        assert_eq!(policy.next_delay(), Duration::from_secs(1));
        assert_eq!(policy.next_delay(), Duration::from_secs(2));
        assert_eq!(policy.next_delay(), Duration::from_secs(4));
        assert_eq!(policy.next_delay(), Duration::from_secs(8));
        assert_eq!(policy.next_delay(), Duration::from_secs(16));
        assert_eq!(policy.next_delay(), Duration::from_secs(32));
        assert_eq!(policy.next_delay(), Duration::from_secs(60)); // would be 64, capped
        assert_eq!(policy.next_delay(), Duration::from_secs(60)); // stays capped
    }

    #[test]
    fn reset_returns_to_base_delay() {
        let mut policy = ReconnectPolicy::new(Duration::from_secs(1), Duration::from_secs(60));
        policy.next_delay();
        policy.next_delay();
        assert_eq!(policy.attempt_count(), 2);
        policy.reset();
        assert_eq!(policy.attempt_count(), 0);
        assert_eq!(policy.next_delay(), Duration::from_secs(1));
    }
}

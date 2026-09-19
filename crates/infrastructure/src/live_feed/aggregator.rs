//! Buckets raw ticks into 1-minute OHLC bars. Pure data structure — no I/O —
//! so the confirmed "1-minute resolution, no tick storage" decision
//! (Section 11) is enforced right here: whatever cadence ticks arrive at,
//! only a completed minute bar ever leaves this struct.

use chrono::{DateTime, Timelike, Utc};
use rust_decimal::Decimal;
use std::collections::HashMap;
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq)]
pub struct MinuteBar {
    pub instrument_id: Uuid,
    pub minute_start: DateTime<Utc>,
    pub open: Decimal,
    pub high: Decimal,
    pub low: Decimal,
    pub close: Decimal,
    pub tick_count: u32,
}

struct InProgressBar {
    minute_start: DateTime<Utc>,
    open: Decimal,
    high: Decimal,
    low: Decimal,
    close: Decimal,
    tick_count: u32,
}

#[derive(Default)]
pub struct MinuteBarAggregator {
    in_progress: HashMap<Uuid, InProgressBar>,
}

fn truncate_to_minute(ts: DateTime<Utc>) -> DateTime<Utc> {
    ts.with_second(0).unwrap().with_nanosecond(0).unwrap()
}

impl MinuteBarAggregator {
    pub fn new() -> Self {
        Self::default()
    }

    /// Feeds one tick in. Returns `Some(MinuteBar)` if this tick's minute
    /// boundary rolled over from the previous tick's, in which case the
    /// *previous, now-complete* minute bar is returned and the new minute
    /// starts fresh with this tick as its opening print.
    pub fn ingest(&mut self, instrument_id: Uuid, price: Decimal, timestamp: DateTime<Utc>) -> Option<MinuteBar> {
        let minute = truncate_to_minute(timestamp);

        match self.in_progress.get_mut(&instrument_id) {
            None => {
                self.in_progress.insert(
                    instrument_id,
                    InProgressBar { minute_start: minute, open: price, high: price, low: price, close: price, tick_count: 1 },
                );
                None
            }
            Some(bar) if bar.minute_start == minute => {
                bar.high = bar.high.max(price);
                bar.low = bar.low.min(price);
                bar.close = price;
                bar.tick_count += 1;
                None
            }
            Some(bar) => {
                // Minute rolled over: snapshot the completed bar, then reset.
                let completed = MinuteBar {
                    instrument_id,
                    minute_start: bar.minute_start,
                    open: bar.open,
                    high: bar.high,
                    low: bar.low,
                    close: bar.close,
                    tick_count: bar.tick_count,
                };
                *bar = InProgressBar { minute_start: minute, open: price, high: price, low: price, close: price, tick_count: 1 };
                Some(completed)
            }
        }
    }

    /// Force-flush whatever's in progress (e.g. on graceful shutdown or
    /// market close) so the last partial minute isn't silently dropped.
    pub fn flush_all(&mut self) -> Vec<MinuteBar> {
        self.in_progress
            .drain()
            .map(|(instrument_id, bar)| MinuteBar {
                instrument_id,
                minute_start: bar.minute_start,
                open: bar.open,
                high: bar.high,
                low: bar.low,
                close: bar.close,
                tick_count: bar.tick_count,
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;
    use rust_decimal_macros::dec;

    fn ts(second: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 1, 1, 9, 15, second).unwrap()
    }
    fn ts_minute(minute: u32, second: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 1, 1, 9, minute, second).unwrap()
    }

    #[test]
    fn ticks_within_same_minute_dont_emit_a_bar() {
        let mut agg = MinuteBarAggregator::new();
        let id = Uuid::new_v4();
        assert!(agg.ingest(id, dec!(100), ts(0)).is_none());
        assert!(agg.ingest(id, dec!(101), ts(30)).is_none());
        assert!(agg.ingest(id, dec!(99), ts(59)).is_none());
    }

    #[test]
    fn minute_rollover_emits_correct_ohlc() {
        let mut agg = MinuteBarAggregator::new();
        let id = Uuid::new_v4();
        agg.ingest(id, dec!(100), ts_minute(15, 0));
        agg.ingest(id, dec!(105), ts_minute(15, 20));
        agg.ingest(id, dec!(98), ts_minute(15, 40));
        let completed = agg.ingest(id, dec!(102), ts_minute(16, 0)); // next minute

        let bar = completed.expect("minute rolled over, must emit");
        assert_eq!(bar.open, dec!(100));
        assert_eq!(bar.high, dec!(105));
        assert_eq!(bar.low, dec!(98));
        assert_eq!(bar.close, dec!(98)); // last tick *of the completed minute*
        assert_eq!(bar.tick_count, 3);
    }

    #[test]
    fn flush_all_returns_in_progress_bars_on_shutdown() {
        let mut agg = MinuteBarAggregator::new();
        let id = Uuid::new_v4();
        agg.ingest(id, dec!(100), ts(0));
        agg.ingest(id, dec!(103), ts(10));

        let flushed = agg.flush_all();
        assert_eq!(flushed.len(), 1);
        assert_eq!(flushed[0].close, dec!(103));
        assert_eq!(flushed[0].tick_count, 2);

        // After flush, state is cleared — a fresh tick starts a new bar.
        assert!(agg.ingest(id, dec!(110), ts(20)).is_none());
    }

    #[test]
    fn tracks_multiple_instruments_independently() {
        let mut agg = MinuteBarAggregator::new();
        let a = Uuid::new_v4();
        let b = Uuid::new_v4();
        agg.ingest(a, dec!(100), ts(0));
        agg.ingest(b, dec!(200), ts(0));
        let flushed = agg.flush_all();
        assert_eq!(flushed.len(), 2);
    }
}

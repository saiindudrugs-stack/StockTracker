//! Market phase classification — Markup / Markdown / Accumulation /
//! Distribution, using SMA-50, SMA-200, and On-Balance Volume (OBV). This
//! mirrors the exact logic the user supplied as a Python reference
//! (SMA/OBV-based Wyckoff-style phase heuristic), reimplemented here so it
//! runs on data already flowing through this app rather than a separate
//! script.
//!
//! Like xirr.rs, this is a statistical/numerical classification, not exact
//! ledger math — SMA and OBV are computed in f64, not Decimal. The Decimal
//! discipline elsewhere in this codebase is specifically for money that
//! must reconcile to the paisa; a 50-day moving average has no such
//! requirement, and the reference implementation itself uses floating
//! point (pandas/numpy).
//!
//! HONESTY NOTE: this heuristic is exactly as fuzzy as the Python it was
//! translated from — "sideways, OBV rising -> Accumulation, else
//! Distribution" is a simplification the reference code's own comments
//! call a "simple heuristic," not a rigorous technical-analysis method.
//! Treat the output as a rough directional read, not a signal to act on
//! mechanically.

use chrono::NaiveDate;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MarketPhase {
    Markup,
    Markdown,
    Accumulation,
    Distribution,
    /// Not enough history to compute SMA-200 yet (needs 200+ daily bars).
    InsufficientData,
}

impl MarketPhase {
    pub fn label(&self) -> &'static str {
        match self {
            MarketPhase::Markup => "Markup",
            MarketPhase::Markdown => "Markdown",
            MarketPhase::Accumulation => "Accumulation",
            MarketPhase::Distribution => "Distribution",
            MarketPhase::InsufficientData => "Insufficient data",
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct DailyBar {
    pub date: NaiveDate,
    pub close: f64,
    pub volume: f64,
}

fn simple_moving_average(values: &[f64], window: usize) -> Option<f64> {
    if values.len() < window {
        return None;
    }
    let slice = &values[values.len() - window..];
    Some(slice.iter().sum::<f64>() / window as f64)
}

/// On-Balance Volume: running total of volume, added when price rises,
/// subtracted when it falls — a rough proxy for whether volume is backing
/// an uptrend or a downtrend. Matches the reference Python's
/// `np.sign(close.diff()) * volume, cumsum()`.
fn on_balance_volume(bars: &[DailyBar]) -> Vec<f64> {
    let mut obv = Vec::with_capacity(bars.len());
    let mut running = 0.0;
    for i in 0..bars.len() {
        if i == 0 {
            obv.push(running);
            continue;
        }
        let diff = bars[i].close - bars[i - 1].close;
        running += diff.signum() * bars[i].volume;
        obv.push(running);
    }
    obv
}

/// Classifies the most recent phase from up to a year of daily bars,
/// oldest-first. Needs at least 200 bars for SMA-200; returns
/// `InsufficientData` otherwise rather than guessing from a shorter window.
pub fn classify_market_phase(bars: &[DailyBar]) -> MarketPhase {
    if bars.len() < 200 {
        return MarketPhase::InsufficientData;
    }

    let closes: Vec<f64> = bars.iter().map(|b| b.close).collect();
    let sma_50 = simple_moving_average(&closes, 50).expect("checked len >= 200 above");
    let sma_200 = simple_moving_average(&closes, 200).expect("checked len >= 200 above");
    let latest_close = *closes.last().expect("bars is non-empty, checked len >= 200");

    if latest_close > sma_50 && sma_50 > sma_200 {
        return MarketPhase::Markup;
    }
    if latest_close < sma_50 && sma_50 < sma_200 {
        return MarketPhase::Markdown;
    }

    // Neither a clean uptrend nor downtrend by the SMA test — use OBV
    // direction over the trailing 20 bars to decide Accumulation vs.
    // Distribution, exactly as the reference Python does.
    let obv = on_balance_volume(bars);
    let window = 20.min(obv.len());
    let recent_obv = &obv[obv.len() - window..];
    if recent_obv.last().unwrap_or(&0.0) > recent_obv.first().unwrap_or(&0.0) {
        MarketPhase::Accumulation
    } else {
        MarketPhase::Distribution
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bar(day: i64, close: f64, volume: f64) -> DailyBar {
        DailyBar {
            date: NaiveDate::from_ymd_opt(2025, 1, 1).unwrap() + chrono::Duration::days(day),
            close,
            volume,
        }
    }

    #[test]
    fn fewer_than_200_bars_is_insufficient_data() {
        let bars: Vec<DailyBar> = (0..150).map(|i| bar(i, 100.0, 1000.0)).collect();
        assert_eq!(classify_market_phase(&bars), MarketPhase::InsufficientData);
    }

    #[test]
    fn steady_uptrend_classifies_as_markup() {
        // Price climbs steadily for 250 days: latest > SMA50 > SMA200 by construction.
        let bars: Vec<DailyBar> = (0..250)
            .map(|i| bar(i, 100.0 + i as f64 * 0.5, 1000.0))
            .collect();
        assert_eq!(classify_market_phase(&bars), MarketPhase::Markup);
    }

    #[test]
    fn steady_downtrend_classifies_as_markdown() {
        let bars: Vec<DailyBar> = (0..250)
            .map(|i| bar(i, 300.0 - i as f64 * 0.5, 1000.0))
            .collect();
        assert_eq!(classify_market_phase(&bars), MarketPhase::Markdown);
    }

    #[test]
    fn flat_price_with_rising_obv_classifies_as_accumulation() {
        // Flat close (SMA50 ~= SMA200 ~= price, so neither trend test
        // fires), but volume skews heavily positive in the last 20 bars —
        // OBV should rise, giving Accumulation.
        let mut bars: Vec<DailyBar> = (0..230).map(|i| bar(i, 100.0, 1000.0)).collect();
        // Alternate up/down ticks in the tail so OBV has signal, weighted
        // toward up-days having far more volume than down-days.
        for i in 210..230 {
            let close = if i % 2 == 0 { 100.5 } else { 99.5 };
            let volume = if i % 2 == 0 { 5000.0 } else { 500.0 };
            bars[i] = bar(i as i64, close, volume);
        }
        assert_eq!(classify_market_phase(&bars), MarketPhase::Accumulation);
    }

    #[test]
    fn flat_price_with_falling_obv_classifies_as_distribution() {
        let mut bars: Vec<DailyBar> = (0..230).map(|i| bar(i, 100.0, 1000.0)).collect();
        for i in 210..230 {
            let close = if i % 2 == 0 { 99.5 } else { 100.5 };
            let volume = if i % 2 == 0 { 5000.0 } else { 500.0 };
            bars[i] = bar(i as i64, close, volume);
        }
        assert_eq!(classify_market_phase(&bars), MarketPhase::Distribution);
    }
}

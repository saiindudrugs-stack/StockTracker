//! Statistical stock/portfolio analysis — daily returns, annualized
//! return/volatility, Pearson correlation between stocks, historical
//! Value-at-Risk, moving-average series for chart overlays, and RSI.
//!
//! Directly modeled on the techniques in "Python for stock analysis" (Rohan
//! Kumar, Analytics Vidhya) that the user referenced: daily return via
//! pct_change, risk (volatility) vs. return per stock, a correlation matrix
//! across held stocks, and historical VaR via the empirical-quantile
//! ("Bootstrap") method. Trading-days-per-year is hardcoded to 252 (NSE's
//! actual annual trading day count is close enough to the standard
//! assumption that a separate constant isn't worth the complexity).
//!
//! DELIBERATELY NOT INCLUDED: the same article also covers price
//! *prediction* (Prophet/ARIMA/LSTM). Its own disclaimer says those "still
//! cannot be used to place bets in the real market" — agreed, and that's
//! why forecasting isn't part of this module. Descriptive statistics about
//! the past are a very different, much more defensible thing than
//! predicting the future.
//!
//! As with xirr.rs and market_phase.rs, this is numerical/statistical work
//! in f64, not the ledger's exact Decimal money arithmetic — consistent
//! with how the rest of this codebase draws that line.

const TRADING_DAYS_PER_YEAR: f64 = 252.0;

/// Day-over-day percentage change, mirroring pandas' `pct_change()`. First
/// element of the input has no prior day, so the output is one element
/// shorter than the input.
pub fn daily_returns(closes: &[f64]) -> Vec<f64> {
    closes.windows(2).map(|w| (w[1] - w[0]) / w[0]).collect()
}

pub fn mean(values: &[f64]) -> f64 {
    if values.is_empty() {
        return 0.0;
    }
    values.iter().sum::<f64>() / values.len() as f64
}

/// Sample standard deviation (n-1 denominator) — matches pandas' default
/// `.std()`, which is what the reference article uses.
pub fn std_dev(values: &[f64]) -> f64 {
    if values.len() < 2 {
        return 0.0;
    }
    let m = mean(values);
    let variance = values.iter().map(|v| (v - m).powi(2)).sum::<f64>() / (values.len() - 1) as f64;
    variance.sqrt()
}

pub fn annualized_return(returns: &[f64]) -> f64 {
    mean(returns) * TRADING_DAYS_PER_YEAR
}

pub fn annualized_volatility(returns: &[f64]) -> f64 {
    std_dev(returns) * TRADING_DAYS_PER_YEAR.sqrt()
}

/// Pearson correlation coefficient between two equal-length return series —
/// the same statistic `df_pivot.corr(method='pearson')` computes in the
/// reference article's correlation-matrix section. Returns `None` for
/// mismatched lengths, fewer than 2 points, or a zero-variance series
/// (correlation is undefined against a constant series).
pub fn pearson_correlation(a: &[f64], b: &[f64]) -> Option<f64> {
    if a.len() != b.len() || a.len() < 2 {
        return None;
    }
    let mean_a = mean(a);
    let mean_b = mean(b);
    let mut cov = 0.0;
    let mut var_a = 0.0;
    let mut var_b = 0.0;
    for i in 0..a.len() {
        let da = a[i] - mean_a;
        let db = b[i] - mean_b;
        cov += da * db;
        var_a += da * da;
        var_b += db * db;
    }
    // Epsilon rather than exact 0.0: floating-point subtraction of nearly-
    // equal values (e.g. a "constant" series like [0.05, 0.05, 0.05]) can
    // leave var_a/var_b at something like 9e-17 instead of a clean zero —
    // caught by this module's own test suite, not a hypothetical.
    if var_a < 1e-12 || var_b < 1e-12 {
        return None;
    }
    Some(cov / (var_a.sqrt() * var_b.sqrt()))
}

/// Historical Value-at-Risk via the empirical-quantile ("Bootstrap")
/// method the article uses: sort observed daily returns and read off the
/// value at the given tail probability. `confidence` of 0.95 means "95%
/// confident daily loss won't exceed this" — internally that reads the 5th
/// percentile (0.05) of the return distribution. Returned as a negative
/// fraction (e.g. -0.045 = a 4.5% loss), matching the article's own sign
/// convention rather than flipping it to a positive "loss amount".
pub fn historical_var(returns: &[f64], confidence: f64) -> Option<f64> {
    if returns.is_empty() || !(0.0..1.0).contains(&confidence) {
        return None;
    }
    let mut sorted: Vec<f64> = returns.to_vec();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let tail = 1.0 - confidence;
    let idx = ((sorted.len() as f64) * tail).floor() as usize;
    let idx = idx.min(sorted.len() - 1);
    Some(sorted[idx])
}

/// Simple moving average as a full aligned series (unlike
/// `market_phase`'s single-latest-value version) — for overlaying on a
/// price chart, where every point needs either a value or an explicit gap
/// before the window fills.
pub fn sma_series(closes: &[f64], window: usize) -> Vec<Option<f64>> {
    if window == 0 {
        return vec![None; closes.len()];
    }
    (0..closes.len())
        .map(|i| {
            if i + 1 < window {
                None
            } else {
                Some(closes[i + 1 - window..=i].iter().sum::<f64>() / window as f64)
            }
        })
        .collect()
}

/// Wilder's RSI (the standard formulation — smoothed average gain/loss,
/// not a plain rolling mean), the classic momentum oscillator from the
/// article's "Momentum" indicator list. Returns `None` for the first
/// `period` points where there isn't enough history yet.
pub fn rsi(closes: &[f64], period: usize) -> Vec<Option<f64>> {
    if period == 0 || closes.len() <= period {
        return vec![None; closes.len()];
    }
    let mut result = vec![None; closes.len()];
    let changes: Vec<f64> = closes.windows(2).map(|w| w[1] - w[0]).collect();

    let mut avg_gain = changes[..period].iter().filter(|c| **c > 0.0).sum::<f64>() / period as f64;
    let mut avg_loss = changes[..period].iter().filter(|c| **c < 0.0).map(|c| -c).sum::<f64>() / period as f64;

    let rsi_at = |avg_gain: f64, avg_loss: f64| -> f64 {
        if avg_loss == 0.0 {
            return 100.0;
        }
        let rs = avg_gain / avg_loss;
        100.0 - (100.0 / (1.0 + rs))
    };
    result[period] = Some(rsi_at(avg_gain, avg_loss));

    for i in period..changes.len() {
        let change = changes[i];
        let gain = change.max(0.0);
        let loss = (-change).max(0.0);
        avg_gain = (avg_gain * (period as f64 - 1.0) + gain) / period as f64;
        avg_loss = (avg_loss * (period as f64 - 1.0) + loss) / period as f64;
        result[i + 1] = Some(rsi_at(avg_gain, avg_loss));
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    fn approx_eq(a: f64, b: f64, tol: f64) -> bool {
        (a - b).abs() < tol
    }

    #[test]
    fn daily_returns_matches_pct_change() {
        let closes = vec![100.0, 110.0, 99.0];
        let returns = daily_returns(&closes);
        assert_eq!(returns.len(), 2);
        assert!(approx_eq(returns[0], 0.10, 1e-9)); // (110-100)/100
        assert!(approx_eq(returns[1], -0.10, 1e-9)); // (99-110)/110 = -0.1
    }

    #[test]
    fn std_dev_of_constant_series_is_zero() {
        assert_eq!(std_dev(&[100.0, 100.0, 100.0]), 0.0);
    }

    #[test]
    fn perfectly_correlated_series_gives_correlation_of_one() {
        let a = vec![0.01, 0.02, -0.01, 0.03, -0.02];
        let b: Vec<f64> = a.iter().map(|x| x * 2.0).collect(); // scaled copy, same direction
        let corr = pearson_correlation(&a, &b).unwrap();
        assert!(approx_eq(corr, 1.0, 1e-9), "corr was {corr}");
    }

    #[test]
    fn perfectly_inverse_series_gives_correlation_of_negative_one() {
        let a = vec![0.01, 0.02, -0.01, 0.03, -0.02];
        let b: Vec<f64> = a.iter().map(|x| -x).collect();
        let corr = pearson_correlation(&a, &b).unwrap();
        assert!(approx_eq(corr, -1.0, 1e-9), "corr was {corr}");
    }

    #[test]
    fn zero_variance_series_has_no_defined_correlation() {
        let a = vec![0.01, 0.02, -0.01];
        let constant = vec![0.05, 0.05, 0.05];
        assert_eq!(pearson_correlation(&a, &constant), None);
    }

    #[test]
    fn historical_var_reads_the_correct_tail_quantile() {
        // 20 returns, evenly spaced from -0.10 to +0.09 in steps of 0.01.
        let returns: Vec<f64> = (0..20).map(|i| -0.10 + i as f64 * 0.01).collect();
        // 95% confidence -> 5th percentile -> index floor(20*0.05)=1 -> second-worst value.
        let var95 = historical_var(&returns, 0.95).unwrap();
        assert!(approx_eq(var95, -0.09, 1e-9), "var95 was {var95}");
    }

    #[test]
    fn sma_series_has_gaps_before_window_fills_then_matches_manual_average() {
        let closes = vec![10.0, 20.0, 30.0, 40.0, 50.0];
        let sma3 = sma_series(&closes, 3);
        assert_eq!(sma3[0], None);
        assert_eq!(sma3[1], None);
        assert!(approx_eq(sma3[2].unwrap(), 20.0, 1e-9)); // (10+20+30)/3
        assert!(approx_eq(sma3[3].unwrap(), 30.0, 1e-9)); // (20+30+40)/3
        assert!(approx_eq(sma3[4].unwrap(), 40.0, 1e-9)); // (30+40+50)/3
    }

    #[test]
    fn rsi_is_100_when_every_change_in_the_window_is_a_gain() {
        // Strictly increasing series: average loss is 0, so RSI must be 100
        // (the rsi_at guard for avg_loss == 0.0 is exactly for this case).
        let closes: Vec<f64> = (0..20).map(|i| 100.0 + i as f64).collect();
        let result = rsi(&closes, 14);
        assert_eq!(result[14], Some(100.0));
    }

    #[test]
    fn rsi_is_0_when_every_change_in_the_window_is_a_loss() {
        let closes: Vec<f64> = (0..20).map(|i| 100.0 - i as f64).collect();
        let result = rsi(&closes, 14);
        assert_eq!(result[14], Some(0.0));
    }

    #[test]
    fn rsi_has_no_value_before_the_period_fills() {
        let closes = vec![100.0, 101.0, 102.0];
        let result = rsi(&closes, 14);
        assert!(result.iter().all(|v| v.is_none()));
    }
}

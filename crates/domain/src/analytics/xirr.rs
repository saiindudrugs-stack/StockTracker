//! XIRR — the annualized internal rate of return for irregularly-timed
//! cashflows. Required by SRS 2.2.3 (dashboards) and 2.2.3 (MF-specific SIP
//! XIRR). Solved numerically since there's no closed form.
//!
//! Note: the rate-solving itself uses f64 (Newton-Raphson on a transcendental
//! equation needs a float), NOT rust_decimal — Decimal is for the ledger's
//! exact money arithmetic (Section value_objects.rs), this is a separate,
//! intentionally-approximate numerical method. The Money/cashflow *inputs*
//! come from exact Decimal ledger amounts; only the solver step is float.

use crate::errors::DomainError;
use chrono::NaiveDate;

#[derive(Debug, Clone, Copy)]
pub struct Cashflow {
    pub date: NaiveDate,
    /// Negative = money out (a Buy/SIP installment), positive = money in
    /// (a Sell, Dividend, or the final mark-to-market value).
    pub amount: f64,
}

const MAX_ITERATIONS: u32 = 100;
const TOLERANCE: f64 = 1e-7;

/// Computes XIRR given a list of dated cashflows (must contain at least one
/// negative and one positive value, per finance convention). Returns the
/// annualized rate as a decimal fraction (e.g. 0.145 = 14.5%).
pub fn compute_xirr(cashflows: &[Cashflow]) -> Result<f64, DomainError> {
    if cashflows.is_empty() {
        return Err(DomainError::XirrInsufficientCashflows);
    }
    let has_negative = cashflows.iter().any(|c| c.amount < 0.0);
    let has_positive = cashflows.iter().any(|c| c.amount > 0.0);
    if !has_negative || !has_positive {
        return Err(DomainError::XirrInsufficientCashflows);
    }

    let t0 = cashflows.iter().map(|c| c.date).min().unwrap();
    let years_from_t0 = |d: NaiveDate| -> f64 { (d - t0).num_days() as f64 / 365.0 };

    let npv = |rate: f64| -> f64 {
        cashflows
            .iter()
            .map(|c| c.amount / (1.0 + rate).powf(years_from_t0(c.date)))
            .sum()
    };
    let npv_derivative = |rate: f64| -> f64 {
        cashflows
            .iter()
            .map(|c| {
                let t = years_from_t0(c.date);
                -t * c.amount / (1.0 + rate).powf(t + 1.0)
            })
            .sum()
    };

    // Newton-Raphson, starting guess 10% — a reasonable prior for equity/MF returns.
    let mut rate = 0.10_f64;
    for _ in 0..MAX_ITERATIONS {
        let f = npv(rate);
        if f.abs() < TOLERANCE {
            return Ok(rate);
        }
        let df = npv_derivative(rate);
        if df.abs() < 1e-12 {
            break; // derivative vanished; fall through to bisection
        }
        let next_rate = rate - f / df;
        // Guard against Newton's method wandering into an undefined region
        // (rate <= -1 makes (1+rate)^t blow up/undefined for non-integer t).
        if next_rate <= -0.999999 || !next_rate.is_finite() {
            break;
        }
        if (next_rate - rate).abs() < TOLERANCE {
            return Ok(next_rate);
        }
        rate = next_rate;
    }

    // Fallback: bisection over a wide bracket, robust even when Newton
    // diverges (common with SIP-style many-small-cashflow series).
    bisection(&npv, -0.9999, 10.0).ok_or(DomainError::XirrDidNotConverge(MAX_ITERATIONS))
}

fn bisection(f: &dyn Fn(f64) -> f64, mut lo: f64, mut hi: f64) -> Option<f64> {
    let mut f_lo = f(lo);
    let f_hi = f(hi);
    if f_lo.signum() == f_hi.signum() {
        return None; // no sign change in bracket, can't bisect
    }
    for _ in 0..200 {
        let mid = (lo + hi) / 2.0;
        let f_mid = f(mid);
        if f_mid.abs() < TOLERANCE || (hi - lo).abs() < 1e-9 {
            return Some(mid);
        }
        if f_mid.signum() == f_lo.signum() {
            lo = mid;
            f_lo = f_mid;
        } else {
            hi = mid;
        }
    }
    Some((lo + hi) / 2.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn single_buy_and_sell_matches_simple_return() {
        // Invest 100,000 on day 0, receive 110,000 exactly 365 days later
        // -> should converge very close to 10%.
        let d0 = NaiveDate::from_ymd_opt(2025, 1, 1).unwrap();
        let d1 = NaiveDate::from_ymd_opt(2026, 1, 1).unwrap();
        let flows = vec![
            Cashflow { date: d0, amount: -100_000.0 },
            Cashflow { date: d1, amount: 110_000.0 },
        ];
        let rate = compute_xirr(&flows).unwrap();
        assert!((rate - 0.10).abs() < 0.001, "rate was {rate}");
    }

    #[test]
    fn sip_style_multiple_cashflows_converges() {
        let flows = vec![
            Cashflow { date: NaiveDate::from_ymd_opt(2024, 1, 1).unwrap(), amount: -10_000.0 },
            Cashflow { date: NaiveDate::from_ymd_opt(2024, 4, 1).unwrap(), amount: -10_000.0 },
            Cashflow { date: NaiveDate::from_ymd_opt(2024, 7, 1).unwrap(), amount: -10_000.0 },
            Cashflow { date: NaiveDate::from_ymd_opt(2024, 10, 1).unwrap(), amount: -10_000.0 },
            Cashflow { date: NaiveDate::from_ymd_opt(2026, 1, 1).unwrap(), amount: 48_000.0 },
        ];
        let rate = compute_xirr(&flows).unwrap();
        assert!(rate.is_finite());
        assert!(rate > 0.0 && rate < 1.0, "rate was {rate}");
    }

    #[test]
    fn rejects_all_negative_cashflows() {
        let flows = vec![Cashflow {
            date: NaiveDate::from_ymd_opt(2025, 1, 1).unwrap(),
            amount: -100.0,
        }];
        assert!(compute_xirr(&flows).is_err());
    }
}

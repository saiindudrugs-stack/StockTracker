//! Core entities. Per SRS 2.2.1: the transaction ledger is the single source
//! of truth; Holding is always *derived*, never hand-edited. This module
//! encodes that rule structurally — Holding::apply is the only way a Holding
//! changes, and it only accepts a Transaction.

use crate::errors::DomainError;
use crate::value_objects::{Isin, Money};
use chrono::{DateTime, NaiveDate, Utc};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AssetClass {
    Equity,
    MutualFund,
    Etf,
    SovereignGoldBond,
    Bond,
    ReitInvit,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Instrument {
    pub id: Uuid,
    pub isin: Isin,
    pub symbol: String,
    pub asset_class: AssetClass,
    pub exchange: String,
    pub sector: Option<String>,
}

/// A transaction type. Bonus/Split carry a ratio instead of a price.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum TransactionType {
    Buy,
    Sell,
    Bonus,
    Split,
    Dividend,
    SipInstallment,
}

/// Append-only ledger entry — the single source of truth per SRS 2.2.1.
/// Once recorded, a Transaction is never mutated; corrections are made by
/// recording an offsetting transaction, preserving full audit history
/// (NFR "Auditability").
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Transaction {
    pub id: Uuid,
    pub portfolio_id: Uuid,
    pub instrument_id: Uuid,
    pub transaction_type: TransactionType,
    pub quantity: Decimal,
    pub price: Money,
    pub fees: Money,
    pub trade_date: NaiveDate,
    pub broker_ref: Option<String>,
    pub recorded_at: DateTime<Utc>,
}

impl Transaction {
    /// Total cash impact of this transaction: negative for a Buy (cash out),
    /// positive for a Sell or Dividend (cash in). Used by the Tax Engine and
    /// by XIRR cashflow construction (Section 2.2.6, 2.2.3).
    pub fn cash_impact(&self) -> Decimal {
        let gross = self.quantity * self.price.amount();
        match self.transaction_type {
            TransactionType::Buy | TransactionType::SipInstallment => {
                -(gross + self.fees.amount())
            }
            TransactionType::Sell | TransactionType::Dividend => gross - self.fees.amount(),
            TransactionType::Bonus | TransactionType::Split => Decimal::ZERO,
        }
    }
}

/// Derived, rebuildable-from-transactions holding snapshot (SRS 2.2.1: "all
/// holdings and analytics are derived, never hand-edited").
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Holding {
    pub portfolio_id: Uuid,
    pub instrument_id: Uuid,
    pub quantity: Decimal,
    pub avg_cost: Decimal,
    pub realized_pnl: Decimal,
}

impl Holding {
    pub fn empty(portfolio_id: Uuid, instrument_id: Uuid) -> Self {
        Self {
            portfolio_id,
            instrument_id,
            quantity: Decimal::ZERO,
            avg_cost: Decimal::ZERO,
            realized_pnl: Decimal::ZERO,
        }
    }

    /// Fold one transaction into the running holding. This is the *only*
    /// mutator — call it in trade_date order over the full ledger to rebuild
    /// a holding from scratch (matches "rebuildable from transaction table"
    /// in the HLD database schema, Section 5.1).
    pub fn apply(&mut self, txn: &Transaction) -> Result<(), DomainError> {
        if txn.instrument_id != self.instrument_id {
            return Ok(()); // not this holding's transaction; caller routes by instrument_id
        }
        match txn.transaction_type {
            TransactionType::Buy | TransactionType::SipInstallment => {
                if txn.quantity <= Decimal::ZERO {
                    return Err(DomainError::InvalidQuantity("buy"));
                }
                let total_cost_before = self.avg_cost * self.quantity;
                let incoming_cost = txn.quantity * txn.price.amount() + txn.fees.amount();
                self.quantity += txn.quantity;
                // round_dp(2): INR's smallest subunit is the paisa (2 decimal
                // places) — an average cost carried to 20+ digits isn't more
                // "correct", it's floating-point-style noise from a division
                // that doesn't terminate cleanly in base 10, and it leaks
                // into every downstream display (dashboard, holdings table).
                self.avg_cost = ((total_cost_before + incoming_cost) / self.quantity).round_dp(2);
            }
            TransactionType::Sell => {
                if txn.quantity <= Decimal::ZERO {
                    return Err(DomainError::InvalidQuantity("sell"));
                }
                if txn.quantity > self.quantity {
                    return Err(DomainError::InsufficientHolding {
                        instrument: self.instrument_id.to_string(),
                        available: self.quantity,
                        requested: txn.quantity,
                    });
                }
                let realized = (txn.price.amount() - self.avg_cost) * txn.quantity
                    - txn.fees.amount();
                self.realized_pnl += realized;
                self.quantity -= txn.quantity;
                // avg_cost is unchanged by a partial sell (FIFO-at-cost-basis
                // simplification for v1; per-lot FIFO for tax purposes lives
                // in the separate tax_lot table / Tax Engine, Section 5.1).
                if self.quantity.is_zero() {
                    self.avg_cost = Decimal::ZERO;
                }
            }
            TransactionType::Bonus => {
                // Bonus shares: quantity increases, avg_cost dilutes, total
                // cost basis unchanged.
                let total_cost = self.avg_cost * self.quantity;
                self.quantity += txn.quantity;
                if !self.quantity.is_zero() {
                    self.avg_cost = (total_cost / self.quantity).round_dp(2);
                }
            }
            TransactionType::Split => {
                // txn.quantity carries the *new total* quantity after split
                // by convention; avg_cost rescales so total cost is unchanged.
                if txn.quantity <= Decimal::ZERO {
                    return Err(DomainError::InvalidQuantity("split"));
                }
                let total_cost = self.avg_cost * self.quantity;
                self.quantity = txn.quantity;
                self.avg_cost = (total_cost / self.quantity).round_dp(2);
            }
            TransactionType::Dividend => {
                // Dividends don't change quantity/cost; they're cash income,
                // handled via cash_impact() for XIRR/cashflow purposes only.
            }
        }
        Ok(())
    }

    pub fn market_value(&self, ltp: Decimal) -> Decimal {
        self.quantity * ltp
    }

    pub fn unrealized_pnl(&self, ltp: Decimal) -> Decimal {
        (ltp - self.avg_cost) * self.quantity
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Portfolio {
    pub id: Uuid,
    pub name: String,
    pub base_currency: crate::value_objects::Currency,
    pub goal_tag: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AlertCondition {
    /// Fires when price falls to or below the threshold.
    StopLoss,
    /// Fires when price rises to or above the threshold.
    Target,
}

/// A user-set stop-loss or target price watch on one instrument within one
/// portfolio (HLD Section 5.1 `alert_rule` table, finally implemented).
/// `triggered` is a one-way flag set once the condition is observed true —
/// it stays set (rather than re-evaluating every check) so a price that
/// dips below a stop-loss and bounces back doesn't silently un-alert you;
/// dismissing/deleting the rule is an explicit user action.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AlertRule {
    pub id: Uuid,
    pub portfolio_id: Uuid,
    pub instrument_id: Uuid,
    pub condition: AlertCondition,
    pub threshold_price: rust_decimal::Decimal,
    pub triggered: bool,
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    fn buy(instrument_id: Uuid, qty: Decimal, price: Decimal, date: NaiveDate) -> Transaction {
        Transaction {
            id: Uuid::new_v4(),
            portfolio_id: Uuid::new_v4(),
            instrument_id,
            transaction_type: TransactionType::Buy,
            quantity: qty,
            price: Money::inr(price),
            fees: Money::inr(dec!(20)),
            trade_date: date,
            broker_ref: None,
            recorded_at: Utc::now(),
        }
    }

    #[test]
    fn buy_then_buy_averages_cost() {
        let instrument_id = Uuid::new_v4();
        let mut h = Holding::empty(Uuid::new_v4(), instrument_id);
        let d = NaiveDate::from_ymd_opt(2026, 1, 1).unwrap();

        h.apply(&buy(instrument_id, dec!(10), dec!(100), d)).unwrap();
        assert_eq!(h.quantity, dec!(10));
        assert_eq!(h.avg_cost, dec!(102)); // (10*100 + 20)/10

        h.apply(&buy(instrument_id, dec!(10), dec!(110), d)).unwrap();
        // total cost = 1020 + (1100+20) = 2140, qty = 20 -> avg 107
        assert_eq!(h.quantity, dec!(20));
        assert_eq!(h.avg_cost, dec!(107));
    }

    /// Regression test for a real bug caught in the running desktop app:
    /// dividing a clean total cost by a quantity that doesn't produce a
    /// terminating decimal (e.g. .../85) yields a 20+ digit repeating
    /// decimal from rust_decimal's exact division. That's not "more
    /// precise" — INR has no subunit smaller than the paisa — and it leaked
    /// into every downstream display: the holdings table, the dashboard's
    /// aggregated P/L, everywhere avg_cost or anything derived from it
    /// showed up. avg_cost must always come out at 2 decimal places.
    #[test]
    fn avg_cost_never_carries_more_precision_than_a_paisa() {
        let instrument_id = Uuid::new_v4();
        let mut h = Holding::empty(Uuid::new_v4(), instrument_id);
        let d = NaiveDate::from_ymd_opt(2026, 1, 1).unwrap();

        // Mirrors the exact shape that produced the bug: enough buys that
        // total_cost / quantity doesn't terminate in base 10.
        h.apply(&buy(instrument_id, dec!(10), dec!(2450.50), d)).unwrap();
        h.apply(&buy(instrument_id, dec!(75), dec!(2500.00), d)).unwrap();

        assert_eq!(h.quantity, dec!(85));
        assert_eq!(
            h.avg_cost.scale(),
            2,
            "avg_cost must be stored at 2 decimal places, not rust_decimal's raw division result"
        );

        // The bug was visible through unrealized_pnl too — confirm it's
        // clean all the way through, not just at the avg_cost field itself.
        let ltp = dec!(2510.00);
        assert_eq!(h.unrealized_pnl(ltp).scale(), 2);
    }

    #[test]
    fn sell_more_than_held_is_rejected() {
        let instrument_id = Uuid::new_v4();
        let mut h = Holding::empty(Uuid::new_v4(), instrument_id);
        let d = NaiveDate::from_ymd_opt(2026, 1, 1).unwrap();
        h.apply(&buy(instrument_id, dec!(5), dec!(100), d)).unwrap();

        let sell = Transaction {
            transaction_type: TransactionType::Sell,
            quantity: dec!(10),
            ..buy(instrument_id, dec!(10), dec!(100), d)
        };
        let err = h.apply(&sell).unwrap_err();
        assert!(matches!(err, DomainError::InsufficientHolding { .. }));
    }

    #[test]
    fn sell_realizes_pnl_at_avg_cost() {
        let instrument_id = Uuid::new_v4();
        let mut h = Holding::empty(Uuid::new_v4(), instrument_id);
        let d = NaiveDate::from_ymd_opt(2026, 1, 1).unwrap();
        h.apply(&buy(instrument_id, dec!(10), dec!(100), d)).unwrap(); // avg_cost 102

        let sell = Transaction {
            transaction_type: TransactionType::Sell,
            quantity: dec!(4),
            price: Money::inr(dec!(150)),
            fees: Money::inr(dec!(10)),
            ..buy(instrument_id, dec!(4), dec!(150), d)
        };
        h.apply(&sell).unwrap();
        // realized = (150 - 102) * 4 - 10 = 192 - 10 = 182
        assert_eq!(h.realized_pnl, dec!(182));
        assert_eq!(h.quantity, dec!(6));
    }
}

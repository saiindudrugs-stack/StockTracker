//! FIFO tax-lot matching, computed on demand from the immutable
//! transaction ledger — deliberately NOT a stored table. The domain
//! model's Holding aggregate uses a simplified average-cost-basis (see
//! the comment on Holding::apply's Sell branch in entities.rs), which is
//! correct for day-to-day P&L display but can't answer "was this specific
//! gain short-term or long-term," since India's equity tax rule (12-month
//! threshold) applies per purchase lot, not to the position as a whole.
//! This module replays the ledger in date order and matches each Sell
//! against the OLDEST remaining lots first (FIFO) to answer that
//! correctly, without needing any new persisted state — it's a report,
//! not an aggregate.
//!
//! India-specific: 12-month long-term threshold used uniformly for both
//! direct equity and equity mutual funds (both get equity tax treatment
//! at 12 months, unlike debt funds at 36 months — this app's mutual fund
//! support is AMFI equity-scheme-oriented, so 12 months is correct for
//! its current scope, not a shortcut).

use crate::entities::{Transaction, TransactionType};
use chrono::NaiveDate;
use rust_decimal::Decimal;

const LTCG_THRESHOLD_DAYS: i64 = 365;

#[derive(Debug, Clone)]
struct OpenLot {
    quantity: Decimal,
    cost_price: Decimal,
    purchase_date: NaiveDate,
}

#[derive(Debug, Clone, Default)]
pub struct RealizedGainsByTerm {
    pub short_term: Decimal,
    pub long_term: Decimal,
}

#[derive(Debug, Clone)]
pub struct OpenLotSummary {
    pub quantity: Decimal,
    pub cost_price: Decimal,
    pub purchase_date: NaiveDate,
    pub is_long_term: bool,
}

/// Replays a single instrument's transactions in date order, matching
/// each Sell against the oldest open lot(s) first. Bonus/Split are
/// handled the same way the Holding aggregate does (Bonus adds a
/// zero-cost lot; Split rescales all open lots to the new share count,
/// preserving total cost) so lot quantities stay consistent with what
/// Holdings actually shows.
fn replay_fifo(transactions: &[Transaction]) -> (Vec<OpenLot>, RealizedGainsByTerm) {
    let mut sorted: Vec<&Transaction> = transactions.iter().collect();
    sorted.sort_by_key(|t| (t.trade_date, t.recorded_at));

    let mut open_lots: Vec<OpenLot> = Vec::new();
    let mut gains = RealizedGainsByTerm::default();

    for txn in sorted {
        match txn.transaction_type {
            TransactionType::Buy | TransactionType::SipInstallment => {
                open_lots.push(OpenLot { quantity: txn.quantity, cost_price: txn.price.amount(), purchase_date: txn.trade_date });
            }
            TransactionType::Bonus => {
                // Zero-cost lot, dated at the bonus credit itself — a
                // bonus share's holding period starts from when it's
                // actually received, not the original holding's date.
                open_lots.push(OpenLot { quantity: txn.quantity, cost_price: Decimal::ZERO, purchase_date: txn.trade_date });
            }
            TransactionType::Split => {
                let total_before: Decimal = open_lots.iter().map(|l| l.quantity).sum();
                if total_before > Decimal::ZERO {
                    let ratio = txn.quantity / total_before;
                    for lot in &mut open_lots {
                        lot.quantity *= ratio;
                        lot.cost_price /= ratio;
                    }
                }
            }
            TransactionType::Dividend => {} // no lot impact — see cash_impact()
            TransactionType::Sell => {
                let mut remaining_to_sell = txn.quantity;
                let per_share_fee = if txn.quantity > Decimal::ZERO { txn.fees.amount() / txn.quantity } else { Decimal::ZERO };

                while remaining_to_sell > Decimal::ZERO {
                    let Some(lot) = open_lots.first_mut() else { break }; // oversold ledger — nothing left to match, drop remainder rather than panic
                    let matched_qty = remaining_to_sell.min(lot.quantity);
                    let gain = (txn.price.amount() - lot.cost_price - per_share_fee) * matched_qty;
                    let holding_days = (txn.trade_date - lot.purchase_date).num_days();

                    if holding_days >= LTCG_THRESHOLD_DAYS {
                        gains.long_term += gain;
                    } else {
                        gains.short_term += gain;
                    }

                    lot.quantity -= matched_qty;
                    remaining_to_sell -= matched_qty;
                    if lot.quantity <= Decimal::ZERO {
                        open_lots.remove(0);
                    }
                }
            }
        }
    }

    (open_lots, gains)
}

/// Public entry point for the STCG/LTCG tax view — realized gains for
/// this instrument's transactions, split by term. `as_of` isn't currently
/// used (all Sells in the ledger are past by definition) but is threaded
/// through for a future "as of a chosen date" report without changing
/// the signature later.
pub fn realized_gains_by_term(transactions: &[Transaction], _as_of: NaiveDate) -> RealizedGainsByTerm {
    replay_fifo(transactions).1
}

/// Public entry point for tax-loss harvesting — the currently OPEN lots
/// (still held), each tagged with whether it's already long-term or would
/// still be short-term if sold today. A harvesting candidate is an open
/// lot whose cost basis is above the current price; this function doesn't
/// know the current price (that's a live quote, not ledger data), so it
/// returns lots for the caller to compare against a price it fetches.
pub fn open_lots(transactions: &[Transaction], as_of: NaiveDate) -> Vec<OpenLotSummary> {
    let (lots, _) = replay_fifo(transactions);
    lots.into_iter()
        .map(|l| OpenLotSummary {
            quantity: l.quantity,
            cost_price: l.cost_price,
            purchase_date: l.purchase_date,
            is_long_term: (as_of - l.purchase_date).num_days() >= LTCG_THRESHOLD_DAYS,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::value_objects::Money;
    use uuid::Uuid;

    fn txn(kind: TransactionType, qty: &str, price: &str, date: &str) -> Transaction {
        Transaction {
            id: Uuid::new_v4(),
            portfolio_id: Uuid::new_v4(),
            instrument_id: Uuid::new_v4(),
            transaction_type: kind,
            quantity: qty.parse().unwrap(),
            price: Money::inr(price.parse().unwrap()),
            fees: Money::inr(Decimal::ZERO),
            trade_date: NaiveDate::parse_from_str(date, "%Y-%m-%d").unwrap(),
            broker_ref: None,
            recorded_at: chrono::Utc::now(),
        }
    }

    #[test]
    fn a_sale_held_over_a_year_is_classified_long_term() {
        let txns = vec![
            txn(TransactionType::Buy, "100", "50", "2024-01-01"),
            txn(TransactionType::Sell, "100", "80", "2025-06-01"), // ~17 months later
        ];
        let gains = realized_gains_by_term(&txns, NaiveDate::from_ymd_opt(2025, 6, 1).unwrap());
        assert_eq!(gains.long_term, Decimal::from(3000)); // (80-50)*100
        assert_eq!(gains.short_term, Decimal::ZERO);
    }

    #[test]
    fn a_sale_held_under_a_year_is_classified_short_term() {
        let txns = vec![
            txn(TransactionType::Buy, "100", "50", "2025-01-01"),
            txn(TransactionType::Sell, "100", "80", "2025-06-01"), // ~5 months later
        ];
        let gains = realized_gains_by_term(&txns, NaiveDate::from_ymd_opt(2025, 6, 1).unwrap());
        assert_eq!(gains.short_term, Decimal::from(3000));
        assert_eq!(gains.long_term, Decimal::ZERO);
    }

    #[test]
    fn fifo_matches_oldest_lot_first_across_two_separate_buys() {
        let txns = vec![
            txn(TransactionType::Buy, "50", "40", "2023-01-01"),  // old lot: LTCG-eligible by the sell date
            txn(TransactionType::Buy, "50", "60", "2025-03-01"),  // recent lot: still short-term
            txn(TransactionType::Sell, "50", "100", "2025-06-01"), // should match the OLD lot first
        ];
        let gains = realized_gains_by_term(&txns, NaiveDate::from_ymd_opt(2025, 6, 1).unwrap());
        // (100-40)*50 = 3000, matched against the old lot => long-term
        assert_eq!(gains.long_term, Decimal::from(3000));
        assert_eq!(gains.short_term, Decimal::ZERO);
    }

    #[test]
    fn a_sale_spanning_two_lots_splits_the_gain_by_each_lots_own_term() {
        let txns = vec![
            txn(TransactionType::Buy, "50", "40", "2023-01-01"),  // long-term by sell date
            txn(TransactionType::Buy, "50", "60", "2025-03-01"),  // short-term by sell date
            txn(TransactionType::Sell, "80", "100", "2025-06-01"), // 50 from lot 1 (LT), 30 from lot 2 (ST)
        ];
        let gains = realized_gains_by_term(&txns, NaiveDate::from_ymd_opt(2025, 6, 1).unwrap());
        assert_eq!(gains.long_term, Decimal::from(3000)); // (100-40)*50
        assert_eq!(gains.short_term, Decimal::from(1200)); // (100-60)*30
    }

    #[test]
    fn fees_are_apportioned_per_share_and_reduce_the_gain() {
        let mut sell = txn(TransactionType::Sell, "100", "80", "2025-06-01");
        sell.fees = Money::inr(Decimal::from(100));
        let txns = vec![txn(TransactionType::Buy, "100", "50", "2024-01-01"), sell];
        let gains = realized_gains_by_term(&txns, NaiveDate::from_ymd_opt(2025, 6, 1).unwrap());
        // (80-50)*100 - 100 fees = 2900
        assert_eq!(gains.long_term, Decimal::from(2900));
    }

    #[test]
    fn bonus_shares_get_a_zero_cost_lot_dated_at_the_bonus_itself() {
        let txns = vec![
            txn(TransactionType::Buy, "100", "50", "2024-01-01"),
            txn(TransactionType::Bonus, "100", "0", "2024-06-01"),
        ];
        let lots = open_lots(&txns, NaiveDate::from_ymd_opt(2025, 6, 1).unwrap());
        assert_eq!(lots.len(), 2);
        assert_eq!(lots[1].cost_price, Decimal::ZERO);
    }

    #[test]
    fn split_rescales_all_open_lots_preserving_total_cost() {
        let txns = vec![
            txn(TransactionType::Buy, "100", "50", "2024-01-01"), // total cost 5000
            txn(TransactionType::Split, "200", "0", "2024-06-01"), // 2-for-1
        ];
        let lots = open_lots(&txns, NaiveDate::from_ymd_opt(2025, 6, 1).unwrap());
        assert_eq!(lots.len(), 1);
        assert_eq!(lots[0].quantity, Decimal::from(200));
        assert_eq!(lots[0].cost_price, Decimal::from(25)); // 5000 / 200
    }

    #[test]
    fn open_lots_reports_still_held_quantity_with_correct_term_flag() {
        let txns = vec![
            txn(TransactionType::Buy, "100", "50", "2024-01-01"),
            txn(TransactionType::Sell, "40", "80", "2025-06-01"),
        ];
        let lots = open_lots(&txns, NaiveDate::from_ymd_opt(2025, 6, 1).unwrap());
        assert_eq!(lots.len(), 1);
        assert_eq!(lots[0].quantity, Decimal::from(60));
        assert!(lots[0].is_long_term);
    }

    #[test]
    fn dividends_never_affect_lots_or_gains() {
        let txns = vec![
            txn(TransactionType::Buy, "100", "50", "2024-01-01"),
            txn(TransactionType::Dividend, "100", "5", "2024-07-01"),
        ];
        let lots = open_lots(&txns, NaiveDate::from_ymd_opt(2025, 6, 1).unwrap());
        assert_eq!(lots.len(), 1);
        assert_eq!(lots[0].quantity, Decimal::from(100));
    }
}

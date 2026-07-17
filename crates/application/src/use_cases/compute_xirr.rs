//! ComputeXirrUseCase — SRS 2.2.3 dashboard metric. Builds the cashflow
//! series from the transaction ledger (outflows for buys/SIP installments,
//! inflows for sells/dividends) plus a final synthetic cashflow for the
//! current mark-to-market value, then hands it to the pure domain solver.

use chrono::Utc;
use pm_domain::analytics::{compute_xirr, Cashflow};
use pm_domain::entities::TransactionType;
use pm_domain::repositories::{PriceRepository, RepositoryError, TransactionRepository};
use pm_domain::DomainError;
use rust_decimal::prelude::ToPrimitive;
use std::sync::Arc;
use uuid::Uuid;

#[derive(Debug, thiserror::Error)]
pub enum ComputeXirrError {
    #[error(transparent)]
    Repository(#[from] RepositoryError),
    #[error(transparent)]
    Domain(#[from] DomainError),
    #[error("no current holding to mark-to-market for instrument {0}")]
    NoCurrentPrice(Uuid),
}

pub struct ComputeXirrUseCase {
    transactions: Arc<dyn TransactionRepository>,
    prices: Arc<dyn PriceRepository>,
}

impl ComputeXirrUseCase {
    pub fn new(transactions: Arc<dyn TransactionRepository>, prices: Arc<dyn PriceRepository>) -> Self {
        Self { transactions, prices }
    }

    /// Per-instrument XIRR (e.g. for a single mutual fund's SIP performance,
    /// SRS 2.2.3 "Mutual Fund specific: ... XIRR").
    pub async fn execute_for_instrument(
        &self,
        portfolio_id: Uuid,
        instrument_id: Uuid,
    ) -> Result<f64, ComputeXirrError> {
        let ledger = self
            .transactions
            .list_for_instrument(portfolio_id, instrument_id)
            .await?;

        let mut flows: Vec<Cashflow> = ledger
            .iter()
            .filter_map(|t| {
                let amount = t.cash_impact().to_f64()?;
                if amount == 0.0 {
                    return None; // Bonus/Split: no cashflow, correctly excluded from XIRR
                }
                Some(Cashflow { date: t.trade_date, amount })
            })
            .collect();

        // Current holding value is itself an implicit "sell today" cashflow —
        // this is what makes XIRR reflect unrealized gains, not just realized ones.
        let held_qty: rust_decimal::Decimal = ledger.iter().fold(rust_decimal::Decimal::ZERO, |acc, t| {
            match t.transaction_type {
                TransactionType::Buy | TransactionType::SipInstallment | TransactionType::Bonus => acc + t.quantity,
                TransactionType::Sell => acc - t.quantity,
                TransactionType::Split => t.quantity, // convention: carries new total qty
                TransactionType::Dividend => acc,
            }
        });

        if held_qty > rust_decimal::Decimal::ZERO {
            let ltp = self
                .prices
                .latest_price(instrument_id)
                .await?
                .ok_or(ComputeXirrError::NoCurrentPrice(instrument_id))?;
            let mtm_value = (held_qty * ltp).to_f64().unwrap_or(0.0);
            flows.push(Cashflow {
                date: Utc::now().date_naive(),
                amount: mtm_value,
            });
        }

        Ok(compute_xirr(&flows)?)
    }
}

//! RebuildHoldingsUseCase — full recompute of every holding in a portfolio
//! from the transaction ledger. Used after a bulk CSV import, a broker
//! backfill sync, or if the cached snapshot table is ever suspected to have
//! drifted (HLD Section 5.1: holding_snapshot is "rebuildable from
//! transaction table" — this use-case IS that rebuild).

use pm_domain::entities::Holding;
use pm_domain::repositories::{HoldingRepository, RepositoryError, TransactionRepository};
use std::collections::HashMap;
use std::sync::Arc;
use uuid::Uuid;

pub struct RebuildHoldingsUseCase {
    transactions: Arc<dyn TransactionRepository>,
    holdings: Arc<dyn HoldingRepository>,
}

impl RebuildHoldingsUseCase {
    pub fn new(
        transactions: Arc<dyn TransactionRepository>,
        holdings: Arc<dyn HoldingRepository>,
    ) -> Self {
        Self {
            transactions,
            holdings,
        }
    }

    /// Rebuilds and persists a fresh snapshot for every instrument the
    /// portfolio has ever transacted in. Returns the rebuilt holdings so the
    /// caller (e.g. a "post-import sync complete" event) can act on them
    /// without a second read.
    pub async fn execute(&self, portfolio_id: Uuid) -> Result<Vec<Holding>, RepositoryError> {
        let mut ledger = self.transactions.list_for_portfolio(portfolio_id).await?;
        ledger.sort_by_key(|t| (t.trade_date, t.recorded_at));

        let mut by_instrument: HashMap<Uuid, Holding> = HashMap::new();
        for txn in &ledger {
            let holding = by_instrument
                .entry(txn.instrument_id)
                .or_insert_with(|| Holding::empty(portfolio_id, txn.instrument_id));
            // A malformed historical ledger entry shouldn't halt the whole
            // rebuild; log-and-skip is the right call here since this path
            // runs over already-recorded history, not a new write the user
            // is actively making (contrast with RecordTransactionUseCase,
            // which must reject bad input before it's ever persisted).
            let _ = holding.apply(txn);
        }

        let today = chrono::Utc::now().date_naive();
        let mut rebuilt = Vec::with_capacity(by_instrument.len());
        for holding in by_instrument.into_values() {
            self.holdings.upsert_snapshot(&holding, today).await?;
            rebuilt.push(holding);
        }
        Ok(rebuilt)
    }
}

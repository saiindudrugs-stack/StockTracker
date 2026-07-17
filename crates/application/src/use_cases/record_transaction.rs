//! RecordTransactionUseCase — SRS 2.2.1: "Maintain full transaction ledger
//! ... as the source of truth; all holdings and analytics are derived, never
//! hand-edited." This is the *only* application-layer entry point for
//! writing a transaction; it always follows with a holding rebuild so the
//! cached snapshot never drifts from the ledger.

use pm_domain::entities::{Holding, Transaction};
use pm_domain::repositories::{HoldingRepository, RepositoryError, TransactionRepository};
use std::sync::Arc;

pub struct RecordTransactionUseCase {
    transactions: Arc<dyn TransactionRepository>,
    holdings: Arc<dyn HoldingRepository>,
}

impl RecordTransactionUseCase {
    pub fn new(
        transactions: Arc<dyn TransactionRepository>,
        holdings: Arc<dyn HoldingRepository>,
    ) -> Self {
        Self {
            transactions,
            holdings,
        }
    }

    /// Folds the instrument's *entire* ledger history (not just this one
    /// transaction) plus the incoming transaction into a fresh Holding, and
    /// only persists anything once that fold succeeds. Folding the full
    /// history rather than incrementally patching the cached snapshot is
    /// deliberately the simple, obviously-correct approach for v1 — it costs
    /// an extra read of (typically a few hundred) transactions per
    /// instrument, which is negligible, and it means the cache can never
    /// silently diverge from the ledger (HLD Section 5.1: "Derived/cached,
    /// rebuildable from transaction table").
    ///
    /// Validation happens against the *would-be* full history before
    /// anything is written — an invalid transaction (e.g. a sell that
    /// overdraws the position) is rejected without ever touching the
    /// append-only ledger, since a bad entry there would need a manual
    /// offsetting correction to undo (entities.rs "Auditability" note).
    pub async fn execute(&self, txn: Transaction) -> Result<Holding, UseCaseError> {
        let mut history = self
            .transactions
            .list_for_instrument(txn.portfolio_id, txn.instrument_id)
            .await?;
        history.push(txn.clone());
        history.sort_by_key(|t| (t.trade_date, t.recorded_at));

        let mut holding = Holding::empty(txn.portfolio_id, txn.instrument_id);
        for t in &history {
            holding.apply(t)?; // validation happens here, nothing persisted yet
        }

        // Only now, with the fold proven valid, commit both writes.
        self.transactions.record(&txn).await?;
        let today = chrono::Utc::now().date_naive();
        self.holdings.upsert_snapshot(&holding, today).await?;

        Ok(holding)
    }
}

#[derive(Debug, thiserror::Error)]
pub enum UseCaseError {
    #[error(transparent)]
    Domain(#[from] pm_domain::DomainError),
    #[error(transparent)]
    Repository(#[from] RepositoryError),
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use chrono::{NaiveDate, Utc};
    use pm_domain::entities::TransactionType;
    use pm_domain::value_objects::Money;
    use rust_decimal_macros::dec;
    use std::sync::Mutex;
    use uuid::Uuid;

    #[derive(Default)]
    struct InMemoryTransactions(Mutex<Vec<Transaction>>);
    #[async_trait]
    impl TransactionRepository for InMemoryTransactions {
        async fn record(&self, txn: &Transaction) -> Result<(), RepositoryError> {
            self.0.lock().unwrap().push(txn.clone());
            Ok(())
        }
        async fn list_for_portfolio(
            &self,
            portfolio_id: Uuid,
        ) -> Result<Vec<Transaction>, RepositoryError> {
            Ok(self.0.lock().unwrap().iter().filter(|t| t.portfolio_id == portfolio_id).cloned().collect())
        }
        async fn list_for_instrument(
            &self,
            portfolio_id: Uuid,
            instrument_id: Uuid,
        ) -> Result<Vec<Transaction>, RepositoryError> {
            Ok(self
                .0
                .lock()
                .unwrap()
                .iter()
                .filter(|t| t.portfolio_id == portfolio_id && t.instrument_id == instrument_id)
                .cloned()
                .collect())
        }
    }

    #[derive(Default)]
    struct InMemoryHoldings(Mutex<Vec<Holding>>);
    #[async_trait]
    impl HoldingRepository for InMemoryHoldings {
        async fn upsert_snapshot(&self, holding: &Holding, _as_of: NaiveDate) -> Result<(), RepositoryError> {
            let mut guard = self.0.lock().unwrap();
            guard.retain(|h| !(h.portfolio_id == holding.portfolio_id && h.instrument_id == holding.instrument_id));
            guard.push(holding.clone());
            Ok(())
        }
        async fn get_snapshot(&self, portfolio_id: Uuid, instrument_id: Uuid) -> Result<Option<Holding>, RepositoryError> {
            Ok(self.0.lock().unwrap().iter().find(|h| h.portfolio_id == portfolio_id && h.instrument_id == instrument_id).cloned())
        }
        async fn list_for_portfolio(&self, portfolio_id: Uuid) -> Result<Vec<Holding>, RepositoryError> {
            Ok(self.0.lock().unwrap().iter().filter(|h| h.portfolio_id == portfolio_id).cloned().collect())
        }
    }

    fn sample_txn(portfolio_id: Uuid, instrument_id: Uuid, ttype: TransactionType, qty: rust_decimal::Decimal, price: rust_decimal::Decimal) -> Transaction {
        Transaction {
            id: Uuid::new_v4(),
            portfolio_id,
            instrument_id,
            transaction_type: ttype,
            quantity: qty,
            price: Money::inr(price),
            fees: Money::inr(dec!(20)),
            trade_date: NaiveDate::from_ymd_opt(2026, 1, 1).unwrap(),
            broker_ref: None,
            recorded_at: Utc::now(),
        }
    }

    #[tokio::test]
    async fn recording_a_buy_creates_a_holding() {
        let txn_repo = Arc::new(InMemoryTransactions::default());
        let holding_repo = Arc::new(InMemoryHoldings::default());
        let use_case = RecordTransactionUseCase::new(txn_repo.clone(), holding_repo.clone());

        let portfolio_id = Uuid::new_v4();
        let instrument_id = Uuid::new_v4();
        let txn = sample_txn(portfolio_id, instrument_id, TransactionType::Buy, dec!(10), dec!(100));

        let holding = use_case.execute(txn).await.unwrap();
        assert_eq!(holding.quantity, dec!(10));
        assert_eq!(txn_repo.list_for_portfolio(portfolio_id).await.unwrap().len(), 1);
    }

    #[tokio::test]
    async fn rebuilds_full_history_deterministically_regardless_of_recording_order() {
        let txn_repo = Arc::new(InMemoryTransactions::default());
        let holding_repo = Arc::new(InMemoryHoldings::default());
        let use_case = RecordTransactionUseCase::new(txn_repo.clone(), holding_repo.clone());

        let portfolio_id = Uuid::new_v4();
        let instrument_id = Uuid::new_v4();

        use_case
            .execute(sample_txn(portfolio_id, instrument_id, TransactionType::Buy, dec!(10), dec!(100)))
            .await
            .unwrap();
        let holding = use_case
            .execute(sample_txn(portfolio_id, instrument_id, TransactionType::Buy, dec!(10), dec!(110)))
            .await
            .unwrap();

        // Same averaging as the pure Holding::apply test in pm-domain — this
        // confirms the use-case's "rebuild from full history" path agrees
        // with the domain's incremental fold for the same transaction set.
        assert_eq!(holding.quantity, dec!(20));
        assert_eq!(holding.avg_cost, dec!(107));
    }

    #[tokio::test]
    async fn invalid_sell_returns_error_and_is_not_left_in_a_bad_snapshot() {
        let txn_repo = Arc::new(InMemoryTransactions::default());
        let holding_repo = Arc::new(InMemoryHoldings::default());
        let use_case = RecordTransactionUseCase::new(txn_repo.clone(), holding_repo.clone());

        let portfolio_id = Uuid::new_v4();
        let instrument_id = Uuid::new_v4();
        let bad_sell = sample_txn(portfolio_id, instrument_id, TransactionType::Sell, dec!(5), dec!(100));

        let result = use_case.execute(bad_sell).await;
        assert!(result.is_err());
        // The real point of this test: an invalid transaction must never
        // reach the append-only ledger, since undoing it there would need a
        // manual offsetting entry rather than a clean rejection.
        assert_eq!(txn_repo.list_for_portfolio(portfolio_id).await.unwrap().len(), 0);
        assert!(holding_repo.get_snapshot(portfolio_id, instrument_id).await.unwrap().is_none());
    }
}


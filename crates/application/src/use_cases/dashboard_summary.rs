//! DashboardSummaryUseCase — SRS 2.2.3: "Net Worth, Today's P/L, Overall P/L
//! ... Asset Allocation". This is the single query the main Dashboard screen
//! (Volume I wireframes, dashboard_wireframe) calls on load and on every
//! price-tick event from the Live Feed Manager.

use pm_domain::repositories::{HoldingRepository, PriceRepository, RepositoryError};
use rust_decimal::Decimal;
use serde::Serialize;
use std::sync::Arc;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct DashboardSummary {
    pub net_worth: Decimal,
    pub overall_unrealized_pnl: Decimal,
    pub overall_realized_pnl: Decimal,
    pub holdings_priced: usize,
    pub holdings_missing_price: usize,
}

pub struct DashboardSummaryUseCase {
    holdings: Arc<dyn HoldingRepository>,
    prices: Arc<dyn PriceRepository>,
}

impl DashboardSummaryUseCase {
    pub fn new(holdings: Arc<dyn HoldingRepository>, prices: Arc<dyn PriceRepository>) -> Self {
        Self { holdings, prices }
    }

    /// Deliberately tolerant of missing prices (e.g. a broker feed hiccup,
    /// NFR "Offline support": "Full read access to last-synced data with no
    /// network"): instruments without a cached price are counted and
    /// reported separately rather than failing the whole dashboard.
    pub async fn execute(&self, portfolio_id: Uuid) -> Result<DashboardSummary, RepositoryError> {
        let holdings = self.holdings.list_for_portfolio(portfolio_id).await?;

        let mut net_worth = Decimal::ZERO;
        let mut overall_unrealized = Decimal::ZERO;
        let mut overall_realized = Decimal::ZERO;
        let mut priced = 0usize;
        let mut missing = 0usize;

        for holding in &holdings {
            overall_realized += holding.realized_pnl;
            match self.prices.latest_price(holding.instrument_id).await? {
                Some(ltp) => {
                    net_worth += holding.market_value(ltp);
                    overall_unrealized += holding.unrealized_pnl(ltp);
                    priced += 1;
                }
                None => missing += 1,
            }
        }

        Ok(DashboardSummary {
            net_worth,
            overall_unrealized_pnl: overall_unrealized,
            overall_realized_pnl: overall_realized,
            holdings_priced: priced,
            holdings_missing_price: missing,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use chrono::NaiveDate;
    use pm_domain::entities::Holding;
    use rust_decimal_macros::dec;
    use std::collections::HashMap;
    use std::sync::Mutex;

    struct FakeHoldings(Vec<Holding>);
    #[async_trait]
    impl HoldingRepository for FakeHoldings {
        async fn upsert_snapshot(&self, _h: &Holding, _d: NaiveDate) -> Result<(), RepositoryError> {
            unimplemented!("not needed for this test")
        }
        async fn get_snapshot(&self, _p: Uuid, _i: Uuid) -> Result<Option<Holding>, RepositoryError> {
            unimplemented!("not needed for this test")
        }
        async fn list_for_portfolio(&self, portfolio_id: Uuid) -> Result<Vec<Holding>, RepositoryError> {
            Ok(self.0.iter().filter(|h| h.portfolio_id == portfolio_id).cloned().collect())
        }
        async fn delete_snapshot(&self, _p: Uuid, _i: Uuid) -> Result<(), RepositoryError> {
            unimplemented!("not needed for this test")
        }
    }

    struct FakePrices(Mutex<HashMap<Uuid, Decimal>>);
    #[async_trait]
    impl PriceRepository for FakePrices {
        async fn upsert_daily_bar(&self, _i: Uuid, _d: NaiveDate, _c: Decimal) -> Result<(), RepositoryError> {
            unimplemented!("not needed for this test")
        }
        async fn latest_price(&self, instrument_id: Uuid) -> Result<Option<Decimal>, RepositoryError> {
            Ok(self.0.lock().unwrap().get(&instrument_id).copied())
        }
        async fn daily_series(&self, _i: Uuid, _f: NaiveDate, _t: NaiveDate) -> Result<Vec<(NaiveDate, Decimal)>, RepositoryError> {
            unimplemented!("not needed for this test")
        }
        async fn upsert_ohlc_bar(&self, _i: Uuid, _bar: pm_domain::repositories::OhlcBar) -> Result<(), RepositoryError> {
            unimplemented!("not needed for this test")
        }
        async fn ohlc_series(&self, _i: Uuid, _f: NaiveDate, _t: NaiveDate) -> Result<Vec<pm_domain::repositories::OhlcBar>, RepositoryError> {
            unimplemented!("not needed for this test")
        }
    }

    #[tokio::test]
    async fn aggregates_net_worth_and_pnl_across_holdings_and_tolerates_missing_prices() {
        let portfolio_id = Uuid::new_v4();
        let priced_instrument = Uuid::new_v4();
        let unpriced_instrument = Uuid::new_v4();

        let holdings = FakeHoldings(vec![
            Holding {
                portfolio_id,
                instrument_id: priced_instrument,
                quantity: dec!(10),
                avg_cost: dec!(100),
                realized_pnl: dec!(50),
            },
            Holding {
                portfolio_id,
                instrument_id: unpriced_instrument,
                quantity: dec!(5),
                avg_cost: dec!(200),
                realized_pnl: dec!(0),
            },
        ]);
        let mut price_map = HashMap::new();
        price_map.insert(priced_instrument, dec!(120));
        let prices = FakePrices(Mutex::new(price_map));

        let use_case = DashboardSummaryUseCase::new(Arc::new(holdings), Arc::new(prices));
        let summary = use_case.execute(portfolio_id).await.unwrap();

        assert_eq!(summary.net_worth, dec!(1200)); // 10 * 120, unpriced holding excluded
        assert_eq!(summary.overall_unrealized_pnl, dec!(200)); // (120-100)*10
        assert_eq!(summary.overall_realized_pnl, dec!(50));
        assert_eq!(summary.holdings_priced, 1);
        assert_eq!(summary.holdings_missing_price, 1);
    }
}

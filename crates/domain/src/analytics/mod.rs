pub mod xirr;
pub mod market_phase;
pub mod portfolio_stats;
pub mod signal;
pub mod tax_lots;

pub use xirr::{compute_xirr, Cashflow};
pub use market_phase::{classify_market_phase, DailyBar, MarketPhase};
pub use portfolio_stats::{
    annualized_return, annualized_volatility, cagr, daily_returns, historical_var, mean,
    pearson_correlation, rsi, sharpe_ratio, simple_interest_value, sma_series, std_dev,
};
pub use signal::{generate_signal, FibLevel, Recommendation, Signal};
pub use tax_lots::{open_lots, realized_gains_by_term, OpenLotSummary, RealizedGainsByTerm};

use thiserror::Error;

/// Errors that originate from domain logic itself — never from I/O.
/// Infrastructure-layer errors (DB, network) are defined in the infrastructure
/// crate and mapped into these where they cross into application use-cases.
#[derive(Debug, Error, Clone, PartialEq)]
pub enum DomainError {
    #[error("negative or zero quantity is not valid for a {0} transaction")]
    InvalidQuantity(&'static str),

    #[error("cannot sell {requested} units, only {available} held of instrument {instrument}")]
    InsufficientHolding {
        instrument: String,
        available: rust_decimal::Decimal,
        requested: rust_decimal::Decimal,
    },

    #[error("XIRR did not converge after {0} iterations")]
    XirrDidNotConverge(u32),

    #[error("XIRR requires at least one inflow and one outflow cashflow")]
    XirrInsufficientCashflows,

    #[error("invalid ISIN format: {0}")]
    InvalidIsin(String),
}

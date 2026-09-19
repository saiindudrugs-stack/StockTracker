//! Value objects: immutable, defined by their value rather than an identity.
//! Per SRS NFR "Security"/"Auditability", money is never represented as f64 —
//! floating point is unacceptable for a ledger. rust_decimal::Decimal is exact.

use crate::errors::DomainError;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use std::fmt;

/// Amount of money in a specific currency. Never compare or add Money of
/// different currencies without an explicit conversion step (not modeled yet —
/// v1 is INR-only per the confirmed India-only tax scope).
#[derive(Debug, Clone, Copy, PartialEq, PartialOrd, Serialize, Deserialize)]
pub struct Money {
    amount: Decimal,
    currency: Currency,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum Currency {
    Inr,
}

impl Money {
    pub fn inr(amount: Decimal) -> Self {
        Self {
            amount,
            currency: Currency::Inr,
        }
    }

    pub fn amount(&self) -> Decimal {
        self.amount
    }

    pub fn currency(&self) -> Currency {
        self.currency
    }

    pub fn zero() -> Self {
        Self::inr(Decimal::ZERO)
    }

    pub fn checked_add(&self, other: &Money) -> Option<Money> {
        if self.currency != other.currency {
            return None;
        }
        Some(Money {
            amount: self.amount + other.amount,
            currency: self.currency,
        })
    }
}

impl fmt::Display for Money {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.currency {
            Currency::Inr => write!(f, "₹{:.2}", self.amount),
        }
    }
}

/// International Securities Identification Number — 12 chars, e.g. INE002A01018.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Isin(String);

impl Isin {
    pub fn parse(raw: &str) -> Result<Self, DomainError> {
        let raw = raw.trim().to_uppercase();
        let valid = raw.len() == 12
            && raw.chars().take(2).all(|c| c.is_ascii_alphabetic())
            && raw.chars().skip(2).all(|c| c.is_ascii_alphanumeric());
        if !valid {
            return Err(DomainError::InvalidIsin(raw));
        }
        Ok(Self(raw))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for Isin {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    #[test]
    fn money_add_same_currency() {
        let a = Money::inr(dec!(100.50));
        let b = Money::inr(dec!(50.25));
        assert_eq!(a.checked_add(&b).unwrap().amount(), dec!(150.75));
    }

    #[test]
    fn isin_parses_valid() {
        assert!(Isin::parse("INE002A01018").is_ok());
    }

    #[test]
    fn isin_rejects_invalid() {
        assert!(Isin::parse("NOTANISIN").is_err());
    }
}

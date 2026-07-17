//! pm-domain: pure business logic for the Portfolio Management Application.
//!
//! This crate has NO infrastructure dependencies (no DB, no HTTP, no filesystem).
//! It defines entities, value objects, domain services, and repository *traits*
//! (interfaces) that the infrastructure crate implements. Per the HLD (Section 3.1),
//! nothing in here should ever need to change because we swapped SQLite for
//! something else, or Zerodha for Upstox.

pub mod value_objects;
pub mod entities;
pub mod analytics;
pub mod repositories;
pub mod errors;

pub use errors::DomainError;

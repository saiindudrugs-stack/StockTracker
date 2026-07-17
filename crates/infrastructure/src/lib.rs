//! pm-infrastructure: implements the domain-defined repository traits and
//! the Broker Adapter Framework (HLD Section 3.1, 3.2). Nothing in the
//! domain or application crates knows this crate exists -- they only see
//! trait objects (Arc<dyn TransactionRepository> etc.), which is what
//! makes the SQLite engine, and each broker, swappable.

pub mod sqlite;
pub mod brokers;
pub mod live_feed;

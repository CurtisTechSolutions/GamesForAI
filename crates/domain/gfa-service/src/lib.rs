//! Transport-independent match events and persistence ports.
//!
//! Hosts must authorize seats and viewers before accepting network requests.
//! Storage adapters must honor the atomicity and idempotency contract in MatchStore.
mod model;
mod ports;

pub use model::*;
pub use ports::*;

//! Transport-independent event-sourced match lifecycle.
//!
//! This library is a trusted-host boundary: callers select authorized viewers and
//! seats. No public transport is exposed here. Authentication and visibility policy
//! must be integrated before exposing these operations to untrusted network callers.
mod error;
mod model;
mod ports;
mod replay;
mod service;

pub use model::*;
pub use ports::*;
pub use service::GameService;

#[cfg(test)]
mod tests;

mod briefing;

mod position;

mod simulation;

mod fork;

mod opponents;
mod planning;
pub use opponents::{
    AnalysisFuture, BuiltinOpponentFactory, OpponentExecutor, OpponentFactory, OpponentFuture,
    OpponentJob,
};

mod seats;

mod scheduling;
pub use scheduling::{OpponentQueueFailure, PendingOpponents};

mod events;

/// Pure Glicko-2 rating periods and fixed calibration anchors.
pub mod ratings;

mod training;

/// Durable provider-call journals and shared spending reservations.
pub mod llm_ledger;

//! Seats controlled by external callers or installed opponents.
use crate::OpponentConfig;
use gfa_core::LegalAction;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Seat assignment. Omitted seat lists retain the all-external default.
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum Seat {
    /// Controlled by the creating caller.
    #[default]
    #[serde(rename = "self")]
    SelfPlayer,
    /// Controlled by a local human.
    Human,
    /// Available for an external player.
    Open,
    /// Driven by an installed player through its observation-only interface.
    Opponent {
        /// Player identifier, level, and resource bounds.
        opponent: OpponentConfig,
        /// Planning seed, independent of the game's RNG; materialized at creation.
        #[serde(default)]
        seed: Option<u64>,
    },
}

/// An automatic reply included in the same atomic command as a caller's action.
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct OpponentReply {
    /// Acting opponent seat.
    pub seat: u8,
    /// Turn before this automatic action.
    pub turn: u64,
    /// Action for perfect-information games; private actions are never exposed here.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub action: Option<LegalAction>,
}

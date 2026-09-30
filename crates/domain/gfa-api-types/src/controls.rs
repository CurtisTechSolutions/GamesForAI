//! Match controls and outcomes separate from engine moves.
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// A seat-scoped control command against an expected turn.
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ControlRequest {
    /// Seat sending this command.
    #[cfg_attr(feature = "openapi", schema(minimum = 0, maximum = 255))]
    pub seat: u8,
    /// Expected accepted-action count.
    #[cfg_attr(feature = "openapi", schema(minimum = 0))]
    pub turn: u64,
}

/// An explicit ending caused by a match control.
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "reason", rename_all = "snake_case")]
pub enum MatchOutcome {
    /// One seat resigned; the engine position remains unchanged.
    Resigned {
        /// Resigning seat.
        seat: u8,
    },
    /// Both seats agreed to draw.
    AgreedDraw,
}

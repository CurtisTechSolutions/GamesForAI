use crate::{Difficulty, Grade};
use gfa_core::{
    schemars::JsonSchema,
    serde::{Deserialize, Serialize},
    GameError,
};
use std::{collections::BTreeMap, sync::OnceLock};

/// Puzzle options. A supplied puzzle is graded from its actual solving techniques.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(crate = "gfa_core::serde", default, deny_unknown_fields)]
#[schemars(crate = "gfa_core::schemars")]
pub struct Config {
    /// Side length: 4, 6, or 9.
    pub size: u8,
    /// Requested generation grade; imported puzzles retain their actual grade.
    pub difficulty: Difficulty,
    /// Optional size²-character grid, '.' or '0' for empty cells.
    pub puzzle: Option<String>,
    /// "silent", "rule_check", or "solution_check:n" for 1..99 mistakes.
    pub mistake_policy: String,
    /// Permit free candidate notes.
    pub allow_notes: bool,
    /// Placement/erasure limit, 1..10000; default 200. Notes do not consume it.
    pub max_moves: Option<u32>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            size: 9,
            difficulty: Difficulty::Easy,
            puzzle: None,
            mistake_policy: "silent".into(),
            allow_notes: true,
            max_moves: None,
        }
    }
}

impl Config {
    pub(crate) fn validate(&self) -> Result<(), GameError> {
        crate::grid::boxes(self.size)?;
        self.policy()?;
        if self.max_moves.is_some_and(|n| !(1..=10000).contains(&n)) {
            return Err(GameError::position(
                "max_moves must be from 1 through 10000",
            ));
        }
        Ok(())
    }

    pub(crate) fn policy(&self) -> Result<Policy, GameError> {
        match self.mistake_policy.as_str() {
            "silent" => Ok(Policy::Silent),
            "rule_check" => Ok(Policy::RuleCheck),
            value => {
                let limit = value
                    .strip_prefix("solution_check:")
                    .and_then(|n| n.parse::<u32>().ok())
                    .filter(|n| (1..=99).contains(n));
                limit.map(Policy::SolutionCheck).ok_or_else(|| {
                    GameError::position(
                        "mistake_policy must be silent, rule_check, or solution_check:1..99",
                    )
                })
            }
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Policy {
    Silent,
    RuleCheck,
    SolutionCheck(u32),
}

/// One-based Sudoku coordinates and digits.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(
    crate = "gfa_core::serde",
    tag = "type",
    rename_all = "snake_case",
    deny_unknown_fields
)]
#[schemars(crate = "gfa_core::schemars")]
pub enum Action {
    /// Place a digit in a mutable cell.
    Place {
        /// Row, 1..=size.
        row: u8,
        /// Column, 1..=size.
        col: u8,
        /// Digit, 1..=size.
        digit: u8,
    },
    /// Clear a nonempty mutable cell.
    Erase {
        /// Row, 1..=size.
        row: u8,
        /// Column, 1..=size.
        col: u8,
    },
    /// Add or remove a free candidate note in an empty cell.
    Note {
        /// Row, 1..=size.
        row: u8,
        /// Column, 1..=size.
        col: u8,
        /// Digit, 1..=size.
        digit: u8,
        /// True to add; false to remove.
        add: bool,
    },
}

impl Action {
    pub(crate) fn coordinates(&self) -> (u8, u8) {
        match *self {
            Self::Place { row, col, .. }
            | Self::Erase { row, col }
            | Self::Note { row, col, .. } => (row, col),
        }
    }
}

/// Rule-level puzzle result.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(crate = "gfa_core::serde", rename_all = "snake_case")]
#[schemars(crate = "gfa_core::schemars")]
pub enum Outcome {
    /// The completed grid matches the unique solution.
    Solved,
    /// The configured number of wrong placements was reached.
    MistakeLimit,
    /// The placement/erasure cap was reached.
    MaxMoves,
}

/// Complete replay state. Solution and private scoring metrics are never observed.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(crate = "gfa_core::serde", deny_unknown_fields)]
pub struct State {
    pub(crate) config: Config,
    pub(crate) puzzle: Vec<u8>,
    pub(crate) grid: Vec<u8>,
    pub(crate) notes: Vec<u16>,
    pub(crate) grade: Grade,
    pub(crate) moves: u32,
    pub(crate) wrong: u32,
    pub(crate) attempts: BTreeMap<u32, u32>,
    pub(crate) erasures: u32,
    pub(crate) actions: u32,
    // Pure per-value memoization; deserialization always starts unvalidated.
    #[serde(skip)]
    pub(crate) validated_puzzle: OnceLock<Vec<u8>>,
}

/// Public board; the same view is available to players and spectators.
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(crate = "gfa_core::serde", deny_unknown_fields)]
#[schemars(crate = "gfa_core::schemars")]
pub(crate) struct BoardView {
    pub size: u8,
    pub grid: Vec<u8>,
    pub givens: Vec<bool>,
    pub notes: Vec<u16>,
    pub conflicts: Vec<usize>,
    pub grade: Grade,
    pub moves: u32,
    pub mistakes: Option<u32>,
    pub attempts: BTreeMap<u32, u32>,
    pub erasures: u32,
    pub actions: u32,
    pub outcome: Option<Outcome>,
}

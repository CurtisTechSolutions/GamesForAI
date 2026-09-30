//! Human solving techniques, applied from simplest to hardest until closure.
mod patterns;

use crate::{grid::boxes, solve, Grid};
use gfa_core::{schemars::JsonSchema, serde::{Deserialize, Serialize}, GameError};

/// The hardest technique required by the deterministic logical solver.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, JsonSchema)]
#[serde(crate = "gfa_core::serde", rename_all = "snake_case")]
#[schemars(crate = "gfa_core::schemars")]
pub enum Difficulty {
    /// Naked and hidden singles.
    #[default]
    Easy,
    /// Pairs and locked candidates (pointing/claiming).
    Medium,
    /// Triples and X-wing.
    Hard,
    /// A search or chain beyond the implemented logical rules is required.
    Expert,
}

/// A reproducible explanation of a solving step.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, JsonSchema)]
#[serde(crate = "gfa_core::serde", rename_all = "snake_case")]
#[schemars(crate = "gfa_core::schemars")]
pub enum Technique {
    /// One candidate remains in a cell.
    NakedSingle,
    /// One cell in a unit can hold the digit.
    HiddenSingle,
    /// Two cells collectively use exactly two digits.
    NakedPair,
    /// Two digits appear in exactly two cells in a unit.
    HiddenPair,
    /// A box confines a digit to a row or column.
    Pointing,
    /// A row or column confines a digit to a box.
    Claiming,
    /// Three cells collectively use exactly three digits.
    NakedTriple,
    /// Three digits appear in exactly three cells in a unit.
    HiddenTriple,
    /// Two rows or columns share exactly two candidate positions.
    XWing,
    /// No supported logical step remains; use the exact-cover solution.
    Guess,
}

impl Technique {
    fn difficulty(self) -> Difficulty {
        match self {
            Self::NakedSingle | Self::HiddenSingle => Difficulty::Easy,
            Self::NakedPair | Self::HiddenPair | Self::Pointing | Self::Claiming => Difficulty::Medium,
            Self::NakedTriple | Self::HiddenTriple | Self::XWing => Difficulty::Hard,
            Self::Guess => Difficulty::Expert,
        }
    }
}

/// A digit assignment or candidate elimination at one zero-based cell.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(crate = "gfa_core::serde", deny_unknown_fields)]
#[schemars(crate = "gfa_core::schemars")]
pub struct Candidate {
    /// Zero-based row-major cell.
    pub cell: usize,
    /// One-based digit.
    pub digit: u8,
}

/// One explainable step; candidates use zero-based row-major cells.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(crate = "gfa_core::serde", deny_unknown_fields)]
#[schemars(crate = "gfa_core::schemars")]
pub struct Hint {
    /// Rule justifying the changes.
    pub technique: Technique,
    /// Digits to place.
    pub placements: Vec<Candidate>,
    /// Candidates that can be removed.
    pub eliminations: Vec<Candidate>,
}

/// Stored grade and the techniques needed to solve the puzzle.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(crate = "gfa_core::serde", deny_unknown_fields)]
#[schemars(crate = "gfa_core::schemars")]
pub struct Grade {
    /// Hardest technique required.
    pub difficulty: Difficulty,
    /// Techniques used, in increasing difficulty order.
    pub techniques: Vec<Technique>,
    /// Number of logical steps before completion or search.
    pub logical_steps: u32,
}

/// Grade a uniquely solvable puzzle, never using clue count as a proxy.
pub fn grade(grid: &Grid) -> Result<Grade, GameError> {
    unique(grid)?;
    let mut logic = Logic::new(grid)?;
    let mut techniques = std::collections::BTreeSet::new();
    let mut logical_steps = 0;
    while logic.grid.cells.contains(&0) {
        let Some(step) = logic.next() else {
            techniques.insert(Technique::Guess);
            break;
        };
        techniques.insert(step.technique);
        logical_steps += 1;
        logic.apply(&step);
    }
    let difficulty = techniques.iter().map(|t| t.difficulty()).max().unwrap_or_default();
    Ok(Grade { difficulty, techniques: techniques.into_iter().collect(), logical_steps })
}

/// Find the next logical placement, including prerequisite eliminations.
/// When logic is exhausted, the returned step explicitly identifies search.
pub fn hint(grid: &Grid) -> Result<Option<Vec<Hint>>, GameError> {
    let solution = unique(grid)?;
    if !grid.cells.contains(&0) { return Ok(None); }
    let mut logic = Logic::new(grid)?;
    let mut steps = Vec::new();
    while let Some(step) = logic.next() {
        let placed = !step.placements.is_empty();
        logic.apply(&step);
        steps.push(step);
        if placed { return Ok(Some(steps)); }
    }
    if let Some(cell) = grid.cells.iter().position(|&digit| digit == 0) {
        steps.push(Hint {
            technique: Technique::Guess,
            placements: vec![Candidate { cell, digit: solution.cells[cell] }],
            eliminations: vec![],
        });
    }
    Ok(Some(steps))
}

fn unique(grid: &Grid) -> Result<Grid, GameError> {
    let result = solve(grid);
    if result.count != 1 {
        return Err(GameError::position("Puzzle must have exactly one solution"));
    }
    result.first.ok_or_else(|| GameError::position("Puzzle has no solution"))
}

struct Logic {
    grid: Grid,
    masks: Vec<u16>,
    units: Vec<Vec<usize>>,
}

impl Logic {
    fn new(grid: &Grid) -> Result<Self, GameError> {
        let n = usize::from(grid.size);
        let (height, width) = boxes(grid.size)?;
        let mut units = Vec::with_capacity(3 * n);
        for row in 0..n { units.push((0..n).map(|col| row * n + col).collect()); }
        for col in 0..n { units.push((0..n).map(|row| row * n + col).collect()); }
        for band in 0..n / height {
            for stack in 0..n / width {
                units.push((0..height).flat_map(|r| (0..width).map(move |c|
                    (band * height + r) * n + stack * width + c)).collect());
            }
        }
        Ok(Self {
            grid: grid.clone(),
            masks: grid.cells.iter().enumerate().map(|(i, &v)| if v == 0 { grid.candidates(i) } else { 0 }).collect(),
            units,
        })
    }

    fn next(&self) -> Option<Hint> {
        self.singles()
            .or_else(|| self.naked(2))
            .or_else(|| self.hidden(2))
            .or_else(|| self.locked())
            .or_else(|| self.naked(3))
            .or_else(|| self.hidden(3))
            .or_else(|| self.x_wing())
    }

    fn singles(&self) -> Option<Hint> {
        for (cell, &mask) in self.masks.iter().enumerate() {
            if mask.count_ones() == 1 {
                return Some(placement(Technique::NakedSingle, cell, mask.trailing_zeros() as u8 + 1));
            }
        }
        for unit in &self.units {
            for digit in 1..=self.grid.size {
                let cells: Vec<_> = unit.iter().copied().filter(|&i| self.masks[i] & (1 << (digit - 1)) != 0).collect();
                if cells.len() == 1 { return Some(placement(Technique::HiddenSingle, cells[0], digit)); }
            }
        }
        None
    }

    fn apply(&mut self, hint: &Hint) {
        for candidate in &hint.eliminations {
            self.masks[candidate.cell] &= !(1 << (candidate.digit - 1));
        }
        for candidate in &hint.placements {
            self.grid.cells[candidate.cell] = candidate.digit;
            self.masks[candidate.cell] = 0;
            for unit in &self.units {
                if unit.contains(&candidate.cell) {
                    for &cell in unit { self.masks[cell] &= !(1 << (candidate.digit - 1)); }
                }
            }
        }
    }
}

fn placement(technique: Technique, cell: usize, digit: u8) -> Hint {
    Hint { technique, placements: vec![Candidate { cell, digit }], eliminations: vec![] }
}

fn elimination(technique: Technique, masks: &[u16], cells: impl Iterator<Item=usize>, remove: u16) -> Option<Hint> {
    let mut eliminations = Vec::new();
    for cell in cells {
        let mut bits = masks[cell] & remove;
        while bits != 0 {
            let digit = bits.trailing_zeros() as u8 + 1;
            eliminations.push(Candidate { cell, digit });
            bits &= bits - 1;
        }
    }
    (!eliminations.is_empty()).then_some(Hint { technique, placements: vec![], eliminations })
}

#[cfg(test)]
mod tests;

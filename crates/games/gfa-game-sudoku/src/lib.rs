//! Pure Sudoku puzzle validation, exact-cover solving, and seeded generation.
mod cover;
mod grid;
mod grading;

use gfa_core::{GameError, SeededRng};
pub use grid::Grid;
pub use grading::{grade, hint, Candidate, Difficulty, Grade, Hint, Technique};

/// A uniqueness check stops at two solutions; the first solution is retained.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Solutions {
    /// Zero (unsatisfiable), one (unique), or two (multiple).
    pub count: u8,
    /// First complete grid, when at least one solution exists.
    pub first: Option<Grid>,
}

/// Solve a consistent grid using minimum-column Algorithm X and dancing links.
/// Conflicting givens are rejected by `Grid` before reaching the solver.
pub fn solve(grid: &Grid) -> Solutions {
    cover::solve(grid)
}

/// Generate a reproducible, locally minimal puzzle with exactly one solution.
///
/// Remove clues in seeded order, retaining each removal only after an exact
/// uniqueness check. Difficulty is not inferred from the number of clues.
pub fn generate(size: u8, seed: u64) -> Result<Grid, GameError> {
    let (height, width) = grid::boxes(size)?;
    let n = usize::from(size);
    let mut rng = SeededRng::new(seed);
    let mut digits: Vec<u8> = (1..=size).collect();
    rng.shuffle(&mut digits);
    let mut rows = shuffled_groups(n, height, &mut rng);
    let mut cols = shuffled_groups(n, width, &mut rng);
    // Independently permuting bands, stacks, and their members preserves boxes.
    if size == 9 && rng.index(2) == Some(1) {
        std::mem::swap(&mut rows, &mut cols);
    }
    let mut cells = Vec::with_capacity(n * n);
    for row in rows {
        for &col in &cols {
            cells.push(digits[(row * width + row / height + col) % n]);
        }
    }
    let mut puzzle = Grid::from_cells(size, cells)?;
    let mut order: Vec<usize> = (0..n * n).collect();
    rng.shuffle(&mut order);
    for index in order {
        let digit = puzzle.cells[index];
        puzzle.cells[index] = 0;
        if solve(&puzzle).count != 1 {
            puzzle.cells[index] = digit;
        }
    }
    Ok(puzzle)
}

fn shuffled_groups(n: usize, group: usize, rng: &mut SeededRng) -> Vec<usize> {
    let mut groups: Vec<usize> = (0..n / group).collect();
    rng.shuffle(&mut groups);
    let mut result = Vec::with_capacity(n);
    for index in groups {
        let mut members: Vec<usize> = (index * group..(index + 1) * group).collect();
        rng.shuffle(&mut members);
        result.extend(members);
    }
    result
}

#[cfg(test)]
mod tests;

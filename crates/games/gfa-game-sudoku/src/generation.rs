use crate::{generate, grade, grid::boxes, shuffled_groups, Difficulty, Grade, Grid};
use gfa_core::{GameError, SeededRng};

// Reproducible seed fixtures, discovered with examples/discover-grades.rs.
// These are generated locally, without third-party puzzle data.
const SEEDS: &[(u8, Difficulty, u64)] = &[
    (4, Difficulty::Easy, 0),
    (6, Difficulty::Easy, 0),
    (6, Difficulty::Medium, 171),
    (6, Difficulty::Expert, 7),
    (9, Difficulty::Easy, 0),
    (9, Difficulty::Medium, 5),
    (9, Difficulty::Hard, 185),
    (9, Difficulty::Expert, 3),
];

/// Generation grades supported by the embedded, solver-verified seed corpus.
/// Arbitrary imported puzzles may be graded at any difficulty.
pub fn supported_difficulties(size: u8) -> Vec<Difficulty> {
    SEEDS
        .iter()
        .filter_map(|&(n, difficulty, _)| (n == size).then_some(difficulty))
        .collect()
}

/// Generate a unique puzzle at a verified technique grade.
///
/// Seeded digit, row, column, band and stack permutations vary a generated base
/// puzzle. Each candidate is regraded, so solving-order differences cannot
/// silently change the advertised difficulty. Unsupported size/grade pairs
/// return a clear error instead of mislabelling a puzzle.
pub fn generate_graded(
    size: u8,
    difficulty: Difficulty,
    seed: u64,
) -> Result<(Grid, Grade), GameError> {
    let source = SEEDS.iter().find(|&&(n, grade, _)| n == size && grade == difficulty)
        .ok_or_else(|| GameError::position(format!(
            "No generated {difficulty:?} corpus for size {size}; use a supported grade or supply a puzzle"
        )))?;
    let base = generate(size, source.2)?;
    let (height, width) = boxes(size)?;
    let n = usize::from(size);
    let mut rng = SeededRng::new(seed);
    for _ in 0..32 {
        let rows = shuffled_groups(n, height, &mut rng);
        let cols = shuffled_groups(n, width, &mut rng);
        let mut digits: Vec<u8> = (1..=size).collect();
        rng.shuffle(&mut digits);
        let transpose = height == width && rng.index(2) == Some(1);
        let mut cells = Vec::with_capacity(n * n);
        for &row in &rows {
            for &col in &cols {
                let index = if transpose {
                    col * n + row
                } else {
                    row * n + col
                };
                let value = base.cells[index];
                cells.push(if value == 0 {
                    0
                } else {
                    digits[usize::from(value - 1)]
                });
            }
        }
        let candidate = Grid::from_cells(size, cells)?;
        let assessment = grade(&candidate)?;
        if assessment.difficulty == difficulty {
            return Ok((candidate, assessment));
        }
    }
    // The source itself is an exact, deterministic fallback when transformed
    // candidates follow a different logical path. Never return a false label.
    let assessment = grade(&base)?;
    if assessment.difficulty != difficulty {
        return Err(GameError::position(
            "Generated puzzle corpus requires recalibration",
        ));
    }
    Ok((base, assessment))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::solve;
    use std::collections::BTreeSet;

    #[test]
    fn every_supported_grade_is_unique_seeded_and_verified(
    ) -> Result<(), Box<dyn std::error::Error>> {
        for &(size, difficulty, _) in SEEDS {
            let mut boards = BTreeSet::new();
            for seed in 0..24 {
                let generated = generate_graded(size, difficulty, seed)?;
                assert_eq!(generated, generate_graded(size, difficulty, seed)?);
                assert_eq!(generated.1.difficulty, difficulty);
                assert_eq!(grade(&generated.0)?, generated.1);
                assert_eq!(solve(&generated.0).count, 1);
                boards.insert(generated.0.notation());
            }
            assert!(boards.len() >= 20, "{size}: {difficulty:?}");
        }
        assert!(generate_graded(4, Difficulty::Hard, 0).is_err());
        assert!(generate_graded(6, Difficulty::Hard, 0).is_err());
        assert!(generate_graded(5, Difficulty::Easy, 0).is_err());
        assert_eq!(supported_difficulties(9).len(), 4);
        assert_eq!(supported_difficulties(5).len(), 0);
        Ok(())
    }
}

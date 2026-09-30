//! Reproduce the generated puzzle corpus used to seed difficulty selection.
use gfa_game_sudoku::{generate, grade, Difficulty};
use std::{collections::BTreeSet, error::Error};

fn main() -> Result<(), Box<dyn Error>> {
    for size in [4, 6, 9] {
        let mut seen = BTreeSet::new();
        for seed in 0..2048 {
            let puzzle = generate(size, seed)?;
            let assessment = grade(&puzzle)?;
            if seen.insert(assessment.difficulty) {
                println!("GFA_PUZZLE:{size}:{seed}:{:?}:{}", assessment.difficulty, puzzle.notation());
            }
            if seen.len() == 4 || size == 4 && seen.contains(&Difficulty::Easy) { break; }
        }
        println!("GFA_GRADES:{size}:{seen:?}");
    }
    Ok(())
}

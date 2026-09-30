use super::*;
use crate::generate;

type TestResult = Result<(), Box<dyn std::error::Error>>;

#[test]
fn grades_singles_and_labels_search_explicitly() -> TestResult {
    let easy = Grid::parse(9, "530070000600195000098000060800060003400803001700020006060000280000419005000080079")?;
    let result = grade(&easy)?;
    assert_eq!(result.difficulty, Difficulty::Easy);
    assert!(result.logical_steps > 0);
    assert!(result.techniques.iter().all(|t| t.difficulty() == Difficulty::Easy));
    let expert = Grid::parse(9, "800000000003600000070090200050007000000045700000100030001000068008500010090000400")?;
    assert_eq!(grade(&expert)?.difficulty, Difficulty::Expert);
    assert!(grade(&Grid::parse(4, "................")?).is_err());
    assert_eq!(hint(&solve(&easy).first.ok_or("solution")?)?, None);
    Ok(())
}

#[test]
fn every_generated_logical_step_preserves_the_unique_solution() -> TestResult {
    let mut observed = std::collections::BTreeSet::new();
    for seed in 0..128 {
        let grid = generate(9, seed)?;
        let solution = unique(&grid)?;
        let mut logic = Logic::new(&grid)?;
        while let Some(step) = logic.next() {
            assert!(!step.placements.is_empty() || !step.eliminations.is_empty());
            for item in &step.placements { assert_eq!(solution.cells[item.cell], item.digit); }
            for item in &step.eliminations { assert_ne!(solution.cells[item.cell], item.digit); }
            observed.insert(step.technique);
            logic.apply(&step);
            assert!(logic.masks.iter().enumerate().all(|(i, &m)| logic.grid.cells[i] != 0 || m != 0));
        }
        assert_eq!(grade(&grid)?, grade(&grid)?);
        let steps = hint(&grid)?.ok_or("hint")?;
        assert!(!steps.last().ok_or("last")?.placements.is_empty());
    }
    assert!(observed.contains(&Technique::NakedSingle));
    assert!(observed.contains(&Technique::HiddenSingle));
    assert!(observed.iter().any(|t| t.difficulty() == Difficulty::Medium));
    Ok(())
}

fn synthetic() -> Result<Logic, GameError> {
    let mut logic = Logic::new(&Grid::parse(9, &".".repeat(81))?)?;
    logic.masks.fill(0);
    Ok(logic)
}

#[test]
fn subsets_and_locked_candidates_eliminate_only_outside_the_pattern() -> TestResult {
    let mut logic = synthetic()?;
    logic.masks[0] = 0b11;
    logic.masks[1] = 0b11;
    logic.masks[2] = 0b111;
    let pair = logic.naked(2).ok_or("pair")?;
    assert_eq!(pair.technique, Technique::NakedPair);
    assert_eq!(pair.eliminations, vec![Candidate { cell: 2, digit: 1 }, Candidate { cell: 2, digit: 2 }]);

    logic.masks[0] = 0b1011;
    logic.masks[1] = 0b10011;
    logic.masks[2] = 0b11100;
    let hidden = logic.hidden(2).ok_or("hidden pair")?;
    assert_eq!(hidden.technique, Technique::HiddenPair);
    assert_eq!(hidden.eliminations, vec![Candidate { cell: 0, digit: 4 }, Candidate { cell: 1, digit: 5 }]);

    let mut logic = synthetic()?;
    logic.masks[0] = 0b011;
    logic.masks[1] = 0b110;
    logic.masks[2] = 0b101;
    logic.masks[3] = 0b1111;
    assert_eq!(logic.naked(3).ok_or("triple")?.technique, Technique::NakedTriple);

    let mut logic = synthetic()?;
    logic.masks[0] = 1;
    logic.masks[1] = 1;
    logic.masks[4] = 1;
    let step = logic.locked().ok_or("pointing")?;
    assert_eq!(step.technique, Technique::Pointing);
    assert_eq!(step.eliminations, vec![Candidate { cell: 4, digit: 1 }]);
    Ok(())
}

#[test]
fn x_wing_works_in_both_orientations() -> TestResult {
    for transpose in [false, true] {
        let index = |r, c| if transpose { c * 9 + r } else { r * 9 + c };
        let mut logic = synthetic()?;
        for row in [0, 3] {
            for col in [1, 7] { logic.masks[index(row, col)] = 1; }
        }
        logic.masks[index(5, 1)] = 3;
        logic.masks[index(5, 4)] = 3;
        let step = logic.x_wing().ok_or("x-wing")?;
        assert_eq!(step.technique, Technique::XWing);
        assert_eq!(step.eliminations, vec![Candidate { cell: index(5, 1), digit: 1 }]);
    }
    Ok(())
}

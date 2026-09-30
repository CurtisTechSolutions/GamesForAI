use super::{outcome, puzzle_solution};
use crate::{hint, Action, Grid, State, Technique};
use gfa_core::{serde_json, Advice, AdviceInfo, ErrorCode, GameError, PlayerId};

pub(super) fn recommend(
    state: &State,
    player: PlayerId,
) -> Result<Option<Advice<Action>>, GameError> {
    if player != 0 {
        return Err(GameError::new(
            ErrorCode::NotYourTurn,
            "Only the solver seat may request a hint",
            "Use seat 0.",
        ));
    }
    if outcome(state).is_some() {
        return Ok(None);
    }
    let size = usize::from(state.config.size);
    let solution = puzzle_solution(state)?;
    if let Some(cell) = state
        .grid
        .iter()
        .zip(solution)
        .position(|(&digit, &correct)| digit != 0 && digit != correct)
    {
        let row = (cell / size + 1) as u8;
        let col = (cell % size + 1) as u8;
        return Ok(Some(Advice {
            action: Action::Erase { row, col },
            info: AdviceInfo {
                summary: format!("Erase r{row}c{col}: its value differs from the puzzle's verified unique solution."),
                technique: "solution_check".into(),
                is_guess: false,
                details: serde_json::json!({"cell":cell,"row":row,"col":col}),
            },
        }));
    }
    let grid = Grid::from_cells(state.config.size, state.grid.clone())?;
    let Some(steps) = hint(&grid)? else {
        return Ok(None);
    };
    let step = steps
        .last()
        .ok_or_else(|| GameError::position("Hint has no steps"))?;
    let placement = step
        .placements
        .first()
        .ok_or_else(|| GameError::position("Hint has no placement"))?;
    let row = (placement.cell / size + 1) as u8;
    let col = (placement.cell % size + 1) as u8;
    let digit = placement.digit;
    let technique = serde_json::to_value(step.technique)?
        .as_str()
        .ok_or_else(|| GameError::position("Hint technique is invalid"))?
        .to_owned();
    let is_guess = step.technique == Technique::Guess;
    let summary = if is_guess {
        format!("Search-backed guess: place {digit} at r{row}c{col}. No supported logical placement remains.")
    } else {
        format!(
            "Use {} to place {digit} at r{row}c{col}.",
            technique.replace('_', " ")
        )
    };
    Ok(Some(Advice {
        action: Action::Place { row, col, digit },
        info: AdviceInfo {
            summary,
            technique,
            is_guess,
            details: serde_json::json!({"steps":steps}),
        },
    }))
}

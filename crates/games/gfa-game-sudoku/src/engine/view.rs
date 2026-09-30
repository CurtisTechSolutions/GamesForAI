use super::{actions::conflict, from_grid, outcome, Sudoku};
use crate::{model::{BoardView, Policy}, Config, Grid, State};
use gfa_core::{serde_json, Game, GameError, Observation, Tensor};

fn board(state: &State) -> BoardView {
    let reveal_mistakes = matches!(state.config.policy(), Ok(Policy::SolutionCheck(_))) ||
        !state.grid.contains(&0) || outcome(state).is_some();
    BoardView {
        size: state.config.size,
        grid: state.grid.clone(),
        givens: state.puzzle.iter().map(|&digit| digit != 0).collect(),
        notes: state.notes.clone(),
        conflicts: state.grid.iter().enumerate().filter_map(|(i, &digit)|
            (digit != 0 && conflict(state, i, digit).is_some()).then_some(i)).collect(),
        grade: state.grade.clone(),
        moves: state.moves,
        mistakes: reveal_mistakes.then_some(state.wrong),
        attempts: state.attempts.clone(),
        erasures: state.erasures,
        actions: state.actions,
        outcome: outcome(state),
    }
}

pub(super) fn observe(state: &State) -> Observation {
    if Sudoku::validate_state(state).is_err() {
        return Observation { text: "Invalid Sudoku state.".into(), json: serde_json::Value::Null, tensor: None };
    }
    let n = usize::from(state.config.size);
    let (height, width) = match crate::grid::boxes(state.config.size) {
        Ok(value) => value, Err(_) => return Observation { text: "Invalid board size.".into(), json: serde_json::Value::Null, tensor: None },
    };
    let view = board(state);
    let mut text = String::from("    ");
    for col in 1..=n {
        text.push_str(&format!("c{col} "));
        if col % width == 0 && col != n { text.push_str("| "); }
    }
    text.push('\n');
    for row in 0..n {
        if row != 0 && row % height == 0 { text.push_str(&format!("    {}\n", "-".repeat(n * 3 + (n / width - 1) * 2))); }
        text.push_str(&format!("r{}  ", row + 1));
        for col in 0..n {
            let digit = state.grid[row * n + col];
            text.push(if digit == 0 { '.' } else { char::from(b'0' + digit) });
            text.push_str("  ");
            if (col + 1) % width == 0 && col + 1 != n { text.push_str("| "); }
        }
        text.push('\n');
    }
    text.push_str(&format!("{} cells left; {} moves; ",
        state.grid.iter().filter(|&&digit| digit == 0).count(), state.moves));
    if let Some(mistakes) = view.mistakes { text.push_str(&format!("{mistakes} mistakes; ")); }
    text.push_str(match view.outcome {
        Some(crate::Outcome::Solved) => "Solved.",
        Some(crate::Outcome::MistakeLimit) => "Mistake limit reached.",
        Some(crate::Outcome::MaxMoves) => "Move/action cap reached.",
        None => "Solver to move (seat 0).",
    });
    let mut values = Vec::with_capacity((n + 1) * n * n);
    for plane in 0..=n {
        for (i, &digit) in state.grid.iter().enumerate() {
            let active = if plane == n { state.puzzle[i] != 0 } else { usize::from(digit) == plane + 1 };
            values.push(if active { 1.0 } else { 0.0 });
        }
    }
    Observation {
        text, json: serde_json::json!(view),
        tensor: Some(Tensor { shape: vec![n + 1, n, n], values }),
    }
}

pub(super) fn reconstruct(config: &Config, observation: &Observation) -> Result<State, GameError> {
    let view: BoardView = serde_json::from_value(observation.json.clone())
        .map_err(|error| GameError::position(error.to_string()))?;
    let len = usize::from(config.size).pow(2);
    if view.size != config.size || view.grid.len() != len || view.givens.len() != len ||
        view.givens.iter().zip(&view.grid).any(|(&fixed, &digit)| fixed && digit == 0) {
        return Err(GameError::position("Observation dimensions or givens are invalid"));
    }
    let puzzle = Grid::from_cells(config.size, view.grid.iter().zip(&view.givens)
        .map(|(&digit, &fixed)| if fixed { digit } else { 0 }).collect())?;
    let mut state = from_grid(config, puzzle)?;
    state.grid = view.grid;
    state.notes = view.notes;
    state.moves = view.moves;
    state.erasures = view.erasures;
    state.actions = view.actions;
    state.attempts = view.attempts;
    let n = usize::from(config.size);
    let mut wrong = 0_u64;
    for (&index, &count) in &state.attempts {
        let crate::Action::Place { row, col, digit } = super::actions::from_index(&state, index)? else {
            return Err(GameError::position("Attempt history contains a non-placement"));
        };
        if state.solution[usize::from(row - 1) * n + usize::from(col - 1)] != digit {
            wrong += u64::from(count);
        }
    }
    state.wrong = u32::try_from(wrong).map_err(|_| GameError::position("Mistake count overflow"))?;
    Sudoku::validate_state(&state)?;
    if board(&state).grade != view.grade || observe(&state).json != observation.json {
        return Err(GameError::position("Observation metadata disagrees with the reconstructed puzzle"));
    }
    Ok(state)
}

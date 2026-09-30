use super::{outcome, Sudoku};
use crate::{model::Policy, Action, Outcome, State};
use gfa_core::{ErrorCode, Game, GameError, StepEvents};

pub(super) fn conflict(state: &State, cell: usize, digit: u8) -> Option<usize> {
    let n = usize::from(state.config.size);
    let (height, width) = crate::grid::boxes(state.config.size).ok()?;
    state.grid.iter().enumerate().find_map(|(other, &value)| {
        (other != cell
            && value == digit
            && (other / n == cell / n
                || other % n == cell % n
                || (other / n / height == cell / n / height
                    && other % n / width == cell % n / width)))
            .then_some(other)
    })
}

pub(super) fn legal(state: &State, player: u8) -> Vec<Action> {
    if player != 0 || Sudoku::validate_state(state).is_err() || outcome(state).is_some() {
        return vec![];
    }
    let n = usize::from(state.config.size);
    let check_rules = state.config.policy() == Ok(Policy::RuleCheck);
    let mut actions = Vec::new();
    for (i, &value) in state.grid.iter().enumerate() {
        if state.puzzle[i] != 0 {
            continue;
        }
        let row = (i / n + 1) as u8;
        let col = (i % n + 1) as u8;
        for digit in 1..=state.config.size {
            if digit != value && (!check_rules || conflict(state, i, digit).is_none()) {
                actions.push(Action::Place { row, col, digit });
            }
        }
        if value != 0 {
            actions.push(Action::Erase { row, col });
        }
        if state.config.allow_notes && value == 0 {
            for digit in 1..=state.config.size {
                actions.push(Action::Note {
                    row,
                    col,
                    digit,
                    add: state.notes[i] & (1 << (digit - 1)) == 0,
                });
            }
        }
    }
    actions
}

pub(super) fn apply(
    state: &mut State,
    player: u8,
    action: &Action,
) -> Result<StepEvents, GameError> {
    Sudoku::validate_state(state)?;
    if outcome(state).is_some() {
        return Err(GameError::new(
            ErrorCode::MatchFinished,
            "Puzzle has ended",
            "Start a new puzzle.",
        ));
    }
    if player != 0 {
        return Err(GameError::new(
            ErrorCode::NotYourTurn,
            "Sudoku has only seat 0",
            "Play as seat 0.",
        ));
    }
    let (row, col) = action.coordinates();
    if !(1..=state.config.size).contains(&row) || !(1..=state.config.size).contains(&col) {
        return Err(GameError::illegal("Coordinates must lie inside the board"));
    }
    let cell = usize::from(row - 1) * usize::from(state.config.size) + usize::from(col - 1);
    if state.puzzle[cell] != 0 {
        return Err(GameError::illegal("A starting given cannot be edited"));
    }
    match *action {
        Action::Place { digit, .. } => {
            if !(1..=state.config.size).contains(&digit) || state.grid[cell] == digit {
                return Err(GameError::illegal(
                    "Place a different digit from 1 through the grid size",
                ));
            }
            if state.config.policy()? == Policy::RuleCheck {
                if let Some(other) = conflict(state, cell, digit) {
                    let n = usize::from(state.config.size);
                    return Err(GameError::illegal(format!(
                        "Digit {digit} conflicts with r{}c{}",
                        other / n + 1,
                        other % n + 1
                    )));
                }
            }
            let wrong = super::puzzle_solution(state)?[cell] != digit;
            state.grid[cell] = digit;
            state.notes[cell] = 0;
            state.moves += 1;
            state.wrong += u32::from(wrong);
            *state.attempts.entry(index(action)).or_default() += 1;
        }
        Action::Erase { .. } => {
            if state.grid[cell] == 0 {
                return Err(GameError::illegal("The cell is already empty"));
            }
            state.grid[cell] = 0;
            state.moves += 1;
            state.erasures += 1;
        }
        Action::Note { digit, add, .. } => {
            if !state.config.allow_notes
                || state.grid[cell] != 0
                || !(1..=state.config.size).contains(&digit)
            {
                return Err(GameError::illegal(
                    "Notes require an empty mutable cell and an allowed digit",
                ));
            }
            let bit = 1 << (digit - 1);
            if (state.notes[cell] & bit == 0) != add {
                return Err(GameError::illegal(
                    "That note already has the requested value",
                ));
            }
            state.notes[cell] ^= bit;
        }
    }
    state.actions += 1;
    Ok(StepEvents {
        truncated: outcome(state) == Some(Outcome::MaxMoves),
        ..StepEvents::default()
    })
}

pub(super) fn notation(action: &Action) -> String {
    match *action {
        Action::Place { row, col, digit } => format!("r{row}c{col}={digit}"),
        Action::Erase { row, col } => format!("r{row}c{col}=0"),
        Action::Note {
            row,
            col,
            digit,
            add,
        } => format!("r{row}c{col}{}{digit}", if add { '+' } else { '-' }),
    }
}

fn bad_action() -> GameError {
    GameError::new(
        ErrorCode::UnparseableAction,
        "Invalid Sudoku action notation or index",
        "Use r1c1=1, r1c1=0, r1c1+1, or r1c1-1 within the board.",
    )
}

pub(super) fn parse(state: &State, text: &str) -> Result<Action, GameError> {
    let bytes = text.as_bytes();
    if bytes.len() != 6
        || bytes[0] != b'r'
        || bytes[2] != b'c'
        || !(b'1'..=b'9').contains(&bytes[1])
        || !(b'1'..=b'9').contains(&bytes[3])
        || !bytes[5].is_ascii_digit()
    {
        return Err(bad_action());
    }
    let row = bytes[1] - b'0';
    let col = bytes[3] - b'0';
    let digit = bytes[5] - b'0';
    if row > state.config.size || col > state.config.size || digit > state.config.size {
        return Err(bad_action());
    }
    match (bytes[4], digit) {
        (b'=', 0) => Ok(Action::Erase { row, col }),
        (b'=', _) => Ok(Action::Place { row, col, digit }),
        (operator @ (b'+' | b'-'), 1..=9) => Ok(Action::Note {
            row,
            col,
            digit,
            add: operator == b'+',
        }),
        _ => Err(bad_action()),
    }
}

pub(super) fn index(action: &Action) -> u32 {
    let (row, col) = action.coordinates();
    if !(1..=9).contains(&row) || !(1..=9).contains(&col) {
        return u32::MAX;
    }
    let cell = u32::from(row - 1) * 9 + u32::from(col - 1);
    match *action {
        Action::Place { digit, .. } if (1..=9).contains(&digit) => cell * 9 + u32::from(digit - 1),
        Action::Erase { .. } => 729 + cell,
        Action::Note { digit, add, .. } if (1..=9).contains(&digit) => {
            810 + (cell * 9 + u32::from(digit - 1)) * 2 + u32::from(!add)
        }
        _ => u32::MAX,
    }
}

pub(super) fn from_index(state: &State, index: u32) -> Result<Action, GameError> {
    let (cell, digit) = if index < 729 {
        (index / 9, index % 9 + 1)
    } else if index < 810 {
        (index - 729, 0)
    } else if index < 2268 {
        ((index - 810) / 18, (index - 810) / 2 % 9 + 1)
    } else {
        return Err(bad_action());
    };
    let row = (cell / 9 + 1) as u8;
    let col = (cell % 9 + 1) as u8;
    if row > state.config.size || col > state.config.size || digit > u32::from(state.config.size) {
        return Err(bad_action());
    }
    Ok(if index < 729 {
        Action::Place {
            row,
            col,
            digit: digit as u8,
        }
    } else if index < 810 {
        Action::Erase { row, col }
    } else {
        Action::Note {
            row,
            col,
            digit: digit as u8,
            add: index.is_multiple_of(2),
        }
    })
}

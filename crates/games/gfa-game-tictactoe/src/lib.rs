//! Deterministic, validated 3×3 Tic-Tac-Toe.
use gfa_core::{
    schema, ErrorCode, Game, GameError, GameSpec, Information, Observation, PlayerId, StepEvents,
    Tensor, TurnStructure, Viewer,
};
use gfa_core::schemars::JsonSchema;
use gfa_core::serde::{Deserialize, Serialize};
use gfa_core::serde_json;

const LINES: [[usize; 3]; 8] = [
    [0, 1, 2], [3, 4, 5], [6, 7, 8],
    [0, 3, 6], [1, 4, 7], [2, 5, 8],
    [0, 4, 8], [2, 4, 6],
];

/// Standard Tic-Tac-Toe has no configurable variants.
#[derive(Clone, Debug, Default, Serialize, Deserialize, JsonSchema)]
#[serde(crate = "gfa_core::serde", deny_unknown_fields)]
#[schemars(crate = "gfa_core::schemars")]
pub struct Config {}

/// Board cells in row-major order and the next seat.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(crate = "gfa_core::serde", deny_unknown_fields)]
#[schemars(crate = "gfa_core::schemars")]
pub struct State {
    board: [Option<PlayerId>; 9],
    to_move: PlayerId,
}

/// One-based row and column.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(crate = "gfa_core::serde", deny_unknown_fields)]
#[schemars(crate = "gfa_core::schemars")]
pub struct Action {
    /// Row 1 through 3, top to bottom.
    #[schemars(range(min = 1, max = 3))]
    pub row: u8,
    /// Column 1 through 3, left to right.
    #[schemars(range(min = 1, max = 3))]
    pub col: u8,
}

/// Stateless engine entry point.
pub struct TicTacToe;

fn wins(board: &[Option<PlayerId>; 9], player: PlayerId) -> bool {
    LINES.iter().any(|line| line.iter().all(|&i| board[i] == Some(player)))
}

impl Game for TicTacToe {
    type State = State;
    type Action = Action;
    type Config = Config;

    fn spec() -> GameSpec {
        GameSpec {
            id: "tictactoe".into(),
            name: "Tic-Tac-Toe".into(),
            summary: "Make three in a row on a 3×3 board; perfect play draws.".into(),
            engine_version: env!("CARGO_PKG_VERSION").into(),
            info_version: "1.0.0".into(),
            num_players: [2, 2],
            seat_names: vec!["X".into(), "O".into()],
            turn_structure: TurnStructure::Sequential,
            information: Information::Perfect,
            stochastic: false,
            max_game_length: 9,
            reward_range: [-1.0, 1.0],
            action_space_size: 9,
            rules_markdown: "X (seat 0) moves first. Alternate placing your mark in an empty cell. Three marks in a row, column, or diagonal wins (+1); the loser receives -1. A full board without a winner draws (0 each). Rows and columns run from 1 to 3, top-left first. Use r1c1 for the top-left cell, r2c2 for the centre, r3c3 for the bottom-right. Occupied cells are illegal. Play ends immediately on a win.".into(),
            action_notation: "r<row>c<col>, with row and col in 1..3; index=(row-1)*3+col-1".into(),
            position_notation: "JSON: {\"board\":[null,...],\"to_move\":0}; 9 cells, 0=X, 1=O".into(),
            config_schema: schema::<Config>(),
            action_schema: schema::<Action>(),
            observation_schema: schema::<State>(),
        }
    }

    fn new_initial_state(_: &Config, _: u64) -> Result<State, GameError> {
        Ok(State { board: [None; 9], to_move: 0 })
    }

    fn validate_state(state: &State) -> Result<(), GameError> {
        if state.board.iter().flatten().any(|&p| p > 1) {
            return Err(GameError::position("Cells may contain only null, 0 (X), or 1 (O)"));
        }
        let x = state.board.iter().filter(|&&p| p == Some(0)).count();
        let o = state.board.iter().filter(|&&p| p == Some(1)).count();
        if !(x == o || x == o + 1) || state.to_move != u8::from(x > o) {
            return Err(GameError::position("Mark counts and to_move must match alternating play"));
        }
        let x_wins = wins(&state.board, 0);
        let o_wins = wins(&state.board, 1);
        if (x_wins && o_wins) || (x_wins && x != o + 1) || (o_wins && x != o) {
            return Err(GameError::position("The winner and mark counts are inconsistent"));
        }
        if x_wins || o_wins {
            let winner = u8::from(o_wins);
            let possible_last_move = (0..9).any(|i| {
                let mut before = state.board;
                if before[i] != Some(winner) {
                    return false;
                }
                before[i] = None;
                !wins(&before, 0) && !wins(&before, 1)
            });
            if !possible_last_move {
                return Err(GameError::position("Marks were played after the game ended"));
            }
        }
        Ok(())
    }

    fn current_players(state: &State) -> Vec<PlayerId> {
        if Self::is_terminal(state) { vec![] } else { vec![state.to_move] }
    }

    fn legal_actions(state: &State, player: PlayerId) -> Vec<Action> {
        if player != state.to_move || Self::is_terminal(state) {
            return vec![];
        }
        state.board.iter().enumerate().filter_map(|(i, cell)| {
            cell.is_none().then_some(Action { row: (i / 3 + 1) as u8, col: (i % 3 + 1) as u8 })
        }).collect()
    }

    fn apply(state: &mut State, player: PlayerId, action: &Action) -> Result<StepEvents, GameError> {
        if Self::is_terminal(state) {
            return Err(GameError::new(ErrorCode::MatchFinished, "The game has ended", "Create or fork a match."));
        }
        if player != state.to_move {
            return Err(GameError::new(ErrorCode::NotYourTurn, "The other seat must act", "Wait for your turn."));
        }
        if !(1..=3).contains(&action.row) || !(1..=3).contains(&action.col) {
            return Err(GameError::illegal("Rows and columns must be in 1..3"));
        }
        let i = usize::from(action.row - 1) * 3 + usize::from(action.col - 1);
        if state.board[i].is_some() {
            return Err(GameError::illegal("That cell is occupied"));
        }
        state.board[i] = Some(player);
        state.to_move = 1 - player;
        Ok(StepEvents::default())
    }

    fn is_terminal(state: &State) -> bool {
        wins(&state.board, 0) || wins(&state.board, 1) || state.board.iter().all(Option::is_some)
    }

    fn returns(state: &State) -> Vec<f64> {
        if wins(&state.board, 0) {
            vec![1.0, -1.0]
        } else if wins(&state.board, 1) {
            vec![-1.0, 1.0]
        } else {
            vec![0.0, 0.0]
        }
    }

    fn observe(state: &State, _: Viewer) -> Observation {
        let mut text = String::from("    c1 c2 c3\n");
        for row in 0..3 {
            text.push_str(&format!("r{}  ", row + 1));
            for col in 0..3 {
                text.push(match state.board[row * 3 + col] {
                    Some(0) => 'X',
                    Some(1) => 'O',
                    _ => '.',
                });
                text.push(' ');
            }
            text.push('\n');
        }
        if Self::is_terminal(state) {
            text.push_str(if wins(&state.board, 0) { "X wins." } else if wins(&state.board, 1) { "O wins." } else { "Draw." });
        } else {
            text.push_str(if state.to_move == 0 { "X to move (seat 0)." } else { "O to move (seat 1)." });
        }
        let values = (0..3).flat_map(|plane| {
            state.board.iter().map(move |&cell| {
                if if plane == 2 { cell.is_none() } else { cell == Some(plane) } { 1.0 } else { 0.0 }
            })
        }).collect();
        Observation {
            text,
            json: serde_json::json!(state),
            tensor: Some(Tensor { shape: vec![3, 3, 3], values }),
        }
    }

    fn action_to_string(_: &State, action: &Action) -> String {
        format!("r{}c{}", action.row, action.col)
    }

    fn action_from_string(_: &State, text: &str) -> Result<Action, GameError> {
        let b = text.as_bytes();
        if b.len() == 4 && b[0] == b'r' && b[2] == b'c' && (b'1'..=b'3').contains(&b[1]) && (b'1'..=b'3').contains(&b[3]) {
            Ok(Action { row: b[1] - b'0', col: b[3] - b'0' })
        } else {
            Err(GameError::new(ErrorCode::UnparseableAction, "Expected r1c1 through r3c3", "Use one of the listed legal actions."))
        }
    }

    fn action_to_index(action: &Action) -> u32 {
        u32::from(action.row.saturating_sub(1)) * 3 + u32::from(action.col.saturating_sub(1))
    }

    fn action_from_index(_: &State, index: u32) -> Result<Action, GameError> {
        if index < 9 {
            Ok(Action { row: (index / 3 + 1) as u8, col: (index % 3 + 1) as u8 })
        } else {
            Err(GameError::new(ErrorCode::UnparseableAction, "Index must be 0..8", "Choose an index from legal_actions."))
        }
    }

    fn state_to_notation(state: &State) -> Result<String, GameError> {
        Self::validate_state(state)?;
        Ok(serde_json::to_string(state)?)
    }

    fn state_from_notation(_: &Config, notation: &str) -> Result<State, GameError> {
        let state = serde_json::from_str(notation).map_err(|e| GameError::position(format!("{e}")))?;
        Self::validate_state(&state)?;
        Ok(state)
    }
}

#[cfg(test)]
mod tests;

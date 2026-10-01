//! Standard Connect Four with bitboard rules and validated gravity.
use gfa_core::schemars::JsonSchema;
use gfa_core::serde::{Deserialize, Serialize};
use gfa_core::serde_json;
use gfa_core::{
    schema, ErrorCode, Game, GameError, GameSpec, Information, Observation, PlayerId, StepEvents,
    Tensor, TurnStructure, Viewer,
};
use std::collections::HashSet;

const PLAYABLE: u64 = 0x0000_fdfb_f7ef_dfbf;

/// Standard 7-column, 6-row Connect Four.
#[derive(Clone, Debug, Default, Serialize, Deserialize, JsonSchema)]
#[serde(crate = "gfa_core::serde", deny_unknown_fields)]
#[schemars(crate = "gfa_core::schemars")]
pub struct Config {}

/// Bitboards use seven bits per column, with six playable bits and one sentinel.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(crate = "gfa_core::serde", deny_unknown_fields)]
pub struct State {
    boards: [u64; 2],
    to_move: PlayerId,
}

/// One-based column, from left to right.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(crate = "gfa_core::serde", deny_unknown_fields)]
#[schemars(crate = "gfa_core::schemars")]
pub struct Action {
    /// Drop a disc into column 1 through 7.
    #[schemars(range(min = 1, max = 7))]
    pub column: u8,
}

#[derive(Serialize, Deserialize, JsonSchema)]
#[serde(crate = "gfa_core::serde")]
#[schemars(crate = "gfa_core::schemars")]
struct Board {
    rows: Vec<Vec<Option<PlayerId>>>,
    to_move: PlayerId,
}

/// Pure Connect Four implementation.
pub struct Connect4;

fn won(bits: u64) -> bool {
    [1, 7, 6, 8].into_iter().any(|shift| {
        let pairs = bits & (bits >> shift);
        pairs & (pairs >> (2 * shift)) != 0
    })
}

fn occupied(state: &State) -> u64 {
    state.boards[0] | state.boards[1]
}

fn reachable(boards: [u64; 2], next: usize, dead: &mut HashSet<[u64; 2]>) -> bool {
    if boards == [0, 0] {
        return true;
    }
    if dead.contains(&boards) {
        return false;
    }
    let all = boards[0] | boards[1];
    let last = 1 - next;
    for col in 0..7 {
        let column = (all >> (col * 7)) & 63;
        if column == 0 {
            continue;
        }
        let height = column.count_ones();
        let bit = 1_u64 << (col * 7 + height - 1);
        if boards[last] & bit == 0 {
            continue;
        }
        let mut before = boards;
        before[last] ^= bit;
        if !won(before[0]) && !won(before[1]) && reachable(before, last, dead) {
            return true;
        }
    }
    dead.insert(boards);
    false
}

impl Game for Connect4 {
    type State = State;
    type Action = Action;
    type Config = Config;

    fn spec() -> GameSpec {
        GameSpec {
            id: "connect4".into(),
            name: "Connect Four".into(),
            summary: "Drop discs into seven columns and connect four in any direction.".into(),
            engine_version: env!("CARGO_PKG_VERSION").into(),
            info_version: "1.1.0".into(),
            num_players: [2, 2],
            seat_names: vec!["Red".into(), "Yellow".into()],
            turn_structure: TurnStructure::Sequential,
            information: Information::Perfect,
            stochastic: false,
            max_game_length: 42,
            reward_range: [-1.0, 1.0],
            action_space_size: 7,
            rules_markdown: "Red (seat 0) starts. Alternate dropping a disc into a non-full column on a 7×6 board. Discs fall to the lowest empty cell. Four connected discs horizontally, vertically, or diagonally win (+1); the loser receives -1. If all 42 cells fill without a winner, both receive 0. Use a column number 1–7: 1 drops at the far left, 4 in the centre, and 7 at the far right. Indices are 0–6. A full column is illegal; play stops immediately on a win.".into(),
            action_notation: "One ASCII digit 1..7; JSON {column: n}; index=column-1".into(),
            position_notation: "JSON {boards:[red_bits,yellow_bits],to_move:seat}; bit=column*7+row from bottom, zero-based; each column's seventh bit is unused.".into(),
            config_schema: schema::<Config>(),
            action_schema: schema::<Action>(),
            observation_schema: schema::<Board>(),
        }
    }

    fn play_guide() -> Option<gfa_core::PlayGuide> {
        Some(gfa_core::PlayGuide {
            objective: "Connect four of your discs horizontally, vertically, or diagonally to win. A full board without a winner draws. Play stops immediately on a win.".into(),
            observation: "Text and JSON rows run top to bottom; columns 1..7 run left to right. R is Red (seat 0), Y is Yellow (seat 1), and . is empty. JSON rows contains six rows of seven cells (null, 0, or 1); to_move is the next seat. Both seats and spectators see the entire board.".into(),
            tensor: "Float32 [3,6,7], planes first, rows top to bottom: Red discs, Yellow discs, and next seat (all 0 for Red or 1 for Yellow).".into(),
            rewards: "Terminal returns are +1 for the winner, -1 for the loser, and 0 each for a draw. Returns before termination are 0. Shaped rewards are not supported.".into(),
            config: "No configurable fields. Use {} for the standard 7-column, 6-row board.".into(),
            solved: "Solved: the first player can force a win on the standard board.".into(),
            common_mistakes: vec!["Choose a column, not a row: gravity selects the lowest empty cell.".into(), "Columns are one-based; discrete indices are zero-based.".into(), "A full column is illegal.".into()],
            strategy_notes: vec!["Check immediate wins and blocks before other moves.".into(), "Central columns participate in more possible four-disc lines.".into()],
        })
    }

    fn new_initial_state(_: &Config, _: u64) -> Result<State, GameError> {
        Ok(State {
            boards: [0, 0],
            to_move: 0,
        })
    }

    fn state_from_observation(
        _: &Config,
        observation: &Observation,
        _: Viewer,
        _: u64,
    ) -> Result<State, GameError> {
        let board: Board = serde_json::from_value(observation.json.clone())
            .map_err(|error| GameError::position(error.to_string()))?;
        if board.rows.len() != 6 || board.rows.iter().any(|row| row.len() != 7) {
            return Err(GameError::position("Expected six rows of seven cells"));
        }
        let mut state = State {
            boards: [0, 0],
            to_move: board.to_move,
        };
        for (row, cells) in board.rows.iter().enumerate() {
            for (col, cell) in cells.iter().enumerate() {
                if let Some(seat) = cell {
                    let bits = state
                        .boards
                        .get_mut(usize::from(*seat))
                        .ok_or_else(|| GameError::position("Cell seat must be 0 or 1"))?;
                    *bits |= 1_u64 << (col * 7 + 5 - row);
                }
            }
        }
        Self::validate_state(&state)?;
        Ok(state)
    }

    fn validate_state(state: &State) -> Result<(), GameError> {
        let all = occupied(state);
        if all & !PLAYABLE != 0 || state.boards[0] & state.boards[1] != 0 {
            return Err(GameError::position(
                "Bitboards overlap or contain cells outside the board",
            ));
        }
        let red = state.boards[0].count_ones();
        let yellow = state.boards[1].count_ones();
        if !(red == yellow || red == yellow + 1) || state.to_move != u8::from(red > yellow) {
            return Err(GameError::position(
                "Disc counts and next seat do not match alternating play",
            ));
        }
        for col in 0..7 {
            let column = (all >> (col * 7)) & 63;
            if column & (column + 1) != 0 {
                return Err(GameError::position(format!(
                    "Column {} has a floating disc",
                    col + 1
                )));
            }
        }
        if !reachable(
            state.boards,
            usize::from(state.to_move),
            &mut HashSet::new(),
        ) {
            return Err(GameError::position(
                "No legal alternating history reaches this position",
            ));
        }
        Ok(())
    }

    fn current_players(state: &State) -> Vec<PlayerId> {
        if Self::is_terminal(state) {
            vec![]
        } else {
            vec![state.to_move]
        }
    }

    fn legal_actions(state: &State, player: PlayerId) -> Vec<Action> {
        if player != state.to_move || Self::is_terminal(state) {
            return vec![];
        }
        (1..=7)
            .filter_map(|column| {
                (occupied(state) & (1_u64 << ((column - 1) * 7 + 5)) == 0)
                    .then_some(Action { column })
            })
            .collect()
    }

    fn apply(
        state: &mut State,
        player: PlayerId,
        action: &Action,
    ) -> Result<StepEvents, GameError> {
        if Self::is_terminal(state) {
            return Err(GameError::new(
                ErrorCode::MatchFinished,
                "The game has ended",
                "Create or fork a match.",
            ));
        }
        if player != state.to_move || player > 1 {
            return Err(GameError::new(
                ErrorCode::NotYourTurn,
                "The other seat must act",
                "Wait for your turn.",
            ));
        }
        if !(1..=7).contains(&action.column) {
            return Err(GameError::illegal("Column must be 1..7"));
        }
        let shift = u32::from(action.column - 1) * 7;
        let height = ((occupied(state) >> shift) & 63).count_ones();
        if height >= 6 {
            return Err(GameError::illegal("This column is full"));
        }
        state.boards[usize::from(player)] |= 1_u64 << (shift + height);
        state.to_move = 1 - player;
        Ok(StepEvents::default())
    }

    fn is_terminal(state: &State) -> bool {
        won(state.boards[0]) || won(state.boards[1]) || occupied(state) == PLAYABLE
    }

    fn returns(state: &State) -> Vec<f64> {
        if won(state.boards[0]) {
            vec![1.0, -1.0]
        } else if won(state.boards[1]) {
            vec![-1.0, 1.0]
        } else {
            vec![0.0, 0.0]
        }
    }

    fn observe(state: &State, viewer: Viewer) -> Observation {
        let mut rows = Vec::new();
        let mut text = String::from("   1 2 3 4 5 6 7\n");
        for row in (0..6).rev() {
            let mut cells = Vec::new();
            text.push_str(&format!("{}  ", 6 - row));
            for col in 0..7 {
                let bit = 1_u64 << (col * 7 + row);
                let cell = if state.boards[0] & bit != 0 {
                    Some(0)
                } else if state.boards[1] & bit != 0 {
                    Some(1)
                } else {
                    None
                };
                cells.push(cell);
                text.push(match cell {
                    Some(0) => 'R',
                    Some(1) => 'Y',
                    _ => '.',
                });
                text.push(' ');
            }
            rows.push(cells);
            text.push('\n');
        }
        text.push_str(if Self::is_terminal(state) {
            if won(state.boards[0]) {
                "Red wins."
            } else if won(state.boards[1]) {
                "Yellow wins."
            } else {
                "Draw."
            }
        } else if state.to_move == 0 {
            "Red to move (seat 0)."
        } else {
            "Yellow to move (seat 1)."
        });
        Observation {
            text,
            json: serde_json::json!(Board {
                rows,
                to_move: state.to_move
            }),
            tensor: Self::observe_tensor(state, viewer),
        }
    }

    fn observe_tensor(state: &State, _: Viewer) -> Option<Tensor> {
        let mut values = vec![0.0; 126];
        for row in 0..6 {
            for col in 0..7 {
                let bit = 1_u64 << (col * 7 + 5 - row);
                let index = row * 7 + col;
                values[index] = f32::from(state.boards[0] & bit != 0);
                values[42 + index] = f32::from(state.boards[1] & bit != 0);
                values[84 + index] = f32::from(state.to_move);
            }
        }
        Some(Tensor {
            shape: vec![3, 6, 7],
            values,
        })
    }

    fn action_to_string(_: &State, action: &Action) -> String {
        action.column.to_string()
    }

    fn action_from_string(_: &State, text: &str) -> Result<Action, GameError> {
        if text.len() == 1 && (b'1'..=b'7').contains(&text.as_bytes()[0]) {
            Ok(Action {
                column: text.as_bytes()[0] - b'0',
            })
        } else {
            Err(GameError::new(
                ErrorCode::UnparseableAction,
                "Expected a column 1..7",
                "Choose one of legal_actions.",
            ))
        }
    }

    fn action_to_index(action: &Action) -> u32 {
        u32::from(action.column.saturating_sub(1))
    }

    fn action_from_index(_: &State, index: u32) -> Result<Action, GameError> {
        if index < 7 {
            Ok(Action {
                column: index as u8 + 1,
            })
        } else {
            Err(GameError::new(
                ErrorCode::UnparseableAction,
                "Index must be 0..6",
                "Choose an index from legal_actions.",
            ))
        }
    }

    fn public_position(state: &State) -> Result<Option<String>, GameError> {
        Self::state_to_notation(state).map(Some)
    }

    fn state_to_notation(state: &State) -> Result<String, GameError> {
        Self::validate_state(state)?;
        Ok(serde_json::to_string(state)?)
    }

    fn state_from_notation(_: &Config, notation: &str) -> Result<State, GameError> {
        let state =
            serde_json::from_str(notation).map_err(|e| GameError::position(format!("{e}")))?;
        Self::validate_state(&state)?;
        Ok(state)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numeric_projection_matches_public_board_through_complete_games() -> Result<(), GameError> {
        for seed in 0..16 {
            let mut state = Connect4::new_initial_state(&Config {}, seed)?;
            let mut rng = gfa_core::SeededRng::new(seed);
            loop {
                let board: Board =
                    serde_json::from_value(Connect4::observe(&state, Viewer::Spectator).json)?;
                for viewer in [
                    Viewer::Player(0),
                    Viewer::Player(1),
                    Viewer::Spectator,
                    Viewer::Omniscient,
                ] {
                    let tensor = Connect4::observe_tensor(&state, viewer)
                        .ok_or_else(|| GameError::position("missing tensor"))?;
                    assert_eq!(tensor.shape, vec![3, 6, 7]);
                    for row in 0..6 {
                        for col in 0..7 {
                            let index = row * 7 + col;
                            assert_eq!(
                                tensor.values[index],
                                f32::from(board.rows[row][col] == Some(0))
                            );
                            assert_eq!(
                                tensor.values[42 + index],
                                f32::from(board.rows[row][col] == Some(1))
                            );
                            assert_eq!(tensor.values[84 + index], f32::from(board.to_move));
                        }
                    }
                }
                if Connect4::is_terminal(&state) {
                    break;
                }
                let seat = state.to_move;
                let actions = Connect4::legal_actions(&state, seat);
                let index = rng
                    .index(actions.len())
                    .ok_or_else(|| GameError::illegal("missing move"))?;
                Connect4::apply(&mut state, seat, &actions[index])?;
            }
        }
        Ok(())
    }

    #[test]
    fn conforms() -> Result<(), Box<dyn std::error::Error>> {
        gfa_testkit::conformance::<Connect4>()
    }

    #[test]
    fn validates_geometry_and_reachability() {
        assert_eq!(PLAYABLE, (0..7).map(|c| 63_u64 << (7 * c)).sum::<u64>());
        assert!(Connect4::validate_state(&State {
            boards: [2, 0],
            to_move: 1
        })
        .is_err());
        // Yellow below red cannot be the first two moves.
        assert!(Connect4::validate_state(&State {
            boards: [2, 1],
            to_move: 0
        })
        .is_err());
        assert!(Connect4::validate_state(&State {
            boards: [64, 0],
            to_move: 1
        })
        .is_err());
    }

    #[test]
    fn vertical_win_ends_play() -> Result<(), GameError> {
        let mut state = Connect4::new_initial_state(&Config {}, 0)?;
        for column in [1, 2, 1, 2, 1, 2, 1] {
            let player = state.to_move;
            Connect4::apply(&mut state, player, &Action { column })?;
        }
        assert_eq!(Connect4::returns(&state), vec![1.0, -1.0]);
        Connect4::validate_state(&state)?;
        assert!(Connect4::legal_actions(&state, 1).is_empty());
        Ok(())
    }
}

#[cfg(test)]
mod tensor_tests {
    use super::*;

    #[test]
    fn planes_match_public_board_orientation_and_turn() -> Result<(), GameError> {
        let mut state = Connect4::new_initial_state(&Config::default(), 0)?;
        for column in [1, 2, 1] {
            let seat = Connect4::current_players(&state)[0];
            Connect4::apply(&mut state, seat, &Action { column })?;
        }
        let observation = Connect4::observe(&state, Viewer::Spectator);
        assert_eq!(observation, Connect4::observe(&state, Viewer::Player(0)));
        assert_eq!(observation, Connect4::observe(&state, Viewer::Player(1)));
        let board: Board = serde_json::from_value(observation.json)?;
        let tensor = observation
            .tensor
            .ok_or_else(|| GameError::position("missing tensor"))?;
        assert_eq!(tensor.shape, vec![3, 6, 7]);
        assert_eq!(tensor.values.len(), 126);
        assert_eq!(tensor.values[4 * 7], 1.0);
        assert_eq!(tensor.values[5 * 7], 1.0);
        assert_eq!(tensor.values[42 + 5 * 7 + 1], 1.0);
        for (row, cells) in board.rows.iter().enumerate() {
            for (col, cell) in cells.iter().enumerate() {
                for plane in 0..2 {
                    assert_eq!(
                        tensor.values[plane * 42 + row * 7 + col],
                        if *cell == Some(plane as PlayerId) {
                            1.0
                        } else {
                            0.0
                        }
                    );
                }
            }
        }
        assert!(tensor.values[84..].iter().all(|value| *value == 1.0));
        Ok(())
    }
}

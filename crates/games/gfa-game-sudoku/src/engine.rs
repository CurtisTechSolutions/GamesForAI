mod actions;
mod view;

use crate::{
    generate_graded, grade,
    model::{BoardView, Policy},
    solve, Action, Config, Grid, Outcome, State,
};
use gfa_core::{
    schema, serde_json, ErrorCode, Game, GameError, GameSpec, Information, Observation, PlayerId,
    StepEvents, TurnStructure, Viewer,
};
use std::sync::OnceLock;

/// Single-player Sudoku with reproducible puzzles and explicit mistake policies.
pub struct Sudoku;

pub(super) fn outcome(state: &State) -> Option<Outcome> {
    if state.grid == state.solution {
        Some(Outcome::Solved)
    } else if matches!(state.config.policy(), Ok(Policy::SolutionCheck(limit)) if state.wrong >= limit)
    {
        Some(Outcome::MistakeLimit)
    } else if state.moves >= state.config.max_moves.unwrap_or(200) || state.actions >= 50000 {
        Some(Outcome::MaxMoves)
    } else {
        None
    }
}

pub(super) fn from_grid(config: &Config, puzzle: Grid) -> Result<State, GameError> {
    config.validate()?;
    if let Some(notation) = &config.puzzle {
        if Grid::parse(config.size, notation)? != puzzle {
            return Err(GameError::position(
                "Configured puzzle disagrees with the imported grid",
            ));
        }
    }
    let assessment = grade(&puzzle)?;
    let solution = solve(&puzzle)
        .first
        .ok_or_else(|| GameError::position("Puzzle has no solution"))?;
    let state = State {
        config: config.clone(),
        puzzle: puzzle.cells().to_vec(),
        solution: solution.cells().to_vec(),
        grid: puzzle.cells().to_vec(),
        notes: vec![0; puzzle.cells().len()],
        grade: assessment,
        moves: 0,
        wrong: 0,
        attempts: Default::default(),
        erasures: 0,
        actions: 0,
        validated_puzzle: OnceLock::new(),
    };
    let _ = state.validated_puzzle.set(());
    Ok(state)
}

impl Game for Sudoku {
    type State = State;
    type Action = Action;
    type Config = Config;

    fn spec() -> GameSpec {
        GameSpec {
            id: "sudoku".into(), name: "Sudoku".into(),
            summary: "Fill a unique-solution puzzle using row, column and box constraints.".into(),
            engine_version: env!("CARGO_PKG_VERSION").into(), info_version: "1.0.0".into(),
            num_players: [1, 1], seat_names: vec!["Solver".into()],
            turn_structure: TurnStructure::Sequential, information: Information::Perfect,
            stochastic: false, max_game_length: 50000, reward_range: [0.0, 1.0], action_space_size: 2268,
            rules_markdown: include_str!("engine/rules.md").into(),
            action_notation: "r<row>c<col>=<digit>, =0 to erase, +digit/-digit for notes. Coordinates are one-based.".into(),
            position_notation: "size²-character puzzle grid (./0 empty); JSON State for lossless in-progress exports including notes and metrics.".into(),
            config_schema: schema::<Config>(), action_schema: schema::<Action>(), observation_schema: schema::<BoardView>(),
        }
    }

    fn play_guide() -> Option<gfa_core::PlayGuide> {
        Some(gfa_core::PlayGuide {
            objective: "Fill every cell so each row, column and box contains digits 1..size exactly once. Fixed givens cannot be edited. Only seat 0 acts; there is no opposing seat.".into(),
            observation: "JSON grid is row-major with 0 empty; givens marks fixed cells; notes are digit bit masks. conflicts lists zero-based conflicting cells. The text grid includes row/column coordinates and box separators. No solution is included.".into(),
            tensor: "Shape [size+1,size,size]: one plane per digit and one givens plane; empty cells have no active digit plane.".into(),
            rewards: "Return +1 for solved, otherwise 0. max_moves counts placements and erasures; notes are free. Mistakes are hidden in silent mode until the grid is full or play ends.".into(),
            config: "size=4,6,9 (default 9); difficulty=easy,medium,hard,expert; puzzle optional; mistake_policy=silent,rule_check,solution_check:n; allow_notes=true; max_moves defaults to 200. Generated 4x4 is easy; 6x6 is easy/medium/expert; 9x9 supports all grades. Imported puzzles are graded independently.".into(),
            solved: "Each accepted puzzle has exactly one solution, verified by an exact-cover solver. Difficulty records the hardest logical technique needed.".into(),
            common_mistakes: vec!["Givens cannot be changed. Coordinates and digits start at 1.".into(), "Notes do not place a digit. r3c5+7 adds a note; r3c5=7 places 7; r3c5=0 erases.".into(), "Silent mode permits conflicting placements; check the complete grid before assuming it is solved.".into()],
            strategy_notes: vec!["Find cells with one candidate, then digits with one possible cell in a row, column or box.".into(), "Use candidate notes to track pairs and locked candidates.".into()],
        })
    }

    fn new_initial_state(config: &Config, seed: u64) -> Result<State, GameError> {
        config.validate().map_err(|mut error| {
            error.code = ErrorCode::InvalidConfig;
            error
        })?;
        let puzzle = match &config.puzzle {
            Some(puzzle) => Grid::parse(config.size, puzzle)?,
            None => generate_graded(config.size, config.difficulty, seed)?.0,
        };
        from_grid(config, puzzle)
    }

    fn state_from_observation(
        config: &Config,
        observation: &Observation,
        _: Viewer,
        _: u64,
    ) -> Result<State, GameError> {
        view::reconstruct(config, observation)
    }

    fn validate_state(state: &State) -> Result<(), GameError> {
        state.config.validate()?;
        let n = usize::from(state.config.size);
        let len = n * n;
        if state.grid.len() != len
            || state.puzzle.len() != len
            || state.solution.len() != len
            || state.notes.len() != len
            || state.grid.iter().any(|&d| d > state.config.size)
            || state.moves > state.config.max_moves.unwrap_or(200)
            || state.actions > 50000
            || state.erasures > state.moves
            || state.wrong > state.moves - state.erasures
            || state.actions < state.moves
        {
            return Err(GameError::position(
                "Grid dimensions, digits or move counters are invalid",
            ));
        }
        let mut placements = 0_u64;
        let mut wrong = 0_u64;
        for (&index, &count) in &state.attempts {
            let Action::Place { row, col, digit } = actions::from_index(state, index)? else {
                return Err(GameError::position(
                    "Attempt counts must refer to placements",
                ));
            };
            if count == 0 {
                return Err(GameError::position("Attempt counts must be positive"));
            }
            placements += u64::from(count);
            let cell = usize::from(row - 1) * n + usize::from(col - 1);
            if state.solution[cell] != digit {
                wrong += u64::from(count);
            }
        }
        if placements != u64::from(state.moves - state.erasures) || wrong != u64::from(state.wrong)
        {
            return Err(GameError::position(
                "Placement history disagrees with scoring counters",
            ));
        }
        let all = (1_u16 << state.config.size) - 1;
        for i in 0..len {
            if state.puzzle[i] != 0 && state.grid[i] != state.puzzle[i]
                || state.notes[i] & !all != 0
                || state.grid[i] != 0 && state.notes[i] != 0
                || !state.config.allow_notes && state.notes[i] != 0
            {
                return Err(GameError::position(
                    "Givens or candidate notes are inconsistent",
                ));
            }
        }
        if state.validated_puzzle.get().is_none() {
            let puzzle = Grid::from_cells(state.config.size, state.puzzle.clone())?;
            let solution = solve(&puzzle);
            if solution.count != 1
                || solution.first.as_ref().map(Grid::cells) != Some(state.solution.as_slice())
                || grade(&puzzle)? != state.grade
            {
                return Err(GameError::position(
                    "Puzzle solution or difficulty metadata is inconsistent",
                ));
            }
            if let Some(notation) = &state.config.puzzle {
                if Grid::parse(state.config.size, notation)? != puzzle {
                    return Err(GameError::position(
                        "Configured puzzle disagrees with the givens",
                    ));
                }
            }
            let _ = state.validated_puzzle.set(());
        }
        Ok(())
    }

    fn current_players(state: &State) -> Vec<PlayerId> {
        if outcome(state).is_none() {
            vec![0]
        } else {
            vec![]
        }
    }

    fn legal_actions(state: &State, player: PlayerId) -> Vec<Action> {
        actions::legal(state, player)
    }

    fn apply(
        state: &mut State,
        player: PlayerId,
        action: &Action,
    ) -> Result<StepEvents, GameError> {
        actions::apply(state, player, action)
    }

    fn is_terminal(state: &State) -> bool {
        matches!(
            outcome(state),
            Some(Outcome::Solved | Outcome::MistakeLimit)
        )
    }

    fn returns(state: &State) -> Vec<f64> {
        vec![if outcome(state) == Some(Outcome::Solved) {
            1.0
        } else {
            0.0
        }]
    }

    fn observe(state: &State, _: Viewer) -> Observation {
        view::observe(state)
    }

    fn action_to_string(_: &State, action: &Action) -> String {
        actions::notation(action)
    }

    fn action_from_string(state: &State, text: &str) -> Result<Action, GameError> {
        actions::parse(state, text)
    }

    fn action_to_index(action: &Action) -> u32 {
        actions::index(action)
    }

    fn action_from_index(state: &State, index: u32) -> Result<Action, GameError> {
        actions::from_index(state, index)
    }

    fn state_to_notation(state: &State) -> Result<String, GameError> {
        Self::validate_state(state)?;
        // A bare grid cannot preserve givens after edits, notes, or mistake history.
        if state.actions == 0 && state.grid == state.puzzle && state.notes.iter().all(|&n| n == 0) {
            Ok(Grid::from_cells(state.config.size, state.grid.clone())?.notation())
        } else {
            Ok(serde_json::to_string(state)?)
        }
    }

    fn state_from_notation(config: &Config, text: &str) -> Result<State, GameError> {
        let state = if text.starts_with('{') {
            let state: State = serde_json::from_str(text)
                .map_err(|error| GameError::position(error.to_string()))?;
            if state.config != *config {
                return Err(GameError::position(
                    "Position config differs from the requested config",
                ));
            }
            state
        } else {
            from_grid(config, Grid::parse(config.size, text)?)?
        };
        Self::validate_state(&state)?;
        Ok(state)
    }
}

#[cfg(test)]
mod tests;

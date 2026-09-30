mod actions;
mod advice;
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
    if puzzle_solution(state).is_ok_and(|solution| state.grid.as_slice() == solution) {
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
    let _ = state.validated_puzzle.set(solution.cells().to_vec());
    Ok(state)
}

pub(super) fn puzzle_solution(state: &State) -> Result<&[u8], GameError> {
    if state.validated_puzzle.get().is_none() {
        let puzzle = Grid::from_cells(state.config.size, state.puzzle.clone())?;
        let solved = solve(&puzzle);
        if solved.count != 1 || grade(&puzzle)? != state.grade {
            return Err(GameError::position(
                "Puzzle uniqueness or difficulty metadata is inconsistent",
            ));
        }
        if let Some(notation) = &state.config.puzzle {
            if Grid::parse(state.config.size, notation)? != puzzle {
                return Err(GameError::position(
                    "Configured puzzle disagrees with the givens",
                ));
            }
        }
        let solution = solved
            .first
            .ok_or_else(|| GameError::position("Puzzle has no solution"))?;
        let _ = state.validated_puzzle.set(solution.cells().to_vec());
    }
    state
        .validated_puzzle
        .get()
        .map(Vec::as_slice)
        .ok_or_else(|| GameError::position("Puzzle solution is unavailable"))
}

impl Game for Sudoku {
    type State = State;
    type Action = Action;
    type Config = Config;

    fn spec() -> GameSpec {
        GameSpec {
            id: "sudoku".into(), name: "Sudoku".into(),
            summary: "A one-seat constraint puzzle with one verified solution.".into(),
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

    fn supports_reference_advice() -> bool {
        true
    }

    fn reference_advice(
        state: &State,
        player: PlayerId,
    ) -> Result<Option<gfa_core::Advice<Action>>, GameError> {
        Self::validate_state(state)?;
        advice::recommend(state, player)
    }

    fn play_guide() -> Option<gfa_core::PlayGuide> {
        Some(gfa_core::PlayGuide {
            objective: "Complete every row, column and box with digits 1..size. Givens are fixed. Only seat 0 acts; no opponent.".into(),
            observation: "JSON grid is row-major, 0 empty. givens marks fixed cells; notes are digit bit masks; conflicts lists cell indices. attempts counts public placements, not correctness. Text has coordinates and box separators.".into(),
            tensor: "Shape [size+1,size,size]: digit one-hot planes plus givens; 0/1 values.".into(),
            rewards: "Solve: +1; fail or truncate: 0. Notes cost no moves.".into(),
            config: "Generation: 4x4 easy; 6x6 easy/medium/expert; 9x9 all grades. Imports get their actual grade. Active options and defaults are listed below.".into(),
            solved: "Exactly one solution. Difficulty is the hardest required technique, verified by the solver.".into(),
            common_mistakes: vec!["Givens are fixed. Coordinates start at 1.".into(), "+7 adds a note; =7 places 7; =0 erases.".into(), "Silent placements may conflict; full does not mean solved.".into()],
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
        let solution = puzzle_solution(state)?;
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
            if solution[cell] != digit {
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

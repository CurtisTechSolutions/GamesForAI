use crate::{planning_position, Bound, EnginePool, Info, Score, SearchResult, Settings, UciError};
use gfa_core::{
    serde_json::{json, Value},
    ActionChoice, AdviceInfo, ChoiceInfo, Clock, DynGame, GameError, Opponent, OpponentError,
    PlayerTurn, SearchLimits, Viewer,
};
use std::{sync::Arc, time::Instant};

/// Observation-only UCI player; no live match state or match RNG reaches its engine.
pub struct UciOpponent {
    pool: Arc<EnginePool>,
    game: Arc<dyn DynGame>,
    config: Value,
    settings: Settings,
}
impl UciOpponent {
    /// Bind an installed pool to an injected rules engine and normalized game configuration.
    pub fn new(
        pool: Arc<EnginePool>,
        game: Arc<dyn DynGame>,
        config: Value,
        settings: Settings,
    ) -> Result<Self, GameError> {
        if !game.supports_uci() {
            return Err(crate::invalid("This game cannot use a UCI player"));
        }
        let config = game.normalize_config(&config)?;
        Ok(Self { pool, game, config, settings })
    }
}
impl Opponent for UciOpponent {
    fn choose_action(
        &self,
        turn: &PlayerTurn<'_>,
        limits: SearchLimits,
        clock: &dyn Clock,
    ) -> Result<ActionChoice, GameError> {
        // Preserve the original trait API. Hosts use decide() to retain operational failure types.
        self.decide(turn, limits, clock).map_err(|error| match error {
            OpponentError::Game(error) => error,
            other => crate::invalid(&other.to_string()),
        })
    }

    fn decide(
        &self,
        turn: &PlayerTurn<'_>,
        mut limits: SearchLimits,
        clock: &dyn Clock,
    ) -> Result<ActionChoice, OpponentError> {
        limits.validate()?;
        let started = Instant::now();
        let position = planning_position(self.game.as_ref(), &self.config, turn, limits.seed)?;
        if clock.now_ms() == u64::MAX { return Err(OpponentError::Cancelled); }
        let elapsed = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
        limits.time_ms = limits.time_ms.checked_sub(elapsed)
            .filter(|&remaining| remaining > 0).ok_or(OpponentError::Timeout)?;
        let result = self.pool.search(&position, &self.settings, limits, clock).map_err(operational)?;
        validate_recommendation(self.game.as_ref(), &self.config, turn, limits, &result)
    }
}

fn operational(error: UciError) -> OpponentError {
    match error {
        UciError::Busy => OpponentError::Busy,
        UciError::Timeout => OpponentError::Timeout,
        UciError::Cancelled => OpponentError::Cancelled,
        UciError::Protocol => OpponentError::InvalidResponse,
        UciError::InvalidInput => crate::invalid("Unsupported UCI engine settings").into(),
        UciError::Unavailable | UciError::Disconnected => OpponentError::Unavailable,
    }
}

/// Validate engine output against the observation's rules state before publishing it.
/// Every retained PV is replayed; impossible moves, null moves, post-terminal moves and
/// illegal PV roots fail the entire result. Centipawns remain typed metadata
/// rather than being represented as a normalized game return.
pub fn validate_recommendation(
    game: &dyn DynGame,
    config: &Value,
    turn: &PlayerTurn<'_>,
    limits: SearchLimits,
    result: &SearchResult,
) -> Result<ActionChoice, OpponentError> {
    let mut state = game.state_from_observation(config, turn.observation, Viewer::Player(turn.seat), limits.seed)?;
    if game.is_terminal(&state)? || game.current_players(&state)? != [turn.seat]
        || game.legal_actions(&state, turn.seat)? != turn.legal_actions
    {
        return Err(OpponentError::InvalidResponse);
    }
    let action = turn.legal_actions.iter().find(|action| action.string == result.best_move)
        .cloned().ok_or(OpponentError::InvalidResponse)?;
    if result.variations.len() > 16 { return Err(OpponentError::InvalidResponse); }
    let initial = state.clone();
    let mut details = vec![];
    let mut chosen: Option<&Info> = None;
    let mut chosen_pv = vec![action.string.clone()];
    for info in &result.variations {
        if info.pv.is_empty() || info.pv.len() > 128 {
            return Err(OpponentError::InvalidResponse);
        }
        state = initial.clone();
        let mut pv = vec![];
        // Public analysis continuations are capped at 32 plies, like the simulation API.
        // We publish the validated prefix with an explicit truncation marker.
        for token in info.pv.iter().take(32) {
            if game.is_terminal(&state)? { return Err(OpponentError::InvalidResponse); }
            let actors = game.current_players(&state)?;
            let [seat] = actors.as_slice() else { return Err(OpponentError::InvalidResponse); };
            let legal = game.legal_actions(&state, *seat)?;
            let next = legal.iter().find(|action| action.string == *token)
                .ok_or(OpponentError::InvalidResponse)?;
            state = game.apply(&state, *seat, &next.json)?.0;
            pv.push(next.string.clone());
        }
        if pv.first() == Some(&action.string) && chosen.is_none() {
            chosen = Some(info);
            chosen_pv = pv.clone();
        }
        let score = info.score.map(|score| match score {
            Score::Centipawns(value) => json!({"unit":"centipawns","value":value}),
            Score::Mate(value) => json!({"unit":"mate_moves","value":value}),
        });
        let bound = match info.bound { Bound::Exact => "exact", Bound::Lower => "lower", Bound::Upper => "upper" };
        details.push(json!({
            "rank":info.multipv.unwrap_or(1),"score":score,"bound":bound,
            "perspective_seat":turn.seat,"wdl_permille":info.wdl,
            "depth":info.depth,"nodes":info.nodes,"time_ms":info.time_ms,
            "principal_variation":pv,"truncated":info.pv.len()>32,
        }));
    }
    if let Some(ponder) = &result.ponder {
        state = game.apply(&initial, turn.seat, &action.json)?.0;
        if game.is_terminal(&state)? { return Err(OpponentError::InvalidResponse); }
        let actors = game.current_players(&state)?;
        let [seat] = actors.as_slice() else { return Err(OpponentError::InvalidResponse); };
        if !game.legal_actions(&state, *seat)?.iter().any(|action| action.string == *ponder) {
            return Err(OpponentError::InvalidResponse);
        }
    }
    let depth = chosen.and_then(|info| info.depth).unwrap_or(0);
    Ok(ActionChoice {
        action,
        info: ChoiceInfo {
            algorithm: "uci".into(), nodes: result.nodes,
            depth: u8::try_from(depth).unwrap_or(u8::MAX),
            evaluation: None, principal_variation: chosen_pv,
            budget_exhausted: result.nodes >= limits.nodes || result.elapsed_ms >= limits.time_ms,
            advice: Some(AdviceInfo {
                summary: format!("{} recommends {}", result.engine_name, result.best_move),
                technique: "engine_search".into(), is_guess: false,
                details: json!({"engine":result.engine_name,"elapsed_ms":result.elapsed_ms,"variations":details}),
            }),
        },
    })
}

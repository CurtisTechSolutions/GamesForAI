//! Resolve a planning source without sharing hidden state or the match RNG.
use crate::{error, replay, GameService};
use gfa_api_types::{ApiError, Assists, SimulationFrom, Start};
use gfa_core::{DynGame, Viewer};
use serde_json::{json, Value};
use std::sync::Arc;

#[derive(Clone, Copy)]
pub(crate) enum AssistKind { Simulation, Analysis }

impl AssistKind {
    fn allowed(self, assists: &Assists) -> bool {
        match self {
            Self::Simulation => assists.allow_simulation,
            Self::Analysis => assists.allow_analysis,
        }
    }
}

pub(crate) struct PlanningPosition {
    pub game: Arc<dyn DynGame>,
    pub config: Value,
    pub frame: replay::Frame,
    pub accounting: Option<(String, u8)>,
}

impl GameService {
    pub(crate) async fn planning_source(
        &self,
        game_id: &str,
        from: &SimulationFrom,
        config: &Value,
        seed: u64,
        viewer: Viewer,
        kind: AssistKind,
    ) -> Result<PlanningPosition, ApiError> {
        let game = self.registry.get(game_id).map_err(error::engine)?;
        let (frame, config, accounting) = match from {
            SimulationFrom::Match {
                match_id,
                seat,
                turn,
            } => {
                if viewer != Viewer::Player(*seat) {
                    return Err(denied(
                        "Planning must use the authorized match seat",
                    ));
                }
                if config != &json!({}) {
                    return Err(ApiError::new(
                        "INVALID_CONFIG",
                        "Match planning uses recorded config",
                        "Omit config for a match source.",
                    ));
                }
                let record = self.record(match_id).await?;
                let Some(origin) = record.origin() else {
                    return Err(error::corrupt());
                };
                if origin.game_id != game_id {
                    return Err(ApiError::new(
                        "INVALID_CONFIG",
                        "Match belongs to another game",
                        "Use the match's game id.",
                    ));
                }
                if !kind.allowed(&origin.assists) {
                    return Err(denied("This assist is disabled for this match"));
                }
                let reconstructed = replay::reconstruct(&self.registry, &record)?;
                let frame = match turn {
                    Some(turn) => reconstructed
                        .frames
                        .iter()
                        .find(|frame| frame.turn == *turn)
                        .ok_or_else(|| {
                            ApiError::new(
                                "INVALID_POSITION",
                                "Historical turn does not exist",
                                "Use a turn from the replay.",
                            )
                        })?,
                    None => reconstructed.current()?,
                };
                let observation = game.observe(&frame.state, viewer).map_err(error::engine)?;
                let state = game
                    .state_from_observation(&origin.config, &observation, viewer, seed)
                    .map_err(error::engine)?;
                let mut planning = crate::simulation::initial(game.as_ref(), state, frame.turn)?;
                planning.terminated = frame.terminated;
                planning.truncated = frame.truncated;
                planning.outcome = frame.outcome.clone();
                planning.draw_offer = frame.draw_offer;
                (planning, game.normalize_config(&origin.config).map_err(error::engine)?, Some((match_id.clone(), *seat)))
            }
            SimulationFrom::Position { position } => {
                let config = game
                    .normalize_config(config)
                    .map_err(error::engine)?;
                game.initial_state(&config, seed).map_err(error::engine)?;
                let state = crate::position::import(
                    game.as_ref(),
                    &config,
                    &Start::Position {
                        position: position.clone(),
                    },
                    seed,
                )?;
                self.guard_standalone_assist(game_id, &config, &state, viewer, kind)
                    .await?;
                (crate::simulation::initial(game.as_ref(), state, 0)?, config, None)
            }
            SimulationFrom::State { state } => {
                let config = game
                    .normalize_config(config)
                    .map_err(error::engine)?;
                game.initial_state(&config, seed).map_err(error::engine)?;
                let state = crate::position::import(
                    game.as_ref(),
                    &config,
                    &Start::State {
                        state: state.clone(),
                    },
                    seed,
                )?;
                self.guard_standalone_assist(game_id, &config, &state, viewer, kind)
                    .await?;
                (crate::simulation::initial(game.as_ref(), state, 0)?, config, None)
            }
        };
        Ok(PlanningPosition { game, config, frame, accounting })
    }

    // Local mode has one owner. Public hosting must scope this index to the
    // authenticated caller's active matches before exposing this service.
    async fn guard_standalone_assist(
        &self,
        game_id: &str,
        config: &Value,
        state: &Value,
        viewer: Viewer,
        kind: AssistKind,
    ) -> Result<(), ApiError> {
        let game = self.registry.get(game_id).map_err(error::engine)?;
        let observation = game.observe(state, viewer).map_err(error::engine)?;
        let fingerprint = crate::simulation::fingerprint(config, &observation)?;
        let mut after = String::new();
        loop {
            let ids = self
                .store
                .list_ids(&after, 100)
                .await
                .map_err(error::store)?;
            if ids.len() > 100
                || ids.iter().any(|id| id <= &after)
                || ids.windows(2).any(|pair| pair[0] >= pair[1])
            {
                return Err(error::corrupt());
            }
            for id in &ids {
                let record = self.record(id).await?;
                let Some(origin) = record.origin() else {
                    return Err(error::corrupt());
                };
                if origin.game_id != game_id || kind.allowed(&origin.assists) {
                    continue;
                }
                let reconstructed = replay::reconstruct(&self.registry, &record)?;
                let current = reconstructed.current()?;
                if current.ended() {
                    continue;
                }
                let config = game
                    .normalize_config(&origin.config)
                    .map_err(error::engine)?;
                let observation = game
                    .observe(&current.state, viewer)
                    .map_err(error::engine)?;
                if crate::simulation::fingerprint(&config, &observation)? == fingerprint {
                    return Err(denied(
                        "This position belongs to an active match where this assist is disabled",
                    ));
                }
            }
            if ids.len() < 100 {
                break;
            }
            after = ids.last().cloned().ok_or_else(error::corrupt)?;
        }
        Ok(())
    }

}

fn denied(message: &str) -> ApiError {
    ApiError::new("ASSIST_NOT_ALLOWED", message, "Use a match whose recorded assists allow this operation.")
}

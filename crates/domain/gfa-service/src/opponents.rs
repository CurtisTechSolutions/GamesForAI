//! Player selection and analysis orchestration, independent of worker runtime.
use crate::{error, planning::AssistKind, GameService};
use gfa_api_types::{
    AnalysisRequest, AnalysisResult, ApiError, OpponentConfig, OpponentLevel, OpponentSpec,
};
use gfa_core::{DynGame, Information, LegalAction, Observation, TurnStructure, Viewer};
use gfa_opponents::{
    ActionChoice, Algorithm, Clock, Opponent, PlayerTurn, Random, ReferenceOpponent, SearchLimits,
    SearchOpponent,
};
use serde_json::Value;
use std::{future::Future, pin::Pin, sync::Arc};

/// Runtime-independent result of one bounded worker job.
pub type OpponentFuture<'a> =
    Pin<Box<dyn Future<Output = Result<ActionChoice, ApiError>> + Send + 'a>>;

/// Bounded recommendations returned by an analysis worker.
pub type AnalysisFuture<'a> =
    Pin<Box<dyn Future<Output = Result<Vec<ActionChoice>, ApiError>> + Send + 'a>>;

/// Select installed players without exposing live state or transport details.
pub trait OpponentFactory: Send + Sync {
    /// Stable catalog for this game.
    fn catalog(&self, game: &dyn DynGame) -> Vec<OpponentSpec>;
    /// Construct one player with validated options and limits.
    fn create(
        &self,
        game: Arc<dyn DynGame>,
        config: Value,
        opponent: &OpponentConfig,
        seed: u64,
    ) -> Result<(Arc<dyn Opponent>, SearchLimits), ApiError>;
}

/// Host worker pool; implementations retain capacity until cancelled jobs finish.
pub trait OpponentExecutor: Send + Sync {
    /// Run outside the request executor, rejecting excess work instead of queueing without bounds.
    fn execute(&self, job: OpponentJob) -> OpponentFuture<'_>;
    /// Analyze on the host's bounded executor. Legacy executors retain single-choice behavior.
    fn analyze(&self, job: OpponentJob) -> AnalysisFuture<'_> {
        Box::pin(async move { self.execute(job).await.map(|choice| vec![choice]) })
    }
}

/// Owned planning input. No raw match state or match RNG is passed to the worker.
pub struct OpponentJob {
    opponent: Arc<dyn Opponent>,
    seat: u8,
    observation: Observation,
    legal_actions: Vec<LegalAction>,
    limits: SearchLimits,
}

impl OpponentJob {
    /// Request ranked analysis recommendations with the same observation-only input.
    pub fn analyze(self, clock: &dyn Clock) -> Result<Vec<ActionChoice>, ApiError> {
        self.opponent
            .analyze(
                &PlayerTurn {
                    seat: self.seat,
                    observation: &self.observation,
                    legal_actions: &self.legal_actions,
                },
                self.limits,
                clock,
            )
            .map_err(error::opponent)
    }

    /// Execute with an injected monotonic clock. The host supplies the runtime.
    pub fn run(self, clock: &dyn Clock) -> Result<ActionChoice, ApiError> {
        self.opponent
            .decide(
                &PlayerTurn {
                    seat: self.seat,
                    observation: &self.observation,
                    legal_actions: &self.legal_actions,
                },
                self.limits,
                clock,
            )
            .map_err(error::opponent)
    }
}

/// Random, alpha-beta, and UCT players from the observation-only player library.
pub struct BuiltinOpponentFactory;

fn spec(id: &str, name: &str, levels: bool) -> OpponentSpec {
    OpponentSpec {
        id: id.into(),
        name: name.into(),
        levels: if levels {
            (1..=10)
                .map(|level| OpponentLevel {
                    level,
                    rating: None,
                })
                .collect()
        } else {
            vec![]
        },
        calibrated: false,
    }
}

impl OpponentFactory for BuiltinOpponentFactory {
    fn catalog(&self, game: &dyn DynGame) -> Vec<OpponentSpec> {
        let mut catalog = vec![spec("random", "Uniform random", false)];
        if game.supports_reference_advice() {
            catalog.push(spec("reference", "Reference solver", false));
        }
        let game = game.spec();
        if game.information == Information::Perfect
            && !game.stochastic
            && game.turn_structure == TurnStructure::Sequential
        {
            catalog.push(spec("mcts", "Monte Carlo tree search", true));
            if game.num_players == [2, 2] {
                catalog.push(spec("minimax", "Alpha-beta minimax", true));
            }
        }
        catalog.sort_by(|a, b| a.id.cmp(&b.id));
        catalog
    }

    fn create(
        &self,
        game: Arc<dyn DynGame>,
        config: Value,
        opponent: &OpponentConfig,
        seed: u64,
    ) -> Result<(Arc<dyn Opponent>, SearchLimits), ApiError> {
        if opponent.uci.is_some() {
            return Err(ApiError::new(
                "INVALID_CONFIG",
                "This player does not accept UCI options",
                "Omit uci or choose an installed UCI opponent.",
            ));
        }
        if !self
            .catalog(game.as_ref())
            .iter()
            .any(|spec| spec.id == opponent.id)
        {
            return Err(ApiError::new(
                "OPPONENT_UNAVAILABLE",
                "Opponent is unavailable for this game",
                "Select an installed opponent from the game's catalog.",
            ));
        }
        if matches!(opponent.id.as_str(), "random" | "reference") && opponent.level.is_some() {
            return Err(ApiError::new(
                "INVALID_CONFIG",
                "This opponent has no difficulty levels",
                "Omit level for random and reference opponents.",
            ));
        }
        let mut limits =
            SearchLimits::for_level(opponent.level.unwrap_or(3), seed).map_err(error::engine)?;
        if let Some(nodes) = opponent.limits.nodes {
            limits.nodes = nodes;
        }
        if let Some(depth) = opponent.limits.depth {
            limits.depth = depth;
        }
        if let Some(time_ms) = opponent.limits.time_ms {
            limits.time_ms = time_ms;
        }
        limits.validate().map_err(error::engine)?;
        let player: Arc<dyn Opponent> = match opponent.id.as_str() {
            "random" => Arc::new(Random),
            "reference" => Arc::new(ReferenceOpponent::new(game, config).map_err(error::engine)?),
            "minimax" => Arc::new(
                SearchOpponent::new(game, config, Algorithm::Minimax).map_err(error::engine)?,
            ),
            "mcts" => {
                Arc::new(SearchOpponent::new(game, config, Algorithm::Mcts).map_err(error::engine)?)
            }
            _ => {
                return Err(ApiError::new(
                    "OPPONENT_UNAVAILABLE",
                    "Opponent disappeared",
                    "Refresh the opponent catalog.",
                ))
            }
        };
        Ok((player, limits))
    }
}

impl GameService {
    pub(crate) async fn choose_opponent(
        &self,
        game: Arc<dyn DynGame>,
        config: Value,
        selection: &OpponentConfig,
        seed: u64,
        seat: u8,
        visible: gfa_api_types::MatchState,
    ) -> Result<ActionChoice, ApiError> {
        let (factory, executor) = self.opponents.as_ref().ok_or_else(|| {
            ApiError::new(
                "ENGINE_UNAVAILABLE",
                "No opponent executor is installed",
                "Configure an opponent worker pool on the host.",
            )
        })?;
        let (opponent, limits) = factory.create(game, config, selection, seed)?;
        let choice = executor
            .execute(OpponentJob {
                opponent,
                seat,
                observation: visible.observation,
                legal_actions: visible.legal_actions.clone(),
                limits,
            })
            .await?;
        if !visible.legal_actions.contains(&choice.action)
            || choice
                .info
                .evaluation
                .is_some_and(|score| !score.is_finite())
        {
            return Err(ApiError::new(
                "ENGINE_UNAVAILABLE",
                "Opponent returned an invalid recommendation",
                "Choose another opponent and report the provider failure.",
            ));
        }
        Ok(choice)
    }

    /// Install a player registry and a bounded host executor before serving requests.
    pub fn with_opponents(
        mut self,
        factory: Arc<dyn OpponentFactory>,
        executor: Arc<dyn OpponentExecutor>,
    ) -> Self {
        self.opponents = Some((factory, executor));
        self
    }

    /// Available opponents in stable identifier order. Unconfigured hosts return an empty catalog.
    pub fn list_opponents(&self, game_id: &str) -> Result<Vec<OpponentSpec>, ApiError> {
        let game = self.registry.get(game_id).map_err(error::engine)?;
        Ok(self
            .opponents
            .as_ref()
            .map_or_else(Vec::new, |(factory, _)| factory.catalog(game.as_ref())))
    }

    /// Recommend legal moves without modifying events, clocks, or idempotency receipts.
    pub async fn analyze(
        &self,
        request: AnalysisRequest,
        viewer: Viewer,
    ) -> Result<AnalysisResult, ApiError> {
        let Viewer::Player(seat) = viewer else {
            return Err(ApiError::new(
                "FORBIDDEN",
                "Analysis requires an authorized player view",
                "Select a seat you may access.",
            ));
        };
        let seed = request.seed.unwrap_or_else(|| self.ids.next_seed());
        let planning = self
            .planning_source(
                &request.game_id,
                &request.from,
                &request.config,
                seed,
                viewer,
                AssistKind::Analysis,
            )
            .await?;
        if planning.frame.ended() {
            return Err(ApiError::new(
                "MATCH_FINISHED",
                "This analysis position has ended",
                "Choose an earlier playable turn.",
            ));
        }
        let visible = planning.frame.project("", planning.game.as_ref(), viewer)?;
        if !visible.to_act.contains(&seat) || visible.legal_actions.is_empty() {
            return Err(ApiError::new(
                "NOT_YOUR_TURN",
                "This seat cannot act in the analysis position",
                "Select the current player or an earlier turn.",
            ));
        }
        let (factory, executor) = self.opponents.as_ref().ok_or_else(|| {
            ApiError::new(
                "ENGINE_UNAVAILABLE",
                "No opponent executor is installed",
                "Configure an opponent worker pool on the host.",
            )
        })?;
        let (opponent, limits) =
            factory.create(planning.game, planning.config, &request.opponent, seed)?;
        let choices = executor
            .analyze(OpponentJob {
                opponent,
                seat,
                observation: visible.observation,
                legal_actions: visible.legal_actions.clone(),
                limits,
            })
            .await?;
        validate_analysis_choices(&choices, &visible.legal_actions)?;
        let choice = choices.first().ok_or_else(invalid_analysis)?;
        Ok(AnalysisResult {
            advice: choice.info.advice.clone(),
            game_id: request.game_id,
            opponent: request.opponent.id,
            seed,
            best_moves: choices.iter().map(|choice| choice.action.clone()).collect(),
            evaluation: choice.info.evaluation,
            principal_variation: choice.info.principal_variation.clone(),
            nodes: choices
                .iter()
                .map(|choice| choice.info.nodes)
                .max()
                .unwrap_or(0),
            depth: choice.info.depth,
            budget_exhausted: choices.iter().any(|choice| choice.info.budget_exhausted),
            variations: choices,
        })
    }
}

#[cfg(test)]
mod failure_tests {
    use super::*;
    #[test]
    fn jobs_use_the_operational_entry_point() -> Result<(), Box<dyn std::error::Error>> {
        struct Failure;
        impl Opponent for Failure {
            fn choose_action(
                &self,
                _: &PlayerTurn<'_>,
                _: SearchLimits,
                _: &dyn Clock,
            ) -> Result<ActionChoice, gfa_core::GameError> {
                Err(gfa_core::GameError::illegal("legacy input error"))
            }
            fn decide(
                &self,
                _: &PlayerTurn<'_>,
                _: SearchLimits,
                _: &dyn Clock,
            ) -> Result<ActionChoice, gfa_core::OpponentError> {
                Err(gfa_core::OpponentError::Timeout)
            }
        }
        struct Frozen;
        impl Clock for Frozen {
            fn now_ms(&self) -> u64 {
                0
            }
        }
        let observation: Observation = serde_json::from_value(serde_json::json!({
            "json": {}, "text": "", "tensor": null
        }))?;
        let job = OpponentJob {
            opponent: Arc::new(Failure),
            seat: 0,
            observation,
            legal_actions: vec![],
            limits: SearchLimits::for_level(1, 0)?,
        };
        assert_eq!(
            job.run(&Frozen).err().ok_or("expected error")?.code,
            "ENGINE_TIMEOUT"
        );
        Ok(())
    }
}

fn invalid_analysis() -> ApiError {
    ApiError::new(
        "ENGINE_INVALID_RESPONSE",
        "Opponent returned invalid analysis recommendations",
        "Select another opponent and report the provider failure.",
    )
}
fn validate_analysis_choices(
    choices: &[ActionChoice],
    legal: &[LegalAction],
) -> Result<(), ApiError> {
    if choices.is_empty() || choices.len() > 16 {
        return Err(invalid_analysis());
    }
    let mut seen = std::collections::BTreeSet::new();
    for choice in choices {
        if !legal.contains(&choice.action)
            || !seen.insert(choice.action.index)
            || choice
                .info
                .evaluation
                .is_some_and(|score| !score.is_finite())
            || choice.info.principal_variation.len() > 128
            || choice
                .info
                .principal_variation
                .first()
                .is_some_and(|first| first != &choice.action.string)
        {
            return Err(invalid_analysis());
        }
    }
    Ok(())
}

#[cfg(test)]
mod analysis_validation_tests {
    use super::*;
    fn choice() -> ActionChoice {
        ActionChoice {
            action: LegalAction {
                string: "a".into(),
                json: serde_json::json!("a"),
                index: 0,
            },
            info: gfa_core::ChoiceInfo {
                advice: None,
                algorithm: "test".into(),
                nodes: 1,
                depth: 1,
                evaluation: Some(0.0),
                principal_variation: vec!["a".into()],
                budget_exhausted: false,
            },
        }
    }
    #[test]
    fn rejects_empty_duplicate_nonfinite_illegal_and_mismatched_analysis() {
        let first = choice();
        let legal = vec![first.action.clone()];
        assert!(validate_analysis_choices(std::slice::from_ref(&first), &legal).is_ok());
        assert!(validate_analysis_choices(&[], &legal).is_err());
        assert!(validate_analysis_choices(&vec![first.clone(); 17], &legal).is_err());
        assert!(validate_analysis_choices(&[first.clone(), first.clone()], &legal).is_err());
        let mut bad = first.clone();
        bad.info.evaluation = Some(f64::NAN);
        assert!(validate_analysis_choices(&[bad], &legal).is_err());
        let mut bad = first.clone();
        bad.action.string = "b".into();
        assert!(validate_analysis_choices(&[bad], &legal).is_err());
        let mut bad = first;
        bad.info.principal_variation[0] = "b".into();
        assert!(validate_analysis_choices(&[bad], &legal).is_err());
    }
}

impl GameService {
    pub(crate) fn briefing_opponents(
        &self,
        info: &mut gfa_api_types::Briefing,
        game: &dyn DynGame,
        detail: gfa_api_types::InfoDetail,
        assists: Option<&gfa_api_types::Assists>,
    ) -> Result<(), ApiError> {
        let catalog = self
            .opponents
            .as_ref()
            .map_or_else(Vec::new, |(factory, _)| factory.catalog(game));
        let analysis_available =
            self.opponents.is_some() && game.spec().information == Information::Perfect;
        let available = if detail == gfa_api_types::InfoDetail::Full {
            serde_json::to_value(&catalog).map_err(|_| crate::briefing::internal())?
        } else {
            serde_json::json!(catalog
                .iter()
                .map(|opponent| serde_json::json!({
                    "id":opponent.id,
                    "levels":opponent.levels.iter().map(|level|level.level).collect::<Vec<_>>(),
                    "calibrated":opponent.calibrated,
                    "ratings":if opponent.levels.iter().any(|level|level.rating.is_some()) {
                        serde_json::json!(opponent.levels)
                    } else { Value::Null }
                }))
                .collect::<Vec<_>>())
        };
        let section = info
            .sections
            .iter_mut()
            .find(|section| section.id == "opponents")
            .ok_or_else(crate::briefing::internal)?;
        section.text = if catalog.is_empty() {
            "No opponent workers are installed on this host. External clients control the seats."
                .into()
        } else {
            "Installed opponents support automatic play and permitted analysis. Null ratings are uncalibrated.".into()
        };
        section.data = serde_json::json!({
            "available":available,"analysis_available":analysis_available,
            "analysis":analysis_available && assists.is_none_or(|assists|assists.allow_analysis),
            "simulation":assists.is_none_or(|assists|assists.allow_simulation)
        });
        info.estimate_tokens()
            .map_err(|_| crate::briefing::internal())
    }
}

use crate::ServerError;
use gfa_api_types::{ApiError, OpponentConfig, OpponentLevel, OpponentSpec};
use gfa_core::{
    serde_json::{json, Value},
    DynGame, GameRegistry, Opponent, PlayerTurn, SearchLimits, Viewer,
};
use gfa_engine_uci::{EnginePool, SandboxConfig, Settings, UciOpponent};
use gfa_service::{BuiltinOpponentFactory, OpponentFactory};
use std::{path::PathBuf, sync::Arc};

/// Host-selected engine resources, never supplied by match participants.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StockfishConfig {
    /// Process isolation and CPU/memory/concurrency policy.
    pub sandbox: SandboxConfig,
    /// Per-process search threads, 1..4.
    pub threads: u8,
    /// Per-process transposition table MiB, 1..256.
    pub hash_mb: u16,
}
impl StockfishConfig {
    /// Standard Linux policy for a trusted installed Stockfish binary.
    pub fn linux(engine: impl Into<PathBuf>) -> Self {
        Self {
            sandbox: SandboxConfig::linux(engine),
            threads: 1,
            hash_mb: 32,
        }
    }
}

struct InstalledOpponents {
    pool: Arc<EnginePool>,
    settings: Settings,
}
impl OpponentFactory for InstalledOpponents {
    fn catalog(&self, game: &dyn DynGame) -> Vec<OpponentSpec> {
        let mut catalog = BuiltinOpponentFactory.catalog(game);
        if game.supports_uci() {
            catalog.push(OpponentSpec {
                id: "stockfish".into(),
                name: "Stockfish".into(),
                levels: (1..=10)
                    .map(|level| OpponentLevel {
                        level,
                        rating: None,
                    })
                    .collect(),
                calibrated: false,
            });
        }
        catalog.sort_by(|a, b| a.id.cmp(&b.id));
        catalog
    }
    fn create(
        &self,
        game: Arc<dyn DynGame>,
        config: Value,
        selection: &OpponentConfig,
        seed: u64,
    ) -> Result<(Arc<dyn Opponent>, SearchLimits), ApiError> {
        if selection.id != "stockfish" {
            return BuiltinOpponentFactory.create(game, config, selection, seed);
        }
        if !game.supports_uci() {
            return Err(ApiError::new(
                "OPPONENT_UNAVAILABLE",
                "Stockfish is unavailable for this game",
                "Choose an opponent from this game's catalog.",
            ));
        }
        let (settings, limits) = settings(selection, seed, &self.settings)?;
        let player = UciOpponent::new(self.pool.clone(), game, config, settings)
            .map_err(|error| invalid(&error.message))?;
        Ok((Arc::new(player), limits))
    }
}
fn invalid(message: &str) -> ApiError {
    ApiError::new(
        "INVALID_CONFIG",
        message,
        "Use level 1..10, or one UCI strength option, and bounded search limits.",
    )
}
fn settings(
    selection: &OpponentConfig,
    seed: u64,
    host: &Settings,
) -> Result<(Settings, SearchLimits), ApiError> {
    let level = selection.level.unwrap_or(3);
    let mut limits =
        SearchLimits::for_level(level, seed).map_err(|error| invalid(&error.message))?;
    // Resource presets and Stockfish's own skill are provisional until measured calibration ships.
    let mut settings = host.clone();
    settings.skill = Some([0, 2, 4, 6, 8, 10, 12, 14, 17, 20][usize::from(level - 1)]);
    if let Some(options) = &selection.uci {
        if (options.skill.is_some() && options.elo.is_some())
            || (selection.level.is_some() && (options.skill.is_some() || options.elo.is_some()))
            || options.skill.is_some_and(|skill| skill > 20)
            || options
                .multipv
                .is_some_and(|count| !(1..=16).contains(&count))
        {
            return Err(invalid(
                "Conflicting strength options or out-of-range UCI settings",
            ));
        }
        if let Some(skill) = options.skill {
            settings.skill = Some(skill);
        }
        if let Some(elo) = options.elo {
            settings.skill = None;
            settings.elo = Some(elo);
        }
        settings.multipv = options.multipv.unwrap_or(1);
    }
    if let Some(nodes) = selection.limits.nodes {
        limits.nodes = nodes;
    }
    if let Some(depth) = selection.limits.depth {
        limits.depth = depth;
    }
    if let Some(time_ms) = selection.limits.time_ms {
        limits.time_ms = time_ms;
    }
    limits.validate().map_err(|error| invalid(&error.message))?;
    Ok((settings, limits))
}

pub(super) async fn factory(
    config: Option<&StockfishConfig>,
    registry: &GameRegistry,
) -> Result<Arc<dyn OpponentFactory>, ServerError> {
    let Some(config) = config else {
        return Ok(Arc::new(BuiltinOpponentFactory));
    };
    if !(1..=4).contains(&config.threads) || !(1..=256).contains(&config.hash_mb) {
        return Err("Invalid Stockfish host resource settings".into());
    }
    let pool = Arc::new(EnginePool::new(config.sandbox.clone())?);
    let settings = Settings {
        threads: config.threads,
        hash_mb: config.hash_mb,
        ..Settings::default()
    };
    // Verify the installed binary and sandbox before announcing a usable server/catalog.
    let game = registry.get("chess")?;
    let player = UciOpponent::new(pool.clone(), game.clone(), json!({}), settings.clone())?;
    tokio::task::spawn_blocking(move || -> Result<(), gfa_core::OpponentError> {
        let state = game.initial_state(&json!({}), 0)?;
        player.decide(
            &PlayerTurn {
                seat: 0,
                observation: &game.observe(&state, Viewer::Player(0))?,
                legal_actions: &game.legal_actions(&state, 0)?,
            },
            SearchLimits {
                nodes: 100,
                depth: 1,
                time_ms: 5000,
                seed: 0,
            },
            &gfa_opponents::SystemClock::default(),
        )?;
        Ok(())
    })
    .await??;
    Ok(Arc::new(InstalledOpponents { pool, settings }))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ladder_and_overrides_are_bounded_and_unambiguous() -> Result<(), ServerError> {
        for level in 1..=10 {
            let selection =
                gfa_core::serde_json::from_value(json!({"id":"stockfish","level":level}))?;
            let (options, budget) = settings(&selection, 7, &Settings::default())?;
            assert_eq!(budget.seed, 7);
            assert!(options.skill.is_some_and(|skill| skill <= 20));
        }
        for fields in [
            json!({"level":0}),
            json!({"level":11}),
            json!({"level":3,"uci":{"elo":1500}}),
            json!({"uci":{"elo":1500,"skill":2}}),
            json!({"uci":{"skill":21}}),
            json!({"uci":{"multipv":17}}),
            json!({"limits":{"nodes":0}}),
        ] {
            let mut value = fields;
            value["id"] = json!("stockfish");
            assert!(settings(
                &gfa_core::serde_json::from_value(value)?,
                0,
                &Settings::default()
            )
            .is_err());
        }
        let selection = gfa_core::serde_json::from_value(
            json!({"id":"stockfish","uci":{"elo":1500,"multipv":3}}),
        )?;
        let (options, _) = settings(&selection, 7, &Settings::default())?;
        assert_eq!(options.elo, Some(1500));
        assert_eq!(options.skill, None);
        assert_eq!(options.multipv, 3);
        Ok(())
    }
}

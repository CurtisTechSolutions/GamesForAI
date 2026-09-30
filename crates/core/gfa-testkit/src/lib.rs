//! Engine contract checks shared by game plugins.
use gfa_core::{Game, Information, SeededRng, Viewer};
use serde_json::to_value;
use std::collections::HashSet;

/// Run seeded playouts, encoding checks, observation schema checks and deterministic replays.
///
/// Imperfect-information engines must additionally supply game-specific paired-state
/// non-leakage tests: generic tests cannot know which parts of their state are private.
pub fn conformance<G: Game>() -> Result<(), Box<dyn std::error::Error>> {
    let spec = G::spec();
    assert!(!spec.id.is_empty());
    assert!(!spec.rules_markdown.is_empty());
    assert!(!spec.action_notation.is_empty());
    assert!(!spec.position_notation.is_empty());
    assert!(spec.max_game_length > 0);
    assert!(spec.action_space_size > 0);
    assert!(spec.num_players[0] > 0 && spec.num_players[0] <= spec.num_players[1]);
    let config = G::Config::default();
    assert!(jsonschema::is_valid(&spec.config_schema, &to_value(&config)?));
    for seed in 0..64 {
        let mut rng = SeededRng::new(seed);
        let mut state = G::new_initial_state(&config, seed)?;
        let mut replay = G::new_initial_state(&config, seed)?;
        let mut ended = false;
        for _ in 0..=spec.max_game_length {
            G::validate_state(&state)?;
            let serialized = to_value(&state)?;
            let round_trip: G::State = serde_json::from_value(serialized.clone())?;
            assert_eq!(to_value(&round_trip)?, serialized);
            let notation = G::state_to_notation(&state)?;
            assert_eq!(
                to_value(G::state_from_notation(&config, &notation)?)?,
                serialized
            );
            assert_eq!(to_value(&replay)?, serialized);
            assert!(G::returns(&state).iter().all(|r| r.is_finite()));
            let public = G::observe(&state, Viewer::Spectator);
            assert!(!public.text.is_empty());
            assert!(jsonschema::is_valid(&spec.observation_schema, &public.json));
            for seat in 0..spec.num_players[0] {
                let observation = G::observe(&state, Viewer::Player(seat));
                assert!(jsonschema::is_valid(&spec.observation_schema, &observation.json));
                if spec.information == Information::Perfect {
                    assert_eq!(observation, public);
                }
                if let Some(tensor) = observation.tensor {
                    assert_eq!(tensor.shape.iter().product::<usize>(), tensor.values.len());
                    assert!(tensor.values.iter().all(|v| v.is_finite()));
                }
            }
            let players = G::current_players(&state);
            if G::is_terminal(&state) {
                assert!(players.is_empty());
                for seat in 0..spec.num_players[0] {
                    assert!(G::legal_actions(&state, seat).is_empty());
                }
                ended = true;
                break;
            }
            assert!(!players.is_empty(), "nonterminal state has no actors");
            let player = players[0];
            let legal = G::legal_actions(&state, player);
            assert!(!legal.is_empty(), "active player has no legal actions");
            let mut indices = HashSet::new();
            let mut strings = HashSet::new();
            for action in &legal {
                let index = G::action_to_index(action);
                let string = G::action_to_string(&state, action);
                assert!(index < spec.action_space_size);
                assert!(indices.insert(index));
                assert!(strings.insert(string.clone()));
                assert!(G::action_from_index(&state, index)? == *action);
                assert!(G::action_from_string(&state, &string)? == *action);
                let encoded = to_value(action)?;
                assert!(jsonschema::is_valid(&spec.action_schema, &encoded));
                let restored: G::Action = serde_json::from_value(encoded)?;
                assert!(restored == *action);
                let mut next = state.clone();
                G::apply(&mut next, player, action)?;
                G::validate_state(&next)?;
            }
            let index = rng.index(legal.len()).ok_or("empty legal actions")?;
            let events = G::apply(&mut state, player, &legal[index])?;
            assert_eq!(events, G::apply(&mut replay, player, &legal[index])?);
            if events.truncated {
                assert!(G::current_players(&state).is_empty());
                ended = true;
                break;
            }
        }
        assert!(ended, "engine exceeded max_game_length");
    }
    Ok(())
}

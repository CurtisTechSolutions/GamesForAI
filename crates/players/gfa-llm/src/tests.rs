use crate::*;
use serde_json::json;

#[test]
fn identity_records_strength_settings_and_accepts_self_hosted_models() -> Result<(), ProviderError> {
    let config = PlayerConfig {
        provider: "openai-compatible".into(),
        model: "my-model-v3".into(),
        ..PlayerConfig::default()
    };
    let identity = config.agent_identity(false, true)?;
    assert_eq!(identity, config.clone().agent_identity(false, true)?);
    assert_ne!(identity, config.agent_identity(true, true)?);
    assert_ne!(identity, config.agent_identity(false, false)?);
    let mut changed = config.clone();
    changed.effort = Effort::High;
    assert_ne!(identity, changed.agent_identity(false, true)?);
    changed = config.clone();
    changed.mode = PlayMode::Direct;
    assert_ne!(identity, changed.agent_identity(false, true)?);
    changed = config;
    changed.prompt_version = "gfa.llm.2".into();
    assert_ne!(identity, changed.agent_identity(false, true)?);
    Ok(())
}

#[test]
fn rejects_secrets_unknown_fields_and_unbounded_settings() -> Result<(), Box<dyn std::error::Error>> {
    let mut config = serde_json::to_value(PlayerConfig::default())?;
    config["api_key"] = json!("must-not-be-persisted");
    assert!(serde_json::from_value::<PlayerConfig>(config).is_err());
    for config in [
        PlayerConfig { max_output_tokens: 0, ..PlayerConfig::default() },
        PlayerConfig { move_timeout_ms: 600_001, ..PlayerConfig::default() },
        PlayerConfig { illegal_move_retries: 11, ..PlayerConfig::default() },
        PlayerConfig { max_game_cost_microusd: 0, ..PlayerConfig::default() },
        PlayerConfig { model: "model\nheader".into(), ..PlayerConfig::default() },
    ] {
        assert_eq!(config.validate(), Err(ProviderError::InvalidConfig));
    }
    Ok(())
}

#[test]
fn accounting_keeps_cache_classes_separate_and_never_wraps() -> Result<(), ProviderError> {
    let usage = Usage {
        input_tokens: 100,
        output_tokens: 50,
        cache_read_input_tokens: 1000,
        cache_creation_5m_tokens: 200,
        cache_creation_1h_tokens: 300,
    };
    let pricing = Pricing {
        version: "test-fixture-not-live-pricing".into(),
        input: 2_000_000,
        output: 10_000_000,
        cache_read: 200_000,
        cache_write_5m: 2_500_000,
        cache_write_1h: 4_000_000,
    };
    assert_eq!(usage.total()?, 1650);
    assert_eq!(pricing.cost(usage)?, 2600);
    assert_eq!(
        pricing.cost(Usage { cache_read_input_tokens: 1, ..Usage::default() })?,
        1
    );
    let overflow = Usage { input_tokens: u64::MAX, output_tokens: 1, ..Usage::default() };
    assert_eq!(overflow.total(), Err(ProviderError::InvalidResponse));
    assert_eq!(pricing.cost(overflow), Err(ProviderError::InvalidResponse));
    Ok(())
}

#[test]
fn budget_rejects_cost_token_and_integer_overflow() -> Result<(), ProviderError> {
    let budget = Budget {
        token_limit: 100,
        cost_limit: 200,
        committed_tokens: 90,
        committed_cost: 150,
    };
    budget.check(10, 50)?;
    for (tokens, cost) in [(11, 0), (0, 51), (u64::MAX, 0), (0, u64::MAX)] {
        assert_eq!(budget.check(tokens, cost), Err(ProviderError::BudgetExhausted));
    }
    assert_eq!(
        Budget { committed_cost: 201, ..budget }.check(0, 0),
        Err(ProviderError::BudgetExhausted)
    );
    Ok(())
}

#[test]
fn opaque_thinking_and_tool_blocks_round_trip_without_edits() -> Result<(), Box<dyn std::error::Error>> {
    let raw = json!({"role":"assistant","content":[
        {"type":"thinking","thinking":"summary","signature":"opaque-provider-signature"},
        {"type":"tool_use","id":"call_1","name":"make_move","input":{"action":"r1c1"}}
    ]});
    let message: ContentMessage = serde_json::from_value(raw.clone())?;
    assert_eq!(serde_json::to_value(message)?, raw);
    assert_eq!(serde_json::from_value::<StopReason>(json!("refusal"))?, StopReason::Refusal);
    assert_eq!(serde_json::from_value::<StopReason>(json!("max_tokens"))?, StopReason::MaxTokens);
    Ok(())
}

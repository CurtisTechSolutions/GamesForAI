use super::*;
use std::collections::HashSet;

pub(super) struct ToolUse {
    pub id: String,
    pub name: String,
    pub input: Value,
}
pub(super) fn tool_uses(message: &ContentMessage) -> Result<Vec<ToolUse>, ProviderError> {
    let mut ids = HashSet::new();
    let mut calls = Vec::new();
    for block in &message.content {
        if block["type"] != "tool_use" {
            continue;
        }
        let id = block["id"]
            .as_str()
            .filter(|id| !id.is_empty() && id.len() <= 256)
            .ok_or(ProviderError::InvalidResponse)?;
        let name = block["name"]
            .as_str()
            .filter(|name| !name.is_empty() && name.len() <= 64)
            .ok_or(ProviderError::InvalidResponse)?;
        if !ids.insert(id) || calls.len() >= 16 || !block["input"].is_object() {
            return Err(ProviderError::InvalidResponse);
        }
        calls.push(ToolUse {
            id: id.into(),
            name: name.into(),
            input: block["input"].clone(),
        });
    }
    Ok(calls)
}
pub(super) fn definitions(assists: AllowedAssists) -> Vec<ToolDefinition> {
    let mut tools = vec![
        definition(
            "get_state",
            "Read this seat's current observation and legal actions.",
            json!({"type":"object","properties":{},"required":[],"additionalProperties":false}),
        ),
        definition(
            "make_move",
            "Select one listed legal action for the host to commit. Explain the decision briefly.",
            json!({"type":"object","properties":{"action":{"type":"string"},"reasoning":{"type":"string"}},"required":["action","reasoning"],"additionalProperties":false}),
        ),
    ];
    if assists.allow_simulation {
        tools.push(definition("simulate_moves", "Inspect hypothetical lines from this seat's current position. The real match is unchanged.", json!({"type":"object","properties":{"lines":{"type":"array","items":{"type":"array","items":{"type":"string"},"maxItems":32},"minItems":1,"maxItems":16}},"required":["lines"],"additionalProperties":false})));
    }
    if assists.allow_analysis {
        tools.push(definition("analyze_position", "Ask an installed engine for recommendations at the current authorized position.", json!({"type":"object","properties":{"opponent":{"type":"string"},"level":{"type":["integer","null"],"minimum":1,"maximum":10}},"required":["opponent","level"],"additionalProperties":false})));
    }
    tools
}
fn definition(name: &str, description: &str, input_schema: Value) -> ToolDefinition {
    ToolDefinition {
        name: name.into(),
        description: description.into(),
        input_schema,
    }
}

pub(super) fn validate_input(name: &str, input: &Value) -> bool {
    let Some(object) = input.as_object() else {
        return false;
    };
    match name {
        "get_state" => object.is_empty(),
        "make_move" => serde_json::from_value::<MoveDecision>(input.clone()).is_ok(),
        "simulate_moves" => {
            object.len() == 1
                && input["lines"].as_array().is_some_and(|lines| {
                    !lines.is_empty()
                        && lines.len() <= 16
                        && lines.iter().all(|line| {
                            line.as_array().is_some_and(|line| {
                                line.len() <= 32
                                    && line.iter().all(|action| {
                                        action.as_str().is_some_and(|action| {
                                            !action.is_empty() && action.len() <= 8192
                                        })
                                    })
                            })
                        })
                })
        }
        "analyze_position" => {
            object.len() == 2
                && object.contains_key("level")
                && input["opponent"]
                    .as_str()
                    .is_some_and(|id| !id.is_empty() && id.len() <= 128 && !id.starts_with("llm:"))
                && (input["level"].is_null()
                    || input["level"]
                        .as_u64()
                        .is_some_and(|level| (1..=10).contains(&level)))
        }
        _ => false,
    }
}

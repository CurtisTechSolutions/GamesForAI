use super::*;
use crate::ProviderFuture;
use std::collections::VecDeque;
use tokio::sync::Mutex;

type TestResult = Result<(), Box<dyn std::error::Error>>;
struct Script {
    responses: Mutex<VecDeque<Result<ProviderResponse, ProviderError>>>,
    requests: Mutex<Vec<ProviderRequest>>,
    delay: Duration,
}
impl Script {
    fn new(responses: Vec<ProviderResponse>) -> Self {
        Self {
            responses: Mutex::new(responses.into_iter().map(Ok).collect()),
            requests: Mutex::new(vec![]),
            delay: Duration::ZERO,
        }
    }
}
impl LlmProvider for Script {
    fn complete<'a>(&'a self, request: &'a ProviderRequest) -> ProviderFuture<'a> {
        Box::pin(async move {
            self.requests.lock().await.push(request.clone());
            tokio::time::sleep(self.delay).await;
            self.responses
                .lock()
                .await
                .pop_front()
                .unwrap_or(Err(ProviderError::Unavailable))
        })
    }
}
#[derive(Default)]
struct Planning {
    calls: Mutex<Vec<(String, Value)>>,
    delay: Duration,
}
impl TurnTools for Planning {
    fn invoke<'a>(&'a self, name: &'a str, input: &'a Value) -> ToolFuture<'a> {
        Box::pin(async move {
            self.calls.lock().await.push((name.into(), input.clone()));
            tokio::time::sleep(self.delay).await;
            ToolResult {
                content: json!({"visible_result":"projected by host"}),
                is_error: false,
            }
        })
    }
}
fn context(turn: u64) -> TurnContext {
    TurnContext {
        turn,
        state: json!({"observation":{"text":"This seat's view only"}}),
        legal_actions: vec!["r1c1".into(), "r2c2".into()],
    }
}
fn response(content: Vec<Value>, stop_reason: StopReason) -> ProviderResponse {
    ProviderResponse {
        id: "test-response".into(),
        model: "test-model".into(),
        message: ContentMessage {
            role: Role::Assistant,
            content,
        },
        usage: Usage {
            input_tokens: 10,
            output_tokens: 10,
            cache_read_input_tokens: 20,
            cache_creation_5m_tokens: 30,
            cache_creation_1h_tokens: 40,
        },
        stop_reason,
        latency_ms: 1,
        raw: json!({"complete_provider_payload":true}),
    }
}
fn direct(action: &str) -> ProviderResponse {
    response(
        vec![
            json!({"type":"text","text":json!({"action":action,"reasoning":"A short decision explanation."}).to_string()}),
        ],
        StopReason::EndTurn,
    )
}
fn tool(id: &str, name: &str, input: Value) -> Value {
    json!({"type":"tool_use","id":id,"name":name,"input":input})
}
fn move_tool(id: &str, action: &str) -> Value {
    tool(id, "make_move", json!({"action":action,"reasoning":""}))
}
fn session(mode: PlayMode, assists: AllowedAssists) -> Result<PlayerSession, ProviderError> {
    PlayerSession::new(
        PlayerConfig {
            mode,
            ..PlayerConfig::default()
        },
        "Immutable rules.",
        assists,
    )
}

#[tokio::test]
async fn direct_retries_illegal_and_refused_moves_and_retains_every_exchange() -> TestResult {
    let script = Script::new(vec![
        direct("not-legal"),
        response(vec![], StopReason::Refusal),
        direct("r2c2"),
    ]);
    let initial = session(PlayMode::Direct, AllowedAssists::default())?;
    let report = initial
        .play_turn(&context(0), &script, &Planning::default())
        .await?;
    assert!(matches!(report.outcome,TurnOutcome::Move(ref decision) if decision.action=="r2c2"));
    assert_eq!(
        report.attempts,
        vec![FailedAttempt::IllegalAction, FailedAttempt::Refusal]
    );
    assert_eq!(report.calls.len(), 3);
    assert_eq!(report.usage.total()?, 330);
    assert!(initial.messages().is_empty());
    let requests = script.requests.lock().await;
    assert_eq!(requests[0].config.max_output_tokens, 4096);
    assert_eq!(requests[1].config.max_output_tokens, 4086);
    assert_eq!(requests[2].config.max_output_tokens, 4076);
    assert!(requests.iter().all(|request| request.tools.is_empty()));
    assert!(requests.iter().all(|request| request
        .messages
        .iter()
        .all(|message| !message.content.is_empty())));
    assert!(requests[1].messages.starts_with(&requests[0].messages));
    assert_eq!(requests[0].system, requests[2].system);
    assert_eq!(initial.agent_identity()?, report.session.agent_identity()?);
    Ok(())
}

#[tokio::test]
async fn retry_exhaustion_never_fabricates_a_fallback_move() -> TestResult {
    let script = Script::new(vec![direct("invalid"); 4]);
    let report = session(PlayMode::Direct, AllowedAssists::default())?
        .play_turn(&context(0), &script, &Planning::default())
        .await?;
    assert_eq!(
        report.outcome,
        TurnOutcome::Failed(TurnFailure::RetriesExhausted)
    );
    assert_eq!(report.attempts.len(), 3);
    assert_eq!(script.requests.lock().await.len(), 3);
    Ok(())
}

#[tokio::test]
async fn direct_schema_rejects_prose_extra_fields_and_multiple_answers() -> TestResult {
    let script = Script::new(vec![
        response(
            vec![json!({"type":"text","text":"I choose r1c1"})],
            StopReason::EndTurn,
        ),
        response(
            vec![
                json!({"type":"text","text":"{\"action\":\"r1c1\",\"reasoning\":\"\",\"extra\":true}"}),
            ],
            StopReason::EndTurn,
        ),
        response(
            vec![
                json!({"type":"text","text":"{}"}),
                json!({"type":"text","text":"{}"}),
            ],
            StopReason::EndTurn,
        ),
    ]);
    let report = session(PlayMode::Direct, AllowedAssists::default())?
        .play_turn(&context(0), &script, &Planning::default())
        .await?;
    assert_eq!(
        report.outcome,
        TurnOutcome::Failed(TurnFailure::RetriesExhausted)
    );
    assert_eq!(report.attempts, vec![FailedAttempt::InvalidResponse; 3]);
    Ok(())
}

#[tokio::test]
async fn allowed_planning_uses_host_and_append_only_signed_history_survives_next_turn() -> TestResult
{
    let assists = AllowedAssists {
        allow_analysis: true,
        allow_simulation: true,
    };
    let thinking =
        json!({"type":"thinking","thinking":"Returned summary.","signature":"do-not-edit"});
    let script = Script::new(vec![
        response(
            vec![
                thinking.clone(),
                tool("state", "get_state", json!({})),
                tool("sim", "simulate_moves", json!({"lines":[["r1c1"]]})),
                tool(
                    "engine",
                    "analyze_position",
                    json!({"opponent":"minimax","level":3}),
                ),
            ],
            StopReason::ToolUse,
        ),
        response(
            vec![thinking.clone(), move_tool("move", "r1c1")],
            StopReason::ToolUse,
        ),
        response(vec![move_tool("later", "r2c2")], StopReason::ToolUse),
    ]);
    let host = Planning::default();
    let initial = session(PlayMode::Tools, assists)?;
    let report = initial.play_turn(&context(0), &script, &host).await?;
    assert_eq!(
        report.outcome,
        TurnOutcome::Move(MoveDecision {
            action: "r1c1".into(),
            reasoning: "Returned summary.".into()
        })
    );
    assert_eq!(report.tools.len(), 4);
    assert_eq!(host.calls.lock().await.len(), 2);
    assert!(report.attempts.is_empty());
    let restored: PlayerSession = serde_json::from_value(serde_json::to_value(&report.session)?)?;
    let next = restored.play_turn(&context(2), &script, &host).await?;
    assert!(next
        .session
        .messages()
        .starts_with(report.session.messages()));
    assert_eq!(next.session.messages()[1].content[0], thinking);
    assert_eq!(next.session.tools, initial.tools);
    assert!(matches!(next.outcome, TurnOutcome::Move(_)));
    assert_eq!(
        restored.play_turn(&context(0), &script, &host).await,
        Err(ProviderError::InvalidConfig)
    );
    Ok(())
}

#[tokio::test]
async fn omitted_assists_and_forged_target_arguments_never_reach_host() -> TestResult {
    let script = Script::new(vec![response(
        vec![
            tool(
                "forbidden",
                "analyze_position",
                json!({"opponent":"minimax","level":3}),
            ),
            tool("target", "get_state", json!({"seat":1})),
            move_tool("good", "r2c2"),
        ],
        StopReason::ToolUse,
    )]);
    let host = Planning::default();
    let report = session(PlayMode::Tools, AllowedAssists::default())?
        .play_turn(&context(0), &script, &host)
        .await?;
    assert!(matches!(report.outcome, TurnOutcome::Move(_)));
    assert_eq!(
        report.attempts,
        vec![
            FailedAttempt::DisallowedTool,
            FailedAttempt::InvalidResponse
        ]
    );
    assert!(host.calls.lock().await.is_empty());
    assert!(script.requests.lock().await[0]
        .tools
        .iter()
        .all(|tool| !matches!(tool.name.as_str(), "analyze_position" | "simulate_moves")));
    assert_eq!(
        report.tools[0].result.content["error"]["code"],
        "ASSIST_NOT_ALLOWED"
    );
    Ok(())
}

#[tokio::test]
async fn a_batch_selects_at_most_one_move_and_answers_all_remaining_tool_ids() -> TestResult {
    let script = Script::new(vec![response(
        vec![
            move_tool("one", "r1c1"),
            move_tool("two", "r2c2"),
            tool("three", "simulate_moves", json!({"lines":[[]]})),
        ],
        StopReason::ToolUse,
    )]);
    let host = Planning::default();
    let report = session(
        PlayMode::Tools,
        AllowedAssists {
            allow_simulation: true,
            allow_analysis: false,
        },
    )?
    .play_turn(&context(0), &script, &host)
    .await?;
    assert!(matches!(report.outcome,TurnOutcome::Move(ref decision) if decision.action=="r1c1"));
    assert!(host.calls.lock().await.is_empty());
    assert_eq!(report.tools.len(), 3);
    assert_eq!(
        report.tools[1].result.content["error"]["code"],
        "TURN_ALREADY_COMPLETED"
    );
    let last = report.session.messages().last().ok_or("missing replies")?;
    assert_eq!(
        last.content
            .iter()
            .map(|block| block["tool_use_id"].as_str())
            .collect::<Vec<_>>(),
        vec![Some("one"), Some("two"), Some("three")]
    );
    Ok(())
}

#[tokio::test]
async fn retries_stop_mid_batch_at_the_exact_limit() -> TestResult {
    let script = Script::new(vec![response(
        vec![
            move_tool("one", "bad"),
            move_tool("two", "bad"),
            move_tool("three", "bad"),
            move_tool("four", "r1c1"),
        ],
        StopReason::ToolUse,
    )]);
    let report = session(PlayMode::Tools, AllowedAssists::default())?
        .play_turn(&context(0), &script, &Planning::default())
        .await?;
    assert_eq!(
        report.outcome,
        TurnOutcome::Failed(TurnFailure::RetriesExhausted)
    );
    assert_eq!(
        report.tools[3].result.content["error"]["code"],
        "RETRIES_EXHAUSTED"
    );
    assert_eq!(report.attempts.len(), 3);
    Ok(())
}

#[tokio::test]
async fn per_move_output_limit_is_shared_by_every_response() -> TestResult {
    let initial = PlayerSession::new(
        PlayerConfig {
            mode: PlayMode::Direct,
            max_output_tokens: 20,
            ..PlayerConfig::default()
        },
        "Rules.",
        AllowedAssists::default(),
    )?;
    let script = Script::new(vec![response(vec![], StopReason::MaxTokens); 3]);
    let report = initial
        .play_turn(&context(0), &script, &Planning::default())
        .await?;
    assert_eq!(
        report.outcome,
        TurnOutcome::Failed(TurnFailure::OutputLimit)
    );
    assert_eq!(report.calls.len(), 2);
    assert_eq!(report.calls[1].request.config.max_output_tokens, 10);
    assert_eq!(report.usage.output_tokens, 20);
    assert_eq!(report.attempts, vec![FailedAttempt::MaxTokens; 2]);
    Ok(())
}

#[tokio::test]
async fn tools_cannot_bypass_direct_mode_or_execute_on_refusal() -> TestResult {
    let host = Planning::default();
    let initial = session(
        PlayMode::Direct,
        AllowedAssists {
            allow_simulation: true,
            allow_analysis: true,
        },
    )?;
    let script = Script::new(vec![
        response(vec![move_tool("unexpected", "r1c1")], StopReason::ToolUse),
        direct("r2c2"),
    ]);
    let report = initial.play_turn(&context(0), &script, &host).await?;
    assert_eq!(report.attempts, vec![FailedAttempt::DisallowedTool]);
    assert!(matches!(report.outcome,TurnOutcome::Move(ref decision) if decision.action=="r2c2"));
    let script = Script::new(vec![
        response(vec![move_tool("refused", "r1c1")], StopReason::Refusal),
        response(vec![move_tool("retry", "r2c2")], StopReason::ToolUse),
    ]);
    let report = session(PlayMode::Tools, AllowedAssists::default())?
        .play_turn(&context(0), &script, &host)
        .await?;
    assert_eq!(report.attempts, vec![FailedAttempt::Refusal]);
    assert!(matches!(report.outcome,TurnOutcome::Move(ref decision) if decision.action=="r2c2"));
    assert!(host.calls.lock().await.is_empty());
    Ok(())
}

#[tokio::test]
async fn endless_planning_has_a_call_limit() -> TestResult {
    let script = Script::new(vec![
        response(
            vec![tool("state", "get_state", json!({}))],
            StopReason::ToolUse
        );
        17
    ]);
    let report = session(PlayMode::Tools, AllowedAssists::default())?
        .play_turn(&context(0), &script, &Planning::default())
        .await?;
    assert_eq!(report.outcome, TurnOutcome::Failed(TurnFailure::CallLimit));
    assert_eq!(report.calls.len(), 16);
    assert!(report.attempts.is_empty());
    Ok(())
}

#[tokio::test]
async fn deadline_includes_provider_and_host_tools_and_cancellation_keeps_original_session(
) -> TestResult {
    let mut script = Script::new(vec![direct("r1c1")]);
    script.delay = Duration::from_millis(200);
    let initial = PlayerSession::new(
        PlayerConfig {
            move_timeout_ms: 50,
            ..PlayerConfig::default()
        },
        "Rules.",
        AllowedAssists {
            allow_simulation: true,
            allow_analysis: false,
        },
    )?;
    let report = initial
        .play_turn(&context(0), &script, &Planning::default())
        .await?;
    assert_eq!(report.outcome, TurnOutcome::Failed(TurnFailure::Timeout));
    assert_eq!(report.calls[0].result, Err(ProviderError::Timeout));
    assert!(initial.messages().is_empty());
    let script = Script::new(vec![response(
        vec![
            tool("slow", "simulate_moves", json!({"lines":[[]]})),
            move_tool("late", "r1c1"),
        ],
        StopReason::ToolUse,
    )]);
    let host = Planning {
        delay: Duration::from_millis(200),
        ..Planning::default()
    };
    let report = initial.play_turn(&context(0), &script, &host).await?;
    assert_eq!(report.outcome, TurnOutcome::Failed(TurnFailure::Timeout));
    assert!(report.tools.iter().all(|exchange| exchange.result.is_error));
    assert_eq!(
        report
            .session
            .messages()
            .last()
            .ok_or("missing replies")?
            .content
            .len(),
        2
    );
    Ok(())
}

#[tokio::test]
async fn provider_budget_failure_is_not_retried_and_malformed_calls_are_retained_for_audit(
) -> TestResult {
    let script = Script::new(vec![]);
    script
        .responses
        .lock()
        .await
        .push_back(Err(ProviderError::BudgetExhausted));
    let report = session(PlayMode::Direct, AllowedAssists::default())?
        .play_turn(&context(0), &script, &Planning::default())
        .await?;
    assert_eq!(
        report.outcome,
        TurnOutcome::Failed(TurnFailure::Provider(ProviderError::BudgetExhausted))
    );
    assert_eq!(report.calls.len(), 1);
    let malformed = response(
        vec![
            move_tool("duplicate", "r1c1"),
            move_tool("duplicate", "r2c2"),
        ],
        StopReason::ToolUse,
    );
    let script = Script::new(vec![malformed.clone()]);
    let report = session(PlayMode::Tools, AllowedAssists::default())?
        .play_turn(&context(0), &script, &Planning::default())
        .await?;
    assert_eq!(
        report.outcome,
        TurnOutcome::Failed(TurnFailure::Provider(ProviderError::InvalidResponse))
    );
    assert_eq!(report.calls[0].result, Ok(malformed));
    assert_eq!(report.session.messages().len(), 1);
    Ok(())
}

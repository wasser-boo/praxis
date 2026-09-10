//! End-to-end handler regressions: real renderer, synthetic provider, temporary files/DB.
//! No Discord, media provider, credentials or live services are used.
use super::*;
use crate::gateway::llm::{
    error::{ErrorKind, ProviderError},
    provider::{ChatResponse, FunctionCall, LLMProvider, ToolCall},
    resilience::ResilienceConfig,
    LLMRouter,
};
use std::{
    collections::VecDeque,
    sync::{Arc, Mutex},
};

enum Step {
    Reply(ChatResponse),
    Unavailable,
    Cancel(ChatResponse),
}
struct ScriptedProvider {
    user: String,
    steps: Mutex<VecDeque<Step>>,
    requests: Arc<Mutex<Vec<ChatRequest>>>,
}
#[async_trait::async_trait]
impl LLMProvider for ScriptedProvider {
    fn name(&self) -> &str {
        "offline-tool-chain"
    }
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
    async fn chat(&self, request: ChatRequest) -> anyhow::Result<ChatResponse> {
        self.requests.lock().unwrap().push(request);
        match self
            .steps
            .lock()
            .unwrap()
            .pop_front()
            .expect("unexpected extra generation")
        {
            Step::Reply(response) => Ok(response),
            Step::Unavailable => Err(ProviderError::new(ErrorKind::Unavailable).into()),
            Step::Cancel(response) => {
                crate::gateway::task_control::cancel(&self.user);
                Ok(response)
            }
        }
    }
}
fn call(id: &str, name: &str, args: serde_json::Value) -> ToolCall {
    ToolCall {
        id: id.into(),
        function: FunctionCall {
            name: name.into(),
            arguments: args.to_string(),
        },
    }
}
fn reply(content: Option<&str>, calls: Vec<ToolCall>) -> ChatResponse {
    ChatResponse {
        content: content.map(String::from),
        tool_calls: (!calls.is_empty()).then_some(calls),
        finish_reason: None,
        usage: None,
    }
}
fn fixture(
    dir: &std::path::Path,
    steps: Vec<Step>,
) -> (GatewayState, String, Arc<Mutex<Vec<ChatRequest>>>) {
    let db = crate::db::Database::new(dir).unwrap();
    crate::db::tools::init_default_tools(&db).unwrap();
    let user = format!("tool-chain-{}", uuid::Uuid::new_v4());
    db.save_context(&db.load_context(&user).unwrap()).unwrap();
    let requests = Arc::new(Mutex::new(Vec::new()));
    let router = LLMRouter::with_providers(
        vec![Box::new(ScriptedProvider {
            user: user.clone(),
            steps: Mutex::new(steps.into()),
            requests: requests.clone(),
        })],
        "offline-tool-chain".into(),
        vec![],
        ResilienceConfig {
            max_attempts: 2,
            initial_backoff_ms: 1,
            max_backoff_ms: 1,
            ..Default::default()
        },
    );
    let state = GatewayState {
        db,
        config: crate::config::Config::from_env(),
        secrets: Default::default(),
        llm: Arc::new(router),
        plugins: Arc::new(crate::plugins::PluginRegistry::new()),
        event_tx: tokio::sync::broadcast::channel(16).0,
        start_time: std::time::Instant::now(),
    };
    (state, user, requests)
}
fn assert_paired_history(state: &GatewayState, user: &str, expected_calls: usize) {
    let history = state.db.get_messages(user, 100).unwrap();
    let calls: Vec<_> = history
        .iter()
        .filter_map(|m| m.tool_calls.as_ref())
        .flatten()
        .collect();
    assert_eq!(calls.len(), expected_calls);
    for call in calls {
        let results: Vec<_> = history
            .iter()
            .filter(|m| m.tool_call_id.as_deref() == Some(&call.id))
            .collect();
        assert_eq!(
            results.len(),
            1,
            "each requested tool needs exactly one durable receipt"
        );
        assert_eq!(
            results[0].tool_name.as_deref(),
            Some(call.function.name.as_str())
        );
    }
    assert!(
        !history.iter().any(|m| m.role == "assistant"
            && m.tool_calls.is_none()
            && m.content.trim().is_empty()),
        "never persist an empty final answer"
    );
    assert!(crate::gateway::task_control::cancellation(user).is_none());
}

#[tokio::test]
#[ignore = "Requires Node and POML_CLI; provider and all inputs are synthetic"]
async fn tool_chain_skill_then_reference_then_action_reaches_final_answer() {
    for max_turns in [None, Some(5)] {
        for with_history in [false, true] {
            let dir = tempfile::tempdir().unwrap();
            let output = dir.path().join("synthetic-artifact.txt");
            let reference = std::env::current_dir()
                .unwrap()
                .join("skills/mnemodim-palace/references/format-cheatsheet.md");
            let steps = vec![
                Step::Reply(reply(
                    None,
                    vec![call(
                        "skill",
                        "use_skill",
                        serde_json::json!({"name":"mnemodim-palace","parameters":{"user_request":"offline synthetic inspection; no media generation"}}),
                    )],
                )),
                Step::Reply(reply(
                    Some(""),
                    vec![call(
                        "reference",
                        "read_file",
                        serde_json::json!({"path":reference}),
                    )],
                )),
                Step::Reply(reply(
                    Some(" \t"),
                    vec![call(
                        "action",
                        "write_file",
                        serde_json::json!({"path":output,"content":"synthetic artifact"}),
                    )],
                )),
                Step::Reply(reply(Some("Synthetic task finished."), vec![])),
            ];
            let (state, user, requests) = fixture(dir.path(), steps);
            state.db.merge_context(&user, serde_json::json!({"settings.max_llm_turns":max_turns,"settings.history_with_toolcalls":with_history})).unwrap();
            let result = handle_message(&state, &user, "offline test", Some("web"))
                .await
                .unwrap();
            assert_eq!(result, "Synthetic task finished.");
            assert_eq!(
                std::fs::read_to_string(output).unwrap(),
                "synthetic artifact"
            );
            assert_paired_history(&state, &user, 3);
            let requests = requests.lock().unwrap();
            assert_eq!(requests.len(), 4);
            for (i, request) in requests.iter().enumerate() {
                assert!(
                    request
                        .tools
                        .as_ref()
                        .unwrap()
                        .iter()
                        .any(|t| t.function.name == "read_file"),
                    "tools lost at continuation {i}"
                );
                assert_eq!(
                    request.messages.iter().filter(|m| m.role == "tool").count(),
                    i,
                    "current tool results lost at continuation {i}"
                );
            }
            assert!(requests[1].messages.iter().any(|m| m.role == "tool"
                && m.content
                    .as_deref()
                    .unwrap_or("")
                    .contains("references/format-cheatsheet.md")));
            assert!(requests[2]
                .messages
                .iter()
                .any(|m| m.tool_call_id.as_deref() == Some("reference")
                    && !m.content.as_deref().unwrap_or("").starts_with("Error:")));
        }
    }
}

#[tokio::test]
#[cfg(unix)]
#[ignore = "Requires Node and POML_CLI; only temporary local side effects"]
async fn tool_chain_retry_never_reexecutes_previous_rounds() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("effects");
    let (state, user, requests) = fixture(
        dir.path(),
        vec![
            Step::Reply(reply(
                None,
                vec![call(
                    "a",
                    "execute_terminal",
                    serde_json::json!({"command":format!("printf a >> '{}'",path.display())}),
                )],
            )),
            Step::Reply(reply(
                None,
                vec![call(
                    "b",
                    "execute_terminal",
                    serde_json::json!({"command":format!("printf b >> '{}'",path.display())}),
                )],
            )),
            Step::Unavailable,
            Step::Reply(reply(Some("Done."), vec![])),
        ],
    );
    assert_eq!(
        handle_message(&state, &user, "offline test", Some("web"))
            .await
            .unwrap(),
        "Done."
    );
    assert_eq!(std::fs::read_to_string(path).unwrap(), "ab");
    assert_paired_history(&state, &user, 2);
    let requests = requests.lock().unwrap();
    assert_eq!(requests.len(), 4);
    assert_eq!(
        serde_json::to_value(&requests[2]).unwrap(),
        serde_json::to_value(&requests[3]).unwrap()
    );
}

#[tokio::test]
#[ignore = "Requires Node and POML_CLI; no real provider calls"]
async fn tool_chain_budget_blocks_excess_calls_and_refuses_tool_only_finalization() {
    let dir = tempfile::tempdir().unwrap();
    let paths: Vec<_> = (0..3)
        .map(|i| dir.path().join(format!("effect-{i}")))
        .collect();
    let calls: Vec<_> = paths
        .iter()
        .enumerate()
        .map(|(i, p)| {
            call(
                &format!("call-{i}"),
                "write_file",
                serde_json::json!({"path":p,"content":"effect"}),
            )
        })
        .collect();
    let (state, user, requests) = fixture(
        dir.path(),
        vec![
            Step::Reply(reply(None, calls[..2].to_vec())),
            Step::Reply(reply(None, calls[2..].to_vec())),
        ],
    );
    state
        .db
        .merge_context(&user, serde_json::json!({"settings.max_tool_calls":1}))
        .unwrap();
    let error = handle_message(&state, &user, "offline test", Some("web"))
        .await
        .unwrap_err();
    assert!(error.to_string().contains("tool-call limit"), "{error}");
    assert!(paths[0].exists());
    assert!(!paths[1].exists() && !paths[2].exists());
    assert_paired_history(&state, &user, 3);
    let requests = requests.lock().unwrap();
    assert_eq!(requests.len(), 2);
    assert!(requests[1].tools.is_none());
}

#[tokio::test]
#[ignore = "Requires Node and POML_CLI; no real provider calls"]
async fn tool_chain_zero_budget_and_true_empty_response_are_not_silent_successes() {
    for max in [0, -1] {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("must-not-exist");
        let (state, user, requests) = fixture(
            dir.path(),
            vec![Step::Reply(reply(
                None,
                vec![call(
                    "blocked",
                    "write_file",
                    serde_json::json!({"path":path,"content":"effect"}),
                )],
            ))],
        );
        state
            .db
            .merge_context(&user, serde_json::json!({"settings.max_tool_calls":max}))
            .unwrap();
        assert!(handle_message(&state, &user, "offline test", Some("web"))
            .await
            .is_err());
        assert!(!path.exists());
        assert!(requests.lock().unwrap()[0].tools.is_none());
        assert_paired_history(&state, &user, 1);
    }
    let dir = tempfile::tempdir().unwrap();
    let (state, user, requests) =
        fixture(dir.path(), vec![Step::Reply(reply(Some(" \n\t"), vec![]))]);
    let error = handle_message(&state, &user, "offline test", Some("web"))
        .await
        .unwrap_err();
    assert_eq!(
        error
            .downcast_ref::<crate::gateway::llm::error::CallFailure>()
            .unwrap()
            .terminal
            .kind,
        ErrorKind::InvalidResponse
    );
    assert_eq!(requests.lock().unwrap().len(), 1);
    assert_paired_history(&state, &user, 0);
}

#[tokio::test]
#[ignore = "Requires Node and POML_CLI; cancellation and files are local mocks"]
async fn tool_chain_cancellation_persists_receipts_without_further_effects() {
    let dir = tempfile::tempdir().unwrap();
    let paths: Vec<_> = (0..3)
        .map(|i| dir.path().join(format!("effect-{i}")))
        .collect();
    let calls: Vec<_> = paths
        .iter()
        .enumerate()
        .map(|(i, p)| {
            call(
                &format!("call-{i}"),
                "write_file",
                serde_json::json!({"path":p,"content":"effect"}),
            )
        })
        .collect();
    let (state, user, requests) = fixture(
        dir.path(),
        vec![
            Step::Reply(reply(None, calls[..1].to_vec())),
            Step::Cancel(reply(None, calls[1..].to_vec())),
        ],
    );
    let error = handle_message(&state, &user, "offline test", Some("web"))
        .await
        .unwrap_err();
    assert!(error.to_string().contains("cancelled"), "{error}");
    assert!(paths[0].exists());
    assert!(!paths[1].exists() && !paths[2].exists());
    assert_paired_history(&state, &user, 3);
    assert_eq!(requests.lock().unwrap().len(), 2);
}

#[tokio::test]
#[ignore = "Requires Node and POML_CLI; no real provider calls"]
async fn tool_chain_agent_limit_never_returns_a_previous_tasks_answer() {
    let dir = tempfile::tempdir().unwrap();
    let (state, user, _) = fixture(
        dir.path(),
        vec![
            Step::Reply(reply(
                None,
                vec![call(
                    "one",
                    "get_context",
                    serde_json::json!({"key":"mode"}),
                )],
            )),
            Step::Reply(reply(
                None,
                vec![call(
                    "two",
                    "get_context",
                    serde_json::json!({"key":"turn"}),
                )],
            )),
        ],
    );
    state
        .db
        .merge_context(&user, serde_json::json!({"settings.max_llm_turns":2}))
        .unwrap();
    state
        .db
        .add_message(
            &user,
            &crate::db::messages::Message::assistant("Old unrelated answer".into()),
        )
        .unwrap();
    let error = handle_message(&state, &user, "offline test", Some("web"))
        .await
        .unwrap_err();
    assert!(error.to_string().contains("turn limit"), "{error}");
    assert_paired_history(&state, &user, 2);
}

#[tokio::test]
#[ignore = "Requires Node and POML_CLI; no real provider calls"]
async fn tool_chain_budget_allows_one_honest_finalization_and_counts_invalid_calls() {
    for valid in [true, false] {
        let dir = tempfile::tempdir().unwrap();
        let tool = if valid {
            call("last", "get_context", serde_json::json!({"key":"mode"}))
        } else {
            call("last", "nonexistent_tool", serde_json::json!({}))
        };
        let (state, user, requests) = fixture(
            dir.path(),
            vec![
                Step::Reply(reply(None, vec![tool])),
                Step::Reply(reply(
                    Some("Partial status: no artifact was generated."),
                    vec![],
                )),
            ],
        );
        state
            .db
            .merge_context(&user, serde_json::json!({"settings.max_tool_calls":1}))
            .unwrap();
        assert_eq!(
            handle_message(&state, &user, "offline test", Some("web"))
                .await
                .unwrap(),
            "Partial status: no artifact was generated."
        );
        assert_paired_history(&state, &user, 1);
        assert_eq!(state.db.load_context(&user).unwrap().turn, 1);
        let requests = requests.lock().unwrap();
        assert_eq!(requests.len(), 2);
        assert!(requests[1].tools.is_none());
        assert!(requests[1]
            .messages
            .iter()
            .any(|m| m.role == "system" && m.content.as_deref().unwrap_or("").contains("budget")));
    }
}

#[test]
fn tool_chain_empty_tts_returns_before_spawning_any_task() {
    // No Tokio runtime: trying to spawn even a doomed TTS task must fail this test.
    for text in ["", " \n\t", "\u{a0}\u{2003}"] {
        spawn_tts(
            text.into(),
            &Default::default(),
            &Default::default(),
            "no-tts-for-empty-text",
        );
    }
}

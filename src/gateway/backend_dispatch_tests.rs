//! Runs unchanged inside BOTH active message dispatchers; temporary DB only.
use crate::gateway::llm::provider::{FunctionCall, ToolCall};

async fn invoke(
    db: &crate::db::Database,
    user: &str,
    name: &str,
    args: serde_json::Value,
) -> String {
    super::execute_tool_call(
        db,
        user,
        &ToolCall {
            id: "backend-test".into(),
            function: FunctionCall {
                name: name.into(),
                arguments: args.to_string(),
            },
        },
        &crate::plugins::PluginRegistry::new(),
    )
    .await
}

#[tokio::test]
async fn backend_dispatcher_memory_is_durable_typed_and_user_scoped() {
    let dir = tempfile::tempdir().unwrap();
    let db = crate::db::Database::new(dir.path()).unwrap();
    db.merge_context(
        "alice",
        serde_json::json!({"custom_data.keep": "unchanged"}),
    )
    .unwrap();
    for _ in 0..2 {
        let result = invoke(
            &db,
            "alice",
            "learn_fact",
            serde_json::json!({"fact": "one fact"}),
        )
        .await;
        assert!(!result.starts_with("Error"), "{result}");
    }
    for value in [serde_json::json!("old"), serde_json::json!({"brief": true})] {
        let result = invoke(
            &db,
            "alice",
            "learn_preference",
            serde_json::json!({"key": "style", "value": value}),
        )
        .await;
        assert!(!result.starts_with("Error"), "{result}");
    }
    let memory = crate::db::memory::load_memory(&db, "alice").unwrap();
    assert_eq!(memory.learned_facts, vec!["one fact"]);
    assert_eq!(
        memory.user_preferences["style"],
        serde_json::json!({"brief": true})
    );
    assert_eq!(
        db.load_context("alice").unwrap().custom_data["keep"],
        "unchanged"
    );
    assert!(crate::db::memory::load_memory(&db, "bob")
        .unwrap()
        .user_preferences
        .is_empty());
    db.conn().execute_batch("DROP TABLE memory").unwrap();
    let result = invoke(
        &db,
        "alice",
        "learn_preference",
        serde_json::json!({"key":"style", "value":"failed"}),
    )
    .await;
    assert!(result.starts_with("Error:"), "{result}");
}

#[tokio::test]
async fn backend_dispatcher_accepts_legacy_sm_update_keys() {
    let dir = tempfile::tempdir().unwrap();
    let db = crate::db::Database::new(dir.path()).unwrap();
    for (key, value) in [
        ("cl_file", serde_json::json!("legacy")),
        ("settings.cl_file", serde_json::json!("chosen")),
        ("cl_data.keep", serde_json::json!(true)),
    ] {
        let result = invoke(
            &db,
            "alice",
            "set_context",
            serde_json::json!({"key": key, "value": value}),
        )
        .await;
        assert!(!result.starts_with("Error:"), "{result}");
    }
    let context = serde_json::to_value(db.load_context("alice").unwrap()).unwrap();
    assert_eq!(context["sm_file"], "legacy");
    assert_eq!(context["settings"]["sm_file"], "chosen");
    assert_eq!(context["cl_data"]["keep"], true);
    assert!(context.get("cl_file").is_none());
}

use super::*;

#[test]
fn download_and_feedback_permissions_are_explicit_and_persisted() {
    let dir = tempfile::tempdir().unwrap();
    let db = Database::new(dir.path()).unwrap();
    let settings = db.load_context("alice").unwrap().settings;
    assert!(!settings.download);
    assert!(!settings.feedback_enabled);
    for key in ["settings.download", "settings.feedback_enabled"] {
        assert!(db
            .merge_context_from_agent("alice", serde_json::json!({key:true}))
            .is_err());
        db.merge_context("alice", serde_json::json!({key:true}))
            .unwrap();
    }
    let settings = db.load_context("alice").unwrap().settings;
    assert!(settings.download);
    assert!(settings.feedback_enabled);
    db.merge_context(
        "alice",
        serde_json::json!({"settings.download":false,"settings.feedback_enabled":false}),
    )
    .unwrap();
    let settings = db.load_context("alice").unwrap().settings;
    assert!(!settings.download);
    assert!(!settings.feedback_enabled);
}

#[test]
fn feedback_limits_are_validated_without_saving_bad_updates() {
    let dir = tempfile::tempdir().unwrap();
    let db = Database::new(dir.path()).unwrap();
    for patch in [
        serde_json::json!({"settings.feedback_max_per_5min":-1}),
        serde_json::json!({"settings.feedback_max_per_5min":1001}),
        serde_json::json!({"settings.feedback_window_secs":0}),
        serde_json::json!({"settings.feedback_window_secs":86401}),
    ] {
        assert!(db.merge_context("alice", patch).is_err());
    }
    db.merge_context(
        "alice",
        serde_json::json!({"settings.feedback_max_per_5min":0,"settings.feedback_window_secs":60}),
    )
    .unwrap();
    let settings = db.load_context("alice").unwrap().settings;
    assert_eq!(settings.feedback_max_per_5min, 0);
    assert_eq!(settings.feedback_window_secs, 60);
}

#[test]
fn retired_settings_load_but_are_not_written_back_as_active_options() {
    let settings: ContextSettings = serde_json::from_value(serde_json::json!({
        "tool_result_limit":1,"feedback_template":"obsolete", "download":true
    }))
    .unwrap();
    assert_eq!(settings.tool_result_limit, Some(1));
    let saved = serde_json::to_value(settings).unwrap();
    assert!(saved.get("tool_result_limit").is_none());
    assert!(saved.get("feedback_template").is_none());
    assert_eq!(saved["download"], true);
}

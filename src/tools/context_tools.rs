pub fn set_context_value(
    db: &crate::db::Database,
    user_id: &str,
    key: &str,
    value: &str,
) -> anyhow::Result<()> {
    let key = crate::db::contexts::canonical_context_key(key);
    if key.contains('.') || matches!(key, "system_template" | "active_skill") {
        let key = if key.contains('.') { key.to_string() } else { format!("settings.{key}") };
        let value = crate::context_cmd::parse_value(value)?;
        db.merge_context(user_id, serde_json::json!({key: value}))?;
        return Ok(());
    }
    let mut ctx = db.load_context(user_id)?;

    match key {
        "mode" => ctx.mode = value.to_string(),
        "username" => ctx.username = Some(value.to_string()),
        "sm_file" => ctx.sm_file = Some(value.to_string()),
        "active_state" => ctx.active_state = Some(value.to_string()),
        "voice_enabled" => ctx.settings.voice_enabled = value.parse().unwrap_or(false),
        "voice_tts_enabled" => ctx.settings.voice_tts_enabled = value.parse().unwrap_or(false),
        "use_tts" => ctx.settings.use_tts = value.parse().unwrap_or(false),
        "use_stt" => ctx.settings.use_stt = value.parse().unwrap_or(true),
        "voice_muted" => ctx.settings.voice_muted = value.parse().unwrap_or(false),
        "voice_deafened" => ctx.settings.voice_deafened = value.parse().unwrap_or(true),
        "voice_auto_pause_enabled" => {
            ctx.settings.voice_auto_pause_enabled = value.parse().unwrap_or(false)
        }
        "max_llm_turns" => ctx.settings.max_llm_turns = value.parse().ok(),
        "max_tool_calls" => ctx.settings.max_tool_calls = value.parse().ok(),
        "provider" => ctx.settings.provider = Some(value.to_string()),
        "model" => ctx.settings.model = Some(value.to_string()),
        "vision_provider" => ctx.settings.vision_provider = Some(value.to_string()),
        "vision_model" => ctx.settings.vision_model = Some(value.to_string()),
        _ => {
            if ctx.custom_data.is_null() {
                ctx.custom_data = serde_json::json!({});
            }
            if let Some(obj) = ctx.custom_data.as_object_mut() {
                obj.insert(
                    key.to_string(),
                    serde_json::Value::String(value.to_string()),
                );
            }
        }
    }

    db.save_context(&ctx)?;
    Ok(())
}

pub fn delete_context_value(
    db: &crate::db::Database,
    user_id: &str,
    key: &str,
) -> anyhow::Result<()> {
    let key = crate::db::contexts::canonical_context_key(key);
    if key.contains('.') || matches!(key, "system_template" | "active_skill") {
        let key = if key.contains('.') { key.to_string() } else { format!("settings.{key}") };
        db.merge_context(user_id, serde_json::json!({key: null}))?;
        return Ok(());
    }
    let mut ctx = db.load_context(user_id)?;

    match key {
        "username" => ctx.username = None,
        "sm_file" => ctx.sm_file = None,
        "active_state" => ctx.active_state = None,
        "max_llm_turns" => ctx.settings.max_llm_turns = None,
        "max_tool_calls" => ctx.settings.max_tool_calls = None,
        "provider" => ctx.settings.provider = None,
        "model" => ctx.settings.model = None,
        "vision_provider" => ctx.settings.vision_provider = None,
        "vision_model" => ctx.settings.vision_model = None,
        _ => {
            if let Some(obj) = ctx.custom_data.as_object_mut() {
                obj.remove(key);
            }
        }
    }

    db.save_context(&ctx)?;
    Ok(())
}

#[cfg(test)]
mod tool_tests {
    use super::*;
    use tempfile::TempDir;

    fn test_db() -> (crate::db::Database, TempDir) {
        let dir = TempDir::new().unwrap();
        let db = crate::db::Database::new(dir.path()).unwrap();
        let ctx = crate::db::contexts::Context {
            user_id: "user1".to_string(),
            ..Default::default()
        };
        db.save_context(&ctx).unwrap();
        (db, dir)
    }

    #[test]
    fn test_set_context_mode() {
        let (db, _dir) = test_db();
        set_context_value(&db, "user1", "mode", "coding").unwrap();
        let ctx = db.load_context("user1").unwrap();
        assert_eq!(ctx.mode, "coding");
    }

    #[test]
    fn test_set_context_username() {
        let (db, _dir) = test_db();
        set_context_value(&db, "user1", "username", "Alice").unwrap();
        let ctx = db.load_context("user1").unwrap();
        assert_eq!(ctx.username, Some("Alice".to_string()));
    }

    #[test]
    fn test_set_context_custom() {
        let (db, _dir) = test_db();
        set_context_value(&db, "user1", "theme", "dark").unwrap();
        let ctx = db.load_context("user1").unwrap();
        assert_eq!(ctx.custom_data["theme"], "dark");
    }

    #[test]
    fn test_delete_context_value() {
        let (db, _dir) = test_db();
        set_context_value(&db, "user1", "username", "Alice").unwrap();
        delete_context_value(&db, "user1", "username").unwrap();
        let ctx = db.load_context("user1").unwrap();
        assert!(ctx.username.is_none());
    }
}

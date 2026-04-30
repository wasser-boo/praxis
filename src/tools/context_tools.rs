pub fn set_context_value(db: &crate::db::Database, user_id: &str, key: &str, value: &str) -> anyhow::Result<()> {
    let mut ctx = db.load_context(user_id)?;

    match key {
        "mode" => ctx.mode = value.to_string(),
        "user_name" => ctx.user_name = Some(value.to_string()),
        "cl_file" => ctx.cl_file = Some(value.to_string()),
        "active_state" => ctx.active_state = Some(value.to_string()),
        "voice_enabled" => ctx.settings.voice_enabled = value.parse().unwrap_or(false),
        "voice_tts_enabled" => ctx.settings.voice_tts_enabled = value.parse().unwrap_or(false),
        "use_tts" => ctx.settings.use_tts = value.parse().unwrap_or(false),
        "voice_muted" => ctx.settings.voice_muted = value.parse().unwrap_or(false),
        "voice_deafened" => ctx.settings.voice_deafened = value.parse().unwrap_or(true),
        "voice_auto_pause_enabled" => ctx.settings.voice_auto_pause_enabled = value.parse().unwrap_or(false),
        "max_llm_turns" => ctx.settings.max_llm_turns = value.parse().ok(),
        "max_tool_calls" => ctx.settings.max_tool_calls = value.parse().ok(),
        _ => {
            if ctx.custom_data.is_null() {
                ctx.custom_data = serde_json::json!({});
            }
            if let Some(obj) = ctx.custom_data.as_object_mut() {
                obj.insert(key.to_string(), serde_json::Value::String(value.to_string()));
            }
        }
    }

    db.save_context(&ctx)?;
    Ok(())
}

pub fn delete_context_value(db: &crate::db::Database, user_id: &str, key: &str) -> anyhow::Result<()> {
    let mut ctx = db.load_context(user_id)?;

    match key {
        "user_name" => ctx.user_name = None,
        "cl_file" => ctx.cl_file = None,
        "active_state" => ctx.active_state = None,
        "max_llm_turns" => ctx.settings.max_llm_turns = None,
        "max_tool_calls" => ctx.settings.max_tool_calls = None,
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
    fn test_set_context_user_name() {
        let (db, _dir) = test_db();
        set_context_value(&db, "user1", "user_name", "Alice").unwrap();
        let ctx = db.load_context("user1").unwrap();
        assert_eq!(ctx.user_name, Some("Alice".to_string()));
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
        set_context_value(&db, "user1", "user_name", "Alice").unwrap();
        delete_context_value(&db, "user1", "user_name").unwrap();
        let ctx = db.load_context("user1").unwrap();
        assert!(ctx.user_name.is_none());
    }
}

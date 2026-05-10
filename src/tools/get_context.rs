pub fn get_context(
    db: &crate::db::Database,
    user_id: &str,
    key: &str,
) -> anyhow::Result<Option<String>> {
    let ctx = db.load_context(user_id)?;

    let value = match key {
        "mode" => Some(ctx.mode.clone()),
        "username" => ctx.username.clone(),
        "cl_file" => ctx.cl_file.clone(),
        "active_state" => ctx.active_state.clone(),
        "turn" => Some(ctx.turn.to_string()),
        "voice_enabled" => Some(ctx.settings.voice_enabled.to_string()),
        "voice_tts_enabled" => Some(ctx.settings.voice_tts_enabled.to_string()),
        "use_tts" => Some(ctx.settings.use_tts.to_string()),
        "use_stt" => Some(ctx.settings.use_stt.to_string()),
        "voice_muted" => Some(ctx.settings.voice_muted.to_string()),
        "voice_deafened" => Some(ctx.settings.voice_deafened.to_string()),
        "voice_auto_pause_enabled" => Some(ctx.settings.voice_auto_pause_enabled.to_string()),
        "max_llm_turns" => ctx.settings.max_llm_turns.map(|v| v.to_string()),
        "max_tool_calls" => ctx.settings.max_tool_calls.map(|v| v.to_string()),
        _ => ctx.custom_data.get(key).and_then(|v| {
            if v.is_string() {
                v.as_str().map(|s| s.to_string())
            } else {
                Some(v.to_string())
            }
        }),
    };

    Ok(value)
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
            mode: "coding".to_string(),
            username: Some("Alice".to_string()),
            ..Default::default()
        };
        db.save_context(&ctx).unwrap();
        (db, dir)
    }

    #[test]
    fn test_get_context_mode() {
        let (db, _dir) = test_db();
        let value = get_context(&db, "user1", "mode").unwrap();
        assert_eq!(value, Some("coding".to_string()));
    }

    #[test]
    fn test_get_context_username() {
        let (db, _dir) = test_db();
        let value = get_context(&db, "user1", "username").unwrap();
        assert_eq!(value, Some("Alice".to_string()));
    }

    #[test]
    fn test_get_context_turn() {
        let (db, _dir) = test_db();
        let value = get_context(&db, "user1", "turn").unwrap();
        assert_eq!(value, Some("0".to_string()));
    }

    #[test]
    fn test_get_context_nonexistent() {
        let (db, _dir) = test_db();
        let value = get_context(&db, "user1", "nonexistent").unwrap();
        assert!(value.is_none());
    }
}

use super::Database;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Context {
    pub user_id: String,
    #[serde(default)]
    pub turn: i32,
    #[serde(default = "default_mode")]
    pub mode: String,
    #[serde(default)]
    pub user_name: Option<String>,
    #[serde(default)]
    pub cl_file: Option<String>,
    #[serde(default)]
    pub active_state: Option<String>,
    #[serde(default)]
    pub active_templates: Vec<String>,
    #[serde(default)]
    pub settings: ContextSettings,
    #[serde(default)]
    pub custom_data: serde_json::Value,
    /// Workflow-specific data for CL system (e.g., application context, device, etc.)
    /// Supports deep merge via dot-notation in set_context
    #[serde(default)]
    pub cl_data: serde_json::Value,
    /// Session identifier for multi-session support (e.g., "default", "session-2")
    #[serde(default)]
    pub session_id: String,
}

impl Default for Context {
    fn default() -> Self {
        Self {
            user_id: String::new(),
            turn: 0,
            mode: default_mode(),
            user_name: None,
            cl_file: None,
            active_state: None,
            active_templates: Vec::new(),
            settings: ContextSettings::default(),
            custom_data: serde_json::Value::Null,
            cl_data: serde_json::Value::Null,
            session_id: String::new(),
        }
    }
}

fn default_mode() -> String {
    "agent".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextSettings {
    #[serde(default)]
    pub voice_enabled: bool,
    #[serde(default = "default_stt")]
    pub voice_stt_type: String,
    #[serde(default)]
    pub voice_vosk_model_path: Option<String>,
    #[serde(default)]
    pub voice_whisper_model_path: Option<String>,
    #[serde(default)]
    pub voice_last_input: Option<String>,
    #[serde(default = "default_listen_timeout")]
    pub voice_listen_timeout_secs: i32,
    #[serde(default)]
    pub voice_owner_id: Option<String>,
    #[serde(default)]
    pub voice_tts_enabled: bool,
    #[serde(default = "default_tts")]
    pub voice_tts_type: String,
    #[serde(default)]
    pub voice_elevenlabs_voice_id: Option<String>,
    #[serde(default = "default_elevenlabs_stt_model")]
    pub elevenlabs_stt_model: String,
    #[serde(default)]
    pub elevenlabs_stt_language: Option<String>,
    #[serde(default)]
    pub elevenlabs_stt_tag_audio_events: bool,
    #[serde(default = "default_elevenlabs_stt_no_verbatim")]
    pub elevenlabs_stt_no_verbatim: bool,
    #[serde(default = "default_elevenlabs_tts_model")]
    pub elevenlabs_tts_model: String,
    #[serde(default = "default_elevenlabs_stability")]
    pub elevenlabs_stability: f32,
    #[serde(default = "default_elevenlabs_similarity_boost")]
    pub elevenlabs_similarity_boost: f32,
    #[serde(default)]
    pub elevenlabs_style: Option<f32>,
    #[serde(default)]
    pub elevenlabs_speed: Option<f32>,
    #[serde(default)]
    pub elevenlabs_tts_language: Option<String>,
    #[serde(default)]
    pub use_tts: bool,
    #[serde(default = "default_true")]
    pub use_stt: bool,
    #[serde(default)]
    pub voice_muted: bool,
    #[serde(default = "default_deafened")]
    pub voice_deafened: bool,
    #[serde(default)]
    pub voice_discord_guild_id: Option<String>,
    #[serde(default)]
    pub rvc_on: bool,
    #[serde(default)]
    pub rvc_server: Option<String>,
    #[serde(default)]
    pub rvc_model_path: Option<String>,
    #[serde(default)]
    pub rvc_index_path: Option<String>,
    #[serde(default)]
    pub voice_audio_output_path: Option<String>,
    #[serde(default)]
    pub qwen_tts_server: Option<String>,
    #[serde(default)]
    pub qwen_tts_model: Option<String>,
    #[serde(default)]
    pub qwen_tts_speaker: Option<String>,
    #[serde(default)]
    pub qwen_tts_language: Option<String>,
    #[serde(default)]
    pub qwen_voice_clone_audio_path: Option<String>,
    #[serde(default)]
    pub qwen_voice_clone_enabled: bool,
    #[serde(default)]
    pub qwen_voice_clone_prompt: Option<String>,
    #[serde(default)]
    pub minimax_voice_id: Option<String>,
    #[serde(default)]
    pub minimax_tts_model: Option<String>,
    #[serde(default)]
    pub mimo_voice_id: Option<String>,
    #[serde(default)]
    pub mimo_tts_type: Option<String>,
    #[serde(default)]
    pub history_with_toolcalls: bool,
    #[serde(default)]
    pub only_tool_calls_no_history: bool,
    #[serde(default)]
    pub download: bool,
    #[serde(default)]
    pub feedback_enabled: bool,
    #[serde(default = "default_feedback_max")]
    pub feedback_max_per_5min: i32,
    #[serde(default = "default_feedback_window")]
    pub feedback_window_secs: i32,
    #[serde(default = "default_allowed")]
    pub allowed_guilds: Vec<String>,
    #[serde(default = "default_allowed")]
    pub allowed_channels: Vec<String>,
    #[serde(default)]
    pub max_llm_turns: Option<i32>,
    #[serde(default)]
    pub max_tool_calls: Option<i32>,
    #[serde(default)]
    pub summarize_char_limit: Option<i32>,
    #[serde(default)]
    pub active_templates: Vec<String>,
    #[serde(default)]
    pub done: bool,
    #[serde(default)]
    pub path: String,
    #[serde(default)]
    pub cl_file: Option<String>,
    #[serde(default)]
    pub active_state: Option<String>,
    #[serde(default)]
    pub current_template: String,
    #[serde(default)]
    pub tags_enabled: bool,
    #[serde(default)]
    pub llm_turn: i32,
    #[serde(default)]
    pub compaction_enabled: bool,
    #[serde(default)]
    pub compaction_summary: String,
    #[serde(default)]
    pub history_token_limit: Option<usize>,
    #[serde(default)]
    pub compaction_token_limit: Option<usize>,
    #[serde(default)]
    pub compaction_template: Option<String>,
    #[serde(default)]
    pub system_template: Option<String>,
    #[serde(default)]
    pub provider: Option<String>,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub vision_provider: Option<String>,
    #[serde(default)]
    pub vision_model: Option<String>,
    #[serde(default)]
    pub agent_name: String,
    #[serde(default)]
    pub voice_wake_words: Vec<String>,
    #[serde(default)]
    pub voice_auto_pause_enabled: bool,
    #[serde(default)]
    pub feedback_mode: Vec<String>,
    #[serde(default)]
    pub feedback_channel_id: Option<String>,
    #[serde(default = "default_feedback_template")]
    pub feedback_template: String,
    #[serde(default)]
    pub message_on_toolcalling: bool,
    #[serde(default = "default_true")]
    pub vm_screenshot_enabled: bool,
    #[serde(default = "default_screenshot_limit")]
    pub vm_screenshot_limit: usize,
    #[serde(default = "default_keyboard_layout")]
    pub vm_keyboard_layout: String,
    #[serde(default = "default_tool_history_limit")]
    pub tool_history_limit: usize,
}

fn default_stt() -> String {
    "vosk".to_string()
}
fn default_tts() -> String {
    "windows_sapi".to_string()
}
fn default_listen_timeout() -> i32 {
    120
}
fn default_deafened() -> bool {
    true
}
fn default_feedback_max() -> i32 {
    10
}
fn default_feedback_window() -> i32 {
    300
}
fn default_allowed() -> Vec<String> {
    vec!["*".to_string()]
}
fn default_elevenlabs_stt_model() -> String {
    "scribe_v2".to_string()
}
fn default_elevenlabs_stt_no_verbatim() -> bool {
    true
}
fn default_elevenlabs_tts_model() -> String {
    "eleven_multilingual_v2".to_string()
}
fn default_elevenlabs_stability() -> f32 {
    0.5
}
fn default_elevenlabs_similarity_boost() -> f32 {
    0.75
}
fn default_elevenlabs_speed() -> Option<f32> {
    Some(0.8)
}
fn default_feedback_template() -> String {
    "tasks/feedback".to_string()
}
fn default_true() -> bool {
    true
}
fn default_screenshot_limit() -> usize {
    5000
}
fn default_keyboard_layout() -> String {
    "us".to_string()
}
fn default_tool_history_limit() -> usize {
    50
}

impl Default for ContextSettings {
    fn default() -> Self {
        Self {
            voice_enabled: false,
            voice_stt_type: default_stt(),
            voice_vosk_model_path: None,
            voice_whisper_model_path: None,
            voice_last_input: None,
            voice_listen_timeout_secs: default_listen_timeout(),
            voice_owner_id: None,
            voice_tts_enabled: false,
            voice_tts_type: default_tts(),
            voice_elevenlabs_voice_id: None,
            elevenlabs_stt_model: default_elevenlabs_stt_model(),
            elevenlabs_stt_language: None,
            elevenlabs_stt_tag_audio_events: false,
            elevenlabs_stt_no_verbatim: default_elevenlabs_stt_no_verbatim(),
            elevenlabs_tts_model: default_elevenlabs_tts_model(),
            elevenlabs_stability: default_elevenlabs_stability(),
            elevenlabs_similarity_boost: default_elevenlabs_similarity_boost(),
            elevenlabs_style: None,
            elevenlabs_speed: default_elevenlabs_speed(),
            elevenlabs_tts_language: None,
            use_tts: false,
            use_stt: true,
            voice_muted: false,
            voice_deafened: default_deafened(),
            voice_discord_guild_id: None,
            rvc_on: false,
            rvc_server: None,
            rvc_model_path: None,
            rvc_index_path: None,
            voice_audio_output_path: None,
            qwen_tts_server: None,
            qwen_tts_model: None,
            qwen_tts_speaker: None,
            qwen_tts_language: None,
            qwen_voice_clone_audio_path: None,
            qwen_voice_clone_enabled: false,
            qwen_voice_clone_prompt: None,
            minimax_voice_id: None,
            minimax_tts_model: None,
            mimo_voice_id: None,
            mimo_tts_type: None,
            history_with_toolcalls: true,
            only_tool_calls_no_history: false,
            download: false,
            feedback_enabled: false,
            feedback_max_per_5min: default_feedback_max(),
            feedback_window_secs: default_feedback_window(),
            allowed_guilds: default_allowed(),
            allowed_channels: default_allowed(),
            max_llm_turns: None,
            max_tool_calls: None,
            summarize_char_limit: None,
            active_templates: Vec::new(),
            done: false,
            path: String::new(),
            cl_file: None,
            active_state: None,
            current_template: String::new(),
            tags_enabled: false,
            llm_turn: 0,
            compaction_enabled: false,
            compaction_summary: String::new(),
            history_token_limit: None,
            compaction_token_limit: None,
            compaction_template: None,
            system_template: None,
            provider: None,
            model: None,
            vision_provider: None,
            vision_model: None,
            agent_name: "assistant".to_string(),
            voice_wake_words: vec!["*".to_string()],
            voice_auto_pause_enabled: false,
            feedback_mode: Vec::new(),
            feedback_channel_id: None,
            feedback_template: default_feedback_template(),
            message_on_toolcalling: false,
            vm_screenshot_enabled: true,
            vm_screenshot_limit: default_screenshot_limit(),
            vm_keyboard_layout: default_keyboard_layout(),
            tool_history_limit: default_tool_history_limit(),
        }
    }
}

impl Database {
    pub fn load_context(&self, user_id: &str) -> anyhow::Result<Context> {
        let conn = self.conn();
        let result = conn.query_row(
            "SELECT data FROM contexts WHERE user_id = ?1",
            rusqlite::params![user_id],
            |row| row.get::<_, String>(0),
        );

        let mut base_ctx = match result {
            Ok(data) => serde_json::from_str::<Context>(&data)?,
            Err(rusqlite::Error::QueryReturnedNoRows) => Context {
                user_id: user_id.to_string(),
                ..Default::default()
            },
            Err(e) => return Err(e.into()),
        };

        // If a non-default session is active, load the session-specific context
        if !base_ctx.session_id.is_empty() && base_ctx.session_id != "default" {
            let key = format!("{}:::{}", user_id, base_ctx.session_id);
            if let Ok(data) = conn.query_row(
                "SELECT data FROM contexts WHERE user_id = ?1",
                rusqlite::params![key],
                |row| row.get::<_, String>(0),
            ) {
                if let Ok(mut session_ctx) = serde_json::from_str::<Context>(&data) {
                    session_ctx.user_id = user_id.to_string();
                    return Ok(session_ctx);
                }
            }
            // Session context doesn't exist yet; FORK from base
            let forked = base_ctx.clone();
            let forked_json = serde_json::to_string(&forked)?;
            conn.execute(
                "INSERT OR REPLACE INTO contexts (user_id, data, updated_at) VALUES (?1, ?2, datetime('now'))",
                rusqlite::params![key, forked_json],
            )?;
            base_ctx.user_id = user_id.to_string();
            return Ok(base_ctx);
        }

        Ok(base_ctx)
    }

    pub fn save_context(&self, ctx: &Context) -> anyhow::Result<()> {
        let conn = self.conn();
        let data = serde_json::to_string(ctx)?;
        if !ctx.session_id.is_empty() && ctx.session_id != "default" {
            let key = format!("{}:::{}", ctx.user_id, ctx.session_id);
            conn.execute(
                "INSERT OR REPLACE INTO contexts (user_id, data, updated_at) VALUES (?1, ?2, datetime('now'))",
                rusqlite::params![key, data],
            )?;
            // Update ONLY session_id on base row so resolve_user_key sees it
            if let Ok(base_data) = conn.query_row(
                "SELECT data FROM contexts WHERE user_id = ?1",
                rusqlite::params![ctx.user_id],
                |row| row.get::<_, String>(0),
            ) {
                if let Ok(mut base_json) = serde_json::from_str::<serde_json::Value>(&base_data) {
                    base_json["session_id"] = serde_json::json!(ctx.session_id);
                    let updated = serde_json::to_string(&base_json)?;
                    conn.execute(
                        "INSERT OR REPLACE INTO contexts (user_id, data, updated_at) VALUES (?1, ?2, datetime('now'))",
                        rusqlite::params![ctx.user_id, updated],
                    )?;
                }
            }
        } else {
            conn.execute(
                "INSERT OR REPLACE INTO contexts (user_id, data, updated_at) VALUES (?1, ?2, datetime('now'))",
                rusqlite::params![ctx.user_id, data],
            )?;
        }
        Ok(())
    }

    pub fn merge_context(
        &self,
        user_id: &str,
        updates: serde_json::Value,
    ) -> anyhow::Result<Context> {
        let mut ctx = self.load_context(user_id)?;
        let mut data: serde_json::Value = serde_json::to_value(&ctx)?;

        if let (Some(obj), Some(updates_obj)) = (data.as_object_mut(), updates.as_object()) {
            for (key, value) in updates_obj {
                if key.contains('.') {
                    set_nested_value(obj, key, value.clone());
                } else {
                    obj.insert(key.clone(), value.clone());
                }
            }
        }

        ctx = serde_json::from_value(data)?;
        self.save_context(&ctx)?;
        Ok(ctx)
    }

    pub fn delete_context(&self, user_id: &str) -> anyhow::Result<()> {
        let conn = self.conn();
        conn.execute(
            "DELETE FROM messages WHERE user_id = ?1 OR user_id LIKE ?2",
            rusqlite::params![user_id, format!("{}:::%", user_id)],
        )?;
        conn.execute(
            "DELETE FROM memory WHERE user_id = ?1 OR user_id LIKE ?2",
            rusqlite::params![user_id, format!("{}:::%", user_id)],
        )?;
        conn.execute(
            "DELETE FROM contexts WHERE user_id = ?1 OR user_id LIKE ?2",
            rusqlite::params![user_id, format!("{}:::%", user_id)],
        )?;
        Ok(())
    }

    pub fn increment_turn(&self, ctx: &mut Context) {
        ctx.turn += 1;
    }

    // ── Session Management ──────────────────────────────────────────────────

    fn context_key(user_id: &str, session_id: &str) -> String {
        if session_id.is_empty() || session_id == "default" {
            user_id.to_string()
        } else {
            format!("{}:::{}", user_id, session_id)
        }
    }

    pub fn load_session_context(
        &self,
        user_id: &str,
        session_id: &str,
    ) -> anyhow::Result<Context> {
        let key = Self::context_key(user_id, session_id);
        let mut ctx = match self.load_context(&key) {
            Ok(c) => c,
            Err(_) => {
                // Fork from base context
                let mut base = self.load_context(user_id)?;
                base.session_id = session_id.to_string();
                // Save forked context under session key
                let data = serde_json::to_string(&base)?;
                let conn = self.conn();
                conn.execute(
                    "INSERT OR REPLACE INTO contexts (user_id, data, updated_at) VALUES (?1, ?2, datetime('now'))",
                    rusqlite::params![key, data],
                )?;
                base
            }
        };
        ctx.user_id = user_id.to_string();
        ctx.session_id = session_id.to_string();
        Ok(ctx)
    }

    pub fn save_session_context(&self, ctx: &Context) -> anyhow::Result<()> {
        let key = Self::context_key(&ctx.user_id, &ctx.session_id);
        let mut persisted = ctx.clone();
        persisted.user_id = key.clone();
        self.save_context(&persisted)
    }

    pub fn create_session(
        &self,
        user_id: &str,
        session_id: &str,
        name: Option<&str>,
    ) -> anyhow::Result<()> {
        let conn = self.conn();
        conn.execute(
            "INSERT INTO user_sessions (user_id, session_id, name) VALUES (?1, ?2, ?3)
             ON CONFLICT(user_id, session_id) DO UPDATE SET updated_at = datetime('now')",
            rusqlite::params![user_id, session_id, name],
        )?;
        Ok(())
    }

    pub fn delete_session(&self, user_id: &str, session_id: &str) -> anyhow::Result<()> {
        let conn = self.conn();
        conn.execute(
            "DELETE FROM user_sessions WHERE user_id = ?1 AND session_id = ?2",
            rusqlite::params![user_id, session_id],
        )?;
        let key = Self::context_key(user_id, session_id);
        let _ = conn.execute(
            "DELETE FROM messages WHERE user_id = ?1",
            rusqlite::params![key],
        );
        let _ = conn.execute(
            "DELETE FROM contexts WHERE user_id = ?1",
            rusqlite::params![key],
        );
        Ok(())
    }

    pub fn list_sessions(&self, user_id: &str) -> anyhow::Result<Vec<(String, String)>> {
        let conn = self.conn();
        let mut stmt = conn.prepare(
            "SELECT session_id, name FROM user_sessions WHERE user_id = ?1 ORDER BY updated_at DESC"
        )?;
        let rows = stmt
            .query_map(rusqlite::params![user_id], |row| {
                let sid: String = row.get(0)?;
                let name: Option<String> = row.get(1)?;
                Ok((sid.clone(), name.unwrap_or(sid)))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    pub fn rename_session(
        &self,
        user_id: &str,
        session_id: &str,
        name: &str,
    ) -> anyhow::Result<()> {
        let conn = self.conn();
        conn.execute(
            "UPDATE user_sessions SET name = ?1, updated_at = datetime('now') WHERE user_id = ?2 AND session_id = ?3",
            rusqlite::params![name, user_id, session_id],
        )?;
        Ok(())
    }

    pub fn clear_session_messages(&self, user_id: &str, session_id: &str) -> anyhow::Result<()> {
        let key = Self::context_key(user_id, session_id);
        let conn = self.conn();
        conn.execute(
            "DELETE FROM messages WHERE user_id = ?1",
            rusqlite::params![key],
        )?;
        Ok(())
    }
}

/// Set a nested value in a JSON object using dot-notation (e.g., "custom_data.device")
/// Creates intermediate objects as needed.
fn set_nested_value(obj: &mut serde_json::Map<String, serde_json::Value>, path: &str, value: serde_json::Value) {
    let parts: Vec<&str> = path.split('.').collect();
    if parts.is_empty() {
        return;
    }

    let mut current = obj;
    for (i, part) in parts.iter().enumerate() {
        if i == parts.len() - 1 {
            // Last part: set the value
            current.insert(part.to_string(), value.clone());
        } else {
            // Intermediate part: navigate or create object
            if !current.contains_key(*part) || !current[*part].is_object() {
                current.insert(part.to_string(), serde_json::json!({}));
            }
            current = current.get_mut(*part).unwrap().as_object_mut().unwrap();
        }
    }
}

#[cfg(test)]
mod db_tests {
    use super::*;
    use tempfile::TempDir;

    fn test_db() -> (Database, TempDir) {
        let dir = TempDir::new().unwrap();
        let db = Database::new(dir.path()).unwrap();
        (db, dir)
    }

    #[test]
    fn test_load_default_context() {
        let (db, _dir) = test_db();
        let ctx = db.load_context("user1").unwrap();
        assert_eq!(ctx.user_id, "user1");
        assert_eq!(ctx.mode, "agent");
        assert_eq!(ctx.turn, 0);
    }

    #[test]
    fn test_save_and_load_context() {
        let (db, _dir) = test_db();
        let ctx = Context {
            user_id: "user1".to_string(),
            turn: 5,
            mode: "coding".to_string(),
            ..Default::default()
        };
        db.save_context(&ctx).unwrap();
        let loaded = db.load_context("user1").unwrap();
        assert_eq!(loaded.turn, 5);
        assert_eq!(loaded.mode, "coding");
    }

    #[test]
    fn test_merge_context() {
        let (db, _dir) = test_db();
        let ctx = Context {
            user_id: "user1".to_string(),
            ..Default::default()
        };
        db.save_context(&ctx).unwrap();

        let updates = serde_json::json!({"mode": "coding", "turn": 3});
        let merged = db.merge_context("user1", updates).unwrap();
        assert_eq!(merged.mode, "coding");
        assert_eq!(merged.turn, 3);
    }

    #[test]
    fn test_merge_context_dot_notation() {
        let (db, _dir) = test_db();
        let ctx = Context {
            user_id: "user1".to_string(),
            ..Default::default()
        };
        db.save_context(&ctx).unwrap();

        // Set custom_data.device via dot-notation
        let updates = serde_json::json!({"custom_data.device": "main"});
        let merged = db.merge_context("user1", updates).unwrap();
        assert_eq!(merged.custom_data["device"], "main");

        // Set another key without overwriting the first
        let updates = serde_json::json!({"custom_data.style": "analytical"});
        let merged = db.merge_context("user1", updates).unwrap();
        assert_eq!(merged.custom_data["device"], "main");
        assert_eq!(merged.custom_data["style"], "analytical");
    }

    #[test]
    fn test_merge_context_nested_dot_notation() {
        let (db, _dir) = test_db();
        let ctx = Context {
            user_id: "user1".to_string(),
            ..Default::default()
        };
        db.save_context(&ctx).unwrap();

        // Set deeply nested value
        let updates = serde_json::json!({"custom_data.app.settings.theme": "dark"});
        let merged = db.merge_context("user1", updates).unwrap();
        assert_eq!(merged.custom_data["app"]["settings"]["theme"], "dark");

        // Set sibling key without overwriting
        let updates = serde_json::json!({"custom_data.app.settings.lang": "en"});
        let merged = db.merge_context("user1", updates).unwrap();
        assert_eq!(merged.custom_data["app"]["settings"]["theme"], "dark");
        assert_eq!(merged.custom_data["app"]["settings"]["lang"], "en");
    }

    #[test]
    fn test_increment_turn() {
        let (db, _dir) = test_db();
        let mut ctx = db.load_context("user1").unwrap();
        assert_eq!(ctx.turn, 0);
        db.increment_turn(&mut ctx);
        assert_eq!(ctx.turn, 1);
    }
}

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
            agent_name: "assistant".to_string(),
            voice_wake_words: vec!["*".to_string()],
            voice_auto_pause_enabled: false,
            feedback_mode: Vec::new(),
            feedback_channel_id: None,
            feedback_template: default_feedback_template(),
            message_on_toolcalling: false,
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

        match result {
            Ok(data) => Ok(serde_json::from_str(&data)?),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(Context {
                user_id: user_id.to_string(),
                ..Default::default()
            }),
            Err(e) => Err(e.into()),
        }
    }

    pub fn save_context(&self, ctx: &Context) -> anyhow::Result<()> {
        let conn = self.conn();
        let data = serde_json::to_string(ctx)?;
        conn.execute(
            "INSERT OR REPLACE INTO contexts (user_id, data, updated_at) VALUES (?1, ?2, datetime('now'))",
            rusqlite::params![ctx.user_id, data],
        )?;
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
                obj.insert(key.clone(), value.clone());
            }
        }

        ctx = serde_json::from_value(data)?;
        self.save_context(&ctx)?;
        Ok(ctx)
    }

    pub fn increment_turn(&self, ctx: &mut Context) {
        ctx.turn += 1;
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
    fn test_increment_turn() {
        let (db, _dir) = test_db();
        let mut ctx = db.load_context("user1").unwrap();
        assert_eq!(ctx.turn, 0);
        db.increment_turn(&mut ctx);
        assert_eq!(ctx.turn, 1);
    }
}

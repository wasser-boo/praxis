pub mod commands;
pub mod handler;
pub mod ws_client;

pub use ws_client::{IncomingMessage, OutgoingMessage, WsClient};

use crate::db::Database;
use crate::discord::handler::DiscordHandler;
use anyhow::Context;
use serenity::model::gateway::GatewayIntents;
use serenity::Client;
use std::sync::Arc;
use tokio::sync::Mutex;
use once_cell::sync::OnceCell;

const DEFAULT_GATEWAY_URL: &str = "ws://localhost:3537/ws";

// ── Voice State ──────────────────────────────────────────────────────────────

static DISCORD_VOICE_STATE: OnceCell<Arc<Mutex<DiscordVoiceState>>> = OnceCell::new();

#[derive(Default)]
pub struct DiscordVoiceState {
    pub guild_id: Option<u64>,
    pub user_id: Option<u64>,
}

pub fn init_discord_voice_state() -> Arc<Mutex<DiscordVoiceState>> {
    let state = Arc::new(Mutex::new(DiscordVoiceState::default()));
    DISCORD_VOICE_STATE.set(state.clone()).ok();
    state
}

pub fn get_discord_voice_state() -> Option<Arc<Mutex<DiscordVoiceState>>> {
    DISCORD_VOICE_STATE.get().cloned()
}

pub async fn set_discord_voice_state(guild_id: Option<u64>, user_id: Option<u64>) {
    if let Some(state) = DISCORD_VOICE_STATE.get() {
        let mut guard = state.lock().await;
        guard.guild_id = guild_id;
        guard.user_id = user_id;
    }
}

// ── Songbird Manager ─────────────────────────────────────────────────────────

#[cfg(feature = "voice_songbird")]
static SONGBIRD_MANAGER: OnceCell<Arc<songbird::Songbird>> = OnceCell::new();

#[cfg(feature = "voice_songbird")]
pub fn set_songbird_manager(manager: Arc<songbird::Songbird>) {
    SONGBIRD_MANAGER.set(manager).ok();
}

#[cfg(feature = "voice_songbird")]
pub fn get_songbird_manager() -> Option<&'static Arc<songbird::Songbird>> {
    SONGBIRD_MANAGER.get()
}

// ── Discord Bot ──────────────────────────────────────────────────────────────

pub struct DiscordBot {
    pub db: Database,
    pub ws_client: Arc<Mutex<WsClient>>,
    pub secrets: crate::db::secrets::Secrets,
}

impl DiscordBot {
    pub async fn new(
        db: Database,
        gateway_url: Option<&str>,
        secrets: crate::db::secrets::Secrets,
    ) -> anyhow::Result<Self> {
        let url = gateway_url.unwrap_or(DEFAULT_GATEWAY_URL);
        let ws_client = WsClient::connect(url).await?;
        Ok(Self {
            db,
            ws_client: Arc::new(Mutex::new(ws_client)),
            secrets,
        })
    }

    pub async fn start(self) -> anyhow::Result<()> {
        // Read from env (.env file)
        let token = std::env::var("DISCORD_BOT_TOKEN")
            .context("DISCORD_BOT_TOKEN not set. Run 'praxis onboard --interactive' to configure.")?;
        let application_id = std::env::var("DISCORD_APPLICATION_ID")
            .context("DISCORD_APPLICATION_ID not set")?
            .parse::<u64>()
            .context("DISCORD_APPLICATION_ID must be a number")?;

        tracing::info!("Discord bot starting with token: {}...", &token[..std::cmp::min(10, token.len())]);
        self.start_with_token(&token, application_id).await
    }

    pub async fn start_with_token(self, token: &str, application_id: u64) -> anyhow::Result<()> {
        let voice_state = init_discord_voice_state();
        let handler = DiscordHandler::new(self.db.clone(), self.ws_client.clone(), self.secrets.clone());

        let mut client_builder = Client::builder(
            token,
            GatewayIntents::MESSAGE_CONTENT
                | GatewayIntents::DIRECT_MESSAGES
                | GatewayIntents::GUILD_MESSAGES
                | GatewayIntents::GUILD_VOICE_STATES
                | GatewayIntents::GUILD_MEMBERS,
        )
        .application_id(serenity::all::ApplicationId::new(application_id))
        .event_handler(handler);

        // Register songbird for voice support
        #[cfg(feature = "voice_songbird")]
        {
            use songbird::SerenityInit;
            use songbird::driver::DecodeMode;
            let voice_config = songbird::Config::default()
                .decode_mode(DecodeMode::Decode(Default::default()));
            client_builder = client_builder.register_songbird_from_config(voice_config);
        }

        let mut client = client_builder.await?;

        // Store songbird manager reference
        #[cfg(feature = "voice_songbird")]
        {
            if let Some(manager) = songbird::serenity::get(&client).await {
                set_songbird_manager(manager.clone());
            }
        }

        // Spawn event listener for file uploads, feedback, voice TTS
        let db = self.db.clone();
        let http = client.http.clone();
        tokio::spawn(async move {
            listen_for_events(db, http).await;
        });

        if let Err(e) = commands::setup_commands(&client.http).await {
            tracing::error!("Failed to setup commands: {}", e);
        }

        tracing::info!("Discord bot starting...");
        client.start().await?;
        Ok(())
    }
}

/// Listen for events from the event channel and handle them
async fn listen_for_events(db: Database, http: Arc<serenity::http::Http>) {
    // Wait a bit for event channel to be initialized
    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    let event_tx = match crate::event_channel::get_event_tx() {
        Some(tx) => tx,
        None => {
            tracing::debug!("No event channel, event listener not started");
            return;
        }
    };

    let mut rx = event_tx.subscribe();
    loop {
        match rx.recv().await {
            Ok(crate::event_channel::GatewayEvent::FileUpload { user_id, file_name, file_path, channel_id }) => {
                tracing::info!("File upload event for user {}: {} to channel {}", user_id, file_name, channel_id);
                let target_channel_id = if !channel_id.is_empty() {
                    channel_id.parse::<u64>().unwrap_or(0)
                } else {
                    0
                };

                let send_to = if target_channel_id > 0 {
                    serenity::model::id::ChannelId::new(target_channel_id)
                } else if let Ok(Some(pairing)) = db.get_pairing_by_internal_user(&user_id) {
                    let discord_user_id = pairing.discord_user_id.parse::<u64>().unwrap_or(0);
                    if discord_user_id > 0 {
                        let user_id_obj = serenity::model::id::UserId::new(discord_user_id);
                        match user_id_obj.create_dm_channel(&http).await {
                            Ok(dm) => dm.id,
                            Err(e) => {
                                tracing::error!("Failed to create DM channel: {}", e);
                                continue;
                            }
                        }
                    } else {
                        continue;
                    }
                } else {
                    continue;
                };

                let req = reqwest::Client::new();
                let file_bytes = tokio::fs::read(&file_path).await.unwrap_or_default();
                let part = reqwest::multipart::Part::bytes(file_bytes)
                    .file_name(file_name.clone());
                let form = reqwest::multipart::Form::new()
                    .part("file", part);

                let bot_token = std::env::var("DISCORD_BOT_TOKEN").unwrap_or_default();
                let url = format!("https://discord.com/api/v10/channels/{}/messages", send_to);

                if let Ok(resp) = req.post(&url)
                    .header("Authorization", format!("Bot {}", bot_token))
                    .multipart(form)
                    .send()
                    .await
                {
                    if resp.status().is_success() {
                        tracing::info!("File {} sent to channel {}", file_name, send_to);
                    } else {
                        tracing::error!("Failed to send file: {} - {}", resp.status(), resp.text().await.unwrap_or_default());
                    }
                }
            }
            Ok(crate::event_channel::GatewayEvent::AgentFeedback { user_id, message }) => {
                tracing::info!("Agent feedback for user {}", user_id);
                if let Ok(Some(pairing)) = db.get_pairing_by_internal_user(&user_id) {
                    let discord_user_id = pairing.discord_user_id.parse::<u64>().unwrap_or(0);
                    if discord_user_id > 0 {
                        let user_id_obj = serenity::model::id::UserId::new(discord_user_id);
                        if let Ok(dm_channel) = user_id_obj.create_dm_channel(&http).await {
                            let _ = dm_channel.send_message(&http, serenity::builder::CreateMessage::default().content(format!("[Feedback] {}", message))).await;
                        }
                    }
                }
            }
            Ok(crate::event_channel::GatewayEvent::ChannelMessage { user_id: _, channel_id, message }) => {
                let channel_idparsed = channel_id.parse::<u64>().unwrap_or(0);
                if channel_idparsed > 0 {
                    let channel = serenity::model::id::ChannelId::new(channel_idparsed);
                    let _ = channel.say(&http, &message).await;
                }
            }
            Ok(crate::event_channel::GatewayEvent::VoiceTts { user_id, audio_data }) => {
                handle_voice_tts(user_id, audio_data).await;
            }
            Ok(_) => {}
            Err(e) => {
                tracing::error!("Event channel error: {}", e);
                tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
            }
        }
    }
}

/// Handle voice TTS - play audio in Discord voice channel
#[cfg(feature = "voice_songbird")]
async fn handle_voice_tts(user_id: String, audio_data: Vec<u8>) {
    tracing::info!("Voice TTS for user {}: {} bytes", user_id, audio_data.len());

    let is_mp3 = audio_data.len() >= 3 &&
        audio_data[0] == 0x49 && audio_data[1] == 0x44 && audio_data[2] == 0x33;
    let original_sample_rate = if is_mp3 {
        44100
    } else {
        crate::voice::tts::get_wav_sample_rate(&audio_data).unwrap_or(48000)
    };

    if let Some(voice_state) = get_discord_voice_state() {
        let guard = voice_state.lock().await;
        if let Some(guild_id) = guard.guild_id {
            drop(guard);
            if let Some(manager) = get_songbird_manager() {
                if let Some(handler) = manager.get(songbird::id::GuildId::from(std::num::NonZeroU64::new(guild_id).unwrap())) {
                    match crate::voice::tts::audio_bytes_to_pcm(&audio_data) {
                        Ok(samples) if !samples.is_empty() => {
                            let wav_data = crate::voice::pcm_to_wav(&samples, original_sample_rate, 1);
                            let mut call = handler.lock().await;
                            let source = songbird::input::Input::from(wav_data);
                            call.play_input(source);
                            tracing::info!("TTS audio played in guild {}", guild_id);
                        }
                        Ok(_) => tracing::warn!("No audio samples"),
                        Err(e) => tracing::error!("Audio decode failed: {}", e),
                    }
                }
            }
        }
    }
}

#[cfg(not(feature = "voice_songbird"))]
async fn handle_voice_tts(_user_id: String, _audio_data: Vec<u8>) {
    tracing::debug!("Voice TTS skipped - voice_songbird not enabled");
}

pub async fn start(db: Database) -> anyhow::Result<()> {
    let secrets = crate::db::secrets::get_secrets();
    start_with_secrets(db, secrets).await
}

pub async fn start_with_secrets(
    db: Database,
    secrets: crate::db::secrets::Secrets,
) -> anyhow::Result<()> {
    let gateway_url = std::env::var("GATEWAY_WS_URL")
        .map(|s| s.into())
        .unwrap_or_else(|_| DEFAULT_GATEWAY_URL.to_string());

    let token = secrets
        .discord_bot_token
        .clone()
        .or_else(|| std::env::var("DISCORD_BOT_TOKEN").ok())
        .context("DISCORD_BOT_TOKEN not set. Run 'praxis onboard --interactive' to configure.")?;

    let application_id = std::env::var("DISCORD_APPLICATION_ID")
        .context("DISCORD_APPLICATION_ID not set")?
        .parse::<u64>()
        .context("DISCORD_APPLICATION_ID must be a number")?;

    let bot = DiscordBot::new(db, Some(&gateway_url), secrets).await?;
    tracing::info!("Discord bot connecting...");
    bot.start_with_token(&token, application_id).await
}

#[cfg(test)]
mod discord_tests {
    use super::*;

    #[tokio::test]
    async fn test_default_gateway_url() {
        assert_eq!(DEFAULT_GATEWAY_URL, "ws://localhost:3537/ws");
    }

    #[tokio::test]
    async fn test_outgoing_message_serialization() {
        let msg = OutgoingMessage::Message {
            user_id: "user123".to_string(),
            content: "Hello".to_string(),
            channel_id: "ch1".to_string(),
        };
        let json = serde_json::to_string(&msg).unwrap();
        assert!(json.contains("\"type\":\"message\""));
        assert!(json.contains("user123"));
    }

    #[tokio::test]
    async fn test_incoming_message_deserialization() {
        let json = r#"{"type":"response","user_id":"user123","content":"Hi there"}"#;
        let msg: IncomingMessage = serde_json::from_str(json).unwrap();
        match msg {
            IncomingMessage::Response { user_id, content } => {
                assert_eq!(user_id, "user123");
                assert_eq!(content, "Hi there");
            }
            _ => panic!("Expected Response variant"),
        }
    }

    #[test]
    fn test_discord_voice_state_default() {
        let state = DiscordVoiceState::default();
        assert!(state.guild_id.is_none());
        assert!(state.user_id.is_none());
    }
}

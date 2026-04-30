use crate::db::Database;
use crate::discord::ws_client::{IncomingMessage, OutgoingMessage, WsClient};
use serenity::async_trait;
use serenity::model::application::Interaction;
use serenity::model::channel::Message;
use serenity::prelude::*;
use std::sync::Arc;
use tokio::sync::Mutex;
use tokio::time::{interval, Duration};

pub struct DiscordHandler {
    pub db: Database,
    pub ws_client: Arc<Mutex<WsClient>>,
    pub voice_muted: Arc<Mutex<bool>>,
    pub voice_deafened: Arc<Mutex<bool>>,
    pub secrets: crate::db::secrets::Secrets,
    pub voice_handler: Option<Arc<crate::voice::handler::VoiceHandler>>,
}

impl DiscordHandler {
    pub fn new(
        db: Database,
        ws_client: Arc<Mutex<WsClient>>,
        secrets: crate::db::secrets::Secrets,
    ) -> Self {
        Self {
            db,
            ws_client,
            voice_muted: Arc::new(Mutex::new(false)),
            voice_deafened: Arc::new(Mutex::new(true)),
            secrets,
            voice_handler: None,
        }
    }

    async fn check_channel_allowed(&self, msg: &Message) -> bool {
        let discord_user_id = msg.author.id.to_string();
        let pairing = match self.db.get_pairing_by_discord(&discord_user_id) {
            Ok(Some(p)) => p,
            _ => return false,
        };

        match self.db.load_context(&pairing.user_id) {
            Ok(ctx) => {
                let guild_id = msg.guild_id.map(|g| g.to_string());
                let channel_id = msg.channel_id.to_string();

                let guild_allowed = guild_id
                    .as_ref()
                    .map(|g| {
                        ctx.settings.allowed_guilds.contains(g)
                            || ctx.settings.allowed_guilds.contains(&"*".to_string())
                    })
                    .unwrap_or(true);
                let channel_allowed = ctx.settings.allowed_channels.contains(&channel_id)
                    || ctx.settings.allowed_channels.contains(&"*".to_string());

                guild_allowed && channel_allowed
            }
            Err(_) => false,
        }
    }
}

#[async_trait]
impl EventHandler for DiscordHandler {
    async fn ready(&self, ctx: Context, ready: serenity::model::gateway::Ready) {
        tracing::info!("Discord bot connected as {}", ready.user.name);
        use serenity::model::user::OnlineStatus;
        ctx.set_presence(None, OnlineStatus::Online);

        #[cfg(feature = "songbird")]
        {
            if let Some(manager) = songbird::serenity::get(&ctx).await {
                crate::discord::set_songbird_manager(manager);
                tracing::info!("Songbird manager stored");
            }
        }
    }

    async fn message(&self, ctx: Context, msg: Message) {
        if msg.author.bot {
            return;
        }

        if msg.content.starts_with('/') {
            return;
        }

        if let Some(guild_id) = msg.guild_id {
            let mentions_me = msg.mentions_me(&ctx).await.unwrap_or(false);
            if !mentions_me {
                tracing::debug!(
                    "Message in guild {} does not mention bot, ignoring",
                    guild_id
                );
                return;
            }
        }

        tracing::info!(
            "Received message from {} in channel {}: {}",
            msg.author.id,
            msg.channel_id,
            msg.content
        );

        let discord_user_id = msg.author.id.to_string();

        let pairing = match self.db.get_pairing_by_discord(&discord_user_id) {
            Ok(Some(p)) => p,
            _ => {
                tracing::warn!(
                    "User {} is not paired, ignoring message",
                    discord_user_id
                );
                return;
            }
        };

        if !self.check_channel_allowed(&msg).await {
            tracing::warn!("Message from guild/channel not in allowed list, ignoring");
            return;
        }

        let typing_channel_id = msg.channel_id;
        let typing_http = ctx.http.clone();

        let typing_handle = tokio::spawn(async move {
            let mut ticker = interval(Duration::from_secs(3));
            loop {
                ticker.tick().await;
                let _ = typing_channel_id.start_typing(&typing_http);
            }
        });

        let payload = OutgoingMessage::Message {
            user_id: pairing.user_id.clone(),
            content: msg.content.clone(),
            channel_id: msg.channel_id.to_string(),
        };

        let ws_client = self.ws_client.lock().await;
        if let Err(e) = ws_client.send(payload).await {
            tracing::error!("Failed to send to gateway: {}", e);
            typing_handle.abort();
            let _ = msg
                .reply(&ctx.http, "Gateway error. Try again later.")
                .await;
            return;
        }

        loop {
            match ws_client.recv().await {
                Ok(IncomingMessage::Response { content, .. }) => {
                    typing_handle.abort();
                    if let Err(e) = send_message_split(&ctx.http, msg.channel_id, &content).await {
                        tracing::error!("Failed to send response: {}", e);
                    }
                    break;
                }
                Ok(IncomingMessage::Feedback { content, .. }) => {
                    if let Err(e) = send_message_split(&ctx.http, msg.channel_id, &content).await {
                        tracing::error!("Failed to send feedback: {}", e);
                    }
                }
                Ok(IncomingMessage::Error { message }) => {
                    typing_handle.abort();
                    let _ = msg
                        .reply(&ctx.http, format!("Error: {}", message))
                        .await;
                    break;
                }
                Ok(IncomingMessage::Pong) => {
                    tracing::debug!("Pong received");
                }
                Ok(_) => {
                    tracing::debug!("Ignoring unexpected message type");
                }
                Err(e) => {
                    tracing::error!("Failed to receive response: {}", e);
                    typing_handle.abort();
                    let _ = msg
                        .reply(&ctx.http, "Failed to get response. Try again.")
                        .await;
                    break;
                }
            }
        }
    }

    async fn interaction_create(&self, ctx: Context, interaction: Interaction) {
        if let Interaction::Command(command) = interaction {
            match command.data.name.as_str() {
                "pair" => {
                    if let Err(e) =
                        crate::discord::commands::handle_pair_command(&self.db, &ctx, &command).await
                    {
                        tracing::error!("Pair command error: {}", e);
                    }
                }
                "mute" => {
                    let mut muted = self.voice_muted.lock().await;
                    *muted = true;
                    tracing::info!("Bot voice muted by user {}", command.user.id);
                    let _ = command
                        .create_response(
                            &ctx.http,
                            serenity::builder::CreateInteractionResponse::Message(
                                serenity::builder::CreateInteractionResponseMessage::new()
                                    .content("Bot microphone is now muted."),
                            ),
                        )
                        .await;
                }
                "unmute" => {
                    let mut muted = self.voice_muted.lock().await;
                    *muted = false;
                    tracing::info!("Bot voice unmuted by user {}", command.user.id);
                    let _ = command
                        .create_response(
                            &ctx.http,
                            serenity::builder::CreateInteractionResponse::Message(
                                serenity::builder::CreateInteractionResponseMessage::new()
                                    .content("Bot microphone is now unmuted."),
                            ),
                        )
                        .await;
                }
                "deafen" => {
                    let mut deafened = self.voice_deafened.lock().await;
                    *deafened = true;
                    tracing::info!("Bot voice deafened by user {}", command.user.id);
                    let _ = command
                        .create_response(
                            &ctx.http,
                            serenity::builder::CreateInteractionResponse::Message(
                                serenity::builder::CreateInteractionResponseMessage::new()
                                    .content("Bot is now deafened (cannot hear)."),
                            ),
                        )
                        .await;
                }
                "undeafen" => {
                    let mut deafened = self.voice_deafened.lock().await;
                    *deafened = false;
                    tracing::info!("Bot voice undeafened by user {}", command.user.id);
                    let _ = command
                        .create_response(
                            &ctx.http,
                            serenity::builder::CreateInteractionResponse::Message(
                                serenity::builder::CreateInteractionResponseMessage::new()
                                    .content("Bot is now undeafened (can hear)."),
                            ),
                        )
                        .await;
                }
                "tts" => {
                    let discord_user_id = command.user.id.to_string();
                    if let Ok(Some(pairing)) = self.db.get_pairing_by_discord(&discord_user_id) {
                        let user_id = &pairing.user_id;
                        if let Ok(mut db_ctx) = self.db.load_context(user_id) {
                            db_ctx.settings.use_tts = !db_ctx.settings.use_tts;
                            let new_state = db_ctx.settings.use_tts;
                            if let Err(e) = self.db.save_context(&db_ctx) {
                                tracing::error!("Failed to save TTS state: {}", e);
                            } else {
                                let msg = if new_state {
                                    "TTS is now **enabled**."
                                } else {
                                    "TTS is now **disabled**."
                                };
                                let _ = command
                                    .create_response(
                                        &ctx.http,
                                        serenity::builder::CreateInteractionResponse::Message(
                                            serenity::builder::CreateInteractionResponseMessage::new()
                                                .content(msg),
                                        ),
                                    )
                                    .await;
                            }
                        }
                    } else {
                        let _ = command
                            .create_response(
                                &ctx.http,
                                serenity::builder::CreateInteractionResponse::Message(
                                    serenity::builder::CreateInteractionResponseMessage::new()
                                        .content("Please pair first with /pair"),
                                ),
                            )
                            .await;
                    }
                }
                "mode" => {
                    let discord_user_id = command.user.id.to_string();
                    let pairing = match self.db.get_pairing_by_discord(&discord_user_id) {
                        Ok(Some(p)) => p,
                        _ => {
                            let _ = command
                                .create_response(
                                    &ctx.http,
                                    serenity::builder::CreateInteractionResponse::Message(
                                        serenity::builder::CreateInteractionResponseMessage::new()
                                            .content("You are not paired with this bot."),
                                    ),
                                )
                                .await;
                            return;
                        }
                    };

                    let mut ctx_data = match self.db.load_context(&pairing.user_id) {
                        Ok(ctx) => ctx,
                        Err(e) => {
                            tracing::error!("Failed to load context for mode toggle: {}", e);
                            let _ = command
                                .create_response(
                                    &ctx.http,
                                    serenity::builder::CreateInteractionResponse::Message(
                                        serenity::builder::CreateInteractionResponseMessage::new()
                                            .content("Failed to load your context."),
                                    ),
                                )
                                .await;
                            return;
                        }
                    };

                    let current_mode = ctx_data
                        .custom_data
                        .get("mode")
                        .and_then(|v| v.as_str())
                        .unwrap_or("agent")
                        .to_string();
                    let new_mode = if current_mode == "chat" {
                        "agent"
                    } else {
                        "chat"
                    };

                    if let Some(obj) = ctx_data.custom_data.as_object_mut() {
                        obj.insert(
                            "mode".to_string(),
                            serde_json::json!(new_mode),
                        );
                    } else {
                        ctx_data.custom_data = serde_json::json!({
                            "mode": new_mode
                        });
                    }

                    if let Err(e) = self.db.save_context(&ctx_data) {
                        tracing::error!("Failed to save context after mode toggle: {}", e);
                        let _ = command
                            .create_response(
                                &ctx.http,
                                serenity::builder::CreateInteractionResponse::Message(
                                    serenity::builder::CreateInteractionResponseMessage::new()
                                        .content("Failed to toggle mode."),
                                ),
                            )
                            .await;
                        return;
                    }

                    let response_text = format!(
                        "Switched from **{}** mode to **{}** mode.",
                        current_mode, new_mode
                    );
                    let _ = command
                        .create_response(
                            &ctx.http,
                            serenity::builder::CreateInteractionResponse::Message(
                                serenity::builder::CreateInteractionResponseMessage::new()
                                    .content(response_text),
                            ),
                        )
                        .await;
                }
                "join" => {
                    let guild_id = match command.guild_id {
                        Some(g) => g,
                        None => {
                            let _ = command
                                .create_response(
                                    &ctx.http,
                                    serenity::builder::CreateInteractionResponse::Message(
                                        serenity::builder::CreateInteractionResponseMessage::new()
                                            .content("This command can only be used in a server."),
                                    ),
                                )
                                .await;
                            return;
                        }
                    };

                    let voice_channel_id = ctx.cache.guild(guild_id)
                        .and_then(|g| g.voice_states.get(&command.user.id).and_then(|vs| vs.channel_id));

                    let voice_channel_id = match voice_channel_id {
                        Some(id) => id,
                        None => {
                            let _ = command
                                .create_response(
                                    &ctx.http,
                                    serenity::builder::CreateInteractionResponse::Message(
                                        serenity::builder::CreateInteractionResponseMessage::new()
                                            .content("You need to be in a voice channel first."),
                                    ),
                                )
                                .await;
                            return;
                        }
                    };

                    #[cfg(feature = "songbird")]
                    {
                        let manager = songbird::serenity::get(&ctx).await;
                        if let Some(manager) = manager {
                            match manager.join(guild_id, voice_channel_id).await {
                                Ok(call) => {
                                    crate::discord::set_discord_voice_state(
                                        Some(guild_id.get()),
                                        Some(command.user.id.get()),
                                    )
                                    .await;

                                    // Set up voice handler and transcription pipeline
                                    let voice_handler = Arc::new(crate::voice::handler::VoiceHandler::new());
                                    let voice_receiver = crate::voice::handler::songbird_integration::VoiceReceiver::new(voice_handler.clone());

                                    // Register event handler on the call
                                    {
                                        let mut call_lock = call.lock().await;
                                        call_lock.add_global_event(
                                            songbird::events::Event::SpeakingStateUpdate,
                                            voice_receiver.clone(),
                                        );
                                        call_lock.add_global_event(
                                            songbird::events::Event::VoiceTick,
                                            voice_receiver.clone(),
                                        );
                                        call_lock.add_global_event(
                                            songbird::events::Event::ClientDisconnect,
                                            voice_receiver.clone(),
                                        );
                                    }

                                    // Set up transcription channel
                                    let (tx, mut rx) = tokio::sync::mpsc::channel::<(u64, Vec<i16>)>(10);
                                    voice_handler.set_transcription_channel(tx).await;

                                    // Spawn transcription processing task
                                    let db = self.db.clone();
                                    let ws_client = self.ws_client.clone();
                                    let secrets = self.secrets.clone();
                                    let voice_muted = self.voice_muted.clone();
                                    tokio::spawn(async move {
                                        tracing::info!("VOICE_PIPELINE: Transcription processor started");
                                        while let Some((discord_user_id, audio_data)) = rx.recv().await {
                                            // Check if muted
                                            if *voice_muted.lock().await {
                                                tracing::debug!("VOICE_PIPELINE: Bot is muted, skipping transcription");
                                                continue;
                                            }

                                            // Look up paired user
                                            let user_id = match db.get_pairing_by_discord(&discord_user_id.to_string()) {
                                                Ok(Some(p)) => p.user_id,
                                                _ => {
                                                    tracing::debug!("VOICE_PIPELINE: No pairing for Discord user {}, skipping", discord_user_id);
                                                    continue;
                                                }
                                            };

                                            tracing::info!("VOICE_PIPELINE: Received {} samples from user {}", audio_data.len(), user_id);

                                            // Get STT config from context
                                            let ctx = db.load_context(&user_id).unwrap_or_else(|_| crate::db::contexts::Context {
                                                user_id: user_id.clone(),
                                                ..Default::default()
                                            });
                                            let stt_type = ctx.settings.voice_stt_type.clone().unwrap_or_else(|| "vosk".to_string());
                                            let api_key = ctx.settings.voice_elevenlabs_api_key.clone()
                                                .or_else(|| secrets.elevenlabs_api_key.clone());
                                            let model_path = match stt_type.as_str() {
                                                "vosk" => ctx.settings.voice_vosk_model_path.clone(),
                                                "whisper" => ctx.settings.voice_whisper_model_path.clone(),
                                                _ => None,
                                            };

                                            // Convert i16 samples to WAV bytes
                                            let wav_data = crate::voice::pcm_to_wav(&audio_data, 48000, 1);

                                            // Transcribe
                                            let transcription = crate::voice::transcribe_audio(
                                                &wav_data,
                                                &stt_type,
                                                api_key.as_deref(),
                                                model_path.as_deref(),
                                            ).await;

                                            let text = match transcription {
                                                Ok(t) => t.trim().to_string(),
                                                Err(e) => {
                                                    tracing::warn!("VOICE_PIPELINE: Transcription failed for user {}: {}", user_id, e);
                                                    continue;
                                                }
                                            };

                                            if text.is_empty() {
                                                tracing::debug!("VOICE_PIPELINE: Empty transcription, skipping");
                                                continue;
                                            }

                                            tracing::info!("VOICE_PIPELINE: Transcribed for user {}: '{}'", user_id, text);

                                            // Check wake words
                                            let wake_words = ctx.settings.voice_wake_words.clone().unwrap_or_default();
                                            let wake_match = crate::voice::wake_word::matches_wake_word(&text, &wake_words);

                                            if !wake_match.matched {
                                                tracing::debug!("VOICE_PIPELINE: No wake word in '{}', skipping", text);
                                                continue;
                                            }

                                            let message_text = if wake_match.remaining_text.is_empty() {
                                                text.clone()
                                            } else {
                                                wake_match.remaining_text.clone()
                                            };

                                            tracing::info!("VOICE_PIPELINE: Wake word {:?} matched, sending message: '{}'",
                                                wake_match.wake_word, message_text);

                                            // Send to gateway via WsClient
                                            let payload = crate::discord::ws_client::OutgoingMessage::Message {
                                                user_id: user_id.clone(),
                                                content: message_text,
                                                channel_id: format!("voice:{}", guild_id),
                                            };

                                            let mut ws = ws_client.lock().await;
                                            if let Err(e) = ws.send(payload).await {
                                                tracing::error!("VOICE_PIPELINE: Failed to send to gateway: {}", e);
                                            }
                                        }
                                        tracing::info!("VOICE_PIPELINE: Transcription processor stopped");
                                    });

                                    tracing::info!("Joined voice channel {} in guild {} with STT pipeline", voice_channel_id, guild_id);
                                    let _ = command
                                        .create_response(
                                            &ctx.http,
                                            serenity::builder::CreateInteractionResponse::Message(
                                                serenity::builder::CreateInteractionResponseMessage::new()
                                                    .content(format!("Joined <#{}> (voice listening active)", voice_channel_id)),
                                            ),
                                        )
                                        .await;
                                }
                                Err(e) => {
                                    tracing::error!("Failed to join voice channel: {}", e);
                                    let _ = command
                                        .create_response(
                                            &ctx.http,
                                            serenity::builder::CreateInteractionResponse::Message(
                                                serenity::builder::CreateInteractionResponseMessage::new()
                                                    .content(format!("Failed to join voice channel: {}", e)),
                                            ),
                                        )
                                        .await;
                                }
                            }
                        } else {
                            let _ = command
                                .create_response(
                                    &ctx.http,
                                    serenity::builder::CreateInteractionResponse::Message(
                                        serenity::builder::CreateInteractionResponseMessage::new()
                                            .content("Voice manager not available."),
                                    ),
                                )
                                .await;
                        }
                    }

                    #[cfg(not(feature = "songbird"))]
                    {
                        let _ = command
                            .create_response(
                                &ctx.http,
                                serenity::builder::CreateInteractionResponse::Message(
                                    serenity::builder::CreateInteractionResponseMessage::new()
                                        .content("Voice support not compiled. Build with --features songbird"),
                                ),
                            )
                            .await;
                    }
                }
                "disconnect" => {
                    let guild_id = match command.guild_id {
                        Some(g) => g,
                        None => {
                            let _ = command
                                .create_response(
                                    &ctx.http,
                                    serenity::builder::CreateInteractionResponse::Message(
                                        serenity::builder::CreateInteractionResponseMessage::new()
                                            .content("This command can only be used in a server."),
                                    ),
                                )
                                .await;
                            return;
                        }
                    };

                    #[cfg(feature = "songbird")]
                    {
                        let manager = songbird::serenity::get(&ctx).await;
                        if let Some(manager) = manager {
                            if manager.get(guild_id).is_some() {
                                if let Err(e) = manager.remove(guild_id).await {
                                    tracing::error!("Failed to disconnect: {}", e);
                                }
                                crate::discord::set_discord_voice_state(None, None).await;
                                tracing::info!("Disconnected from voice in guild {}", guild_id);
                                let _ = command
                                    .create_response(
                                        &ctx.http,
                                        serenity::builder::CreateInteractionResponse::Message(
                                            serenity::builder::CreateInteractionResponseMessage::new()
                                                .content("Disconnected from voice channel."),
                                        ),
                                    )
                                    .await;
                            } else {
                                let _ = command
                                    .create_response(
                                        &ctx.http,
                                        serenity::builder::CreateInteractionResponse::Message(
                                            serenity::builder::CreateInteractionResponseMessage::new()
                                                .content("Not in a voice channel."),
                                        ),
                                    )
                                    .await;
                            }
                        }
                    }

                    #[cfg(not(feature = "songbird"))]
                    {
                        let _ = command
                            .create_response(
                                &ctx.http,
                                serenity::builder::CreateInteractionResponse::Message(
                                    serenity::builder::CreateInteractionResponseMessage::new()
                                        .content("Voice support not compiled."),
                                ),
                            )
                            .await;
                    }
                }
                _ => {}
            }
        }
    }
}

/// Split a message into chunks of max 2000 characters (Discord limit).
/// Tries to split at newlines first, then at spaces, then hard-cut.
pub fn split_message(content: &str, max_len: usize) -> Vec<String> {
    if content.len() <= max_len {
        return vec![content.to_string()];
    }

    let mut chunks = Vec::new();
    let mut remaining = content;

    while remaining.len() > max_len {
        // Try to split at newline
        let split_pos = remaining[..max_len].rfind('\n')
            .or_else(|| remaining[..max_len].rfind(' '))
            .unwrap_or(max_len);

        let (chunk, rest) = remaining.split_at(split_pos);
        chunks.push(chunk.trim().to_string());
        remaining = rest.trim_start();
    }

    if !remaining.is_empty() {
        chunks.push(remaining.to_string());
    }

    chunks
}

/// Send a message to Discord, splitting if necessary.
pub async fn send_message_split(
    http: &serenity::http::Http,
    channel_id: serenity::model::id::ChannelId,
    content: &str,
) -> Result<(), serenity::Error> {
    let chunks = split_message(content, 2000);
    for chunk in chunks {
        channel_id.say(http, &chunk).await?;
    }
    Ok(())
}

#[cfg(test)]
mod discord_tests {
    use super::*;

    #[test]
    fn test_handler_types_exist() {
        assert!(true);
    }

    #[test]
    fn test_split_message_short() {
        let msg = "Hello World";
        let chunks = split_message(msg, 2000);
        assert_eq!(chunks, vec!["Hello World"]);
    }

    #[test]
    fn test_split_message_long() {
        let msg = "a".repeat(3000);
        let chunks = split_message(&msg, 2000);
        assert_eq!(chunks.len(), 2);
        assert_eq!(chunks[0].len(), 2000);
        assert_eq!(chunks[1].len(), 1000);
    }

    #[test]
    fn test_split_message_at_newline() {
        let mut msg = "a".repeat(1990);
        msg.push('\n');
        msg.push_str("b".repeat(100).as_str());

        let chunks = split_message(&msg, 2000);
        assert_eq!(chunks.len(), 2);
        assert!(chunks[0].ends_with('a'));
        assert!(chunks[1].starts_with('b'));
    }

    #[test]
    fn test_split_message_at_space() {
        let mut msg = "a".repeat(1990);
        msg.push(' ');
        msg.push_str("b".repeat(100).as_str());

        let chunks = split_message(&msg, 2000);
        assert_eq!(chunks.len(), 2);
    }

    #[test]
    fn test_split_message_multiple_chunks() {
        let msg = "x".repeat(5000);
        let chunks = split_message(&msg, 2000);
        assert_eq!(chunks.len(), 3);
        assert_eq!(chunks[0].len(), 2000);
        assert_eq!(chunks[1].len(), 2000);
        assert_eq!(chunks[2].len(), 1000);
    }

    #[test]
    fn test_split_message_empty() {
        let chunks = split_message("", 2000);
        assert_eq!(chunks, vec![""]);
    }
}

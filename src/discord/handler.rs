use crate::db::Database;
use crate::discord::ws_client::{IncomingMessage, OutgoingMessage, WsClient};
use serenity::async_trait;
use serenity::model::application::Interaction;
use serenity::model::channel::Message;
use serenity::prelude::*;
use std::sync::Arc;
use tokio::sync::Mutex;
use tokio::time::{interval, Duration};

fn generate_silent_wav(duration_ms: u32) -> Vec<u8> {
    let spec = hound::WavSpec {
        channels: 1,
        sample_rate: 48000,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let num_samples = (48000 * duration_ms / 1000) as usize;
    let mut buffer = Vec::new();
    {
        let mut writer = hound::WavWriter::new(std::io::Cursor::new(&mut buffer), spec).unwrap();
        for i in 0..num_samples {
            // Very quiet tone (amplitude 100) instead of silence
            // This ensures Discord detects us as "speaking"
            let sample =
                ((i as f32 * 220.0 * 2.0 * std::f32::consts::PI / 48000.0).sin() * 100.0) as i16;
            writer.write_sample(sample).unwrap();
        }
        writer.finalize().unwrap();
    }
    buffer
}

pub struct DiscordHandler {
    pub db: Database,
    pub ws_client: Arc<Mutex<WsClient>>,
    pub request_lock: Arc<Mutex<()>>,
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
            request_lock: Arc::new(Mutex::new(())),
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
        tracing::debug!(
            author = %msg.author.id,
            channel = %msg.channel_id,
            content_len = msg.content.len(),
            content = %msg.content,
            "Received message"
        );

        if msg.author.bot {
            return;
        }

        // Check for empty content which indicates missing MESSAGE_CONTENT intent
        if msg.content.is_empty() {
            tracing::warn!(
                author = %msg.author.id,
                channel = %msg.channel_id,
                "Message has empty content - ensure MESSAGE_CONTENT intent is enabled in Discord Developer Portal (Bot -> Privileged Gateway Intents)"
            );
            // Don't return early - still process reactions and other interactions
        }

        if msg.content.starts_with('/') {
            return;
        }

        // Check if this is a response to a pending ask_question
        let channel_id = msg.channel_id.to_string();
        let is_question_response = if !msg.content.is_empty() {
            // Don't hold the lock while calling handle_message_reply
            crate::tools::discord_interactive::handle_message_reply(
                &channel_id,
                &msg.content,
                &msg.author.id.to_string(),
            )
            .await
        } else {
            false
        };

        // If this was a question response, don't process it as a normal message
        if is_question_response {
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
                tracing::warn!("User {} is not paired, ignoring message", discord_user_id);
                return;
            }
        };

        // Check if there's an active agent loop for this user - inject message if so
        if let Some(sender) = crate::gateway::agent_loop::get_user_input_sender(&pairing.user_id).await {
            tracing::info!("Injecting Discord message into active agent loop for user {}", pairing.user_id);
            if let Err(e) = sender.send(msg.content.clone()) {
                tracing::warn!("Failed to inject message into agent loop: {}", e);
            } else {
                let _ = msg.reply(&ctx.http, "Message received - continuing...").await;
            }
            return;
        }

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

        let _req_guard = self.request_lock.lock().await;

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

        let mut thinking_msg_ids: Vec<serenity::model::id::MessageId> = Vec::new();

        loop {
            match ws_client.recv().await {
                Ok(IncomingMessage::Response { content, .. }) => {
                    typing_handle.abort();
                    for mid in &thinking_msg_ids {
                        let _ = msg.channel_id.delete_message(&ctx.http, mid).await;
                    }
                    let trimmed = content.trim();
                    if !trimmed.is_empty() {
                        if let Err(e) = send_message_split(&ctx.http, msg.channel_id, trimmed).await
                        {
                            tracing::error!("Failed to send response: {}", e);
                        }
                    } else {
                        tracing::warn!("LLM returned empty response, skipping send");
                    }
                    break;
                }
                Ok(IncomingMessage::Feedback { content, .. }) => {
                    match msg.channel_id.say(&ctx.http, &content).await {
                        Ok(feedback_msg) => {
                            thinking_msg_ids.push(feedback_msg.id);
                        }
                        Err(e) => {
                            tracing::error!("Failed to send feedback: {}", e);
                        }
                    }
                }
                Ok(IncomingMessage::Error { message }) => {
                    typing_handle.abort();
                    for mid in &thinking_msg_ids {
                        let _ = msg.channel_id.delete_message(&ctx.http, mid).await;
                    }
                    let _ = msg.reply(&ctx.http, format!("Error: {}", message)).await;
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
                    for mid in &thinking_msg_ids {
                        let _ = msg.channel_id.delete_message(&ctx.http, mid).await;
                    }
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
                        crate::discord::commands::handle_pair_command(&self.db, &ctx, &command)
                            .await
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
                        obj.insert("mode".to_string(), serde_json::json!(new_mode));
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

                    let voice_channel_id = ctx.cache.guild(guild_id).and_then(|g| {
                        g.voice_states
                            .get(&command.user.id)
                            .and_then(|vs| vs.channel_id)
                    });

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

                                    // Check context for deafened and voice_enabled settings
                                    let discord_user_id = command.user.id.to_string();
                                    let (should_deafen, voice_listening) =
                                        match self.db.get_pairing_by_discord(&discord_user_id) {
                                            Ok(Some(pairing)) => {
                                                match self.db.load_context(&pairing.user_id) {
                                                    Ok(ctx) => (
                                                        ctx.settings.voice_deafened,
                                                        ctx.settings.voice_enabled
                                                            && ctx.settings.use_stt,
                                                    ),
                                                    Err(_) => (true, false),
                                                }
                                            }
                                            _ => (true, false),
                                        };

                                    {
                                        let mut call_lock = call.lock().await;
                                        let _ = call_lock.deafen(should_deafen).await;
                                    }
                                    *self.voice_deafened.lock().await = should_deafen;
                                    tracing::info!(
                                        "Voice: Bot deafened={} (from context)",
                                        should_deafen
                                    );

                                    // Set up voice handler and transcription pipeline
                                    let voice_handler =
                                        Arc::new(crate::voice::handler::VoiceHandler::new());

                                    let fallback_discord_id = command.user.id.get();

                                    // Set allowed Discord user IDs (only paired users)
                                    let mut allowed_ids = vec![fallback_discord_id];
                                    if let Ok(all_pairings) = self.db.list_all_pairings() {
                                        for p in &all_pairings {
                                            if let Ok(did) = p.discord_user_id.parse::<u64>() {
                                                allowed_ids.push(did);
                                            }
                                        }
                                    }
                                    voice_handler.set_allowed_discord_ids(allowed_ids).await;

                                    // Load context for voice settings
                                    if let Ok(Some(pairing)) = self
                                        .db
                                        .get_pairing_by_discord(&fallback_discord_id.to_string())
                                    {
                                        if let Ok(ctx) = self.db.load_context(&pairing.user_id) {
                                            voice_handler
                                                .set_auto_pause(
                                                    ctx.settings.voice_auto_pause_enabled,
                                                )
                                                .await;
                                        }
                                    }

                                    let voice_receiver = crate::voice::handler::songbird_integration::VoiceReceiver::new(voice_handler.clone());

                                    // Register event handler on the call
                                    {
                                        let mut call_lock = call.lock().await;
                                        call_lock.add_global_event(
                                            songbird::events::Event::Core(
                                                songbird::events::CoreEvent::SpeakingStateUpdate,
                                            ),
                                            voice_receiver.clone(),
                                        );
                                        call_lock.add_global_event(
                                            songbird::events::Event::Core(
                                                songbird::events::CoreEvent::VoiceTick,
                                            ),
                                            voice_receiver.clone(),
                                        );
                                        call_lock.add_global_event(
                                            songbird::events::Event::Core(
                                                songbird::events::CoreEvent::ClientDisconnect,
                                            ),
                                            voice_receiver.clone(),
                                        );

                                        // Play a brief silent clip to force Discord to send
                                        // SpeakingStateUpdate events for all users in the channel.
                                        // Without this, the SSRC-to-user mapping never populates
                                        // and we can't identify who is speaking.
                                        let silent_wav = generate_silent_wav(500);
                                        tracing::info!("VOICE: Playing silent WAV ({} bytes) to trigger SpeakingStateUpdate", silent_wav.len());
                                        call_lock
                                            .play_input(songbird::input::Input::from(silent_wav));
                                        tracing::info!(
                                            "VOICE: Silent WAV queued, waiting for events..."
                                        );
                                    }

                                    // Set up transcription channel
                                    let (tx, mut rx) =
                                        tokio::sync::mpsc::channel::<(String, Vec<i16>)>(10);
                                    voice_handler.set_transcription_channel(tx).await;

                                    // Spawn periodic buffer flusher (independent of VoiceTick)
                                    let flush_handler = voice_handler.clone();
                                    tokio::spawn(async move {
                                        let mut interval = tokio::time::interval(
                                            std::time::Duration::from_millis(200),
                                        );
                                        loop {
                                            interval.tick().await;
                                            flush_handler.process_all_buffers().await;
                                        }
                                    });

                                    // Spawn transcription processing task
                                    let db = self.db.clone();
                                    let ws_client = self.ws_client.clone();
                                    let req_lock = self.request_lock.clone();
                                    let secrets = self.secrets.clone();
                                    let voice_muted = self.voice_muted.clone();
                                    let http = ctx.http.clone();
                                    tokio::spawn(async move {
                                        tracing::info!(
                                            "VOICE_PIPELINE: Transcription processor started"
                                        );
                                        while let Some((user_id, audio_data)) = rx.recv().await {
                                            // Check if muted
                                            if *voice_muted.lock().await {
                                                tracing::debug!("VOICE_PIPELINE: Bot is muted, skipping transcription");
                                                continue;
                                            }

                                            tracing::info!(
                                                "VOICE_PIPELINE: Received {} samples from user {}",
                                                audio_data.len(),
                                                user_id
                                            );

                                            // Resolve Discord ID → Praxis user ID for context lookup
                                            let praxis_user_id = if let Ok(Some(pairing)) =
                                                db.get_pairing_by_discord(&user_id)
                                            {
                                                pairing.user_id.clone()
                                            } else {
                                                user_id.clone()
                                            };

                                            // Get STT config from context
                                            let ctx = db
                                                .load_context(&praxis_user_id)
                                                .unwrap_or_else(|_| crate::db::contexts::Context {
                                                    user_id: praxis_user_id.clone(),
                                                    ..Default::default()
                                                });
                                            let stt_type = ctx.settings.voice_stt_type.clone();

                                            if !ctx.settings.voice_enabled || !ctx.settings.use_stt
                                            {
                                                tracing::debug!("VOICE_PIPELINE: voice_enabled={}, use_stt={} for user {}, skipping transcription", ctx.settings.voice_enabled, ctx.settings.use_stt, user_id);
                                                continue;
                                            }

                                            let api_key = secrets.elevenlabs_api_key.clone();
                                            let model_path = match stt_type.as_str() {
                                                "vosk" => {
                                                    ctx.settings.voice_vosk_model_path.clone()
                                                }
                                                "whisper" => {
                                                    ctx.settings.voice_whisper_model_path.clone()
                                                }
                                                _ => None,
                                            };

                                            let stt_config = crate::voice::STTConfig {
                                                engine: stt_type,
                                                api_key,
                                                model_path,
                                                elevenlabs_model: ctx
                                                    .settings
                                                    .elevenlabs_stt_model
                                                    .clone(),
                                                elevenlabs_language: ctx
                                                    .settings
                                                    .elevenlabs_stt_language
                                                    .clone(),
                                                elevenlabs_tag_audio_events: ctx
                                                    .settings
                                                    .elevenlabs_stt_tag_audio_events,
                                                elevenlabs_no_verbatim: ctx
                                                    .settings
                                                    .elevenlabs_stt_no_verbatim,
                                            };

                                            let wav_data =
                                                crate::voice::pcm_to_wav(&audio_data, 16000, 1);

                                            let transcription = crate::voice::transcribe_audio(
                                                &wav_data,
                                                &stt_config,
                                            )
                                            .await;

                                            let text = match transcription {
                                                Ok(t) => t.trim().to_string(),
                                                Err(e) => {
                                                    tracing::warn!("VOICE_PIPELINE: Transcription failed for user {}: {}", user_id, e);
                                                    continue;
                                                }
                                            };

                                            if text.is_empty() {
                                                tracing::debug!(
                                                    "VOICE_PIPELINE: Empty transcription, skipping"
                                                );
                                                continue;
                                            }

                                            tracing::info!(
                                                "VOICE_PIPELINE: Transcribed for user {}: '{}'",
                                                user_id,
                                                text
                                            );

                                            // Check wake words
                                            let wake_words = ctx.settings.voice_wake_words.clone();
                                            let wake_match =
                                                crate::voice::wake_word::matches_wake_word(
                                                    &text,
                                                    &wake_words,
                                                );

                                            if !wake_match.matched {
                                                tracing::debug!("VOICE_PIPELINE: No wake word in '{}', skipping", text);
                                                continue;
                                            }

                                            let message_text =
                                                if wake_match.remaining_text.is_empty() {
                                                    text.clone()
                                                } else {
                                                    wake_match.remaining_text.clone()
                                                };

                                            tracing::info!("VOICE_PIPELINE: Wake word {:?} matched, sending message: '{}'",
                                                wake_match.wake_word, message_text);

                                            // Send to gateway via WsClient
                                            let payload = crate::discord::ws_client::OutgoingMessage::Message {
                                                user_id: praxis_user_id.clone(),
                                                content: message_text,
                                                channel_id: format!("voice:{}", guild_id),
                                            };

                                            // Acquire request lock to prevent race with text handler
                                            let _req_guard = req_lock.lock().await;

                                            {
                                                let ws = ws_client.lock().await;
                                                if let Err(e) = ws.send(payload).await {
                                                    tracing::error!(
                                                        "VOICE_PIPELINE: Failed to send to gateway: {}",
                                                        e
                                                    );
                                                    continue;
                                                }
                                            }

                                            // Consume the response and send text to Discord
                                            let response_text;
                                            {
                                                let ws = ws_client.lock().await;
                                                match ws.recv().await {
                                                    Ok(crate::discord::ws_client::IncomingMessage::Response { content, .. }) => {
                                                        tracing::info!("VOICE_PIPELINE: Got response ({} chars)", content.len());
                                                        response_text = Some(content);
                                                    }
                                                    Ok(crate::discord::ws_client::IncomingMessage::Feedback { .. }) => {
                                                        response_text = None;
                                                    }
                                                    Ok(crate::discord::ws_client::IncomingMessage::Error { message }) => {
                                                        tracing::warn!("VOICE_PIPELINE: Gateway error: {}", message);
                                                        response_text = None;
                                                    }
                                                    Ok(_) => { response_text = None; }
                                                    Err(e) => {
                                                        tracing::warn!("VOICE_PIPELINE: Failed to receive response: {}", e);
                                                        response_text = None;
                                                    }
                                                }
                                            }
                                            // _req_guard dropped here, releasing lock

                                            // Send text response to Discord channel
                                            if let Some(text) = response_text {
                                                if !text.trim().is_empty() {
                                                    let guild = guild_id;
                                                    if let Ok(channels) =
                                                        guild.channels(&http).await
                                                    {
                                                        if let Some((channel_id, _)) = channels.iter()
                                                            .find(|(_, ch)| ch.kind == serenity::model::channel::ChannelType::Text)
                                                        {
                                                            if let Err(e) = send_message_split(
                                                                &http, *channel_id, &text
                                                            ).await {
                                                                tracing::error!("VOICE_PIPELINE: Failed to send text response: {}", e);
                                                            }
                                                        }
                                                    }
                                                }
                                            }
                                        }
                                        tracing::info!(
                                            "VOICE_PIPELINE: Transcription processor stopped"
                                        );
                                    });

                                    tracing::info!(
                                        "Joined voice channel {} in guild {} with STT pipeline",
                                        voice_channel_id,
                                        guild_id
                                    );
                                    let _ = command
                                        .create_response(
                                            &ctx.http,
                                            serenity::builder::CreateInteractionResponse::Message(
                                                serenity::builder::CreateInteractionResponseMessage::new()
                                                    .content(if voice_listening {
                                                        format!("Joined <#{}> (voice listening active)", voice_channel_id)
                                                    } else {
                                                        format!("Joined <#{}> (voice listening disabled)", voice_channel_id)
                                                    }),
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
                "compact" => {
                    let discord_user_id = command.user.id.to_string();
                    let pairing = match self.db.get_pairing_by_discord(&discord_user_id) {
                        Ok(Some(p)) => p,
                        _ => {
                            let _ = command
                                .create_response(
                                    &ctx.http,
                                    serenity::builder::CreateInteractionResponse::Message(
                                        serenity::builder::CreateInteractionResponseMessage::new()
                                            .content("Please pair first with /pair"),
                                    ),
                                )
                                .await;
                            return;
                        }
                    };

                    let _ = command
                        .create_response(
                            &ctx.http,
                            serenity::builder::CreateInteractionResponse::Defer(
                                serenity::builder::CreateInteractionResponseMessage::new()
                                    .content("Compacting conversation..."),
                            ),
                        )
                        .await;

                    let compact_msg = crate::discord::ws_client::OutgoingMessage::Compact {
                        user_id: pairing.user_id.clone(),
                    };

                    let ws = self.ws_client.lock().await;
                    match ws.send_and_recv_until_response(compact_msg, |_| {}).await {
                        Ok(Ok(response)) => {
                            let _ = command
                                .edit_response(
                                    &ctx.http,
                                    serenity::builder::EditInteractionResponse::default()
                                        .content(&response),
                                )
                                .await;
                        }
                        Ok(Err(e)) => {
                            let _ = command
                                .edit_response(
                                    &ctx.http,
                                    serenity::builder::EditInteractionResponse::default()
                                        .content(format!("Compaction failed: {}", e)),
                                )
                                .await;
                        }
                        Err(e) => {
                            tracing::error!("Compact WS error: {}", e);
                            let _ = command
                                .edit_response(
                                    &ctx.http,
                                    serenity::builder::EditInteractionResponse::default()
                                        .content("Failed to compact. Try again."),
                                )
                                .await;
                        }
                    }
                }
                _ => {}
            }
        }
    }

    async fn reaction_add(&self, _ctx: Context, reaction: serenity::model::channel::Reaction) {
        let channel_id = reaction.channel_id.to_string();
        let emoji_raw = reaction.emoji.as_data();
        let emoji = urlencoding::decode(&emoji_raw)
            .unwrap_or_default()
            .into_owned();
        let user_id = reaction.user_id.map(|u| u.to_string()).unwrap_or_default();

        crate::tools::discord_interactive::handle_reaction(&channel_id, &emoji, &user_id).await;
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
        let split_pos = remaining[..max_len]
            .rfind('\n')
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

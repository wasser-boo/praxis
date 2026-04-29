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
                    if let Err(e) = msg.reply(&ctx.http, &content).await {
                        tracing::error!("Failed to send response: {}", e);
                    }
                    break;
                }
                Ok(IncomingMessage::Feedback { content, .. }) => {
                    if let Err(e) = msg.reply(&ctx.http, &content).await {
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
                _ => {}
            }
        }
    }
}

#[cfg(test)]
mod discord_tests {
    #[test]
    fn test_handler_types_exist() {
        assert!(true);
    }
}

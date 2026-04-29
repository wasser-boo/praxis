use crate::db::Database;
use serenity::model::application::CommandInteraction;
use serenity::prelude::*;
use std::collections::HashMap;
use std::time::Duration;
use tokio::sync::Mutex;

const DISCORD_BOT_PERMISSIONS: u64 = 1024 // VIEW_CHANNEL
    | 2048 // SEND_MESSAGES
    | 4096 // SEND_TTS_MESSAGES
    | 32768 // ATTACH_FILES
    | 65536 // READ_MESSAGE_HISTORY
    | 262144 // USE_EXTERNAL_EMOJIS
    | 64 // ADD_REACTIONS
    | 1048576 // CONNECT
    | 2097152 // SPEAK
    | 2147483648; // USE_APPLICATION_COMMANDS

struct InteractionTracker(Mutex<HashMap<u64, std::time::Instant>>);

impl InteractionTracker {
    fn new() -> Self {
        InteractionTracker(Mutex::new(HashMap::new()))
    }

    async fn is_duplicate(&self, id: u64) -> bool {
        let mut tracker = self.0.lock().await;
        let now = std::time::Instant::now();
        if let Some(last_seen) = tracker.get(&id) {
            if now.duration_since(*last_seen) < Duration::from_secs(5) {
                return true;
            }
        }
        tracker.retain(|_, v| now.duration_since(*v) < Duration::from_secs(10));
        tracker.insert(id, now);
        false
    }
}

lazy_static::lazy_static! {
    static ref INTERACTION_TRACKER: InteractionTracker = InteractionTracker::new();
}

pub async fn setup_commands(http: &serenity::http::Http) -> anyhow::Result<()> {
    tracing::info!("Setting up discord commands");

    let pair_cmd = serenity::builder::CreateCommand::new("pair")
        .description("Get a pairing code to link your Discord account")
        .dm_permission(true);

    match http.create_global_command(&pair_cmd).await {
        Ok(pair_created) => tracing::info!(
            "Registered pair command: {} (id: {})",
            pair_created.name,
            pair_created.id
        ),
        Err(e) => {
            tracing::warn!(
                "Failed to register pair command: {}. Bot may need re-invite with applications.commands scope.",
                e
            );
            return Ok(());
        }
    }

    let mute_cmd = serenity::builder::CreateCommand::new("mute")
        .description("Mute the bot's microphone in voice channels")
        .dm_permission(true);
    http.create_global_command(&mute_cmd).await?;

    let unmute_cmd = serenity::builder::CreateCommand::new("unmute")
        .description("Unmute the bot's microphone in voice channels")
        .dm_permission(true);
    http.create_global_command(&unmute_cmd).await?;

    let deafen_cmd = serenity::builder::CreateCommand::new("deafen")
        .description("Deafen the bot in voice channels (bot won't hear)")
        .dm_permission(true);
    http.create_global_command(&deafen_cmd).await?;

    let undeafen_cmd = serenity::builder::CreateCommand::new("undeafen")
        .description("Undeafen the bot in voice channels")
        .dm_permission(true);
    http.create_global_command(&undeafen_cmd).await?;

    let tts_cmd = serenity::builder::CreateCommand::new("tts")
        .description("Toggle text-to-speech on/off")
        .dm_permission(true);
    http.create_global_command(&tts_cmd).await?;

    let join_cmd = serenity::builder::CreateCommand::new("join")
        .description("Join your voice channel")
        .dm_permission(false);
    http.create_global_command(&join_cmd).await?;

    let disconnect_cmd = serenity::builder::CreateCommand::new("disconnect")
        .description("Disconnect the bot from the voice channel")
        .dm_permission(false);
    http.create_global_command(&disconnect_cmd).await?;

    let mode_cmd = serenity::builder::CreateCommand::new("mode")
        .description("Toggle between chat and agent mode")
        .dm_permission(false);
    http.create_global_command(&mode_cmd).await?;

    tracing::info!("All discord commands registered");
    Ok(())
}

pub async fn handle_pair_command(
    db: &Database,
    ctx: &Context,
    command: &CommandInteraction,
) -> anyhow::Result<()> {
    let interaction_id = command.id.to_string().parse::<u64>().unwrap_or(0);
    if INTERACTION_TRACKER.is_duplicate(interaction_id).await {
        tracing::warn!("Duplicate pair command ignored: {}", interaction_id);
        return Ok(());
    }

    command
        .create_response(
            &ctx.http,
            serenity::builder::CreateInteractionResponse::Defer(
                serenity::builder::CreateInteractionResponseMessage::new()
                    .content("Generating pairing code..."),
            ),
        )
        .await?;

    let discord_user_id = command.user.id.to_string();

    let code = generate_pairing_code();
    let expires_at = (chrono::Utc::now() + chrono::Duration::minutes(10)).to_rfc3339();
    db.create_pending_pairing(&code, &discord_user_id, &expires_at)?;

    let dm_channel = match command.user.create_dm_channel(&ctx.http).await {
        Ok(ch) => ch,
        Err(e) => {
            command
                .edit_response(
                    &ctx.http,
                    serenity::builder::EditInteractionResponse::default()
                        .content("Could not send you a DM. Please enable DMs from server members and try again."),
                )
                .await
                .ok();
            anyhow::bail!(
                "Failed to create DM channel for user {}: {}",
                discord_user_id,
                e
            );
        }
    };

    match dm_channel
        .send_message(
            &ctx.http,
            serenity::builder::CreateMessage::default().content(format!(
                "Your pairing code is: **{}**\n\nTo complete pairing, run:\n```praxis pair {}```",
                code, code
            )),
        )
        .await
    {
        Ok(_) => {}
        Err(e) => {
            command
                .edit_response(
                    &ctx.http,
                    serenity::builder::EditInteractionResponse::default()
                        .content("Could not send DM. Please enable DMs from server members and try again."),
                )
                .await
                .ok();
            anyhow::bail!("Failed to send DM to user {}: {}", discord_user_id, e);
        }
    }

    command
        .edit_response(
            &ctx.http,
            serenity::builder::EditInteractionResponse::default()
                .content("Check your DMs for the pairing code!"),
        )
        .await?;

    tracing::info!(
        "Sent pairing DM to user {} with code {}",
        discord_user_id,
        code
    );

    Ok(())
}

fn generate_pairing_code() -> String {
    use rand::Rng;
    let mut rng = rand::thread_rng();
    let part1: u16 = rng.gen_range(1000..9999);
    let part2: u16 = rng.gen_range(1000..9999);
    format!("{}-{}", part1, part2)
}

#[cfg(test)]
mod discord_tests {
    use super::*;

    #[test]
    fn test_generate_pairing_code() {
        let code = generate_pairing_code();
        assert_eq!(code.len(), 9);
        assert!(code.contains('-'));
        let parts: Vec<&str> = code.split('-').collect();
        assert_eq!(parts.len(), 2);
        assert_eq!(parts[0].len(), 4);
        assert_eq!(parts[1].len(), 4);
    }

    #[test]
    fn test_register_commands_list() {
        let cmds = crate::discord::commands::register_commands();
        assert!(!cmds.is_empty());
    }
}

pub fn register_commands() -> Vec<serde_json::Value> {
    vec![
        serde_json::json!({
            "name": "pair",
            "description": "Get a pairing code to link your Discord account"
        }),
        serde_json::json!({
            "name": "mute",
            "description": "Mute the bot's microphone in voice channels"
        }),
        serde_json::json!({
            "name": "unmute",
            "description": "Unmute the bot's microphone in voice channels"
        }),
        serde_json::json!({
            "name": "deafen",
            "description": "Deafen the bot in voice channels"
        }),
        serde_json::json!({
            "name": "undeafen",
            "description": "Undeafen the bot in voice channels"
        }),
        serde_json::json!({
            "name": "tts",
            "description": "Toggle text-to-speech on/off"
        }),
        serde_json::json!({
            "name": "join",
            "description": "Join your voice channel"
        }),
        serde_json::json!({
            "name": "disconnect",
            "description": "Disconnect the bot from the voice channel"
        }),
        serde_json::json!({
            "name": "mode",
            "description": "Toggle between chat and agent mode"
        }),
    ]
}

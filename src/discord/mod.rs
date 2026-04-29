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

const DEFAULT_GATEWAY_URL: &str = "ws://localhost:3537/ws";

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
        let token = std::env::var("DISCORD_BOT_TOKEN")
            .context("DISCORD_BOT_TOKEN not set. Run 'praxis onboard --interactive' to configure.")?;
        let application_id = std::env::var("DISCORD_APPLICATION_ID")
            .context("DISCORD_APPLICATION_ID not set")?
            .parse::<u64>()
            .context("DISCORD_APPLICATION_ID must be a number")?;

        self.start_with_token(&token, application_id).await
    }

    pub async fn start_with_token(self, token: &str, application_id: u64) -> anyhow::Result<()> {
        let handler = DiscordHandler::new(self.db.clone(), self.ws_client.clone(), self.secrets.clone());

        let mut client = Client::builder(
            token,
            GatewayIntents::MESSAGE_CONTENT
                | GatewayIntents::DIRECT_MESSAGES
                | GatewayIntents::GUILD_MESSAGES
                | GatewayIntents::GUILD_VOICE_STATES
                | GatewayIntents::GUILD_MEMBERS,
        )
        .application_id(serenity::all::ApplicationId::new(application_id))
        .event_handler(handler)
        .await?;

        if let Err(e) = commands::setup_commands(&client.http).await {
            tracing::error!("Failed to setup commands: {}", e);
        }

        client.start().await?;
        Ok(())
    }
}

pub async fn start(db: Database) -> anyhow::Result<()> {
    let secrets = crate::db::secrets::get_secrets().await.unwrap_or_default();
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
}

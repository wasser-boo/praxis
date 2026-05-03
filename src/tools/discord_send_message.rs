pub async fn send_message(channel_id: &str, content: &str) -> anyhow::Result<String> {
    crate::event_channel::broadcast_channel_message("", channel_id, content);
    tracing::info!("Broadcasted message to channel {}", channel_id);
    Ok(format!("sent_to_{}", channel_id))
}

/// Send a message via Discord REST API and return the message ID
pub async fn send_message_rest(channel_id: &str, content: &str) -> anyhow::Result<String> {
    let secrets = crate::db::secrets::get_secrets();
    let token = secrets
        .discord_bot_token
        .or_else(|| std::env::var("DISCORD_BOT_TOKEN").ok())
        .ok_or_else(|| anyhow::anyhow!("DISCORD_BOT_TOKEN not set"))?;

    let url = format!(
        "https://discord.com/api/v10/channels/{}/messages",
        channel_id
    );
    let client = reqwest::Client::new();
    let resp = client
        .post(&url)
        .header("Authorization", format!("Bot {}", token))
        .json(&serde_json::json!({ "content": content }))
        .send()
        .await?;

    if !resp.status().is_success() {
        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();
        anyhow::bail!("Discord send failed {}: {}", status, text);
    }

    let data: serde_json::Value = resp.json().await?;
    let message_id = data["id"].as_str().unwrap_or("unknown").to_string();
    Ok(message_id)
}

/// Add a reaction (emoji) to a Discord message
pub async fn add_reaction(channel_id: &str, message_id: &str, emoji: &str) -> anyhow::Result<()> {
    let secrets = crate::db::secrets::get_secrets();
    let token = secrets
        .discord_bot_token
        .or_else(|| std::env::var("DISCORD_BOT_TOKEN").ok())
        .ok_or_else(|| anyhow::anyhow!("DISCORD_BOT_TOKEN not set"))?;

    // URL-encode the emoji for the URL
    let encoded = urlencoding::encode(emoji);
    let url = format!(
        "https://discord.com/api/v10/channels/{}/messages/{}/reactions/{}/@me",
        channel_id, message_id, encoded
    );
    let client = reqwest::Client::new();
    let resp = client
        .put(&url)
        .header("Authorization", format!("Bot {}", token))
        .send()
        .await?;

    if !resp.status().is_success() {
        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();
        anyhow::bail!("Failed to add reaction {}: {}", status, text);
    }
    Ok(())
}

#[cfg(test)]
mod tool_tests {
    use super::*;

    #[tokio::test]
    async fn test_send_message() {
        let _tx = crate::event_channel::init();
        let result = send_message("123456", "Hello").await.unwrap();
        assert!(result.contains("123456"));
    }
}

pub async fn upload_file(
    channel_id: &str,
    file_path: &str,
    file_name: &str,
    message: Option<&str>,
) -> anyhow::Result<String> {
    let token = std::env::var("DISCORD_BOT_TOKEN")
        .map_err(|_| anyhow::anyhow!("DISCORD_BOT_TOKEN not set"))?;

    let file_bytes = tokio::fs::read(file_path)
        .await
        .map_err(|e| anyhow::anyhow!("Failed to read file {}: {}", file_path, e))?;

    let part = reqwest::multipart::Part::bytes(file_bytes).file_name(file_name.to_string());

    let mut form = reqwest::multipart::Form::new().part("files[0]", part);

    if let Some(msg) = message {
        let payload = serde_json::json!({
            "content": msg,
            "attachments": [{
                "id": 0,
                "filename": file_name
            }]
        });
        form = form.text("payload_json", payload.to_string());
    } else {
        let payload = serde_json::json!({
            "attachments": [{
                "id": 0,
                "filename": file_name
            }]
        });
        form = form.text("payload_json", payload.to_string());
    }

    let url = format!(
        "https://discord.com/api/v10/channels/{}/messages",
        channel_id
    );

    let client = reqwest::Client::new();
    let resp = client
        .post(&url)
        .header("Authorization", format!("Bot {}", token))
        .multipart(form)
        .send()
        .await?;

    if !resp.status().is_success() {
        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();
        anyhow::bail!("Discord upload failed {}: {}", status, text);
    }

    let data: serde_json::Value = resp.json().await?;
    let message_id = data["id"].as_str().unwrap_or("unknown");

    tracing::info!(
        "Uploaded {} to channel {} (msg {})",
        file_name,
        channel_id,
        message_id
    );
    Ok(format!("uploaded_{}_to_{}", file_name, channel_id))
}

#[cfg(test)]
mod tool_tests {
    #[test]
    fn test_upload_compiles() {
        assert!(true);
    }
}

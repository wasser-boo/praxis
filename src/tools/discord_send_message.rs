pub async fn send_message(channel_id: &str, content: &str) -> anyhow::Result<String> {
    crate::event_channel::broadcast_channel_message("", channel_id, content);
    tracing::info!("Broadcasted message to channel {}", channel_id);
    Ok(format!("sent_to_{}", channel_id))
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

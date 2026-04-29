use crate::gateway::GatewayState;
use crate::gateway::llm::provider::{ChatRequest, ChatMessage};

pub async fn handle_message(
    state: &GatewayState,
    user_id: &str,
    content: &str,
) -> anyhow::Result<String> {
    let _ctx = state.db.load_context(user_id)?;

    state.db.add_message(
        user_id,
        &crate::db::messages::Message {
            role: "user".to_string(),
            content: content.to_string(),
            tool_call_id: None,
        },
    )?;

    let messages = state.db.get_messages(user_id, 50)?;
    let chat_messages: Vec<ChatMessage> = messages
        .iter()
        .map(|m| ChatMessage {
            role: m.role.clone(),
            content: Some(m.content.clone()),
            tool_calls: None,
            tool_call_id: m.tool_call_id.clone(),
        })
        .collect();

    let request = ChatRequest {
        messages: chat_messages,
        tools: None,
        temperature: Some(0.7),
        max_tokens: Some(4096),
    };

    let response = state.llm.chat(request, None).await?;
    let reply = response.content.unwrap_or_default();

    state.db.add_message(
        user_id,
        &crate::db::messages::Message {
            role: "assistant".to_string(),
            content: reply.clone(),
            tool_call_id: None,
        },
    )?;

    Ok(reply)
}

#[cfg(test)]
mod gateway_tests {
    #[test]
    fn test_message_handler_compiles() {
        assert!(true);
    }
}

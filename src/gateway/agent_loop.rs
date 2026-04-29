use crate::gateway::GatewayState;
use crate::gateway::llm::provider::{ChatRequest, ChatMessage, ToolCall};

pub async fn run_agent_loop(
    state: &GatewayState,
    user_id: &str,
    max_turns: Option<i32>,
) -> anyhow::Result<String> {
    let max = max_turns.unwrap_or(10);
    let mut turn = 0;

    loop {
        if turn >= max {
            return Ok("Max turns reached".to_string());
        }

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

        if let Some(content) = &response.content {
            state.db.add_message(
                user_id,
                &crate::db::messages::Message {
                    role: "assistant".to_string(),
                    content: content.clone(),
                    tool_call_id: None,
                },
            )?;

            return Ok(content.clone());
        }

        turn += 1;
    }
}

#[cfg(test)]
mod gateway_tests {
    #[test]
    fn test_agent_loop_compiles() {
        assert!(true);
    }
}

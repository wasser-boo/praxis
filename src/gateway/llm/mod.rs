pub mod anthropic;
pub mod mimo;
pub mod minimax;
pub mod ollama;
pub mod openai;
pub mod provider;
mod tests;

use provider::{ChatRequest, ChatResponse, LLMProvider};

pub struct LLMRouter {
    providers: Vec<Box<dyn LLMProvider>>,
    default_provider: String,
}

impl LLMRouter {
    pub fn new(config: &crate::config::Config, secrets: &crate::db::secrets::Secrets) -> Self {
        let mut providers: Vec<Box<dyn LLMProvider>> = Vec::new();

        if let Some(ref key) = secrets.openai_api_key {
            providers.push(Box::new(openai::OpenAIProvider::new(
                key.clone(),
                config.openai_model.clone(),
                config.openai_api_base.clone(),
            )));
        }

        if let Some(ref key) = secrets.anthropic_api_key {
            providers.push(Box::new(anthropic::AnthropicProvider::new(
                key.clone(),
                config.anthropic_model.clone(),
                config.anthropic_api_base.clone(),
            )));
        }

        providers.push(Box::new(ollama::OllamaProvider::new(
            config.ollama_api_base.clone(),
            config.ollama_model.clone(),
        )));

        if let Some(ref key) = secrets.minimax_api_key {
            providers.push(Box::new(minimax::MiniMaxProvider::new(
                key.clone(),
                config.minimax_model.clone(),
                config.minimax_api_base.clone(),
                config.minimax_api_mode.clone(),
            )));
        }

        if let Some(ref key) = secrets.mimo_api_key {
            providers.push(Box::new(mimo::MiMoProvider::new(
                key.clone(),
                config.mimo_model.clone(),
                config.mimo_api_base.clone(),
                config.mimo_api_mode.clone(),
            )));
        }

        Self {
            providers,
            default_provider: config.use_provider.clone(),
        }
    }

    pub async fn chat(
        &self,
        request: ChatRequest,
        provider: Option<&str>,
    ) -> anyhow::Result<ChatResponse> {
        let provider_name = provider.unwrap_or(&self.default_provider);

        for p in &self.providers {
            if p.name() == provider_name {
                match p.chat(request.clone()).await {
                    Ok(response) => return Ok(response),
                    Err(e) => {
                        tracing::warn!("Provider {} failed: {}", provider_name, e);
                        break;
                    }
                }
            }
        }

        for p in &self.providers {
            if p.name() != provider_name {
                tracing::info!("Trying fallback provider: {}", p.name());
                match p.chat(request.clone()).await {
                    Ok(response) => {
                        tracing::info!("Fallback to {} successful", p.name());
                        return Ok(response);
                    }
                    Err(e) => {
                        tracing::warn!("Fallback {} failed: {}", p.name(), e);
                    }
                }
            }
        }

        Err(anyhow::anyhow!("All LLM providers failed"))
    }

    pub async fn health_check(&self) -> bool {
        for p in &self.providers {
            if p.health_check().await {
                return true;
            }
        }
        false
    }

    pub fn provider_names(&self) -> Vec<String> {
        self.providers
            .iter()
            .map(|p| p.name().to_string())
            .collect()
    }

    pub fn default_provider(&self) -> &str {
        &self.default_provider
    }

    /// Multi-turn tool loop: calls LLM, executes tools, feeds results back.
    /// Loops until no more tool calls or max_iterations reached.
    pub async fn chat_with_tools(
        &self,
        db: &crate::db::Database,
        user_id: &str,
        mut messages: Vec<provider::ChatMessage>,
        tools: Vec<provider::ToolDefinition>,
        max_iterations: Option<i32>,
        feedback_tx: Option<tokio::sync::mpsc::UnboundedSender<String>>,
    ) -> anyhow::Result<ChatWithToolsResult> {
        let max_iterations = max_iterations.unwrap_or(200);
        let mut iteration = 0;
        let mut tool_call_records: Vec<ToolCallRecord> = Vec::new();
        let mut feedback_messages: Vec<String> = Vec::new();
        let mut last_assistant_response = String::new();
        let mut agent_signal = AgentSignalFromTool::None;

        loop {
            iteration += 1;
            if iteration > max_iterations {
                anyhow::bail!("Max tool iterations exceeded ({})", max_iterations);
            }

            let request = ChatRequest {
                messages: messages.clone(),
                tools: if tools.is_empty() {
                    None
                } else {
                    Some(tools.clone())
                },
                temperature: Some(0.7),
                max_tokens: Some(4096),
                model: None,
            };

            let response = self.chat(request, None).await?;

            // Add assistant message to history
            let assistant_content = response.content.clone().unwrap_or_default();
            if !assistant_content.is_empty() {
                last_assistant_response = assistant_content.clone();
            }

            messages.push(provider::ChatMessage {
                role: "assistant".to_string(),
                content: response.content.clone(),
                content_parts: None,
                tool_calls: response.tool_calls.clone(),
                tool_call_id: None,
            });

            // If no tool calls, we're done
            let tool_calls = match response.tool_calls {
                Some(calls) if !calls.is_empty() => calls,
                _ => break,
            };

            // Execute each tool call
            for tool_call in &tool_calls {
                let args: serde_json::Value =
                    serde_json::from_str(&tool_call.function.arguments).unwrap_or_default();

                tracing::info!(tool = %tool_call.function.name, "Executing tool call");

                // Handle agent control signals specially
                let result_str = match tool_call.function.name.as_str() {
                    "agent_complete" => {
                        agent_signal = AgentSignalFromTool::Done;
                        crate::tools::agent_control::run(
                            db,
                            user_id,
                            crate::tools::agent_control::AgentControlSignal::Complete,
                        )
                        .await
                        .unwrap_or_else(|e| format!("Error: {}", e))
                    }
                    "agent_next" => {
                        agent_signal = AgentSignalFromTool::Next;
                        crate::tools::agent_control::run(
                            db,
                            user_id,
                            crate::tools::agent_control::AgentControlSignal::Next,
                        )
                        .await
                        .unwrap_or_else(|e| format!("Error: {}", e))
                    }
                    "agent_set_path" => {
                        let path = args
                            .get("path")
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .to_string();
                        agent_signal = AgentSignalFromTool::Path(path.clone());
                        crate::tools::agent_control::run(
                            db,
                            user_id,
                            crate::tools::agent_control::AgentControlSignal::Path(path),
                        )
                        .await
                        .unwrap_or_else(|e| format!("Error: {}", e))
                    }
                    "agent_feedback" => {
                        let msg = args
                            .get("message")
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .to_string();
                        if !msg.is_empty() {
                            if let Some(ref tx) = feedback_tx {
                                let _ = tx.send(msg.clone());
                            }
                            feedback_messages.push(msg);
                        }
                        "Feedback sent successfully.".to_string()
                    }
                    "execute_terminal" => {
                        let command = args["command"].as_str().unwrap_or("");
                        match crate::tools::execute_terminal::execute_terminal(command, None).await
                        {
                            Ok(result) => {
                                if result.exit_code == 0 {
                                    if result.stdout.is_empty() {
                                        "Command executed (no output)".to_string()
                                    } else {
                                        result.stdout
                                    }
                                } else {
                                    format!(
                                        "Exit code: {}\nStdout: {}\nStderr: {}",
                                        result.exit_code, result.stdout, result.stderr
                                    )
                                }
                            }
                            Err(e) => format!("Error: {}", e),
                        }
                    }
                    "write_file" => {
                        let path = args["path"].as_str().unwrap_or("");
                        let content = args["content"].as_str().unwrap_or("");
                        match crate::tools::write_file::write_file(path, content).await {
                            Ok(_) => format!("File written: {}", path),
                            Err(e) => format!("Error: {}", e),
                        }
                    }
                    "edit_file" => {
                        let path = args["path"].as_str().unwrap_or("");
                        let old_text = args["old_text"].as_str().unwrap_or("");
                        let new_text = args["new_text"].as_str().unwrap_or("");
                        match crate::tools::edit_file::edit_file(path, old_text, new_text).await {
                            Ok(_) => format!("File edited: {}", path),
                            Err(e) => format!("Error: {}", e),
                        }
                    }
                    "read_file" => {
                        let path = args["path"].as_str().unwrap_or("");
                        match std::fs::read_to_string(path) {
                            Ok(content) => {
                                if content.len() > 10000 {
                                    format!(
                                        "{}...\n[Truncated - {} bytes]",
                                        &content[..10000],
                                        content.len()
                                    )
                                } else {
                                    content
                                }
                            }
                            Err(e) => format!("Error: {}", e),
                        }
                    }
                    "learn_fact" => {
                        let fact = args["fact"].as_str().unwrap_or("");
                        let mut memory = crate::db::memory::load_memory(db, user_id);
                        crate::db::memory::add_learned_fact(&mut memory, fact);
                        let _ = crate::db::memory::save_memory(db, user_id, &memory);
                        format!("Learned: {}", fact)
                    }
                    "learn_preference" => {
                        let key = args["key"].as_str().unwrap_or("");
                        let value = args.get("value").cloned().unwrap_or(serde_json::json!(""));
                        let mut memory = crate::db::memory::load_memory(db, user_id);
                        crate::db::memory::update_preference(&mut memory, key, &value);
                        let _ = crate::db::memory::save_memory(db, user_id, &memory);
                        format!("Preference saved: {}", key)
                    }
                    "learn_topic" => {
                        let topic = args["topic"].as_str().unwrap_or("");
                        let mut memory = crate::db::memory::load_memory(db, user_id);
                        crate::db::memory::add_topic(&mut memory, topic);
                        let _ = crate::db::memory::save_memory(db, user_id, &memory);
                        format!("Topic tracked: {}", topic)
                    }
                    other => format!("Unknown tool: {}", other),
                };

                // Check if this was an image tool that returned content_parts
                let image_result = if tool_call.function.name == "understand_image" {
                    Some(crate::tools::understand_image::run(&args).await)
                } else {
                    None
                };

                let (final_result_str, content_parts) = if let Some(img) = image_result {
                    (img.text, Some(img.content_parts))
                } else {
                    (result_str.clone(), None)
                };

                tool_call_records.push(ToolCallRecord {
                    name: tool_call.function.name.clone(),
                    arguments: tool_call.function.arguments.clone(),
                    result: final_result_str.clone(),
                });

                // Add tool result to messages
                messages.push(provider::ChatMessage {
                    role: "tool".to_string(),
                    content: Some(final_result_str),
                    content_parts,
                    tool_calls: None,
                    tool_call_id: Some(tool_call.id.clone()),
                });
            }

            if agent_signal == AgentSignalFromTool::Done {
                break;
            }
        }

        Ok(ChatWithToolsResult {
            response: last_assistant_response,
            tool_calls: tool_call_records,
            feedback_messages,
            agent_signal,
        })
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum AgentSignalFromTool {
    None,
    Done,
    Next,
    Path(String),
}

#[derive(Debug)]
pub struct ToolCallRecord {
    pub name: String,
    pub arguments: String,
    pub result: String,
}

#[derive(Debug)]
pub struct ChatWithToolsResult {
    pub response: String,
    pub tool_calls: Vec<ToolCallRecord>,
    pub feedback_messages: Vec<String>,
    pub agent_signal: AgentSignalFromTool,
}

#[cfg(test)]
mod gateway_tests {
    use super::*;

    #[test]
    fn test_llm_router_creation() {
        let config = crate::config::Config::from_env();
        let secrets = crate::db::secrets::Secrets::default();
        let _router = LLMRouter::new(&config, &secrets);
    }
}

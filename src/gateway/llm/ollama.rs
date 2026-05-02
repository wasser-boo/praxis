use super::provider::*;
use async_trait::async_trait;

pub struct OllamaProvider {
    base_url: String,
    model: String,
    client: reqwest::Client,
}

impl OllamaProvider {
    pub fn new(base_url: String, model: String) -> Self {
        Self {
            base_url,
            model,
            client: reqwest::Client::new(),
        }
    }
}

#[async_trait]
impl LLMProvider for OllamaProvider {
    async fn chat(&self, request: ChatRequest) -> anyhow::Result<ChatResponse> {
        let url = format!("{}/api/chat", self.base_url);

        let messages: Vec<serde_json::Value> = request
            .messages
            .iter()
            .map(|m| {
                let mut msg = serde_json::json!({
                    "role": m.role,
                    "content": m.content.as_deref().unwrap_or("")
                });
                // Include tool calls in assistant messages
                if let Some(ref tool_calls) = m.tool_calls {
                    let ollama_calls: Vec<serde_json::Value> = tool_calls.iter().map(|tc| {
                        serde_json::json!({
                            "function": {
                                "name": tc.function.name,
                                "arguments": serde_json::from_str::<serde_json::Value>(&tc.function.arguments).unwrap_or_default()
                            }
                        })
                    }).collect();
                    msg["tool_calls"] = serde_json::json!(ollama_calls);
                }
                msg
            })
            .collect();

        let model = request.model.as_deref().unwrap_or(&self.model);
        let mut body = serde_json::json!({
            "model": model,
            "messages": messages,
            "stream": false,
        });

        // Add tools if provided
        if let Some(ref tools) = request.tools {
            let ollama_tools: Vec<serde_json::Value> = tools
                .iter()
                .map(|t| {
                    serde_json::json!({
                        "type": "function",
                        "function": {
                            "name": t.function.name,
                            "description": t.function.description,
                            "parameters": t.function.parameters
                        }
                    })
                })
                .collect();
            body["tools"] = serde_json::json!(ollama_tools);
        }

        let resp = self.client.post(&url).json(&body).send().await?;

        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            anyhow::bail!("Ollama API error {}: {}", status, text);
        }

        let data: serde_json::Value = resp.json().await?;
        let message = &data["message"];

        let content = message["content"].as_str().map(|s| s.to_string());

        // Parse tool calls from Ollama response
        let tool_calls = message["tool_calls"].as_array().map(|calls| {
            calls
                .iter()
                .filter_map(|tc| {
                    let func = tc.get("function")?;
                    let name = func.get("name")?.as_str()?.to_string();
                    let args = func.get("arguments")?.clone();
                    Some(ToolCall {
                        id: format!("call_{}", uuid::Uuid::new_v4()),
                        function: FunctionCall {
                            name,
                            arguments: args.to_string(),
                        },
                    })
                })
                .collect()
        });

        Ok(ChatResponse {
            content,
            tool_calls,
            finish_reason: Some("stop".to_string()),
            usage: None,
        })
    }

    fn name(&self) -> &str {
        "ollama"
    }

    async fn health_check(&self) -> bool {
        let url = format!("{}/api/tags", self.base_url);
        self.client.get(&url).send().await.is_ok()
    }
}

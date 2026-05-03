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
                // Handle content_parts (images etc.)
                if let Some(ref parts) = m.content_parts {
                    let mut images: Vec<String> = Vec::new();
                    for p in parts {
                        if let super::provider::ContentPart::ImageUrl { image_url } = p {
                            // Handle data: URLs (base64 encoded)
                            if let Some(b64) = image_url.url.strip_prefix("data:").and_then(|s| s.find(",").map(|i| &s[i+1..])) {
                                images.push(b64.to_string());
                            }
                            // Handle file:// URLs - read and encode as base64
                            else if let Some(path) = image_url.url.strip_prefix("file://") {
                                if let Ok(bytes) = std::fs::read(path) {
                                    use base64::Engine;
                                    let b64 = base64::engine::general_purpose::STANDARD.encode(&bytes);
                                    images.push(b64);
                                }
                            }
                            // Handle raw file paths (no scheme)
                            else if !image_url.url.starts_with("http") {
                                if let Ok(bytes) = std::fs::read(&image_url.url) {
                                    use base64::Engine;
                                    let b64 = base64::engine::general_purpose::STANDARD.encode(&bytes);
                                    images.push(b64);
                                }
                            }
                        }
                    }
                    if !images.is_empty() {
                        msg["images"] = serde_json::json!(images);
                    }
                }
                // Include tool calls in assistant messages
                if let Some(ref tool_calls) = m.tool_calls {
                    let ollama_calls: Vec<serde_json::Value> = tool_calls.iter().map(|tc| {
                        serde_json::json!({
                            "id": tc.id,
                            "type": "function",
                            "function": {
                                "name": tc.function.name,
                                "arguments": serde_json::from_str::<serde_json::Value>(&tc.function.arguments).unwrap_or_default()
                            }
                        })
                    }).collect();
                    msg["tool_calls"] = serde_json::json!(ollama_calls);
                }
                // For tool result messages, use tool_name (Ollama expects this)
                if m.role == "tool" {
                    if let Some(ref tool_name) = m.tool_name {
                        msg["tool_name"] = serde_json::json!(tool_name);
                    }
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

        tracing::debug!(target: "ollama", "Request body: {}", serde_json::to_string_pretty(&body).unwrap_or_default());

        let resp = self.client.post(&url).json(&body).send().await?;

        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            anyhow::bail!("Ollama API error {}: {}", status, text);
        }

        let data: serde_json::Value = resp.json().await?;
        tracing::debug!(target: "ollama", "Response: {}", serde_json::to_string_pretty(&data).unwrap_or_default());
        let message = &data["message"];

        let content = message["content"].as_str().map(|s| s.to_string());

        // Parse tool calls from Ollama response
        let tool_calls = message["tool_calls"].as_array().map(|calls| {
            tracing::debug!(target: "ollama", "Found {} tool calls in response", calls.len());
            calls
                .iter()
                .filter_map(|tc| {
                    let func = tc.get("function")?;
                    let name = func.get("name")?.as_str()?.to_string();
                    let args = func.get("arguments")?.clone();
                    tracing::debug!(target: "ollama", "Tool call: {} with args: {}", name, args);
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

        if tool_calls.is_none() {
            tracing::debug!(target: "ollama", "No tool calls found in response. Content: {:?}", content);
        }

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

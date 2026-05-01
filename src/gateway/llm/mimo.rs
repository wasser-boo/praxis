use super::provider::*;
use crate::config::ApiMode;
use async_trait::async_trait;

pub struct MiMoProvider {
    api_key: String,
    model: String,
    base_url: String,
    api_mode: ApiMode,
    client: reqwest::Client,
}

impl MiMoProvider {
    pub fn new(api_key: String, model: String, base_url: String, api_mode: ApiMode) -> Self {
        Self {
            api_key,
            model,
            base_url,
            api_mode,
            client: reqwest::Client::new(),
        }
    }
}

#[async_trait]
impl LLMProvider for MiMoProvider {
    async fn chat(&self, request: ChatRequest) -> anyhow::Result<ChatResponse> {
        match self.api_mode {
            ApiMode::Anthropic => self.chat_anthropic(request).await,
            ApiMode::OpenAI => self.chat_openai(request).await,
        }
    }

    fn name(&self) -> &str {
        "mimo"
    }
}

impl MiMoProvider {
    async fn chat_openai(&self, request: ChatRequest) -> anyhow::Result<ChatResponse> {
        let url = format!("{}/chat/completions", self.base_url);

        let messages: Vec<serde_json::Value> = request
            .messages
            .iter()
            .map(|m| {
                let mut msg = serde_json::json!({
                    "role": m.role,
                    "content": m.content.as_deref().unwrap_or("")
                });
                if let Some(ref tool_calls) = m.tool_calls {
                    let calls: Vec<serde_json::Value> = tool_calls.iter().map(|tc| {
                    serde_json::json!({
                        "id": tc.id,
                        "type": "function",
                        "function": { "name": tc.function.name, "arguments": tc.function.arguments }
                    })
                }).collect();
                    msg["tool_calls"] = serde_json::json!(calls);
                }
                if let Some(ref tcid) = m.tool_call_id {
                    msg["tool_call_id"] = serde_json::json!(tcid);
                }
                msg
            })
            .collect();

        let mut body = serde_json::json!({
            "model": self.model,
            "messages": messages,
        });

        if let Some(ref tools) = request.tools {
            body["tools"] = serde_json::json!(tools);
        }

        let resp = self
            .client
            .post(&url)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .json(&body)
            .send()
            .await?;

        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            anyhow::bail!("MiMo API error {}: {}", status, text);
        }

        let data: serde_json::Value = resp.json().await?;
        let choice = &data["choices"][0];
        let message = &choice["message"];

        let content = message["content"].as_str().map(|s| s.to_string());
        let tool_calls = parse_openai_tool_calls(message);

        Ok(ChatResponse {
            content,
            tool_calls,
            finish_reason: choice["finish_reason"].as_str().map(|s| s.to_string()),
            usage: data["usage"].as_object().map(|u| Usage {
                prompt_tokens: u["prompt_tokens"].as_u64().unwrap_or(0) as u32,
                completion_tokens: u["completion_tokens"].as_u64().unwrap_or(0) as u32,
                total_tokens: u["total_tokens"].as_u64().unwrap_or(0) as u32,
            }),
        })
    }

    async fn chat_anthropic(&self, request: ChatRequest) -> anyhow::Result<ChatResponse> {
        let url = format!("{}/v1/messages", self.base_url);

        let (system_prompt, messages) =
            super::anthropic::build_anthropic_messages(&request.messages);

        let mut body = serde_json::json!({
            "model": self.model,
            "messages": messages,
            "max_tokens": request.max_tokens.unwrap_or(4096),
        });

        if !system_prompt.is_empty() {
            body["system"] = serde_json::json!(system_prompt);
        }

        if let Some(ref tools) = request.tools {
            body["tools"] = serde_json::json!(super::anthropic::build_anthropic_tools(tools));
        }

        let resp = self
            .client
            .post(&url)
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", "2023-06-01")
            .json(&body)
            .send()
            .await?;

        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            anyhow::bail!("MiMo Anthropic API error {}: {}", status, text);
        }

        let data: serde_json::Value = resp.json().await?;
        super::anthropic::parse_anthropic_response(&data)
    }
}

fn parse_openai_tool_calls(message: &serde_json::Value) -> Option<Vec<ToolCall>> {
    message["tool_calls"].as_array().map(|calls| {
        calls
            .iter()
            .filter_map(|tc| {
                Some(ToolCall {
                    id: tc["id"].as_str().unwrap_or("call_0").to_string(),
                    function: FunctionCall {
                        name: tc["function"]["name"].as_str()?.to_string(),
                        arguments: tc["function"]["arguments"]
                            .as_str()
                            .unwrap_or("{}")
                            .to_string(),
                    },
                })
            })
            .collect()
    })
}

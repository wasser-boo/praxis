use async_trait::async_trait;
use super::provider::*;

pub struct AnthropicProvider {
    api_key: String,
    model: String,
    base_url: String,
    client: reqwest::Client,
}

impl AnthropicProvider {
    pub fn new(api_key: String, model: String, base_url: String) -> Self {
        Self {
            api_key,
            model,
            base_url,
            client: reqwest::Client::new(),
        }
    }
}

#[async_trait]
impl LLMProvider for AnthropicProvider {
    async fn chat(&self, request: ChatRequest) -> anyhow::Result<ChatResponse> {
        let url = format!("{}/v1/messages", self.base_url);

        let messages: Vec<serde_json::Value> = request
            .messages
            .iter()
            .map(|m| {
                serde_json::json!({
                    "role": m.role,
                    "content": m.content.as_deref().unwrap_or("")
                })
            })
            .collect();

        let body = serde_json::json!({
            "model": self.model,
            "messages": messages,
            "max_tokens": request.max_tokens.unwrap_or(4096),
        });

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
            anyhow::bail!("Anthropic API error {}: {}", status, text);
        }

        let data: serde_json::Value = resp.json().await?;
        let content = data["content"][0]["text"]
            .as_str()
            .map(|s| s.to_string());

        Ok(ChatResponse {
            content,
            tool_calls: None,
            finish_reason: data["stop_reason"].as_str().map(|s| s.to_string()),
            usage: Some(Usage {
                prompt_tokens: data["usage"]["input_tokens"].as_u64().unwrap_or(0) as u32,
                completion_tokens: data["usage"]["output_tokens"].as_u64().unwrap_or(0) as u32,
                total_tokens: 0,
            }),
        })
    }

    fn name(&self) -> &str {
        "anthropic"
    }
}

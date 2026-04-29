use async_trait::async_trait;
use super::provider::*;

pub struct MiMoProvider {
    api_key: String,
    client: reqwest::Client,
}

impl MiMoProvider {
    pub fn new(api_key: String) -> Self {
        Self {
            api_key,
            client: reqwest::Client::new(),
        }
    }
}

#[async_trait]
impl LLMProvider for MiMoProvider {
    async fn chat(&self, request: ChatRequest) -> anyhow::Result<ChatResponse> {
        let url = "https://api.mimo.com/v1/chat/completions";

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
            "model": "mimo",
            "messages": messages,
        });

        let resp = self
            .client
            .post(url)
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
        let content = data["choices"][0]["message"]["content"]
            .as_str()
            .map(|s| s.to_string());

        Ok(ChatResponse {
            content,
            tool_calls: None,
            finish_reason: Some("stop".to_string()),
            usage: None,
        })
    }

    fn name(&self) -> &str {
        "mimo"
    }
}

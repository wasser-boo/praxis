use async_trait::async_trait;
use super::provider::*;

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
                serde_json::json!({
                    "role": m.role,
                    "content": m.content.as_deref().unwrap_or("")
                })
            })
            .collect();

        let body = serde_json::json!({
            "model": self.model,
            "messages": messages,
            "stream": false,
        });

        let resp = self.client.post(&url).json(&body).send().await?;

        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            anyhow::bail!("Ollama API error {}: {}", status, text);
        }

        let data: serde_json::Value = resp.json().await?;
        let content = data["message"]["content"]
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
        "ollama"
    }

    async fn health_check(&self) -> bool {
        let url = format!("{}/api/tags", self.base_url);
        self.client.get(&url).send().await.is_ok()
    }
}

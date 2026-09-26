//! OpenAI Codex (ChatGPT subscription) provider.
//!
//! Uses the OAuth tokens the `codex` CLI stores in `~/.codex/auth.json`
//! (`codex login` / `codex login --device-auth`) against the Codex Responses
//! endpoint. Praxis never runs the browser flow itself; `/login codex` imports
//! the CLI's tokens (or launches the CLI's device-auth on the gateway machine)
//! and keeps them in the encrypted secret store under `custom.codex_auth`.
//! Access tokens are refreshed here when they expire.
use super::provider::*;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::sync::Mutex;

pub const CODEX_RESPONSES_URL: &str = "https://chatgpt.com/backend-api/codex/responses";
pub const CODEX_TOKEN_URL: &str = "https://auth.openai.com/oauth/token";
/// Public client id of the Codex CLI; the refresh grant needs no secret.
pub const CODEX_CLIENT_ID: &str = "app_EMoamEEZ73f0CkXaXp7hrann";
pub const DEFAULT_MODEL: &str = "gpt-5-codex";
/// Secret-store key (in `Secrets::custom`) holding the JSON `CodexAuth`.
pub const SECRET_KEY: &str = "codex_auth";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct CodexAuth {
    pub access_token: String,
    pub refresh_token: String,
    #[serde(default)]
    pub id_token: Option<String>,
    #[serde(default)]
    pub account_id: Option<String>,
    /// RFC3339 time the access token was obtained (refresh after ~8 days
    /// or on 401).
    #[serde(default)]
    pub last_refresh: Option<String>,
}

impl CodexAuth {
    /// Parse the Codex CLI's `auth.json`.
    pub fn from_cli_file(text: &str) -> anyhow::Result<Self> {
        let value: serde_json::Value = serde_json::from_str(text)?;
        let tokens = value.get("tokens").ok_or_else(|| anyhow::anyhow!("auth.json has no ChatGPT tokens (API-key mode?)"))?;
        let access_token = tokens["access_token"].as_str().filter(|s| !s.is_empty()).ok_or_else(|| anyhow::anyhow!("auth.json has no access_token"))?.to_string();
        let refresh_token = tokens["refresh_token"].as_str().unwrap_or_default().to_string();
        let mut auth = Self {
            account_id: tokens["account_id"].as_str().map(str::to_string),
            id_token: tokens["id_token"].as_str().map(str::to_string),
            last_refresh: value["last_refresh"].as_str().map(str::to_string),
            access_token,
            refresh_token,
        };
        if auth.account_id.is_none() {
            auth.account_id = account_id_from_jwt(&auth.access_token);
        }
        Ok(auth)
    }

    pub fn from_secrets(secrets: &crate::db::secrets::Secrets) -> Option<Self> {
        secrets.custom.get(SECRET_KEY).and_then(|s| serde_json::from_str(s).ok()).filter(|a: &Self| !a.access_token.is_empty())
    }

    pub fn store(&self, secrets: &mut crate::db::secrets::Secrets) {
        secrets.custom.insert(SECRET_KEY.into(), serde_json::to_string(self).unwrap_or_default());
    }

    /// Location of the Codex CLI token file on this machine.
    pub fn cli_auth_path() -> Option<std::path::PathBuf> {
        let home = std::env::var_os("CODEX_HOME").map(std::path::PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|h| std::path::PathBuf::from(h).join(".codex")))?;
        Some(home.join("auth.json"))
    }

    /// Masked identity for status output; never the token.
    pub fn describe(&self) -> String {
        let email = self.id_token.as_deref().and_then(jwt_claims).and_then(|c| c["email"].as_str().map(str::to_string));
        match (email, &self.account_id) {
            (Some(email), _) => format!("ChatGPT account {email}"),
            (None, Some(account)) => format!("ChatGPT account …{}", account.chars().rev().take(6).collect::<String>().chars().rev().collect::<String>()),
            (None, None) => "ChatGPT account".into(),
        }
    }
}

fn jwt_claims(token: &str) -> Option<serde_json::Value> {
    use base64::Engine;
    let payload = token.split('.').nth(1)?;
    let bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD.decode(payload).ok()?;
    serde_json::from_slice(&bytes).ok()
}

pub fn account_id_from_jwt(token: &str) -> Option<String> {
    let claims = jwt_claims(token)?;
    claims["https://api.openai.com/auth"]["chatgpt_account_id"].as_str().map(str::to_string)
}

fn jwt_expired(token: &str) -> bool {
    jwt_claims(token)
        .and_then(|c| c["exp"].as_i64())
        .is_some_and(|exp| exp - 60 <= chrono::Utc::now().timestamp())
}

/// Called after a refresh so the new tokens outlive this router instance.
pub type OnRefresh = Box<dyn Fn(&CodexAuth) + Send + Sync>;

pub struct CodexProvider {
    auth: Mutex<CodexAuth>,
    model: String,
    base_url: String,
    token_url: String,
    client: reqwest::Client,
    on_refresh: Option<OnRefresh>,
}

impl CodexProvider {
    pub fn new(auth: CodexAuth, model: String, on_refresh: Option<OnRefresh>) -> Self {
        Self::with_urls(auth, model, CODEX_RESPONSES_URL.into(), CODEX_TOKEN_URL.into(), on_refresh)
    }

    pub fn with_urls(auth: CodexAuth, model: String, base_url: String, token_url: String, on_refresh: Option<OnRefresh>) -> Self {
        Self { auth: Mutex::new(auth), model, base_url, token_url, client: super::http::client(), on_refresh }
    }

    fn auth(&self) -> CodexAuth {
        self.auth.lock().map(|a| a.clone()).unwrap_or_else(|e| e.into_inner().clone())
    }

    async fn refresh(&self) -> anyhow::Result<CodexAuth> {
        let current = self.auth();
        anyhow::ensure!(!current.refresh_token.is_empty(), "Codex login expired and no refresh token is stored; run /login codex again");
        let response = self.client.post(&self.token_url)
            .json(&serde_json::json!({
                "client_id": CODEX_CLIENT_ID,
                "grant_type": "refresh_token",
                "refresh_token": current.refresh_token,
                "scope": "openid profile email",
            }))
            .send().await.map_err(super::error::ProviderError::from_reqwest)?;
        let data = super::http::json(response).await?;
        let access_token = data["access_token"].as_str().filter(|s| !s.is_empty())
            .ok_or_else(|| anyhow::anyhow!("Codex token refresh returned no access token; run /login codex again"))?;
        let mut next = CodexAuth {
            access_token: access_token.to_string(),
            refresh_token: data["refresh_token"].as_str().filter(|s| !s.is_empty()).unwrap_or(&current.refresh_token).to_string(),
            id_token: data["id_token"].as_str().map(str::to_string).or(current.id_token),
            account_id: current.account_id,
            last_refresh: Some(chrono::Utc::now().to_rfc3339()),
        };
        if next.account_id.is_none() {
            next.account_id = account_id_from_jwt(&next.access_token);
        }
        if let Ok(mut guard) = self.auth.lock() {
            *guard = next.clone();
        }
        if let Some(hook) = &self.on_refresh {
            hook(&next);
        }
        Ok(next)
    }

    fn build_body(&self, request: &ChatRequest) -> serde_json::Value {
        let model = request.model.as_deref().unwrap_or(&self.model);
        let mut instructions = Vec::new();
        let mut input = Vec::new();
        for m in &request.messages {
            match m.role.as_str() {
                "system" if input.is_empty() => {
                    if let Some(text) = m.content.as_deref().filter(|t| !t.is_empty()) {
                        instructions.push(text.to_string());
                    }
                }
                "system" => input.push(serde_json::json!({"type": "message", "role": "developer",
                    "content": [{"type": "input_text", "text": m.content.as_deref().unwrap_or("")}]})),
                "tool" => {
                    input.push(serde_json::json!({"type": "function_call_output",
                        "call_id": m.tool_call_id.as_deref().unwrap_or(""),
                        "output": m.content.as_deref().unwrap_or("")}));
                    if let Some(parts) = &m.content_parts {
                        let images: Vec<_> = parts.iter().filter_map(|p| match p {
                            ContentPart::ImageUrl { image_url } => Some(serde_json::json!({"type": "input_image", "image_url": image_url.url, "detail": image_url.detail.as_deref().unwrap_or("auto")})),
                            _ => None,
                        }).collect();
                        if !images.is_empty() {
                            input.push(serde_json::json!({"type": "message", "role": "user", "content": images}));
                        }
                    }
                }
                "assistant" => {
                    if let Some(text) = m.content.as_deref().filter(|t| !t.is_empty()) {
                        input.push(serde_json::json!({"type": "message", "role": "assistant",
                            "content": [{"type": "output_text", "text": text}]}));
                    }
                    for call in m.tool_calls.iter().flatten() {
                        input.push(serde_json::json!({"type": "function_call", "call_id": call.id,
                            "name": call.function.name, "arguments": call.function.arguments}));
                    }
                }
                _ => {
                    let mut content = Vec::new();
                    if let Some(text) = m.content.as_deref().filter(|t| !t.is_empty()) {
                        content.push(serde_json::json!({"type": "input_text", "text": text}));
                    }
                    for part in m.content_parts.iter().flatten() {
                        match part {
                            ContentPart::Text { text } => content.push(serde_json::json!({"type": "input_text", "text": text})),
                            ContentPart::ImageUrl { image_url } => content.push(serde_json::json!({"type": "input_image", "image_url": image_url.url, "detail": image_url.detail.as_deref().unwrap_or("auto")})),
                        }
                    }
                    if content.is_empty() {
                        content.push(serde_json::json!({"type": "input_text", "text": ""}));
                    }
                    input.push(serde_json::json!({"type": "message", "role": "user", "content": content}));
                }
            }
        }
        let tools: Vec<_> = request.tools.iter().flatten().map(|t| serde_json::json!({
            "type": "function", "name": t.function.name, "description": t.function.description,
            "parameters": t.function.parameters, "strict": false,
        })).collect();
        let mut body = serde_json::json!({
            "model": model,
            "instructions": instructions.join("\n\n"),
            "input": input,
            "tools": tools,
            "tool_choice": "auto",
            "parallel_tool_calls": false,
            "store": false,
            "stream": true,
            "include": [],
        });
        if let Some(level) = request.thinking.and_then(|t| t.level()) {
            body["reasoning"] = serde_json::json!({"effort": level, "summary": "auto"});
        }
        body
    }

    async fn send(&self, body: &serde_json::Value, on_delta: &(dyn Fn(StreamDelta) + Send + Sync)) -> anyhow::Result<ChatResponse> {
        let mut auth = self.auth();
        if jwt_expired(&auth.access_token) {
            auth = self.refresh().await?;
        }
        let mut response = self.request(&auth, body).await?;
        if response.status().as_u16() == 401 {
            auth = self.refresh().await?;
            response = self.request(&auth, body).await?;
        }
        let response = super::http::checked(response).await?;
        parse_sse(response, on_delta).await
    }

    async fn request(&self, auth: &CodexAuth, body: &serde_json::Value) -> anyhow::Result<reqwest::Response> {
        let mut request = self.client.post(&self.base_url)
            .bearer_auth(&auth.access_token)
            .header("OpenAI-Beta", "responses=experimental")
            .header("originator", "praxis")
            .header("session_id", uuid::Uuid::new_v4().to_string())
            .header("accept", "text/event-stream")
            .json(body);
        if let Some(account) = &auth.account_id {
            request = request.header("chatgpt-account-id", account);
        }
        Ok(request.send().await.map_err(super::error::ProviderError::from_reqwest)?)
    }
}

/// Consume the Responses SSE stream. Text deltas are forwarded as they arrive;
/// the final `response.completed` event is authoritative for tool calls.
async fn parse_sse(mut response: reqwest::Response, on_delta: &(dyn Fn(StreamDelta) + Send + Sync)) -> anyhow::Result<ChatResponse> {
    let mut buffer = String::new();
    let mut text = String::new();
    let mut reasoning = String::new();
    let mut completed: Option<serde_json::Value> = None;
    let mut items: Vec<serde_json::Value> = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(super::error::ProviderError::from_reqwest)? {
        buffer.push_str(&String::from_utf8_lossy(&chunk));
        while let Some(pos) = buffer.find("\n\n") {
            let event = buffer[..pos].to_string();
            buffer.drain(..pos + 2);
            let data: String = event.lines().filter_map(|l| l.strip_prefix("data:")).map(str::trim).collect::<Vec<_>>().join("\n");
            if data.is_empty() || data == "[DONE]" { continue; }
            let Ok(value) = serde_json::from_str::<serde_json::Value>(&data) else { continue };
            match value["type"].as_str().unwrap_or("") {
                "response.output_text.delta" => {
                    if let Some(delta) = value["delta"].as_str() {
                        text.push_str(delta);
                        on_delta(StreamDelta::Text { text: delta.to_string() });
                    }
                }
                "response.reasoning_summary_text.delta" | "response.reasoning_text.delta" => {
                    if let Some(delta) = value["delta"].as_str() {
                        reasoning.push_str(delta);
                        on_delta(StreamDelta::Reasoning { text: delta.to_string() });
                    }
                }
                "response.output_item.done" => items.push(value["item"].clone()),
                "response.completed" => completed = Some(value["response"].clone()),
                "response.failed" | "error" => {
                    let message = value.pointer("/response/error/message").or_else(|| value.pointer("/error/message")).or_else(|| value.get("message"))
                        .and_then(|m| m.as_str()).unwrap_or("Codex request failed");
                    anyhow::bail!("Codex: {}", message.chars().take(300).collect::<String>());
                }
                _ => {}
            }
        }
    }
    let output = completed.as_ref().and_then(|r| r["output"].as_array().cloned()).unwrap_or(items);
    let mut content = String::new();
    let mut tool_calls = Vec::new();
    for item in &output {
        match item["type"].as_str().unwrap_or("") {
            "message" => for part in item["content"].as_array().into_iter().flatten() {
                if let Some(t) = part["text"].as_str() { content.push_str(t); }
            },
            "function_call" => tool_calls.push(ToolCall {
                id: item["call_id"].as_str().or(item["id"].as_str()).unwrap_or_default().to_string(),
                function: FunctionCall {
                    name: item["name"].as_str().unwrap_or_default().to_string(),
                    arguments: item["arguments"].as_str().unwrap_or("{}").to_string(),
                },
            }),
            _ => {}
        }
    }
    if content.is_empty() { content = text; }
    let usage = completed.as_ref().and_then(|r| r.get("usage")).map(|u| Usage {
        prompt_tokens: u["input_tokens"].as_u64().unwrap_or(0) as u32,
        completion_tokens: u["output_tokens"].as_u64().unwrap_or(0) as u32,
        total_tokens: u["total_tokens"].as_u64().unwrap_or(0) as u32,
    });
    Ok(ChatResponse {
        finish_reason: Some(if tool_calls.is_empty() { "stop" } else { "tool_calls" }.into()),
        content: if content.is_empty() { None } else { Some(content) },
        reasoning_content: if reasoning.is_empty() { None } else { Some(reasoning) },
        tool_calls: if tool_calls.is_empty() { None } else { Some(tool_calls) },
        usage,
    })
}

#[async_trait]
impl LLMProvider for CodexProvider {
    async fn chat(&self, request: ChatRequest) -> anyhow::Result<ChatResponse> {
        self.send(&self.build_body(&request), &|_| {}).await
    }

    async fn chat_stream_events(&self, request: ChatRequest, on_delta: &(dyn Fn(StreamDelta) + Send + Sync)) -> anyhow::Result<ChatResponse> {
        self.send(&self.build_body(&request), on_delta).await
    }

    fn name(&self) -> &str { "codex" }
    fn as_any(&self) -> &dyn std::any::Any { self }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::{matchers::{header, method, path}, Mock, MockServer, ResponseTemplate};

    fn jwt(claims: serde_json::Value) -> String {
        use base64::Engine;
        let enc = |v: &serde_json::Value| base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(v.to_string());
        format!("{}.{}.sig", enc(&serde_json::json!({"alg": "none"})), enc(&claims))
    }

    #[test]
    fn imports_codex_cli_auth_file_and_account_id() {
        let token = jwt(serde_json::json!({"exp": 4102444800i64, "https://api.openai.com/auth": {"chatgpt_account_id": "acct_123"}}));
        let file = serde_json::json!({"auth_mode": "chatgpt", "tokens": {"access_token": token, "refresh_token": "r1"}}).to_string();
        let auth = CodexAuth::from_cli_file(&file).unwrap();
        assert_eq!(auth.account_id.as_deref(), Some("acct_123"));
        assert!(!jwt_expired(&auth.access_token));
        assert!(CodexAuth::from_cli_file(r#"{"OPENAI_API_KEY":"sk"}"#).is_err());
        let mut secrets = crate::db::secrets::Secrets::default();
        auth.store(&mut secrets);
        assert_eq!(CodexAuth::from_secrets(&secrets), Some(auth));
    }

    #[tokio::test]
    async fn refreshes_expired_token_and_parses_responses_stream() {
        let server = MockServer::start().await;
        let fresh = jwt(serde_json::json!({"exp": 4102444800i64, "https://api.openai.com/auth": {"chatgpt_account_id": "acct_9"}}));
        Mock::given(method("POST")).and(path("/token"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"access_token": fresh, "refresh_token": "r2"})))
            .expect(1).mount(&server).await;
        let sse = concat!(
            "event: response.output_text.delta\ndata: {\"type\":\"response.output_text.delta\",\"delta\":\"Hel\"}\n\n",
            "data: {\"type\":\"response.output_text.delta\",\"delta\":\"lo\"}\n\n",
            "data: {\"type\":\"response.output_item.done\",\"item\":{\"type\":\"function_call\",\"call_id\":\"c1\",\"name\":\"read_file\",\"arguments\":\"{\\\"path\\\":\\\"a\\\"}\"}}\n\n",
            "data: {\"type\":\"response.completed\",\"response\":{\"output\":[{\"type\":\"message\",\"content\":[{\"type\":\"output_text\",\"text\":\"Hello\"}]},{\"type\":\"function_call\",\"call_id\":\"c1\",\"name\":\"read_file\",\"arguments\":\"{\\\"path\\\":\\\"a\\\"}\"}],\"usage\":{\"input_tokens\":5,\"output_tokens\":7,\"total_tokens\":12}}}\n\n",
        );
        Mock::given(method("POST")).and(path("/responses")).and(header("chatgpt-account-id", "acct_9")).and(header("Authorization", format!("Bearer {fresh}")))
            .respond_with(ResponseTemplate::new(200).insert_header("content-type", "text/event-stream").set_body_string(sse))
            .expect(1).mount(&server).await;
        let expired = jwt(serde_json::json!({"exp": 1i64}));
        let refreshed = std::sync::Arc::new(Mutex::new(None));
        let sink = refreshed.clone();
        let provider = CodexProvider::with_urls(
            CodexAuth { access_token: expired, refresh_token: "r1".into(), ..Default::default() },
            "gpt-5-codex".into(), format!("{}/responses", server.uri()), format!("{}/token", server.uri()),
            Some(Box::new(move |a: &CodexAuth| { *sink.lock().unwrap() = Some(a.clone()); })),
        );
        let request = ChatRequest {
            messages: vec![
                ChatMessage { role: "system".into(), content: Some("sys".into()), reasoning_content: None, content_parts: None, tool_calls: None, tool_call_id: None, tool_name: None },
                ChatMessage { role: "user".into(), content: Some("hi".into()), reasoning_content: None, content_parts: None, tool_calls: None, tool_call_id: None, tool_name: None },
            ],
            tools: None, temperature: None, max_tokens: None, model: None, vision_provider: None, vision_model: None, thinking: None,
        };
        let body = provider.build_body(&request);
        assert_eq!(body["instructions"], "sys");
        assert_eq!(body["input"][0]["content"][0]["text"], "hi");
        let streamed = std::sync::Arc::new(Mutex::new(String::new()));
        let s2 = streamed.clone();
        let response = provider.chat_stream_events(request, &move |d| if let StreamDelta::Text { text } = d { s2.lock().unwrap().push_str(&text) }).await.unwrap();
        assert_eq!(response.content.as_deref(), Some("Hello"));
        assert_eq!(*streamed.lock().unwrap(), "Hello");
        assert_eq!(response.tool_calls.unwrap()[0].function.name, "read_file");
        assert_eq!(response.usage.unwrap().total_tokens, 12);
        assert_eq!(refreshed.lock().unwrap().as_ref().unwrap().refresh_token, "r2");
        assert_eq!(refreshed.lock().unwrap().as_ref().unwrap().account_id.as_deref(), Some("acct_9"));
    }
}

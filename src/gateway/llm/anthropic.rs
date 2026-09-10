use super::provider::*;
use async_trait::async_trait;

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
            client: super::http::client(),
        }
    }
}

#[async_trait]
impl LLMProvider for AnthropicProvider {
    async fn chat(&self, request: ChatRequest) -> anyhow::Result<ChatResponse> {
        let url = format!("{}/v1/messages", self.base_url);

        let (system_prompt, messages) = build_anthropic_messages(&request.messages);

        let model = request.model.as_deref().unwrap_or(&self.model);
        let mut body = serde_json::json!({
            "model": model,
            "messages": messages,
            "max_tokens": request.max_tokens.unwrap_or(4096),
        });

        if !system_prompt.is_empty() {
            body["system"] = serde_json::json!(system_prompt);
        }

        if let Some(ref tools) = request.tools {
            body["tools"] = serde_json::json!(build_anthropic_tools(tools));
        }

        let resp = self
            .client
            .post(&url)
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", "2023-06-01")
            .json(&body)
            .send()
            .await.map_err(super::error::ProviderError::from_reqwest)?;

        let data = super::http::json(resp).await?;
        parse_anthropic_response(&data)
    }

    fn name(&self) -> &str {
        "anthropic"
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

pub fn build_anthropic_messages(messages: &[ChatMessage]) -> (String, Vec<serde_json::Value>) {
    let mut system_prompt = String::new();
    let mut out: Vec<serde_json::Value> = Vec::new();

    for m in messages {
        if m.role == "system" {
            system_prompt = m.content.clone().unwrap_or_default();
            continue;
        }
        if m.role == "tool" {
            let content_value = if let Some(ref parts) = m.content_parts {
                let mut arr: Vec<serde_json::Value> = Vec::new();
                if let Some(ref text) = m.content {
                    if !text.is_empty() {
                        arr.push(serde_json::json!({"type": "text", "text": text}));
                    }
                }
                for p in parts {
                    match p {
                        ContentPart::Text { text } => {
                            arr.push(serde_json::json!({"type": "text", "text": text}))
                        }
                        ContentPart::ImageUrl { image_url } => {
                            // Anthropic uses source blocks for images
                            if let Some(base64_data) = image_url
                                .url
                                .strip_prefix("data:")
                                .and_then(|s| s.find(",").map(|i| &s[i + 1..]))
                            {
                                let media_type = image_url
                                    .url
                                    .split(';')
                                    .next()
                                    .unwrap_or("image/png")
                                    .strip_prefix("data:")
                                    .unwrap_or("image/png");
                                arr.push(serde_json::json!({
                                    "type": "image",
                                    "source": {
                                        "type": "base64",
                                        "media_type": media_type,
                                        "data": base64_data
                                    }
                                }));
                            }
                        }
                    }
                }
                serde_json::json!(arr)
            } else {
                serde_json::json!(m.content.as_deref().unwrap_or(""))
            };
            let tool_result_content = serde_json::json!([{
                "type": "tool_result",
                "tool_use_id": m.tool_call_id.as_deref().unwrap_or(""),
                "content": content_value
            }]);
            out.push(serde_json::json!({ "role": "user", "content": tool_result_content }));
            continue;
        }
        if m.role == "assistant" {
            if let Some(ref tool_calls) = m.tool_calls {
                let mut blocks: Vec<serde_json::Value> = Vec::new();
                if let Some(ref text) = m.content {
                    if !text.is_empty() {
                        blocks.push(serde_json::json!({"type": "text", "text": text}));
                    }
                }
                for tc in tool_calls {
                    let args: serde_json::Value =
                        serde_json::from_str(&tc.function.arguments).unwrap_or_default();
                    blocks.push(serde_json::json!({
                        "type": "tool_use",
                        "id": tc.id,
                        "name": tc.function.name,
                        "input": args
                    }));
                }
                out.push(serde_json::json!({ "role": "assistant", "content": blocks }));
                continue;
            }
        }
        if let Some(ref parts) = m.content_parts {
            let mut arr: Vec<serde_json::Value> = Vec::new();
            if let Some(ref text) = m.content {
                if !text.is_empty() {
                    arr.push(serde_json::json!({"type": "text", "text": text}));
                }
            }
            for p in parts {
                match p {
                    ContentPart::Text { text } => {
                        arr.push(serde_json::json!({"type": "text", "text": text}))
                    }
                    ContentPart::ImageUrl { image_url } => {
                        if let Some(base64_data) = image_url
                            .url
                            .strip_prefix("data:")
                            .and_then(|s| s.find(",").map(|i| &s[i + 1..]))
                        {
                            let media_type = image_url
                                .url
                                .split(';')
                                .next()
                                .unwrap_or("image/png")
                                .strip_prefix("data:")
                                .unwrap_or("image/png");
                            arr.push(serde_json::json!({
                                "type": "image",
                                "source": {
                                    "type": "base64",
                                    "media_type": media_type,
                                    "data": base64_data
                                }
                            }));
                        }
                    }
                }
            }
            out.push(serde_json::json!({ "role": m.role, "content": arr }));
        } else {
            out.push(serde_json::json!({
                "role": m.role,
                "content": m.content.as_deref().unwrap_or("")
            }));
        }
    }

    (system_prompt, out)
}

pub fn build_anthropic_tools(tools: &[ToolDefinition]) -> Vec<serde_json::Value> {
    tools
        .iter()
        .map(|t| {
            serde_json::json!({
                "name": t.function.name,
                "description": t.function.description,
                "input_schema": t.function.parameters
            })
        })
        .collect()
}

pub fn parse_anthropic_response(data: &serde_json::Value) -> anyhow::Result<ChatResponse> {
    let mut text_content = String::new();
    let mut tool_calls: Vec<ToolCall> = Vec::new();

    if let Some(content) = data["content"].as_array() {
        for block in content {
            match block["type"].as_str() {
                Some("text") => {
                    if let Some(text) = block["text"].as_str() {
                        if !text_content.is_empty() {
                            text_content.push('\n');
                        }
                        text_content.push_str(text);
                    }
                }
                Some("tool_use") => {
                    tool_calls.push(ToolCall {
                        id: block["id"].as_str().unwrap_or_default().to_string(),
                        function: FunctionCall {
                            name: block["name"].as_str().unwrap_or_default().to_string(),
                            arguments: block["input"].to_string(),
                        },
                    });
                }
                _ => {}
            }
        }
    }

    let content = if text_content.is_empty() {
        None
    } else {
        Some(text_content)
    };
    let tool_calls = if tool_calls.is_empty() {
        None
    } else {
        Some(tool_calls)
    };

    Ok(ChatResponse {
        content,
        tool_calls,
        finish_reason: data["stop_reason"].as_str().map(|s| s.to_string()),
        usage: Some(Usage {
            prompt_tokens: data["usage"]["input_tokens"].as_u64().unwrap_or(0) as u32,
            completion_tokens: data["usage"]["output_tokens"].as_u64().unwrap_or(0) as u32,
            total_tokens: 0,
        }),
    })
}

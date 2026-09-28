//! Codex Responses SSE adapter. Display deltas are never executable calls.
use super::super::{
    error::{ErrorKind, ProviderError},
    http,
    provider::*,
};

/// Only a terminal success may produce executable tool calls. The shared byte
/// decoder handles CRLF, fragmented UTF-8 and bounded frames without data loss.
pub(super) async fn parse_sse(
    mut response: reqwest::Response,
    on_delta: &(dyn Fn(StreamDelta) + Send + Sync),
) -> anyhow::Result<ChatResponse> {
    let headers = response.headers().clone();
    let mut decoder = crate::sse::Decoder::default();
    let mut text = String::new();
    let mut reasoning = String::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(ProviderError::from_reqwest)?
    {
        for event in decoder
            .push(&chunk)
            .map_err(|_| ProviderError::new(ErrorKind::InvalidResponse))?
        {
            if event.data == "[DONE]" {
                continue;
            }
            let value: serde_json::Value = serde_json::from_str(&event.data)
                .map_err(|_| ProviderError::new(ErrorKind::InvalidResponse))?;
            match value["type"].as_str().unwrap_or(&event.event) {
                "response.output_text.delta" | "response.refusal.delta" => {
                    if let Some(delta) = value["delta"].as_str() {
                        text.push_str(delta);
                        on_delta(StreamDelta::Text {
                            text: delta.to_string(),
                        });
                    }
                }
                "response.reasoning_summary_text.delta" | "response.reasoning_text.delta" => {
                    if let Some(delta) = value["delta"].as_str() {
                        reasoning.push_str(delta);
                        on_delta(StreamDelta::Reasoning {
                            text: delta.to_string(),
                        });
                    }
                }
                "response.output_item.added" if value["item"]["type"] == "function_call" => {
                    let item = &value["item"];
                    on_delta(StreamDelta::ToolCall {
                        index: value["output_index"].as_u64().unwrap_or(0) as usize,
                        id: item["call_id"].as_str().map(str::to_string),
                        name: item["name"].as_str().map(str::to_string),
                        arguments: None,
                    });
                }
                "response.function_call_arguments.delta" => on_delta(StreamDelta::ToolCall {
                    index: value["output_index"].as_u64().unwrap_or(0) as usize,
                    id: None,
                    name: None,
                    arguments: value["delta"].as_str().map(str::to_string),
                }),
                "response.completed" => {
                    return completed_response(&value["response"], text, reasoning)
                }
                "response.incomplete" => {
                    let kind = match value["response"]["incomplete_details"]["reason"].as_str() {
                        Some("max_output_tokens") => ErrorKind::OutputLimit,
                        _ => ErrorKind::InvalidResponse,
                    };
                    return Err(ProviderError {
                        cause: Some("Codex response incomplete; no tools executed"),
                        ..ProviderError::new(kind)
                    }
                    .into());
                }
                "response.failed" | "error" => {
                    let data = value.get("response").unwrap_or(&value);
                    // Some error events carry code/type at the top level.
                    let wrapped = serde_json::json!({"error": data});
                    let mut error = http::payload_error(
                        200,
                        if data.get("error").is_some() {
                            data
                        } else {
                            &wrapped
                        },
                    );
                    http::metadata(&headers, &mut error);
                    return Err(error.into());
                }
                _ => {}
            }
        }
    }
    Err(ProviderError::new(ErrorKind::Interrupted).into())
}

fn completed_response(
    completed: &serde_json::Value,
    text: String,
    reasoning: String,
) -> anyhow::Result<ChatResponse> {
    let invalid = || ProviderError::new(ErrorKind::InvalidResponse);
    if completed.get("status").is_some_and(|s| s != "completed") {
        return Err(invalid().into());
    }
    let output = completed["output"].as_array().ok_or_else(invalid)?;
    let mut content = String::new();
    let mut tool_calls = Vec::new();
    for item in output {
        match item["type"].as_str().unwrap_or("") {
            "message" => {
                for part in item["content"].as_array().into_iter().flatten() {
                    if let Some(t) = part["text"].as_str().or_else(|| part["refusal"].as_str()) {
                        content.push_str(t);
                    }
                }
            }
            "function_call" => {
                if item.get("status").is_some_and(|s| s != "completed") {
                    return Err(invalid().into());
                }
                let id = item["call_id"]
                    .as_str()
                    .filter(|s| !s.is_empty())
                    .ok_or_else(invalid)?;
                let name = item["name"]
                    .as_str()
                    .filter(|s| !s.is_empty())
                    .ok_or_else(invalid)?;
                let arguments = item["arguments"].as_str().ok_or_else(invalid)?;
                let parsed: serde_json::Value =
                    serde_json::from_str(arguments).map_err(|_| invalid())?;
                if !parsed.is_object() || tool_calls.iter().any(|call: &ToolCall| call.id == id) {
                    return Err(invalid().into());
                }
                tool_calls.push(ToolCall {
                    id: id.into(),
                    function: FunctionCall {
                        name: name.into(),
                        arguments: arguments.into(),
                    },
                });
            }
            _ => {}
        }
    }
    if content.is_empty() {
        content = text;
    }
    if content.trim().is_empty() && tool_calls.is_empty() {
        return Err(invalid().into());
    }
    let usage = completed.get("usage").filter(|u| u.is_object()).map(|u| {
        let prompt_tokens = u["input_tokens"].as_u64().unwrap_or(0).min(u32::MAX as u64) as u32;
        let completion_tokens = u["output_tokens"]
            .as_u64()
            .unwrap_or(0)
            .min(u32::MAX as u64) as u32;
        Usage {
            prompt_tokens,
            completion_tokens,
            total_tokens: u["total_tokens"]
                .as_u64()
                .map(|n| n.min(u32::MAX as u64) as u32)
                .unwrap_or_else(|| prompt_tokens.saturating_add(completion_tokens)),
        }
    });
    Ok(ChatResponse {
        finish_reason: Some(
            if tool_calls.is_empty() {
                "stop"
            } else {
                "tool_calls"
            }
            .into(),
        ),
        content: if content.is_empty() {
            None
        } else {
            Some(content)
        },
        reasoning_content: if reasoning.is_empty() {
            None
        } else {
            Some(reasoning)
        },
        tool_calls: if tool_calls.is_empty() {
            None
        } else {
            Some(tool_calls)
        },
        usage,
    })
}

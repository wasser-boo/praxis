use async_trait::async_trait;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatRequest {
    pub messages: Vec<ChatMessage>,
    pub tools: Option<Vec<ToolDefinition>>,
    pub temperature: Option<f32>,
    pub max_tokens: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(skip)]
    pub vision_provider: Option<String>,
    #[serde(skip)]
    pub vision_model: Option<String>,
    /// Extended-reasoning toggle: None = provider default, Some("off") disables,
    /// Some("on") enables where the model supports it.
    #[serde(skip)]
    pub thinking: Option<ThinkingMode>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ThinkingMode {
    On,
    Off,
    Low,
    Medium,
    High,
    Xhigh,
}

impl ThinkingMode {
    /// Setting-String → Mode. Stufen (Qwen3.8 thinking levels): off → xhigh;
    /// "on" = Alias High, "auto"/unbekannt = None (Provider-Default).
    pub fn from_setting(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "on" | "true" | "1" => Some(Self::On),
            "off" | "false" | "0" | "none" => Some(Self::Off),
            "low" => Some(Self::Low),
            "medium" | "med" => Some(Self::Medium),
            "high" => Some(Self::High),
            "xhigh" | "extra_high" | "max" => Some(Self::Xhigh),
            _ => None,
        }
    }

    /// Provider-übergreifender Level-Name ("low"/"medium"/"high"/"xhigh");
    /// On → "high", Off → None.
    pub fn level(&self) -> Option<&'static str> {
        match self {
            Self::On | Self::High => Some("high"),
            Self::Low => Some("low"),
            Self::Medium => Some("medium"),
            Self::Xhigh => Some("xhigh"),
            Self::Off => None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum ContentPart {
    #[serde(rename = "text")]
    Text { text: String },
    #[serde(rename = "image_url")]
    ImageUrl { image_url: ImageUrlDetail },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImageUrlDetail {
    pub url: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    pub role: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reasoning_content: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content_parts: Option<Vec<ContentPart>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<ToolCall>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_name: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ToolCall {
    pub id: String,
    pub function: FunctionCall,
}

// llama-server rejects assistant tool_calls without a type field; inject it on
// every request serialization (the stored history omits it).
impl serde::Serialize for ToolCall {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeStruct;
        let mut s = serializer.serialize_struct("ToolCall", 3)?;
        s.serialize_field("type", "function")?;
        s.serialize_field("id", &self.id)?;
        s.serialize_field("function", &self.function)?;
        s.end()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FunctionCall {
    pub name: String,
    pub arguments: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolDefinition {
    #[serde(rename = "type")]
    pub tool_type: String,
    pub function: FunctionDefinition,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FunctionDefinition {
    pub name: String,
    pub description: String,
    pub parameters: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatResponse {
    pub content: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reasoning_content: Option<String>,
    pub tool_calls: Option<Vec<ToolCall>>,
    pub finish_reason: Option<String>,
    pub usage: Option<Usage>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Usage {
    pub prompt_tokens: u32,
    pub completion_tokens: u32,
    pub total_tokens: u32,
}

#[async_trait]
pub trait LLMProvider: Send + Sync {
    async fn chat(&self, request: ChatRequest) -> anyhow::Result<ChatResponse>;
    /// Adapters without native streaming return one complete response. Retry,
    /// cancellation and rate-limit policy are identical for both paths.
    async fn chat_stream(
        &self,
        request: ChatRequest,
        _on_token: &(dyn Fn(String) + Send + Sync),
    ) -> anyhow::Result<ChatResponse> {
        self.chat(request).await
    }
    fn name(&self) -> &str;
    fn as_any(&self) -> &dyn std::any::Any;
    async fn health_check(&self) -> bool {
        true
    }
}

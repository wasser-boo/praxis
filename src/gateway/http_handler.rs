use crate::gateway::GatewayState;
use axum::extract::State;
use axum::Json;
use serde::Serialize;

#[derive(Serialize)]
pub struct HealthResponse {
    pub status: String,
    pub uptime_secs: u64,
    pub version: String,
}

#[derive(Serialize)]
pub struct StatusResponse {
    pub status: String,
    pub version: String,
    pub uptime_secs: u64,
    pub llm_providers: Vec<String>,
    pub default_provider: String,
}

#[derive(serde::Deserialize)]
pub struct ChatRequest {
    user_id: String,
    message: String,
}

#[derive(serde::Serialize)]
pub struct ChatResponse {
    pub success: bool,
    pub response: Option<String>,
    pub error: Option<String>,
}

pub async fn health_check(State(state): State<GatewayState>) -> Json<HealthResponse> {
    Json(HealthResponse {
        status: "ok".to_string(),
        uptime_secs: state.start_time.elapsed().as_secs(),
        version: env!("CARGO_PKG_VERSION").to_string(),
    })
}

pub async fn status(State(state): State<GatewayState>) -> Json<StatusResponse> {
    let providers: Vec<String> = state.llm.provider_names();
    let default_provider = state.config.use_provider.clone();

    Json(StatusResponse {
        status: "ok".to_string(),
        version: env!("CARGO_PKG_VERSION").to_string(),
        uptime_secs: state.start_time.elapsed().as_secs(),
        llm_providers: providers,
        default_provider,
    })
}

/// REST endpoint for web chat - same as WebSocket but via HTTP POST
pub async fn chat_handler(
    State(state): State<GatewayState>,
    Json(req): Json<ChatRequest>,
) -> Json<ChatResponse> {
    tracing::info!(user_id = %req.user_id, "Web chat message received");

    match crate::gateway::message_handler::handle_message(
        &state,
        &req.user_id,
        &req.message,
        Some("web"),
    )
    .await
    {
        Ok(reply) => Json(ChatResponse {
            success: true,
            response: Some(reply),
            error: None,
        }),
        Err(e) => Json(ChatResponse {
            success: false,
            response: None,
            error: Some(e.to_string()),
        }),
    }
}

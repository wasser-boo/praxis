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

#[cfg(test)]
mod gateway_tests {
    #[test]
    fn test_health_compiles() {
        assert!(true);
    }
}

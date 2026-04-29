use axum::extract::State;
use axum::Json;
use serde::Serialize;
use crate::gateway::GatewayState;

#[derive(Serialize)]
pub struct HealthResponse {
    pub status: String,
    pub uptime_secs: u64,
    pub version: String,
}

pub async fn health_check(
    State(state): State<GatewayState>,
) -> Json<HealthResponse> {
    Json(HealthResponse {
        status: "ok".to_string(),
        uptime_secs: state.start_time.elapsed().as_secs(),
        version: env!("CARGO_PKG_VERSION").to_string(),
    })
}

#[cfg(test)]
mod gateway_tests {
    #[test]
    fn test_health_compiles() {
        assert!(true);
    }
}

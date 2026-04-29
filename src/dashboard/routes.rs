use axum::Router;
use axum::Json;
use serde::Serialize;

#[derive(Serialize)]
pub struct StatusResponse {
    pub status: String,
    pub version: String,
    pub uptime_secs: u64,
    pub active_users: usize,
}

pub fn routes(db: crate::db::Database) -> Router {
    Router::new()
        .route("/", axum::routing::get(index))
        .route("/api/status", axum::routing::get(status))
        .route("/api/contexts", axum::routing::get(list_contexts))
        .with_state(db)
}

async fn index() -> &'static str {
    "Praxis Dashboard"
}

async fn status() -> Json<serde_json::Value> {
    Json(serde_json::json!({
        "status": "ok",
        "version": env!("CARGO_PKG_VERSION")
    }))
}

async fn list_contexts(
    axum::extract::State(_db): axum::extract::State<crate::db::Database>,
) -> Json<serde_json::Value> {
    Json(serde_json::json!({
        "contexts": []
    }))
}

#[cfg(test)]
mod dashboard_tests {
    #[test]
    fn test_routes_compiles() {
        assert!(true);
    }
}

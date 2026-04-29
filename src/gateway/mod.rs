pub mod auth;
pub mod cron_scheduler;
pub mod http_handler;
pub mod llm;
pub mod message_handler;
pub mod poml;
pub mod rate_limiter;
pub mod templates;
pub mod ws_handler;

use std::sync::Arc;

#[derive(Clone)]
pub struct GatewayState {
    pub db: crate::db::Database,
    pub config: crate::config::Config,
    pub secrets: crate::db::secrets::Secrets,
    pub llm: Arc<llm::LLMRouter>,
    pub event_tx: tokio::sync::broadcast::Sender<crate::event_channel::GatewayEvent>,
    pub start_time: std::time::Instant,
}

pub async fn start(db: crate::db::Database, config: crate::config::Config) -> anyhow::Result<()> {
    let secrets = crate::db::secrets::get_secrets().await.unwrap_or_default();
    let event_tx = crate::event_channel::init();
    let llm = Arc::new(llm::LLMRouter::new(&config, &secrets));

    let state = GatewayState {
        db,
        config: config.clone(),
        secrets,
        llm,
        event_tx,
        start_time: std::time::Instant::now(),
    };

    let app = axum::Router::new()
        .route("/ws", axum::routing::get(ws_handler::ws_handler))
        .route("/health", axum::routing::get(http_handler::health_check))
        .route("/api/auth/login", axum::routing::post(auth::login_handler))
        .with_state(state.clone());

    let addr = format!("0.0.0.0:{}", config.gateway_port);
    tracing::info!("Gateway listening on {}", addr);

    let listener = tokio::net::TcpListener::bind(&addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}

#[cfg(test)]
mod gateway_tests {
    #[test]
    fn test_gateway_compiles() {
        assert!(true);
    }
}

pub mod routes;

pub struct DashboardServer {
    port: u16,
    db: crate::db::Database,
}

impl DashboardServer {
    pub fn new(port: u16, db: crate::db::Database) -> Self {
        Self { port, db }
    }

    pub async fn start(&self) -> anyhow::Result<()> {
        let app = routes::routes(self.db.clone());

        let addr = format!("0.0.0.0:{}", self.port);
        tracing::info!("Dashboard starting on {}", addr);

        let listener = tokio::net::TcpListener::bind(&addr).await?;
        axum::serve(listener, app).await?;

        Ok(())
    }
}

#[cfg(test)]
mod dashboard_tests {
    use super::*;

    #[test]
    fn test_dashboard_creation() {
        let dir = tempfile::TempDir::new().unwrap();
        let db = crate::db::Database::new(dir.path()).unwrap();
        let server = DashboardServer::new(1337, db);
        assert_eq!(server.port, 1337);
    }
}

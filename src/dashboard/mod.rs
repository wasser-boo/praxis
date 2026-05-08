pub mod routes;

pub struct DashboardServer {
    port: u16,
    tls: bool,
    db: crate::db::Database,
}

impl DashboardServer {
    pub fn new(port: u16, tls: bool, db: crate::db::Database) -> Self {
        Self { port, tls, db }
    }

    pub async fn start(&self) -> anyhow::Result<()> {
        let app = routes::routes(self.db.clone());

        let addr = format!("0.0.0.0:{}", self.port);

        if self.tls {
            rustls::crypto::CryptoProvider::install_default(
                rustls::crypto::aws_lc_rs::default_provider(),
            )
            .expect("Failed to install TLS crypto provider");

            let cert = generate_self_signed_cert()?;
            let config = axum_server::tls_rustls::RustlsConfig::from_der(
                vec![cert.cert_der.clone()],
                cert.key_der.clone(),
            )
            .await?;

            let proto = if self.port == 443 { "https" } else { "https" };
            tracing::info!("Dashboard starting on {}://{}", proto, addr);

            axum_server::bind_rustls(addr.parse()?, config)
                .serve(app.into_make_service())
                .await?;
        } else {
            tracing::info!("Dashboard starting on http://{}", addr);
            let listener = tokio::net::TcpListener::bind(&addr).await?;
            axum::serve(listener, app).await?;
        }

        Ok(())
    }
}

struct SelfSignedCert {
    cert_der: Vec<u8>,
    key_der: Vec<u8>,
}

fn generate_self_signed_cert() -> anyhow::Result<SelfSignedCert> {
    let key = rcgen::KeyPair::generate()?;
    let mut params = rcgen::CertificateParams::new(vec!["localhost".to_string()])?;
    params.distinguished_name = rcgen::DistinguishedName::new();
    params.distinguished_name.push(
        rcgen::DnType::OrganizationName,
        "Praxis Dashboard",
    );
    params.distinguished_name.push(
        rcgen::DnType::CommonName,
        "Praxis Dashboard",
    );

    let cert = params.self_signed(&key)?;

    Ok(SelfSignedCert {
        cert_der: cert.der().to_vec(),
        key_der: key.serialize_der(),
    })
}

#[cfg(test)]
mod dashboard_tests {
    use super::*;

    #[test]
    fn test_dashboard_creation() {
        let dir = tempfile::TempDir::new().unwrap();
        let db = crate::db::Database::new(dir.path()).unwrap();
        let server = DashboardServer::new(1337, false, db);
        assert_eq!(server.port, 1337);
    }
}

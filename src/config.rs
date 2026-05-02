use std::env;

#[derive(Clone, Debug, PartialEq)]
pub enum ApiMode {
    OpenAI,
    Anthropic,
}

impl ApiMode {
    pub fn from_str(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "anthropic" => ApiMode::Anthropic,
            _ => ApiMode::OpenAI,
        }
    }
}

#[derive(Clone, Debug)]
pub struct Config {
    pub poml_cli: String,
    pub use_provider: String,
    pub openai_api_key: Option<String>,
    pub openai_model: String,
    pub openai_api_base: String,
    pub anthropic_api_key: Option<String>,
    pub anthropic_model: String,
    pub anthropic_api_base: String,
    pub ollama_api_base: String,
    pub ollama_model: String,
    pub minimax_api_key: Option<String>,
    pub minimax_model: String,
    pub minimax_api_base: String,
    pub minimax_api_mode: ApiMode,
    pub mimo_api_key: Option<String>,
    pub mimo_model: String,
    pub mimo_api_base: String,
    pub mimo_api_mode: ApiMode,
    pub gateway_port: u16,
    pub gateway_api_key: String,
    pub dashboard_port: u16,
    pub dashboard_admin_password: String,
    pub data_dir: String,
    pub rust_log: String,
    pub vm_enabled: bool,
    pub vm_cpu_cores: u32,
    pub vm_ram_mb: u32,
    pub vm_disk_size: String,
    pub vm_arch: String,
    pub vm_mode: String,        // "shared" or "vm"
    pub vm_socket_mode: String, // "unix" or "tcp" (auto-detected per OS if empty)
}

impl Config {
    pub fn from_env() -> Self {
        Self {
            poml_cli: env::var("POML_CLI").unwrap_or_else(|_| "./poml/js/cli.cjs".to_string()),
            use_provider: env::var("USE_PROVIDER").unwrap_or_else(|_| "openai".to_string()),
            openai_api_key: env::var("OPENAI_API_KEY").ok(),
            openai_model: env::var("OPENAI_MODEL").unwrap_or_else(|_| "gpt-4o".to_string()),
            openai_api_base: env::var("OPENAI_API_BASE")
                .unwrap_or_else(|_| "https://api.openai.com/v1".to_string()),
            anthropic_api_key: env::var("ANTHROPIC_API_KEY").ok(),
            anthropic_model: env::var("ANTHROPIC_MODEL")
                .unwrap_or_else(|_| "claude-3-5-sonnet-20241022".to_string()),
            anthropic_api_base: env::var("ANTHROPIC_API_BASE")
                .unwrap_or_else(|_| "https://api.anthropic.com".to_string()),
            ollama_api_base: env::var("OLLAMA_API_BASE")
                .unwrap_or_else(|_| "http://localhost:11434".to_string()),
            ollama_model: env::var("OLLAMA_MODEL").unwrap_or_else(|_| "llama3".to_string()),
            minimax_api_key: env::var("MINIMAX_API_KEY").ok(),
            minimax_model: env::var("MINIMAX_MODEL")
                .unwrap_or_else(|_| "MiniMax-Text-01".to_string()),
            minimax_api_base: env::var("MINIMAX_API_BASE")
                .unwrap_or_else(|_| "https://api.minimax.chat/v1".to_string()),
            minimax_api_mode: ApiMode::from_str(
                &env::var("MINIMAX_API_MODE").unwrap_or_else(|_| "openai".to_string()),
            ),
            mimo_api_key: env::var("MIMO_API_KEY").ok(),
            mimo_model: env::var("MIMO_MODEL").unwrap_or_else(|_| "mimo".to_string()),
            mimo_api_base: env::var("MIMO_API_BASE")
                .unwrap_or_else(|_| "https://api.mimo.com/v1".to_string()),
            mimo_api_mode: ApiMode::from_str(
                &env::var("MIMO_API_MODE").unwrap_or_else(|_| "openai".to_string()),
            ),
            gateway_port: env::var("GATEWAY_PORT")
                .unwrap_or_else(|_| "3537".to_string())
                .parse()
                .unwrap_or(3537),
            gateway_api_key: env::var("GATEWAY_API_KEY").unwrap_or_default(),
            dashboard_port: env::var("DASHBOARD_PORT")
                .unwrap_or_else(|_| "1337".to_string())
                .parse()
                .unwrap_or(1337),
            dashboard_admin_password: env::var("DASHBOARD_ADMIN_PASSWORD").unwrap_or_default(),
            data_dir: env::var("DATA_DIR").unwrap_or_else(|_| "./data".to_string()),
            rust_log: env::var("RUST_LOG").unwrap_or_else(|_| "info".to_string()),
            vm_enabled: env::var("VM_ENABLED")
                .map(|v| v == "true" || v == "1")
                .unwrap_or(false),
            vm_cpu_cores: env::var("VM_CPU_CORES")
                .unwrap_or_else(|_| "2".to_string())
                .parse()
                .unwrap_or(2),
            vm_ram_mb: env::var("VM_RAM_MB")
                .unwrap_or_else(|_| "4096".to_string())
                .parse()
                .unwrap_or(4096),
            vm_disk_size: env::var("VM_DISK_SIZE").unwrap_or_else(|_| "40G".to_string()),
            vm_arch: env::var("VM_ARCH").unwrap_or_else(|_| "x86_64".to_string()),
            vm_mode: env::var("VM_MODE").unwrap_or_else(|_| "shared".to_string()),
            vm_socket_mode: env::var("VM_SOCKET_MODE").unwrap_or_else(|_| {
                if cfg!(target_os = "linux") {
                    "unix".to_string()
                } else {
                    "tcp".to_string()
                }
            }),
        }
    }

    pub fn apply_secrets(&mut self, secrets: &crate::db::secrets::Secrets) {
        if let Some(ref key) = secrets.gateway_api_key {
            if !key.is_empty() && self.gateway_api_key.is_empty() {
                self.gateway_api_key = key.clone();
            }
        }
        if let Some(ref pass) = secrets.dashboard_admin_password {
            if !pass.is_empty() && self.dashboard_admin_password.is_empty() {
                self.dashboard_admin_password = pass.clone();
            }
        }
    }

    pub fn ensure_generated(&mut self) {
        if self.gateway_api_key.is_empty() {
            let key = uuid::Uuid::new_v4().to_string();
            tracing::info!("GATEWAY_API_KEY not set, generated: {}", key);
            self.gateway_api_key = key;
        }
        if self.dashboard_admin_password.is_empty() {
            let pass = uuid::Uuid::new_v4().to_string()[..12].to_string();
            tracing::info!("DASHBOARD_ADMIN_PASSWORD not set, generated: {}", pass);
            self.dashboard_admin_password = pass;
        }
    }

    pub fn validate(&self) -> anyhow::Result<()> {
        if self.gateway_api_key.len() < 16 {
            anyhow::bail!("GATEWAY_API_KEY must be at least 16 characters");
        }
        if self.dashboard_admin_password.len() < 8 {
            anyhow::bail!("DASHBOARD_ADMIN_PASSWORD must be at least 8 characters");
        }
        Ok(())
    }
}

#[cfg(test)]
mod security_tests {
    use super::*;

    #[test]
    fn test_config_defaults() {
        let config = Config::from_env();
        assert!(!config.data_dir.is_empty());
        assert!(!config.rust_log.is_empty());
    }

    #[test]
    fn test_validate_short_api_key() {
        let config = Config {
            gateway_api_key: "short".to_string(),
            dashboard_admin_password: "longpassword".to_string(),
            ..Config::from_env()
        };
        assert!(config.validate().is_err());
    }

    #[test]
    fn test_validate_short_password() {
        let config = Config {
            gateway_api_key: "a".repeat(20),
            dashboard_admin_password: "short".to_string(),
            ..Config::from_env()
        };
        assert!(config.validate().is_err());
    }

    #[test]
    fn test_validate_ok() {
        let config = Config {
            gateway_api_key: "a".repeat(20),
            dashboard_admin_password: "longpassword".to_string(),
            ..Config::from_env()
        };
        assert!(config.validate().is_ok());
    }
}

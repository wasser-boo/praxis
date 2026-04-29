use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Secrets {
    pub discord_bot_token: Option<String>,
    pub minimax_api_key: Option<String>,
    pub mimo_api_key: Option<String>,
    pub openai_api_key: Option<String>,
    pub anthropic_api_key: Option<String>,
    pub elevenlabs_api_key: Option<String>,
    pub gateway_api_key: Option<String>,
}

static SECRETS: RwLock<Option<Secrets>> = RwLock::const_new(None);

pub async fn get_secrets() -> Option<Secrets> {
    SECRETS.read().await.clone()
}

pub async fn init_secrets(secrets: Secrets) {
    let mut lock = SECRETS.write().await;
    *lock = Some(secrets);
}

pub async fn update_secrets(new_secrets: Secrets) {
    let mut lock = SECRETS.write().await;
    *lock = Some(new_secrets);
}

#[cfg(test)]
mod security_tests {
    use super::*;

    #[tokio::test]
    async fn test_secrets_default() {
        let secrets = Secrets::default();
        assert!(secrets.openai_api_key.is_none());
    }

    #[tokio::test]
    async fn test_init_and_get_secrets() {
        let secrets = Secrets {
            openai_api_key: Some("test-key".to_string()),
            ..Default::default()
        };
        init_secrets(secrets).await;
        let loaded = get_secrets().await.unwrap();
        assert_eq!(loaded.openai_api_key, Some("test-key".to_string()));
    }
}

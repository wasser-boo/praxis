use serde::{Deserialize, Serialize};
use std::sync::OnceLock;

static SECRETS: OnceLock<Secrets> = OnceLock::new();

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Secrets {
    #[serde(default)]
    pub discord_bot_token: Option<String>,
    #[serde(default)]
    pub minimax_api_key: Option<String>,
    #[serde(default)]
    pub mimo_api_key: Option<String>,
    #[serde(default)]
    pub openai_api_key: Option<String>,
    #[serde(default)]
    pub anthropic_api_key: Option<String>,
    #[serde(default)]
    pub elevenlabs_api_key: Option<String>,
    #[serde(default)]
    pub gateway_api_key: Option<String>,
    #[serde(default)]
    pub dashboard_admin_password: Option<String>,
    #[serde(default)]
    pub voice_elevenlabs_api_key: Option<String>,
    #[serde(default)]
    pub voice_elevenlabs_stt_api_key: Option<String>,
}

pub fn init_secrets(secrets: Secrets) {
    SECRETS.set(secrets).ok();
}

pub fn get_secrets() -> Secrets {
    SECRETS.get().cloned().unwrap_or_default()
}

pub fn load_secrets_with_password(password: &str) -> anyhow::Result<Secrets> {
    if super::enc2::has_encrypted_secrets() {
        let json = super::enc2::load_encrypted_secrets(password)?;
        let secrets: Secrets = serde_json::from_str(&json)?;
        tracing::info!("Loaded encrypted secrets");
        return Ok(secrets);
    }

    Err(anyhow::anyhow!("No encrypted secrets found. Run 'praxis onboard --interactive' to set up."))
}

pub fn save_secrets(secrets: &Secrets, password: &str) -> anyhow::Result<()> {
    let content = serde_json::to_string(secrets)?;
    super::enc2::save_encrypted_secrets(&content, password)?;
    tracing::info!("Saved encrypted secrets");
    Ok(())
}

pub fn has_secrets() -> bool {
    super::enc2::has_encrypted_secrets()
}

pub fn mask_secret(secret: &Option<String>) -> String {
    match secret {
        Some(s) if s.len() > 4 => format!("***{}", &s[s.len()-4..]),
        Some(_) => "***".to_string(),
        None => "".to_string(),
    }
}

pub fn migrate_plaintext_to_encrypted(password: &str) -> anyhow::Result<()> {
    let path = std::path::Path::new("secrets.json");
    if !path.exists() {
        tracing::info!("No plaintext secrets.json to migrate");
        return Ok(());
    }

    let content = std::fs::read_to_string(path)?;
    let secrets: Secrets = serde_json::from_str(&content)?;

    save_secrets(&secrets, password)?;

    let backup = "secrets.json.migrated";
    std::fs::rename(path, backup)?;
    tracing::info!("Migrated secrets.json to encrypted format, backup at {}", backup);

    Ok(())
}

#[cfg(test)]
mod security_tests {
    use super::*;

    #[test]
    fn test_secrets_default() {
        let secrets = Secrets::default();
        assert!(secrets.openai_api_key.is_none());
        assert!(secrets.discord_bot_token.is_none());
        assert!(secrets.minimax_api_key.is_none());
    }

    #[test]
    fn test_secrets_serialization() {
        let secrets = Secrets {
            discord_bot_token: Some("token".to_string()),
            minimax_api_key: Some("key".to_string()),
            ..Default::default()
        };
        let json = serde_json::to_string(&secrets).unwrap();
        let deserialized: Secrets = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.discord_bot_token, Some("token".to_string()));
        assert_eq!(deserialized.minimax_api_key, Some("key".to_string()));
    }

    #[test]
    fn test_mask_secret() {
        assert_eq!(mask_secret(&None), "");
        assert_eq!(mask_secret(&Some("abc".to_string())), "***");
        assert_eq!(mask_secret(&Some("abcdefghijklmnop".to_string())), "***mnop");
    }

    #[test]
    fn test_secrets_roundtrip() {
        let secrets = Secrets {
            discord_bot_token: Some("token123".to_string()),
            minimax_api_key: Some("key456".to_string()),
            mimo_api_key: Some("mimo789".to_string()),
            ..Default::default()
        };

        let json = serde_json::to_string(&secrets).unwrap();
        let loaded: Secrets = serde_json::from_str(&json).unwrap();

        assert_eq!(loaded.discord_bot_token, secrets.discord_bot_token);
        assert_eq!(loaded.minimax_api_key, secrets.minimax_api_key);
        assert_eq!(loaded.mimo_api_key, secrets.mimo_api_key);
    }

    #[test]
    fn test_secrets_partial() {
        let json = r#"{"discord_bot_token":"token"}"#;
        let secrets: Secrets = serde_json::from_str(json).unwrap();
        assert_eq!(secrets.discord_bot_token, Some("token".to_string()));
        assert!(secrets.minimax_api_key.is_none());
        assert!(secrets.mimo_api_key.is_none());
    }
}

use serde::{Deserialize, Serialize};
use std::sync::RwLock;

static SECRETS: RwLock<Option<Secrets>> = RwLock::new(None);
/// Master key retained by the running gateway so `/login` and token refreshes
/// can persist credentials without prompting. Process memory only; never
/// exported to env, argv or child processes. Opt out with
/// `PRAXIS_RETAIN_MASTER_KEY=0` (logins then live until restart).
static MASTER_KEY: RwLock<Option<String>> = RwLock::new(None);

/// Tests that mutate the process-global store serialize on this lock.
#[cfg(test)]
pub fn test_lock() -> std::sync::MutexGuard<'static, ()> {
    static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    LOCK.lock().unwrap_or_else(|e| e.into_inner())
}

pub fn retain_master_password(password: &str) {
    if std::env::var("PRAXIS_RETAIN_MASTER_KEY").is_ok_and(|v| v == "0" || v.eq_ignore_ascii_case("false")) {
        return;
    }
    if let Ok(mut guard) = MASTER_KEY.write() {
        *guard = Some(password.to_string());
    }
}

pub fn master_key_retained() -> bool {
    MASTER_KEY.read().is_ok_and(|g| g.is_some())
}

/// Save to the encrypted store when the gateway holds the master key.
/// Returns Ok(false) when it cannot persist (in-memory update only).
pub fn persist_if_unlocked(secrets: &Secrets) -> anyhow::Result<bool> {
    let key = MASTER_KEY.read().ok().and_then(|g| g.clone());
    match key {
        Some(key) => {
            save_secrets(secrets, &key)?;
            Ok(true)
        }
        None => Ok(false),
    }
}

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
    pub openrouter_api_key: Option<String>,
    #[serde(default)]
    pub ollama_api_key: Option<String>,
    #[serde(default)]
    pub llamacpp_api_key: Option<String>,
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
    #[serde(default)]
    pub custom: std::collections::HashMap<String, String>,
}

impl Secrets {
    /// Resolve a declared plugin credential without serializing the full store.
    /// Explicit custom values win; empty/onboarding placeholders never shadow
    /// existing native media credentials. Native gateway/admin secrets stay private.
    pub fn plugin_secret(&self, key: &str) -> Option<&str> {
        let usable = |value: &&str| !value.trim().is_empty() && value.trim() != "CHANGE_ME";
        self.custom.get(key).map(String::as_str).filter(usable).or_else(|| {
            match key {
                "elevenlabs_api_key" => self.elevenlabs_api_key.as_deref(),
                "openrouter_api_key" => self.openrouter_api_key.as_deref(),
                _ => None,
            }.filter(usable)
        })
    }
}

pub fn init_secrets(secrets: Secrets) {
    if let Ok(mut guard) = SECRETS.write() {
        *guard = Some(secrets);
    }
}

pub fn get_secrets() -> Secrets {
    SECRETS
        .read()
        .ok()
        .and_then(|guard| guard.clone())
        .unwrap_or_default()
}

pub fn load_secrets_with_password(password: &str) -> anyhow::Result<Secrets> {
    if super::enc2::has_encrypted_secrets() {
        let json = super::enc2::load_encrypted_secrets(password)?;
        let secrets: Secrets = serde_json::from_str(&json)?;
        tracing::info!("Loaded encrypted secrets");
        return Ok(secrets);
    }

    Err(anyhow::anyhow!(
        "No encrypted secrets found. Run 'praxis onboard --interactive' to set up."
    ))
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
        Some(s) if s.len() > 4 => format!("***{}", &s[s.len() - 4..]),
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
    tracing::info!(
        "Migrated secrets.json to encrypted format, backup at {}",
        backup
    );

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
        assert_eq!(
            mask_secret(&Some("abcdefghijklmnop".to_string())),
            "***mnop"
        );
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

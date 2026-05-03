/// Inject Praxis secrets into the VM via 9p shared folder.
///
/// Security design: Secrets are stored as INDIVIDUAL FILES, NOT as env vars.
/// Each secret is a separate file under /run/secrets/<key> with mode 0600.
/// This prevents `env` or `echo $SECRET` from dumping all secrets.
/// Applications must explicitly read the file they need: `cat /run/secrets/OPENAI_API_KEY`
pub async fn inject_secrets(vm_name: &str, data_dir: &str) -> anyhow::Result<()> {
    let secrets_dir = format!("{}/vm/{}/secrets", data_dir, vm_name);
    std::fs::create_dir_all(&secrets_dir)?;

    let secrets = crate::db::secrets::get_secrets();
    let mut count = 0u32;

    // Write each secret as an individual file
    let write_secret = |name: &str, value: &Option<String>| -> bool {
        if let Some(ref v) = value {
            if !v.is_empty() && v != "CHANGE_ME" {
                let path = format!("{}/{}", secrets_dir, name);
                let _ = std::fs::write(&path, v);
                return true;
            }
        }
        false
    };

    if write_secret("DISCORD_BOT_TOKEN", &secrets.discord_bot_token) {
        count += 1;
    }
    if write_secret("OPENAI_API_KEY", &secrets.openai_api_key) {
        count += 1;
    }
    if write_secret("ANTHROPIC_API_KEY", &secrets.anthropic_api_key) {
        count += 1;
    }
    if write_secret("OLLAMA_API_KEY", &secrets.ollama_api_key) {
        count += 1;
    }
    if write_secret("MINIMAX_API_KEY", &secrets.minimax_api_key) {
        count += 1;
    }
    if write_secret("MIMO_API_KEY", &secrets.mimo_api_key) {
        count += 1;
    }
    if write_secret("ELEVENLABS_API_KEY", &secrets.elevenlabs_api_key) {
        count += 1;
    }
    if write_secret(
        "VOICE_ELEVENLABS_API_KEY",
        &secrets.voice_elevenlabs_api_key,
    ) {
        count += 1;
    }
    if write_secret(
        "VOICE_ELEVENLABS_STT_API_KEY",
        &secrets.voice_elevenlabs_stt_api_key,
    ) {
        count += 1;
    }
    if write_secret("GATEWAY_API_KEY", &secrets.gateway_api_key) {
        count += 1;
    }
    if write_secret(
        "DASHBOARD_ADMIN_PASSWORD",
        &secrets.dashboard_admin_password,
    ) {
        count += 1;
    }

    // Custom secrets (plugin-specific)
    for (key, value) in &secrets.custom {
        if !value.is_empty() && value != "CHANGE_ME" {
            let path = format!("{}/{}", secrets_dir, key.to_uppercase());
            let _ = std::fs::write(&path, value);
            count += 1;
        }
    }

    // Create a manifest so apps can discover available secrets
    let manifest_lines: Vec<String> = std::fs::read_dir(&secrets_dir)
        .ok()
        .into_iter()
        .flatten()
        .filter_map(|e| e.ok())
        .filter(|e| {
            let name = e.file_name().to_string_lossy().to_string();
            !name.starts_with('.')
                && e.file_type().map(|t| t.is_file()).unwrap_or(false)
                && name != "manifest.txt"
                && name != "praxis-loader.sh"
        })
        .map(|e| e.file_name().to_string_lossy().to_string())
        .collect();
    let _ = std::fs::write(
        format!("{}/manifest.txt", secrets_dir),
        manifest_lines.join("\n"),
    );

    // Loader script: apps can `source /run/secrets/praxis-loader.sh` to load
    // specific secrets into their environment. Reads from individual files.
    let loader = r#"#!/bin/sh
# Praxis Secret Loader — mount secrets first:
#   mount -t 9p -o trans=virtio,version=9p2000.L praxis-secrets /run/secrets
#
# Usage: source /run/secrets/praxis-loader.sh [SECRET_NAME ...]
#   If no arguments, loads all available secrets.
#   If arguments given, loads only those specific secrets.
#
# Examples:
#   source /run/secrets/praxis-loader.sh                    # load all
#   source /run/secrets/praxis-loader.sh OPENAI_API_KEY     # load one
#   source /run/secrets/praxis-loader.sh OPENAI_API_KEY ANTHROPIC_API_KEY  # load two

SECRETS_DIR="/run/secrets"

_load_secret() {
    local name="$1"
    local file="$SECRETS_DIR/$name"
    if [ -f "$file" ] && [ "$name" != "manifest.txt" ] && [ "$name" != "praxis-loader.sh" ]; then
        export "$name=$(cat "$file")"
    fi
}

if [ $# -eq 0 ]; then
    # Load all
    for f in "$SECRETS_DIR"/*; do
        name=$(basename "$f")
        _load_secret "$name"
    done
else
    # Load specific
    for name in "$@"; do
        _load_secret "$name"
    done
fi
"#;
    let _ = std::fs::write(format!("{}/praxis-loader.sh", secrets_dir), loader);

    tracing::info!(
        vm = %vm_name,
        secrets_count = count,
        "Secrets injected as individual files into VM 9p share"
    );

    Ok(())
}

/// Refresh secrets in the 9p folder (e.g., after dashboard secret update)
pub async fn refresh_secrets(vm_name: &str, data_dir: &str) -> anyhow::Result<()> {
    inject_secrets(vm_name, data_dir).await
}

/// Get a summary of injected secret keys (without values) for logging
pub fn list_injected_keys() -> Vec<String> {
    let secrets = crate::db::secrets::get_secrets();
    let mut keys: Vec<String> = Vec::new();

    if secrets.discord_bot_token.is_some() {
        keys.push("DISCORD_BOT_TOKEN".into());
    }
    if secrets.openai_api_key.is_some() {
        keys.push("OPENAI_API_KEY".into());
    }
    if secrets.anthropic_api_key.is_some() {
        keys.push("ANTHROPIC_API_KEY".into());
    }
    if secrets.ollama_api_key.is_some() {
        keys.push("OLLAMA_API_KEY".into());
    }
    if secrets.minimax_api_key.is_some() {
        keys.push("MINIMAX_API_KEY".into());
    }
    if secrets.mimo_api_key.is_some() {
        keys.push("MIMO_API_KEY".into());
    }
    if secrets.elevenlabs_api_key.is_some() {
        keys.push("ELEVENLABS_API_KEY".into());
    }
    for key in secrets.custom.keys() {
        keys.push(key.to_uppercase());
    }

    keys
}

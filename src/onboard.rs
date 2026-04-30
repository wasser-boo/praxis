use std::io::{self, Write};

pub fn run_interactive_onboard() -> anyhow::Result<()> {
    println!();
    println!("========================================");
    println!("   Praxis AI Agent Platform - Setup");
    println!("========================================");
    println!();

    let mut env_lines: Vec<String> = Vec::new();

    // POML CLI
    println!("--- POML CLI ---");
    let poml_cli = prompt_with_default("POML CLI Path", "./poml/js/cli.cjs");
    env_lines.push(format!("POML_CLI={}", poml_cli));
    println!();

    // LLM Provider
    println!("--- LLM Provider ---");
    println!("Which LLM provider do you want to use?");
    println!("  1) OpenAI (GPT-4)");
    println!("  2) Anthropic (Claude)");
    println!("  3) Ollama (Local)");
    println!("  4) MiniMax");
    println!("  5) MiMo");
    println!();

    let provider_choice = prompt_choice("Select provider", &["1", "2", "3", "4", "5"], "1")?;
    let provider_name = match provider_choice.as_str() {
        "1" => {
            let api_key = prompt_required("OpenAI API Key")?;
            let model = prompt_with_default("OpenAI Model", "gpt-4o");
            let base_url = prompt_with_default("OpenAI API Base URL", "https://api.openai.com/v1");
            env_lines.push(format!("OPENAI_API_KEY={}", api_key));
            env_lines.push(format!("OPENAI_MODEL={}", model));
            env_lines.push(format!("OPENAI_API_BASE={}", base_url));
            "openai"
        }
        "2" => {
            let api_key = prompt_required("Anthropic API Key")?;
            let model = prompt_with_default("Anthropic Model", "claude-3-5-sonnet-20241022");
            let base_url = prompt_with_default("Anthropic API Base URL", "https://api.anthropic.com");
            env_lines.push(format!("ANTHROPIC_API_KEY={}", api_key));
            env_lines.push(format!("ANTHROPIC_MODEL={}", model));
            env_lines.push(format!("ANTHROPIC_API_BASE={}", base_url));
            "anthropic"
        }
        "3" => {
            let base_url = prompt_with_default("Ollama API Base URL", "http://localhost:11434");
            let model = prompt_with_default("Ollama Model", "llama3");
            env_lines.push(format!("OLLAMA_API_BASE={}", base_url));
            env_lines.push(format!("OLLAMA_MODEL={}", model));
            "ollama"
        }
        "4" => {
            let api_key = prompt_required("MiniMax API Key")?;
            let model = prompt_with_default("MiniMax Model", "MiniMax-Text-01");
            let base_url = prompt_with_default("MiniMax API Base URL", "https://api.minimax.chat/v1");
            println!("  API compatibility mode:");
            println!("    1) OpenAI-compatible (default)");
            println!("    2) Anthropic-compatible");
            let api_mode = prompt_choice("Select API mode", &["1", "2"], "1")?;
            let mode_str = if api_mode == "2" { "anthropic" } else { "openai" };
            env_lines.push(format!("MINIMAX_API_KEY={}", api_key));
            env_lines.push(format!("MINIMAX_MODEL={}", model));
            env_lines.push(format!("MINIMAX_API_BASE={}", base_url));
            env_lines.push(format!("MINIMAX_API_MODE={}", mode_str));
            "minimax"
        }
        "5" => {
            let api_key = prompt_required("MiMo API Key")?;
            let model = prompt_with_default("MiMo Model", "mimo-v2.5-pro");
            let base_url = prompt_with_default("MiMo API Base URL", "https://api.xiaomimimo.com/v1");
            println!("  API compatibility mode:");
            println!("    1) OpenAI-compatible (default)");
            println!("    2) Anthropic-compatible");
            let api_mode = prompt_choice("Select API mode", &["1", "2"], "1")?;
            let mode_str = if api_mode == "2" { "anthropic" } else { "openai" };
            env_lines.push(format!("MIMO_API_KEY={}", api_key));
            env_lines.push(format!("MIMO_MODEL={}", model));
            env_lines.push(format!("MIMO_API_BASE={}", base_url));
            env_lines.push(format!("MIMO_API_MODE={}", mode_str));
            "mimo"
        }
        _ => unreachable!(),
    };

    env_lines.push(format!("USE_PROVIDER={}", provider_name));
    println!();

    // Discord (optional)
    println!("--- Discord Bot (Optional) ---");
    println!("Do you want to configure Discord?");
    let setup_discord = prompt_yes_no("Setup Discord?", false)?;

    if setup_discord {
        let bot_token = prompt_required("Discord Bot Token")?;
        let app_id = prompt_required("Discord Application ID")?;
        env_lines.push(format!("DISCORD_BOT_TOKEN={}", bot_token));
        env_lines.push(format!("DISCORD_APPLICATION_ID={}", app_id));
    }
    println!();

    // Gateway
    println!("--- Gateway ---");
    let gateway_port = prompt_with_default("Gateway Port", "3537");
    let gateway_api_key = prompt_with_default_or_generate("Gateway API Key", 32);
    env_lines.push(format!("GATEWAY_PORT={}", gateway_port));
    env_lines.push(format!("GATEWAY_API_KEY={}", gateway_api_key));
    println!();

    // Dashboard
    println!("--- Dashboard ---");
    let dashboard_port = prompt_with_default("Dashboard Port", "1337");
    let admin_password = prompt_password("Dashboard Admin Password")?;
    env_lines.push(format!("DASHBOARD_PORT={}", dashboard_port));
    env_lines.push(format!("DASHBOARD_ADMIN_PASSWORD={}", admin_password));
    println!();

    // Master password for encrypted secrets
    println!("--- Encryption ---");
    println!("All secrets will be encrypted with a master password (enc2).");
    let master_password = prompt_password("Set MASTER_KEY password")?;
    println!();

    // Data directory
    println!("--- Data Storage ---");
    let data_dir = prompt_with_default("Data Directory", "./data");
    env_lines.push(format!("DATA_DIR={}", data_dir));
    println!();

    // Logging
    println!("--- Logging ---");
    println!("Log levels: trace, debug, info, warn, error");
    let log_level = prompt_with_default("Log Level", "info");
    env_lines.push(format!("RUST_LOG={}", log_level));
    println!();

    // Voice (optional)
    println!("--- Voice (Optional) ---");
    println!("Do you want to configure voice features?");
    let setup_voice = prompt_yes_no("Setup Voice?", false)?;

    if setup_voice {
        println!("Speech-to-Text engine:");
        println!("  1) Vosk (local, free)");
        println!("  2) Whisper (local, free)");
        println!("  3) ElevenLabs (cloud, paid)");
        let stt_choice = prompt_choice("Select STT", &["1", "2", "3"], "1")?;
        let stt_type = match stt_choice.as_str() {
            "1" => "vosk",
            "2" => "whisper",
            "3" => "elevenlabs",
            _ => "vosk",
        };
        env_lines.push(format!("VOICE_STT_TYPE={}", stt_type));

        if stt_type == "vosk" {
            let model_path = prompt_with_default("Vosk Model Path", "./models/vosk-model-small-de");
            env_lines.push(format!("VOSK_MODEL_PATH={}", model_path));
        } else if stt_type == "whisper" {
            let model_path = prompt_with_default("Whisper Model Path", "./models/ggml-tiny.en.bin");
            env_lines.push(format!("WHISPER_MODEL_PATH={}", model_path));
        } else if stt_type == "elevenlabs" {
            let api_key = prompt_required("ElevenLabs API Key")?;
            env_lines.push(format!("ELEVENLABS_API_KEY={}", api_key));
        }

        println!();
        println!("Text-to-Speech engine:");
        println!("  1) Windows SAPI (local, free)");
        println!("  2) ElevenLabs (cloud, paid)");
        println!("  3) Qwen TTS (local, free)");
        let tts_choice = prompt_choice("Select TTS", &["1", "2", "3"], "1")?;
        let tts_type = match tts_choice.as_str() {
            "1" => "windows_sapi",
            "2" => "elevenlabs",
            "3" => "qwen_tts",
            _ => "windows_sapi",
        };
        env_lines.push(format!("VOICE_TTS_TYPE={}", tts_type));

        if tts_type == "elevenlabs" {
            if !env_lines.iter().any(|l| l.starts_with("ELEVENLABS_API_KEY=")) {
                let api_key = prompt_required("ElevenLabs API Key")?;
                env_lines.push(format!("ELEVENLABS_API_KEY={}", api_key));
            }
            let voice_id = prompt_with_default("ElevenLabs Voice ID", "21m00Tcm4TlvDq8ikWAM");
            env_lines.push(format!("ELEVENLABS_VOICE_ID={}", voice_id));
        } else if tts_type == "qwen_tts" {
            let server = prompt_with_default("Qwen TTS Server URL", "http://localhost:8001");
            env_lines.push(format!("QWEN_TTS_SERVER={}", server));
        }
    }
    println!();

    // Write .env file
    println!("--- Saving Configuration ---");
    let env_path = ".env";

    let mut content = String::new();
    content.push_str("# Praxis AI Agent Platform Configuration\n");
    content.push_str("# Generated by: praxis onboard --interactive\n\n");

    content.push_str("# LLM Provider\n");
    for line in &env_lines {
        if line.starts_with("USE_PROVIDER=")
            || line.starts_with("OPENAI_")
            || line.starts_with("ANTHROPIC_")
            || line.starts_with("OLLAMA_")
            || line.starts_with("MINIMAX_")
            || line.starts_with("MIMO_")
        {
            content.push_str(line);
            content.push('\n');
        }
    }
    content.push('\n');

    content.push_str("# Discord\n");
    for line in &env_lines {
        if line.starts_with("DISCORD_") {
            content.push_str(line);
            content.push('\n');
        }
    }
    content.push('\n');

    content.push_str("# Gateway\n");
    for line in &env_lines {
        if line.starts_with("GATEWAY_") {
            content.push_str(line);
            content.push('\n');
        }
    }
    content.push('\n');

    content.push_str("# Dashboard\n");
    for line in &env_lines {
        if line.starts_with("DASHBOARD_") {
            content.push_str(line);
            content.push('\n');
        }
    }
    content.push('\n');

    content.push_str("# Data\n");
    for line in &env_lines {
        if line.starts_with("DATA_DIR=") || line.starts_with("RUST_LOG=") {
            content.push_str(line);
            content.push('\n');
        }
    }
    content.push('\n');

    content.push_str("# Voice\n");
    for line in &env_lines {
        if line.starts_with("VOICE_")
            || line.starts_with("VOSK_")
            || line.starts_with("WHISPER_")
            || line.starts_with("ELEVENLABS_")
            || line.starts_with("QWEN_")
        {
            content.push_str(line);
            content.push('\n');
        }
    }
    content.push('\n');

    content.push_str("# Plugins\n");
    for line in &env_lines {
        if line.starts_with("PLUGIN_") {
            content.push_str(line);
            content.push('\n');
        }
    }

    std::fs::write(env_path, content)?;

    println!("Configuration saved to {}", env_path);

    // Create directories
    std::fs::create_dir_all("templates")?;
    std::fs::create_dir_all("contextlanguage")?;
    std::fs::create_dir_all("data")?;
    std::fs::create_dir_all("skills")?;
    std::fs::create_dir_all("plugins")?;
    println!("Created directories: templates/, contextlanguage/, data/, skills/, plugins/");

    // Ask about MiniMax image plugin
    if provider_name == "minimax" || provider_name == "mimo" {
        println!();
        println!("--- Plugins ---");
        println!("The MiniMax Image plugin provides image generation and analysis tools.");
        let install_plugin = prompt_yes_no("Install MiniMax Image plugin?", true)?;
        if install_plugin {
            env_lines.push("PLUGIN_MINIMAX_IMAGE=true".to_string());
            println!("MiniMax Image plugin will be enabled.");
        }
    }

    // Create database
    let data_dir = env_lines.iter()
        .find(|l| l.starts_with("DATA_DIR="))
        .map(|l| l.strip_prefix("DATA_DIR=").unwrap_or("./data"))
        .unwrap_or("./data");
    let _db = crate::db::Database::new(std::path::Path::new(data_dir))?;
    println!("Database initialized");

    // Save all secrets to encrypted storage (enc2)
    let secrets = crate::db::secrets::Secrets {
        discord_bot_token: env_lines.iter()
            .find(|l| l.starts_with("DISCORD_BOT_TOKEN="))
            .map(|l| l.strip_prefix("DISCORD_BOT_TOKEN=").unwrap_or("").to_string()),
        openai_api_key: env_lines.iter()
            .find(|l| l.starts_with("OPENAI_API_KEY="))
            .map(|l| l.strip_prefix("OPENAI_API_KEY=").unwrap_or("").to_string()),
        anthropic_api_key: env_lines.iter()
            .find(|l| l.starts_with("ANTHROPIC_API_KEY="))
            .map(|l| l.strip_prefix("ANTHROPIC_API_KEY=").unwrap_or("").to_string()),
        minimax_api_key: env_lines.iter()
            .find(|l| l.starts_with("MINIMAX_API_KEY="))
            .map(|l| l.strip_prefix("MINIMAX_API_KEY=").unwrap_or("").to_string()),
        mimo_api_key: env_lines.iter()
            .find(|l| l.starts_with("MIMO_API_KEY="))
            .map(|l| l.strip_prefix("MIMO_API_KEY=").unwrap_or("").to_string()),
        elevenlabs_api_key: env_lines.iter()
            .find(|l| l.starts_with("ELEVENLABS_API_KEY="))
            .map(|l| l.strip_prefix("ELEVENLABS_API_KEY=").unwrap_or("").to_string()),
        gateway_api_key: Some(gateway_api_key.clone()),
        dashboard_admin_password: Some(admin_password.clone()),
        ..Default::default()
    };
    crate::db::secrets::save_secrets(&secrets, &master_password)?;
    println!("Secrets encrypted and saved to secrets.enc2");

    // Remove sensitive values from .env since they are now in enc2
    let env_path = ".env";
    let env_content = std::fs::read_to_string(env_path)?;
    let filtered: String = env_content.lines()
        .filter(|line| {
            !line.starts_with("OPENAI_API_KEY=")
                && !line.starts_with("ANTHROPIC_API_KEY=")
                && !line.starts_with("MINIMAX_API_KEY=")
                && !line.starts_with("MIMO_API_KEY=")
                && !line.starts_with("ELEVENLABS_API_KEY=")
                && !line.starts_with("DISCORD_BOT_TOKEN=")
                && !line.starts_with("GATEWAY_API_KEY=")
                && !line.starts_with("DASHBOARD_ADMIN_PASSWORD=")
        })
        .collect::<Vec<_>>()
        .join("\n");
    std::fs::write(env_path, filtered)?;
    println!("Sensitive values removed from .env (stored in secrets.enc2 instead)");

    println!();
    println!("========================================");
    println!("   Setup Complete!");
    println!("========================================");
    println!();
    println!("Next steps:");
    println!("  1. Review your .env file");
    println!("  2. Run: praxis run");
    println!("  3. Enter your MASTER_KEY when prompted to unlock secrets");
    println!();

    Ok(())
}

fn prompt(message: &str) -> anyhow::Result<String> {
    print!("{}: ", message);
    io::stdout().flush()?;
    let mut input = String::new();
    io::stdin().read_line(&mut input)?;
    Ok(input.trim().to_string())
}

fn prompt_required(message: &str) -> anyhow::Result<String> {
    loop {
        let value = prompt(message)?;
        if !value.is_empty() {
            return Ok(value);
        }
        println!("  This field is required.");
    }
}

fn prompt_with_default(message: &str, default: &str) -> String {
    print!("{} [{}]: ", message, default);
    io::stdout().flush().unwrap();
    let mut input = String::new();
    io::stdin().read_line(&mut input).unwrap();
    let trimmed = input.trim();
    if trimmed.is_empty() {
        default.to_string()
    } else {
        trimmed.to_string()
    }
}

fn prompt_with_default_or_generate(message: &str, length: usize) -> String {
    print!("{} [press Enter to generate]: ", message);
    io::stdout().flush().unwrap();
    let mut input = String::new();
    io::stdin().read_line(&mut input).unwrap();
    let trimmed = input.trim();
    if trimmed.is_empty() {
        generate_random_key(length)
    } else {
        trimmed.to_string()
    }
}

fn prompt_password(message: &str) -> anyhow::Result<String> {
    loop {
        let password = rpassword::prompt_password(format!("{}: ", message))?;
        if password.len() < 8 {
            println!("  Password must be at least 8 characters.");
            continue;
        }
        let confirm = rpassword::prompt_password("Confirm password: ")?;
        if password == confirm {
            return Ok(password);
        }
        println!("  Passwords do not match. Try again.");
    }
}

fn prompt_yes_no(message: &str, default: bool) -> anyhow::Result<bool> {
    let default_str = if default { "Y/n" } else { "y/N" };
    print!("{} [{}]: ", message, default_str);
    io::stdout().flush()?;
    let mut input = String::new();
    io::stdin().read_line(&mut input)?;
    let trimmed = input.trim().to_lowercase();
    if trimmed.is_empty() {
        Ok(default)
    } else {
        Ok(trimmed == "y" || trimmed == "yes")
    }
}

fn prompt_choice(message: &str, options: &[&str], default: &str) -> anyhow::Result<String> {
    loop {
        print!("{} [{}]: ", message, default);
        io::stdout().flush()?;
        let mut input = String::new();
        io::stdin().read_line(&mut input)?;
        let trimmed = input.trim();
        if trimmed.is_empty() {
            return Ok(default.to_string());
        }
        if options.contains(&trimmed) {
            return Ok(trimmed.to_string());
        }
        println!("  Invalid choice. Please select one of: {:?}", options);
    }
}

fn generate_random_key(length: usize) -> String {
    use rand::Rng;
    let mut rng = rand::thread_rng();
    let chars: Vec<char> = "abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789"
        .chars()
        .collect();
    let key: String = (0..length)
        .map(|_| chars[rng.gen_range(0..chars.len())])
        .collect();
    println!("  Generated key: {}", key);
    key
}

#[cfg(test)]
mod security_tests {
    use super::*;

    #[test]
    fn test_generate_random_key() {
        let key = generate_random_key(32);
        assert_eq!(key.len(), 32);
    }

    #[test]
    fn test_generate_random_key_different() {
        let key1 = generate_random_key(16);
        let key2 = generate_random_key(16);
        assert_ne!(key1, key2);
    }
}

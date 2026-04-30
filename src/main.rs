use anyhow::Context;
use clap::Parser;
use std::io::{self, Write};
use std::path::Path;

#[derive(Parser)]
#[command(name = "praxis")]
#[command(about = "AI Agent Platform — Praxis")]
#[command(subcommand_required = false)]
enum Cli {
    /// Start all services
    Run {
        /// Master key password for encrypted secrets (non-interactive mode)
        #[arg(long)]
        password: Option<String>,
        /// Enable Discord bot
        #[arg(long)]
        discord: bool,
        /// Enable dashboard
        #[arg(long)]
        dashboard: bool,
    },
    /// Pair a Discord user with a pairing code
    Pair {
        #[arg(value_name = "CODE")]
        code: String,
    },
    /// Interactive onboarding setup
    Onboard {
        #[arg(long)]
        interactive: bool,
    },
}

#[tokio::main]
async fn main() {
    if let Err(e) = run().await {
        eprintln!("Error: {:#}", e);
        std::process::exit(1);
    }
}

async fn run() -> anyhow::Result<()> {
    dotenvy::dotenv().ok();

    let env_filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info"));

    tracing_subscriber::fmt()
        .with_env_filter(env_filter)
        .init();

    let cli = Cli::parse();

    match cli {
        Cli::Run { password, discord, dashboard } => run_services(password, discord, dashboard).await,
        Cli::Pair { code } => pair_command(&code).await,
        Cli::Onboard { interactive: true } => onboard_command(),
        Cli::Onboard { interactive: false } => {
            anyhow::bail!("Onboard requires --interactive flag");
        }
    }
}

async fn run_services(cli_password: Option<String>, enable_discord: bool, enable_dashboard: bool) -> anyhow::Result<()> {
    // Check if we need master key for encrypted secrets
    let master_password = if praxis::db::secrets::has_secrets() {
        let resolved = cli_password.or_else(|| std::env::var("MASTER_KEY").ok());
        if let Some(password) = resolved {
            if !praxis::db::enc2::verify_password(&password) {
                anyhow::bail!("Invalid MASTER_KEY");
            }
            Some(password)
        } else {
            let password = rpassword::prompt_password("Enter MASTER_KEY to unlock secrets: ")
                .map_err(|e| anyhow::anyhow!("Failed to read password: {}", e))?;
            if !praxis::db::enc2::verify_password(&password) {
                anyhow::bail!("Invalid MASTER_KEY");
            }
            Some(password)
        }
    } else {
        None
    };

    let data_dir = std::env::var("DATA_DIR")
        .unwrap_or_else(|_| "./data".to_string());
    let db = praxis::db::Database::new(Path::new(&data_dir))?;

    tracing::info!("Starting Praxis v{}...", env!("CARGO_PKG_VERSION"));

    // Load secrets (encrypted or plaintext)
    let secrets = if let Some(ref password) = master_password {
        praxis::db::secrets::load_secrets_with_password(password)?
    } else {
        praxis::db::secrets::Secrets {
            discord_bot_token: std::env::var("DISCORD_BOT_TOKEN").ok(),
            openai_api_key: std::env::var("OPENAI_API_KEY").ok(),
            anthropic_api_key: std::env::var("ANTHROPIC_API_KEY").ok(),
            minimax_api_key: std::env::var("MINIMAX_API_KEY").ok(),
            mimo_api_key: std::env::var("MIMO_API_KEY").ok(),
            elevenlabs_api_key: std::env::var("ELEVENLABS_API_KEY").ok(),
            gateway_api_key: std::env::var("GATEWAY_API_KEY").ok(),
            ..Default::default()
        }
    };

    // Initialize global secrets
    praxis::db::secrets::init_secrets(secrets.clone());

    let config = praxis::config::Config::from_env();
    config.validate()?;

    // Gateway
    let gateway_db = db.clone();
    let gateway_config = config.clone();
    let gateway_handle = tokio::spawn(async move {
        if let Err(e) = praxis::gateway::start(gateway_db, gateway_config).await {
            tracing::error!("Gateway error: {}", e);
        }
    });

    // Dashboard (optional)
    if enable_dashboard {
        let dashboard_db = db.clone();
        let dashboard_port = config.dashboard_port;
        tokio::spawn(async move {
            let server = praxis::dashboard::DashboardServer::new(dashboard_port, dashboard_db);
            if let Err(e) = server.start().await {
                tracing::error!("Dashboard error: {}", e);
            }
        });
        tracing::info!("Dashboard starting on port {}", config.dashboard_port);
    }

    // Discord (optional)
    if enable_discord {
        let discord_db = db.clone();
        let discord_secrets = secrets.clone();
        tokio::spawn(async move {
            if let Err(e) = praxis::discord::start_with_secrets(discord_db, discord_secrets).await {
                tracing::error!("Discord error: {}", e);
            }
        });
        tracing::info!("Discord bot starting...");
    }

    tracing::info!("Praxis started successfully");
    gateway_handle.await?;

    Ok(())
}

async fn pair_command(code: &str) -> anyhow::Result<()> {
    dotenvy::dotenv().ok();

    let config = praxis::config::Config::from_env();
    let db = praxis::db::Database::new(Path::new(&config.data_dir))?;

    match db.get_pending_pairing(code) {
        Ok(Some(pending)) => {
            let user_id = uuid::Uuid::new_v4().to_string();
            db.create_pairing(&user_id, &pending.discord_user_id, None)?;
            db.delete_pending_pairing(code)?;
            println!("Pairing successful!");
            println!("User ID: {}", user_id);
        }
        Ok(None) => {
            println!("Invalid or expired pairing code");
        }
        Err(e) => {
            println!("Error: {}", e);
        }
    }

    Ok(())
}

fn onboard_command() -> anyhow::Result<()> {
    println!("=== Praxis AI Agent Platform - Interactive Setup ===\n");

    let poml_cli = prompt("POML CLI path", Some("./poml/js/cli.cjs".to_string()), |input| {
        if input.is_empty() {
            return Ok("./poml/js/cli.cjs".to_string());
        }
        let path = std::path::Path::new(&input);
        if !path.exists() {
            anyhow::bail!("File not found: {}", input);
        }
        Ok(input.to_string())
    })?;

    let use_provider = prompt("USE_PROVIDER (minimax or mimo)", Some("minimax".to_string()), |input| {
        let val = if input.is_empty() { "minimax".to_string() } else { input.to_lowercase() };
        if val != "minimax" && val != "mimo" && val != "openai" && val != "anthropic" && val != "ollama" {
            anyhow::bail!("USE_PROVIDER must be 'minimax', 'mimo', 'openai', 'anthropic', or 'ollama'");
        }
        Ok(val)
    })?;

    let mut secrets = praxis::db::secrets::Secrets::default();
    let mut env_lines: Vec<String> = Vec::new();

    env_lines.push(format!("POML_CLI={}", poml_cli));
    env_lines.push(format!("USE_PROVIDER={}", use_provider));

    match use_provider.as_str() {
        "minimax" => {
            let key = prompt("MINIMAX_API_KEY", None, |input| {
                if input.is_empty() { anyhow::bail!("MINIMAX_API_KEY is required"); }
                Ok(input.to_string())
            })?;
            let model = prompt("MINIMAX_MODEL", Some("MiniMax-Text-01".to_string()), |input| {
                Ok(if input.is_empty() { "MiniMax-Text-01".to_string() } else { input.to_string() })
            })?;
            let base = prompt("MINIMAX_API_BASE", Some("https://api.minimax.chat/v1".to_string()), |input| {
                Ok(if input.is_empty() { "https://api.minimax.chat/v1".to_string() } else { input.to_string() })
            })?;
            secrets.minimax_api_key = Some(key);
            env_lines.push(format!("MINIMAX_MODEL={}", model));
            env_lines.push(format!("MINIMAX_API_BASE={}", base));
        }
        "mimo" => {
            let key = prompt("MIMO_API_KEY", None, |input| {
                if input.is_empty() { anyhow::bail!("MIMO_API_KEY is required"); }
                Ok(input.to_string())
            })?;
            let model = prompt("MIMO_MODEL", Some("mimo-v2.5-pro".to_string()), |input| {
                Ok(if input.is_empty() { "mimo-v2.5-pro".to_string() } else { input.to_string() })
            })?;
            let base = prompt("MIMO_API_BASE", Some("https://api.xiaomimimo.com/v1".to_string()), |input| {
                Ok(if input.is_empty() { "https://api.xiaomimimo.com/v1".to_string() } else { input.to_string() })
            })?;
            secrets.mimo_api_key = Some(key);
            env_lines.push(format!("MIMO_MODEL={}", model));
            env_lines.push(format!("MIMO_API_BASE={}", base));
        }
        "openai" => {
            let key = prompt("OPENAI_API_KEY", None, |input| {
                if input.is_empty() { anyhow::bail!("OPENAI_API_KEY is required"); }
                Ok(input.to_string())
            })?;
            let model = prompt("OPENAI_MODEL", Some("gpt-4o".to_string()), |input| {
                Ok(if input.is_empty() { "gpt-4o".to_string() } else { input.to_string() })
            })?;
            let base = prompt("OPENAI_API_BASE", Some("https://api.openai.com/v1".to_string()), |input| {
                Ok(if input.is_empty() { "https://api.openai.com/v1".to_string() } else { input.to_string() })
            })?;
            secrets.openai_api_key = Some(key);
            env_lines.push(format!("OPENAI_MODEL={}", model));
            env_lines.push(format!("OPENAI_API_BASE={}", base));
        }
        "anthropic" => {
            let key = prompt("ANTHROPIC_API_KEY", None, |input| {
                if input.is_empty() { anyhow::bail!("ANTHROPIC_API_KEY is required"); }
                Ok(input.to_string())
            })?;
            let model = prompt("ANTHROPIC_MODEL", Some("claude-3-5-sonnet-20241022".to_string()), |input| {
                Ok(if input.is_empty() { "claude-3-5-sonnet-20241022".to_string() } else { input.to_string() })
            })?;
            let base = prompt("ANTHROPIC_API_BASE", Some("https://api.anthropic.com".to_string()), |input| {
                Ok(if input.is_empty() { "https://api.anthropic.com".to_string() } else { input.to_string() })
            })?;
            secrets.anthropic_api_key = Some(key);
            env_lines.push(format!("ANTHROPIC_MODEL={}", model));
            env_lines.push(format!("ANTHROPIC_API_BASE={}", base));
        }
        "ollama" => {
            let base = prompt("OLLAMA_API_BASE", Some("http://localhost:11434".to_string()), |input| {
                Ok(if input.is_empty() { "http://localhost:11434".to_string() } else { input.to_string() })
            })?;
            let model = prompt("OLLAMA_MODEL", Some("llama3".to_string()), |input| {
                Ok(if input.is_empty() { "llama3".to_string() } else { input.to_string() })
            })?;
            env_lines.push(format!("OLLAMA_API_BASE={}", base));
            env_lines.push(format!("OLLAMA_MODEL={}", model));
        }
        _ => unreachable!(),
    }

    // Discord
    let discord_bot_token = prompt("DISCORD_BOT_TOKEN", None, |input| {
        if input.is_empty() { anyhow::bail!("DISCORD_BOT_TOKEN is required"); }
        Ok(input.to_string())
    })?;
    let discord_application_id = prompt("DISCORD_APPLICATION_ID", None, |input| {
        if input.is_empty() { anyhow::bail!("DISCORD_APPLICATION_ID is required"); }
        if input.parse::<u64>().is_err() { anyhow::bail!("DISCORD_APPLICATION_ID must be a number"); }
        Ok(input.to_string())
    })?;
    secrets.discord_bot_token = Some(discord_bot_token);
    env_lines.push(format!("DISCORD_APPLICATION_ID={}", discord_application_id));

    let data_dir = prompt("DATA_DIR", Some("./data".to_string()), |input| {
        Ok(if input.is_empty() { "./data".to_string() } else { input.to_string() })
    })?;
    env_lines.push(format!("DATA_DIR={}", data_dir));

    // Master key for encrypted secrets
    println!("\n=== Secret Store Setup ===");
    println!("API keys will be encrypted with a master key.");
    println!("You'll need this key to unlock secrets at startup.");

    let master_key = loop {
        let password = rpassword::prompt_password("MASTER_KEY (min 8 chars): ")
            .map_err(|e| anyhow::anyhow!("Failed to read password: {}", e))?;
        if password.len() < 8 {
            println!("MASTER_KEY must be at least 8 characters");
            continue;
        }
        let confirm = rpassword::prompt_password("Confirm MASTER_KEY: ")
            .map_err(|e| anyhow::anyhow!("Failed to read password: {}", e))?;
        if password != confirm {
            println!("MASTER_KEYs do not match, try again");
            continue;
        }
        break password;
    };

    println!("\n=== Configuration Summary ===");
    println!("USE_PROVIDER: {}", use_provider);
    println!("DISCORD_APPLICATION_ID: {}", discord_application_id);
    println!("DATA_DIR: {}", data_dir);
    println!("MASTER_KEY: ****");

    let confirm = prompt("\nSave configuration? (y/N)", None, |input| {
        Ok(input.to_lowercase())
    })?;

    if confirm == "y" || confirm == "yes" {
        // Save non-secret config to .env
        let env_content: String = env_lines.iter()
            .map(|l| format!("{}\n", l))
            .collect();
        std::fs::write(".env", env_content)?;
        println!(".env file created (non-secret config only)");

        // Save secrets to encrypted store
        praxis::db::secrets::save_secrets(&secrets, &master_key)?;
        println!("Secrets encrypted and saved to secrets.enc2");

        // Create directories
        std::fs::create_dir_all("templates")?;
        std::fs::create_dir_all("contextlanguage")?;
        std::fs::create_dir_all("data")?;
        println!("Created directories: templates/, contextlanguage/, data/");

        // Create database
        let _db = praxis::db::Database::new(Path::new(&data_dir))?;

        println!("\nSetup complete! Run 'praxis run' or 'praxis run --password <key>' to start.");
    } else {
        println!("\nConfiguration not saved.");
    }

    Ok(())
}

fn prompt<F>(message: &str, default: Option<String>, validator: F) -> anyhow::Result<String>
where
    F: Fn(&str) -> anyhow::Result<String>,
{
    loop {
        print!("{}: ", message);
        if let Some(ref d) = default {
            print!(" [{}]", d);
        }
        print!(": ");
        io::stdout().flush().unwrap();

        let mut input = String::new();
        io::stdin().read_line(&mut input).unwrap();
        let input = input.trim().to_string();

        match validator(&input) {
            Ok(value) => {
                println!();
                return Ok(value);
            }
            Err(e) => {
                println!("Error: {}\n", e);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cli_parsing_run() {
        let args = vec!["praxis", "run"];
        let cli = Cli::try_parse_from(args).unwrap();
        assert!(matches!(cli, Cli::Run { .. }));
    }

    #[test]
    fn test_cli_parsing_run_with_flags() {
        let args = vec!["praxis", "run", "--discord", "--dashboard"];
        let cli = Cli::try_parse_from(args).unwrap();
        match cli {
            Cli::Run { discord, dashboard, .. } => {
                assert!(discord);
                assert!(dashboard);
            }
            _ => panic!("Expected Run variant"),
        }
    }

    #[test]
    fn test_cli_parsing_run_with_password() {
        let args = vec!["praxis", "run", "--password", "test123"];
        let cli = Cli::try_parse_from(args).unwrap();
        match cli {
            Cli::Run { password, .. } => {
                assert_eq!(password, Some("test123".to_string()));
            }
            _ => panic!("Expected Run variant"),
        }
    }

    #[test]
    fn test_cli_parsing_pair() {
        let args = vec!["praxis", "pair", "ABCD-1234"];
        let cli = Cli::try_parse_from(args).unwrap();
        match cli {
            Cli::Pair { code } => assert_eq!(code, "ABCD-1234"),
            _ => panic!("Expected Pair variant"),
        }
    }

    #[test]
    fn test_cli_parsing_onboard() {
        let args = vec!["praxis", "onboard", "--interactive"];
        let cli = Cli::try_parse_from(args).unwrap();
        match cli {
            Cli::Onboard { interactive } => assert!(interactive),
            _ => panic!("Expected Onboard variant"),
        }
    }
}

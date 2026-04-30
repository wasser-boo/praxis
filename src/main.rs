use clap::Parser;
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
        /// Disable Discord bot
        #[arg(long)]
        no_discord: bool,
        /// Disable dashboard
        #[arg(long)]
        no_dashboard: bool,
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
        Cli::Run { password, no_discord, no_dashboard } => run_services(password, !no_discord, !no_dashboard).await,
        Cli::Pair { code } => pair_command(&code).await,
        Cli::Onboard { interactive: true } => praxis::onboard::run_interactive_onboard(),
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
            dashboard_admin_password: std::env::var("DASHBOARD_ADMIN_PASSWORD").ok(),
            ..Default::default()
        }
    };

    // Initialize global secrets
    praxis::db::secrets::init_secrets(secrets.clone());

    // Build config, overriding sensitive fields from secrets if available
    let mut config = praxis::config::Config::from_env();
    if let Some(ref key) = secrets.gateway_api_key {
        if !key.is_empty() {
            config.gateway_api_key = key.clone();
        }
    }
    if let Some(ref pass) = secrets.dashboard_admin_password {
        if !pass.is_empty() {
            config.dashboard_admin_password = pass.clone();
        }
    }
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
        let args = vec!["praxis", "run", "--no-discord", "--no-dashboard"];
        let cli = Cli::try_parse_from(args).unwrap();
        match cli {
            Cli::Run { no_discord, no_dashboard, .. } => {
                assert!(no_discord);
                assert!(no_dashboard);
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

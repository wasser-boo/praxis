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
    /// Manage the Praxis system service
    Service {
        #[command(subcommand)]
        action: ServiceAction,
    },
}

#[derive(clap::Subcommand)]
enum ServiceAction {
    /// Install Praxis as a system service (systemd)
    Install,
    /// Stop the Praxis service
    Stop,
    /// Start the Praxis service
    Start,
    /// Show Praxis service logs
    Logs {
        /// Follow logs in real-time
        #[arg(long, short)]
        follow: bool,
        /// Number of lines to show
        #[arg(long, short = 'n', default_value = "100")]
        lines: usize,
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

    let cli = Cli::parse();

    // Service commands don't need logging setup
    if let Cli::Service { action } = &cli {
        return handle_service_action(action).await;
    }

    // Set up logging with file rotation
    let log_dir = std::env::var("LOG_DIR").unwrap_or_else(|_| "./logs".to_string());
    let _ = std::fs::create_dir_all(&log_dir);

    let file_appender = tracing_appender::rolling::daily(&log_dir, "praxis.log");
    let (file_writer, _guard) = tracing_appender::non_blocking(file_appender);

    let env_filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info"));

    let stdout_layer = tracing_subscriber::fmt::layer()
        .with_writer(std::io::stdout);

    let file_layer = tracing_subscriber::fmt::layer()
        .with_writer(file_writer)
        .with_ansi(false);

    use tracing_subscriber::prelude::*;

    tracing_subscriber::registry()
        .with(env_filter)
        .with(stdout_layer)
        .with(file_layer)
        .init();

    match cli {
        Cli::Run { password, no_discord, no_dashboard } => run_services(password, !no_discord, !no_dashboard).await,
        Cli::Pair { code } => pair_command(&code).await,
        Cli::Onboard { interactive: true } => praxis::onboard::run_interactive_onboard(),
        Cli::Onboard { interactive: false } => {
            anyhow::bail!("Onboard requires --interactive flag");
        }
        Cli::Service { .. } => unreachable!(),
    }
}

async fn run_services(cli_password: Option<String>, enable_discord: bool, enable_dashboard: bool) -> anyhow::Result<()> {
    // Check if we need master key for encrypted secrets
    let master_password = if praxis::db::secrets::has_secrets() {
        let stored_hash = std::env::var("PRAXIS_MASTER_KEY_HASH").ok();

        let password = if let Some(pass) = cli_password.or_else(|| std::env::var("MASTER_KEY").ok()) {
            pass
        } else {
            rpassword::prompt_password("Enter MASTER_KEY to unlock secrets: ")
                .map_err(|e| anyhow::anyhow!("Failed to read password: {}", e))?
        };

        // If running as service with stored Argon2 hash, verify against hash first
        if let Some(ref hash) = stored_hash {
            if !praxis::db::enc2::verify_master_key_hash(&password, hash) {
                anyhow::bail!("Invalid MASTER_KEY (hash mismatch)");
            }
        } else {
            // Interactive mode: verify by attempting decryption
            if !praxis::db::enc2::verify_password(&password) {
                anyhow::bail!("Invalid MASTER_KEY");
            }
        }

        Some(password)
    } else {
        None
    };

    let data_dir = std::env::var("DATA_DIR")
        .unwrap_or_else(|_| "./data".to_string());
    let db = praxis::db::Database::new(Path::new(&data_dir))?;

    tracing::info!("Starting Praxis v{}...", env!("CARGO_PKG_VERSION"));

    // Initialize default tools if needed
    if let Err(e) = praxis::db::tools::init_default_tools(&db) {
        tracing::warn!("Failed to init default tools: {}", e);
    }

    // Sync templates from disk to database
    praxis::dashboard::routes::sync_templates_from_disk(&db);

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
    config.apply_secrets(&secrets);
    config.ensure_generated();
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

async fn handle_service_action(action: &ServiceAction) -> anyhow::Result<()> {
    let binary_path = std::env::current_exe()
        .map_err(|_| anyhow::anyhow!("Could not determine binary path"))?
        .to_string_lossy()
        .to_string();

    let working_dir = std::env::current_dir()
        .map_err(|_| anyhow::anyhow!("Could not determine working directory"))?
        .to_string_lossy()
        .to_string();

    match action {
        ServiceAction::Install => {
            println!("Enter the MASTER_KEY password for the service:");
            let password = rpassword::prompt_password("MASTER_KEY: ")
                .map_err(|e| anyhow::anyhow!("Failed to read password: {}", e))?;

            if password.is_empty() {
                anyhow::bail!("MASTER_KEY cannot be empty");
            }

            let hash = praxis::db::enc2::hash_master_key(&password);

            let service_content = format!(
                r#"[Unit]
Description=Praxis AI Agent Platform
After=network.target

[Service]
Type=simple
User={user}
WorkingDirectory={working_dir}
ExecStart={binary_path} run
Restart=on-failure
RestartSec=5
Environment=PRAXIS_MASTER_KEY_HASH={hash}

[Install]
WantedBy=multi-user.target
"#,
                user = std::env::var("USER").unwrap_or_else(|_| "root".to_string()),
                working_dir = working_dir,
                binary_path = binary_path,
                hash = hash,
            );

            let service_path = "/etc/systemd/system/praxis.service";
            std::fs::write(service_path, service_content)?;

            std::process::Command::new("systemctl")
                .args(["daemon-reload"])
                .status()?;

            println!("Service installed at {}", service_path);
            println!("MASTER_KEY stored as Argon2 hash (not plaintext).");
            println!("To start: sudo systemctl enable praxis && sudo systemctl start praxis");
        }
        ServiceAction::Stop => {
            let status = std::process::Command::new("systemctl")
                .args(["stop", "praxis"])
                .status()?;
            if status.success() {
                println!("Praxis service stopped");
            } else {
                anyhow::bail!("Failed to stop service. Is it installed?");
            }
        }
        ServiceAction::Start => {
            let status = std::process::Command::new("systemctl")
                .args(["start", "praxis"])
                .status()?;
            if status.success() {
                println!("Praxis service started");
            } else {
                anyhow::bail!("Failed to start service. Is it installed?");
            }
        }
        ServiceAction::Logs { follow, lines } => {
            let mut args = vec!["-u", "praxis", "--no-pager"];
            if *follow {
                args.push("-f");
            }
            args.push("-n");
            let lines_str = lines.to_string();
            args.push(&lines_str);

            let status = std::process::Command::new("journalctl")
                .args(&args)
                .status()?;
            if !status.success() {
                anyhow::bail!("Failed to retrieve logs. Is the service installed?");
            }
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
            _ => panic!("Expected Run variant"),
        }
    }

    #[test]
    fn test_cli_parsing_service_install() {
        let args = vec!["praxis", "service", "install"];
        let cli = Cli::try_parse_from(args).unwrap();
        match cli {
            Cli::Service { action } => assert!(matches!(action, ServiceAction::Install)),
            _ => panic!("Expected Service variant"),
        }
    }

    #[test]
    fn test_cli_parsing_service_stop() {
        let args = vec!["praxis", "service", "stop"];
        let cli = Cli::try_parse_from(args).unwrap();
        match cli {
            Cli::Service { action } => assert!(matches!(action, ServiceAction::Stop)),
            _ => panic!("Expected Service variant"),
        }
    }

    #[test]
    fn test_cli_parsing_service_start() {
        let args = vec!["praxis", "service", "start"];
        let cli = Cli::try_parse_from(args).unwrap();
        match cli {
            Cli::Service { action } => assert!(matches!(action, ServiceAction::Start)),
            _ => panic!("Expected Service variant"),
        }
    }

    #[test]
    fn test_cli_parsing_service_logs_default() {
        let args = vec!["praxis", "service", "logs"];
        let cli = Cli::try_parse_from(args).unwrap();
        match cli {
            Cli::Service { action } => match action {
                ServiceAction::Logs { follow, lines } => {
                    assert!(!follow);
                    assert_eq!(lines, 100);
                }
                _ => panic!("Expected Logs variant"),
            },
            _ => panic!("Expected Service variant"),
        }
    }

    #[test]
    fn test_cli_parsing_service_logs_follow() {
        let args = vec!["praxis", "service", "logs", "-f", "-n", "50"];
        let cli = Cli::try_parse_from(args).unwrap();
        match cli {
            Cli::Service { action } => match action {
                ServiceAction::Logs { follow, lines } => {
                    assert!(follow);
                    assert_eq!(lines, 50);
                }
                _ => panic!("Expected Logs variant"),
            },
            _ => panic!("Expected Service variant"),
        }
    }
}


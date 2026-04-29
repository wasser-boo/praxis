use clap::Parser;
use praxis::config::Config;
use praxis::db::Database;
use std::path::Path;

#[derive(Parser)]
#[command(name = "praxis")]
#[command(about = "AI Agent Platform — Praxis")]
#[command(subcommand_required = false)]
enum Cli {
    /// Start all services
    Run {
        #[arg(long)]
        password: Option<String>,
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

    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    let cli = Cli::parse();

    match cli {
        Cli::Run { password } => {
            let config = Config::from_env();
            config.validate()?;

            let db = Database::new(Path::new(&config.data_dir))?;

            tracing::info!("Starting Praxis v{}...", env!("CARGO_PKG_VERSION"));

            let gateway_db = db.clone();
            let gateway_config = config.clone();

            let gateway_handle = tokio::spawn(async move {
                if let Err(e) = praxis::gateway::start(gateway_db, gateway_config).await {
                    tracing::error!("Gateway error: {}", e);
                }
            });

            tracing::info!("Praxis started successfully");
            gateway_handle.await?;
        }
        Cli::Pair { code } => {
            let config = Config::from_env();
            let db = Database::new(Path::new(&config.data_dir))?;

            match db.get_pending_pairing(&code) {
                Ok(Some(pending)) => {
                    let user_id = uuid::Uuid::new_v4().to_string();
                    db.create_pairing(&user_id, &pending.discord_user_id, None)?;
                    db.delete_pending_pairing(&code)?;
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
        }
        Cli::Onboard { interactive: true } => {
            praxis::onboard::run_interactive_onboard()?;
        }
        Cli::Onboard { interactive: false } => {
            anyhow::bail!("Onboard requires --interactive flag");
        }
    }

    Ok(())
}

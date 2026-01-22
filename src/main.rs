use clap::{Parser, Subcommand};
use std::io::Write;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

use manifest::api::{self, DeploymentMode};
use manifest::{db, mcp};

const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Print startup banner to the specified writer
fn print_banner<W: Write>(mut w: W, url: &str) {
    let banner = format!(
        r#"
  ◇ ○ ●  M A N I F E S T

  Living Feature Documentation
  v{} · {}
"#,
        VERSION, url
    );
    let _ = writeln!(w, "{}", banner);
}

#[derive(Parser)]
#[command(name = "manifest")]
#[command(version)]
#[command(about = "Living feature documentation for AI-assisted development")]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand)]
enum Commands {
    /// Start the Manifest server
    Serve {
        /// Port for HTTP API
        #[arg(short, long, default_value = "17010")]
        port: u16,

        /// Bind address (use 0.0.0.0 for remote/container deployment)
        #[arg(short, long, default_value = "127.0.0.1")]
        bind: String,

        /// Run as daemon
        #[arg(short, long)]
        daemon: bool,
    },
    /// Start MCP server via stdio (for Claude Code integration)
    Mcp,
    /// Check server status
    Status,
    /// Stop the daemon
    Stop,
    /// Migrate existing projects to use root features
    MigrateRoots,
}

/// Initialize tracing with output to stderr (for MCP mode) or stdout
fn init_tracing(use_stderr: bool) {
    let filter = tracing_subscriber::EnvFilter::new(
        std::env::var("RUST_LOG").unwrap_or_else(|_| "manifest=debug,tower_http=debug".into()),
    );

    if use_stderr {
        // MCP mode: log to stderr so stdout is clean for protocol
        tracing_subscriber::registry()
            .with(filter)
            .with(tracing_subscriber::fmt::layer().with_writer(std::io::stderr))
            .init();
    } else {
        tracing_subscriber::registry()
            .with(filter)
            .with(tracing_subscriber::fmt::layer())
            .init();
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Load .env file if present (ignored if missing)
    dotenvy::dotenv().ok();

    let cli = Cli::parse();

    // MCP mode needs stderr for logging since stdout is the protocol channel
    let use_stderr = matches!(cli.command, Some(Commands::Mcp));
    init_tracing(use_stderr);

    match cli.command {
        Some(Commands::Serve {
            port,
            bind,
            daemon: _,
        }) => {
            // Allow env var override for container deployment
            let bind_addr = std::env::var("MANIFEST_BIND_ADDR").unwrap_or(bind);

            // Validate deployment mode BEFORE starting server (fail-secure)
            let mode = DeploymentMode::from_env().expect("Configuration validation failed");

            // Production safety: crash if local mode in production
            if mode == DeploymentMode::Local {
                let is_prod = std::env::var("FLY_APP_NAME").is_ok()
                    || std::env::var("RAILWAY_ENVIRONMENT").is_ok()
                    || std::env::var("RENDER").is_ok();
                if is_prod {
                    panic!("FATAL: MANIFEST_MODE=local is forbidden in production. Set MANIFEST_MODE=cloud.");
                }
            }

            print_banner(std::io::stdout(), &format!("http://{}:{}", bind_addr, port));
            tracing::info!("Starting Manifest server on {}:{}", bind_addr, port);
            tracing::info!("Running in {} mode", mode.as_str().to_uppercase());

            let db = db::Database::open_default().await?;
            db.migrate().await?;

            let app = if mode == DeploymentMode::Cloud {
                // Cloud mode: use Clerk authentication
                let verifier = api::ClerkVerifier::from_env()
                    .expect("Clerk configuration should be validated by DeploymentMode::from_env");
                api::create_router_with_clerk(db, verifier)
            } else {
                // Local mode: no authentication
                api::create_router(db)
            };

            let listener = tokio::net::TcpListener::bind(format!("{}:{}", bind_addr, port)).await?;
            tracing::info!("Manifest server listening on http://{}:{}", bind_addr, port);

            axum::serve(listener, app).await?;
        }
        Some(Commands::Mcp) => {
            // MCP server uses HTTP client to connect to the API
            // No local database needed - configure via MANIFEST_URL env var
            print_banner(std::io::stderr(), "MCP");
            mcp::run_stdio_server().await?;
        }
        Some(Commands::Status) => {
            println!("Checking Manifest server status...");
            // TODO: Check if server is running
        }
        Some(Commands::Stop) => {
            println!("Stopping Manifest server...");
            // TODO: Stop daemon
        }
        Some(Commands::MigrateRoots) => {
            println!("Migrating existing projects to use root features...");
            let db = db::Database::open_default().await?;
            db.migrate().await?;

            let report = db.migrate_to_root_features().await?;
            println!("Migration complete:");
            println!("  Projects migrated: {}", report.projects_migrated);
            println!("  Features reparented: {}", report.features_reparented);
            println!(
                "  Projects skipped (already migrated): {}",
                report.projects_skipped
            );
        }
        None => {
            // Default: start server
            let bind_addr =
                std::env::var("MANIFEST_BIND_ADDR").unwrap_or_else(|_| "127.0.0.1".into());
            let port: u16 = std::env::var("MANIFEST_PORT")
                .ok()
                .and_then(|p| p.parse().ok())
                .unwrap_or(17010);

            // Validate deployment mode BEFORE starting server (fail-secure)
            let mode = DeploymentMode::from_env().expect("Configuration validation failed");

            // Production safety: crash if local mode in production
            if mode == DeploymentMode::Local {
                let is_prod = std::env::var("FLY_APP_NAME").is_ok()
                    || std::env::var("RAILWAY_ENVIRONMENT").is_ok()
                    || std::env::var("RENDER").is_ok();
                if is_prod {
                    panic!("FATAL: MANIFEST_MODE=local is forbidden in production. Set MANIFEST_MODE=cloud.");
                }
            }

            print_banner(std::io::stdout(), &format!("http://{}:{}", bind_addr, port));
            tracing::info!("Starting Manifest server on {}:{}", bind_addr, port);
            tracing::info!("Running in {} mode", mode.as_str().to_uppercase());

            let db = db::Database::open_default().await?;
            db.migrate().await?;

            let app = if mode == DeploymentMode::Cloud {
                // Cloud mode: use Clerk authentication
                let verifier = api::ClerkVerifier::from_env()
                    .expect("Clerk configuration should be validated by DeploymentMode::from_env");
                api::create_router_with_clerk(db, verifier)
            } else {
                // Local mode: no authentication
                api::create_router(db)
            };

            let listener = tokio::net::TcpListener::bind(format!("{}:{}", bind_addr, port)).await?;
            tracing::info!("Manifest server listening on http://{}:{}", bind_addr, port);

            axum::serve(listener, app).await?;
        }
    }

    Ok(())
}

use clap::{Parser, Subcommand};
use std::io::Write;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

use manifest::{api, db, mcp};

const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Print startup banner to the specified writer
fn print_banner<W: Write>(mut w: W, mode: &str) {
    let banner = format!(
        r#"
  █▀▄▀█ ▄▀█ █▄ █ █ █▀▀ █▀▀ █▀ ▀█▀
  █ ▀ █ █▀█ █ ▀█ █ █▀  ██▄ ▄█  █

  Living Feature Documentation
  Version: {}
  Mode:    {}
"#,
        VERSION, mode
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

            print_banner(
                std::io::stdout(),
                &format!("HTTP Server ({}:{})", bind_addr, port),
            );
            tracing::info!("Starting Manifest server on {}:{}", bind_addr, port);

            let db = db::Database::open_default()?;
            db.migrate()?;

            let app = api::create_router(db);

            let listener = tokio::net::TcpListener::bind(format!("{}:{}", bind_addr, port)).await?;
            tracing::info!("Manifest server listening on http://{}:{}", bind_addr, port);

            axum::serve(listener, app).await?;
        }
        Some(Commands::Mcp) => {
            // MCP server uses HTTP client to connect to the API
            // No local database needed - configure via MANIFEST_URL env var
            let mode = if mcp::is_ide_mode() {
                "MCP (IDE)"
            } else {
                "MCP (CLI)"
            };
            print_banner(std::io::stderr(), mode);
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
            let db = db::Database::open_default()?;
            db.migrate()?;

            let report = db.migrate_to_root_features()?;
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

            print_banner(
                std::io::stdout(),
                &format!("HTTP Server ({}:{})", bind_addr, port),
            );
            tracing::info!("Starting Manifest server on {}:{}", bind_addr, port);

            let db = db::Database::open_default()?;
            db.migrate()?;

            let app = api::create_router(db);

            let listener = tokio::net::TcpListener::bind(format!("{}:{}", bind_addr, port)).await?;
            tracing::info!("Manifest server listening on http://{}:{}", bind_addr, port);

            axum::serve(listener, app).await?;
        }
    }

    Ok(())
}

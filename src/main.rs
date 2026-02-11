use clap::{Parser, Subcommand};
use std::io::Write;
use std::path::PathBuf;
use tokio::sync::watch;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

use manifest::api;
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
#[command(disable_version_flag = true)]
struct Cli {
    /// Print version
    #[arg(short = 'v', long = "version", action = clap::ArgAction::Version)]
    version: (),

    /// Path to the SQLite database file
    #[arg(long, global = true, env = "MANIFEST_DB")]
    db: Option<PathBuf>,

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

        /// Run as background daemon (not yet implemented)
        #[arg(short, long)]
        daemon: bool,
    },
    /// Start MCP server via stdio (for Claude Code integration)
    Mcp,
    /// Open the Manifest dashboard in the browser
    Open {
        /// Port the server is running on
        #[arg(short, long, default_value = "17010")]
        port: u16,
    },
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

/// Start the HTTP server with graceful shutdown support.
/// Returns `true` if the server should restart (e.g. after a settings change).
async fn run_server(
    bind_addr: String,
    port: u16,
    db_path: Option<PathBuf>,
    shutdown_rx: watch::Receiver<bool>,
) -> anyhow::Result<bool> {
    let database = db::Database::open_with_override(db_path).await?;
    database.migrate().await?;

    let app = api::create_router_with_shutdown(database);

    // Start the ACP idle session reaper (reaps agent processes idle > 30 min)
    api::start_session_reaper();

    let listener = tokio::net::TcpListener::bind(format!("{}:{}", bind_addr, port)).await?;
    tracing::info!("Manifest server listening on http://{}:{}", bind_addr, port);

    axum::serve(listener, app)
        .with_graceful_shutdown(async move {
            let mut rx = shutdown_rx;
            // Wait for the shutdown signal
            while !*rx.borrow_and_update() {
                if rx.changed().await.is_err() {
                    break;
                }
            }
            tracing::info!("Shutdown signal received, draining connections...");
        })
        .await?;

    Ok(true)
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

            print_banner(std::io::stdout(), &format!("http://{}:{}", bind_addr, port));
            tracing::info!("Starting Manifest server on {}:{}", bind_addr, port);

            let (shutdown_tx, shutdown_rx) = watch::channel(false);
            api::set_shutdown_sender(shutdown_tx);

            let should_restart = run_server(bind_addr, port, cli.db, shutdown_rx).await?;

            if should_restart {
                re_exec();
            }
        }
        Some(Commands::Mcp) => {
            // MCP server uses HTTP client to connect to the API
            // No local database needed - configure via MANIFEST_URL env var
            print_banner(std::io::stderr(), "MCP");
            mcp::run_stdio_server().await?;
        }
        Some(Commands::Open { port }) => {
            let url = format!("http://localhost:{}", port);

            // Check if server is running
            match reqwest::Client::new()
                .get(format!("{}/health", url))
                .timeout(std::time::Duration::from_secs(2))
                .send()
                .await
            {
                Ok(resp) if resp.status().is_success() => {
                    open::that(&url)?;
                    println!("Opened {}", url);
                }
                _ => {
                    eprintln!(
                        "Server is not running on port {}. Start it with: manifest serve",
                        port
                    );
                    std::process::exit(1);
                }
            }
        }
        Some(Commands::Status) => {
            eprintln!("Not implemented yet");
            std::process::exit(1);
        }
        Some(Commands::Stop) => {
            eprintln!("Not implemented yet");
            std::process::exit(1);
        }
        Some(Commands::MigrateRoots) => {
            println!("Migrating existing projects to use root features...");
            let database = db::Database::open_with_override(cli.db).await?;
            database.migrate().await?;

            let report = database.migrate_to_root_features().await?;
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

            print_banner(std::io::stdout(), &format!("http://{}:{}", bind_addr, port));
            tracing::info!("Starting Manifest server on {}:{}", bind_addr, port);

            let (shutdown_tx, shutdown_rx) = watch::channel(false);
            api::set_shutdown_sender(shutdown_tx);

            let should_restart = run_server(bind_addr, port, cli.db, shutdown_rx).await?;

            if should_restart {
                re_exec();
            }
        }
    }

    Ok(())
}

/// Re-execute the current process to restart with new configuration.
fn re_exec() -> ! {
    let exe = std::env::current_exe().expect("Failed to get current executable path");
    let args: Vec<String> = std::env::args().collect();

    tracing::info!("Re-executing server process...");

    // On Unix, use exec to replace the current process
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        let err = std::process::Command::new(&exe).args(&args[1..]).exec();
        // exec() only returns on error
        panic!("Failed to re-exec: {}", err);
    }

    #[cfg(not(unix))]
    {
        // On non-Unix, spawn a new process and exit
        let _ = std::process::Command::new(&exe)
            .args(&args[1..])
            .spawn()
            .expect("Failed to spawn new process");
        std::process::exit(0);
    }
}

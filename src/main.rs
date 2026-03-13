use clap::{Parser, Subcommand};
use std::io::Write;
use std::path::PathBuf;
use tokio::sync::watch;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

use manifest::api;
use manifest::{db, mcp};

const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Return the path to the PID file: `<data_dir>/manifest.pid`.
fn pid_file_path() -> Option<PathBuf> {
    if let Ok(data_dir) = std::env::var("MANIFEST_DATA_DIR") {
        Some(PathBuf::from(data_dir).join("manifest.pid"))
    } else {
        directories::ProjectDirs::from("", "", "manifest")
            .map(|dirs| dirs.data_dir().join("manifest.pid"))
    }
}

/// Write the current process PID to the PID file.
fn write_pid_file() {
    if let Some(path) = pid_file_path() {
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let _ = std::fs::write(&path, std::process::id().to_string());
    }
}

/// Remove the PID file.
fn remove_pid_file() {
    if let Some(path) = pid_file_path() {
        let _ = std::fs::remove_file(path);
    }
}

/// Read the PID from the PID file, returning None if missing or unreadable.
fn read_pid_file() -> Option<u32> {
    let path = pid_file_path()?;
    std::fs::read_to_string(path).ok()?.trim().parse().ok()
}

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
    /// Manage remote backends (Turso databases)
    Remote {
        #[command(subcommand)]
        action: RemoteAction,
    },
}

#[derive(Subcommand)]
enum RemoteAction {
    /// Add a new remote backend
    Add {
        /// Name for this remote (e.g., "work", "personal")
        name: String,
        /// Connection URL (e.g., libsql://mydb.turso.io)
        #[arg(long)]
        url: String,
        /// Auth token
        #[arg(long)]
        token: String,
        /// Backend provider
        #[arg(long, default_value = "turso")]
        provider: String,
    },
    /// Remove a remote backend
    Remove {
        /// Name of the remote to remove
        name: String,
    },
    /// List all configured remotes
    List,
    /// Update a remote's URL or token
    Update {
        /// Name of the remote to update
        name: String,
        /// New connection URL
        #[arg(long)]
        url: Option<String>,
        /// New auth token
        #[arg(long)]
        token: Option<String>,
    },
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
/// Returns `true` if the server should restart (e.g. after a settings change),
/// `false` if it received a termination signal.
async fn run_server(
    bind_addr: String,
    port: u16,
    db_path: Option<PathBuf>,
    shutdown_rx: watch::Receiver<bool>,
) -> anyhow::Result<bool> {
    let database = db::Database::open_with_override(db_path).await?;
    database.migrate().await?;

    let app = api::create_router(database);

    let listener = tokio::net::TcpListener::bind(format!("{}:{}", bind_addr, port)).await?;
    tracing::info!("Manifest server listening on http://{}:{}", bind_addr, port);

    write_pid_file();

    let should_restart = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let restart_flag = should_restart.clone();

    axum::serve(listener, app)
        .with_graceful_shutdown(async move {
            let mut rx = shutdown_rx;
            let mut sigterm =
                tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
                    .expect("failed to register SIGTERM handler");

            tokio::select! {
                _ = async {
                    while !*rx.borrow_and_update() {
                        if rx.changed().await.is_err() {
                            break;
                        }
                    }
                } => {
                    // Watch channel triggered (e.g. settings change) — restart
                    restart_flag.store(true, std::sync::atomic::Ordering::SeqCst);
                    tracing::info!("Shutdown signal received, draining connections...");
                }
                _ = sigterm.recv() => {
                    tracing::info!("SIGTERM received, draining connections...");
                }
            }
        })
        .await?;

    remove_pid_file();

    Ok(should_restart.load(std::sync::atomic::Ordering::SeqCst))
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
        Some(Commands::Serve { port, bind }) => {
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
            let port: u16 = std::env::var("MANIFEST_PORT")
                .ok()
                .and_then(|p| p.parse().ok())
                .unwrap_or(17010);

            let Some(pid) = read_pid_file() else {
                println!("Manifest server is not running.");
                std::process::exit(1);
            };

            let alive = std::process::Command::new("kill")
                .arg("-0")
                .arg(pid.to_string())
                .status()
                .is_ok_and(|s| s.success());

            if !alive {
                remove_pid_file();
                println!("Manifest server is not running.");
                std::process::exit(1);
            }

            // Process is alive — check the HTTP endpoint for version
            let url = format!("http://127.0.0.1:{}/api/v1/version", port);
            match reqwest::Client::new()
                .get(&url)
                .timeout(std::time::Duration::from_secs(2))
                .send()
                .await
            {
                Ok(resp) if resp.status().is_success() => {
                    if let Ok(body) = resp.json::<serde_json::Value>().await {
                        let version = body["version"].as_str().unwrap_or("unknown");
                        println!(
                            "Manifest server running (pid {}, port {}, v{}).",
                            pid, port, version
                        );
                    } else {
                        println!("Manifest server running (pid {}, port {}).", pid, port);
                    }
                }
                _ => {
                    println!(
                        "Manifest server process alive (pid {}) but not responding on port {}.",
                        pid, port
                    );
                }
            }
        }
        Some(Commands::Stop) => {
            if let Some(pid) = read_pid_file() {
                let status = std::process::Command::new("kill")
                    .arg("-0")
                    .arg(pid.to_string())
                    .status();
                let alive = matches!(status, Ok(s) if s.success());

                if alive {
                    let _ = std::process::Command::new("kill")
                        .arg(pid.to_string())
                        .status();
                    println!("Sent stop signal to Manifest server (pid {}).", pid);
                } else {
                    remove_pid_file();
                    eprintln!("Stale PID file (process {} not running). Cleaned up.", pid);
                    std::process::exit(1);
                }
            } else {
                eprintln!("Manifest server is not running (no PID file found).");
                std::process::exit(1);
            }
        }
        Some(Commands::Remote { action }) => {
            let database = db::Database::open_with_override(cli.db).await?;
            database.migrate().await?;

            match action {
                RemoteAction::Add {
                    name,
                    url,
                    token,
                    provider,
                } => {
                    let input = manifest_core::models::CreateRemoteInput {
                        name: name.clone(),
                        provider: Some(provider),
                        url,
                        token,
                    };
                    match database.create_remote(&input).await {
                        Ok(remote) => {
                            println!(
                                "Remote '{}' added (provider: {}).",
                                remote.name, remote.provider
                            );
                            println!("  URL: {}", remote.url);
                        }
                        Err(e) => {
                            eprintln!("Error: {}", e);
                            std::process::exit(1);
                        }
                    }
                }
                RemoteAction::Remove { name } => {
                    let Some(remote) = database.get_remote_by_name(&name).await? else {
                        eprintln!("Remote '{}' not found.", name);
                        std::process::exit(1);
                    };
                    database.delete_remote(remote.id).await?;
                    println!("Remote '{}' removed. Local project data preserved.", name);
                }
                RemoteAction::List => {
                    let remotes = database.list_remotes().await?;
                    if remotes.is_empty() {
                        println!("No remotes configured.");
                        println!("  Add one with: manifest remote add <name> --url <url> --token <token>");
                    } else {
                        println!("{:<15} {:<10} {:<8} {}", "NAME", "PROVIDER", "SYNC", "URL");
                        for r in &remotes {
                            let sync_status = if r.sync_enabled { "on" } else { "off" };
                            println!(
                                "{:<15} {:<10} {:<8} {}",
                                r.name, r.provider, sync_status, r.url
                            );
                        }
                    }
                }
                RemoteAction::Update { name, url, token } => {
                    let Some(remote) = database.get_remote_by_name(&name).await? else {
                        eprintln!("Remote '{}' not found.", name);
                        std::process::exit(1);
                    };
                    let input = manifest_core::models::UpdateRemoteInput {
                        url,
                        token,
                        sync_enabled: None,
                    };
                    database.update_remote(remote.id, &input).await?;
                    println!("Remote '{}' updated.", name);
                }
            }
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

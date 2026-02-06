# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

**Coding Standards**: Follow `coding-guidelines.md` for all Rust code.

## Overview

Manifest is an MCP server for living feature documentation. It tracks **features** (system capabilities) rather than work items, providing a persistent description of what the system IS rather than a changelog of what happened.

## Build & Test

```bash
cargo build                    # Debug build
cargo build --release          # Release build
cargo test --all               # Run all tests (~80+ specs across 4 files)
cargo test db_spec             # Database specs only
cargo test api_spec            # API specs only
cargo test mcp_protocol_spec   # MCP protocol specs only
cargo test acp_spec            # Agent chat protocol specs only
cargo test -p manifest-core    # Core unit tests only
cargo run                      # Start server on port 17010
cargo run -- serve -p 8080     # Start on custom port
cargo run -- mcp               # Start MCP server via stdio
cargo clippy --all -- -W clippy::all  # Lint check
```

### CLI Subcommands

- `serve` — Start HTTP API server (default port 17010)
- `mcp` — Start MCP server via stdio (for Claude Code/Cursor integration)
- `open` — Open Manifest dashboard in browser
- `status` — Check if server is running
- `stop` — Stop the daemon
- `migrate-roots` — Migrate existing projects to use root features

### BDD Testing with Speculate

Tests use [speculate2](https://crates.io/crates/speculate2) for BDD-style specs:

```rust
speculate! {
    describe "features" {
        before {
            let db = Database::open_memory().expect("...");
            db.migrate().expect("...");
        }

        it "creates a feature" {
            // ...
        }
    }
}
```

Test files in `tests/`: `db_spec.rs`, `api_spec.rs`, `mcp_protocol_spec.rs`, `acp_spec.rs`

## Development Practices

**Contract-First Development**: When adding or modifying API endpoints:

1. Update `openapi.yaml` first (or immediately after implementation)
2. Add tests for the new behavior
3. Implement the feature

The OpenAPI spec is the source of truth for the HTTP API.

## Architecture

**Stack**: Rust 2021 + Axum 0.8 + SQLx (SQLite/PostgreSQL) + Tokio

### Two-Crate Structure

```
manifest-server/           # Binary crate — HTTP API, MCP server, CLI
├── src/
│   ├── main.rs            # CLI (clap) with serve/mcp/status/stop/open subcommands
│   ├── api/               # HTTP API — validation & presentation layer
│   │   ├── mod.rs         # Router setup, all routes under /api/v1
│   │   ├── handlers/      # Request handlers (wildcard re-exports via mod.rs)
│   │   ├── auth.rs        # API key auth (local) + Clerk JWT (cloud)
│   │   ├── middleware.rs   # CORS, tracing, auth, security headers
│   │   └── validation.rs  # Input validation helpers
│   ├── mcp/               # MCP server — execution layer for AI agents
│   │   ├── server.rs      # Tool registration + MCP protocol handling
│   │   ├── client.rs      # HTTP client that calls own API
│   │   ├── tools/         # Tool implementations (projects, features, versions, etc.)
│   │   └── tree_render.rs # ASCII feature tree rendering
│   ├── acp/               # Agent Chat Protocol — multi-agent JSON-RPC over stdio
│   │   ├── router.rs      # Session pool (AcpRouter as static LazyLock)
│   │   ├── transport.rs   # Reader/writer tasks + pending request tracking
│   │   ├── process.rs     # Subprocess spawning for Claude/Gemini/Copilot/Codex
│   │   ├── registry.rs    # Agent process registry
│   │   └── types.rs       # JSON-RPC 2.0 message types with classify()
│   └── analysis/          # Codebase analysis for project discovery
│       ├── scanner.rs     # Directory/module detection
│       ├── parsers.rs     # Language/framework detection
│       ├── git_history.rs # Commit history analysis
│       ├── feature_extractor.rs
│       └── markdown_gen.rs
│
manifest-core/             # Library crate — models, DB operations, business logic
├── src/
│   ├── models/            # Domain types (Feature, Project, Version, Session, Task, etc.)
│   └── db/
│       ├── mod.rs         # All CRUD operations + business logic + migrations
│       └── schema.rs      # SQLite schema definition
└── migrations/
    └── 20240101000000_initial.sql  # Fresh install schema
```

### Layering

The **API layer** (`src/api/`) validates input, maps HTTP concerns, and delegates to the DB layer. No business logic here.

The **MCP layer** (`src/mcp/`) orchestrates AI agent interactions. MCP tools call through a `ManifestClient` which hits the HTTP API, inheriting all validation. Adds agent-specific formatting (tree rendering, text responses).

**Business logic lives in the DB layer** (`manifest-core/src/db/mod.rs`). Both API and MCP converge here. Examples: auto-assigning features to the "next" version when started, enforcing minimum unreleased version counts, respecting project settings.

### Data Model

Features form a **hierarchical tree**:

```
Authentication/                 <- feature node with context
├── Login/                      <- feature node with context
│   ├── Email + Password        <- leaf (can have sessions)
│   └── OAuth/                  <- feature node
│       ├── Google              <- leaf
│       └── GitHub              <- leaf
└── Session Management          <- leaf
```

**Permanent entities:**

- **Feature**: Self-referential tree via `parent_id`. States: Proposed → InProgress → Implemented → Archived. Only leaf nodes can have sessions.
- **FeatureHistory**: Append-only log of work sessions + commit references.
- **Project**: Container with directories, guidance settings (`ac_level`, `ac_format`).
- **Version**: Release milestones with semantic versioning. Lifecycle: next → planned → released.

**Ephemeral entities (deleted when session completes):**

- **Session**: One per leaf feature during active work.
- **Task**: Work unit assigned to an agent (claude/gemini/copilot/codex).

### Database

- **Engine**: SQLx with `AnyPool` (SQLite primary, PostgreSQL supported via `DbDialect` enum)
- **Location**: `~/.local/share/manifest/manifest.db`
- **Migrations**: Fresh installs use `manifest-core/migrations/20240101000000_initial.sql`. Existing DBs use incremental `migrate_*()` functions in `db/mod.rs`.
- **IDs**: TEXT (UUIDs), dates as RFC3339 strings

### Code Patterns

- Enums use manual `as_str()`/`from_str()` for DB serialization (not derive macros)
- `Result<Option<T>>` for get operations (None = not found, Err = DB error)
- Dynamic SQL building for partial updates (UpdateFeatureInput, etc.)
- `api/handlers` is a private module with wildcard re-exports — to expose items to `main.rs`, add `pub use handlers::foo;` in `api/mod.rs`

### ACP (Agent Chat Protocol)

The `src/acp/` module implements JSON-RPC 2.0 over stdio for multi-agent support (Claude, Gemini, Copilot, Codex). Key patterns:

- `RawJsonRpcMessage::classify()` routes messages by type
- `mpsc::channel` + `ReceiverStream` for async streaming (not `async_stream::stream!` which doesn't work with `tokio::select!`)
- Session reaper: every 5 min, kills agent processes idle > 30 min
- `AgentTransport` needs clone accessors (`clone_write_tx`, `clone_pending`, `clone_next_id`) for spawned tasks

### API Routes

All routes prefixed with `/api/v1`:

- **Projects**: CRUD at `/projects`, `/projects/{id}` + directories, features (list/create/bulk/roots/tree/next), versions, history, focus, SSE subscribe
- **Features**: CRUD at `/features`, `/features/{id}` + children, context, diff, history
- **Versions**: CRUD at `/versions`, `/versions/{id}` + assignment
- **Chat**: `POST /chat/completions` (agent chat)
- **Settings**: `GET/PUT /settings`, MCP status/configure
- **Analysis**: `GET /codebase/analyze`, filesystem browse/mkdir
- **Terminal**: WebSocket at `/terminal/ws`
- **MCP**: Stateless HTTP at `/mcp`

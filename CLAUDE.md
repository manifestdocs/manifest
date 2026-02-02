# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

**Coding Standards**: Follow `coding-guidelines.md` for all Rust code.

## Overview

Manifest (formerly "RocketManifest", "Legion") is an MCP server for living feature documentation. It was extracted from the RocketCrew VSCode extension to provide a standalone backend for technical product management.

### Core Philosophy: Features as Living Documentation

Unlike JIRA/Linear which track **work items** that accumulate as closed history, Manifest tracks **features** that describe the current state of the system:

| Traditional Tools          | Manifest                            |
| -------------------------- | ----------------------------------- |
| Issue (work item)          | Feature (system capability)         |
| Open → Closed → Forgotten  | Proposed → Implemented → **Living** |
| Changelog of what happened | Description of what IS              |

Features are not work items to be closed. They are living descriptions that evolve with the codebase.

### MCP Server Purpose

AI agents access features through deterministic MCP tools (not grep):

- `get_task_context` - Get assigned task with feature context
- `start_task` - Signal work is beginning
- `complete_task` - Mark task as complete

Agents are scoped to their assigned task and report progress back. Tasks are small units of work (1-3 story points). AI agents manage their own internal punch lists without Manifest tracking granular sub-items.

## Build & Test

```bash
cargo build                    # Debug build
cargo build --release          # Release build
cargo test                     # Run all tests
cargo test db_spec             # Run db specs only
cargo run                      # Start server on port 17010
cargo run -- serve -p 8080     # Start on custom port
```

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

Test files: `tests/db_spec.rs`, `tests/api_spec.rs` (58 tests total)

## Development Practices

**Contract-First Development**: When adding or modifying API endpoints:

1. Update `openapi.yaml` first (or immediately after implementation)
2. Add tests for the new behavior
3. Implement the feature

The OpenAPI spec is the source of truth for the HTTP API. Keeping it current is as important as writing tests.

## Architecture

**Stack**: Rust 2024 + Axum 0.8 + SQLite (rusqlite with WAL mode) + Tokio

```
src/
├── main.rs         # CLI (clap) with serve/status/stop subcommands
├── api/            # HTTP API — validation & presentation layer
│   ├── mod.rs      # Router setup, all routes under /api/v1
│   └── handlers/   # Request handlers (extract State<Database>)
├── db/
│   ├── mod.rs      # Database wrapper with CRUD operations + business logic
│   └── schema.rs   # SQLite schema (embedded, auto-migrated)
├── models/         # Domain types with serde + enums with as_str/from_str
└── mcp/            # MCP server — execution layer for AI agents
```

### API vs MCP: Layering

The **API layer** (`src/api/`) is the validation and presentation layer. It validates input (via `validation.rs`), maps HTTP concerns (status codes, JSON), and delegates to the DB layer. It does not contain business logic.

The **MCP layer** (`src/mcp/`) is the execution layer for AI agents. MCP tools call through a `ManifestClient` which hits the HTTP API, so they inherit all API validation. MCP tools add agent-specific orchestration (e.g., rendering trees, formatting responses as text).

**Business logic lives in the DB layer** (`manifest-core/src/db/mod.rs`). Both API handlers and MCP tools converge here. Examples: auto-assigning features to the "now" version when started, enforcing minimum unreleased version counts after release, respecting `default_feature_destination` project settings. This ensures consistent behavior regardless of whether a human (web UI → API) or an AI agent (MCP → API) triggers the action.

### Data Model

Features form a **hierarchical tree** (like a file browser):

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

- **Feature**: Self-referential tree via `parent_id`. Any node can have content (story + details). Only **leaf nodes** can have sessions.
- **FeatureHistory**: Append-only log of implementation sessions (like `git log` for a feature). Records what was done during each session and links to git commits. This is NOT feature versioning—the feature content itself is mutable. History answers "what work was done on this feature and when?"

**Ephemeral entities (exist only during active work):**

- **Session**: One active session per feature at a time. When completed, tasks are squashed into a `feature_history` entry and deleted.
- **Task**: Work unit within Session, assigned to an agent (claude/gemini/codex). Self-referential via `parent_id` for optional sub-tasks. Deleted when session completes.

```
Session lifecycle:
1. Create session on leaf feature
2. Create tasks, agents work
3. Session completes:
   └─> Summary of work → feature_history entry
   └─> Task records deleted
   └─> Session marked completed
```

Key methods: `get_root_features()`, `get_children(id)`, `is_leaf(id)`

### API Routes

All routes prefixed with `/api/v1`:

- Projects: CRUD at `/projects`, `/projects/{id}`
  - `/projects/{id}/directories` - GET/POST project directories
  - `/projects/{id}/features` - GET/POST features for project
  - `/projects/{id}/features/roots` - GET root features
  - `/projects/{id}/features/tree` - GET complete feature tree (nested)
- Features: CRUD at `/features`, `/features/{id}`
  - `/features/{id}/children` - GET direct children
  - `/features/{id}/history` - GET feature history
- Sessions: POST `/sessions`, GET `/sessions/{id}`, `/sessions/{id}/status`
  - Only allowed on leaf features (returns 500 if feature has children)
- Tasks: GET/PUT `/tasks/{id}`

### Database

- Location: `~/.local/share/manifest/manifest.db` (via `directories` crate)
- Schema auto-migrates on startup via `db.migrate()`
- All IDs stored as TEXT (UUIDs), dates as RFC3339 strings, content as JSON

### Code Patterns

- Enums use manual `as_str()`/`from_str()` for DB serialization (not derive macros)
- `Result<Option<T>>` pattern for get operations (None = not found, Err = DB error)
- Dynamic SQL building for partial updates (UpdateTaskInput, UpdateFeatureInput)
- Database wrapped in `Arc<Mutex<Connection>>` for thread-safe sharing

## Related Projects

- **RocketCrew** (`../RocketCrew`) - VSCode extension that consumes this server's HTTP API
- **ManifestExtension** (`../ManifestExtension`) - VSCode extension with feature tree (icon style reference)
- `../RocketCrew/design/vscode-extension-mvp.md` - Full vision for the TPM workflow
- `design/legion-server-architecture.md` - Server architecture spec (local)

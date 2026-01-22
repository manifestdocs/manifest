# Manifest

Living feature documentation for AI-assisted development.

Manifest is an MCP server and HTTP API that tracks **features** (system capabilities) rather than work items. Unlike traditional issue trackers where items are closed and forgotten, features are living descriptions that evolve with your codebase.

## Quick Start

```bash
# Start the server
manifest serve

# Or run as MCP server for Claude Code
manifest mcp
```

## Installation

### From Source

```bash
git clone https://github.com/rocket-tycoon/manifest
cd manifest
cargo install --path .
```

### Homebrew (coming soon)

```bash
brew install rocket-tycoon/tap/manifest
```

## Core Concepts

| Concept | Description |
|---------|-------------|
| **Feature** | A capability of the system, organized in a hierarchical tree. Features progress through states: `proposed` → `specified` → `implemented` → `deprecated` |
| **Session** | A work session on a leaf feature. Only one active session per feature at a time. When completed, creates a history entry. |
| **Task** | A unit of work within a session, assigned to an AI agent. Small enough for one agent (1-3 story points). |
| **History** | Append-only log of implementation sessions—like `git log` for a feature |

### Feature Lifecycle

```
Traditional Tools          Manifest
─────────────────          ────────
Issue (work item)    →     Feature (system capability)
Open → Closed        →     Proposed → Implemented → Living
Changelog of what    →     Description of what IS
  happened
```

Features are not work items to be closed. They are living documentation that evolves with the codebase.

---

# For Users

## CLI Commands

```bash
# Start HTTP server on default port (17010)
manifest serve

# Start on custom port
manifest serve -p 8080

# Start MCP server via stdio (for Claude Code)
manifest mcp

# Check server status
manifest status

# Stop the server
manifest stop
```

## Claude Code Integration

Add Manifest as an MCP server in your Claude Code configuration:

```json
{
  "mcpServers": {
    "manifest": {
      "command": "manifest",
      "args": ["mcp"]
    }
  }
}
```

### MCP Tools (18 total)

**Setup Tools** (one-time project initialization):

| Tool | Description |
|------|-------------|
| `create_project` | Create a project container for features. |
| `add_project_directory` | Link a filesystem path to a project. |
| `create_feature` | Define a single system capability. |
| `plan_features` | Define an entire feature tree in one call. |

**Discovery Tools** (find what to work on):

| Tool | Description |
|------|-------------|
| `get_project_context` | Get project info from a directory path. |
| `find_features` | Find features by project, state, or search query. Returns summaries only. |
| `get_feature` | Get full details of a specific feature. |
| `get_feature_history` | View past implementation sessions. |
| `update_feature_state` | Transition feature through lifecycle. |

**Orchestrator Tools** (manage sessions and tasks):

| Tool | Description |
|------|-------------|
| `create_session` | Start work session on a leaf feature. |
| `create_task` | Create a task within a session. |
| `breakdown_feature` | Create session + tasks in one call. |
| `list_session_tasks` | Monitor progress of all tasks. |
| `complete_session` | Finalize session, create history entry. |

**Agent Tools** (execute assigned work):

| Tool | Description |
|------|-------------|
| `get_task_context` | Get assigned task with full feature context. Call FIRST. |
| `start_task` | Signal work is beginning. Sets status to `running`. |
| `complete_task` | Signal task is finished. Only call when verified. |

### Complete Workflow

```
┌─────────────────────────────────────────────────────────────────┐
│                        SETUP (once)                             │
├─────────────────────────────────────────────────────────────────┤
│  1. create_project("MyApp", instructions="Use TDD...")          │
│  2. add_project_directory(project_id, "/path/to/myapp")         │
│  3. plan_features(project_id, features=[...], confirm=true)     │
└─────────────────────────────────────────────────────────────────┘
                              │
                              ▼
┌─────────────────────────────────────────────────────────────────┐
│                    DISCOVERY (orchestrator)                     │
├─────────────────────────────────────────────────────────────────┤
│  4. get_project_context("/path/to/myapp") → instructions        │
│  5. find_features(state="proposed") → find ready work           │
│     - OR find_features(query="auth") → search by keyword        │
│  6. get_feature(feature_id) → read full specification           │
│  7. get_feature_history(feature_id) → review past work          │
└─────────────────────────────────────────────────────────────────┘
                              │
                              ▼
┌─────────────────────────────────────────────────────────────────┐
│                    BREAKDOWN (orchestrator)                     │
├─────────────────────────────────────────────────────────────────┤
│  8. breakdown_feature(feature_id, goal, tasks=[...])            │
│     → session_id, [task_id_1, task_id_2, ...]                   │
│  9. Spawn agents with task IDs                                  │
└─────────────────────────────────────────────────────────────────┘
                              │
              ┌───────────────┼───────────────┐
              ▼               ▼               ▼
┌─────────────────┐ ┌─────────────────┐ ┌─────────────────┐
│   AGENT 1       │ │   AGENT 2       │ │   AGENT 3       │
├─────────────────┤ ├─────────────────┤ ├─────────────────┤
│ get_task_context│ │ get_task_context│ │ get_task_context│
│ start_task      │ │ start_task      │ │ start_task      │
│ [write code]    │ │ [write code]    │ │ [write code]    │
│ [run tests]     │ │ [run tests]     │ │ [run tests]     │
│ complete_task   │ │ complete_task   │ │ complete_task   │
└─────────────────┘ └─────────────────┘ └─────────────────┘
              │               │               │
              └───────────────┼───────────────┘
                              ▼
┌─────────────────────────────────────────────────────────────────┐
│                   COMPLETION (orchestrator)                     │
├─────────────────────────────────────────────────────────────────┤
│ 10. list_session_tasks(session_id) → all completed?             │
│ 11. complete_session(session_id, summary, commits)              │
│     → feature marked "implemented", history created             │
└─────────────────────────────────────────────────────────────────┘
```

## HTTP API

Base URL: `http://localhost:17010/api/v1`

Full API documentation is available in [openapi.yaml](./openapi.yaml).

### Key Endpoints

```bash
# Projects
GET    /projects                    # List all projects
POST   /projects                    # Create project
GET    /projects/{id}/features      # List features for project
GET    /projects/{id}/features/tree # Get complete feature tree

# Features
GET    /features/{id}               # Get feature
PUT    /features/{id}               # Update feature
GET    /features/{id}/children      # Get direct children
GET    /features/{id}/history       # Get implementation history

# Sessions (leaf features only)
POST   /sessions                    # Create session
GET    /sessions/{id}/status        # Get status with tasks
POST   /sessions/{id}/complete      # Complete session

# Tasks
GET    /tasks/{id}                  # Get task
PUT    /tasks/{id}                  # Update task status
POST   /tasks/{id}/notes            # Add implementation note
```

### Example: Create a Feature and Session

```bash
# Create a project
curl -X POST http://localhost:17010/api/v1/projects \
  -H "Content-Type: application/json" \
  -d '{"name": "my-app", "description": "My application"}'

# Create a feature
curl -X POST http://localhost:17010/api/v1/projects/{project_id}/features \
  -H "Content-Type: application/json" \
  -d '{
    "title": "User Authentication",
    "story": "As a user, I want to log in so I can access my account",
    "state": "specified"
  }'

# Start a session
curl -X POST http://localhost:17010/api/v1/sessions \
  -H "Content-Type: application/json" \
  -d '{
    "feature_id": "{feature_id}",
    "goal": "Implement login flow",
    "tasks": [{
      "title": "Login form",
      "scope": "Create login form with email/password validation",
      "agent_type": "claude"
    }]
  }'
```

## Data Storage

| Platform | Location |
|----------|----------|
| macOS | `~/.local/share/manifest/manifest.db` |
| Linux | `~/.local/share/manifest/manifest.db` |
| Windows | `%APPDATA%\manifest\manifest.db` |

The database auto-migrates on startup.

---

# For Contributors

## Development

Use the Makefile for common tasks:

```bash
make run          # Run in local mode (default)
make run-local    # Same as above
make run-cloud    # Run in cloud mode (requires env vars)
make run-cloud-env # Run cloud mode, loading .env file
make build        # Release build
make test         # Run all tests
make check        # Type check without building
make clean        # Remove build artifacts
```

### Local vs Cloud Mode

| Mode | When to Use | Authentication |
|------|-------------|----------------|
| **Local** | Development, personal use, CLI tools | None required |
| **Cloud** | Production, multi-tenant deployment | Clerk JWT required |

**Local mode** (`make run` or `make run-local`):
- No authentication required
- All endpoints accessible
- Rate limiting disabled (unless `MANIFEST_API_KEY` set)
- Use for local development and single-user scenarios

**Cloud mode** (`make run-cloud`):
- Requires Clerk authentication configuration
- All API endpoints require valid JWT
- SSE requires `?token=` query parameter
- MCP endpoint requires Bearer token
- Rate limiting enabled by default
- Use for production multi-tenant deployments

### Cloud Mode Setup

1. Create a `.env` file (copy from `.env.example`):
   ```bash
   cp .env.example .env
   ```

2. Configure Clerk credentials:
   ```bash
   # .env
   MANIFEST_MODE=cloud
   CLERK_DOMAIN=your-app.clerk.accounts.dev
   CLERK_AUTHORIZED_PARTIES=http://localhost:5173,https://your-domain.com
   ```

3. Run with environment:
   ```bash
   make run-cloud-env
   ```

Or export variables directly:
```bash
export CLERK_DOMAIN=your-app.clerk.accounts.dev
export CLERK_AUTHORIZED_PARTIES=http://localhost:5173
make run-cloud
```

## Building

```bash
# Debug build
cargo build

# Release build
cargo build --release

# Run tests
cargo test
```

## Project Structure

```
src/
├── main.rs          # CLI entry point (clap)
├── lib.rs           # Library root
├── api/
│   ├── mod.rs       # Router setup, all routes under /api/v1
│   └── handlers/    # Request handlers
├── db/
│   ├── mod.rs       # Database wrapper with CRUD operations
│   └── schema.rs    # SQLite schema (embedded, auto-migrated)
├── mcp/
│   ├── mod.rs       # MCP server and tool handlers
│   └── types.rs     # Request/response types for MCP tools
└── models/          # Domain types (Feature, Session, Task, etc.)

tests/
├── api_spec.rs      # HTTP API integration tests
└── db_spec.rs       # Database unit tests
```

## Testing

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

Run tests:

```bash
cargo test                    # All tests
cargo test db_spec            # Database tests only
cargo test api_spec           # API tests only
```

## Code Patterns

- **Enums**: Use manual `as_str()`/`from_str()` for DB serialization (not derive macros)
- **Error handling**: `Result<Option<T>>` for get operations (None = not found, Err = DB error)
- **Updates**: Dynamic SQL building for partial updates (`UpdateFeatureInput`, etc.)
- **Thread safety**: Database wrapped in `Arc<Mutex<Connection>>`

## Contract-First Development

When adding or modifying API endpoints:

1. Update `openapi.yaml` first (or immediately after implementation)
2. Add tests for the new behavior
3. Implement the feature

The OpenAPI spec is the source of truth for the HTTP API.

## Architecture Decisions

### Why SQLite?

- Single-file database, no external dependencies
- WAL mode for concurrent reads
- Auto-migrates on startup
- Portable across platforms

### Why MCP + HTTP?

- **MCP**: Direct integration with Claude Code and other AI tools
- **HTTP**: For VSCode extension, web UI, CLI tools, and any HTTP client

### Why Features Instead of Issues?

Features are living documentation. Unlike issues that are "closed and forgotten," features describe the current state of the system and evolve with the codebase.

## Related Projects

- **RocketCrew** - VSCode extension that consumes this API
- **RocketIndex** - Code indexer for semantic navigation

## License

[Business Source License 1.1](./LICENSE) - Free for personal and internal use. Converts to Apache 2.0 after 4 years.

---

## Quick Reference

### Feature States

| State | Description |
|-------|-------------|
| `proposed` | Initial idea, not yet fully specified |
| `specified` | Requirements defined, ready for implementation |
| `implemented` | Built and deployed (enters "living" phase) |
| `deprecated` | No longer active, kept for historical reference |

### Task States

| State | Description |
|-------|-------------|
| `pending` | Not yet started |
| `running` | Agent is working on it |
| `completed` | Work finished successfully |
| `failed` | Work could not be completed |

### Agent Types

| Type | Description |
|------|-------------|
| `claude` | Anthropic Claude |
| `gemini` | Google Gemini |
| `codex` | OpenAI Codex |

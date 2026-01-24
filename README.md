# Manifest

Living feature documentation for AI-assisted development.

Manifest is an MCP server and HTTP API that tracks **features** (system capabilities) rather than work items. Unlike traditional issue trackers where items are closed and forgotten, features are living descriptions that evolve with your codebase.

## Installation

### Quick Start

```bash
brew install manifestdocs/tap/manifest
```

After installation, start Manifest as a background service:

```bash
brew services start manifest
```

Manifest runs at `http://localhost:17010` by default.

Other service commands:

```bash
brew services stop manifest    # Stop the service
brew services restart manifest # Restart the service
brew services info manifest    # Check service status
```

To run manually in the foreground instead:

```bash
manifest serve
```

---

### MCP Configuration

Manifest includes an MCP server that lets AI agents read your product features as specs and write history when they complete work. Configure it for your preferred coding agent below.

#### Claude Code

Add via CLI:

```bash
claude mcp add manifest --scope user -- manifest mcp
```

Or edit `~/.claude.json` directly:

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

Restart Claude Code for changes to take effect.

---

#### Cursor

Edit your Cursor MCP config:

**macOS:** `~/.cursor/mcp.json`
**Windows:** `%USERPROFILE%\.cursor\mcp.json`

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

Restart Cursor after saving.

---

#### Windsurf

Edit your Windsurf MCP config:

**macOS:** `~/.codeium/windsurf/mcp_config.json`
**Windows:** `%USERPROFILE%\.codeium\windsurf\mcp_config.json`

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

Restart Windsurf after saving.

---

### Verify Installation

After configuring your coding agent, verify the MCP server is working:

1. Start a new session
2. Ask the agent to list your Manifest projects
3. The agent should be able to read your feature tree from Manifest

---

### Configuration Options

If Manifest is running on a non-default port or host, pass the URL:

```json
{
  "mcpServers": {
    "manifest": {
      "command": "manifest",
      "args": ["mcp", "--url", "http://localhost:8080"]
    }
  }
}
```

---

### Troubleshooting

**Server not connecting?**
- Ensure Manifest is running (`brew services info manifest` or `manifest serve`)
- Check that the `manifest` binary is in your PATH
- Restart your coding agent after config changes

**Permission issues on macOS?**
```bash
which manifest
# Should return /opt/homebrew/bin/manifest or /usr/local/bin/manifest
```

**Check MCP server logs:**
```bash
manifest mcp --verbose
```

---

### Claude Code plugin (optional)

Run this inside Claude Code for slash commands:

```
/plugin install manifestdocs/claude-plugins/manifest
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

## Agent Configuration

Add Manifest as an MCP server to your CLI agent:

### Claude Code

```bash
claude mcp add manifest -- manifest mcp
```

### Goose

```bash
goose mcp add manifest -- manifest mcp
```

### OpenAI Operator

```bash
operator mcp add manifest -- manifest mcp
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
- **HTTP**: For web UI, CLI tools, and any HTTP client

### Why Features Instead of Issues?

Features are living documentation. Unlike issues that are "closed and forgotten," features describe the current state of the system and evolve with the codebase.

## Related Projects

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

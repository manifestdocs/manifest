# Rust Code Quality Review

Date: 2025-01-XX  
Scope: `manifest-core`, HTTP API, and MCP server

## Summary

Across the codebase the architecture is clear and well-modularized, but a few recurring themes emerged:

1. Multi-step database operations are not wrapped in transactions, so failures can leave partial state behind.
2. Some error handling paths intentionally mask corruption or misclassify server faults as client mistakes.
3. Performance-oriented tweaks (pushing pagination into SQL, reusing DB handles, bounding in-memory structures) would pay off as data volume grows.
4. Several UX-facing components (MCP responses, desktop app menus) need polish so downstream tools and humans get better feedback.

---

## Progress Tracker

### Completed ✅

| Item                                       | Details                                                                                                                                                                                                                            |
| ------------------------------------------ | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| **Typed error enum in `manifest-core`**    | `ManifestError` expanded with `NotFound`, `Validation`, and `InvalidState` variants. Includes helper methods (`not_found()`, `validation()`, `invalid_state()`) for ergonomic construction.                                        |
| **Loud error reporting for parse helpers** | `parse_uuid()` and `parse_datetime()` now panic with descriptive messages instead of silently substituting `Uuid::nil()` / `Utc::now()`. This prevents corrupted data from propagating through the system.                         |
| **ManifestError usage in DB layer**        | `create_feature`, `create_session`, `create_task`, `complete_session`, `get_session_status`, `add_project_directory` now use `ManifestError` for domain errors.                                                                    |
| **Typed error handling in API**            | `internal_error()` now downcasts `anyhow::Error` to check for `ManifestError` and maps variants to appropriate HTTP status codes (`NotFound` → 404, `Validation` → 400, `InvalidState` → 409). No more fragile substring matching. |
| **MCP client timeouts**                    | `ManifestClient` now uses `Client::builder()` with 10s connect timeout and 30s request timeout to prevent indefinite hangs.                                                                                                        |
| **Transaction wrapping**                   | `create_session` and `complete_session` now use `Connection::transaction()` to ensure atomicity. Session+tasks are created/deleted together, and history entries are created within the same transaction as session completion.    |
| **Bulk feature creation transactions**     | Added `create_features_bulk()` method and refactored `bulk_create_features` handler to flatten feature trees with pre-generated UUIDs, then insert all features in a single transaction.                                           |
| **SQL-based pagination**                   | Added `get_all_features_paginated()` and `get_features_by_project_paginated()` methods that push `LIMIT`/`OFFSET` into SQL queries. API handlers now use these instead of fetching all rows and slicing in Rust.                   |

### Remaining 📋

**Medium Priority:**

- [ ] **Connection pooling** – Single `Arc<Mutex<Connection>>` creates contention. Consider `r2d2_sqlite` for higher concurrency scenarios.
- [ ] **MCP response format** – Tool handlers serialize to pretty JSON strings instead of returning structured content.
- [ ] **URL encoding in MCP client** – Manual percent-encoding only escapes a few characters. Use `reqwest::Url` or `serde_urlencoded`.

---

## Recommendations by Area

### 1. `manifest-core` (core library)

| Issue                                                                                                                  | Impact                                                                                                                           | Recommendation                                                                                                                                 |
| ---------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------- |
| ~~`create_session`, `complete_session`, and bulk feature creation run multiple SQL statements without a transaction.~~ | ~~A mid-sequence failure (e.g., inserting tasks) leaves inconsistent data—sessions without tasks or histories without cleanup.~~ | ✅ **FIXED** – Both methods now use `Connection::transaction()`. Bulk feature creation uses `create_features_bulk()` with pre-generated UUIDs. |
| ~~`parse_uuid`/`parse_datetime` silently substitute `Uuid::nil()` / `Utc::now()` on parsing errors.~~                  | ~~Data corruption in the DB is masked and propagates through the system with apparently valid values.~~                          | ✅ **FIXED** – Now panics with descriptive error message on parse failure.                                                                     |
| Single `Arc<Mutex<Connection>>` is shared for all operations.                                                          | Long-running read queries block writers and vice versa, reducing concurrency.                                                    | Consider a small pool of connections (`r2d2_sqlite`) or enabling SQLite shared-cache mode with per-request connections to reduce contention.   |

### 2. HTTP API (`src/api/*`)

| Issue                                                                                                             | Impact                                                                                                             | Recommendation                                                                                                                                                         |
| ----------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------ | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| ~~`internal_error` inspects error strings for substrings like `"leaf"`/`"not found"` to decide the HTTP status.~~ | ~~Legitimate server bugs that contain those words are misreported as `400 Bad Request`, obscuring real failures.~~ | ✅ **FIXED** – `internal_error()` now downcasts to `ManifestError` and maps variants to HTTP status codes: `NotFound` → 404, `Validation` → 400, `InvalidState` → 409. |
| ~~`list_features` / `list_project_features` fetch _all_ rows then paginate in Rust.~~                             | ~~Memory/time usage grows linearly with the dataset, defeating pagination.~~                                       | ✅ **FIXED** – Added `get_all_features_paginated()` and `get_features_by_project_paginated()` that push `LIMIT`/`OFFSET` into SQL.                                     |
| ~~Bulk feature creation loops without a transaction.~~                                                            | ~~Confirming a feature plan can leave half-baked trees if any insert fails.~~                                      | ✅ **FIXED** – Added `Database::create_features_bulk()` that inserts all features in a single transaction. Handler flattens tree with pre-generated UUIDs.             |
| Rate limiter stores an ever-growing `Vec<Instant>` per IP without cleanup.                                        | Memory usage for active clients grows unbounded, and every check grabs a global mutex.                             | Replace with a fixed-size ring buffer or bucketed counter, and schedule periodic pruning outside request hot paths.                                                    |

### 3. MCP Server (`src/mcp/*`)

| Issue                                                                             | Impact                                                                                | Recommendation                                                                                        |
| --------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------- |
| Tool handlers serialize responses to pretty JSON strings before returning.        | MCP clients must parse JSON manually and can't leverage structured content channels.  | Return structured content (e.g., `Content::json(value)` or equivalent) so clients receive typed data. |
| ~~`ManifestClient` uses the default `reqwest::Client` with no timeouts/retries.~~ | ~~Network hiccups can hang a tool call indefinitely, freezing the orchestrator/IDE.~~ | ✅ **FIXED** – Client now configured with 10s connect timeout and 30s request timeout.                |
| Manual percent-encoding of search queries only escapes a few characters.          | Queries containing Unicode or other reserved bytes become invalid URLs.               | Build URLs via `reqwest::Url` or `serde_urlencoded` to ensure correct encoding.                       |

---

## Implementation Notes

### ManifestError Pattern

The error type in `manifest-core/src/db/mod.rs`:

```rust
#[derive(Debug, Clone)]
pub enum ManifestError {
    NotFound(String),
    Validation(String),
}
```

To use throughout the codebase:

1. Have DB methods return `Result<T, ManifestError>` for domain errors (not found, validation failures)
2. Keep `anyhow::Result` for unexpected/infrastructure errors (SQLite failures, etc.)
3. In API handlers, match on `ManifestError` variants to return appropriate HTTP status codes

### Transaction Pattern (Implemented)

See `create_session` and `complete_session` in `manifest-core/src/db/mod.rs`:

```rust
pub fn create_session(&self, input: CreateSessionInput) -> Result<SessionResponse> {
    // Validation first (outside transaction)
    self.get_feature(input.feature_id)?
        .ok_or_else(|| ManifestError::not_found("Feature"))?;

    let mut conn = self.conn.lock().expect("database lock poisoned");
    let tx = conn.transaction()?;

    // All operations use &tx instead of conn
    tx.execute("INSERT INTO sessions ...", params![...])?;
    for task_input in input.tasks {
        tx.execute("INSERT INTO tasks ...", params![...])?;
    }

    tx.commit()?;
    Ok(SessionResponse { session, tasks })
}
```

### MCP Client Timeout Pattern (Implemented)

See `src/mcp/client.rs`:

```rust
impl ManifestClient {
    pub fn new(base_url: impl Into<String>, api_key: Option<String>) -> Self {
        let client = Client::builder()
            .connect_timeout(Duration::from_secs(10))
            .timeout(Duration::from_secs(30))
            .build()
            .expect("Failed to build HTTP client");

        Self {
            base_url: base_url.into(),
            api_key,
            client,
        }
    }
}
```

---

Implementing these changes will improve data integrity, observability, and user experience as the system scales.

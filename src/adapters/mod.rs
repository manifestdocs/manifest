//! Test output adapter system.
//!
//! Parses raw test runner output into structured `TestResult[]` records using
//! embedded Lua scripts. Built-in adapters handle common frameworks (cargo test,
//! pytest, jest, go test). Custom adapters can be placed in `.manifest/adapters/`.

mod builtin;
mod detect;
mod lua_runtime;

use manifest_core::models::TestSuite;
use std::path::Path;

pub use builtin::list_builtin_adapters;
pub use detect::detect_adapter;

/// Errors that can occur during adapter execution.
#[derive(Debug)]
pub enum AdapterError {
    /// Lua script has a structural problem (missing parse function, bad return type).
    Script(String),
    /// Runtime error during execution (timeout, memory, Lua error).
    Runtime(String),
    /// Adapter not found.
    NotFound(String),
}

impl std::fmt::Display for AdapterError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AdapterError::Script(msg) => write!(f, "Adapter script error: {msg}"),
            AdapterError::Runtime(msg) => write!(f, "Adapter runtime error: {msg}"),
            AdapterError::NotFound(name) => write!(f, "Adapter not found: {name}"),
        }
    }
}

impl std::error::Error for AdapterError {}

/// Result of parsing test output through an adapter.
pub struct AdapterResult {
    /// The adapter name that was used.
    pub adapter_name: String,
    /// Parsed test results grouped into suites.
    pub test_suites: Vec<TestSuite>,
}

/// Parse test output using the appropriate adapter.
///
/// Resolution order:
/// 1. If `adapter_name` is provided, use it directly.
/// 2. If not, auto-detect from the command string.
/// 3. Look for a custom adapter at `<project_dir>/.manifest/adapters/<name>.lua`.
/// 4. Fall back to a built-in adapter.
///
/// Returns `Ok(None)` if no adapter could be found or matched.
/// Returns `Ok(Some(result))` on successful parsing.
/// Adapter errors are logged and returned as `Ok(None)` for graceful fallback.
pub fn parse_test_output(
    command: &str,
    output: &str,
    adapter_name: Option<&str>,
    project_dir: Option<&str>,
) -> Option<AdapterResult> {
    // Step 1: Determine adapter name
    let name = match adapter_name {
        Some(n) => n.to_string(),
        None => match detect_adapter(command) {
            Some(n) => n.to_string(),
            None => {
                tracing::debug!("No adapter matched for command: {command}");
                return None;
            }
        },
    };

    // Step 2: Try custom adapter from project directory
    if let Some(dir) = project_dir {
        let custom_path = Path::new(dir)
            .join(".manifest")
            .join("adapters")
            .join(format!("{name}.lua"));

        if custom_path.exists() {
            match std::fs::read_to_string(&custom_path) {
                Ok(script) => {
                    return execute_and_wrap(&name, &script, output);
                }
                Err(e) => {
                    tracing::warn!(
                        "Failed to read custom adapter {}: {e}",
                        custom_path.display()
                    );
                }
            }
        }
    }

    // Step 3: Try built-in adapter
    match builtin::get_builtin_adapter(&name) {
        Some(script) => execute_and_wrap(&name, script, output),
        None => {
            tracing::debug!("No adapter found for name: {name}");
            None
        }
    }
}

/// Execute an adapter script and wrap the result, logging errors gracefully.
///
/// Lua adapters return flat results (with optional suite per-test). We group
/// them into `TestSuite` structs here so callers get the structured format.
fn execute_and_wrap(name: &str, script: &str, output: &str) -> Option<AdapterResult> {
    use manifest_core::models::group_into_suites;

    match lua_runtime::execute_adapter(script, output) {
        Ok(flat_results) => {
            let count = flat_results.len();
            let test_suites = group_into_suites(flat_results);
            tracing::debug!(
                "Adapter '{name}' parsed {count} test results into {} suites",
                test_suites.len()
            );
            Some(AdapterResult {
                adapter_name: name.to_string(),
                test_suites,
            })
        }
        Err(e) => {
            tracing::warn!("Adapter '{name}' failed: {e}");
            None
        }
    }
}

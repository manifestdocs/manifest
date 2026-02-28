use manifest_core::models::proof::{TestResult, TestState};
use mlua::{Lua, Result as LuaResult, StdLib, Table, Value};
use std::str::FromStr;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use super::AdapterError;

/// Maximum instructions before timeout (~5s CPU).
const INSTRUCTION_LIMIT: u64 = 50_000_000;

/// Memory limit for Lua VM (8 MB).
const MEMORY_LIMIT: usize = 8 * 1024 * 1024;

/// Create a sandboxed Lua VM with only string, table, and math libraries.
fn create_sandbox() -> Result<Lua, AdapterError> {
    let lua = Lua::new_with(
        StdLib::STRING | StdLib::TABLE | StdLib::MATH,
        mlua::LuaOptions::default(),
    )
    .map_err(|e| AdapterError::Runtime(format!("Failed to create Lua VM: {e}")))?;

    lua.set_memory_limit(MEMORY_LIMIT)
        .map_err(|e| AdapterError::Runtime(format!("Failed to set memory limit: {e}")))?;

    // Remove dangerous globals
    let globals = lua.globals();
    for name in &["require", "dofile", "loadfile", "load"] {
        let _ = globals.set(*name, Value::Nil);
    }

    // Set instruction count hook for timeout
    let count = Arc::new(AtomicU64::new(0));
    let count_clone = count.clone();
    lua.set_hook(
        mlua::HookTriggers::new().every_nth_instruction(10_000),
        move |_lua, _debug| {
            let current = count_clone.fetch_add(10_000, Ordering::Relaxed);
            if current >= INSTRUCTION_LIMIT {
                Err(mlua::Error::RuntimeError(
                    "Adapter execution timeout: instruction limit exceeded".to_string(),
                ))
            } else {
                Ok(())
            }
        },
    );

    Ok(lua)
}

/// Execute a Lua adapter script against raw test output.
///
/// The script must define a `parse(output)` function that returns an array of
/// test result tables.
pub fn execute_adapter(script: &str, output: &str) -> Result<Vec<TestResult>, AdapterError> {
    let lua = create_sandbox()?;

    // Load and execute the adapter script
    lua.load(script)
        .exec()
        .map_err(|e| AdapterError::Script(format!("Failed to load adapter script: {e}")))?;

    // Get the parse function
    let parse: mlua::Function = lua.globals().get("parse").map_err(|_| {
        AdapterError::Script("Adapter script must define a `parse(output)` function".to_string())
    })?;

    // Call parse(output)
    let result: Value = parse
        .call(output)
        .map_err(|e| AdapterError::Runtime(format!("Adapter parse() failed: {e}")))?;

    // Convert result table to Vec<TestResult>
    match result {
        Value::Table(table) => table_to_test_results(&table),
        Value::Nil => Ok(vec![]),
        other => Err(AdapterError::Script(format!(
            "parse() must return a table, got {}",
            other.type_name()
        ))),
    }
}

/// Convert a Lua table (array of test result tables) to Vec<TestResult>.
fn table_to_test_results(table: &Table) -> Result<Vec<TestResult>, AdapterError> {
    let mut results = Vec::new();

    for pair in table.pairs::<usize, Value>() {
        let (i, value) =
            pair.map_err(|e| AdapterError::Script(format!("Invalid table structure: {e}")))?;

        match value {
            Value::Table(entry) => {
                let result = table_to_test_result(&entry, i)?;
                results.push(result);
            }
            _ => {
                return Err(AdapterError::Script(format!(
                    "Entry {i} must be a table, got {}",
                    value.type_name()
                )));
            }
        }
    }

    Ok(results)
}

/// Convert a single Lua table to a TestResult.
fn table_to_test_result(entry: &Table, index: usize) -> Result<TestResult, AdapterError> {
    let name: String = entry.get("name").map_err(|_| {
        AdapterError::Script(format!("Entry {index}: missing required field 'name'"))
    })?;

    let state_str: String = entry.get("state").map_err(|_| {
        AdapterError::Script(format!("Entry {index}: missing required field 'state'"))
    })?;

    let state = TestState::from_str(&state_str).map_err(|_| {
        AdapterError::Script(format!(
            "Entry {index}: invalid state '{state_str}', expected passed/failed/errored/skipped"
        ))
    })?;

    let suite: Option<String> = get_optional_string(entry, "suite");
    let file: Option<String> = get_optional_string(entry, "file");
    let message: Option<String> = get_optional_string(entry, "message");

    let line: Option<u32> = entry
        .get::<Option<i64>>("line")
        .ok()
        .flatten()
        .map(|v| v as u32);

    let duration_ms: Option<u64> = entry
        .get::<Option<i64>>("duration_ms")
        .ok()
        .flatten()
        .map(|v| v as u64);

    Ok(TestResult {
        name,
        suite,
        state,
        file,
        line,
        duration_ms,
        message,
    })
}

/// Get an optional string field from a Lua table.
fn get_optional_string(table: &Table, key: &str) -> Option<String> {
    match table.get::<Value>(key) {
        Ok(Value::String(s)) => s.to_str().ok().map(|s| s.to_string()),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn executes_simple_adapter() {
        let script = r#"
            function parse(output)
                return {
                    { name = "test_one", state = "passed" },
                    { name = "test_two", state = "failed", message = "assertion error" },
                }
            end
        "#;

        let results = execute_adapter(script, "").unwrap();
        assert_eq!(results.len(), 2);
        assert_eq!(results[0].name, "test_one");
        assert_eq!(results[0].state, TestState::Passed);
        assert_eq!(results[1].name, "test_two");
        assert_eq!(results[1].state, TestState::Failed);
        assert_eq!(results[1].message.as_deref(), Some("assertion error"));
    }

    #[test]
    fn handles_all_fields() {
        let script = r#"
            function parse(output)
                return {
                    {
                        name = "auth_test",
                        suite = "auth",
                        state = "passed",
                        file = "tests/auth.rs",
                        line = 42,
                        duration_ms = 120,
                        message = nil,
                    },
                }
            end
        "#;

        let results = execute_adapter(script, "").unwrap();
        assert_eq!(results.len(), 1);
        let r = &results[0];
        assert_eq!(r.name, "auth_test");
        assert_eq!(r.suite.as_deref(), Some("auth"));
        assert_eq!(r.state, TestState::Passed);
        assert_eq!(r.file.as_deref(), Some("tests/auth.rs"));
        assert_eq!(r.line, Some(42));
        assert_eq!(r.duration_ms, Some(120));
        assert!(r.message.is_none());
    }

    #[test]
    fn rejects_missing_parse_function() {
        let script = "-- no parse function";
        let err = execute_adapter(script, "").unwrap_err();
        assert!(matches!(err, AdapterError::Script(_)));
    }

    #[test]
    fn rejects_invalid_state() {
        let script = r#"
            function parse(output)
                return {{ name = "t", state = "invalid" }}
            end
        "#;

        let err = execute_adapter(script, "").unwrap_err();
        assert!(matches!(err, AdapterError::Script(_)));
    }

    #[test]
    fn rejects_missing_name() {
        let script = r#"
            function parse(output)
                return {{ state = "passed" }}
            end
        "#;

        let err = execute_adapter(script, "").unwrap_err();
        assert!(matches!(err, AdapterError::Script(_)));
    }

    #[test]
    fn handles_empty_results() {
        let script = r#"
            function parse(output)
                return {}
            end
        "#;

        let results = execute_adapter(script, "").unwrap();
        assert!(results.is_empty());
    }

    #[test]
    fn handles_nil_return() {
        let script = r#"
            function parse(output)
                return nil
            end
        "#;

        let results = execute_adapter(script, "").unwrap();
        assert!(results.is_empty());
    }

    #[test]
    fn sandbox_blocks_dangerous_functions() {
        // require should be nil
        let script = r#"
            function parse(output)
                local ok, err = pcall(require, "os")
                if not ok then
                    return {{ name = "sandbox_check", state = "passed" }}
                end
                return {{ name = "sandbox_check", state = "failed", message = "require was available" }}
            end
        "#;

        let results = execute_adapter(script, "").unwrap();
        assert_eq!(results[0].state, TestState::Passed);
    }

    #[test]
    fn passes_output_to_parse() {
        let script = r#"
            function parse(output)
                if string.find(output, "PASS") then
                    return {{ name = "found_pass", state = "passed" }}
                end
                return {{ name = "no_pass", state = "failed" }}
            end
        "#;

        let results = execute_adapter(script, "PASS: all tests").unwrap();
        assert_eq!(results[0].name, "found_pass");
        assert_eq!(results[0].state, TestState::Passed);
    }

    #[test]
    fn timeout_on_infinite_loop() {
        let script = r#"
            function parse(output)
                while true do end
                return {}
            end
        "#;

        let err = execute_adapter(script, "").unwrap_err();
        assert!(matches!(err, AdapterError::Runtime(_)));
    }
}

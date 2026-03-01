//! Integration specs for the test output adapter system.
//!
//! Tests the full adapter pipeline: command detection → adapter resolution →
//! Lua parsing → structured TestResult output. Uses realistic test runner
//! output samples to verify each built-in adapter.

use manifest::adapters::{list_builtin_adapters, parse_test_output, AdapterResult};
use manifest_core::models::TestState;

/// Flatten all tests from an AdapterResult's suites into a single Vec,
/// carrying the suite name as a tuple for easy assertion.
struct FlatTest {
    name: String,
    suite: String,
    state: TestState,
    file: Option<String>,
    line: Option<u32>,
    duration_ms: Option<u64>,
    message: Option<String>,
}

fn flatten(result: &AdapterResult) -> Vec<FlatTest> {
    result
        .test_suites
        .iter()
        .flat_map(|suite| {
            suite.tests.iter().map(move |t| FlatTest {
                name: t.name.clone(),
                suite: suite.name.clone(),
                state: t.state,
                file: t.file.clone().or_else(|| suite.file.clone()),
                line: t.line,
                duration_ms: t.duration_ms,
                message: t.message.clone(),
            })
        })
        .collect()
}

fn total_test_count(result: &AdapterResult) -> usize {
    result.test_suites.iter().map(|s| s.tests.len()).sum()
}

// ============================================================
// Built-in Adapter Inventory
// ============================================================

mod builtin_adapters {
    use super::*;

    #[test]
    fn lists_all_built_in_adapters() {
        let adapters = list_builtin_adapters();
        assert_eq!(adapters.len(), 4);
        assert!(adapters.contains(&"cargo-test"));
        assert!(adapters.contains(&"pytest"));
        assert!(adapters.contains(&"jest"));
        assert!(adapters.contains(&"go-test"));
    }
}

// ============================================================
// Cargo Test Adapter
// ============================================================

mod cargo_test_adapter {
    use super::*;

    const CARGO_TEST_OUTPUT: &str = "\
running 4 tests
test auth::tests::login_with_valid_credentials ... ok
test auth::tests::login_with_invalid_password ... FAILED
test auth::tests::signup_creates_user ... ok
test db::tests::connection_pool_reuse ... ignored

failures:

---- auth::tests::login_with_invalid_password stdout ----
thread 'auth::tests::login_with_invalid_password' panicked at 'assertion failed: `(left == right)`
  left: `Err(InvalidPassword)`,
 right: `Ok(())`', src/auth.rs:42:9

failures:
    auth::tests::login_with_invalid_password

test result: FAILED. 2 passed; 1 failed; 1 ignored; 0 measured; 0 filtered out; finished in 0.45s
";

    #[test]
    fn parses_passed_and_failed_tests() {
        let result = parse_test_output("cargo test --all", CARGO_TEST_OUTPUT, None, None)
            .expect("Adapter should match cargo test");

        assert_eq!(result.adapter_name, "cargo-test");
        assert_eq!(total_test_count(&result), 4);
    }

    #[test]
    fn maps_ok_to_passed_state() {
        let result = parse_test_output("cargo test", CARGO_TEST_OUTPUT, None, None).unwrap();
        let tests = flatten(&result);

        let passed: Vec<_> = tests
            .iter()
            .filter(|t| t.state == TestState::Passed)
            .collect();

        assert_eq!(passed.len(), 2);
        assert!(passed
            .iter()
            .any(|t| t.name == "auth::tests::login_with_valid_credentials"));
        assert!(passed
            .iter()
            .any(|t| t.name == "auth::tests::signup_creates_user"));
    }

    #[test]
    fn maps_failed_to_failed_state() {
        let result = parse_test_output("cargo test", CARGO_TEST_OUTPUT, None, None).unwrap();
        let tests = flatten(&result);

        let failed: Vec<_> = tests
            .iter()
            .filter(|t| t.state == TestState::Failed)
            .collect();

        assert_eq!(failed.len(), 1);
        assert_eq!(failed[0].name, "auth::tests::login_with_invalid_password");
    }

    #[test]
    fn maps_ignored_to_skipped_state() {
        let result = parse_test_output("cargo test", CARGO_TEST_OUTPUT, None, None).unwrap();
        let tests = flatten(&result);

        let skipped: Vec<_> = tests
            .iter()
            .filter(|t| t.state == TestState::Skipped)
            .collect();

        assert_eq!(skipped.len(), 1);
        assert_eq!(skipped[0].name, "db::tests::connection_pool_reuse");
    }

    #[test]
    fn extracts_suite_from_module_path() {
        let result = parse_test_output("cargo test", CARGO_TEST_OUTPUT, None, None).unwrap();
        let tests = flatten(&result);

        let login = tests
            .iter()
            .find(|t| t.name == "auth::tests::login_with_valid_credentials")
            .unwrap();

        assert_eq!(login.suite, "auth::tests");
    }

    #[test]
    fn extracts_failure_messages() {
        let result = parse_test_output("cargo test", CARGO_TEST_OUTPUT, None, None).unwrap();
        let tests = flatten(&result);

        let failed = tests.iter().find(|t| t.state == TestState::Failed).unwrap();

        let msg = failed
            .message
            .as_ref()
            .expect("Failed test should have message");
        assert!(
            msg.contains("assertion failed"),
            "Failure message should contain the panic text"
        );
    }

    #[test]
    fn auto_detects_from_cargo_nextest_command() {
        let output = "\
test auth::login ... ok
test auth::signup ... ok
";
        let result = parse_test_output("cargo nextest run --all", output, None, None)
            .expect("Should detect cargo-test adapter");

        assert_eq!(result.adapter_name, "cargo-test");
        assert_eq!(total_test_count(&result), 2);
    }
}

// ============================================================
// Pytest Adapter
// ============================================================

mod pytest_adapter {
    use super::*;

    const PYTEST_OUTPUT: &str = "\
============================= test session starts ==============================
platform linux -- Python 3.11.5, pytest-7.4.0
collected 3 items

tests/test_auth.py::test_login PASSED
tests/test_auth.py::test_invalid_password FAILED
tests/test_auth.py::test_signup SKIPPED

================================ FAILURES ================================
________________________________ test_invalid_password ________________________________

    def test_invalid_password():
>       assert login('user', 'wrong') == True
E       AssertionError: assert False == True

tests/test_auth.py:15: AssertionError
=========================== short test summary info ============================
FAILED tests/test_auth.py::test_invalid_password - AssertionError
========================= 1 failed, 1 passed, 1 skipped =======================
";

    #[test]
    fn parses_passed_failed_skipped() {
        let result = parse_test_output("pytest -v", PYTEST_OUTPUT, None, None)
            .expect("Adapter should match pytest");

        assert_eq!(result.adapter_name, "pytest");
        assert_eq!(total_test_count(&result), 3);

        let tests = flatten(&result);
        let states: Vec<_> = tests.iter().map(|t| &t.state).collect();
        assert!(states.contains(&&TestState::Passed));
        assert!(states.contains(&&TestState::Failed));
        assert!(states.contains(&&TestState::Skipped));
    }

    #[test]
    fn extracts_file_path() {
        let result = parse_test_output("pytest -v", PYTEST_OUTPUT, None, None).unwrap();
        let tests = flatten(&result);

        let login = tests.iter().find(|t| t.name == "test_login").unwrap();

        assert_eq!(login.file.as_deref(), Some("tests/test_auth.py"));
    }

    #[test]
    fn auto_detects_from_python_m_pytest_command() {
        let output = "tests/test_api.py::test_health PASSED\n";
        let result = parse_test_output("python -m pytest tests/", output, None, None)
            .expect("Should detect pytest adapter");

        assert_eq!(result.adapter_name, "pytest");
    }
}

// ============================================================
// Jest/Vitest Adapter
// ============================================================

mod jest_adapter {
    use super::*;

    const JEST_OUTPUT: &str = "\
 PASS src/auth.test.ts
  Authentication
    ✓ logs in with valid credentials (5 ms)
    ✕ rejects invalid password (10 ms)
    ○ skipped handles rate limiting

Test Suites: 1 passed, 1 total
Tests:       1 failed, 1 skipped, 1 passed, 3 total
";

    #[test]
    fn parses_unicode_markers() {
        let result = parse_test_output("pnpm test", JEST_OUTPUT, None, None)
            .expect("Adapter should match jest");

        assert_eq!(result.adapter_name, "jest");
        assert_eq!(total_test_count(&result), 3);
    }

    #[test]
    fn extracts_duration_from_parenthesized_time() {
        let result = parse_test_output("pnpm test", JEST_OUTPUT, None, None).unwrap();
        let tests = flatten(&result);

        let login = tests
            .iter()
            .find(|t| t.name == "logs in with valid credentials")
            .unwrap();

        assert_eq!(login.duration_ms, Some(5));
        assert_eq!(login.state, TestState::Passed);
    }

    #[test]
    fn tracks_describe_block_as_suite() {
        let result = parse_test_output("pnpm test", JEST_OUTPUT, None, None).unwrap();
        let tests = flatten(&result);

        let login = tests
            .iter()
            .find(|t| t.name == "logs in with valid credentials")
            .unwrap();

        assert_eq!(login.suite, "Authentication");
    }

    #[test]
    fn extracts_file_from_header() {
        let result = parse_test_output("pnpm test", JEST_OUTPUT, None, None).unwrap();
        let tests = flatten(&result);

        assert!(
            tests
                .iter()
                .all(|t| t.file.as_deref() == Some("src/auth.test.ts")),
            "All tests should inherit file from PASS header"
        );
    }

    #[test]
    fn auto_detects_from_vitest_command() {
        let output = " PASS src/utils.test.ts\n  ✓ adds numbers (1 ms)\n";
        let result = parse_test_output("npx vitest run", output, None, None)
            .expect("Should detect jest adapter for vitest");

        assert_eq!(result.adapter_name, "jest");
    }
}

// ============================================================
// Go Test Adapter
// ============================================================

mod go_test_adapter {
    use super::*;

    const GO_TEST_OUTPUT: &str = "\
=== RUN   TestAuth
=== RUN   TestAuth/valid_login
--- PASS: TestAuth/valid_login (0.01s)
=== RUN   TestAuth/invalid_password
        auth_test.go:42: expected error, got nil
--- FAIL: TestAuth/invalid_password (0.00s)
--- PASS: TestAuth (0.02s)
FAIL
exit status 1
FAIL    github.com/example/auth 0.025s
";

    #[test]
    fn parses_pass_and_fail() {
        let result = parse_test_output("go test -v ./...", GO_TEST_OUTPUT, None, None)
            .expect("Adapter should match go test");

        assert_eq!(result.adapter_name, "go-test");
        let tests = flatten(&result);

        let passed = tests
            .iter()
            .filter(|t| t.state == TestState::Passed)
            .count();
        let failed = tests
            .iter()
            .filter(|t| t.state == TestState::Failed)
            .count();

        assert!(passed >= 1, "Should have at least 1 passed test");
        assert!(failed >= 1, "Should have at least 1 failed test");
    }

    #[test]
    fn extracts_suite_from_subtest_parent() {
        let result = parse_test_output("go test -v ./...", GO_TEST_OUTPUT, None, None).unwrap();
        let tests = flatten(&result);

        let subtest = tests
            .iter()
            .find(|t| t.name == "TestAuth/valid_login")
            .unwrap();

        assert_eq!(subtest.suite, "TestAuth");
    }

    #[test]
    fn extracts_duration_in_milliseconds() {
        let result = parse_test_output("go test -v ./...", GO_TEST_OUTPUT, None, None).unwrap();
        let tests = flatten(&result);

        let subtest = tests
            .iter()
            .find(|t| t.name == "TestAuth/valid_login")
            .unwrap();

        assert_eq!(subtest.duration_ms, Some(10));
    }

    #[test]
    fn extracts_file_and_line_from_failure_output() {
        let result = parse_test_output("go test -v ./...", GO_TEST_OUTPUT, None, None).unwrap();
        let tests = flatten(&result);

        let failed = tests
            .iter()
            .find(|t| t.name == "TestAuth/invalid_password")
            .unwrap();

        assert_eq!(failed.file.as_deref(), Some("auth_test.go"));
        assert_eq!(failed.line, Some(42));
    }
}

// ============================================================
// Resolution Logic
// ============================================================

mod resolution {
    use super::*;

    #[test]
    fn returns_none_for_unknown_command() {
        let result = parse_test_output("ruby test.rb", "some output", None, None);
        assert!(
            result.is_none(),
            "Should return None when no adapter matches"
        );
    }

    #[test]
    fn explicit_adapter_name_overrides_detection() {
        // Pass a "cargo test" command but force the pytest adapter
        let output = "tests/test_api.py::test_health PASSED\n";
        let result = parse_test_output("cargo test", output, Some("pytest"), None)
            .expect("Explicit adapter name should override auto-detection");

        assert_eq!(result.adapter_name, "pytest");
    }

    #[test]
    fn returns_none_for_nonexistent_explicit_adapter() {
        let result = parse_test_output(
            "cargo test",
            "some output",
            Some("nonexistent-adapter"),
            None,
        );

        assert!(
            result.is_none(),
            "Should return None when explicit adapter doesn't exist"
        );
    }

    #[test]
    fn gracefully_handles_empty_output() {
        let result = parse_test_output("cargo test", "", None, None);

        match result {
            Some(r) => assert_eq!(
                total_test_count(&r),
                0,
                "Empty output should yield no tests"
            ),
            None => {} // Also acceptable
        }
    }

    #[test]
    fn gracefully_handles_unparseable_output() {
        let garbage = "This is not test output at all.\nJust random text.\n";
        let result = parse_test_output("cargo test", garbage, None, None);

        match result {
            Some(r) => assert_eq!(
                total_test_count(&r),
                0,
                "Garbage output should yield no tests"
            ),
            None => {} // Also acceptable
        }
    }

    #[test]
    fn custom_adapter_from_project_dir_takes_precedence() {
        // Create a temp directory with a custom adapter
        let temp = tempfile::tempdir().expect("Failed to create temp dir");
        let adapter_dir = temp.path().join(".manifest").join("adapters");
        std::fs::create_dir_all(&adapter_dir).expect("Failed to create adapter dir");

        // Write a custom cargo-test adapter that always returns a fixed result
        let custom_script = r#"
function parse(output)
    return {
        { name = "custom_adapter_test", state = "passed" }
    }
end
"#;
        std::fs::write(adapter_dir.join("cargo-test.lua"), custom_script)
            .expect("Failed to write custom adapter");

        let result = parse_test_output(
            "cargo test",
            "test whatever ... ok",
            None,
            Some(temp.path().to_str().unwrap()),
        )
        .expect("Custom adapter should be found");

        assert_eq!(result.adapter_name, "cargo-test");
        assert_eq!(total_test_count(&result), 1);
        let tests = flatten(&result);
        assert_eq!(
            tests[0].name, "custom_adapter_test",
            "Custom adapter should override the built-in one"
        );
    }
}

// ============================================================
// Sandbox Safety
// ============================================================

mod sandbox {
    use super::*;

    #[test]
    fn custom_adapter_cannot_access_filesystem() {
        let temp = tempfile::tempdir().expect("Failed to create temp dir");
        let adapter_dir = temp.path().join(".manifest").join("adapters");
        std::fs::create_dir_all(&adapter_dir).expect("Failed to create adapter dir");

        // Write a malicious adapter that tries to read a file
        let malicious = r#"
function parse(output)
    local f = io.open("/etc/passwd", "r")
    if f then
        f:close()
        return {{ name = "exploit", state = "failed", message = "io.open was available!" }}
    end
    return {{ name = "sandbox_safe", state = "passed" }}
end
"#;
        std::fs::write(adapter_dir.join("cargo-test.lua"), malicious)
            .expect("Failed to write adapter");

        // Should either fail gracefully (return None) or the io call errors
        // because io library is not loaded in the sandbox
        let result = parse_test_output("cargo test", "", None, Some(temp.path().to_str().unwrap()));

        match result {
            Some(r) => {
                // If it parsed successfully, it must be the safe path
                let tests = flatten(&r);
                assert!(
                    tests.is_empty() || tests.iter().all(|t| t.name != "exploit"),
                    "Sandbox must block filesystem access"
                );
            }
            None => {} // Adapter failed gracefully — that's fine
        }
    }

    #[test]
    fn custom_adapter_timeout_does_not_hang() {
        let temp = tempfile::tempdir().expect("Failed to create temp dir");
        let adapter_dir = temp.path().join(".manifest").join("adapters");
        std::fs::create_dir_all(&adapter_dir).expect("Failed to create adapter dir");

        let infinite_loop = r#"
function parse(output)
    while true do end
    return {}
end
"#;
        std::fs::write(adapter_dir.join("cargo-test.lua"), infinite_loop)
            .expect("Failed to write adapter");

        // Should return None (timeout) without hanging
        let result = parse_test_output("cargo test", "", None, Some(temp.path().to_str().unwrap()));

        assert!(
            result.is_none(),
            "Infinite loop adapter should timeout and return None"
        );
    }
}

// ============================================================
// Dogfood: Parse Manifest's Own Test Output
// ============================================================

mod dogfood {
    use super::*;

    /// Representative sample of `cargo test --all` output from this project.
    /// Kept inline so this test stays self-contained and doesn't depend on
    /// a live test run or external files.
    const OWN_TEST_OUTPUT: &str = "\
running 113 tests
test adapters::detect::tests::detects_go_test ... ok
test adapters::detect::tests::detects_cargo_test ... ok
test adapters::detect::tests::detects_jest_and_vitest ... ok
test adapters::detect::tests::detects_pytest ... ok
test adapters::detect::tests::handles_leading_whitespace ... ok
test adapters::detect::tests::returns_none_for_unknown ... ok
test adapters::lua_runtime::tests::executes_simple_adapter ... ok
test adapters::lua_runtime::tests::handles_all_fields ... ok
test adapters::lua_runtime::tests::handles_empty_results ... ok
test adapters::lua_runtime::tests::handles_nil_return ... ok
test adapters::lua_runtime::tests::passes_output_to_parse ... ok
test adapters::lua_runtime::tests::rejects_invalid_state ... ok
test adapters::lua_runtime::tests::rejects_missing_name ... ok
test adapters::lua_runtime::tests::rejects_missing_parse_function ... ok
test adapters::lua_runtime::tests::sandbox_blocks_dangerous_functions ... ok
test adapters::lua_runtime::tests::timeout_on_infinite_loop ... ok
test analysis::feature_extractor::tests::test_are_similar_titles ... ok
test api::auth::tests::test_constant_time_compare ... ok
test api::middleware::tests::rate_limiter_allows_requests_under_limit ... ok
test api::middleware::tests::rate_limiter_blocks_requests_over_limit ... ok
test mcp::tools::spec::tests::empty_details_should_block ... ok
test mcp::tools::spec::tests::sparse_details_warns ... ok
test mcp::tools::spec::tests::sufficient_details_no_warnings ... ok
test mcp::tree_render::tests::test_single_root ... ok
test mcp::tree_render::tests::test_with_children ... ok

test result: ok. 113 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.12s

running 28 tests
test builtin_adapters::lists_all_built_in_adapters ... ok
test cargo_test_adapter::parses_passed_and_failed_tests ... ok
test cargo_test_adapter::maps_ok_to_passed_state ... ok
test resolution::returns_none_for_unknown_command ... ok
test sandbox::custom_adapter_timeout_does_not_hang ... ok

test result: ok. 28 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.16s

running 19 tests
test project_settings::test_adapter::defaults_to_null_in_project_response ... ok
test feature_roots::returns_empty_list_when_no_features_exist ... ok
test feature_cascade_delete::deletes_children_when_parent_is_deleted ... ok
test security_auth::health_endpoint_is_accessible_without_auth ... ok

test result: ok. 19 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.15s

running 24 tests
test db::tests::migrate_is_idempotent ... ok
test models::feature::tests::derive_all_implemented_returns_implemented ... ok
test models::feature::tests::derive_any_in_progress_returns_in_progress ... ok

test result: ok. 24 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.02s
";

    #[test]
    fn parses_own_test_suite_output() {
        let result = parse_test_output("cargo test --all", OWN_TEST_OUTPUT, None, None)
            .expect("cargo-test adapter should match our own test output");

        assert_eq!(result.adapter_name, "cargo-test");
        assert!(
            total_test_count(&result) > 0,
            "Should parse at least some tests from our own output"
        );
    }

    #[test]
    fn all_own_tests_are_passed() {
        let result = parse_test_output("cargo test --all", OWN_TEST_OUTPUT, None, None).unwrap();
        let tests = flatten(&result);

        let non_passed: Vec<_> = tests
            .iter()
            .filter(|t| t.state != TestState::Passed)
            .collect();

        assert!(
            non_passed.is_empty(),
            "All tests in our suite should be passed, but found non-passed: {:?}",
            non_passed.iter().map(|t| &t.name).collect::<Vec<_>>()
        );
    }

    #[test]
    fn extracts_correct_test_count() {
        let result = parse_test_output("cargo test --all", OWN_TEST_OUTPUT, None, None).unwrap();

        // The sample output has 25 + 5 + 4 + 3 = 37 test lines shown
        // (we only included representative subsets, not all lines)
        assert_eq!(
            total_test_count(&result),
            37,
            "Should parse exactly the test lines present in the sample"
        );
    }

    #[test]
    fn extracts_module_suites_from_own_tests() {
        let result = parse_test_output("cargo test --all", OWN_TEST_OUTPUT, None, None).unwrap();
        let tests = flatten(&result);

        // Verify suite extraction from our module paths
        let adapter_test = tests
            .iter()
            .find(|t| t.name == "adapters::detect::tests::detects_cargo_test")
            .expect("Should find our own adapter detect test");

        assert_eq!(
            adapter_test.suite, "adapters::detect::tests",
            "Suite should be the module path prefix"
        );
    }

    #[test]
    fn parses_tests_across_multiple_test_binaries() {
        let result = parse_test_output("cargo test --all", OWN_TEST_OUTPUT, None, None).unwrap();
        let tests = flatten(&result);

        // Tests from the main lib binary (adapters::, api::, mcp::)
        let lib_tests = tests
            .iter()
            .filter(|t| {
                t.name.starts_with("adapters::")
                    || t.name.starts_with("api::")
                    || t.name.starts_with("mcp::")
                    || t.name.starts_with("analysis::")
            })
            .count();

        // Tests from integration test binaries (no module prefix)
        let integration_tests = tests
            .iter()
            .filter(|t| {
                t.name.starts_with("builtin_adapters::")
                    || t.name.starts_with("cargo_test_adapter::")
                    || t.name.starts_with("resolution::")
                    || t.name.starts_with("sandbox::")
                    || t.name.starts_with("project_settings::")
                    || t.name.starts_with("feature_roots::")
                    || t.name.starts_with("feature_cascade_delete::")
                    || t.name.starts_with("security_auth::")
                    || t.name.starts_with("db::")
                    || t.name.starts_with("models::")
            })
            .count();

        assert!(lib_tests > 0, "Should parse tests from the lib binary");
        assert!(
            integration_tests > 0,
            "Should parse tests from integration test binaries"
        );
    }

    #[test]
    fn explicit_adapter_setting_works_for_own_project() {
        // Simulate what happens when test_adapter is set to "cargo-test" on the project
        let result = parse_test_output(
            "cargo test --all",
            OWN_TEST_OUTPUT,
            Some("cargo-test"),
            None,
        )
        .expect("Explicit cargo-test adapter should work");

        assert_eq!(result.adapter_name, "cargo-test");
        assert!(total_test_count(&result) > 0);
    }
}

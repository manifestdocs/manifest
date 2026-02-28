/// Built-in Lua adapter scripts, embedded at compile time.

const CARGO_TEST: &str = include_str!("cargo_test.lua");
const PYTEST: &str = include_str!("pytest.lua");
const JEST: &str = include_str!("jest.lua");
const GO_TEST: &str = include_str!("go_test.lua");

/// Get a built-in adapter script by name.
///
/// Returns `None` if no built-in adapter exists with the given name.
pub fn get_builtin_adapter(name: &str) -> Option<&'static str> {
    match name {
        "cargo-test" => Some(CARGO_TEST),
        "pytest" => Some(PYTEST),
        "jest" => Some(JEST),
        "go-test" => Some(GO_TEST),
        _ => None,
    }
}

/// List all available built-in adapter names.
pub fn list_builtin_adapters() -> &'static [&'static str] {
    &["cargo-test", "go-test", "jest", "pytest"]
}

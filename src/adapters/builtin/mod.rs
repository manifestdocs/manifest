//! Built-in Lua adapter scripts, embedded at compile time.

const CARGO_TEST: &str = include_str!("cargo_test.lua");
const DART_TEST: &str = include_str!("dart_test.lua");
const DOTNET_TEST: &str = include_str!("dotnet_test.lua");
const ELIXIR_TEST: &str = include_str!("elixir_test.lua");
const GO_TEST: &str = include_str!("go_test.lua");
const JEST: &str = include_str!("jest.lua");
const JUNIT: &str = include_str!("junit.lua");
const PHPUNIT: &str = include_str!("phpunit.lua");
const PYTEST: &str = include_str!("pytest.lua");
const RSPEC: &str = include_str!("rspec.lua");
const SWIFT_TEST: &str = include_str!("swift_test.lua");

/// Get a built-in adapter script by name.
///
/// Returns `None` if no built-in adapter exists with the given name.
pub fn get_builtin_adapter(name: &str) -> Option<&'static str> {
    match name {
        "cargo-test" => Some(CARGO_TEST),
        "dart-test" => Some(DART_TEST),
        "dotnet-test" => Some(DOTNET_TEST),
        "elixir-test" => Some(ELIXIR_TEST),
        "go-test" => Some(GO_TEST),
        "jest" => Some(JEST),
        "junit" => Some(JUNIT),
        "phpunit" => Some(PHPUNIT),
        "pytest" => Some(PYTEST),
        "rspec" => Some(RSPEC),
        "swift-test" => Some(SWIFT_TEST),
        _ => None,
    }
}

/// List all available built-in adapter names.
pub fn list_builtin_adapters() -> &'static [&'static str] {
    &[
        "cargo-test",
        "dart-test",
        "dotnet-test",
        "elixir-test",
        "go-test",
        "jest",
        "junit",
        "phpunit",
        "pytest",
        "rspec",
        "swift-test",
    ]
}

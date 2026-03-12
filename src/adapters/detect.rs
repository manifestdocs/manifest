/// Auto-detect the adapter name from a test command string.
///
/// Returns `None` if the command doesn't match any known pattern.
pub fn detect_adapter(command: &str) -> Option<&'static str> {
    let trimmed = command.trim();

    // Match against known command prefixes (order matters — more specific first)
    if trimmed.starts_with("cargo test") || trimmed.starts_with("cargo nextest") {
        Some("cargo-test")
    } else if trimmed.starts_with("pytest")
        || trimmed.starts_with("python -m pytest")
        || trimmed.starts_with("python3 -m pytest")
    {
        Some("pytest")
    } else if trimmed.starts_with("npx jest")
        || trimmed.starts_with("npx vitest")
        || trimmed.starts_with("jest ")
        || trimmed == "jest"
        || trimmed.starts_with("vitest ")
        || trimmed == "vitest"
        || trimmed.starts_with("pnpm test")
        || trimmed.starts_with("npm test")
        || trimmed.starts_with("yarn test")
        || trimmed.starts_with("bun test")
    {
        Some("jest")
    } else if trimmed.starts_with("go test") {
        Some("go-test")
    } else if trimmed.starts_with("dotnet test") {
        Some("dotnet-test")
    } else if trimmed.starts_with("rspec")
        || trimmed.starts_with("bundle exec rspec")
        || trimmed.starts_with("bin/rspec")
    {
        Some("rspec")
    } else if trimmed.starts_with("mvn test")
        || trimmed.starts_with("mvn verify")
        || trimmed.starts_with("./mvnw test")
        || trimmed.starts_with("gradle test")
        || trimmed.starts_with("./gradlew test")
    {
        Some("junit")
    } else if trimmed.starts_with("phpunit")
        || trimmed.starts_with("./vendor/bin/phpunit")
        || trimmed.starts_with("php artisan test")
    {
        Some("phpunit")
    } else if trimmed.starts_with("swift test") {
        Some("swift-test")
    } else if trimmed.starts_with("mix test") {
        Some("elixir-test")
    } else if trimmed.starts_with("dart test") || trimmed.starts_with("flutter test") {
        Some("dart-test")
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_cargo_test() {
        assert_eq!(detect_adapter("cargo test"), Some("cargo-test"));
        assert_eq!(detect_adapter("cargo test auth_spec"), Some("cargo-test"));
        assert_eq!(
            detect_adapter("cargo test --all -- --nocapture"),
            Some("cargo-test")
        );
        assert_eq!(
            detect_adapter("cargo nextest run --all"),
            Some("cargo-test")
        );
    }

    #[test]
    fn detects_pytest() {
        assert_eq!(detect_adapter("pytest"), Some("pytest"));
        assert_eq!(detect_adapter("pytest tests/auth"), Some("pytest"));
        assert_eq!(detect_adapter("pytest -v --tb=short"), Some("pytest"));
        assert_eq!(detect_adapter("python -m pytest"), Some("pytest"));
        assert_eq!(detect_adapter("python3 -m pytest tests/"), Some("pytest"));
    }

    #[test]
    fn detects_jest_and_vitest() {
        assert_eq!(detect_adapter("jest"), Some("jest"));
        assert_eq!(detect_adapter("jest --coverage"), Some("jest"));
        assert_eq!(detect_adapter("npx jest"), Some("jest"));
        assert_eq!(detect_adapter("npx vitest run"), Some("jest"));
        assert_eq!(detect_adapter("vitest run"), Some("jest"));
        assert_eq!(detect_adapter("pnpm test"), Some("jest"));
        assert_eq!(detect_adapter("npm test"), Some("jest"));
        assert_eq!(detect_adapter("yarn test"), Some("jest"));
        assert_eq!(detect_adapter("bun test"), Some("jest"));
    }

    #[test]
    fn detects_go_test() {
        assert_eq!(detect_adapter("go test ./..."), Some("go-test"));
        assert_eq!(detect_adapter("go test -v ./pkg/..."), Some("go-test"));
    }

    #[test]
    fn detects_dotnet_test() {
        assert_eq!(detect_adapter("dotnet test"), Some("dotnet-test"));
        assert_eq!(
            detect_adapter("dotnet test --filter Auth"),
            Some("dotnet-test")
        );
    }

    #[test]
    fn detects_rspec() {
        assert_eq!(detect_adapter("rspec"), Some("rspec"));
        assert_eq!(detect_adapter("rspec spec/auth_spec.rb"), Some("rspec"));
        assert_eq!(detect_adapter("bundle exec rspec"), Some("rspec"));
        assert_eq!(detect_adapter("bundle exec rspec spec/"), Some("rspec"));
        assert_eq!(detect_adapter("bin/rspec"), Some("rspec"));
    }

    #[test]
    fn detects_junit() {
        assert_eq!(detect_adapter("mvn test"), Some("junit"));
        assert_eq!(detect_adapter("mvn verify"), Some("junit"));
        assert_eq!(detect_adapter("./mvnw test"), Some("junit"));
        assert_eq!(detect_adapter("gradle test"), Some("junit"));
        assert_eq!(detect_adapter("./gradlew test"), Some("junit"));
    }

    #[test]
    fn detects_phpunit() {
        assert_eq!(detect_adapter("phpunit"), Some("phpunit"));
        assert_eq!(detect_adapter("./vendor/bin/phpunit"), Some("phpunit"));
        assert_eq!(
            detect_adapter("./vendor/bin/phpunit tests/"),
            Some("phpunit")
        );
        assert_eq!(detect_adapter("php artisan test"), Some("phpunit"));
    }

    #[test]
    fn detects_swift_test() {
        assert_eq!(detect_adapter("swift test"), Some("swift-test"));
        assert_eq!(
            detect_adapter("swift test --filter AuthTests"),
            Some("swift-test")
        );
    }

    #[test]
    fn detects_elixir_test() {
        assert_eq!(detect_adapter("mix test"), Some("elixir-test"));
        assert_eq!(
            detect_adapter("mix test test/auth_test.exs"),
            Some("elixir-test")
        );
    }

    #[test]
    fn detects_dart_test() {
        assert_eq!(detect_adapter("dart test"), Some("dart-test"));
        assert_eq!(detect_adapter("flutter test"), Some("dart-test"));
        assert_eq!(
            detect_adapter("flutter test test/auth_test.dart"),
            Some("dart-test")
        );
    }

    #[test]
    fn returns_none_for_unknown() {
        assert_eq!(detect_adapter("ruby test.rb"), None);
        assert_eq!(detect_adapter("make test"), None);
        assert_eq!(detect_adapter(""), None);
    }

    #[test]
    fn handles_leading_whitespace() {
        assert_eq!(detect_adapter("  cargo test"), Some("cargo-test"));
        assert_eq!(detect_adapter("\tpytest"), Some("pytest"));
    }
}

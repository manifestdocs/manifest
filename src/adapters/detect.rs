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

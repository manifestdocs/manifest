//! Background version check that compares the running server version
//! against the latest GitHub release and produces an update notice.

use std::time::Duration;

const CURRENT: &str = env!("CARGO_PKG_VERSION");
const RELEASES_URL: &str = "https://api.github.com/repos/manifestdocs/manifest/releases/latest";

/// Fetch the latest release tag from GitHub and return an update notice
/// string if a newer version is available, or `None` if up-to-date or
/// if the check fails (network errors are silently ignored).
pub async fn check_for_update() -> Option<String> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(5))
        .user_agent(format!("manifest/{CURRENT}"))
        .build()
        .ok()?;

    let resp: serde_json::Value = client
        .get(RELEASES_URL)
        .send()
        .await
        .ok()?
        .json()
        .await
        .ok()?;

    let tag = resp["tag_name"].as_str()?;
    let latest = tag.trim_start_matches('v');

    if is_newer(latest, CURRENT) {
        Some(format!(
            "\n\n---\n\
            Manifest v{latest} is available (you have v{CURRENT}). \
            Run `brew upgrade manifest` to update."
        ))
    } else {
        None
    }
}

/// Returns true if `a` is a strictly newer semver than `b`.
/// Ignores pre-release suffixes; falls back to string comparison on parse error.
fn is_newer(a: &str, b: &str) -> bool {
    match (parse_semver(a), parse_semver(b)) {
        (Some(av), Some(bv)) => av > bv,
        _ => a > b,
    }
}

fn parse_semver(s: &str) -> Option<(u32, u32, u32)> {
    let s = s.split('-').next()?; // strip pre-release
    let mut parts = s.split('.');
    let major = parts.next()?.parse().ok()?;
    let minor = parts.next()?.parse().ok()?;
    let patch = parts.next()?.parse().ok()?;
    Some((major, minor, patch))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_newer() {
        assert!(is_newer("1.1.0", "1.0.0"));
        assert!(is_newer("2.0.0", "1.9.9"));
        assert!(!is_newer("1.0.0", "1.0.0"));
        assert!(!is_newer("0.9.0", "1.0.0"));
    }

    #[test]
    fn strips_prerelease() {
        assert!(is_newer("1.1.0-beta.1", "1.0.0"));
        assert!(!is_newer("1.0.0-beta.1", "1.0.0"));
    }
}

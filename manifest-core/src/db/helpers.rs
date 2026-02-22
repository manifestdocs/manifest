use std::str::FromStr;

use anyhow::Result;
use chrono::{DateTime, Utc};
use sqlx::any::AnyRow;
use sqlx::Row;
use uuid::Uuid;

use super::DbDialect;
use crate::models::*;

/// Append `LIMIT` / `OFFSET` clauses to a SQL string based on optional pagination params.
///
/// Returns the next available parameter index after any newly appended `$N` placeholders.
/// Bind order convention: limit (if `Some`) is bound first, then offset (if `Some`).
/// When only offset is provided, the dialect's unlimited-offset literal is inlined
/// (e.g. `LIMIT -1` for SQLite, `LIMIT ALL` for Postgres) so no extra bind is needed.
pub(crate) fn append_pagination(
    sql: &mut String,
    limit: Option<u32>,
    offset: Option<u32>,
    next_param: u32,
    dialect: &DbDialect,
) -> u32 {
    let mut p = next_param;
    match (limit, offset) {
        (Some(_), Some(_)) => {
            sql.push_str(&format!(" LIMIT ${p} OFFSET ${}", p + 1));
            p += 2;
        }
        (Some(_), None) => {
            sql.push_str(&format!(" LIMIT ${p}"));
            p += 1;
        }
        (None, Some(_)) => {
            sql.push_str(&format!(" {} OFFSET ${p}", dialect.unlimited_offset_sql()));
            p += 1;
        }
        (None, None) => {}
    }
    p
}

/// Parse a UUID string from the database into a strongly-typed ID.
pub(crate) fn parse_id<T: From<Uuid>>(s: String) -> Result<T> {
    Uuid::parse_str(&s)
        .map(T::from)
        .map_err(|_| anyhow::anyhow!("Invalid UUID stored in database: {}", s))
}

/// Parse an RFC3339 or naive datetime string from the database into a UTC timestamp.
pub(crate) fn parse_datetime(s: String) -> Result<DateTime<Utc>> {
    chrono::DateTime::parse_from_rfc3339(&s)
        .map(|dt| dt.with_timezone(&Utc))
        .or_else(|_| {
            chrono::NaiveDateTime::parse_from_str(&s, "%Y-%m-%d %H:%M:%S").map(|dt| dt.and_utc())
        })
        .map_err(|_| anyhow::anyhow!("Invalid timestamp stored in database: {}", s))
}

/// Convert a name to a URL-friendly slug.
/// - Lowercase
/// - Replace non-alphanumeric with hyphens
/// - Collapse multiple hyphens
/// - Trim leading/trailing hyphens
#[must_use]
pub(crate) fn slugify(name: &str) -> String {
    let mut slug = String::with_capacity(name.len());
    let mut last_was_hyphen = true; // Start true to skip leading hyphens

    for c in name.chars() {
        if c.is_ascii_alphanumeric() {
            slug.push(c.to_ascii_lowercase());
            last_was_hyphen = false;
        } else if !last_was_hyphen {
            slug.push('-');
            last_was_hyphen = true;
        }
    }

    // Trim trailing hyphen
    if slug.ends_with('-') {
        slug.pop();
    }

    slug
}

/// Derive a key prefix from a project slug for display IDs (e.g., "MAN" from "manifest").
///
/// Takes the first word of the slug (split on `-`), uppercases it, and caps at 5 characters.
/// Falls back to "PRJ" if the slug is empty or produces no alpha characters.
#[must_use]
pub(crate) fn derive_key_prefix(slug: &str) -> String {
    let first_word = slug.split('-').next().unwrap_or("");
    let prefix: String = first_word
        .chars()
        .filter(|c| c.is_ascii_alphabetic())
        .take(5)
        .collect::<String>()
        .to_ascii_uppercase();

    if prefix.is_empty() {
        "PRJ".to_string()
    } else {
        prefix
    }
}

/// Validate that a version name is a semantic version: `MAJOR.MINOR.PATCH` with optional `v` prefix.
/// Examples: `0.1.0`, `v1.0.0`, `v2.3.1`.
#[must_use]
pub(crate) fn is_valid_semver(name: &str) -> bool {
    let name = name.strip_prefix('v').unwrap_or(name);
    let parts: Vec<&str> = name.split('.').collect();
    parts.len() == 3
        && parts
            .iter()
            .all(|p| !p.is_empty() && p.parse::<u32>().is_ok())
}

/// Compute the next version name by parsing existing versions and incrementing the minor version.
/// Falls back to "0.1.0" if no parseable versions exist.
#[must_use]
pub(crate) fn compute_next_version_name(versions: &[Version]) -> String {
    struct SemVer {
        major: u32,
        minor: u32,
    }

    let parsed: Vec<SemVer> = versions
        .iter()
        .filter_map(|v| {
            // Match "0.1.0" or "v0.1.0" style names
            let name = v.name.strip_prefix('v').unwrap_or(&v.name);
            let parts: Vec<&str> = name.split('.').collect();
            if parts.len() >= 2 {
                let major = parts[0].parse::<u32>().ok()?;
                let minor = parts[1].parse::<u32>().ok()?;
                Some(SemVer { major, minor })
            } else {
                None
            }
        })
        .collect();

    if parsed.is_empty() {
        return "0.1.0".to_string();
    }

    let highest = parsed.iter().max_by_key(|v| (v.major, v.minor)).unwrap();
    format!("{}.{}.0", highest.major, highest.minor + 1)
}

/// Map a database row to a [`Project`].
pub(crate) fn row_to_project(row: &AnyRow) -> Result<Project> {
    Ok(Project {
        id: parse_id(row.get("id"))?,
        slug: row.get("slug"),
        name: row.get("name"),
        description: row.get("description"),
        instructions: row.get("instructions"),
        current_version_id: row
            .get::<Option<String>, _>("current_version_id")
            .map(parse_id)
            .transpose()?,
        root_feature_id: row
            .get::<Option<String>, _>("root_feature_id")
            .map(parse_id)
            .transpose()?,
        default_feature_destination: row
            .get::<Option<String>, _>("default_feature_destination")
            .unwrap_or_else(|| "backlog".to_string()),
        detail_level: row
            .get::<Option<String>, _>("detail_level")
            .and_then(|s| GuidanceLevel::from_str(&s).ok())
            .unwrap_or(GuidanceLevel::Standard),
        ac_level: row
            .get::<Option<String>, _>("ac_level")
            .and_then(|s| GuidanceLevel::from_str(&s).ok())
            .unwrap_or(GuidanceLevel::Standard),
        ac_format: row
            .get::<Option<String>, _>("ac_format")
            .and_then(|s| AcFormat::from_str(&s).ok())
            .unwrap_or(AcFormat::Checkbox),
        key_prefix: row
            .get::<Option<String>, _>("key_prefix")
            .unwrap_or_default(),
        created_at: parse_datetime(row.get("created_at"))?,
        updated_at: parse_datetime(row.get("updated_at"))?,
    })
}

/// Map a database row to a [`ProjectDirectory`].
pub(crate) fn row_to_project_directory(row: &AnyRow) -> Result<ProjectDirectory> {
    Ok(ProjectDirectory {
        id: parse_id(row.get("id"))?,
        project_id: parse_id(row.get("project_id"))?,
        path: row.get("path"),
        git_remote: row.get("git_remote"),
        is_primary: row.get::<i32, _>("is_primary") != 0,
        instructions: row.get("instructions"),
        created_at: parse_datetime(row.get("created_at"))?,
    })
}

/// Map a database row to a [`Version`].
pub(crate) fn row_to_version(row: &AnyRow) -> Result<Version> {
    Ok(Version {
        id: parse_id(row.get("id"))?,
        project_id: parse_id(row.get("project_id"))?,
        name: row.get("name"),
        description: row.get("description"),
        released_at: row
            .get::<Option<String>, _>("released_at")
            .map(parse_datetime)
            .transpose()?,
        created_at: parse_datetime(row.get("created_at"))?,
        updated_at: parse_datetime(row.get("updated_at"))?,
    })
}

/// Map a database row to a [`Feature`].
pub(crate) fn row_to_feature(row: &AnyRow) -> Result<Feature> {
    Ok(Feature {
        id: parse_id(row.get("id"))?,
        project_id: parse_id(row.get("project_id"))?,
        parent_id: row
            .get::<Option<String>, _>("parent_id")
            .map(parse_id)
            .transpose()?,
        title: row.get("title"),
        details: row.get("details"),
        desired_details: row.get("desired_details"),
        details_summary: row.get("details_summary"),
        state: FeatureState::from_str(&row.get::<String, _>("state"))
            .unwrap_or(FeatureState::Proposed),
        priority: row.get("priority"),
        feature_number: row.get::<Option<i32>, _>("feature_number"),
        target_version_id: row
            .get::<Option<String>, _>("target_version_id")
            .map(parse_id)
            .transpose()?,
        verification_result: row
            .get::<Option<String>, _>("verification_result")
            .and_then(|s| serde_json::from_str(&s).ok()),
        verified_at: row
            .get::<Option<String>, _>("verified_at")
            .and_then(|s| parse_datetime(s).ok()),
        created_at: parse_datetime(row.get("created_at"))?,
        updated_at: parse_datetime(row.get("updated_at"))?,
    })
}

/// Map a database row to a [`FeatureSummary`].
pub(crate) fn row_to_feature_summary(row: &AnyRow) -> Result<FeatureSummary> {
    Ok(FeatureSummary {
        id: parse_id(row.get("id"))?,
        project_id: parse_id(row.get("project_id"))?,
        parent_id: row
            .get::<Option<String>, _>("parent_id")
            .map(parse_id)
            .transpose()?,
        title: row.get("title"),
        state: FeatureState::from_str(&row.get::<String, _>("state"))
            .unwrap_or(FeatureState::Proposed),
        priority: row.get("priority"),
        feature_number: row.get::<Option<i32>, _>("feature_number"),
        target_version_id: row
            .get::<Option<String>, _>("target_version_id")
            .map(parse_id)
            .transpose()?,
    })
}

/// Map a database row to a [`FeatureSummaryContext`].
pub(crate) fn row_to_feature_summary_context(row: &AnyRow) -> Result<FeatureSummaryContext> {
    Ok(FeatureSummaryContext {
        id: parse_id(row.get("id"))?,
        title: row.get("title"),
        state: FeatureState::from_str(&row.get::<String, _>("state"))
            .unwrap_or(FeatureState::Proposed),
    })
}

/// Map a database row to a [`FeatureHistory`].
pub(crate) fn row_to_feature_history(row: &AnyRow) -> Result<FeatureHistory> {
    let details_json: String = row.get("details");
    let details: HistoryDetails = serde_json::from_str(&details_json).unwrap_or_default();

    Ok(FeatureHistory {
        id: parse_id(row.get("id"))?,
        feature_id: parse_id(row.get("feature_id"))?,
        version_id: row
            .get::<Option<String>, _>("version_id")
            .map(parse_id)
            .transpose()?,
        details,
        created_at: parse_datetime(row.get("created_at"))?,
    })
}

/// Map a database row to a [`ProjectHistoryEntry`].
pub(crate) fn row_to_project_history_entry(row: &AnyRow) -> Result<ProjectHistoryEntry> {
    let details_json: String = row.get("details");
    let details: HistoryDetails = serde_json::from_str(&details_json).unwrap_or_default();

    Ok(ProjectHistoryEntry {
        id: parse_id(row.get::<String, _>("id"))?,
        feature_id: parse_id(row.get::<String, _>("feature_id"))?,
        feature_title: row.get("title"),
        feature_state: FeatureState::from_str(&row.get::<String, _>("state"))
            .unwrap_or(FeatureState::Proposed),
        version_id: row
            .get::<Option<String>, _>("version_id")
            .map(parse_id)
            .transpose()?,
        version_name: row.get("name"),
        summary: details.summary,
        commits: details.commits,
        created_at: parse_datetime(row.get::<String, _>("created_at"))?,
    })
}

/// Map a database row to a [`User`].
pub(crate) fn row_to_user(row: &AnyRow) -> Result<User> {
    Ok(User {
        id: parse_id(row.get("id"))?,
        email: row.get("email"),
        email_verified_at: row
            .get::<Option<String>, _>("email_verified_at")
            .map(parse_datetime)
            .transpose()?,
        display_name: row.get("display_name"),
        avatar_url: row.get("avatar_url"),
        created_at: parse_datetime(row.get("created_at"))?,
        updated_at: parse_datetime(row.get("updated_at"))?,
    })
}

/// Map a database row to an [`OAuthIdentity`].
pub(crate) fn row_to_oauth_identity(row: &AnyRow) -> Result<OAuthIdentity> {
    Ok(OAuthIdentity {
        id: parse_id(row.get("id"))?,
        user_id: parse_id(row.get("user_id"))?,
        provider: row.get("provider"),
        provider_user_id: row.get("provider_user_id"),
        provider_email: row.get("provider_email"),
        access_token: row.get("access_token"),
        refresh_token: row.get("refresh_token"),
        token_expires_at: row
            .get::<Option<String>, _>("token_expires_at")
            .map(parse_datetime)
            .transpose()?,
        created_at: parse_datetime(row.get("created_at"))?,
    })
}

/// Map a database row to a [`ProjectMembership`].
pub(crate) fn row_to_project_membership(row: &AnyRow) -> Result<ProjectMembership> {
    Ok(ProjectMembership {
        id: parse_id(row.get("id"))?,
        project_id: parse_id(row.get("project_id"))?,
        user_id: parse_id(row.get("user_id"))?,
        role: MembershipRole::from_str(&row.get::<String, _>("role"))
            .unwrap_or(MembershipRole::Viewer),
        invited_by: row
            .get::<Option<String>, _>("invited_by")
            .map(parse_id)
            .transpose()?,
        created_at: parse_datetime(row.get("created_at"))?,
    })
}

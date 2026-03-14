use std::str::FromStr;

use anyhow::Result;
use chrono::{DateTime, Utc};
use uuid::Uuid;

use crate::models::*;

/// Append `LIMIT` / `OFFSET` clauses to a SQL string based on optional pagination params.
///
/// Uses `?N` positional parameter syntax for libsql.
/// Returns the next available parameter index after any newly appended `?N` placeholders.
pub(crate) fn append_pagination(
    sql: &mut String,
    limit: Option<u32>,
    offset: Option<u32>,
    next_param: u32,
) -> u32 {
    let mut p = next_param;
    match (limit, offset) {
        (Some(_), Some(_)) => {
            sql.push_str(&format!(" LIMIT ?{p} OFFSET ?{}", p + 1));
            p += 2;
        }
        (Some(_), None) => {
            sql.push_str(&format!(" LIMIT ?{p}"));
            p += 1;
        }
        (None, Some(_)) => {
            sql.push_str(&format!(" LIMIT -1 OFFSET ?{p}"));
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
#[must_use]
pub(crate) fn slugify(name: &str) -> String {
    let mut slug = String::with_capacity(name.len());
    let mut last_was_hyphen = true;

    for c in name.chars() {
        if c.is_ascii_alphanumeric() {
            slug.push(c.to_ascii_lowercase());
            last_was_hyphen = false;
        } else if !last_was_hyphen {
            slug.push('-');
            last_was_hyphen = true;
        }
    }

    if slug.ends_with('-') {
        slug.pop();
    }

    slug
}

/// Derive a key prefix from a project slug for display IDs.
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

/// Validate that a version name is a semantic version.
#[must_use]
pub(crate) fn is_valid_semver(name: &str) -> bool {
    let name = name.strip_prefix('v').unwrap_or(name);
    let parts: Vec<&str> = name.split('.').collect();
    parts.len() == 3
        && parts
            .iter()
            .all(|p| !p.is_empty() && p.parse::<u32>().is_ok())
}

/// Compute the next version name by incrementing the minor version.
#[must_use]
pub(crate) fn compute_next_version_name(versions: &[Version]) -> String {
    struct SemVer {
        major: u32,
        minor: u32,
    }

    let parsed: Vec<SemVer> = versions
        .iter()
        .filter_map(|v| {
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

// ============================================================
// Row mapping helpers for libsql::Row
// ============================================================

/// Helper to get a String from a libsql Row by column name.
///
/// Builds a column name→index map from the row and retrieves the value.
/// This provides named-column access over libsql's index-based Row API.
pub(crate) fn get_str(row: &libsql::Row, col: &str) -> String {
    // libsql Row has column_name(idx) to find column names
    let count = row.column_count();
    for i in 0..count {
        if let Some(name) = row.column_name(i) {
            if name.eq_ignore_ascii_case(col) {
                return row.get::<String>(i).unwrap_or_default();
            }
        }
    }
    String::new()
}

/// Helper to get an optional String from a libsql Row by column name.
pub(crate) fn get_opt_str(row: &libsql::Row, col: &str) -> Option<String> {
    let count = row.column_count();
    for i in 0..count {
        if let Some(name) = row.column_name(i) {
            if name.eq_ignore_ascii_case(col) {
                return row.get::<Option<String>>(i).unwrap_or(None);
            }
        }
    }
    None
}

/// Helper to get an i32 from a libsql Row by column name.
pub(crate) fn get_i32(row: &libsql::Row, col: &str) -> i32 {
    let count = row.column_count();
    for i in 0..count {
        if let Some(name) = row.column_name(i) {
            if name.eq_ignore_ascii_case(col) {
                return row.get::<i32>(i).unwrap_or(0);
            }
        }
    }
    0
}

/// Helper to get an optional i32 from a libsql Row by column name.
pub(crate) fn get_opt_i32(row: &libsql::Row, col: &str) -> Option<i32> {
    let count = row.column_count();
    for i in 0..count {
        if let Some(name) = row.column_name(i) {
            if name.eq_ignore_ascii_case(col) {
                return row.get::<Option<i32>>(i).unwrap_or(None);
            }
        }
    }
    None
}

/// Helper to get an i64 from a libsql Row by column name.
pub(crate) fn get_i64(row: &libsql::Row, col: &str) -> i64 {
    let count = row.column_count();
    for i in 0..count {
        if let Some(name) = row.column_name(i) {
            if name.eq_ignore_ascii_case(col) {
                return row.get::<i64>(i).unwrap_or(0);
            }
        }
    }
    0
}

/// Helper to get an optional i64 from a libsql Row by column name.
pub(crate) fn get_opt_i64(row: &libsql::Row, col: &str) -> Option<i64> {
    let count = row.column_count();
    for i in 0..count {
        if let Some(name) = row.column_name(i) {
            if name.eq_ignore_ascii_case(col) {
                return row.get::<Option<i64>>(i).unwrap_or(None);
            }
        }
    }
    None
}

/// Map a database row to a [`Project`].
pub(crate) fn row_to_project(row: &libsql::Row) -> Result<Project> {
    Ok(Project {
        id: parse_id(get_str(row, "id"))?,
        slug: get_str(row, "slug"),
        name: get_str(row, "name"),
        description: get_opt_str(row, "description"),
        instructions: get_opt_str(row, "instructions"),
        current_version_id: get_opt_str(row, "current_version_id")
            .map(parse_id)
            .transpose()?,
        root_feature_id: get_opt_str(row, "root_feature_id")
            .map(parse_id)
            .transpose()?,
        default_feature_destination: get_opt_str(row, "default_feature_destination")
            .unwrap_or_else(|| "backlog".to_string()),
        test_adapter: get_opt_str(row, "test_adapter"),
        context_budget: get_opt_i64(row, "context_budget"),
        key_prefix: get_opt_str(row, "key_prefix").unwrap_or_default(),
        created_at: parse_datetime(get_str(row, "created_at"))?,
        updated_at: parse_datetime(get_str(row, "updated_at"))?,
    })
}

/// Map a database row to a [`SpecTemplate`].
pub(crate) fn row_to_spec_template(row: &libsql::Row) -> Result<SpecTemplate> {
    Ok(SpecTemplate {
        id: parse_id(get_str(row, "id"))?,
        project_id: parse_id(get_str(row, "project_id"))?,
        name: get_str(row, "name"),
        description: get_opt_str(row, "description"),
        content: get_str(row, "content"),
        is_default: get_i32(row, "is_default") != 0,
        created_at: parse_datetime(get_str(row, "created_at"))?,
        updated_at: parse_datetime(get_str(row, "updated_at"))?,
    })
}

/// Map a database row to a [`ProjectDirectory`].
pub(crate) fn row_to_project_directory(row: &libsql::Row) -> Result<ProjectDirectory> {
    Ok(ProjectDirectory {
        id: parse_id(get_str(row, "id"))?,
        project_id: parse_id(get_str(row, "project_id"))?,
        path: get_str(row, "path"),
        git_remote: get_opt_str(row, "git_remote"),
        is_primary: get_i32(row, "is_primary") != 0,
        instructions: get_opt_str(row, "instructions"),
        created_at: parse_datetime(get_str(row, "created_at"))?,
    })
}

/// Map a database row to a [`Version`].
pub(crate) fn row_to_version(row: &libsql::Row) -> Result<Version> {
    Ok(Version {
        id: parse_id(get_str(row, "id"))?,
        project_id: parse_id(get_str(row, "project_id"))?,
        name: get_str(row, "name"),
        description: get_opt_str(row, "description"),
        released_at: get_opt_str(row, "released_at")
            .map(parse_datetime)
            .transpose()?,
        created_at: parse_datetime(get_str(row, "created_at"))?,
        updated_at: parse_datetime(get_str(row, "updated_at"))?,
    })
}

/// Map a database row to a [`Feature`].
pub(crate) fn row_to_feature(row: &libsql::Row) -> Result<Feature> {
    Ok(Feature {
        id: parse_id(get_str(row, "id"))?,
        project_id: parse_id(get_str(row, "project_id"))?,
        parent_id: get_opt_str(row, "parent_id")
            .map(parse_id)
            .transpose()?,
        title: get_str(row, "title"),
        details: get_opt_str(row, "details"),
        desired_details: get_opt_str(row, "desired_details"),
        details_summary: get_opt_str(row, "details_summary"),
        state: FeatureState::from_str(&get_str(row, "state"))
            .unwrap_or(FeatureState::Proposed),
        priority: get_i32(row, "priority"),
        feature_number: get_opt_i32(row, "feature_number"),
        target_version_id: get_opt_str(row, "target_version_id")
            .map(parse_id)
            .transpose()?,
        verification_result: get_opt_str(row, "verification_result")
            .and_then(|s| serde_json::from_str(&s).ok()),
        verified_at: get_opt_str(row, "verified_at")
            .and_then(|s| parse_datetime(s).ok()),
        claimed_by: get_opt_str(row, "claimed_by"),
        claimed_at: get_opt_str(row, "claimed_at")
            .and_then(|s| parse_datetime(s).ok()),
        claim_metadata: get_opt_str(row, "claim_metadata"),
        created_at: parse_datetime(get_str(row, "created_at"))?,
        updated_at: parse_datetime(get_str(row, "updated_at"))?,
    })
}

/// Map a database row to a [`FeatureSummary`].
pub(crate) fn row_to_feature_summary(row: &libsql::Row) -> Result<FeatureSummary> {
    Ok(FeatureSummary {
        id: parse_id(get_str(row, "id"))?,
        project_id: parse_id(get_str(row, "project_id"))?,
        parent_id: get_opt_str(row, "parent_id")
            .map(parse_id)
            .transpose()?,
        title: get_str(row, "title"),
        state: FeatureState::from_str(&get_str(row, "state"))
            .unwrap_or(FeatureState::Proposed),
        priority: get_i32(row, "priority"),
        feature_number: get_opt_i32(row, "feature_number"),
        target_version_id: get_opt_str(row, "target_version_id")
            .map(parse_id)
            .transpose()?,
    })
}

/// Map a database row to a [`FeatureSummaryContext`].
pub(crate) fn row_to_feature_summary_context(row: &libsql::Row) -> Result<FeatureSummaryContext> {
    Ok(FeatureSummaryContext {
        id: parse_id(get_str(row, "id"))?,
        title: get_str(row, "title"),
        state: FeatureState::from_str(&get_str(row, "state"))
            .unwrap_or(FeatureState::Proposed),
    })
}

/// Map a database row to a [`FeatureHistory`].
pub(crate) fn row_to_feature_history(row: &libsql::Row) -> Result<FeatureHistory> {
    let details_json = get_str(row, "details");
    let details: HistoryDetails = serde_json::from_str(&details_json).unwrap_or_default();

    Ok(FeatureHistory {
        id: parse_id(get_str(row, "id"))?,
        feature_id: parse_id(get_str(row, "feature_id"))?,
        version_id: get_opt_str(row, "version_id")
            .map(parse_id)
            .transpose()?,
        details,
        created_at: parse_datetime(get_str(row, "created_at"))?,
    })
}

/// Map a database row to a [`ProjectHistoryEntry`].
pub(crate) fn row_to_project_history_entry(row: &libsql::Row) -> Result<ProjectHistoryEntry> {
    let details_json = get_str(row, "details");
    let details: HistoryDetails = serde_json::from_str(&details_json).unwrap_or_default();

    Ok(ProjectHistoryEntry {
        id: parse_id(get_str(row, "id"))?,
        feature_id: parse_id(get_str(row, "feature_id"))?,
        feature_title: get_str(row, "title"),
        feature_state: FeatureState::from_str(&get_str(row, "state"))
            .unwrap_or(FeatureState::Proposed),
        version_id: get_opt_str(row, "version_id")
            .map(parse_id)
            .transpose()?,
        version_name: get_opt_str(row, "name"),
        summary: details.summary,
        commits: details.commits,
        created_at: parse_datetime(get_str(row, "created_at"))?,
    })
}

/// Map a database row to a [`Remote`].
pub(crate) fn row_to_remote(row: &libsql::Row) -> Result<Remote> {
    Ok(Remote {
        id: parse_id(get_str(row, "id"))?,
        name: get_str(row, "name"),
        provider: get_str(row, "provider"),
        url: get_str(row, "url"),
        sync_enabled: get_i32(row, "sync_enabled") != 0,
        created_at: parse_datetime(get_str(row, "created_at"))?,
        updated_at: parse_datetime(get_str(row, "updated_at"))?,
    })
}

/// Map a database row to a [`ProjectRemote`].
pub(crate) fn row_to_project_remote(row: &libsql::Row) -> Result<ProjectRemote> {
    Ok(ProjectRemote {
        project_id: parse_id(get_str(row, "project_id"))?,
        remote_id: parse_id(get_str(row, "remote_id"))?,
        sync_state: SyncState::from_str(&get_str(row, "sync_state"))
            .unwrap_or(SyncState::Active),
        last_synced_at: get_opt_str(row, "last_synced_at")
            .map(parse_datetime)
            .transpose()?,
    })
}

/// Map a database row to a [`Proof`].
pub(crate) fn row_to_proof(row: &libsql::Row) -> Result<Proof> {
    let test_suites: Option<Vec<TestSuite>> = get_opt_str(row, "tests").and_then(|s| {
        serde_json::from_str::<Vec<TestSuite>>(&s).ok().or_else(|| {
            serde_json::from_str::<Vec<FlatTestResult>>(&s)
                .ok()
                .map(group_into_suites)
        })
    });

    let evidence: Vec<Evidence> = get_opt_str(row, "evidence")
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default();

    Ok(Proof {
        id: parse_id(get_str(row, "id"))?,
        feature_id: parse_id(get_str(row, "feature_id"))?,
        history_id: get_opt_str(row, "history_id")
            .map(parse_id)
            .transpose()?,
        command: get_str(row, "command"),
        exit_code: get_i32(row, "exit_code"),
        output: get_opt_str(row, "output"),
        test_suites,
        evidence,
        commit_sha: get_opt_str(row, "commit_sha"),
        agent_type: get_opt_str(row, "agent_type"),
        created_at: parse_datetime(get_str(row, "created_at"))?,
    })
}

// Re-export the get helpers for use by other db modules
pub(crate) use self::get_i32 as row_get_i32;
pub(crate) use self::get_i64 as row_get_i64;
pub(crate) use self::get_opt_i64 as row_get_opt_i64;
pub(crate) use self::get_opt_str as row_get_opt_str;
pub(crate) use self::get_str as row_get_str;

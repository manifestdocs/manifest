//! Directory browsing endpoints for project setup.
//!
//! Provides a filesystem tree browser filtered to directories only, skipping
//! noise directories like `node_modules` and `.git`. Used by the web UI when
//! associating directories with projects.

use axum::{extract::Query, http::StatusCode, Json};
use serde::{Deserialize, Serialize};
use std::path::{Component, Path, PathBuf};
use validator::Validate;

use super::ApiError;
use crate::api::validation::ValidatedJson;

#[derive(Debug, Deserialize)]
pub struct BrowseQuery {
    /// Absolute path to browse. Defaults to user's home directory.
    pub path: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct BrowseResponse {
    /// The absolute path being browsed.
    pub path: String,
    /// Parent directory path, or null if at root.
    pub parent: Option<String>,
    /// Subdirectory entries at this path.
    pub entries: Vec<DirectoryEntry>,
}

#[derive(Debug, Serialize)]
pub struct DirectoryEntry {
    /// Directory name.
    pub name: String,
    /// Full absolute path.
    pub path: String,
    /// Whether this directory contains subdirectories.
    pub has_children: bool,
}

/// Directories to skip when listing (noise, not useful for project selection).
const SKIP_DIRS: &[&str] = &[
    "node_modules",
    "target",
    ".git",
    "__pycache__",
    ".cache",
    "dist",
    "build",
    ".next",
    ".nuxt",
    ".svelte-kit",
    "vendor",
    ".tox",
    ".venv",
    "venv",
    ".gradle",
    ".idea",
    ".vscode",
];

/// Hard cap on how many raw directory entries are inspected per browse request.
const MAX_BROWSE_SCAN_ENTRIES: usize = 5_000;
/// Hard cap on how many directory items are returned per browse request.
const MAX_BROWSE_RESULTS: usize = 500;
/// Hard cap for child scans used to compute `has_children`.
const MAX_CHILD_SCAN_ENTRIES: usize = 200;

/// Browse directories at a given path.
///
/// Returns subdirectories with metadata for building a directory browser UI.
/// Skips hidden directories and common noise directories.
pub async fn browse_filesystem(
    Query(query): Query<BrowseQuery>,
) -> Result<Json<BrowseResponse>, ApiError> {
    let browse_path = resolve_browse_path(&query)?;
    let root = validate_browse_root(&browse_path)?;
    let entries = list_directory_entries(root.clone()).await?;
    let parent = root.parent().map(|p| p.to_string_lossy().to_string());

    Ok(Json(BrowseResponse {
        path: browse_path,
        parent,
        entries,
    }))
}

fn resolve_browse_path(query: &BrowseQuery) -> Result<String, ApiError> {
    match &query.path {
        Some(path) => Ok(path.clone()),
        None => {
            let home = dirs::home_dir().ok_or_else(|| {
                ApiError::from((
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "Could not determine home directory".to_string(),
                ))
            })?;
            Ok(home.to_string_lossy().to_string())
        }
    }
}

fn validate_browse_root(path: &str) -> Result<PathBuf, ApiError> {
    let root = Path::new(path);

    if !root.is_absolute() {
        return Err(ApiError::from((
            StatusCode::BAD_REQUEST,
            "Path must be absolute".to_string(),
        )));
    }

    let restrictions = crate::api::config::PathRestrictions::from_env();
    if let Err(e) = restrictions.validate(root) {
        return Err(ApiError::from((
            StatusCode::FORBIDDEN,
            format!("Access denied: {}", e),
        )));
    }

    if !root.exists() {
        return Err(ApiError::from((
            StatusCode::NOT_FOUND,
            "Directory not found".to_string(),
        )));
    }
    if !root.is_dir() {
        return Err(ApiError::from((
            StatusCode::BAD_REQUEST,
            "Path is not a directory".to_string(),
        )));
    }

    Ok(root.to_path_buf())
}

async fn list_directory_entries(root: PathBuf) -> Result<Vec<DirectoryEntry>, ApiError> {
    tokio::task::spawn_blocking(move || -> Result<Vec<DirectoryEntry>, String> {
        let read_dir =
            std::fs::read_dir(&root).map_err(|e| format!("Cannot read directory: {}", e))?;

        let mut entries = Vec::new();
        let mut scanned = 0usize;

        for entry in read_dir {
            if scanned >= MAX_BROWSE_SCAN_ENTRIES || entries.len() >= MAX_BROWSE_RESULTS {
                tracing::info!(
                    path = %root.display(),
                    scanned,
                    returned = entries.len(),
                    "Directory browse capped by configured limits"
                );
                break;
            }
            scanned += 1;

            let Ok(entry) = entry else {
                continue;
            };
            let Ok(file_type) = entry.file_type() else {
                continue;
            };
            if !file_type.is_dir() {
                continue;
            }

            let name = entry.file_name().to_string_lossy().to_string();
            if name.starts_with('.') || SKIP_DIRS.contains(&name.as_str()) {
                continue;
            }

            let entry_path = entry.path();
            entries.push(DirectoryEntry {
                name,
                path: entry_path.to_string_lossy().to_string(),
                has_children: peek_has_subdirs(&entry_path, MAX_CHILD_SCAN_ENTRIES),
            });
        }

        entries.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
        Ok(entries)
    })
    .await
    .map_err(|e| ApiError::from((StatusCode::INTERNAL_SERVER_ERROR, e.to_string())))?
    .map_err(|e| ApiError::from((StatusCode::FORBIDDEN, e)))
}

#[derive(Debug, Deserialize, Validate)]
pub struct MkdirRequest {
    /// Absolute path of the directory to create.
    #[validate(length(min = 1, max = 4_096))]
    pub path: String,
}

#[derive(Debug, Serialize)]
pub struct MkdirResponse {
    /// The absolute path that was created.
    pub path: String,
}

/// Create a directory, including intermediate directories.
///
/// Safety guarantees:
/// - Path must be absolute
/// - Path must pass security restrictions (no /etc, /var, etc.)
/// - Path must not contain `..` components
/// - Idempotent: succeeds if the directory already exists
pub async fn create_directory(
    ValidatedJson(body): ValidatedJson<MkdirRequest>,
) -> Result<Json<MkdirResponse>, ApiError> {
    let target = Path::new(&body.path);

    // Must be absolute
    if !target.is_absolute() {
        return Err(ApiError::from((
            StatusCode::BAD_REQUEST,
            "Path must be absolute".to_string(),
        )));
    }

    // Reject path traversal components
    for component in target.components() {
        if matches!(component, Component::ParentDir) {
            return Err(ApiError::from((
                StatusCode::BAD_REQUEST,
                "Path must not contain '..' components".to_string(),
            )));
        }
    }

    // Validate the target path against security restrictions
    let restrictions = crate::api::config::PathRestrictions::from_env();
    if let Err(e) = restrictions.validate(target) {
        return Err(ApiError::from((
            StatusCode::FORBIDDEN,
            format!("Access denied: {}", e),
        )));
    }

    // Create directory and all intermediate parents (blocking I/O off async runtime)
    let target_owned = target.to_path_buf();
    tokio::task::spawn_blocking(move || std::fs::create_dir_all(&target_owned))
        .await
        .map_err(|e| ApiError::from((StatusCode::INTERNAL_SERVER_ERROR, e.to_string())))?
        .map_err(|e| {
            ApiError::from((
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Failed to create directory: {}", e),
            ))
        })?;

    Ok(Json(MkdirResponse { path: body.path }))
}

/// Check if a directory contains any visible subdirectories.
/// Early-exits on first match for performance.
fn peek_has_subdirs(path: &Path, max_scan_entries: usize) -> bool {
    let Ok(read_dir) = std::fs::read_dir(path) else {
        return false;
    };

    let mut scanned = 0usize;
    for entry in read_dir.flatten() {
        if scanned >= max_scan_entries {
            return false;
        }
        scanned += 1;

        let Ok(ft) = entry.file_type() else {
            continue;
        };
        if !ft.is_dir() {
            continue;
        }
        let name = entry.file_name();
        let name_str = name.to_string_lossy();
        if name_str.starts_with('.') {
            continue;
        }
        if SKIP_DIRS.contains(&name_str.as_ref()) {
            continue;
        }
        return true;
    }

    false
}

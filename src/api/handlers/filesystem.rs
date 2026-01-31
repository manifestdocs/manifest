use axum::{extract::Query, http::StatusCode, Json};
use serde::{Deserialize, Serialize};
use std::path::Path;

use super::ApiError;

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

/// Browse directories at a given path.
///
/// Returns subdirectories with metadata for building a directory browser UI.
/// Skips hidden directories and common noise directories.
pub async fn browse_filesystem(
    Query(query): Query<BrowseQuery>,
) -> Result<Json<BrowseResponse>, ApiError> {
    let browse_path = match &query.path {
        Some(p) => p.clone(),
        None => {
            let home = dirs::home_dir().ok_or_else(|| {
                ApiError::from((
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "Could not determine home directory".to_string(),
                ))
            })?;
            home.to_string_lossy().to_string()
        }
    };

    let root = Path::new(&browse_path);

    // Validate path is absolute
    if !root.is_absolute() {
        return Err(ApiError::from((
            StatusCode::BAD_REQUEST,
            "Path must be absolute".to_string(),
        )));
    }

    // Validate path against security restrictions
    let restrictions = crate::api::config::PathRestrictions::from_env();
    if let Err(e) = restrictions.validate(root) {
        return Err(ApiError::from((
            StatusCode::FORBIDDEN,
            format!("Access denied: {}", e),
        )));
    }

    // Validate directory exists
    if !root.exists() {
        return Err(ApiError::from((
            StatusCode::NOT_FOUND,
            format!("Directory not found: {}", browse_path),
        )));
    }
    if !root.is_dir() {
        return Err(ApiError::from((
            StatusCode::BAD_REQUEST,
            format!("Path is not a directory: {}", browse_path),
        )));
    }

    // Read directory entries
    let mut entries = Vec::new();
    let read_dir = std::fs::read_dir(root).map_err(|e| {
        ApiError::from((
            StatusCode::FORBIDDEN,
            format!("Cannot read directory: {}", e),
        ))
    })?;

    for entry in read_dir.flatten() {
        let file_type = match entry.file_type() {
            Ok(ft) => ft,
            Err(_) => continue,
        };

        if !file_type.is_dir() {
            continue;
        }

        let name = entry.file_name().to_string_lossy().to_string();

        // Skip hidden directories
        if name.starts_with('.') {
            continue;
        }

        // Skip noise directories
        if SKIP_DIRS.contains(&name.as_str()) {
            continue;
        }

        let entry_path = entry.path();
        let has_children = peek_has_subdirs(&entry_path);

        entries.push(DirectoryEntry {
            name,
            path: entry_path.to_string_lossy().to_string(),
            has_children,
        });
    }

    entries.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));

    let parent = root.parent().map(|p| p.to_string_lossy().to_string());

    Ok(Json(BrowseResponse {
        path: browse_path,
        parent,
        entries,
    }))
}

/// Check if a directory contains any visible subdirectories.
/// Early-exits on first match for performance.
fn peek_has_subdirs(path: &Path) -> bool {
    let Ok(read_dir) = std::fs::read_dir(path) else {
        return false;
    };

    for entry in read_dir.flatten() {
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

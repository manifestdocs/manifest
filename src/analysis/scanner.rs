//! Directory and module scanning for codebase analysis.

use std::path::Path;

use crate::mcp::{DirectorySignal, ModuleSignal, ProjectType};

/// Scan directories up to max_depth.
pub fn scan_directories(root: &Path, max_depth: u32) -> Vec<DirectorySignal> {
    let mut directories = Vec::new();
    scan_directories_recursive(root, root, 0, max_depth, &mut directories);
    directories
}

fn scan_directories_recursive(
    base: &Path,
    current: &Path,
    depth: u32,
    max_depth: u32,
    result: &mut Vec<DirectorySignal>,
) {
    if depth > max_depth {
        return;
    }

    let entries = match std::fs::read_dir(current) {
        Ok(e) => e,
        Err(_) => return,
    };

    for entry in entries.filter_map(|e| e.ok()) {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }

        let name = entry.file_name().to_string_lossy().to_string();

        // Skip hidden and common excluded directories
        if name.starts_with('.')
            || name == "node_modules"
            || name == "target"
            || name == "__pycache__"
            || name == "venv"
            || name == ".venv"
            || name == "dist"
            || name == "build"
            || name == "coverage"
        {
            continue;
        }

        // Count files in this directory (non-recursive)
        let file_count = std::fs::read_dir(&path)
            .map(|entries| {
                entries
                    .filter_map(|e| e.ok())
                    .filter(|e| e.path().is_file())
                    .count()
            })
            .unwrap_or(0) as u32;

        // Only include directories with files or that are significant
        let kind = classify_directory(&name);
        if file_count > 0 || kind != "unknown" {
            let relative_path = path.strip_prefix(base).unwrap_or(&path);
            result.push(DirectorySignal {
                path: relative_path.to_string_lossy().to_string(),
                kind: kind.to_string(),
                file_count,
            });
        }

        // Recurse into subdirectories
        scan_directories_recursive(base, &path, depth + 1, max_depth, result);
    }
}

fn classify_directory(name: &str) -> &'static str {
    match name.to_lowercase().as_str() {
        "src" | "lib" | "app" | "source" | "sources" => "source",
        "tests" | "test" | "__tests__" | "spec" | "specs" => "tests",
        "docs" | "doc" | "documentation" => "docs",
        "config" | "configs" | ".config" | "settings" => "config",
        "api" | "handlers" | "routes" | "endpoints" => "source",
        "models" | "entities" | "schemas" => "source",
        "utils" | "helpers" | "common" | "shared" => "source",
        "components" | "views" | "pages" => "source",
        "services" | "core" | "domain" => "source",
        _ => "unknown",
    }
}

/// Detect modules based on language conventions.
pub fn detect_modules(root: &Path, project_type: &ProjectType) -> Vec<ModuleSignal> {
    let mut modules = Vec::new();
    let src_dirs = ["src", "lib", "app"];

    for src_dir in &src_dirs {
        let src_path = root.join(src_dir);
        if !src_path.exists() {
            continue;
        }

        detect_modules_in_dir(&src_path, root, project_type, &mut modules);
    }

    // Also check root for modules
    detect_modules_in_dir(root, root, project_type, &mut modules);

    modules
}

fn detect_modules_in_dir(
    dir: &Path,
    base: &Path,
    project_type: &ProjectType,
    modules: &mut Vec<ModuleSignal>,
) {
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return,
    };

    for entry in entries.filter_map(|e| e.ok()) {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().to_string();

        // Skip hidden
        if name.starts_with('.') {
            continue;
        }

        if path.is_dir() {
            // Check for module index files
            let is_module = match project_type.language.as_str() {
                "rust" => path.join("mod.rs").exists() || path.join("lib.rs").exists(),
                "typescript" | "javascript" => {
                    path.join("index.ts").exists()
                        || path.join("index.tsx").exists()
                        || path.join("index.js").exists()
                }
                "python" => path.join("__init__.py").exists(),
                "go" => {
                    // Go packages are directories with .go files
                    std::fs::read_dir(&path)
                        .map(|entries| {
                            entries
                                .filter_map(|e| e.ok())
                                .any(|e| e.path().extension().is_some_and(|ext| ext == "go"))
                        })
                        .unwrap_or(false)
                }
                _ => false,
            };

            if is_module {
                // Count files to determine if major
                let file_count = count_source_files(&path, project_type);
                let is_major = file_count > 5
                    || matches!(
                        name.to_lowercase().as_str(),
                        "api" | "core" | "db" | "handlers" | "models" | "services" | "domain"
                    );

                let relative_path = path.strip_prefix(base).unwrap_or(&path);
                modules.push(ModuleSignal {
                    name: name.clone(),
                    path: relative_path.to_string_lossy().to_string(),
                    is_major,
                });
            }
        }
    }
}

/// Count source files in a directory (recursive).
fn count_source_files(dir: &Path, project_type: &ProjectType) -> u32 {
    let extensions: &[&str] = match project_type.language.as_str() {
        "rust" => &["rs"],
        "typescript" => &["ts", "tsx"],
        "javascript" => &["js", "jsx"],
        "python" => &["py"],
        "go" => &["go"],
        "fsharp" => &["fs", "fsi"],
        _ => &[],
    };

    std::fs::read_dir(dir)
        .map(|entries| {
            entries
                .filter_map(|e| e.ok())
                .filter(|e| {
                    let path = e.path();
                    if path.is_file() {
                        path.extension()
                            .is_some_and(|ext| extensions.iter().any(|e| ext == *e))
                    } else {
                        false
                    }
                })
                .count()
        })
        .unwrap_or(0) as u32
}

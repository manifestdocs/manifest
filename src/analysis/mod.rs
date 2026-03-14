//! Codebase analysis for project discovery.
//!
//! Analyzes project directories to detect language, frameworks, modules,
//! and generate feature hints for AI agents.

pub mod feature_extractor;
pub mod git_history;
pub mod markdown_gen;
mod parsers;
mod scanner;

use std::path::Path;
use std::process::Command;

use crate::mcp::{
    DirectorySignal, DocumentationContent, FeatureHint, ModuleSignal, ProjectAnalysis, ProjectType,
};

pub use parsers::{detect_project_type, extract_project_metadata, get_git_remote};
pub use scanner::{detect_modules, scan_directories};

/// Analyze a codebase directory to discover project structure.
///
/// Returns detected language, frameworks, modules, and documentation.
/// Used by AI agents before plan_features to understand what capabilities exist.
pub fn analyze(root: &Path, include_docs: bool, max_depth: u32) -> ProjectAnalysis {
    let project_type = detect_project_type(root);
    let (name, description) = extract_project_metadata(root, &project_type);
    let git_remote = get_git_remote(root);
    let directories = scan_directories(root, max_depth);
    let modules = detect_modules(root, &project_type);

    let documentation = if include_docs {
        Some(read_documentation(root))
    } else {
        None
    };

    let hints = generate_feature_hints(root, &directories, &modules, &project_type);
    let file_count = count_git_files(root);
    let commit_count = count_git_commits(root);
    let (has_subprojects, subproject_paths) = detect_subprojects(root);

    ProjectAnalysis {
        name,
        description,
        project_type,
        git_remote,
        directories,
        modules,
        documentation,
        hints,
        file_count,
        commit_count,
        has_subprojects,
        subproject_paths,
    }
}

/// Count tracked files via `git ls-files`. Returns 0 if not a git repo.
fn count_git_files(root: &Path) -> u32 {
    Command::new("git")
        .args(["ls-files"])
        .current_dir(root)
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).lines().count() as u32)
        .unwrap_or(0)
}

/// Count git commits on current branch. Returns 0 if not a git repo.
fn count_git_commits(root: &Path) -> u32 {
    Command::new("git")
        .args(["rev-list", "--count", "HEAD"])
        .current_dir(root)
        .output()
        .ok()
        .filter(|o| o.status.success())
        .and_then(|o| {
            String::from_utf8_lossy(&o.stdout)
                .trim()
                .parse::<u32>()
                .ok()
        })
        .unwrap_or(0)
}

/// Detect subprojects by scanning for build manifests at depth > 0.
///
/// Returns `(true, paths)` if 2+ manifest files found at different directory levels,
/// indicating a monorepo structure.
fn detect_subprojects(root: &Path) -> (bool, Vec<String>) {
    use std::collections::HashSet;

    let skip_dirs: HashSet<&str> = ["node_modules", "target", ".git", "vendor", "dist", "build"]
        .into_iter()
        .collect();

    let mut subproject_roots: Vec<String> = Vec::new();

    detect_subprojects_walk(root, root, 0, 3, &skip_dirs, &mut subproject_roots);

    let has = subproject_roots.len() >= 2;
    (has, subproject_roots)
}

fn detect_subprojects_walk(
    root: &Path,
    dir: &Path,
    depth: u32,
    max_depth: u32,
    skip_dirs: &std::collections::HashSet<&str>,
    results: &mut Vec<String>,
) {
    if depth > max_depth {
        return;
    }

    // Only check depth > 0 (skip root-level manifests)
    if depth > 0 {
        let is_subproject = is_cargo_workspace_member(dir)
            || has_package_json_workspaces(dir)
            || dir.join("go.mod").exists()
            || dir.join("pyproject.toml").exists();

        if is_subproject {
            if let Ok(rel) = dir.strip_prefix(root) {
                results.push(rel.to_string_lossy().to_string());
            }
            return; // Don't recurse into subprojects
        }
    }

    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let name = entry.file_name();
        let name_str = name.to_string_lossy();
        if name_str.starts_with('.') || skip_dirs.contains(name_str.as_ref()) {
            continue;
        }
        detect_subprojects_walk(root, &path, depth + 1, max_depth, skip_dirs, results);
    }
}

/// Check if a directory has its own Cargo.toml (not a workspace root).
fn is_cargo_workspace_member(dir: &Path) -> bool {
    let cargo_toml = dir.join("Cargo.toml");
    if !cargo_toml.exists() {
        return false;
    }
    // It's a subproject if it has Cargo.toml but is NOT a workspace root
    if let Ok(content) = std::fs::read_to_string(&cargo_toml) {
        return !content.contains("[workspace]");
    }
    false
}

/// Check if a directory has package.json (but not one with "workspaces" — that's a workspace root).
fn has_package_json_workspaces(dir: &Path) -> bool {
    let pkg = dir.join("package.json");
    if !pkg.exists() {
        return false;
    }
    if let Ok(content) = std::fs::read_to_string(&pkg) {
        // It's a subproject if it has package.json without "workspaces"
        return !content.contains("\"workspaces\"");
    }
    false
}

/// Read documentation files.
fn read_documentation(root: &Path) -> DocumentationContent {
    let readme = read_doc_file(root, &["README.md", "README", "readme.md", "Readme.md"]);
    let claude_md = read_doc_file(root, &["CLAUDE.md", "claude.md"]);

    DocumentationContent { readme, claude_md }
}

fn read_doc_file(root: &Path, names: &[&str]) -> Option<String> {
    for name in names {
        let path = root.join(name);
        if path.exists() {
            if let Ok(content) = std::fs::read_to_string(&path) {
                // Truncate to ~500 lines
                let lines: Vec<&str> = content.lines().take(500).collect();
                let truncated = lines.join("\n");
                if lines.len() == 500 && content.lines().count() > 500 {
                    return Some(format!("{}\n\n... (truncated)", truncated));
                }
                return Some(truncated);
            }
        }
    }
    None
}

/// Generate feature hints from project structure.
fn generate_feature_hints(
    root: &Path,
    directories: &[DirectorySignal],
    modules: &[ModuleSignal],
    project_type: &ProjectType,
) -> Vec<FeatureHint> {
    use std::collections::HashMap;

    let mut hints = Vec::new();
    let mut seen_hints: HashMap<String, bool> = HashMap::new();

    // Hint from major modules
    for module in modules.iter().filter(|m| m.is_major) {
        let title = match module.name.to_lowercase().as_str() {
            "api" | "handlers" | "routes" | "endpoints" => "HTTP API",
            "db" | "database" | "persistence" | "storage" => "Data Persistence",
            "auth" | "authentication" => "Authentication",
            "models" | "entities" | "domain" => "Domain Model",
            "mcp" => "MCP Server",
            "cli" => "CLI Interface",
            _ => &module.name,
        };

        if !seen_hints.contains_key(title) {
            seen_hints.insert(title.to_string(), true);
            hints.push(FeatureHint {
                title: title.to_string(),
                reason: format!("Major module detected: {}", module.name),
                paths: vec![module.path.clone()],
            });
        }
    }

    // Hint from source directories with significant content
    for dir in directories
        .iter()
        .filter(|d| d.kind == "source" && d.file_count > 3)
    {
        let dir_name = std::path::Path::new(&dir.path)
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default();

        let title = match dir_name.to_lowercase().as_str() {
            "api" | "handlers" | "routes" => "HTTP API",
            "db" | "database" | "models" => "Data Persistence",
            "auth" => "Authentication",
            "components" | "views" => "UI Components",
            "services" => "Business Logic",
            _ => continue,
        };

        if !seen_hints.contains_key(title) {
            seen_hints.insert(title.to_string(), true);
            hints.push(FeatureHint {
                title: title.to_string(),
                reason: format!("Source directory with {} files", dir.file_count),
                paths: vec![dir.path.clone()],
            });
        }
    }

    // Hint from frameworks
    for framework in &project_type.frameworks {
        let title = match framework.as_str() {
            "axum" | "actix" | "rocket" | "warp" | "express" | "fastify" | "fastapi" | "flask"
            | "django" => "HTTP API",
            "sveltekit" | "next" => "Server-Side Rendering",
            "react" | "vue" | "svelte" => "Frontend UI",
            _ => continue,
        };

        if !seen_hints.contains_key(title) {
            seen_hints.insert(title.to_string(), true);
            hints.push(FeatureHint {
                title: title.to_string(),
                reason: format!("Framework detected: {}", framework),
                paths: Vec::new(),
            });
        }
    }

    // Check for specific files that indicate features
    if (root.join("Dockerfile").exists() || root.join("docker-compose.yml").exists())
        && !seen_hints.contains_key("Container Deployment")
    {
        hints.push(FeatureHint {
            title: "Container Deployment".to_string(),
            reason: "Docker configuration found".to_string(),
            paths: vec!["Dockerfile".to_string()],
        });
    }

    if (root.join("openapi.yaml").exists() || root.join("openapi.json").exists())
        && !seen_hints.contains_key("API Documentation")
    {
        hints.push(FeatureHint {
            title: "API Documentation".to_string(),
            reason: "OpenAPI spec found".to_string(),
            paths: vec!["openapi.yaml".to_string()],
        });
    }

    hints
}

/// Generate a feature tree markdown document from a codebase.
///
/// Combines:
/// - Static code analysis (modules, directories, frameworks)
/// - Git history analysis (feat: commits, deletions)
/// - Optional external symbol data
///
/// Returns the markdown document and statistics.
pub fn generate_feature_tree(
    root: &Path,
    project_name: &str,
    since: Option<&str>,
    symbols: Option<&[feature_extractor::SymbolData]>,
) -> (String, feature_extractor::TreeStats) {
    // 1. Run existing analysis
    let analysis = analyze(root, false, 3);

    // 2. Analyze git history
    let git = git_history::analyze_git_history(root, since, 500);

    // 3. Extract features
    let tree = feature_extractor::extract_features(&analysis, &git, symbols);

    // 4. Generate markdown
    let options = markdown_gen::MarkdownOptions {
        project_name: project_name.to_string(),
        branch: git.branch,
        commit_sha: git.head_sha,
        include_files: true,
        include_evidence: true,
        max_files: 5,
    };

    let stats = tree.stats.clone();
    let markdown = markdown_gen::generate_markdown(&tree, &options);

    (markdown, stats)
}

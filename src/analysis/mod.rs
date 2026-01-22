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

    ProjectAnalysis {
        name,
        description,
        project_type,
        git_remote,
        directories,
        modules,
        documentation,
        hints,
    }
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
/// - Optional RocketIndex symbols
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

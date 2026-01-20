//! Project manifest parsing for type detection and metadata extraction.

use std::path::Path;

use crate::mcp::ProjectType;

/// Detect project type from manifest files.
pub fn detect_project_type(root: &Path) -> ProjectType {
    // Check for Cargo.toml (Rust)
    if root.join("Cargo.toml").exists() {
        let mut frameworks = Vec::new();

        // Read Cargo.toml to detect frameworks
        if let Ok(content) = std::fs::read_to_string(root.join("Cargo.toml")) {
            if content.contains("axum") {
                frameworks.push("axum".to_string());
            }
            if content.contains("actix") {
                frameworks.push("actix".to_string());
            }
            if content.contains("rocket") {
                frameworks.push("rocket".to_string());
            }
            if content.contains("warp") {
                frameworks.push("warp".to_string());
            }
            if content.contains("tokio") {
                frameworks.push("tokio".to_string());
            }
        }

        return ProjectType {
            language: "rust".to_string(),
            frameworks,
            build_tool: Some("cargo".to_string()),
        };
    }

    // Check for package.json (TypeScript/JavaScript)
    if root.join("package.json").exists() {
        let mut frameworks = Vec::new();
        let mut language = "javascript".to_string();
        let mut build_tool = None;

        if let Ok(content) = std::fs::read_to_string(root.join("package.json")) {
            // Detect TypeScript
            if root.join("tsconfig.json").exists() || content.contains("typescript") {
                language = "typescript".to_string();
            }

            // Detect build tool
            if content.contains("\"pnpm\"") || root.join("pnpm-lock.yaml").exists() {
                build_tool = Some("pnpm".to_string());
            } else if root.join("yarn.lock").exists() {
                build_tool = Some("yarn".to_string());
            } else {
                build_tool = Some("npm".to_string());
            }

            // Detect frameworks
            if content.contains("svelte") {
                frameworks.push("svelte".to_string());
            }
            if content.contains("@sveltejs/kit") {
                frameworks.push("sveltekit".to_string());
            }
            if content.contains("\"react\"") {
                frameworks.push("react".to_string());
            }
            if content.contains("\"next\"") {
                frameworks.push("next".to_string());
            }
            if content.contains("\"vue\"") {
                frameworks.push("vue".to_string());
            }
            if content.contains("\"express\"") {
                frameworks.push("express".to_string());
            }
            if content.contains("\"fastify\"") {
                frameworks.push("fastify".to_string());
            }
        }

        return ProjectType {
            language,
            frameworks,
            build_tool,
        };
    }

    // Check for F#/C# projects
    let fsproj = std::fs::read_dir(root).ok().and_then(|entries| {
        entries
            .filter_map(|e| e.ok())
            .find(|e| e.path().extension().is_some_and(|ext| ext == "fsproj"))
    });
    if fsproj.is_some() || root.join("*.sln").exists() {
        return ProjectType {
            language: "fsharp".to_string(),
            frameworks: Vec::new(),
            build_tool: Some("dotnet".to_string()),
        };
    }

    // Check for Python
    if root.join("pyproject.toml").exists() || root.join("setup.py").exists() {
        let mut frameworks = Vec::new();
        let mut build_tool = None;

        if root.join("pyproject.toml").exists() {
            if let Ok(content) = std::fs::read_to_string(root.join("pyproject.toml")) {
                if content.contains("poetry") {
                    build_tool = Some("poetry".to_string());
                }
                if content.contains("fastapi") {
                    frameworks.push("fastapi".to_string());
                }
                if content.contains("django") {
                    frameworks.push("django".to_string());
                }
                if content.contains("flask") {
                    frameworks.push("flask".to_string());
                }
            }
        }
        if build_tool.is_none() {
            build_tool = Some("pip".to_string());
        }

        return ProjectType {
            language: "python".to_string(),
            frameworks,
            build_tool,
        };
    }

    // Check for Go
    if root.join("go.mod").exists() {
        return ProjectType {
            language: "go".to_string(),
            frameworks: Vec::new(),
            build_tool: Some("go".to_string()),
        };
    }

    // Unknown
    ProjectType {
        language: "unknown".to_string(),
        frameworks: Vec::new(),
        build_tool: None,
    }
}

/// Extract project name and description from manifest files.
pub fn extract_project_metadata(
    root: &Path,
    project_type: &ProjectType,
) -> (Option<String>, Option<String>) {
    match project_type.language.as_str() {
        "rust" => {
            if let Ok(content) = std::fs::read_to_string(root.join("Cargo.toml")) {
                let name = extract_toml_value(&content, "name");
                let description = extract_toml_value(&content, "description");
                return (name, description);
            }
        }
        "typescript" | "javascript" => {
            if let Ok(content) = std::fs::read_to_string(root.join("package.json")) {
                if let Ok(json) = serde_json::from_str::<serde_json::Value>(&content) {
                    let name = json.get("name").and_then(|v| v.as_str()).map(String::from);
                    let description = json
                        .get("description")
                        .and_then(|v| v.as_str())
                        .map(String::from);
                    return (name, description);
                }
            }
        }
        "python" => {
            if let Ok(content) = std::fs::read_to_string(root.join("pyproject.toml")) {
                let name = extract_toml_value(&content, "name");
                let description = extract_toml_value(&content, "description");
                return (name, description);
            }
        }
        "go" => {
            if let Ok(content) = std::fs::read_to_string(root.join("go.mod")) {
                // First line is usually "module github.com/org/name"
                if let Some(line) = content.lines().next() {
                    if line.starts_with("module ") {
                        let module = line.trim_start_matches("module ").trim();
                        let name = module.rsplit('/').next().map(String::from);
                        return (name, None);
                    }
                }
            }
        }
        _ => {}
    }
    (None, None)
}

/// Simple TOML value extraction (handles quoted strings).
fn extract_toml_value(content: &str, key: &str) -> Option<String> {
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with(&format!("{} =", key)) || trimmed.starts_with(&format!("{}=", key)) {
            let value = trimmed.split('=').nth(1)?.trim();
            // Remove quotes
            let unquoted = value.trim_matches('"').trim_matches('\'');
            if !unquoted.is_empty() {
                return Some(unquoted.to_string());
            }
        }
    }
    None
}

/// Get git remote URL from .git/config.
pub fn get_git_remote(root: &Path) -> Option<String> {
    let config_path = root.join(".git/config");
    if let Ok(content) = std::fs::read_to_string(config_path) {
        // Look for [remote "origin"] section and url =
        let mut in_origin = false;
        for line in content.lines() {
            let trimmed = line.trim();
            if trimmed == "[remote \"origin\"]" {
                in_origin = true;
            } else if trimmed.starts_with('[') {
                in_origin = false;
            } else if in_origin && trimmed.starts_with("url = ") {
                return Some(trimmed.trim_start_matches("url = ").to_string());
            }
        }
    }
    None
}

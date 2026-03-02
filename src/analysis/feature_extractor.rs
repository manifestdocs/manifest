//! Feature extraction from codebase analysis.
//!
//! Synthesizes features from multiple sources:
//! - Existing project analysis (modules, directories, hints)
//! - Git history (feat: commits, deletions)
//! - External symbol data (if available)

use std::collections::HashMap;

use crate::mcp::{ModuleSignal, ProjectAnalysis};

use super::git_history::{CommitType, FeatureCommit, GitAnalysis};

/// The extracted feature tree ready for markdown generation.
#[derive(Debug, Clone)]
pub struct ExtractedFeatureTree {
    /// Top-level chapters (major capability groupings).
    pub chapters: Vec<Chapter>,
    /// Statistics about the extraction.
    pub stats: TreeStats,
    /// Warnings encountered during extraction.
    pub warnings: Vec<String>,
}

/// A top-level grouping of features (e.g., "Authentication", "HTTP API").
#[derive(Debug, Clone)]
pub struct Chapter {
    /// Chapter title.
    pub title: String,
    /// How this chapter was identified.
    pub source: ChapterSource,
    /// Module path if from module analysis.
    pub module_path: Option<String>,
    /// Number of files in this chapter's scope.
    pub file_count: u32,
    /// Features within this chapter.
    pub features: Vec<ExtractedFeature>,
}

/// How a chapter was identified.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChapterSource {
    /// Detected from module structure (src/api/, etc.)
    Module,
    /// Detected from hub symbols (external indexer)
    Symbol,
    /// Inferred from framework (axum → HTTP API)
    Framework,
    /// From git history patterns
    GitHistory,
}

impl ChapterSource {
    pub fn as_str(&self) -> &'static str {
        match self {
            ChapterSource::Module => "Module",
            ChapterSource::Symbol => "Symbol",
            ChapterSource::Framework => "Framework",
            ChapterSource::GitHistory => "Git History",
        }
    }
}

/// An extracted feature.
#[derive(Debug, Clone)]
pub struct ExtractedFeature {
    /// Feature title.
    pub title: String,
    /// Brief description if available.
    pub description: Option<String>,
    /// Current state.
    pub state: FeatureState,
    /// Evidence supporting this feature.
    pub evidence: Vec<Evidence>,
    /// Child features (for hierarchical structure).
    pub children: Vec<ExtractedFeature>,
    /// Related file paths.
    pub files: Vec<String>,
}

/// State of an extracted feature.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FeatureState {
    /// Feature exists in codebase.
    Implemented,
    /// Feature was proposed in commits but may not be complete.
    Proposed,
    /// Feature was removed (detected deletion).
    Deprecated,
}

impl FeatureState {
    pub fn as_str(&self) -> &'static str {
        match self {
            FeatureState::Implemented => "implemented",
            FeatureState::Proposed => "proposed",
            FeatureState::Deprecated => "deprecated",
        }
    }

    pub fn symbol(&self) -> &'static str {
        match self {
            FeatureState::Implemented => "●",
            FeatureState::Proposed => "◇",
            FeatureState::Deprecated => "✗",
        }
    }
}

/// Evidence for a feature's existence.
#[derive(Debug, Clone)]
pub struct Evidence {
    /// Type of evidence.
    pub kind: EvidenceKind,
    /// Human-readable description.
    pub description: String,
}

/// Types of evidence.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EvidenceKind {
    /// From a commit message.
    Commit,
    /// From a symbol definition.
    Symbol,
    /// From module/directory structure.
    Module,
    /// From file deletion.
    Deletion,
}

impl EvidenceKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            EvidenceKind::Commit => "Commit",
            EvidenceKind::Symbol => "Symbol",
            EvidenceKind::Module => "Module",
            EvidenceKind::Deletion => "Deleted",
        }
    }
}

/// Statistics about the extraction.
#[derive(Debug, Clone, Default)]
pub struct TreeStats {
    pub total_chapters: u32,
    pub total_features: u32,
    pub implemented_count: u32,
    pub proposed_count: u32,
    pub deprecated_count: u32,
    pub commits_analyzed: u32,
}

/// External symbol data for optional enrichment.
#[derive(Debug, Clone)]
pub struct SymbolData {
    /// Symbol name.
    pub name: String,
    /// Fully qualified name.
    pub qualified_name: String,
    /// Symbol kind (function, class, module, etc.).
    pub kind: String,
    /// File where defined.
    pub file: String,
    /// Number of references across codebase.
    pub reference_count: u32,
}

/// Extract feature tree from analysis results.
pub fn extract_features(
    analysis: &ProjectAnalysis,
    git: &GitAnalysis,
    symbols: Option<&[SymbolData]>,
) -> ExtractedFeatureTree {
    let mut chapters: Vec<Chapter> = Vec::new();
    let mut warnings: Vec<String> = Vec::new();
    let mut stats = TreeStats::default();

    // Track used feature names to avoid duplicates
    let mut used_features: HashMap<String, usize> = HashMap::new();

    // 1. Create chapters from major modules
    for module in analysis.modules.iter().filter(|m| m.is_major) {
        let chapter_title = module_to_chapter_title(&module.name);
        if let Some(&chapter_idx) = used_features.get(&chapter_title) {
            // Add to existing chapter
            chapters[chapter_idx].file_count += count_module_files(module);
        } else {
            let chapter_idx = chapters.len();
            chapters.push(Chapter {
                title: chapter_title.clone(),
                source: ChapterSource::Module,
                module_path: Some(module.path.clone()),
                file_count: count_module_files(module),
                features: Vec::new(),
            });
            used_features.insert(chapter_title, chapter_idx);
        }
    }

    // 2. Create chapters from framework detection
    for framework in &analysis.project_type.frameworks {
        if let Some(chapter_title) = framework_to_chapter(framework) {
            used_features
                .entry(chapter_title.clone())
                .or_insert_with(|| {
                    let chapter_idx = chapters.len();
                    chapters.push(Chapter {
                        title: chapter_title,
                        source: ChapterSource::Framework,
                        module_path: None,
                        file_count: 0,
                        features: Vec::new(),
                    });
                    chapter_idx
                });
        }
    }

    // 3. Add chapters from feature hints
    for hint in &analysis.hints {
        if !used_features.contains_key(&hint.title) {
            let chapter_idx = chapters.len();
            chapters.push(Chapter {
                title: hint.title.clone(),
                source: ChapterSource::Module,
                module_path: hint.paths.first().cloned(),
                file_count: 0,
                features: Vec::new(),
            });
            used_features.insert(hint.title.clone(), chapter_idx);
        }
    }

    // 4. Extract features from git commits
    let commit_features = extract_from_commits(&git.feature_commits);
    stats.commits_analyzed = git.feature_commits.len() as u32;

    // Assign features to chapters based on file paths
    for feature in commit_features {
        let chapter_idx = find_best_chapter(&feature, &chapters);
        if let Some(idx) = chapter_idx {
            chapters[idx].features.push(feature);
        } else {
            // Create "Other" chapter if needed
            let other_idx = used_features.entry("Other".to_string()).or_insert_with(|| {
                let idx = chapters.len();
                chapters.push(Chapter {
                    title: "Other".to_string(),
                    source: ChapterSource::GitHistory,
                    module_path: None,
                    file_count: 0,
                    features: Vec::new(),
                });
                idx
            });
            chapters[*other_idx].features.push(feature);
        }
    }

    // 5. Add deprecation features from deletions
    for deletion in &git.deletions {
        let feature = ExtractedFeature {
            title: path_to_feature_name(&deletion.path),
            description: Some(format!("Removed: {}", deletion.message)),
            state: FeatureState::Deprecated,
            evidence: vec![Evidence {
                kind: EvidenceKind::Deletion,
                description: format!("`{}` - {}", deletion.sha, deletion.message),
            }],
            children: Vec::new(),
            files: vec![deletion.path.clone()],
        };

        let chapter_idx = find_best_chapter(&feature, &chapters);
        if let Some(idx) = chapter_idx {
            chapters[idx].features.push(feature);
        }
    }

    // 6. Enrich with external symbols if available
    if let Some(symbols) = symbols {
        enrich_with_symbols(&mut chapters, symbols, &mut warnings);
    }

    // 7. Deduplicate and merge similar features within chapters
    for chapter in &mut chapters {
        deduplicate_features(&mut chapter.features);
    }

    // 8. Remove empty chapters
    chapters.retain(|c| !c.features.is_empty());

    // 9. Sort chapters alphabetically, but put "Other" last
    chapters.sort_by(|a, b| {
        if a.title == "Other" {
            std::cmp::Ordering::Greater
        } else if b.title == "Other" {
            std::cmp::Ordering::Less
        } else {
            a.title.cmp(&b.title)
        }
    });

    // 10. Calculate stats
    stats.total_chapters = chapters.len() as u32;
    for chapter in &chapters {
        for feature in &chapter.features {
            count_feature_stats(&mut stats, feature);
        }
    }

    ExtractedFeatureTree {
        chapters,
        stats,
        warnings,
    }
}

/// Convert module name to chapter title.
fn module_to_chapter_title(name: &str) -> String {
    match name.to_lowercase().as_str() {
        "api" | "handlers" | "routes" | "endpoints" => "HTTP API".to_string(),
        "db" | "database" | "persistence" | "storage" => "Data Persistence".to_string(),
        "auth" | "authentication" => "Authentication".to_string(),
        "models" | "entities" | "domain" => "Domain Model".to_string(),
        "mcp" => "MCP Server".to_string(),
        "cli" => "CLI Interface".to_string(),
        "components" | "ui" => "UI Components".to_string(),
        "services" => "Business Logic".to_string(),
        "analysis" => "Analysis".to_string(),
        _ => titlecase(name),
    }
}

/// Convert framework name to chapter title.
fn framework_to_chapter(framework: &str) -> Option<String> {
    match framework {
        "axum" | "actix" | "rocket" | "warp" | "express" | "fastify" | "fastapi" | "flask"
        | "django" => Some("HTTP API".to_string()),
        "sveltekit" | "next" => Some("Server-Side Rendering".to_string()),
        "react" | "vue" | "svelte" => Some("Frontend UI".to_string()),
        _ => None,
    }
}

/// Count files in a module (simplified).
fn count_module_files(_module: &ModuleSignal) -> u32 {
    // This would need actual file system access to be accurate
    // For now, return a placeholder
    5
}

/// Extract features from commit messages.
fn extract_from_commits(commits: &[FeatureCommit]) -> Vec<ExtractedFeature> {
    let mut features: Vec<ExtractedFeature> = Vec::new();
    let mut seen: HashMap<String, usize> = HashMap::new();

    for commit in commits {
        // Skip removal commits (handled separately)
        if commit.commit_type == CommitType::Remove {
            continue;
        }

        let normalized_name = normalize_feature_name(&commit.feature_name);
        if normalized_name.is_empty() {
            continue;
        }

        if let Some(&idx) = seen.get(&normalized_name) {
            // Add evidence to existing feature
            features[idx].evidence.push(Evidence {
                kind: EvidenceKind::Commit,
                description: format!("`{}` - {}", commit.sha, commit.message),
            });
            // Add files
            for file in &commit.files {
                if !features[idx].files.contains(file) {
                    features[idx].files.push(file.clone());
                }
            }
        } else {
            let idx = features.len();
            features.push(ExtractedFeature {
                title: titlecase(&commit.feature_name),
                description: None,
                state: FeatureState::Implemented,
                evidence: vec![Evidence {
                    kind: EvidenceKind::Commit,
                    description: format!("`{}` - {}", commit.sha, commit.message),
                }],
                children: Vec::new(),
                files: commit.files.clone(),
            });
            seen.insert(normalized_name, idx);
        }
    }

    features
}

/// Find the best matching chapter for a feature based on its files.
fn find_best_chapter(feature: &ExtractedFeature, chapters: &[Chapter]) -> Option<usize> {
    let mut best_match: Option<(usize, u32)> = None;

    for (idx, chapter) in chapters.iter().enumerate() {
        if let Some(ref module_path) = chapter.module_path {
            let matches: u32 = feature
                .files
                .iter()
                .filter(|f| f.contains(module_path))
                .count() as u32;
            if matches > 0 && best_match.map(|(_, c)| matches > c).unwrap_or(true) {
                best_match = Some((idx, matches));
            }
        }

        // Also check by title similarity
        let title_lower = chapter.title.to_lowercase();
        for file in &feature.files {
            let file_lower = file.to_lowercase();
            if (file_lower.contains(&title_lower)
                || title_lower.contains(
                    &file_lower
                        .split('/')
                        .next_back()
                        .unwrap_or("")
                        .replace(".rs", "")
                        .replace(".ts", ""),
                ))
                && best_match.is_none()
            {
                best_match = Some((idx, 1));
            }
        }
    }

    best_match.map(|(idx, _)| idx)
}

/// Convert a file path to a feature name.
fn path_to_feature_name(path: &str) -> String {
    let filename = path
        .split('/')
        .next_back()
        .unwrap_or(path)
        .split('.')
        .next()
        .unwrap_or(path);

    titlecase(filename)
}

/// Normalize a feature name for comparison.
fn normalize_feature_name(name: &str) -> String {
    name.to_lowercase()
        .chars()
        .filter(|c| c.is_alphanumeric() || *c == ' ')
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// Convert string to title case.
fn titlecase(s: &str) -> String {
    let mut result = String::new();
    let mut capitalize_next = true;

    for c in s.chars() {
        if c == '_' || c == '-' {
            result.push(' ');
            capitalize_next = true;
        } else if capitalize_next {
            result.extend(c.to_uppercase());
            capitalize_next = false;
        } else {
            result.push(c);
        }
    }

    result
}

/// Enrich chapters with external symbol data.
fn enrich_with_symbols(
    chapters: &mut [Chapter],
    symbols: &[SymbolData],
    warnings: &mut Vec<String>,
) {
    // Find high-reference symbols (architectural hubs)
    let hub_symbols: Vec<_> = symbols.iter().filter(|s| s.reference_count >= 5).collect();

    if hub_symbols.is_empty() {
        warnings.push("No hub symbols found in symbol data".to_string());
        return;
    }

    // Add evidence from symbols to matching features
    for chapter in chapters.iter_mut() {
        for feature in &mut chapter.features {
            for symbol in &hub_symbols {
                // Check if symbol file matches feature files
                if feature.files.iter().any(|f| f == &symbol.file) {
                    feature.evidence.push(Evidence {
                        kind: EvidenceKind::Symbol,
                        description: format!(
                            "`{}` ({}, {} refs)",
                            symbol.qualified_name, symbol.kind, symbol.reference_count
                        ),
                    });
                }
            }
        }
    }
}

/// Deduplicate and merge similar features.
fn deduplicate_features(features: &mut Vec<ExtractedFeature>) {
    // Sort by title for consistent ordering
    features.sort_by(|a, b| a.title.cmp(&b.title));

    // Merge features with very similar titles
    let mut i = 0;
    while i < features.len() {
        let mut j = i + 1;
        while j < features.len() {
            if are_similar_titles(&features[i].title, &features[j].title) {
                // Merge j into i
                let merged = features.remove(j);
                features[i].evidence.extend(merged.evidence);
                for file in merged.files {
                    if !features[i].files.contains(&file) {
                        features[i].files.push(file);
                    }
                }
            } else {
                j += 1;
            }
        }
        i += 1;
    }
}

/// Check if two titles are similar enough to merge.
fn are_similar_titles(a: &str, b: &str) -> bool {
    let a_norm = normalize_feature_name(a);
    let b_norm = normalize_feature_name(b);

    // Exact match after normalization
    if a_norm == b_norm {
        return true;
    }

    // One contains the other
    if a_norm.contains(&b_norm) || b_norm.contains(&a_norm) {
        return true;
    }

    false
}

/// Count feature stats recursively.
fn count_feature_stats(stats: &mut TreeStats, feature: &ExtractedFeature) {
    stats.total_features += 1;
    match feature.state {
        FeatureState::Implemented => stats.implemented_count += 1,
        FeatureState::Proposed => stats.proposed_count += 1,
        FeatureState::Deprecated => stats.deprecated_count += 1,
    }
    for child in &feature.children {
        count_feature_stats(stats, child);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_module_to_chapter_title() {
        assert_eq!(module_to_chapter_title("api"), "HTTP API");
        assert_eq!(module_to_chapter_title("auth"), "Authentication");
        assert_eq!(module_to_chapter_title("custom_module"), "Custom Module");
    }

    #[test]
    fn test_normalize_feature_name() {
        assert_eq!(normalize_feature_name("User Auth"), "user auth");
        assert_eq!(normalize_feature_name("user-auth"), "userauth");
        assert_eq!(
            normalize_feature_name("  Multiple   Spaces  "),
            "multiple spaces"
        );
    }

    #[test]
    fn test_titlecase() {
        assert_eq!(titlecase("user_auth"), "User Auth");
        assert_eq!(titlecase("api-handler"), "Api Handler");
        assert_eq!(titlecase("simple"), "Simple");
    }

    #[test]
    fn test_are_similar_titles() {
        assert!(are_similar_titles("User Auth", "user auth"));
        assert!(are_similar_titles("Add User Auth", "User Auth"));
        assert!(!are_similar_titles("User Auth", "API Handler"));
    }
}

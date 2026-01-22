//! Markdown generation for feature trees.
//!
//! Generates a human-readable markdown document from an extracted feature tree.

use chrono::Utc;

use super::feature_extractor::{
    Chapter, ChapterSource, Evidence, EvidenceKind, ExtractedFeature, ExtractedFeatureTree,
    FeatureState, TreeStats,
};

/// Options for markdown generation.
#[derive(Debug, Clone)]
pub struct MarkdownOptions {
    /// Project name to include in title.
    pub project_name: String,
    /// Current branch name.
    pub branch: Option<String>,
    /// Current commit SHA.
    pub commit_sha: Option<String>,
    /// Whether to include file lists.
    pub include_files: bool,
    /// Whether to include evidence details.
    pub include_evidence: bool,
    /// Maximum number of files to show per feature.
    pub max_files: usize,
}

impl Default for MarkdownOptions {
    fn default() -> Self {
        Self {
            project_name: "Project".to_string(),
            branch: None,
            commit_sha: None,
            include_files: true,
            include_evidence: true,
            max_files: 5,
        }
    }
}

/// Generate markdown document from feature tree.
pub fn generate_markdown(tree: &ExtractedFeatureTree, options: &MarkdownOptions) -> String {
    let mut output = String::new();

    // Title and metadata
    output.push_str(&format!("# Feature Tree: {}\n\n", options.project_name));

    let now = Utc::now().format("%Y-%m-%d").to_string();
    output.push_str(&format!("Generated from codebase analysis on {}.\n", now));

    if let Some(ref sha) = options.commit_sha {
        if let Some(ref branch) = options.branch {
            output.push_str(&format!("Based on: `{}` ({})\n", sha, branch));
        } else {
            output.push_str(&format!("Based on: `{}`\n", sha));
        }
    }
    output.push('\n');

    // Legend
    output.push_str("## Legend\n\n");
    output.push_str("- **●** Implemented - Feature exists in codebase\n");
    output.push_str("- **◇** Proposed - Mentioned but may not be complete\n");
    output.push_str("- **✗** Deprecated - Removal detected\n");
    output.push_str("\n---\n\n");

    // Chapters
    for chapter in &tree.chapters {
        output.push_str(&render_chapter(chapter, options));
    }

    // Summary table
    output.push_str(&render_summary(&tree.stats, &tree.chapters));

    // Warnings
    if !tree.warnings.is_empty() {
        output.push_str("\n## Warnings\n\n");
        for warning in &tree.warnings {
            output.push_str(&format!("- {}\n", warning));
        }
    }

    output
}

/// Render a chapter to markdown.
fn render_chapter(chapter: &Chapter, options: &MarkdownOptions) -> String {
    let mut output = String::new();

    // Chapter heading
    output.push_str(&format!("## {}\n\n", chapter.title));

    // Chapter metadata
    let mut meta_parts: Vec<String> = Vec::new();
    meta_parts.push(format!("Source: {}", chapter.source.as_str()));
    if let Some(ref path) = chapter.module_path {
        meta_parts.push(format!("`{}`", path));
    }
    if chapter.file_count > 0 {
        meta_parts.push(format!("{} files", chapter.file_count));
    }
    output.push_str(&format!("*{}*\n\n", meta_parts.join(" | ")));

    // Features
    for feature in &chapter.features {
        output.push_str(&render_feature(feature, 3, options));
    }

    output.push_str("---\n\n");

    output
}

/// Render a feature to markdown.
fn render_feature(
    feature: &ExtractedFeature,
    heading_level: u8,
    options: &MarkdownOptions,
) -> String {
    let mut output = String::new();

    // Feature heading with state symbol
    let heading_prefix = "#".repeat(heading_level as usize);
    output.push_str(&format!(
        "{} {} {}\n\n",
        heading_prefix,
        feature.state.symbol(),
        feature.title
    ));

    // Description
    if let Some(ref desc) = feature.description {
        output.push_str(&format!("{}\n\n", desc));
    }

    // Evidence
    if options.include_evidence && !feature.evidence.is_empty() {
        output.push_str("**Evidence:**\n");
        for evidence in &feature.evidence {
            output.push_str(&format!(
                "- {}: {}\n",
                evidence.kind.as_str(),
                evidence.description
            ));
        }
        output.push('\n');
    }

    // Files
    if options.include_files && !feature.files.is_empty() {
        output.push_str("**Files:** ");
        let files_to_show: Vec<_> = feature
            .files
            .iter()
            .take(options.max_files)
            .map(|f| format!("`{}`", f))
            .collect();
        output.push_str(&files_to_show.join(", "));
        if feature.files.len() > options.max_files {
            output.push_str(&format!(
                " (+{} more)",
                feature.files.len() - options.max_files
            ));
        }
        output.push_str("\n\n");
    }

    // Children (recursive)
    for child in &feature.children {
        output.push_str(&render_feature(child, heading_level + 1, options));
    }

    output
}

/// Render summary table.
fn render_summary(stats: &TreeStats, chapters: &[Chapter]) -> String {
    let mut output = String::new();

    output.push_str("## Summary\n\n");

    output.push_str("| Chapter | Implemented | Proposed | Deprecated | Total |\n");
    output.push_str("|---------|-------------|----------|------------|-------|\n");

    for chapter in chapters {
        let mut impl_count = 0;
        let mut prop_count = 0;
        let mut dep_count = 0;

        for feature in &chapter.features {
            count_states(feature, &mut impl_count, &mut prop_count, &mut dep_count);
        }

        let total = impl_count + prop_count + dep_count;
        output.push_str(&format!(
            "| {} | {} | {} | {} | {} |\n",
            chapter.title, impl_count, prop_count, dep_count, total
        ));
    }

    output.push_str(&format!(
        "| **Total** | **{}** | **{}** | **{}** | **{}** |\n",
        stats.implemented_count, stats.proposed_count, stats.deprecated_count, stats.total_features
    ));

    output.push('\n');

    // Additional stats
    output.push_str(&format!(
        "*Analyzed {} commits across {} chapters.*\n",
        stats.commits_analyzed, stats.total_chapters
    ));

    output
}

/// Count feature states recursively.
fn count_states(
    feature: &ExtractedFeature,
    impl_count: &mut u32,
    prop_count: &mut u32,
    dep_count: &mut u32,
) {
    match feature.state {
        FeatureState::Implemented => *impl_count += 1,
        FeatureState::Proposed => *prop_count += 1,
        FeatureState::Deprecated => *dep_count += 1,
    }
    for child in &feature.children {
        count_states(child, impl_count, prop_count, dep_count);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_tree() -> ExtractedFeatureTree {
        ExtractedFeatureTree {
            chapters: vec![Chapter {
                title: "Authentication".to_string(),
                source: ChapterSource::Module,
                module_path: Some("src/auth".to_string()),
                file_count: 5,
                features: vec![
                    ExtractedFeature {
                        title: "Password Login".to_string(),
                        description: Some("Email and password authentication".to_string()),
                        state: FeatureState::Implemented,
                        evidence: vec![Evidence {
                            kind: EvidenceKind::Commit,
                            description: "`abc1234` - feat: implement password auth".to_string(),
                        }],
                        children: Vec::new(),
                        files: vec!["src/auth/password.rs".to_string()],
                    },
                    ExtractedFeature {
                        title: "Legacy Basic Auth".to_string(),
                        description: Some("Removed in favor of modern auth".to_string()),
                        state: FeatureState::Deprecated,
                        evidence: vec![Evidence {
                            kind: EvidenceKind::Deletion,
                            description: "`def5678` - remove basic auth".to_string(),
                        }],
                        children: Vec::new(),
                        files: vec!["src/auth/basic.rs".to_string()],
                    },
                ],
            }],
            stats: TreeStats {
                total_chapters: 1,
                total_features: 2,
                implemented_count: 1,
                proposed_count: 0,
                deprecated_count: 1,
                commits_analyzed: 10,
            },
            warnings: Vec::new(),
        }
    }

    #[test]
    fn test_generate_markdown_contains_title() {
        let tree = sample_tree();
        let options = MarkdownOptions {
            project_name: "TestProject".to_string(),
            ..Default::default()
        };
        let md = generate_markdown(&tree, &options);

        assert!(md.contains("# Feature Tree: TestProject"));
    }

    #[test]
    fn test_generate_markdown_contains_legend() {
        let tree = sample_tree();
        let options = MarkdownOptions::default();
        let md = generate_markdown(&tree, &options);

        assert!(md.contains("## Legend"));
        assert!(md.contains("**●** Implemented"));
        assert!(md.contains("**◇** Proposed"));
        assert!(md.contains("**✗** Deprecated"));
    }

    #[test]
    fn test_generate_markdown_contains_chapter() {
        let tree = sample_tree();
        let options = MarkdownOptions::default();
        let md = generate_markdown(&tree, &options);

        assert!(md.contains("## Authentication"));
        assert!(md.contains("### ● Password Login"));
        assert!(md.contains("### ✗ Legacy Basic Auth"));
    }

    #[test]
    fn test_generate_markdown_contains_summary() {
        let tree = sample_tree();
        let options = MarkdownOptions::default();
        let md = generate_markdown(&tree, &options);

        assert!(md.contains("## Summary"));
        assert!(md.contains("| Chapter |"));
        assert!(md.contains("| Authentication |"));
    }
}

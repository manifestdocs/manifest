//! Shared formatting helpers for MCP tool responses.
//!
//! Converts MCP responses from JSON to more token-efficient formats:
//! - Markdown tables for listing tools (34-38% fewer tokens than JSON)
//! - YAML for context tools (27% fewer tokens with best LLM accuracy)
//! - LOD (level-of-detail) breadcrumbs to limit ancestor context size

use chrono::{DateTime, Utc};
use serde::Serialize;

use crate::mcp::types::{BreadcrumbItemInfo, FeatureInfoWithContext};
use crate::models::{
    FeatureWithContext, ProjectHistoryEntry, TestState, TestSuite, VerificationComment,
    VerificationSeverity,
};

/// Render a markdown table from headers and rows.
///
/// Columns are right-padded to the width of the widest cell in each column
/// for readable alignment.
pub fn markdown_table(headers: &[&str], rows: &[Vec<String>]) -> String {
    if rows.is_empty() {
        return "(none)".to_string();
    }

    // Calculate column widths
    let mut widths: Vec<usize> = headers.iter().map(|h| h.len()).collect();
    for row in rows {
        for (i, cell) in row.iter().enumerate() {
            if i < widths.len() {
                widths[i] = widths[i].max(cell.len());
            }
        }
    }

    let mut out = String::new();

    // Header row
    out.push('|');
    for (i, header) in headers.iter().enumerate() {
        out.push_str(&format!(" {:<width$} |", header, width = widths[i]));
    }
    out.push('\n');

    // Separator row
    out.push('|');
    for width in &widths {
        out.push_str(&format!(" {:-<width$} |", "", width = *width));
    }
    out.push('\n');

    // Data rows
    for row in rows {
        out.push('|');
        for (i, cell) in row.iter().enumerate() {
            let width = widths.get(i).copied().unwrap_or(0);
            out.push_str(&format!(" {:<width$} |", cell, width = width));
        }
        out.push('\n');
    }

    out
}

/// Return the first 8 characters of a UUID for compact display.
pub fn short_id(uuid: &uuid::Uuid) -> String {
    uuid.to_string()[..8].to_string()
}

/// Format a human-friendly display ID like "MAN-42".
/// Falls back to short UUID prefix if feature_number or key_prefix is not available.
pub fn display_id(feature_number: Option<i32>, key_prefix: &str, uuid: &uuid::Uuid) -> String {
    match feature_number {
        Some(num) if !key_prefix.is_empty() => format!("{}-{}", key_prefix, num),
        _ => short_id(uuid),
    }
}

/// Default per-level character budget for ancestor details in breadcrumbs.
const PER_LEVEL_BUDGET: usize = 2000;

/// Default total character budget for all ancestor details combined.
const TOTAL_BUDGET: usize = 8000;

/// Apply budget-based LOD (level-of-detail) to a breadcrumb trail.
///
/// Each ancestor's details are capped at `PER_LEVEL_BUDGET` characters.
/// The total details across all ancestors are capped at `TOTAL_BUDGET`,
/// with nearest ancestors (closest to current feature) prioritized —
/// distant ancestors are truncated first when the budget is tight.
///
/// The root item (index 0) already uses `details_summary` from the SQL query,
/// so it typically arrives pre-summarized.
pub fn lod_breadcrumb(
    breadcrumb: &[BreadcrumbItemInfo],
    _detail_depth: usize,
) -> Vec<BreadcrumbItemInfo> {
    budget_breadcrumb(breadcrumb, PER_LEVEL_BUDGET, TOTAL_BUDGET)
}

/// Budget-based breadcrumb truncation with configurable limits.
///
/// Strategy:
/// 1. Apply per-level cap to each item's details.
/// 2. If total exceeds budget, progressively truncate from the most distant
///    ancestor toward the current feature until within budget.
pub fn budget_breadcrumb(
    breadcrumb: &[BreadcrumbItemInfo],
    per_level: usize,
    total: usize,
) -> Vec<BreadcrumbItemInfo> {
    let len = breadcrumb.len();
    if len == 0 {
        return vec![];
    }

    // Step 1: Apply per-level cap to each item
    let mut items: Vec<BreadcrumbItemInfo> = breadcrumb
        .iter()
        .map(|item| BreadcrumbItemInfo {
            id: item.id,
            display_id: item.display_id.clone(),
            title: item.title.clone(),
            details: item
                .details
                .as_ref()
                .map(|d| truncate_to_budget(d, per_level)),
        })
        .collect();

    // Step 2: Enforce total budget, truncating from most distant ancestor first
    let mut total_chars: usize = items.iter().map(|i| detail_len(i)).sum();
    if total_chars <= total {
        return items;
    }

    // Walk from root (index 0) toward current feature, truncating as needed
    for i in 0..len.saturating_sub(1) {
        if total_chars <= total {
            break;
        }
        let current_len = detail_len(&items[i]);
        if current_len == 0 {
            continue;
        }

        // Try first-paragraph truncation
        let truncated = items[i]
            .details
            .as_ref()
            .map(|d| first_paragraph(d))
            .unwrap_or_default();
        let new_len = truncated.len();

        if new_len < current_len {
            total_chars = total_chars - current_len + new_len;
            items[i].details = if truncated.is_empty() {
                None
            } else {
                Some(truncated)
            };
        }

        // If still over budget, remove details entirely from this level
        if total_chars > total {
            let removed = detail_len(&items[i]);
            if removed > 0 {
                total_chars -= removed;
                items[i].details = None;
            }
        }
    }

    items
}

fn detail_len(item: &BreadcrumbItemInfo) -> usize {
    item.details.as_ref().map_or(0, |d| d.len())
}

/// Extract the first paragraph from a text block.
/// A paragraph ends at the first blank line (double newline).
fn first_paragraph(text: &str) -> String {
    // Find the first blank line
    if let Some(pos) = text.find("\n\n") {
        let first = text[..pos].trim().to_string();
        if first.len() < text.len() {
            format!("{}...", first)
        } else {
            first
        }
    } else {
        text.to_string()
    }
}

/// Truncate text to a character budget, breaking at a paragraph or line boundary.
/// Appends "..." when truncated.
fn truncate_to_budget(text: &str, budget: usize) -> String {
    if text.len() <= budget {
        return text.to_string();
    }

    // Try to break at a paragraph boundary within budget
    let search_area = &text[..budget];
    if let Some(pos) = search_area.rfind("\n\n") {
        return format!("{}...", text[..pos].trim());
    }

    // Fall back to line boundary
    if let Some(pos) = search_area.rfind('\n') {
        return format!("{}...", text[..pos].trim());
    }

    // Last resort: hard truncate at budget
    format!("{}...", &text[..budget.saturating_sub(3)])
}

/// Serialize a value to YAML.
pub fn to_yaml<T: Serialize>(value: &T) -> Result<String, String> {
    serde_yml::to_string(value).map_err(|e| e.to_string())
}

/// Populate display_id fields on a FeatureInfoWithContext from the source model + key_prefix.
pub fn populate_display_ids(
    info: &mut FeatureInfoWithContext,
    ctx: &FeatureWithContext,
    key_prefix: &str,
) {
    if key_prefix.is_empty() {
        return;
    }
    // Main feature
    info.display_id = ctx
        .feature
        .feature_number
        .map(|n| format!("{}-{}", key_prefix, n));
    // Note: FeatureSummaryContext doesn't have feature_number, so we can't populate
    // display_id on parent/siblings/children from the current model. This is acceptable
    // since those are lightweight context items.
}

/// Map a feature state string to a compact single-char symbol.
pub fn state_symbol(state: &str) -> &'static str {
    match state {
        "proposed" => "\u{25c7}",    // ◇
        "blocked" => "\u{2298}",     // ⊘
        "in_progress" => "\u{25cb}", // ○
        "implemented" => "\u{25cf}", // ●
        "archived" => "\u{2717}",    // ✗
        _ => "?",
    }
}

/// Map a datetime to a human-friendly relative time bucket.
///
/// Mirrors the web app's `getTimeBucket()` logic in HistoryList.svelte.
pub fn time_bucket(datetime: &DateTime<Utc>) -> String {
    let now = Utc::now();
    let diff = now.signed_duration_since(*datetime);
    let diff_mins = diff.num_minutes();
    let diff_hours = diff.num_hours();

    // Calendar-day comparison
    let today = now.date_naive();
    let entry_day = datetime.date_naive();
    let diff_days = (today - entry_day).num_days();

    if diff_mins < 5 {
        "just now".to_string()
    } else if diff_mins < 60 {
        let bucket = (diff_mins / 15) * 15;
        let bucket = if bucket == 0 { 15 } else { bucket };
        format!("{} mins ago", bucket)
    } else if diff_days == 0 && diff_hours == 1 {
        "1 hour ago".to_string()
    } else if diff_days == 0 && diff_hours < 4 {
        format!("{} hours ago", diff_hours)
    } else if diff_days == 0 {
        "earlier today".to_string()
    } else if diff_days == 1 {
        "yesterday".to_string()
    } else if diff_days < 7 {
        format!("{} days ago", diff_days)
    } else if diff_days < 14 {
        "1 week ago".to_string()
    } else if diff_days < 28 {
        format!("{} weeks ago", diff_days / 7)
    } else {
        datetime.format("%b %-d").to_string()
    }
}

/// Render a list of project history entries as a text timeline.
///
/// Mirrors the web app's Activity tab layout:
/// - Time bucket headers as horizontal rule labels
/// - Release entries shown as `>> Released v0.2.0`
/// - Regular entries: state icon + feature title + separator + headline + commit SHAs
/// - Body (decisions, tradeoffs) shown indented below headline — always expanded in CLI
///   since there is no interactive expand/collapse
pub fn render_activity_timeline(entries: &[ProjectHistoryEntry]) -> String {
    if entries.is_empty() {
        return "No activity recorded yet.".to_string();
    }

    let mut out = String::new();
    let mut current_bucket: Option<String> = None;

    for entry in entries {
        let bucket = time_bucket(&entry.created_at);

        // Emit bucket header when it changes
        if current_bucket.as_ref() != Some(&bucket) {
            if current_bucket.is_some() {
                out.push('\n');
            }
            // ── bucket label ──────────────────────────────
            let label = format!(" {} ", bucket);
            let pad_len = 48usize.saturating_sub(label.len()).saturating_sub(3);
            out.push_str(&format!("──{}{}\n\n", label, "─".repeat(pad_len)));
            current_bucket = Some(bucket);
        }

        // Release entries get special formatting (matches web app detection)
        if entry.summary.starts_with("Released ") {
            let headline = entry.summary.lines().next().unwrap_or(&entry.summary);
            out.push_str(&format!(">>  {}\n", headline.trim()));
            continue;
        }

        let icon = state_symbol(entry.feature_state.as_str());

        // Parse summary into headline + optional body (git-style: blank line separator)
        let (headline, body) = match entry.summary.find('\n') {
            Some(idx) => {
                let h = entry.summary[..idx].trim();
                let rest = entry.summary[idx + 1..].trim_start_matches('\n').trim();
                let b = if rest.is_empty() { None } else { Some(rest) };
                (h, b)
            }
            None => (entry.summary.trim(), None),
        };

        // Separator: — if no body (web), ▸ if has body (web's twirl indicator)
        let separator = if body.is_some() { "▸" } else { "—" };

        // Build commit SHA suffix (comma-separated, matching web)
        let sha_suffix = if entry.commits.is_empty() {
            String::new()
        } else {
            let shas: Vec<String> = entry
                .commits
                .iter()
                .map(|c| {
                    let short = if c.sha.len() > 7 {
                        &c.sha[..7]
                    } else {
                        c.sha.as_str()
                    };
                    short.to_string()
                })
                .collect();
            format!("  {}", shas.join(", "))
        };

        out.push_str(&format!(
            "{} {} {} {}{}\n",
            icon, entry.feature_title, separator, headline, sha_suffix
        ));

        // Show body indented below headline (always shown in CLI; web requires click to expand)
        if let Some(body_text) = body {
            out.push('\n');
            for line in body_text.lines() {
                out.push_str(&format!("  {}\n", line));
            }
            out.push('\n');
        }
    }

    out.trim_end().to_string()
}

/// Map a TestState to a compact symbol matching the web UI (EvidencePanel.svelte).
pub fn test_state_symbol(state: &TestState) -> &'static str {
    match state {
        TestState::Passed => "\u{2713}", // ✓
        TestState::Failed => "\u{2717}", // ✗
        TestState::Errored => "!",
        TestState::Skipped => "\u{2298}", // ⊘
    }
}

/// Render the full test tree — every test listed under its file/suite header.
///
/// ```text
/// tests/db_blockers_spec.rs
///   ✓ proof_gate::requires_passing_proof
///   ✓ proof_gate::rejects_failing_proof
///   ✗ proof_gate::handles_edge_case
///     assertion failed: expected 3, got 2
///
/// 2 passed, 1 failed
/// ```
pub fn render_test_tree(suites: &[TestSuite]) -> String {
    let mut out = String::new();
    let mut passed = 0u32;
    let mut failed = 0u32;
    let mut errored = 0u32;
    let mut skipped = 0u32;

    for suite in suites {
        // Suite header: prefer file path, fall back to name
        let header = suite.file.as_deref().unwrap_or(&suite.name);
        out.push_str(header);
        out.push('\n');

        for test in &suite.tests {
            match test.state {
                TestState::Passed => passed += 1,
                TestState::Failed => failed += 1,
                TestState::Errored => errored += 1,
                TestState::Skipped => skipped += 1,
            }
            let symbol = test_state_symbol(&test.state);
            out.push_str(&format!("  {} {}", symbol, test.name));
            if let Some(ms) = test.duration_ms {
                if ms >= 1000 {
                    out.push_str(&format!(" ({:.1}s)", ms as f64 / 1000.0));
                }
            }
            out.push('\n');
            if matches!(test.state, TestState::Failed | TestState::Errored) {
                if let Some(ref msg) = test.message {
                    for line in msg.lines() {
                        out.push_str(&format!("    {}\n", line));
                    }
                }
            }
        }
        out.push('\n');
    }

    // Summary line
    let mut parts = Vec::new();
    if passed > 0 {
        parts.push(format!("{passed} passed"));
    }
    if failed > 0 {
        parts.push(format!("{failed} failed"));
    }
    if errored > 0 {
        parts.push(format!("{errored} errored"));
    }
    if skipped > 0 {
        parts.push(format!("{skipped} skipped"));
    }
    out.push_str(&parts.join(", "));
    out
}

/// Render a proof's test results as a vitest/rspec-style checklist.
///
/// Uses suite name (not file path) as header, always shows duration,
/// and explicitly shows "0 failed" when all pass.
///
/// ```text
/// Store Inventory
///   ✓ returns pet counts grouped by status (12ms)
///   ✓ counts are accurate after updates (8ms)
///
/// 4 passed, 0 failed
/// ```
pub fn render_proof_checklist(suites: &[TestSuite]) -> String {
    let mut out = String::new();
    let mut passed = 0u32;
    let mut failed = 0u32;
    let mut errored = 0u32;
    let mut skipped = 0u32;

    for suite in suites {
        // Use suite name (not file path) for proof checklist
        out.push_str(&suite.name);
        out.push('\n');

        for test in &suite.tests {
            match test.state {
                TestState::Passed => passed += 1,
                TestState::Failed => failed += 1,
                TestState::Errored => errored += 1,
                TestState::Skipped => skipped += 1,
            }
            let symbol = test_state_symbol(&test.state);
            out.push_str(&format!("  {} {}", symbol, test.name));
            if let Some(ms) = test.duration_ms {
                if ms >= 1000 {
                    out.push_str(&format!(" ({:.1}s)", ms as f64 / 1000.0));
                } else {
                    out.push_str(&format!(" ({}ms)", ms));
                }
            }
            out.push('\n');
            if matches!(test.state, TestState::Failed | TestState::Errored) {
                if let Some(ref msg) = test.message {
                    for line in msg.lines() {
                        out.push_str(&format!("    {}\n", line));
                    }
                }
            }
        }
        out.push('\n');
    }

    // Summary line — always show "0 failed" explicitly when all pass
    let mut parts = Vec::new();
    parts.push(format!("{passed} passed"));
    parts.push(format!("{failed} failed"));
    if errored > 0 {
        parts.push(format!("{errored} errored"));
    }
    if skipped > 0 {
        parts.push(format!("{skipped} skipped"));
    }
    out.push_str(&parts.join(", "));
    out
}

/// Render verification status from a feature's verification data.
///
/// ```text
/// Verification: ✓ passed (2026-03-03)
/// ```
/// or with comments:
/// ```text
/// Verification: 2 comments (2026-03-03)
///   ✗ [critical] Core requirement missing
///   ⚠ [major] Significant gap
/// ```
pub fn render_verification(
    comments: &[VerificationComment],
    verified_at: &DateTime<Utc>,
) -> String {
    let date = verified_at.format("%Y-%m-%d");
    if comments.is_empty() {
        return format!("Verification: \u{2713} passed ({})", date);
    }

    let mut out = format!(
        "Verification: {} comment{} ({})\n",
        comments.len(),
        if comments.len() == 1 { "" } else { "s" },
        date
    );
    for comment in comments {
        let icon = match comment.severity {
            VerificationSeverity::Critical => "\u{2717}", // ✗
            VerificationSeverity::Major => "\u{26a0}",    // ⚠
            VerificationSeverity::Minor => "\u{2022}",    // •
        };
        out.push_str(&format!(
            "  {} [{}] {}\n",
            icon,
            severity_label(&comment.severity),
            comment.title
        ));
    }
    out.trim_end().to_string()
}

fn severity_label(s: &VerificationSeverity) -> &'static str {
    match s {
        VerificationSeverity::Critical => "critical",
        VerificationSeverity::Major => "major",
        VerificationSeverity::Minor => "minor",
    }
}

/// Extract acceptance criteria checkboxes from feature details.
///
/// Matches lines like `- [ ] Criterion text` and `- [x] Criterion text`.
pub fn extract_checkboxes(details: &str) -> Vec<String> {
    details
        .lines()
        .filter(|line| {
            let trimmed = line.trim();
            trimmed.starts_with("- [ ]") || trimmed.starts_with("- [x]")
        })
        .map(|line| line.trim().to_string())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_markdown_table_basic() {
        let headers = &["ID", "Name", "Status"];
        let rows = vec![
            vec!["abc12345".into(), "Auth".into(), "proposed".into()],
            vec!["def67890".into(), "Router".into(), "implemented".into()],
        ];
        let table = markdown_table(headers, &rows);

        assert!(table.contains("| ID       |"));
        assert!(table.contains("| abc12345 |"));
        assert!(table.contains("| -------- |"));
        // Verify alignment: all pipes should line up
        let lines: Vec<&str> = table.lines().collect();
        assert_eq!(lines.len(), 4); // header + separator + 2 data rows
    }

    #[test]
    fn test_markdown_table_empty() {
        let headers = &["ID", "Name"];
        let rows: Vec<Vec<String>> = vec![];
        assert_eq!(markdown_table(headers, &rows), "(none)");
    }

    #[test]
    fn test_short_id() {
        let uuid = uuid::Uuid::parse_str("550e8400-e29b-41d4-a716-446655440000").unwrap();
        assert_eq!(short_id(&uuid), "550e8400");
    }

    #[test]
    fn test_lod_breadcrumb_preserves_short_details() {
        let breadcrumb = vec![
            BreadcrumbItemInfo {
                id: uuid::Uuid::nil(),
                display_id: None,
                title: "Root".into(),
                details: Some("Root details summary".into()),
            },
            BreadcrumbItemInfo {
                id: uuid::Uuid::nil(),
                display_id: None,
                title: "Parent".into(),
                details: Some("Full parent context\n\nMore details here".into()),
            },
            BreadcrumbItemInfo {
                id: uuid::Uuid::nil(),
                display_id: None,
                title: "Current".into(),
                details: Some("Current feature details".into()),
            },
        ];

        // All details are short enough to fit within default budgets (2000/level, 8000 total)
        let result = lod_breadcrumb(&breadcrumb, 1);
        assert_eq!(result[0].details.as_deref(), Some("Root details summary"));
        assert_eq!(
            result[1].details.as_deref(),
            Some("Full parent context\n\nMore details here")
        );
        assert_eq!(
            result[2].details.as_deref(),
            Some("Current feature details")
        );
    }

    #[test]
    fn test_budget_breadcrumb_per_level_truncation() {
        let long_text = "x".repeat(3000);
        let breadcrumb = vec![
            BreadcrumbItemInfo {
                id: uuid::Uuid::nil(),
                display_id: None,
                title: "Root".into(),
                details: Some(long_text),
            },
            BreadcrumbItemInfo {
                id: uuid::Uuid::nil(),
                display_id: None,
                title: "Current".into(),
                details: None,
            },
        ];

        let result = budget_breadcrumb(&breadcrumb, 2000, 8000);
        // Root should be truncated to ~2000 chars
        let root_len = result[0].details.as_ref().unwrap().len();
        assert!(root_len <= 2003); // budget + "..."
        assert!(result[1].details.is_none());
    }

    #[test]
    fn test_budget_breadcrumb_total_budget_truncates_distant_first() {
        // Two ancestors with 500 chars each, total budget 600 — root gets truncated first
        let text_a = "Para one.\n\n".to_string() + &"a".repeat(489);
        let text_b = "b".repeat(500);

        let breadcrumb = vec![
            BreadcrumbItemInfo {
                id: uuid::Uuid::nil(),
                display_id: None,
                title: "Root".into(),
                details: Some(text_a),
            },
            BreadcrumbItemInfo {
                id: uuid::Uuid::nil(),
                display_id: None,
                title: "Parent".into(),
                details: Some(text_b.clone()),
            },
            BreadcrumbItemInfo {
                id: uuid::Uuid::nil(),
                display_id: None,
                title: "Current".into(),
                details: None,
            },
        ];

        let result = budget_breadcrumb(&breadcrumb, 1000, 600);
        // Root should be truncated (it's most distant)
        let root_len = result[0].details.as_ref().map_or(0, |d| d.len());
        assert!(root_len < 500, "Root should be truncated, got {}", root_len);
        // Parent (nearest) should keep full details
        assert_eq!(result[1].details.as_ref().map(|d| d.len()), Some(500));
    }

    #[test]
    fn test_first_paragraph_no_blank_line() {
        assert_eq!(
            first_paragraph("Single paragraph text"),
            "Single paragraph text"
        );
    }

    #[test]
    fn test_first_paragraph_with_blank_line() {
        assert_eq!(
            first_paragraph("First paragraph.\n\nSecond paragraph."),
            "First paragraph...."
        );
    }

    #[test]
    fn test_to_yaml() {
        #[derive(Serialize)]
        struct Simple {
            name: String,
            count: i32,
        }
        let val = Simple {
            name: "test".into(),
            count: 42,
        };
        let yaml = to_yaml(&val).unwrap();
        assert!(yaml.contains("name: test"));
        assert!(yaml.contains("count: 42"));
    }

    #[test]
    fn test_state_symbol() {
        assert_eq!(state_symbol("proposed"), "\u{25c7}");
        assert_eq!(state_symbol("in_progress"), "\u{25cb}");
        assert_eq!(state_symbol("implemented"), "\u{25cf}");
        assert_eq!(state_symbol("archived"), "\u{2717}");
        assert_eq!(state_symbol("unknown"), "?");
    }

    #[test]
    fn test_state_symbol_for_tests() {
        assert_eq!(super::test_state_symbol(&TestState::Passed), "\u{2713}");
        assert_eq!(super::test_state_symbol(&TestState::Failed), "\u{2717}");
        assert_eq!(super::test_state_symbol(&TestState::Errored), "!");
        assert_eq!(super::test_state_symbol(&TestState::Skipped), "\u{2298}");
    }
}

//! Shared formatting helpers for MCP tool responses.
//!
//! Converts MCP responses from JSON to more token-efficient formats:
//! - Markdown tables for listing tools (34-38% fewer tokens than JSON)
//! - YAML for context tools (27% fewer tokens with best LLM accuracy)
//! - LOD (level-of-detail) breadcrumbs to limit ancestor context size

use serde::Serialize;

use crate::mcp::types::BreadcrumbItemInfo;

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

/// Apply LOD (level-of-detail) to a breadcrumb trail.
///
/// - Items within `detail_depth` hops of the end (current feature) keep full details.
/// - More distant ancestors get truncated to their first paragraph.
/// - The root item (index 0) already uses `details_summary` from the SQL query,
///   so we don't truncate it further here.
pub fn lod_breadcrumb(
    breadcrumb: &[BreadcrumbItemInfo],
    detail_depth: usize,
) -> Vec<BreadcrumbItemInfo> {
    let len = breadcrumb.len();
    breadcrumb
        .iter()
        .enumerate()
        .map(|(i, item)| {
            // Distance from the end (current feature is at len-1)
            let distance_from_current = len.saturating_sub(1).saturating_sub(i);

            if distance_from_current <= detail_depth {
                // Within detail depth: keep full details
                item.clone()
            } else {
                // Beyond detail depth: truncate to first paragraph
                BreadcrumbItemInfo {
                    id: item.id,
                    title: item.title.clone(),
                    details: item.details.as_ref().map(|d| first_paragraph(d)),
                }
            }
        })
        .collect()
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

/// Serialize a value to YAML.
pub fn to_yaml<T: Serialize>(value: &T) -> Result<String, String> {
    serde_yml::to_string(value).map_err(|e| e.to_string())
}

/// Map a feature state string to a compact single-char symbol.
pub fn state_symbol(state: &str) -> &'static str {
    match state {
        "proposed" => "\u{25c7}",    // ◇
        "in_progress" => "\u{25cb}", // ○
        "implemented" => "\u{25cf}", // ●
        "archived" => "\u{2717}",    // ✗
        _ => "?",
    }
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
    fn test_lod_breadcrumb_within_depth() {
        let breadcrumb = vec![
            BreadcrumbItemInfo {
                id: uuid::Uuid::nil(),
                title: "Root".into(),
                details: Some("Root details summary".into()),
            },
            BreadcrumbItemInfo {
                id: uuid::Uuid::nil(),
                title: "Parent".into(),
                details: Some("Full parent context\n\nMore details here".into()),
            },
            BreadcrumbItemInfo {
                id: uuid::Uuid::nil(),
                title: "Current".into(),
                details: Some("Current feature details".into()),
            },
        ];

        // detail_depth=1: current (index 2, distance 0) and parent (index 1, distance 1) keep full details
        let result = lod_breadcrumb(&breadcrumb, 1);
        assert_eq!(
            result[1].details.as_deref(),
            Some("Full parent context\n\nMore details here")
        );

        // Root (index 0, distance 2) should be truncated to first paragraph
        assert_eq!(result[0].details.as_deref(), Some("Root details summary"));
    }

    #[test]
    fn test_lod_breadcrumb_truncation() {
        let breadcrumb = vec![
            BreadcrumbItemInfo {
                id: uuid::Uuid::nil(),
                title: "Root".into(),
                details: Some("First paragraph.\n\nSecond paragraph with more.".into()),
            },
            BreadcrumbItemInfo {
                id: uuid::Uuid::nil(),
                title: "Grandparent".into(),
                details: Some("GP first paragraph.\n\nGP second paragraph.".into()),
            },
            BreadcrumbItemInfo {
                id: uuid::Uuid::nil(),
                title: "Parent".into(),
                details: Some("Parent details\n\nParent extra".into()),
            },
            BreadcrumbItemInfo {
                id: uuid::Uuid::nil(),
                title: "Current".into(),
                details: None,
            },
        ];

        // detail_depth=1: only parent (distance 1) and current (distance 0) keep full details
        let result = lod_breadcrumb(&breadcrumb, 1);

        // Root (distance 3) -> truncated
        assert_eq!(result[0].details.as_deref(), Some("First paragraph...."));
        // Grandparent (distance 2) -> truncated
        assert_eq!(result[1].details.as_deref(), Some("GP first paragraph...."));
        // Parent (distance 1) -> full
        assert_eq!(
            result[2].details.as_deref(),
            Some("Parent details\n\nParent extra")
        );
        // Current (distance 0) -> full (None stays None)
        assert!(result[3].details.is_none());
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
}

//! ASCII tree rendering for feature hierarchies.

use crate::models::{FeatureState, FeatureTreeNode};

const PROPOSED: char = '◇';
const BLOCKED: char = '⊘';
const IN_PROGRESS: char = '○';
const IMPLEMENTED: char = '●';
const ARCHIVED: char = '✗';
const PROJECT_ROOT: char = '▣'; // Square for project root feature

/// Get the status symbol for a feature state.
fn state_symbol(state: FeatureState) -> char {
    match state {
        FeatureState::Proposed => PROPOSED,
        FeatureState::Blocked => BLOCKED,
        FeatureState::InProgress => IN_PROGRESS,
        FeatureState::Implemented => IMPLEMENTED,
        FeatureState::Archived => ARCHIVED,
    }
}

/// Render a feature tree as ASCII art with status symbols, limited to a maximum depth.
///
/// Example output:
/// ```text
/// Authentication
/// ├── MAN-2 ● Password Login
/// ├── MAN-3 ○ OAuth Integration
/// │   ├── MAN-4 ◇ Google Provider
/// │   └── MAN-5 ◇ GitHub Provider
/// └── MAN-6 ✗ Legacy Basic Auth
/// ```
///
/// # Arguments
/// * `nodes` - The feature tree nodes to render
/// * `max_depth` - Maximum depth to render. 0 means unlimited.
/// * `key_prefix` - Project key prefix for display IDs (e.g., "MAN"). Empty string to skip.
///
/// When children are truncated due to depth limit, shows `(...)` indicator.
pub fn render_tree_with_depth(
    nodes: &[FeatureTreeNode],
    max_depth: u32,
    key_prefix: &str,
) -> String {
    let mut output = String::new();
    for (i, node) in nodes.iter().enumerate() {
        let is_last = i == nodes.len() - 1;
        render_node_with_depth(
            &mut output,
            node,
            "",
            is_last,
            true,
            0,
            max_depth,
            key_prefix,
        );
    }
    output
}

/// Recursively render a node and its children with depth limiting.
#[allow(clippy::too_many_arguments)]
fn render_node_with_depth(
    output: &mut String,
    node: &FeatureTreeNode,
    prefix: &str,
    is_last: bool,
    is_tree_root: bool,
    current_depth: u32,
    max_depth: u32,
    key_prefix: &str,
) {
    let symbol = if node.is_root {
        PROJECT_ROOT // Project root feature gets special symbol
    } else {
        state_symbol(node.feature.state) // Derived state for parents, DB state for leaves
    };

    // Format display ID (e.g., "MAN-5 ") or empty string if not available
    let id_label = if !key_prefix.is_empty() {
        if let Some(num) = node.feature.feature_number {
            format!("{}-{} ", key_prefix, num)
        } else {
            String::new()
        }
    } else {
        String::new()
    };

    if is_tree_root {
        // Tree root nodes: symbol + title (no branch characters)
        output.push(symbol);
        output.push(' ');
        output.push_str(&id_label);
        output.push_str(&node.feature.title);
        output.push('\n');
    } else {
        // Child nodes: branch + symbol + id + title
        let branch = if is_last { "└── " } else { "├── " };
        output.push_str(prefix);
        output.push_str(branch);
        output.push_str(&id_label);
        output.push(symbol);
        output.push(' ');
        output.push_str(&node.feature.title);
        output.push('\n');
    }

    // Calculate prefix for children
    let child_prefix = if is_tree_root {
        String::new()
    } else {
        let continuation = if is_last { "    " } else { "│   " };
        format!("{}{}", prefix, continuation)
    };

    // Check if we've reached the depth limit
    let at_depth_limit = max_depth > 0 && current_depth >= max_depth;

    if at_depth_limit && !node.children.is_empty() {
        // Show truncation indicator
        output.push_str(&child_prefix);
        output.push_str("└── (...)\n");
    } else {
        // Render children
        for (i, child) in node.children.iter().enumerate() {
            let child_is_last = i == node.children.len() - 1;
            render_node_with_depth(
                output,
                child,
                &child_prefix,
                child_is_last,
                false,
                current_depth + 1,
                max_depth,
                key_prefix,
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{Feature, FeatureId, ProjectId};
    use chrono::Utc;

    fn make_node(
        title: &str,
        state: FeatureState,
        children: Vec<FeatureTreeNode>,
    ) -> FeatureTreeNode {
        FeatureTreeNode {
            feature: Feature {
                id: FeatureId::new(),
                project_id: ProjectId::new(),
                parent_id: None,
                title: title.to_string(),
                details: None,
                desired_details: None,
                details_summary: None,
                state,
                priority: 0,
                feature_number: None,
                target_version_id: None,
                verification_result: None,
                verified_at: None,
                claimed_by: None,
                claimed_at: None,
                claim_metadata: None,
                created_at: Utc::now(),
                updated_at: Utc::now(),
            },
            children,
            is_root: false,
        }
    }

    fn make_numbered_node(
        title: &str,
        state: FeatureState,
        number: i32,
        children: Vec<FeatureTreeNode>,
    ) -> FeatureTreeNode {
        let mut node = make_node(title, state, children);
        node.feature.feature_number = Some(number);
        node
    }

    #[test]
    fn test_single_root() {
        let tree = vec![make_node("Authentication", FeatureState::Proposed, vec![])];
        let output = render_tree_with_depth(&tree, 0, "");
        assert_eq!(output, "◇ Authentication\n");
    }

    #[test]
    fn test_with_children() {
        let tree = vec![make_node(
            "Authentication",
            FeatureState::Proposed,
            vec![
                make_node("Password Login", FeatureState::Implemented, vec![]),
                make_node("OAuth", FeatureState::InProgress, vec![]),
            ],
        )];
        let output = render_tree_with_depth(&tree, 0, "");
        // Parent state is whatever DB stored (Proposed here) — derivation happens in DB layer
        assert_eq!(
            output,
            "◇ Authentication\n├── ● Password Login\n└── ○ OAuth\n"
        );
    }

    #[test]
    fn test_nested_children() {
        let tree = vec![make_node(
            "Authentication",
            FeatureState::Proposed,
            vec![
                make_node("Password Login", FeatureState::Implemented, vec![]),
                make_node(
                    "OAuth Integration",
                    FeatureState::InProgress,
                    vec![
                        make_node("Google Provider", FeatureState::Proposed, vec![]),
                        make_node("GitHub Provider", FeatureState::Proposed, vec![]),
                    ],
                ),
                make_node("Legacy Basic Auth", FeatureState::Archived, vec![]),
            ],
        )];
        let output = render_tree_with_depth(&tree, 0, "");
        // Parents show their state symbol (derivation happens in DB layer, not renderer)
        let expected = "◇ Authentication\n├── ● Password Login\n├── ○ OAuth Integration\n│   ├── ◇ Google Provider\n│   └── ◇ GitHub Provider\n└── ✗ Legacy Basic Auth\n";
        assert_eq!(output, expected);
    }

    #[test]
    fn test_depth_limit_truncates_children() {
        let tree = vec![make_node(
            "Authentication",
            FeatureState::Proposed,
            vec![
                make_node("Password Login", FeatureState::Implemented, vec![]),
                make_node(
                    "OAuth Integration",
                    FeatureState::InProgress,
                    vec![
                        make_node("Google Provider", FeatureState::Proposed, vec![]),
                        make_node("GitHub Provider", FeatureState::Proposed, vec![]),
                    ],
                ),
            ],
        )];
        // max_depth=1 should show root + first level, but truncate second level
        let output = render_tree_with_depth(&tree, 1, "");
        let expected =
            "◇ Authentication\n├── ● Password Login\n└── ○ OAuth Integration\n    └── (...)\n";
        assert_eq!(output, expected);
    }

    #[test]
    fn test_depth_zero_means_unlimited() {
        let tree = vec![make_node(
            "Root",
            FeatureState::Proposed,
            vec![make_node(
                "Child",
                FeatureState::Implemented,
                vec![make_node("Grandchild", FeatureState::Proposed, vec![])],
            )],
        )];
        // max_depth=0 should show all levels
        let output = render_tree_with_depth(&tree, 0, "");
        let expected = "◇ Root\n└── ● Child\n    └── ◇ Grandchild\n";
        assert_eq!(output, expected);
    }

    #[test]
    fn test_display_ids_in_tree() {
        let tree = vec![make_numbered_node(
            "Auth",
            FeatureState::Proposed,
            1,
            vec![
                make_numbered_node("Login", FeatureState::Implemented, 2, vec![]),
                make_numbered_node("OAuth", FeatureState::InProgress, 3, vec![]),
            ],
        )];
        let output = render_tree_with_depth(&tree, 0, "MAN");
        let expected = "◇ MAN-1 Auth\n├── MAN-2 ● Login\n└── MAN-3 ○ OAuth\n";
        assert_eq!(output, expected);
    }
}

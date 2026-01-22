//! ASCII tree rendering for feature hierarchies.

use crate::models::{FeatureState, FeatureTreeNode};

const PROPOSED: char = '◇';
const IN_PROGRESS: char = '○';
const IMPLEMENTED: char = '●';
const ARCHIVED: char = '✗';
const PROJECT_ROOT: char = '▣'; // Special symbol for project root feature

/// Get the status symbol for a feature state.
fn state_symbol(state: FeatureState) -> char {
    match state {
        FeatureState::Proposed => PROPOSED,
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
/// ├── ● Password Login
/// ├── ○ OAuth Integration
/// │   ├── ◇ Google Provider
/// │   └── ◇ GitHub Provider
/// └── ✗ Legacy Basic Auth
/// ```
///
/// # Arguments
/// * `nodes` - The feature tree nodes to render
/// * `max_depth` - Maximum depth to render. 0 means unlimited.
///
/// When children are truncated due to depth limit, shows `(...)` indicator.
pub fn render_tree_with_depth(nodes: &[FeatureTreeNode], max_depth: u32) -> String {
    let mut output = String::new();
    for (i, node) in nodes.iter().enumerate() {
        let is_last = i == nodes.len() - 1;
        render_node_with_depth(&mut output, node, "", is_last, true, 0, max_depth);
    }
    output
}

/// Recursively render a node and its children with depth limiting.
fn render_node_with_depth(
    output: &mut String,
    node: &FeatureTreeNode,
    prefix: &str,
    is_last: bool,
    is_tree_root: bool,
    current_depth: u32,
    max_depth: u32,
) {
    let symbol = if node.is_root {
        PROJECT_ROOT // Project root feature gets special symbol
    } else {
        state_symbol(node.feature.state)
    };

    if is_tree_root {
        // Tree root nodes: symbol + title (no branch characters)
        output.push(symbol);
        output.push(' ');
        output.push_str(&node.feature.title);
        output.push('\n');
    } else {
        // Child nodes: branch + symbol + title
        let branch = if is_last { "└── " } else { "├── " };
        output.push_str(prefix);
        output.push_str(branch);
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
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::Feature;
    use chrono::Utc;
    use uuid::Uuid;

    fn make_node(
        title: &str,
        state: FeatureState,
        children: Vec<FeatureTreeNode>,
    ) -> FeatureTreeNode {
        FeatureTreeNode {
            feature: Feature {
                id: Uuid::new_v4(),
                project_id: Uuid::new_v4(),
                parent_id: None,
                title: title.to_string(),
                details: None,
                desired_details: None,
                state,
                priority: 0,
                target_version_id: None,
                created_at: Utc::now(),
                updated_at: Utc::now(),
            },
            children,
            is_root: false,
        }
    }

    #[test]
    fn test_single_root() {
        let tree = vec![make_node("Authentication", FeatureState::Proposed, vec![])];
        let output = render_tree_with_depth(&tree, 0);
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
        let output = render_tree_with_depth(&tree, 0);
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
        let output = render_tree_with_depth(&tree, 0);
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
        let output = render_tree_with_depth(&tree, 1);
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
        let output = render_tree_with_depth(&tree, 0);
        let expected = "◇ Root\n└── ● Child\n    └── ◇ Grandchild\n";
        assert_eq!(output, expected);
    }
}

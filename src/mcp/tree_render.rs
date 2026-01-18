//! ASCII tree rendering for feature hierarchies.

use crate::models::{FeatureState, FeatureTreeNode};

const PROPOSED: char = '◇';
const SPECIFIED: char = '○';
const IMPLEMENTED: char = '●';
const DEPRECATED: char = '✗';

/// Get the status symbol for a feature state.
fn state_symbol(state: FeatureState) -> char {
    match state {
        FeatureState::Proposed => PROPOSED,
        FeatureState::Specified => SPECIFIED,
        FeatureState::Implemented => IMPLEMENTED,
        FeatureState::Deprecated => DEPRECATED,
    }
}

/// Render a feature tree as ASCII art with status symbols.
///
/// Example output:
/// ```text
/// Authentication
/// ├── ● Password Login
/// ├── ○ OAuth Integration
/// │   ├── • Google Provider
/// │   └── • GitHub Provider
/// └── ✗ Legacy Basic Auth
/// ```
pub fn render_tree(nodes: &[FeatureTreeNode]) -> String {
    let mut output = String::new();
    for (i, node) in nodes.iter().enumerate() {
        let is_last = i == nodes.len() - 1;
        render_node(&mut output, node, "", is_last, true);
    }
    output
}

/// Recursively render a node and its children.
fn render_node(
    output: &mut String,
    node: &FeatureTreeNode,
    prefix: &str,
    is_last: bool,
    is_root: bool,
) {
    let symbol = state_symbol(node.feature.state);

    if is_root {
        // Root nodes: just title (no branch characters)
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
    let child_prefix = if is_root {
        String::new()
    } else {
        let continuation = if is_last { "    " } else { "│   " };
        format!("{}{}", prefix, continuation)
    };

    // Render children
    for (i, child) in node.children.iter().enumerate() {
        let child_is_last = i == node.children.len() - 1;
        render_node(output, child, &child_prefix, child_is_last, false);
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
        }
    }

    #[test]
    fn test_single_root() {
        let tree = vec![make_node("Authentication", FeatureState::Proposed, vec![])];
        let output = render_tree(&tree);
        assert_eq!(output, "Authentication\n");
    }

    #[test]
    fn test_with_children() {
        let tree = vec![make_node(
            "Authentication",
            FeatureState::Proposed,
            vec![
                make_node("Password Login", FeatureState::Implemented, vec![]),
                make_node("OAuth", FeatureState::Specified, vec![]),
            ],
        )];
        let output = render_tree(&tree);
        assert_eq!(
            output,
            "Authentication\n├── ● Password Login\n└── ○ OAuth\n"
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
                    FeatureState::Specified,
                    vec![
                        make_node("Google Provider", FeatureState::Proposed, vec![]),
                        make_node("GitHub Provider", FeatureState::Proposed, vec![]),
                    ],
                ),
                make_node("Legacy Basic Auth", FeatureState::Deprecated, vec![]),
            ],
        )];
        let output = render_tree(&tree);
        let expected = "Authentication\n├── ● Password Login\n├── ○ OAuth Integration\n│   ├── ◇ Google Provider\n│   └── ◇ GitHub Provider\n└── ✗ Legacy Basic Auth\n";
        assert_eq!(output, expected);
    }
}

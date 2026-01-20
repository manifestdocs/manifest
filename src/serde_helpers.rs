//! Shared serde helper functions for default values.

/// Default function for serde that returns `true`.
/// Use with `#[serde(default = "crate::serde_helpers::default_true")]`
pub fn default_true() -> bool {
    true
}

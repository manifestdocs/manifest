//! GitHub backend for Manifest.
//!
//! Implements [`FeatureStore`](crate::store::FeatureStore) using GitHub Issues, Labels,
//! Milestones, and Sub-Issues as the data layer. Local SQLite acts as a read cache
//! via [`CachedStore`](crate::store::CachedStore).
//!
//! # Modules
//!
//! - [`parser`] — Parses GitHub issue bodies and extracts Manifest metadata
//! - [`labels`] — Maps between Manifest feature states and GitHub labels
//! - [`client`] — GraphQL client for fetching issues, milestones, sub-issues
//! - [`store`] — [`FeatureStore`] implementation

pub mod client;
pub mod labels;
pub mod parser;
pub mod store;

pub use client::GitHubClient;
pub use labels::{label_to_state, state_to_label, MANIFEST_LABELS};
pub use parser::{parse_issue_body, IssueMetadata};
pub use store::GitHubStore;

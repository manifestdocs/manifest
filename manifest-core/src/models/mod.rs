//! Domain models for Manifest.
//!
//! # Core Concepts
//!
//! ## Permanent Entities
//!
//! - [`Feature`]: Living documentation of system capabilities, forming a hierarchical tree.
//!   Any node can have content, but only leaf nodes can have sessions.
//! - [`FeatureHistory`]: Append-only log of work done on features (like `git log` for a feature).
//! - [`Project`]: Top-level container with associated directories and features.
//!
//! ## Ephemeral Entities
//!
//! These exist only during active work and are deleted when sessions complete:
//!
//! - [`Session`]: Active work session on a leaf feature (one at a time per feature).
//! - [`Task`]: Work unit within a session, assigned to an AI agent.

mod feature;
mod history;
mod project;
mod session;
mod task;
mod user;
mod version;

/// Error returned when parsing a string into a domain enum fails.
#[derive(Debug, Clone, thiserror::Error)]
#[error("invalid value: '{0}'")]
pub struct ParseEnumError(pub String);

pub use feature::*;
pub use history::*;
pub use project::*;
pub use session::*;
pub use task::*;
pub use user::*;
pub use version::*;

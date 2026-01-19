//! Core library for Manifest.
//!
//! This crate provides the domain models and database operations for Manifest,
//! independent of any transport layer (HTTP, MCP, etc.).
//!
//! # Usage
//!
//! ```no_run
//! use manifest_core::db::Database;
//! use manifest_core::models::*;
//!
//! let db = Database::open_default()?;
//! db.migrate()?;
//!
//! let features = db.get_all_features()?;
//! # Ok::<(), anyhow::Error>(())
//! ```

pub mod db;
pub mod models;

// Re-export commonly used types at crate root
pub use db::{Database, FeatureEvent};

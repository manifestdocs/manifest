//! Core library for Manifest.
//!
//! This crate provides the domain models and database operations for Manifest,
//! independent of any transport layer (HTTP, MCP, etc.).
//!
//! # Usage
//!
//! ```ignore
//! use manifest_core::db::Database;
//! use manifest_core::models::*;
//!
//! let db = Database::open_default().await?;
//! db.migrate()?;
//!
//! let features = db.get_all_features()?;
//! # Ok::<(), anyhow::Error>(())
//! ```

pub mod config;
pub mod db;
pub mod models;
pub mod store;

// Re-export commonly used types at crate root
pub use config::ServerConfig;
pub use db::{Database, FeatureEvent, RootFeatureMigrationReport};

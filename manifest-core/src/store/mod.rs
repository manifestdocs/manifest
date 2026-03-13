//! Backend-agnostic storage layer for Manifest.
//!
//! This module defines the [`FeatureStore`] trait that abstracts all feature data operations.
//! The existing SQLite code refactors into the first implementation (`SqliteStore`). Future
//! backends (Turso, GitHub) implement the same trait. Application code receives a
//! `Arc<dyn FeatureStore>` and never knows which backend is active.
//!
//! # Architecture
//!
//! - **Shared data** (projects, features, versions, history, blockers) flows through `FeatureStore`
//! - **Local-only data** (sessions, tasks, proofs, directories) stays as direct SQLite access
//! - **`FeatureChangeset`** carries write intent with field-level granularity
//! - **`FeatureQuery`** provides backend-agnostic filtering
//! - **`StoreCapabilities`** lets application code adapt to backend differences

mod error;
mod query;
pub mod sqlite;
mod traits;
mod types;

pub use error::*;
pub use query::*;
pub use sqlite::*;
pub use traits::*;
pub use types::*;

pub mod adapters;
pub mod analysis;
pub mod api;
#[cfg(feature = "embed-web")]
pub mod assets;
pub mod mcp;
pub mod serde_helpers;

// Re-export from manifest-core for convenience
pub use manifest_core::db;
pub use manifest_core::models;

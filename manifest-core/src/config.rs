//! Server configuration file support.
//!
//! Reads and writes `config.json` in the platform data directory
//! (e.g. `~/.local/share/manifest/config.json` on Linux).

use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Persisted server configuration.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ServerConfig {
    /// Override path for the SQLite database file.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub database_path: Option<String>,
}

impl ServerConfig {
    /// Load config from the platform config file. Returns defaults if the file doesn't exist.
    pub fn load() -> Result<Self> {
        let path = Self::config_path()?;
        if !path.exists() {
            return Ok(Self::default());
        }
        let contents = std::fs::read_to_string(&path)?;
        let config: ServerConfig = serde_json::from_str(&contents)?;
        Ok(config)
    }

    /// Save config to the platform config file.
    pub fn save(&self) -> Result<()> {
        let path = Self::config_path()?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let contents = serde_json::to_string_pretty(self)?;
        std::fs::write(&path, contents)?;
        Ok(())
    }

    /// Returns the path to the config file.
    pub fn config_path() -> Result<PathBuf> {
        let dirs = directories::ProjectDirs::from("", "", "manifest")
            .ok_or_else(|| anyhow::anyhow!("Could not determine data directory"))?;
        Ok(dirs.data_dir().join("config.json"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config_has_no_database_path() {
        let config = ServerConfig::default();
        assert!(config.database_path.is_none());
    }

    #[test]
    fn roundtrip_serialization() {
        let config = ServerConfig {
            database_path: Some("/tmp/test.db".into()),
        };
        let json = serde_json::to_string(&config).unwrap();
        let parsed: ServerConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.database_path, Some("/tmp/test.db".into()));
    }

    #[test]
    fn empty_json_deserializes_to_defaults() {
        let config: ServerConfig = serde_json::from_str("{}").unwrap();
        assert!(config.database_path.is_none());
    }
}

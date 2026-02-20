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

    /// CLI agent to use for chat (claude, gemini, copilot). Defaults to "claude".
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default_agent: Option<String>,

    /// Whether the web terminal is enabled. Defaults to false (disabled).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub terminal_enabled: Option<bool>,
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

    /// Returns whether the terminal is enabled (defaults to false).
    pub fn is_terminal_enabled(&self) -> bool {
        self.terminal_enabled.unwrap_or(false)
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
        assert!(config.default_agent.is_none());
        assert!(config.terminal_enabled.is_none());
    }

    #[test]
    fn terminal_disabled_by_default() {
        let config = ServerConfig::default();
        assert!(!config.is_terminal_enabled());
    }

    #[test]
    fn terminal_enabled_when_set() {
        let config = ServerConfig {
            terminal_enabled: Some(true),
            ..Default::default()
        };
        assert!(config.is_terminal_enabled());
    }

    #[test]
    fn roundtrip_serialization() {
        let config = ServerConfig {
            database_path: Some("/tmp/test.db".into()),
            default_agent: Some("claude".into()),
            terminal_enabled: None,
        };
        let json = serde_json::to_string(&config).unwrap();
        let parsed: ServerConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.database_path, Some("/tmp/test.db".into()));
        assert_eq!(parsed.default_agent, Some("claude".into()));
    }

    #[test]
    fn roundtrip_with_default_agent() {
        let config = ServerConfig {
            database_path: None,
            default_agent: Some("gemini".into()),
            terminal_enabled: None,
        };
        let json = serde_json::to_string(&config).unwrap();
        // database_path should be omitted, default_agent present
        assert!(!json.contains("database_path"));
        assert!(json.contains("gemini"));
        let parsed: ServerConfig = serde_json::from_str(&json).unwrap();
        assert!(parsed.database_path.is_none());
        assert_eq!(parsed.default_agent, Some("gemini".into()));
    }

    #[test]
    fn empty_json_deserializes_to_defaults() {
        let config: ServerConfig = serde_json::from_str("{}").unwrap();
        assert!(config.database_path.is_none());
        assert!(config.default_agent.is_none());
    }
}

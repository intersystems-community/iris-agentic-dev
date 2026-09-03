use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// A single IRIS server entry in the iad-native config file.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ServerEntry {
    pub host: String,
    pub port: u16,
    pub namespace: String,
    pub username: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scheme: Option<String>,
    /// Plaintext credential stored as a fallback when the OS keychain is unavailable
    /// (e.g. headless MCP contexts, Remote SSH). Keychain takes priority when present.
    /// Never returned in any tool response — only read for connection auth.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub password: Option<String>,
}

/// The iad-native servers config file (`~/.config/iris-agentic-dev/servers.json`).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ServersConfig {
    pub version: u32,
    pub servers: HashMap<String, ServerEntry>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default: Option<String>,
}

impl Default for ServersConfig {
    fn default() -> Self {
        Self {
            version: 1,
            servers: HashMap::new(),
            default: None,
        }
    }
}

/// Returns the platform-correct path for the iad-native servers config file.
///
/// - macOS/Linux: `~/.config/iris-agentic-dev/servers.json`
/// - Windows: `%APPDATA%\iris-agentic-dev\servers.json`
pub fn native_config_path() -> PathBuf {
    #[cfg(target_os = "windows")]
    {
        let appdata = std::env::var("APPDATA").unwrap_or_else(|_| ".".to_string());
        PathBuf::from(appdata)
            .join("iris-agentic-dev")
            .join("servers.json")
    }
    #[cfg(not(target_os = "windows"))]
    {
        // dirs::config_dir() returns ~/.config on Linux and ~/Library/Application Support on macOS.
        // We want ~/.config on both for consistency with the spec.
        let base = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
        base.join(".config")
            .join("iris-agentic-dev")
            .join("servers.json")
    }
}

/// Loads the iad-native config from the given path.
///
/// Returns `ServersConfig::default()` if the file does not exist.
/// Logs a warning and returns default if the file is present but malformed.
pub fn load_from_path(path: &Path) -> ServersConfig {
    match std::fs::read_to_string(path) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => ServersConfig::default(),
        Err(e) => {
            tracing::warn!("Failed to read servers config at {}: {e}", path.display());
            ServersConfig::default()
        }
        Ok(contents) => match serde_json::from_str(&contents) {
            Ok(cfg) => cfg,
            Err(e) => {
                tracing::warn!("Failed to parse servers config at {}: {e}", path.display());
                ServersConfig::default()
            }
        },
    }
}

/// Loads the iad-native config from the default platform path.
///
/// Returns `ServersConfig::default()` if the file does not exist or cannot be parsed.
pub fn load_native_config() -> ServersConfig {
    load_from_path(&native_config_path())
}

/// Saves the config to the given path using an atomic write (temp file + rename).
///
/// Creates parent directories if they do not exist.
pub fn save_to_path(cfg: &ServersConfig, path: &Path) -> Result<(), Box<dyn std::error::Error>> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let tmp = path.with_extension("json.tmp");
    let contents = serde_json::to_string_pretty(cfg)?;
    std::fs::write(&tmp, contents)?;
    std::fs::rename(&tmp, path)?;
    Ok(())
}

/// Saves the config to the default platform path using an atomic write.
pub fn save_native_config(cfg: &ServersConfig) -> Result<(), Box<dyn std::error::Error>> {
    save_to_path(cfg, &native_config_path())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn make_two_server_config() -> ServersConfig {
        let mut servers = HashMap::new();
        servers.insert(
            "dev".to_string(),
            ServerEntry {
                host: "localhost".to_string(),
                port: 52780,
                namespace: "USER".to_string(),
                username: "_SYSTEM".to_string(),
                description: Some("Dev container".to_string()),
                scheme: Some("http".to_string()),
                password: None,
            },
        );
        servers.insert(
            "prod".to_string(),
            ServerEntry {
                host: "prod.example.com".to_string(),
                port: 443,
                namespace: "PROD".to_string(),
                username: "admin".to_string(),
                description: None,
                scheme: Some("https".to_string()),
                password: None,
            },
        );
        ServersConfig {
            version: 1,
            servers,
            default: Some("dev".to_string()),
        }
    }

    #[test]
    fn load_native_config_missing_file_returns_default() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("does-not-exist.json");
        let cfg = load_from_path(&path);
        assert!(cfg.servers.is_empty());
        assert_eq!(cfg.version, 1);
        assert!(cfg.default.is_none());
    }

    #[test]
    fn save_and_load_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("servers.json");

        let original = make_two_server_config();
        save_to_path(&original, &path).expect("save should succeed");

        assert!(path.exists(), "file should exist after save");

        let loaded = load_from_path(&path);
        assert_eq!(loaded, original);
        assert_eq!(loaded.servers.len(), 2);

        let dev = loaded.servers.get("dev").unwrap();
        assert_eq!(dev.host, "localhost");
        assert_eq!(dev.port, 52780);
        assert_eq!(dev.namespace, "USER");
        assert_eq!(dev.description.as_deref(), Some("Dev container"));

        let prod = loaded.servers.get("prod").unwrap();
        assert_eq!(prod.host, "prod.example.com");
        assert_eq!(prod.port, 443);
        assert!(prod.description.is_none());
    }

    #[test]
    fn native_config_path_ends_correctly() {
        let path = native_config_path();
        let path_str = path.to_string_lossy();
        assert!(
            path_str.contains("iris-agentic-dev"),
            "path should contain iris-agentic-dev, got: {path_str}"
        );
        assert!(
            path_str.ends_with("servers.json"),
            "path should end with servers.json, got: {path_str}"
        );
        // Also check the combined suffix
        let expected_suffix = std::path::MAIN_SEPARATOR_STR.to_string()
            + "iris-agentic-dev"
            + std::path::MAIN_SEPARATOR_STR
            + "servers.json";
        assert!(
            path_str.ends_with(&expected_suffix),
            "path should end with iris-agentic-dev/servers.json (with correct separator), got: {path_str}"
        );
    }

    #[test]
    fn version_preserved_on_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("servers.json");
        let cfg = ServersConfig {
            version: 1,
            servers: HashMap::new(),
            default: None,
        };
        save_to_path(&cfg, &path).expect("save should succeed");
        let loaded = load_from_path(&path);
        assert_eq!(loaded.version, 1);
    }

    #[test]
    fn atomic_write_creates_parent_dirs() {
        let dir = tempfile::tempdir().unwrap();
        let nested = dir
            .path()
            .join("a")
            .join("b")
            .join("c")
            .join("servers.json");
        let cfg = ServersConfig::default();
        save_to_path(&cfg, &nested).expect("should create parent dirs and save");
        assert!(nested.exists());
    }

    #[test]
    fn malformed_json_returns_default() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("servers.json");
        fs::write(&path, b"{ this is not valid json }").unwrap();
        let cfg = load_from_path(&path);
        assert!(cfg.servers.is_empty());
        assert_eq!(cfg.version, 1);
    }
}

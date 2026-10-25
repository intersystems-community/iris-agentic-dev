use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// A single IRIS server entry in the iad-native config file.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct ServerEntry {
    pub host: String,
    pub port: u16,
    pub namespace: String,
    pub username: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scheme: Option<String>,
    /// URL path prefix the IRIS web server is served under, e.g. `"/hs20261"` for a HealthShare
    /// instance behind a shared gateway. Same meaning as `web_prefix` in
    /// `.iris-agentic-dev.toml` and `webServer.pathPrefix` in a VS Code Server Manager profile,
    /// whose spelling is accepted as an alias here.
    ///
    /// Stored as the operator typed it; normalised when the URL is built
    /// (`connection_pool::web_prefix_path_part`). `None` and `Some("")` both mean no prefix.
    #[serde(default, alias = "pathPrefix", skip_serializing_if = "Option::is_none")]
    pub web_prefix: Option<String>,
    /// Plaintext credential stored as a fallback when the OS keychain is unavailable
    /// (e.g. headless MCP contexts, Remote SSH). Keychain takes priority when present.
    /// Never returned in any tool response — only read for connection auth.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub password: Option<String>,
    /// The endpoint `password` was saved for (`scheme://host:port[/prefix]`). The password is used
    /// only while the entry still points there, so editing the host in this file, or
    /// re-registering the name at another host, cannot send it somewhere new (spec 141, FR-023).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub password_endpoint: Option<String>,
    /// `"server-manager"` for an entry `iris_import_servers` wrote: its password is the Server
    /// Manager profile's, read only while the entry's endpoint equals the profile's.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub credential: Option<String>,
    /// Check this server's certificate? `false` turns it off for this entry only. Absent validates.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tls_verify: Option<bool>,
    /// The restricted account the agent's code runs as on this server (spec 141, R2). Its password
    /// is in the OS keychain under iad's own service, bound to this endpoint.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub service_username: Option<String>,
    /// Plaintext fallback for the service account's password, under the same rule as `password`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub service_password: Option<String>,
    /// The Docker container of this server, the only one a docker exec fallback may use for it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub container: Option<String>,
}

/// What `iris_add_server` was asked to register.
#[derive(Debug, Clone, Default)]
pub struct NewServer {
    pub host: String,
    pub port: u16,
    pub namespace: String,
    pub username: String,
    pub description: Option<String>,
    pub scheme: Option<String>,
    pub web_prefix: Option<String>,
    pub tls_verify: Option<bool>,
    pub password: Option<String>,
}

/// What the OS keychain did with a new password.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeychainOutcome {
    /// No password was given, so the keychain was not asked.
    NotTried,
    Stored,
    /// There is no keychain here (headless host, Remote SSH).
    Unavailable,
}

/// The decision `iris_add_server` makes about one entry and its credential.
#[derive(Debug, Clone)]
pub struct ServerMerge {
    pub entry: ServerEntry,
    /// An earlier password was dropped because the endpoint moved.
    pub password_cleared: bool,
    /// Where this call left the password: `"keychain"`, `"file"` or `"none"`.
    pub credential_store: &'static str,
    /// The endpoint of an earlier keychain item that the move made stale: delete it.
    pub stale_keychain: Option<crate::iris::endpoint::Endpoint>,
    /// The new endpoint is not where the old entry pointed.
    pub moved: bool,
}

/// The endpoint of an entry, parsed. A scheme that is not `http` or `https` is an error.
pub fn entry_endpoint(
    e: &ServerEntry,
) -> Result<crate::iris::endpoint::Endpoint, crate::iris::endpoint::InvalidEndpoint> {
    crate::iris::endpoint::Endpoint::parse(
        e.scheme.as_deref(),
        &e.host,
        e.port,
        e.web_prefix.as_deref(),
    )
}

/// Decide an entry and its credential from the old entry, the new parameters and what the keychain
/// did (R4b D11). Pure: the caller does the keychain write first and saves the file once, last.
///
/// - The endpoint is parsed first; a bad scheme is an error and nothing is saved.
/// - A moved endpoint drops the old password, plaintext and keychain item alike.
/// - A password the keychain took leaves none in the entry. One it could not take stays in the
///   entry, bound to the endpoint it was given for.
/// - No new password and no move: the stored one stays.
pub fn merge_server(
    old: Option<&ServerEntry>,
    new: &NewServer,
    keychain: KeychainOutcome,
) -> Result<ServerMerge, crate::iris::endpoint::InvalidEndpoint> {
    let mut entry = ServerEntry {
        host: new.host.clone(),
        port: new.port,
        namespace: new.namespace.clone(),
        username: new.username.clone(),
        description: new.description.clone(),
        scheme: new.scheme.clone(),
        web_prefix: new.web_prefix.clone(),
        password: None,
        password_endpoint: None,
        credential: None,
        tls_verify: new.tls_verify,
        ..Default::default()
    };
    let endpoint = entry_endpoint(&entry)?;
    let old_endpoint = old.and_then(|o| entry_endpoint(o).ok());
    let moved = match (&old_endpoint, old) {
        (Some(oe), _) => !oe.same_server(&endpoint),
        (None, Some(_)) => true,
        (None, None) => false,
    };
    let had_password = old.is_some_and(|o| o.password.is_some());
    let password_cleared = moved && (had_password || old.is_some_and(|o| o.credential.is_some()));
    let stale_keychain = if moved { old_endpoint } else { None };
    // A certificate setting the operator wrote by hand stays while the server stays where it is.
    if entry.tls_verify.is_none() && !moved {
        entry.tls_verify = old.and_then(|o| o.tls_verify);
    }

    let credential_store = match (new.password.as_deref(), keychain) {
        (Some(_), KeychainOutcome::Stored) => "keychain",
        (Some(pw), _) => {
            entry.password = Some(pw.to_string());
            entry.password_endpoint = Some(endpoint.base_url());
            "file"
        }
        (None, _) if !moved => {
            // Nothing new, nothing moved: what was stored stays.
            if let Some(o) = old {
                entry.password = o.password.clone();
                entry.password_endpoint = o.password_endpoint.clone();
                entry.credential = o.credential.clone();
            }
            if entry.password.is_some() {
                "file"
            } else {
                "none"
            }
        }
        (None, _) => "none",
    };

    Ok(ServerMerge {
        entry,
        password_cleared,
        credential_store,
        stale_keychain,
        moved,
    })
}

/// Carry the service-account settings of a registration onto the merged entry (spec 141, R2).
///
/// `service_username` and `container` are not secrets and are replaced by what the call gave. The
/// plaintext service password of an earlier entry stays only while the endpoint has not moved and
/// the account is the same one; it is bound to the entry's `password_endpoint` like the primary
/// password, so the binding is carried with it.
pub fn carry_service_account(
    merge: &mut ServerMerge,
    old: Option<&ServerEntry>,
    service_username: Option<&str>,
    container: Option<&str>,
) {
    let entry = &mut merge.entry;
    entry.service_username = service_username.map(str::to_string);
    entry.container = container.map(str::to_string);
    entry.service_password = None;
    let kept = old.filter(|o| !merge.moved && o.service_username.as_deref() == service_username);
    if let Some(o) = kept.filter(|o| o.service_password.is_some()) {
        entry.service_password = o.service_password.clone();
        if entry.password_endpoint.is_none() {
            entry.password_endpoint = o.password_endpoint.clone();
        }
    }
}

/// Is `name` the name of a Server Manager profile? `iris_add_server` refuses such a name: an entry
/// of that name would be read as the profile.
pub fn name_conflicts(
    name: &str,
    profiles: &[crate::iris::server_manager::ServerManagerProfile],
) -> bool {
    profiles.iter().any(|p| p.name.eq_ignore_ascii_case(name))
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

/// Saves the config to the given path using an atomic write (temp file + rename), owner-only:
/// the file holds passwords on a platform with no keychain (spec 141, FR-023).
///
/// Creates parent directories if they do not exist.
pub fn save_to_path(cfg: &ServersConfig, path: &Path) -> Result<(), Box<dyn std::error::Error>> {
    let contents = serde_json::to_string_pretty(cfg)?;
    crate::iris::private_file::write_private_atomic(path, contents.as_bytes())?;
    Ok(())
}

/// Saves the config to the default platform path using an atomic write.
pub fn save_native_config(cfg: &ServersConfig) -> Result<(), Box<dyn std::error::Error>> {
    save_to_path(cfg, &native_config_path())
}

/// Check a `web_prefix` before it is written to the registry.
///
/// Only one shape is refused: a prefix carrying a scheme or a host. Concatenated onto the base URL
/// it produces `http://host:8080http://other/hs`, which fails later as a connection error with
/// nothing pointing at the prefix. Everything else — empty, bare, slash-wrapped, multi-segment — is
/// accepted and normalised at URL-build time.
///
/// Returns `Err(message)` quoting what was passed.
pub fn validate_web_prefix(prefix: &str) -> Result<(), String> {
    let trimmed = prefix.trim();
    if trimmed.contains("://") || trimmed.starts_with("//") {
        return Err(format!(
            "web_prefix must be a path only, e.g. \"/hs20261\" — got \"{prefix}\". \
             The scheme, host, and port come from the entry's own scheme/host/port fields."
        ));
    }
    Ok(())
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
                web_prefix: None,
                ..Default::default()
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
                web_prefix: None,
                ..Default::default()
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

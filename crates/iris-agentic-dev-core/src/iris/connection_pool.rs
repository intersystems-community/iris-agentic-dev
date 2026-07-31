//! Multi-instance connection pool for IRIS connections (072-multi-instance-pool).
//!
//! `ConnectionPool` holds a named set of `IrisConnection` instances loaded from
//! all configured sources in priority order (first-wins name dedup).

use std::collections::HashMap;
use std::sync::Arc;

use rmcp::ErrorData as McpError;

use crate::iris::connection::{DiscoverySource, IrisConnection};

// ── Constants ─────────────────────────────────────────────────────────────────

const SERVER_NOT_FOUND_PREFIX: &str = "SERVER_NOT_FOUND: no server named '";

// ── ConnectionPool ────────────────────────────────────────────────────────────

/// A named pool of IRIS connections, loaded from all configured sources.
pub struct ConnectionPool {
    instances: HashMap<String, Arc<IrisConnection>>,
    default_name: Option<String>,
    /// Source label per connection name. Values: `"iad-native"`, `"vscode"`, `"fleet"`, `"env"`.
    sources: HashMap<String, String>,
}

impl ConnectionPool {
    /// Create an empty pool with no connections and no default.
    pub fn empty() -> Self {
        Self {
            instances: HashMap::new(),
            default_name: None,
            sources: HashMap::new(),
        }
    }

    /// Return the source label for a named connection, or `"unknown"` if not found.
    ///
    /// Source values: `"iad-native"`, `"vscode"`, `"fleet"`, `"env"`.
    pub fn source_of(&self, name: &str) -> &str {
        self.sources
            .get(name)
            .map(|s| s.as_str())
            .unwrap_or("unknown")
    }

    /// Retrieve a connection by name, or the default connection when `name` is `None`.
    ///
    /// - `get(None)` with no default → `IRIS_UNREACHABLE` error.
    /// - `get(Some("x"))` when `"x"` is absent → `SERVER_NOT_FOUND` error.
    pub fn get(&self, name: Option<&str>) -> Result<Arc<IrisConnection>, McpError> {
        match name {
            None => {
                // Return default, or error if no default.
                match &self.default_name {
                    Some(default) => self
                        .instances
                        .get(default.as_str())
                        .cloned()
                        .ok_or_else(|| {
                            McpError::invalid_request(
                                format!(
                                    "IRIS_UNREACHABLE: no IRIS connection configured (default '{default}' missing from pool). \
                                     Set IRIS_HOST or configure servers.json.",
                                ),
                                None,
                            )
                        }),
                    None => {
                        if let Some(conn) = self.instances.values().next() {
                            return Ok(conn.clone());
                        }
                        Err(McpError::invalid_request(
                            "IRIS_UNREACHABLE: no IRIS connection configured. \
                             Set IRIS_HOST or configure servers.json.",
                            None,
                        ))
                    }
                }
            }
            Some(n) => self.instances.get(n).cloned().ok_or_else(|| {
                McpError::invalid_request(
                    format!(
                        "{SERVER_NOT_FOUND_PREFIX}{n}' in pool. \
                         Use iris_servers to list available instances."
                    ),
                    None,
                )
            }),
        }
    }

    /// Return sorted list of server names in the pool.
    pub fn names(&self) -> Vec<&str> {
        let mut names: Vec<&str> = self.instances.keys().map(|s| s.as_str()).collect();
        names.sort();
        names
    }

    /// Return the name of the default connection, if any.
    pub fn default_name(&self) -> Option<&str> {
        self.default_name.as_deref()
    }

    /// Return the number of connections in the pool.
    pub fn len(&self) -> usize {
        self.instances.len()
    }

    /// Return `true` if the pool contains no connections.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Create a `ConnectionPoolBuilder` to assemble a pool from multiple sources.
    pub fn builder() -> ConnectionPoolBuilder {
        ConnectionPoolBuilder {
            instances: Vec::new(),
            default_name: None,
            sources: HashMap::new(),
        }
    }
}

// ── ConnectionPoolBuilder ─────────────────────────────────────────────────────

/// Assembles a `ConnectionPool` from multiple sources with first-wins name dedup.
pub struct ConnectionPoolBuilder {
    /// Ordered insertion list; first entry for a given name wins.
    instances: Vec<(String, Arc<IrisConnection>)>,
    default_name: Option<String>,
    /// Source label for each entry, keyed by name.
    sources: HashMap<String, String>,
}

impl ConnectionPoolBuilder {
    /// Add a named connection. If `is_default` is true and no default has been set yet,
    /// this entry becomes the default. Duplicate names are silently ignored (first wins).
    /// Source is recorded as `"unknown"`.
    pub fn add(&mut self, name: String, conn: IrisConnection, is_default: bool) {
        self.add_with_source(name, conn, is_default, "unknown");
    }

    /// Add a named connection with an explicit source label.
    ///
    /// Source values used by `load_pool`: `"iad-native"`, `"vscode"`, `"fleet"`, `"env"`.
    /// Duplicate names are silently ignored (first wins).
    pub fn add_with_source(
        &mut self,
        name: String,
        conn: IrisConnection,
        is_default: bool,
        source: &str,
    ) {
        // First-wins: only add if name not already present.
        let already_present = self.instances.iter().any(|(n, _)| n == &name);
        if !already_present {
            if is_default && self.default_name.is_none() {
                self.default_name = Some(name.clone());
            }
            self.sources.insert(name.clone(), source.to_string());
            self.instances.push((name, Arc::new(conn)));
        }
    }

    /// Consume the builder and produce a `ConnectionPool`.
    pub fn build(self) -> ConnectionPool {
        let instances: HashMap<String, Arc<IrisConnection>> = self.instances.into_iter().collect();
        ConnectionPool {
            instances,
            default_name: self.default_name,
            sources: self.sources,
        }
    }
}

// ── load_pool ─────────────────────────────────────────────────────────────────

/// Load a `ConnectionPool` from all configured sources in priority order.
///
/// Sources (first-wins name dedup across all sources):
/// 1. iad-native `servers.json` (`load_native_config`)
/// 2. VS Code / Cursor Server Manager `settings.json` (`parse_sm_settings`)
/// 3. `[instance.*]` blocks from workspace TOML (`load_fleet_config`)
/// 4. `IRIS_HOST` env var → `"_env"` entry
///
/// Default precedence: `servers.json` `"default"` field; then `"_env"` if present;
/// then implicit first entry (when pool has exactly one entry).
///
/// Never panics — any source that fails to load is silently skipped.
pub fn load_pool(config_file: Option<&std::path::Path>) -> ConnectionPool {
    use crate::iris::server_manager;
    use crate::iris::servers_config;
    use crate::iris::workspace_config;

    let mut builder = ConnectionPool::builder();

    // ── Source 1: iad-native servers.json ────────────────────────────────────
    let native_cfg = servers_config::load_native_config();
    let native_default = native_cfg.default.clone();
    for (name, entry) in &native_cfg.servers {
        let scheme = entry.scheme.as_deref().unwrap_or("http");
        let base_url = format!("{}://{}:{}", scheme, entry.host, entry.port);
        let is_default = native_default.as_deref() == Some(name.as_str());
        let conn = IrisConnection::new(
            base_url,
            entry.namespace.clone(),
            entry.username.clone(),
            "", // password resolved from keychain below
            DiscoverySource::ServerManager {
                server_name: name.clone(),
            },
        );
        // Attempt keychain credential resolution; fall back to empty string.
        let password =
            server_manager::resolve_credential(name, &entry.username).unwrap_or_default();
        let mut conn = conn;
        conn.password = password;
        builder.add_with_source(name.clone(), conn, is_default, "iad-native");
    }

    // ── Source 2: VS Code / Cursor Server Manager settings.json ──────────────
    let sm_paths: Vec<std::path::PathBuf> = {
        let mut paths = Vec::new();
        if let Some(p) = server_manager::sm_settings_path() {
            paths.push(p);
        }
        // Cursor paths (complement to VS Code sm_settings_path)
        if let Some(home) = dirs::home_dir() {
            #[cfg(target_os = "macos")]
            paths.push(home.join("Library/Application Support/Cursor/User/settings.json"));
            #[cfg(not(target_os = "macos"))]
            paths.push(home.join(".config/Cursor/User/settings.json"));
        }
        paths
    };

    for sm_path in &sm_paths {
        let profiles = server_manager::parse_sm_settings(sm_path);
        for profile in profiles {
            let path_part = profile
                .path_prefix
                .as_deref()
                .map(|p| format!("/{}", p.trim_matches('/')))
                .unwrap_or_default();
            let base_url = format!(
                "{}://{}:{}{}",
                profile.scheme, profile.host, profile.port, path_part
            );
            let password = server_manager::resolve_credential(&profile.name, &profile.username)
                .unwrap_or_else(|_| profile.password_deprecated.clone().unwrap_or_default());
            let conn = IrisConnection::new(
                base_url,
                "USER", // SM profiles don't carry a namespace; default to USER
                profile.username.clone(),
                password,
                DiscoverySource::ServerManager {
                    server_name: profile.name.clone(),
                },
            );
            builder.add_with_source(profile.name.clone(), conn, false, "vscode");
        }
    }

    // ── Source 3: [instance.*] blocks from workspace TOML ────────────────────
    let workspace_path_str = config_file.and_then(|p| p.to_str());
    if let Some(fleet) = workspace_config::load_fleet_config(workspace_path_str) {
        if fleet.mode.as_deref() == Some("operate") {
            for (name, inst) in &fleet.instance {
                let host = inst.host.as_deref().unwrap_or("localhost");
                let port = inst.web_port.unwrap_or(52773);
                let base_url = format!("http://{}:{}", host, port);
                let ns = inst.namespace.as_deref().unwrap_or("USER");
                let user = inst.username.as_deref().unwrap_or("_SYSTEM");
                let pw = inst.password.as_deref().unwrap_or("");
                let conn = IrisConnection::new(base_url, ns, user, pw, DiscoverySource::EnvVar);
                builder.add_with_source(name.clone(), conn, false, "fleet");
            }
        }
    }

    // ── Source 4: IRIS_HOST env var → "_env" ─────────────────────────────────
    if let Ok(iris_host) = std::env::var("IRIS_HOST") {
        if !iris_host.is_empty() {
            let port = std::env::var("IRIS_WEB_PORT")
                .ok()
                .and_then(|p| p.parse::<u16>().ok())
                .unwrap_or(52773);
            let scheme = std::env::var("IRIS_SCHEME").unwrap_or_else(|_| "http".to_string());
            let base_url = format!("{}://{}:{}", scheme, iris_host, port);
            let ns = std::env::var("IRIS_NAMESPACE").unwrap_or_else(|_| "USER".to_string());
            let user = std::env::var("IRIS_USERNAME").unwrap_or_else(|_| "_SYSTEM".to_string());
            let pw = std::env::var("IRIS_PASSWORD").unwrap_or_default();
            let conn = IrisConnection::new(base_url, ns, user, pw, DiscoverySource::EnvVar);
            // "_env" is the default if no native default was set
            let is_default = builder.default_name.is_none();
            builder.add_with_source("_env".to_string(), conn, is_default, "env");
        }
    }

    builder.build()
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn make_conn(base_url: &str) -> IrisConnection {
        IrisConnection::new(base_url, "USER", "_SYSTEM", "", DiscoverySource::EnvVar)
    }

    fn pool_with_one(name: &str, base_url: &str) -> ConnectionPool {
        let mut b = ConnectionPool::builder();
        b.add(name.to_string(), make_conn(base_url), false);
        b.build()
    }

    // T011.1 — empty pool, get(None) returns IRIS_UNREACHABLE
    #[test]
    fn get_none_on_empty_pool_returns_unreachable() {
        let pool = ConnectionPool::empty();
        let err = pool.get(None).unwrap_err();
        let msg: &str = &err.message;
        assert!(
            msg.contains("IRIS_UNREACHABLE"),
            "expected IRIS_UNREACHABLE, got: {msg}"
        );
    }

    // T011.2 — pool with one entry "a", get(Some("x")) returns SERVER_NOT_FOUND
    #[test]
    fn get_named_missing_returns_not_found() {
        let pool = pool_with_one("a", "http://localhost:52773");
        let err = pool.get(Some("x")).unwrap_err();
        let msg: &str = &err.message;
        assert!(
            msg.contains("SERVER_NOT_FOUND"),
            "expected SERVER_NOT_FOUND, got: {msg}"
        );
    }

    // T011.3 — pool with entry "a", get(Some("a")) returns the right connection
    #[test]
    fn get_named_present_returns_connection() {
        let pool = pool_with_one("a", "http://localhost:52773");
        let conn = pool.get(Some("a")).expect("should find 'a'");
        assert_eq!(conn.base_url, "http://localhost:52773");
    }

    // T011.4 — pool with "a" and "b", default="a", get(None) returns "a"
    #[test]
    fn get_none_with_default_returns_default() {
        let mut b = ConnectionPool::builder();
        b.add("a".to_string(), make_conn("http://a:52773"), true);
        b.add("b".to_string(), make_conn("http://b:52773"), false);
        let pool = b.build();
        let conn = pool.get(None).expect("should return default 'a'");
        assert_eq!(conn.base_url, "http://a:52773");
    }

    // T011.5 — pool with 3 entries, len() == 3
    #[test]
    fn len_returns_correct_count() {
        let mut b = ConnectionPool::builder();
        b.add("a".to_string(), make_conn("http://a:52773"), false);
        b.add("b".to_string(), make_conn("http://b:52773"), false);
        b.add("c".to_string(), make_conn("http://c:52773"), false);
        let pool = b.build();
        assert_eq!(pool.len(), 3);
    }

    // T011.6 — cascade: first-wins dedup — "dev" added from native source wins over vscode source
    #[test]
    fn cascade_iad_native_wins_over_vscode() {
        let mut b = ConnectionPool::builder();
        // First source (iad-native): "dev" at port 52780
        b.add("dev".to_string(), make_conn("http://localhost:52780"), true);
        // Second source (vscode): "dev" at port 52773 — should be silently dropped
        b.add(
            "dev".to_string(),
            make_conn("http://localhost:52773"),
            false,
        );
        let pool = b.build();
        // Only one "dev" in pool
        assert_eq!(pool.len(), 1);
        let conn = pool.get(Some("dev")).expect("should find 'dev'");
        assert_eq!(
            conn.base_url, "http://localhost:52780",
            "native source should win over vscode source"
        );
    }
}

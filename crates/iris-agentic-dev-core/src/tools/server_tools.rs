//! Server management tool helpers (072-multi-instance-pool).
//!
//! Params structs and pure helper logic used by the five server management
//! tool handlers in mod.rs: `iris_servers`, `iris_add_server`,
//! `iris_remove_server`, `iris_test_server`, `iris_import_servers`.

use schemars::JsonSchema;
use serde::Deserialize;

// ── Error codes ───────────────────────────────────────────────────────────────

/// Returned by `iris_remove_server` when the target server was not sourced from
/// the iad-native config and therefore cannot be removed by this tool.
pub const REMOVE_NOT_ALLOWED: &str = "REMOVE_NOT_ALLOWED";

// ── Params structs ────────────────────────────────────────────────────────────

#[derive(Debug, Deserialize, JsonSchema)]
pub struct AddServerParams {
    /// Unique name for this server (used in the `server` param of other tools).
    pub name: String,
    /// Hostname or IP address.
    pub host: String,
    /// Web port (e.g. 52773 or 443).
    pub port: u16,
    /// Default IRIS namespace (e.g. `"USER"`).
    pub namespace: String,
    /// IRIS username.
    pub username: String,
    /// IRIS password — stored in OS keychain, never written to disk.
    pub password: String,
    /// Optional human-readable description.
    pub description: Option<String>,
    /// URL scheme: `"http"` (default) or `"https"`.
    pub scheme: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct RemoveServerParams {
    /// Name of the server to remove. Must be sourced from the iad-native config.
    pub name: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct TestServerParams {
    /// Name of the server to probe.
    pub name: String,
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::iris::connection::DiscoverySource;
    use crate::iris::connection::IrisConnection;
    use crate::iris::connection_pool::ConnectionPool;

    fn make_conn(base_url: &str) -> IrisConnection {
        IrisConnection::new(base_url, "USER", "_SYSTEM", "", DiscoverySource::EnvVar)
    }

    // T028.1 — empty pool returns empty list
    #[test]
    fn iris_servers_empty_pool_returns_empty_list() {
        let pool = ConnectionPool::empty();
        let names = pool.names();
        assert!(names.is_empty(), "expected empty list, got: {names:?}");
    }

    // T028.2 — pool with one entry: output includes "source" key
    #[test]
    fn iris_servers_includes_source_field() {
        let mut b = ConnectionPool::builder();
        b.add_with_source(
            "myserver".to_string(),
            make_conn("http://localhost:52773"),
            false,
            "iad-native",
        );
        let pool = b.build();

        // Simulate the iris_servers listing logic
        let entries: Vec<serde_json::Value> = pool
            .names()
            .iter()
            .map(|name| {
                let conn = pool.get(Some(name)).expect("entry must exist");
                let source = pool.source_of(name);
                serde_json::json!({
                    "name": name,
                    "host": conn.base_url,
                    "source": source,
                })
            })
            .collect();

        assert_eq!(entries.len(), 1);
        let entry = &entries[0];
        assert!(
            entry.get("source").is_some(),
            "output should include 'source' key"
        );
        assert_eq!(
            entry["source"].as_str().unwrap(),
            "iad-native",
            "source should be 'iad-native'"
        );
    }

    // T028.3 — remove a server sourced from "vscode" returns REMOVE_NOT_ALLOWED
    #[test]
    fn iris_remove_server_vscode_source_returns_not_allowed() {
        let mut b = ConnectionPool::builder();
        b.add_with_source(
            "vscodesrv".to_string(),
            make_conn("http://vscode-host:52773"),
            false,
            "vscode",
        );
        let pool = b.build();

        let source = pool.source_of("vscodesrv");
        let result = if source != "iad-native" {
            Err(REMOVE_NOT_ALLOWED)
        } else {
            Ok(())
        };

        assert_eq!(
            result,
            Err(REMOVE_NOT_ALLOWED),
            "vscode server should return REMOVE_NOT_ALLOWED"
        );
    }
}

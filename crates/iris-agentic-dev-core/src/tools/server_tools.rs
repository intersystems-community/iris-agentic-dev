//! Server management tool helpers (072-multi-instance-pool, 098-server-probe).
//!
//! Params structs and pure helper logic used by the five server management
//! tool handlers in mod.rs: `iris_servers`, `iris_add_server`,
//! `iris_remove_server`, `iris_test_server`, `iris_import_servers`.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

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

/// Params for `iris_test_server` (098-server-probe).
///
/// Accepts either a pool-registered server name OR ad-hoc connection params.
/// When `host` is provided, bypasses the pool and probes the target directly.
#[derive(Debug, Default, Deserialize, JsonSchema)]
pub struct TestServerParams {
    /// Name of a registered server to probe. Mutually exclusive with `host`.
    pub name: Option<String>,
    /// Hostname or IP for an ad-hoc probe (not required to be in the pool).
    pub host: Option<String>,
    /// Web port for the ad-hoc probe (defaults to 52773 when omitted).
    #[serde(default)]
    pub web_port: Option<u16>,
    /// IRIS username for the ad-hoc probe.
    #[serde(default)]
    pub username: Option<String>,
    /// IRIS password for the ad-hoc probe.
    #[serde(default)]
    pub password: Option<String>,
}

/// Optional params for `iris_servers` (098-server-probe).
#[derive(Debug, Default, Deserialize, JsonSchema)]
pub struct IrisServersParams {
    /// When `true`, probe each server for reachability and include
    /// `reachable`, `latency_ms`, and `error` fields in the response.
    /// Default `false` (or absent) — fast path, `reachable: null` per entry.
    #[serde(default)]
    pub probe: Option<bool>,
}

/// Result of a single server probe (098-server-probe).
#[derive(Debug, Serialize)]
pub struct ProbeResult {
    pub reachable: bool,
    pub auth: bool,
    pub iris_version: Option<String>,
    pub atelier_version: Option<String>,
    pub namespace: Option<String>,
    pub latency_ms: Option<u128>,
    pub error: Option<String>,
}

/// Probe a single IRIS server via Atelier REST (`GET /api/atelier/`) with a 5-second timeout.
///
/// Returns `ProbeResult { reachable: false, auth: false, ... error: Some(...) }` on any
/// network error or timeout. Returns `reachable: true, auth: false` on HTTP 401.
pub async fn probe_server(
    host: &str,
    web_port: u16,
    namespace: &str,
    username: &str,
    password: &str,
) -> ProbeResult {
    use crate::iris::connection::IrisConnection;
    use std::time::Instant;

    let client = match IrisConnection::probe_client() {
        Ok(c) => c,
        Err(e) => {
            return ProbeResult {
                reachable: false,
                auth: false,
                iris_version: None,
                atelier_version: None,
                namespace: None,
                latency_ms: None,
                error: Some(format!("Failed to build HTTP client: {e}")),
            };
        }
    };

    let url = format!("http://{host}:{web_port}/api/atelier/");
    let start = Instant::now();

    let result = tokio::time::timeout(
        std::time::Duration::from_secs(5),
        client.get(&url).basic_auth(username, Some(password)).send(),
    )
    .await;

    let latency_ms = start.elapsed().as_millis();

    match result {
        Err(_timeout) => ProbeResult {
            reachable: false,
            auth: false,
            iris_version: None,
            atelier_version: None,
            namespace: None,
            latency_ms: Some(latency_ms),
            error: Some("Connection timed out after 5 seconds".to_string()),
        },
        Ok(Err(e)) => ProbeResult {
            reachable: false,
            auth: false,
            iris_version: None,
            atelier_version: None,
            namespace: None,
            latency_ms: Some(latency_ms),
            error: Some(e.to_string()),
        },
        Ok(Ok(resp)) => {
            let status = resp.status();
            if status == reqwest::StatusCode::UNAUTHORIZED {
                return ProbeResult {
                    reachable: true,
                    auth: false,
                    iris_version: None,
                    atelier_version: None,
                    namespace: Some(namespace.to_string()),
                    latency_ms: Some(latency_ms),
                    error: Some("Authentication failed (HTTP 401)".to_string()),
                };
            }
            if !status.is_success() {
                return ProbeResult {
                    reachable: true,
                    auth: false,
                    iris_version: None,
                    atelier_version: None,
                    namespace: Some(namespace.to_string()),
                    latency_ms: Some(latency_ms),
                    error: Some(format!("HTTP {status}")),
                };
            }
            // Parse Atelier root response for version info.
            let (iris_version, atelier_version) =
                if let Ok(body) = resp.json::<serde_json::Value>().await {
                    let content = &body["result"]["content"];
                    let iv = content["version"].as_str().map(|s| s.to_string());
                    let av = content["api"].as_u64().map(|v| v.to_string());
                    (iv, av)
                } else {
                    (None, None)
                };
            ProbeResult {
                reachable: true,
                auth: true,
                iris_version,
                atelier_version,
                namespace: Some(namespace.to_string()),
                latency_ms: Some(latency_ms),
                error: None,
            }
        }
    }
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

    // T008 — TestServerParams ad-hoc fields deserialize correctly
    #[test]
    fn test_server_params_adhoc_deserializes() {
        let j = r#"{"host":"localhost","web_port":52780,"username":"_SYSTEM","password":"SYS"}"#;
        let p: TestServerParams = serde_json::from_str(j).expect("must deserialize");
        assert_eq!(p.host.as_deref(), Some("localhost"));
        assert_eq!(p.web_port, Some(52780));
        assert_eq!(p.username.as_deref(), Some("_SYSTEM"));
        assert_eq!(p.password.as_deref(), Some("SYS"));
        assert!(p.name.is_none());
    }

    // T009 — TestServerParams both None round-trips
    #[test]
    fn test_server_params_both_none() {
        let j = r#"{}"#;
        let p: TestServerParams = serde_json::from_str(j).expect("must deserialize");
        assert!(p.name.is_none());
        assert!(p.host.is_none());
    }

    // T010 — TestServerParams name-only path unchanged
    #[test]
    fn test_server_params_name_only() {
        let j = r#"{"name":"myserver"}"#;
        let p: TestServerParams = serde_json::from_str(j).expect("must deserialize");
        assert_eq!(p.name.as_deref(), Some("myserver"));
        assert!(p.host.is_none());
    }

    // T030 — IrisServersParams deserializes correctly
    #[test]
    fn test_iris_servers_params_deserialize() {
        let j_none = r#"{}"#;
        let p: IrisServersParams = serde_json::from_str(j_none).expect("must deserialize");
        assert!(p.probe.is_none());

        let j_true = r#"{"probe":true}"#;
        let p2: IrisServersParams = serde_json::from_str(j_true).expect("must deserialize");
        assert_eq!(p2.probe, Some(true));

        let j_false = r#"{"probe":false}"#;
        let p3: IrisServersParams = serde_json::from_str(j_false).expect("must deserialize");
        assert_eq!(p3.probe, Some(false));
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

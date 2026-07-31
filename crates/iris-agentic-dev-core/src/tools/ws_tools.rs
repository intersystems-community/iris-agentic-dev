//! WebSocket terminal session tool params (072-b).
//!
//! Tool handlers for `iris_ws_open`, `iris_ws_exec`, `iris_ws_close` live in
//! `tools/mod.rs` (in the `#[tool_router]` impl block). This file holds the
//! parameter structs and shared helpers.

use schemars::JsonSchema;
use serde::Deserialize;

// ── Params structs ────────────────────────────────────────────────────────────

#[derive(Debug, Deserialize, JsonSchema)]
pub struct WsOpenParams {
    /// Named registered IRIS instance to open the session on. Defaults to the active connection.
    #[serde(default)]
    pub server: Option<String>,
    /// IRIS namespace for the terminal session. Defaults to the connection's default namespace.
    #[serde(default)]
    pub namespace: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct WsExecParams {
    /// Session token returned by `iris_ws_open`.
    pub session: String,
    /// ObjectScript code to execute in the terminal session.
    pub code: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct WsCloseParams {
    /// Session token returned by `iris_ws_open`.
    pub session: String,
}

// ── Unit tests ────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use crate::iris::connection::{AtelierVersion, DiscoverySource, IrisConnection};

    fn make_conn_with_version(v: AtelierVersion) -> IrisConnection {
        let mut c = IrisConnection::new(
            "http://localhost:52780",
            "USER",
            "_SYSTEM",
            "SYS",
            DiscoverySource::EnvVar,
        );
        c.atelier_version = v;
        c
    }

    // T056: version gate — V1 does not support WS terminal.
    #[test]
    fn atelier_v1_does_not_support_ws_terminal() {
        let conn = make_conn_with_version(AtelierVersion::V1);
        assert!(
            !conn.atelier_version.supports_ws_terminal(),
            "V1 must not support WS terminal"
        );
    }

    // V2 also does not support WS terminal.
    #[test]
    fn atelier_v2_does_not_support_ws_terminal() {
        let conn = make_conn_with_version(AtelierVersion::V2);
        assert!(
            !conn.atelier_version.supports_ws_terminal(),
            "V2 must not support WS terminal"
        );
    }

    // V7 supports WS terminal.
    #[test]
    fn atelier_v7_supports_ws_terminal() {
        let conn = make_conn_with_version(AtelierVersion::V7);
        assert!(
            conn.atelier_version.supports_ws_terminal(),
            "V7 must support WS terminal"
        );
    }

    // V8 also supports WS terminal.
    #[test]
    fn atelier_v8_supports_ws_terminal() {
        let conn = make_conn_with_version(AtelierVersion::V8);
        assert!(
            conn.atelier_version.supports_ws_terminal(),
            "V8 must support WS terminal"
        );
    }
}

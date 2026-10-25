//! Parse VS Code settings.json for IRIS connection configuration.
//! Supports both direct host/port connections and named server references.

use crate::iris::connection::{DiscoverySource, IrisConnection};
use serde::Deserialize;
use std::collections::HashMap;
use std::path::Path;

#[derive(Debug, Deserialize, Default)]
pub struct VsCodeSettings {
    #[serde(rename = "objectscript.conn")]
    pub objectscript_conn: Option<ObjectScriptConn>,
    #[serde(rename = "intersystems.servers")]
    pub intersystems_servers: Option<HashMap<String, IntersystemsServer>>,
}

#[derive(Debug, Deserialize)]
pub struct ObjectScriptConn {
    pub active: Option<bool>,
    pub host: Option<String>,
    pub port: Option<u16>,
    /// The ObjectScript extension's own key: `true` means the web server is served over TLS.
    pub https: Option<bool>,
    pub username: Option<String>,
    pub password: Option<String>,
    pub ns: Option<String>,
    /// Named server reference (key into intersystems.servers)
    pub server: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct IntersystemsServer {
    #[serde(rename = "webServer")]
    pub web_server: WebServerSpec,
    #[serde(rename = "superServer")]
    pub super_server: Option<SuperServerSpec>,
    pub username: Option<String>,
    pub password: Option<String>,
    pub description: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct WebServerSpec {
    pub host: Option<String>,
    pub port: Option<u16>,
    pub scheme: Option<String>,
    #[serde(rename = "pathPrefix")]
    pub path_prefix: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct SuperServerSpec {
    pub host: Option<String>,
    pub port: Option<u16>,
}

impl IntersystemsServer {
    /// Returns the native SuperServer port if configured.
    pub fn super_server_port(&self) -> Option<u16> {
        self.super_server.as_ref().and_then(|ss| ss.port)
    }
}

/// Strip JSONC (JSON with Comments) syntax so serde_json can parse VS Code settings.json.
///
/// Handles: full-line // comments, inline // comments, /* block comments */,
/// and trailing commas before `}` or `]`.
fn strip_jsonc(src: &str) -> String {
    let mut out = String::with_capacity(src.len());
    let chars: Vec<char> = src.chars().collect();
    let mut i = 0;
    let mut in_string = false;

    while i < chars.len() {
        let c = chars[i];

        if in_string {
            out.push(c);
            if c == '\\' && i + 1 < chars.len() {
                // escaped character — emit both and skip next
                i += 1;
                out.push(chars[i]);
            } else if c == '"' {
                in_string = false;
            }
            i += 1;
            continue;
        }

        // Outside a string
        if c == '"' {
            in_string = true;
            out.push(c);
            i += 1;
            continue;
        }

        // Line comment //
        if c == '/' && i + 1 < chars.len() && chars[i + 1] == '/' {
            while i < chars.len() && chars[i] != '\n' {
                i += 1;
            }
            continue;
        }

        // Block comment /* ... */
        if c == '/' && i + 1 < chars.len() && chars[i + 1] == '*' {
            i += 2;
            while i + 1 < chars.len() && !(chars[i] == '*' && chars[i + 1] == '/') {
                i += 1;
            }
            i += 2; // skip */
            continue;
        }

        // Trailing comma: ,  followed (possibly with whitespace) by } or ]
        if c == ',' {
            let mut j = i + 1;
            while j < chars.len()
                && (chars[j] == ' ' || chars[j] == '\t' || chars[j] == '\n' || chars[j] == '\r')
            {
                j += 1;
            }
            if j < chars.len() && (chars[j] == '}' || chars[j] == ']') {
                // skip the comma
                i += 1;
                continue;
            }
        }

        out.push(c);
        i += 1;
    }
    out
}

/// Parse a VS Code settings.json file.
/// Bug 10: the old parser only stripped full-line // comments.
/// This version handles inline //, /* */ block comments, and trailing commas.
pub fn parse_vscode_settings(path: impl AsRef<Path>) -> anyhow::Result<VsCodeSettings> {
    let content = std::fs::read_to_string(path.as_ref())?;
    // First try raw JSON (fast path for files without JSONC syntax).
    if let Ok(settings) = serde_json::from_str::<VsCodeSettings>(&content) {
        return Ok(settings);
    }
    // Strip JSONC syntax and retry.
    let cleaned = strip_jsonc(&content);
    let settings: VsCodeSettings = serde_json::from_str(&cleaned).unwrap_or_default();
    Ok(settings)
}

/// A connection built from `.vscode/settings.json`, and the credentials held back building it.
#[derive(Debug)]
pub struct ResolvedVsCodeConn {
    pub conn: IrisConnection,
    pub set_aside: Vec<crate::iris::config_sources::SetAsideItem>,
}

/// What `.vscode/settings.json` yields.
#[derive(Debug)]
pub enum VsCodeResolution {
    /// The file names no active connection, or one that cannot be built (a bad address).
    NotConfigured,
    Connection(ResolvedVsCodeConn),
    /// The file names a connection and supplies no password iad may use: none in the file, none in
    /// the keychain (or no keychain), or no user-level profile to vouch for the address. No
    /// connection is built; the item says what is missing (spec 141, T204). A password is never
    /// guessed.
    PasswordMissing(crate::iris::config_sources::SetAsideItem),
}

/// The set-aside item for a connection whose password cannot be read.
fn password_missing(
    server: Option<&str>,
    target: &str,
    why: &str,
    lift: &str,
) -> crate::iris::config_sources::SetAsideItem {
    let name = server.unwrap_or("default");
    crate::iris::config_sources::SetAsideItem {
        id: format!("{name}:password_missing"),
        part: None,
        kind: crate::iris::config_sources::SetAsideKind::CredentialWithheld,
        setting: "password".to_string(),
        server: Some(name.to_string()),
        file: None,
        reason: format!(
            ".vscode/settings.json connects to {target} and iad has no password for it: {why}. \
             No connection was built."
        ),
        lift: lift.to_string(),
    }
}

impl VsCodeSettings {
    /// Convert parsed settings to an IrisConnection, resolving named servers against the
    /// user-level Server Manager profiles. `None` when the file names no connection or the
    /// password cannot be read; [`Self::resolve_connection`] says which.
    pub async fn to_iris_connection(&self) -> Option<IrisConnection> {
        match self.resolve_connection().await {
            VsCodeResolution::Connection(r) => {
                let mut conn = r.conn;
                if let Some(item) = r.set_aside.first() {
                    conn.credentials_withheld = Some(item.reason.clone());
                }
                Some(conn)
            }
            _ => None,
        }
    }

    /// [`Self::to_iris_connection`], keeping the reason a connection was not built.
    pub async fn resolve_connection(&self) -> VsCodeResolution {
        let profiles = crate::iris::server_manager::user_profiles();
        self.resolve_with_profiles(&profiles)
    }

    /// [`Self::to_iris_connection`] against a given list of user-level profiles, so a test needs
    /// no settings file of the user's. A named server gets a Server Manager keychain password only
    /// when the workspace file's address for it equals the user-level profile of the same name
    /// (spec 141, FR-024): a repository cannot name a host of its own and borrow the secret.
    pub fn connection_with_profiles(
        &self,
        profiles: &[crate::iris::server_manager::ServerManagerProfile],
    ) -> Option<ResolvedVsCodeConn> {
        match self.resolve_with_profiles(profiles) {
            VsCodeResolution::Connection(r) => Some(r),
            _ => None,
        }
    }

    /// [`Self::connection_with_profiles`], and when no connection is built, why not.
    pub fn resolve_with_profiles(
        &self,
        profiles: &[crate::iris::server_manager::ServerManagerProfile],
    ) -> VsCodeResolution {
        use VsCodeResolution::{Connection, NotConfigured, PasswordMissing};
        let Some(conn) = self.objectscript_conn.as_ref() else {
            return NotConfigured;
        };
        if conn.active == Some(false) {
            return NotConfigured;
        }

        // Named server path
        if let Some(server_name) = &conn.server {
            let Some(server) = self
                .intersystems_servers
                .as_ref()
                .and_then(|s| s.get(server_name))
            else {
                return NotConfigured;
            };
            let endpoint = match crate::iris::endpoint::Endpoint::parse(
                server.web_server.scheme.as_deref(),
                server.web_server.host.as_deref().unwrap_or("localhost"),
                server.web_server.port.unwrap_or(52773),
                server.web_server.path_prefix.as_deref(),
            ) {
                Ok(e) => e,
                Err(e) => {
                    tracing::warn!("INVALID_CONFIG: .vscode server '{server_name}': {e}");
                    return NotConfigured;
                }
            };
            let username = server.username.as_deref().unwrap_or("_SYSTEM");
            // Password priority: settings.json field → the Server Manager's keychain item, only
            // when this address is the profile's own. The ObjectScript extension stores passwords
            // in SecretStorage, not settings.json, so the keychain is the normal path.
            let mut set_aside = Vec::new();
            let password = if let Some(pw) = server.password.as_deref() {
                pw.to_string()
            } else {
                use crate::iris::server_manager::{sm_secret_for_in, SmSecretError};
                let missing = |why: String| {
                    PasswordMissing(password_missing(
                        Some(server_name),
                        &endpoint.to_string(),
                        &why,
                        "save the password for this server in Server Manager, or add a \
                         password to the entry",
                    ))
                };
                match sm_secret_for_in(profiles, server_name, username, &endpoint) {
                    Ok(pw) => pw,
                    Err(SmSecretError::Withheld { reason, .. }) => {
                        set_aside.push(crate::iris::config_sources::SetAsideItem {
                            id: format!("{server_name}:server_manager"),
                            part: None,
                            kind: crate::iris::config_sources::SetAsideKind::CredentialWithheld,
                            setting: "Server Manager keychain secret".to_string(),
                            server: Some(server_name.clone()),
                            file: None,
                            reason: format!(
                                "not sent to {endpoint}: .vscode/settings.json names this server \
                                 at an address other than the user-level profile's. {reason}"
                            ),
                            lift: "make the endpoints match, or give the entry its own password"
                                .to_string(),
                        });
                        String::new()
                    }
                    Err(SmSecretError::NoProfile { name }) => {
                        return missing(format!(
                            "the file sets no password and the user-level Server Manager \
                             settings have no server named '{name}' to vouch for the address"
                        ));
                    }
                    Err(SmSecretError::Credential(c)) => {
                        return missing(format!(
                            "the file sets no password and the keychain could not supply the \
                             Server Manager one ({c})"
                        ));
                    }
                }
            };
            let ns = conn.ns.as_deref().unwrap_or("USER");

            let iris_conn = IrisConnection::new(
                endpoint.base_url(),
                ns,
                username,
                password,
                DiscoverySource::VsCodeSettings,
            );
            // Note: super_server_port is available if needed for native connections
            return Connection(ResolvedVsCodeConn {
                conn: iris_conn,
                set_aside,
            });
        }

        // Direct host/port path
        let host = conn.host.as_deref().unwrap_or("localhost");
        let port = conn.port.unwrap_or(52773);
        let username = conn.username.as_deref().unwrap_or("_SYSTEM");
        let ns = conn.ns.as_deref().unwrap_or("USER");
        let scheme = if conn.https == Some(true) {
            "https"
        } else {
            "http"
        };
        let endpoint = match crate::iris::endpoint::Endpoint::parse(Some(scheme), host, port, None)
        {
            Ok(e) => e,
            Err(e) => {
                tracing::warn!("INVALID_CONFIG: objectscript.conn: {e}");
                return NotConfigured;
            }
        };
        let Some(password) = conn.password.as_deref() else {
            return PasswordMissing(password_missing(
                None,
                &endpoint.to_string(),
                "the file sets no password, and the editor's own prompt is not available here",
                "add a password to objectscript.conn, or connect with IRIS_HOST, IRIS_USERNAME and \
                 IRIS_PASSWORD",
            ));
        };

        Connection(ResolvedVsCodeConn {
            conn: IrisConnection::new(
                endpoint.base_url(),
                ns,
                username,
                password,
                DiscoverySource::VsCodeSettings,
            ),
            set_aside: Vec::new(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn strip(s: &str) -> String {
        strip_jsonc(s)
    }

    #[test]
    fn strip_plain_json_unchanged() {
        let s = r#"{"key": "value"}"#;
        assert_eq!(strip(s).trim(), s.trim());
    }

    #[test]
    fn strip_line_comment() {
        let s = "{\n// comment\n\"key\": 1\n}";
        let out = strip(s);
        assert!(!out.contains("// comment"));
        assert!(out.contains("\"key\""));
    }

    #[test]
    fn strip_inline_comment() {
        let s = r#"{"key": 1 // inline comment
}"#;
        let out = strip(s);
        assert!(!out.contains("// inline comment"));
        assert!(out.contains("\"key\""));
    }

    #[test]
    fn strip_block_comment() {
        let s = r#"{"key": /* block */ 1}"#;
        let out = strip(s);
        assert!(!out.contains("block"));
        assert!(out.contains("\"key\""));
    }

    #[test]
    fn strip_trailing_comma_object() {
        let s = r#"{"key": 1,}"#;
        let out = strip(s);
        // After stripping trailing comma, valid JSON
        let parsed: serde_json::Value = serde_json::from_str(&out).expect("valid json after strip");
        assert_eq!(parsed["key"], 1);
    }

    #[test]
    fn strip_trailing_comma_array() {
        let s = r#"[1, 2, 3,]"#;
        let out = strip(s);
        let parsed: serde_json::Value = serde_json::from_str(&out).expect("valid json");
        assert_eq!(parsed.as_array().unwrap().len(), 3);
    }

    #[test]
    fn strip_preserves_url_slashes_in_string() {
        let s = r#"{"url": "http://localhost:52773"}"#;
        let out = strip(s);
        assert!(
            out.contains("http://localhost:52773"),
            "url slashes preserved: {out}"
        );
    }

    #[test]
    fn strip_escaped_quote_in_string() {
        let s = r#"{"key": "say \"hello\""}"#;
        let out = strip(s);
        assert!(out.contains(r#"say \"hello\""#));
    }

    #[test]
    fn parse_settings_from_tmpfile_direct_host() {
        let mut f = tempfile::NamedTempFile::new().unwrap();
        writeln!(
            f,
            r#"{{
            "objectscript.conn": {{
                "active": true,
                "host": "localhost",
                "port": 52773,
                "username": "_SYSTEM",
                "password": "SYS",
                "ns": "USER"
            }}
        }}"#
        )
        .unwrap();
        let settings = parse_vscode_settings(f.path()).unwrap();
        let conn = settings.objectscript_conn.unwrap();
        assert_eq!(conn.host.as_deref(), Some("localhost"));
        assert_eq!(conn.port, Some(52773));
        assert_eq!(conn.ns.as_deref(), Some("USER"));
    }

    #[test]
    fn parse_settings_from_tmpfile_with_comment() {
        let mut f = tempfile::NamedTempFile::new().unwrap();
        writeln!(
            f,
            r#"{{
            // VS Code ObjectScript settings
            "objectscript.conn": {{
                "active": true,
                "host": "myserver", // the server
                "port": 52773,
            }}
        }}"#
        )
        .unwrap();
        let settings = parse_vscode_settings(f.path()).unwrap();
        let conn = settings.objectscript_conn.unwrap();
        assert_eq!(conn.host.as_deref(), Some("myserver"));
    }

    #[test]
    fn parse_settings_active_false() {
        let mut f = tempfile::NamedTempFile::new().unwrap();
        writeln!(
            f,
            r#"{{"objectscript.conn": {{"active": false, "host": "h", "port": 1}}}}"#
        )
        .unwrap();
        let settings = parse_vscode_settings(f.path()).unwrap();
        let conn = settings.objectscript_conn.unwrap();
        assert_eq!(conn.active, Some(false));
    }

    #[test]
    fn parse_settings_nonexistent_file_returns_error() {
        let result = parse_vscode_settings("/nonexistent/path/settings.json");
        assert!(result.is_err());
    }

    #[test]
    fn parse_settings_empty_file_returns_default() {
        let mut f = tempfile::NamedTempFile::new().unwrap();
        writeln!(f, "{{}}").unwrap();
        let settings = parse_vscode_settings(f.path()).unwrap();
        assert!(settings.objectscript_conn.is_none());
    }

    #[tokio::test]
    async fn to_iris_connection_active_false_returns_none() {
        let settings = VsCodeSettings {
            objectscript_conn: Some(ObjectScriptConn {
                active: Some(false),
                host: Some("localhost".to_string()),
                port: Some(52773),
                https: None,
                username: None,
                password: None,
                ns: None,
                server: None,
            }),
            intersystems_servers: None,
        };
        let conn = settings.to_iris_connection().await;
        assert!(conn.is_none());
    }

    #[tokio::test]
    async fn to_iris_connection_direct_host() {
        let settings = VsCodeSettings {
            objectscript_conn: Some(ObjectScriptConn {
                active: Some(true),
                host: Some("localhost".to_string()),
                port: Some(52773),
                https: None,
                username: Some("_SYSTEM".to_string()),
                password: Some("SYS".to_string()),
                ns: Some("USER".to_string()),
                server: None,
            }),
            intersystems_servers: None,
        };
        let conn = settings.to_iris_connection().await;
        assert!(conn.is_some());
    }

    #[tokio::test]
    async fn to_iris_connection_no_conn_returns_none() {
        let settings = VsCodeSettings {
            objectscript_conn: None,
            intersystems_servers: None,
        };
        let conn = settings.to_iris_connection().await;
        assert!(conn.is_none());
    }

    #[test]
    fn super_server_port_returns_port_when_configured() {
        let server = IntersystemsServer {
            web_server: WebServerSpec {
                host: Some("localhost".to_string()),
                port: Some(52773),
                scheme: None,
                path_prefix: None,
            },
            super_server: Some(SuperServerSpec {
                host: Some("localhost".to_string()),
                port: Some(1972),
            }),
            username: None,
            password: None,
            description: None,
        };
        assert_eq!(server.super_server_port(), Some(1972));
    }

    #[test]
    fn super_server_port_returns_none_when_absent() {
        let server = IntersystemsServer {
            web_server: WebServerSpec {
                host: Some("localhost".to_string()),
                port: Some(52773),
                scheme: None,
                path_prefix: None,
            },
            super_server: None,
            username: None,
            password: None,
            description: None,
        };
        assert!(server.super_server_port().is_none());
    }

    #[tokio::test]
    async fn to_iris_connection_named_server() {
        let mut servers = HashMap::new();
        servers.insert(
            "my-iris".to_string(),
            IntersystemsServer {
                web_server: WebServerSpec {
                    host: Some("iris-host".to_string()),
                    port: Some(52773),
                    scheme: Some("http".to_string()),
                    path_prefix: None,
                },
                super_server: None,
                username: Some("admin".to_string()),
                password: Some("pass".to_string()),
                description: None,
            },
        );
        let settings = VsCodeSettings {
            objectscript_conn: Some(ObjectScriptConn {
                active: None,
                host: None,
                port: None,
                https: None,
                username: None,
                password: None,
                ns: Some("PROD".to_string()),
                server: Some("my-iris".to_string()),
            }),
            intersystems_servers: Some(servers),
        };
        let conn = settings.to_iris_connection().await;
        assert!(conn.is_some());
        let c = conn.unwrap();
        assert!(c.base_url.contains("iris-host"));
        assert!(c.base_url.contains("52773"));
        assert_eq!(c.namespace, "PROD");
    }

    #[tokio::test]
    async fn to_iris_connection_named_server_with_path_prefix() {
        let mut servers = HashMap::new();
        servers.insert(
            "gw-iris".to_string(),
            IntersystemsServer {
                web_server: WebServerSpec {
                    host: Some("gateway-host".to_string()),
                    port: Some(80),
                    scheme: Some("http".to_string()),
                    path_prefix: Some("iriscore".to_string()),
                },
                super_server: None,
                username: None,
                password: Some("pw".to_string()),
                description: None,
            },
        );
        let settings = VsCodeSettings {
            objectscript_conn: Some(ObjectScriptConn {
                active: None,
                host: None,
                port: None,
                https: None,
                username: None,
                password: None,
                ns: Some("USER".to_string()),
                server: Some("gw-iris".to_string()),
            }),
            intersystems_servers: Some(servers),
        };
        let conn = settings.to_iris_connection().await;
        assert!(conn.is_some());
        let c = conn.unwrap();
        assert!(
            c.base_url.contains("iriscore"),
            "base_url should include path prefix: {}",
            c.base_url
        );
    }

    #[tokio::test]
    async fn to_iris_connection_named_server_missing_from_map_returns_none() {
        let mut servers = HashMap::new();
        servers.insert(
            "other-server".to_string(),
            IntersystemsServer {
                web_server: WebServerSpec {
                    host: Some("other".to_string()),
                    port: Some(52773),
                    scheme: None,
                    path_prefix: None,
                },
                super_server: None,
                username: None,
                password: None,
                description: None,
            },
        );
        let settings = VsCodeSettings {
            objectscript_conn: Some(ObjectScriptConn {
                active: None,
                host: None,
                port: None,
                https: None,
                username: None,
                password: None,
                ns: None,
                server: Some("nonexistent-server".to_string()),
            }),
            intersystems_servers: Some(servers),
        };
        let conn = settings.to_iris_connection().await;
        assert!(conn.is_none(), "missing server key must return None");
    }
    fn from_json(body: &str) -> VsCodeSettings {
        serde_json::from_str(body).expect("settings parse")
    }

    /// A named server with no `intersystems.servers` block names nothing to connect to.
    #[test]
    fn a_named_server_with_no_servers_block_gives_no_connection() {
        let s = from_json(r#"{"objectscript.conn":{"server":"dev"}}"#);
        assert!(s.connection_with_profiles(&[]).is_none());
    }

    /// A host that is not a bare host (it carries a path) is refused, whether it came from a named
    /// server or from the direct `objectscript.conn` fields. Nothing is guessed in its place.
    #[test]
    fn an_invalid_host_gives_no_connection_on_either_path() {
        let named = from_json(
            r#"{"objectscript.conn":{"server":"dev"},
                "intersystems.servers":{"dev":{"webServer":{"host":"a/b","port":52773}}}}"#,
        );
        assert!(named.connection_with_profiles(&[]).is_none());

        let direct = from_json(r#"{"objectscript.conn":{"host":"a/b","port":52773}}"#);
        assert!(direct.connection_with_profiles(&[]).is_none());

        let bad_scheme = from_json(
            r#"{"objectscript.conn":{"server":"dev"},
                "intersystems.servers":{"dev":{"webServer":{"host":"h","scheme":"ftp"}}}}"#,
        );
        assert!(bad_scheme.connection_with_profiles(&[]).is_none());
    }

    /// The direct path uses `https` when the extension's own `https` key is true.
    #[test]
    fn the_direct_path_follows_the_https_key() {
        let s = from_json(
            r#"{"objectscript.conn":{"host":"iris.example","port":443,"https":true,"password":"pw"}}"#,
        );
        let r = s.connection_with_profiles(&[]).expect("a connection");
        assert!(
            r.conn.base_url.starts_with("https://iris.example"),
            "{}",
            r.conn.base_url
        );
        assert!(r.set_aside.is_empty());
    }
}

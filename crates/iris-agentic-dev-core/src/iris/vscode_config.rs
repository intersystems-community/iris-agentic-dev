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

impl VsCodeSettings {
    /// Convert parsed settings to an IrisConnection, resolving named servers.
    pub async fn to_iris_connection(&self) -> Option<IrisConnection> {
        let conn = self.objectscript_conn.as_ref()?;
        if conn.active == Some(false) {
            return None;
        }

        // Named server path
        if let Some(server_name) = &conn.server {
            let servers = self.intersystems_servers.as_ref()?;
            let server = servers.get(server_name)?;
            let host = server.web_server.host.as_deref().unwrap_or("localhost");
            let web_port = server.web_server.port.unwrap_or(52773);
            let scheme = server.web_server.scheme.as_deref().unwrap_or("http");
            let path_prefix = server
                .web_server
                .path_prefix
                .as_deref()
                .unwrap_or("")
                .trim_matches('/');
            let base_url = if path_prefix.is_empty() {
                format!("{}://{}:{}", scheme, host, web_port)
            } else {
                format!("{}://{}:{}/{}", scheme, host, web_port, path_prefix)
            };
            let username = server.username.as_deref().unwrap_or("_SYSTEM");
            let password = server.password.as_deref().unwrap_or("SYS");
            let ns = conn.ns.as_deref().unwrap_or("USER");

            let iris_conn = IrisConnection::new(
                base_url,
                ns,
                username,
                password,
                DiscoverySource::VsCodeSettings,
            );
            // Note: super_server_port is available if needed for native connections
            return Some(iris_conn);
        }

        // Direct host/port path
        let host = conn.host.as_deref().unwrap_or("localhost");
        let port = conn.port.unwrap_or(52773);
        let username = conn.username.as_deref().unwrap_or("_SYSTEM");
        let password = conn.password.as_deref().unwrap_or("SYS");
        let ns = conn.ns.as_deref().unwrap_or("USER");
        let base_url = format!("http://{}:{}", host, port);

        Some(IrisConnection::new(
            base_url,
            ns,
            username,
            password,
            DiscoverySource::VsCodeSettings,
        ))
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
                password: None,
                description: None,
            },
        );
        let settings = VsCodeSettings {
            objectscript_conn: Some(ObjectScriptConn {
                active: None,
                host: None,
                port: None,
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
}

//! WebSocket terminal session pool for persistent IRIS terminal connections (072-b).
//!
//! Implements the IRIS Atelier WebSocket terminal protocol (v7+).
//! Sessions are persistent — variables and process state survive across `exec()` calls.
//!
//! Token format: `ws:{server_name}:{namespace}:{uuid}`

use std::collections::HashMap;
use std::sync::Arc;

use futures_util::{SinkExt, StreamExt};
use rmcp::ErrorData as McpError;
use serde_json::{json, Value};
use tokio::sync::Mutex;
use tokio_tungstenite::{connect_async, tungstenite::Message};

use crate::iris::connection::IrisConnection;

// ── Error codes ───────────────────────────────────────────────────────────────

pub const SESSION_STALE: &str = "SESSION_STALE";
pub const SESSION_WS_DISCONNECTED: &str = "SESSION_WS_DISCONNECTED";
pub const SESSION_WS_UNAVAILABLE: &str = "SESSION_WS_UNAVAILABLE";
pub const SESSION_TIMEOUT: &str = "SESSION_TIMEOUT";

/// Timeout waiting for a WS frame from IRIS.
const WS_FRAME_TIMEOUT_SECS: u64 = 30;

// ── Internal types ────────────────────────────────────────────────────────────

type WsSink = futures_util::stream::SplitSink<
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>,
    Message,
>;

type WsStream = futures_util::stream::SplitStream<
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>,
>;

struct WsSessionInner {
    #[allow(dead_code)]
    server_name: String,
    #[allow(dead_code)]
    namespace: String,
    #[allow(dead_code)]
    uuid: String,
    sink: WsSink,
    stream: WsStream,
}

// ── WsSessionPool ─────────────────────────────────────────────────────────────

/// Thread-safe pool of live WebSocket terminal sessions keyed by token.
pub struct WsSessionPool {
    sessions: Mutex<HashMap<String, WsSessionInner>>,
}

impl WsSessionPool {
    /// Create a new empty session pool.
    pub fn new() -> Self {
        Self {
            sessions: Mutex::new(HashMap::new()),
        }
    }

    /// Parse a session token into `(server_name, namespace, uuid)`.
    ///
    /// Returns `None` for tokens that don't match the `ws:{server}:{ns}:{uuid}` format,
    /// or where the uuid portion is empty.
    pub fn parse_token(token: &str) -> Option<(String, String, String)> {
        // Token: ws:{server_name}:{namespace}:{uuid}
        // Split on ':' with a limit of 4 parts so server/ns names containing ':' are rejected
        // (they are not valid IRIS server names or namespace names).
        let parts: Vec<&str> = token.splitn(4, ':').collect();
        if parts.len() != 4 {
            return None;
        }
        if parts[0] != "ws" {
            return None;
        }
        let server = parts[1].to_string();
        let namespace = parts[2].to_string();
        let uuid = parts[3].to_string();
        if server.is_empty() || namespace.is_empty() || uuid.is_empty() {
            return None;
        }
        Some((server, namespace, uuid))
    }

    /// Build a session token from components.
    pub fn make_token(server: &str, ns: &str, uuid: &str) -> String {
        format!("ws:{}:{}:{}", server, ns, uuid)
    }

    /// Open a new WebSocket terminal session against `conn`.
    ///
    /// Protocol:
    /// 1. GET `/api/atelier/` with Basic auth → collect CSP session cookies.
    /// 2. Connect WS to `/api/atelier/v7/%25SYS/terminal` with Cookie header.
    /// 3. Wait for `{"type":"init"}` frame.
    /// 4. Send `{"type":"config","namespace":"<ns>","rawMode":false}`.
    /// 5. Wait for `{"type":"prompt"}` frame.
    /// 6. Return token.
    pub async fn open(
        pool_ref: &Arc<Self>,
        conn: &IrisConnection,
        server_name: &str,
        namespace: &str,
    ) -> Result<String, McpError> {
        // Step 1: fetch CSP session cookies via Basic auth HTTP GET.
        let cookie_string = get_csp_session_cookie(conn).await?;

        // Step 2: build WebSocket URL — always targets %25SYS (URL-encoded %SYS).
        let base = conn.base_url.trim_end_matches('/');
        let ws_url = build_ws_url(base, "/api/atelier/v7/%25SYS/terminal");

        // Build the WebSocket request with Cookie and Authorization headers.
        use tokio_tungstenite::tungstenite::ClientRequestBuilder;
        let credentials = base64_basic_auth(&conn.username, &conn.password);
        let uri: tokio_tungstenite::tungstenite::http::Uri = ws_url
            .parse()
            .map_err(|e| McpError::internal_error(format!("invalid WS URL: {e}"), None))?;
        let request = ClientRequestBuilder::new(uri)
            .with_header("Cookie", &cookie_string)
            .with_header("Authorization", format!("Basic {}", credentials));

        let (ws_stream, _response) = connect_async(request).await.map_err(|e| {
            let msg = e.to_string();
            if msg.contains("404") || msg.contains("not found") {
                McpError::invalid_request(
                    format!(
                        "{}: {}",
                        SESSION_WS_UNAVAILABLE,
                        "IRIS Atelier API v7 required for WebSocket terminal (IRIS 2023.2+)"
                    ),
                    None,
                )
            } else {
                McpError::internal_error(
                    format!("{}: WebSocket connect failed: {e}", SESSION_WS_DISCONNECTED),
                    None,
                )
            }
        })?;

        let (mut sink, mut stream) = ws_stream.split();

        // Step 3: wait for {"type":"init"} from server.
        wait_for_type(&mut stream, "init").await?;

        // Step 4: send config frame.
        let config_msg = json!({
            "type": "config",
            "namespace": namespace,
            "rawMode": false,
        });
        sink.send(Message::Text(config_msg.to_string().into()))
            .await
            .map_err(|e| {
                McpError::internal_error(
                    format!("{}: send config failed: {e}", SESSION_WS_DISCONNECTED),
                    None,
                )
            })?;

        // Step 5: wait for {"type":"prompt"} from server.
        wait_for_type(&mut stream, "prompt").await?;

        // Step 6: generate UUID and build token.
        let uuid = uuid::Uuid::new_v4().to_string();
        let token = Self::make_token(server_name, namespace, &uuid);

        // Store session.
        let inner = WsSessionInner {
            server_name: server_name.to_string(),
            namespace: namespace.to_string(),
            uuid,
            sink,
            stream,
        };
        pool_ref.sessions.lock().await.insert(token.clone(), inner);

        Ok(token)
    }

    /// Execute ObjectScript code in a persistent session.
    ///
    /// Sends `{"type":"prompt","input":"<code>"}`, collects all `{"type":"output"}` frames
    /// (using the `"text"` field) until the next `{"type":"prompt"}`, and returns
    /// the concatenated output text.
    pub async fn exec(pool_ref: &Arc<Self>, token: &str, code: &str) -> Result<String, McpError> {
        let _parsed = Self::parse_token(token).ok_or_else(|| {
            McpError::invalid_request(format!("{}: invalid token format", SESSION_STALE), None)
        })?;

        let mut sessions = pool_ref.sessions.lock().await;
        let inner = sessions.get_mut(token).ok_or_else(|| {
            McpError::invalid_request(
                format!(
                    "{}: Session token references an unknown server or expired session",
                    SESSION_STALE
                ),
                None,
            )
        })?;

        // Send the prompt/input frame.
        let prompt_msg = json!({
            "type": "prompt",
            "input": code,
        });
        inner
            .sink
            .send(Message::Text(prompt_msg.to_string().into()))
            .await
            .map_err(|e| {
                McpError::internal_error(
                    format!("{}: send failed: {e}", SESSION_WS_DISCONNECTED),
                    None,
                )
            })?;

        // Collect output frames until the next prompt frame.
        let timeout_dur = std::time::Duration::from_secs(WS_FRAME_TIMEOUT_SECS);
        let mut output = String::new();

        loop {
            let frame = tokio::time::timeout(timeout_dur, inner.stream.next())
                .await
                .map_err(|_| {
                    McpError::internal_error(
                        format!("{}: No response from IRIS within timeout", SESSION_TIMEOUT),
                        None,
                    )
                })?;

            let msg = match frame {
                Some(Ok(m)) => m,
                Some(Err(e)) => {
                    return Err(McpError::internal_error(
                        format!("{}: connection error: {e}", SESSION_WS_DISCONNECTED),
                        None,
                    ))
                }
                None => {
                    return Err(McpError::internal_error(
                        format!("{}: connection closed by server", SESSION_WS_DISCONNECTED),
                        None,
                    ))
                }
            };

            let text: String = match msg {
                Message::Text(t) => t.to_string(),
                Message::Binary(b) => String::from_utf8_lossy(&b).into_owned(),
                Message::Close(_) => {
                    return Err(McpError::internal_error(
                        format!("{}: server closed connection", SESSION_WS_DISCONNECTED),
                        None,
                    ))
                }
                // Ping/Pong — ignore and continue.
                _ => continue,
            };

            let parsed: Value = serde_json::from_str(&text).unwrap_or(Value::Null);
            match parsed["type"].as_str() {
                Some("output") => {
                    if let Some(t) = parsed["text"].as_str() {
                        output.push_str(t);
                    }
                }
                Some("prompt") => {
                    // Prompt frame signals end of output for this exec.
                    break;
                }
                _ => {
                    // Other frame types (e.g. "error") — include any text if present.
                    if let Some(t) = parsed["text"].as_str() {
                        output.push_str(t);
                    }
                }
            }
        }

        Ok(output)
    }

    /// Close a WebSocket terminal session.
    ///
    /// Sends `{"type":"interrupt"}`, drops the connection, and removes the session
    /// from the pool.
    pub async fn close(pool_ref: &Arc<Self>, token: &str) -> Result<(), McpError> {
        let _parsed = Self::parse_token(token).ok_or_else(|| {
            McpError::invalid_request(format!("{}: invalid token format", SESSION_STALE), None)
        })?;

        let mut sessions = pool_ref.sessions.lock().await;
        let mut inner = sessions.remove(token).ok_or_else(|| {
            McpError::invalid_request(
                format!(
                    "{}: Session token references an unknown server or expired session",
                    SESSION_STALE
                ),
                None,
            )
        })?;

        // Best-effort: send interrupt and close.
        let interrupt = json!({"type": "interrupt"});
        let _ = inner
            .sink
            .send(Message::Text(interrupt.to_string().into()))
            .await;
        let _ = inner.sink.close().await;

        Ok(())
    }
}

impl Default for WsSessionPool {
    fn default() -> Self {
        Self::new()
    }
}

// ── Helpers ───────────────────────────────────────────────────────────────────

/// Fetch a CSP session cookie by issuing a GET `/api/atelier/` with Basic auth.
///
/// Collects ALL `Set-Cookie` response headers and assembles them into a single
/// `"k=v; k2=v2"` cookie string for use in the WS handshake.
async fn get_csp_session_cookie(conn: &IrisConnection) -> Result<String, McpError> {
    // Use a fresh client without cookie_store so we can see raw Set-Cookie headers.
    let client = reqwest::Client::builder()
        .danger_accept_invalid_certs(
            std::env::var("IRIS_INSECURE")
                .map(|v| v == "1" || v == "true")
                .unwrap_or(false),
        )
        .build()
        .map_err(|e| McpError::internal_error(format!("HTTP client build failed: {e}"), None))?;

    let url = format!("{}/api/atelier/", conn.base_url.trim_end_matches('/'));
    let resp = client
        .get(&url)
        .basic_auth(&conn.username, Some(&conn.password))
        .send()
        .await
        .map_err(|e| {
            McpError::internal_error(
                format!("{}: cookie fetch failed: {e}", SESSION_WS_DISCONNECTED),
                None,
            )
        })?;

    // Collect all Set-Cookie values into "k=v; k2=v2" format.
    let cookies: Vec<String> = resp
        .headers()
        .get_all(reqwest::header::SET_COOKIE)
        .iter()
        .filter_map(|v| v.to_str().ok())
        .map(|s| {
            // Each Set-Cookie header value is "name=value; Path=...; ..."
            // We only want the first segment (name=value).
            s.split(';').next().unwrap_or(s).trim().to_string()
        })
        .collect();

    Ok(cookies.join("; "))
}

/// Convert an HTTP base URL to a WebSocket URL (http → ws, https → wss).
fn build_ws_url(base: &str, path: &str) -> String {
    if let Some(rest) = base.strip_prefix("https://") {
        format!("wss://{}{}", rest, path)
    } else if let Some(rest) = base.strip_prefix("http://") {
        format!("ws://{}{}", rest, path)
    } else {
        format!("ws://{}{}", base, path)
    }
}

/// Base64-encode Basic auth credentials.
fn base64_basic_auth(username: &str, password: &str) -> String {
    use base64::Engine;
    let input = format!("{}:{}", username, password);
    base64::engine::general_purpose::STANDARD.encode(input)
}

/// Wait for a WS frame with the given `"type"` field, discarding others.
async fn wait_for_type(stream: &mut WsStream, expected_type: &str) -> Result<Value, McpError> {
    let timeout_dur = std::time::Duration::from_secs(WS_FRAME_TIMEOUT_SECS);
    loop {
        let frame = tokio::time::timeout(timeout_dur, stream.next())
            .await
            .map_err(|_| {
                McpError::internal_error(
                    format!(
                        "{}: No response from IRIS within timeout waiting for '{}'",
                        SESSION_TIMEOUT, expected_type
                    ),
                    None,
                )
            })?;

        let msg = match frame {
            Some(Ok(m)) => m,
            Some(Err(e)) => {
                return Err(McpError::internal_error(
                    format!(
                        "{}: WS error waiting for '{}': {e}",
                        SESSION_WS_DISCONNECTED, expected_type
                    ),
                    None,
                ))
            }
            None => {
                return Err(McpError::internal_error(
                    format!(
                        "{}: WS closed waiting for '{}'",
                        SESSION_WS_DISCONNECTED, expected_type
                    ),
                    None,
                ))
            }
        };

        let text: String = match msg {
            Message::Text(t) => t.to_string(),
            Message::Binary(b) => String::from_utf8_lossy(&b).into_owned(),
            Message::Close(_) => {
                return Err(McpError::internal_error(
                    format!(
                        "{}: server closed WS while waiting for '{}'",
                        SESSION_WS_DISCONNECTED, expected_type
                    ),
                    None,
                ))
            }
            _ => continue,
        };

        let parsed: Value = serde_json::from_str(&text).unwrap_or(Value::Null);
        if parsed["type"].as_str() == Some(expected_type) {
            return Ok(parsed);
        }
        // Discard unrecognised frames and keep waiting.
    }
}

// ── Unit tests ────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // T046: parse_token tests

    #[test]
    fn parse_token_valid() {
        let r = WsSessionPool::parse_token("ws:dev:USER:abc-123-def");
        assert_eq!(
            r,
            Some((
                "dev".to_string(),
                "USER".to_string(),
                "abc-123-def".to_string()
            ))
        );
    }

    #[test]
    fn parse_token_bad_prefix() {
        assert_eq!(WsSessionPool::parse_token("bad-token"), None);
    }

    #[test]
    fn parse_token_empty_uuid() {
        assert_eq!(WsSessionPool::parse_token("ws:dev:USER:"), None);
    }

    #[test]
    fn parse_token_roundtrip() {
        let token = WsSessionPool::make_token("myserver", "MYNS", "uuid-123");
        let parsed = WsSessionPool::parse_token(&token);
        assert_eq!(
            parsed,
            Some((
                "myserver".to_string(),
                "MYNS".to_string(),
                "uuid-123".to_string()
            ))
        );
    }

    #[test]
    fn parse_token_missing_parts() {
        assert_eq!(WsSessionPool::parse_token("ws:dev:USER"), None);
        assert_eq!(WsSessionPool::parse_token("ws:dev"), None);
        assert_eq!(WsSessionPool::parse_token("ws"), None);
        assert_eq!(WsSessionPool::parse_token(""), None);
    }

    #[test]
    fn parse_token_empty_server() {
        assert_eq!(WsSessionPool::parse_token("ws::USER:uuid-123"), None);
    }

    #[test]
    fn parse_token_empty_namespace() {
        assert_eq!(WsSessionPool::parse_token("ws:dev::uuid-123"), None);
    }

    #[test]
    fn make_token_format() {
        let t = WsSessionPool::make_token("srv", "NS", "123");
        assert!(t.starts_with("ws:srv:NS:123"), "token: {t}");
    }

    #[test]
    fn build_ws_url_http() {
        assert_eq!(
            build_ws_url("http://localhost:52780", "/api/atelier/v7/%25SYS/terminal"),
            "ws://localhost:52780/api/atelier/v7/%25SYS/terminal"
        );
    }

    #[test]
    fn build_ws_url_https() {
        assert_eq!(
            build_ws_url("https://myserver:443", "/api/atelier/v7/%25SYS/terminal"),
            "wss://myserver:443/api/atelier/v7/%25SYS/terminal"
        );
    }
}

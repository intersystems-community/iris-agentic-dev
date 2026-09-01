//! Server Manager connection discovery (044-servermanager-discovery).
//!
//! Reads IRIS server profiles from the VS Code Server Manager extension's
//! `intersystems.servers` key in VS Code's user `settings.json`, and resolves
//! credentials from the OS keychain using the same key format as Server Manager.

use serde::Deserialize;
use std::path::{Path, PathBuf};

#[cfg(target_os = "macos")]
use apple_native_keyring_store;
#[cfg(target_os = "windows")]
use windows_native_keyring_store;
#[cfg(all(
    unix,
    not(any(target_os = "macos", target_os = "ios", target_os = "android"))
))]
use zbus_secret_service_keyring_store;

// ── Types ────────────────────────────────────────────────────────────────────

/// A parsed Server Manager connection profile from VS Code settings.json.
#[derive(Debug, Clone)]
pub struct ServerManagerProfile {
    /// The map key name (e.g. `"dev-local"`).
    pub name: String,
    pub host: String,
    /// Defaults to 52773.
    pub port: u16,
    /// Defaults to `"http"`.
    pub scheme: String,
    pub path_prefix: Option<String>,
    /// Defaults to `"_SYSTEM"`.
    pub username: String,
    /// Deprecated `password` field from old Server Manager versions — usually absent.
    pub password_deprecated: Option<String>,
}

/// Error types for Server Manager credential resolution and server selection.
#[derive(Debug)]
pub enum SmCredentialError {
    /// Keychain lookup found no entry for this server / username combination.
    CredentialNotFound { server_name: String },
    /// Multiple servers configured and `IRIS_SERVER_NAME` not set (or names a missing server).
    Ambiguous { available: Vec<String> },
    /// Underlying keychain access error.
    KeychainError { server_name: String, detail: String },
    /// OS keychain is not available (no daemon, headless host, Remote SSH without keychain).
    /// Credential cannot be resolved until a keychain daemon is running or the connection is
    /// configured via `.iris-agentic-dev.toml`.
    KeychainUnavailable { server_name: String, detail: String },
}

impl std::fmt::Display for SmCredentialError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SmCredentialError::CredentialNotFound { server_name } => write!(
                f,
                "No credential found for Server Manager server '{server_name}'. \
                 On Windows: credentials are read from VS Code's state.vscdb — make sure \
                 iris-agentic-dev is v1.2.8 or later. \
                 On macOS/Linux: open VS Code → Server Manager → right-click the server → Reconnect."
            ),
            SmCredentialError::Ambiguous { available } => write!(
                f,
                "Multiple Server Manager servers configured: {}. \
                 Set IRIS_SERVER_NAME to one of these values.",
                available.join(", ")
            ),
            SmCredentialError::KeychainError {
                server_name,
                detail,
            } => write!(
                f,
                "Keychain access error for server '{server_name}': {detail}"
            ),
            SmCredentialError::KeychainUnavailable {
                server_name,
                detail,
            } => write!(
                f,
                "OS keychain is unavailable for server '{server_name}' ({detail}). \
                 On headless hosts and Remote SSH sessions the keychain daemon is often \
                 not accessible to out-of-process MCP clients. \
                 Workaround: add the connection to .iris-agentic-dev.toml with host/port/\
                 username/password — the file hot-reloads without restarting the server. \
                 credential_status will show 'keychain_unavailable' until resolved."
            ),
        }
    }
}

// ── Raw deserialization types ─────────────────────────────────────────────────

#[derive(Deserialize, Default)]
struct RawWebServer {
    host: Option<String>,
    port: Option<u16>,
    scheme: Option<String>,
    #[serde(rename = "pathPrefix")]
    path_prefix: Option<String>,
}

#[derive(Deserialize, Default)]
struct RawServerEntry {
    #[serde(rename = "webServer", default)]
    web_server: RawWebServer,
    username: Option<String>,
    password: Option<String>,
}

// ── settings.json parsing ─────────────────────────────────────────────────────

/// Return the platform-specific path to the VS Code user settings.json.
/// Returns `None` if the home directory cannot be determined.
pub fn sm_settings_path() -> Option<PathBuf> {
    let home = dirs::home_dir()?;
    #[cfg(target_os = "macos")]
    {
        Some(home.join("Library/Application Support/Code/User/settings.json"))
    }
    #[cfg(target_os = "windows")]
    {
        // %APPDATA%\Code\User\settings.json
        std::env::var("APPDATA")
            .ok()
            .map(|appdata| PathBuf::from(appdata).join("Code/User/settings.json"))
            .or_else(|| Some(home.join("AppData/Roaming/Code/User/settings.json")))
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        Some(home.join(".config/Code/User/settings.json"))
    }
}

/// Parse `intersystems.servers` from a VS Code `settings.json` file.
///
/// Returns an empty `Vec` if:
/// - The file does not exist.
/// - The file is not valid JSON.
/// - The `intersystems.servers` key is absent.
///
/// The `/default` key (which names the default server, not a server entry) is silently skipped.
/// Malformed individual server entries are silently skipped.
pub fn parse_sm_settings(path: &Path) -> Vec<ServerManagerProfile> {
    let contents = match std::fs::read_to_string(path) {
        Ok(c) => c,
        Err(e) => {
            tracing::debug!("SM settings not found at {}: {e}", path.display());
            return vec![];
        }
    };

    let root: serde_json::Value = match serde_json::from_str(&contents) {
        Ok(v) => v,
        Err(e) => {
            tracing::warn!("SM settings at {} is not valid JSON: {e}", path.display());
            return vec![];
        }
    };

    // VS Code stores settings as flat dotted keys ("intersystems.servers") or
    // sometimes as nested objects — handle both.
    let servers = match root
        .get("intersystems.servers")
        .and_then(|s| s.as_object())
        .or_else(|| {
            root.get("intersystems")
                .and_then(|i| i.get("servers"))
                .and_then(|s| s.as_object())
        }) {
        Some(m) => m,
        None => return vec![],
    };

    let mut profiles = Vec::new();
    for (key, value) in servers {
        // Skip the /default key (it's a string naming the default server, not a server entry)
        if key.starts_with('/') {
            continue;
        }

        let entry: RawServerEntry = match serde_json::from_value(value.clone()) {
            Ok(e) => e,
            Err(e) => {
                tracing::warn!("SM: could not parse server entry '{key}': {e}");
                continue;
            }
        };

        let host = match entry.web_server.host {
            Some(h) if !h.is_empty() => h,
            _ => {
                tracing::debug!("SM: server '{key}' has no host — skipping");
                continue;
            }
        };

        profiles.push(ServerManagerProfile {
            name: key.clone(),
            host,
            port: entry.web_server.port.unwrap_or(52773),
            scheme: entry
                .web_server
                .scheme
                .unwrap_or_else(|| "http".to_string()),
            path_prefix: entry.web_server.path_prefix,
            username: entry.username.unwrap_or_else(|| "_SYSTEM".to_string()),
            password_deprecated: entry.password,
        });
    }

    profiles
}

// ── Server selection ──────────────────────────────────────────────────────────

/// Select the active server profile from a list.
///
/// - If exactly one profile: auto-selects.
/// - If multiple profiles: requires `IRIS_SERVER_NAME` env var naming the server.
/// - If `IRIS_SERVER_NAME` is set but doesn't match any profile: returns `Ambiguous`.
/// - If no profiles: returns `Ambiguous` with empty list.
pub fn select_server(
    profiles: &[ServerManagerProfile],
) -> Result<&ServerManagerProfile, SmCredentialError> {
    match profiles.len() {
        0 => Err(SmCredentialError::Ambiguous { available: vec![] }),
        1 => Ok(&profiles[0]),
        _ => {
            let server_name = std::env::var("IRIS_SERVER_NAME").unwrap_or_default();
            if server_name.is_empty() {
                return Err(SmCredentialError::Ambiguous {
                    available: profiles.iter().map(|p| p.name.clone()).collect(),
                });
            }
            let server_name_lower = server_name.to_lowercase();
            match profiles
                .iter()
                .find(|p| p.name.to_lowercase() == server_name_lower)
            {
                Some(p) => Ok(p),
                None => Err(SmCredentialError::Ambiguous {
                    available: profiles.iter().map(|p| p.name.clone()).collect(),
                }),
            }
        }
    }
}

// ── Credential resolution ─────────────────────────────────────────────────────

/// Initialize the platform-specific OS keychain store.
///
/// Must be called once at application startup before any `resolve_credential` calls.
/// On headless Linux hosts (Remote SSH, CI, Docker without a running keychain daemon)
/// the store init may fail gracefully — the function never panics.
///
/// Tests bypass this by calling `keyring_core::set_default_store` directly with a mock store.
///
/// Background: `keyring` v4.1.3's `v1::Entry::new` has a bug — the `AtomicBool` guard
/// that is supposed to call `set_credential_store()` on the first invocation uses
/// `compare_exchange(false, true) == Ok(true)`, which can never be true (the exchange
/// returns `Ok(false)` on first success). We work around it by calling the platform
/// store constructors directly.
pub fn init_platform_keystore() {
    #[cfg(target_os = "macos")]
    {
        match apple_native_keyring_store::keychain::Store::new() {
            Ok(store) => keyring_core::set_default_store(store),
            Err(e) => tracing::warn!("macOS Keychain init failed: {e}"),
        }
    }
    #[cfg(target_os = "windows")]
    {
        match windows_native_keyring_store::Store::new() {
            Ok(store) => keyring_core::set_default_store(store),
            Err(e) => tracing::warn!("Windows Credential Manager init failed: {e}"),
        }
    }
    #[cfg(all(
        unix,
        not(any(target_os = "macos", target_os = "ios", target_os = "android"))
    ))]
    {
        match zbus_secret_service_keyring_store::Store::new() {
            Ok(store) => keyring_core::set_default_store(store),
            Err(e) => tracing::warn!(
                "Secret Service keychain init failed (headless host or no D-Bus session?): {e}. \
                 Credential storage unavailable — use .iris-agentic-dev.toml as fallback."
            ),
        }
    }
}

/// Keychain service name used by the InterSystems Server Manager VS Code extension
/// (`intersystems-community.servermanager`) to store IRIS server credentials.
///
/// The extension registers an authentication provider with ID `"intersystems-server-credentials"`.
/// VS Code's `SecretStorage` API stores secrets keyed by this auth provider ID — not the
/// application name. All VS Code-compatible forks (Cursor, Windsurf, VS Code Insiders,
/// VSCodium) that load the same extension share this service name; the fork identity
/// never appears in the credential path.
///
/// Confirmed from installed extension source:
///   `~/.vscode/extensions/intersystems-community.servermanager-3.12.3/dist/extension.js`
///   `AUTHENTICATION_PROVIDER = "intersystems-server-credentials"`
///
/// Platform note: macOS Keychain / Windows Credential Manager / Linux Secret Service —
/// the OS store varies but the service name string is always `"intersystems-server-credentials"`.
const SM_KEYCHAIN_SERVICE: &str = "intersystems-server-credentials";

/// Store a credential in the OS keychain using the Server Manager key format.
///
/// Uses service `SM_KEYCHAIN_SERVICE` = `"intersystems-server-credentials"` and
/// account `"credentialProvider:<server-name>/<username-lowercase>"`.
///
/// # Errors
/// Returns `SmCredentialError::KeychainError` on any failure.
pub fn store_credential(
    server_name: &str,
    username: &str,
    password: &str,
) -> Result<(), SmCredentialError> {
    let account = format!(
        "credentialProvider:{}/{}",
        server_name,
        username.to_lowercase()
    );

    let entry = keyring_core::Entry::new(SM_KEYCHAIN_SERVICE, &account)
        .map_err(|e: keyring_core::Error| map_keychain_error(server_name, e))?;

    entry
        .set_password(password)
        .map_err(|e: keyring_core::Error| map_keychain_error(server_name, e))?;

    Ok(())
}

fn map_keychain_error(server_name: &str, e: keyring_core::Error) -> SmCredentialError {
    match e {
        keyring_core::Error::NoDefaultStore => SmCredentialError::KeychainUnavailable {
            server_name: server_name.to_string(),
            detail: "no default keychain store".to_string(),
        },
        keyring_core::Error::NoStorageAccess(msg) => SmCredentialError::KeychainUnavailable {
            server_name: server_name.to_string(),
            detail: format!("keychain access denied: {msg}"),
        },
        other => SmCredentialError::KeychainError {
            server_name: server_name.to_string(),
            detail: other.to_string(),
        },
    }
}

/// Resolve a Server Manager credential from the OS keychain.
///
/// Uses service `SM_KEYCHAIN_SERVICE` = `"intersystems-server-credentials"` and
/// account `"credentialProvider:<server-name>/<username-lowercase>"`.
/// Uses `keyring_core::Entry` directly so tests can inject a mock store via
/// `keyring_core::set_default_store` without conflicting with the `keyring::v1` `Once` guard.
///
/// # Errors
/// Returns `SmCredentialError` on any failure — callers must surface this immediately
/// and NOT fall through to other discovery sources.
pub fn resolve_credential(server_name: &str, username: &str) -> Result<String, SmCredentialError> {
    let account = format!(
        "credentialProvider:{}/{}",
        server_name,
        username.to_lowercase()
    );

    // Use keyring_core::Entry directly so mock store injection works in tests.
    let entry = keyring_core::Entry::new(SM_KEYCHAIN_SERVICE, &account).map_err(
        |e: keyring_core::Error| SmCredentialError::KeychainError {
            server_name: server_name.to_string(),
            detail: e.to_string(),
        },
    )?;

    let wcm_result: Result<String, SmCredentialError> = match entry.get_password() {
        Ok(pw) => {
            tracing::debug!("SM credential resolved for '{server_name}' via WCM");
            return Ok(pw);
        }
        Err(keyring_core::Error::NoEntry) => Err(SmCredentialError::CredentialNotFound {
            server_name: server_name.to_string(),
        }),
        Err(keyring_core::Error::NoDefaultStore) => Err(SmCredentialError::KeychainUnavailable {
            server_name: server_name.to_string(),
            detail: "no default keychain store".to_string(),
        }),
        Err(keyring_core::Error::NoStorageAccess(msg)) => {
            Err(SmCredentialError::KeychainUnavailable {
                server_name: server_name.to_string(),
                detail: format!("keychain access denied: {msg}"),
            })
        }
        Err(e) => Err(SmCredentialError::KeychainError {
            server_name: server_name.to_string(),
            detail: e.to_string(),
        }),
    };

    // On Windows, Server Manager stores credentials in VS Code's state.vscdb
    // (safeStorage / AES-256-GCM), not in Windows Credential Manager. Try
    // state.vscdb whenever WCM has no entry or is unavailable.
    #[cfg(target_os = "windows")]
    {
        tracing::debug!("WCM lookup failed for '{server_name}', trying state.vscdb fallback");
        return resolve_vscode_secret(server_name, &account, None).map_err(|e| {
            SmCredentialError::KeychainError {
                server_name: server_name.to_string(),
                detail: e,
            }
        });
    }

    #[cfg(not(target_os = "windows"))]
    wcm_result
}

// ── Windows vscdb credential fallback ────────────────────────────────────────

/// Read a Server Manager credential from VS Code's `state.vscdb` on Windows.
///
/// This is the two-stage unseal: DPAPI on the Local State AES key, then
/// AES-256-GCM on the stored value. Called by `resolve_credential` when
/// Windows Credential Manager has no entry.
///
/// `db_path_override` is for testing — pass `None` in production.
#[cfg(target_os = "windows")]
pub fn resolve_vscode_secret(
    server_name: &str,
    account: &str,
    db_path_override: Option<&std::path::Path>,
) -> Result<String, String> {
    use crate::iris::vscode_payload::{
        decode_payload, decrypt_safe_storage, parse_local_state_key, DecodedPayload,
    };

    let db_path = match db_path_override {
        Some(p) => p.to_path_buf(),
        None => vscdb_state_db_path()?,
    };

    let secret_key = format!(
        r#"secret://{{"extensionId":"intersystems-community.servermanager","key":"{account}"}}"#
    );

    let (stored, _sqlite_type) = vscdb_read_secret(&db_path, &secret_key).map_err(|e| {
        // state.vscdb found but key missing — not an error worth surfacing loudly
        tracing::debug!("vscdb lookup failed for '{server_name}': {e}");
        e
    })?;

    match decode_payload(&stored)? {
        DecodedPayload::Dpapi(ciphertext) => {
            let plaintext = vscdb_dpapi_decrypt(&ciphertext, "the stored secret")?;
            String::from_utf8(plaintext).map_err(|e| format!("decrypted bytes are not UTF-8: {e}"))
        }
        DecodedPayload::SafeStorage(envelope) => {
            let local_state = vscdb_local_state_path(&db_path)?;
            let json = std::fs::read_to_string(&local_state)
                .map_err(|e| format!("cannot read {}: {e}", local_state.display()))?;
            let sealed_key = parse_local_state_key(&json)?;
            let aes_key = vscdb_dpapi_decrypt(&sealed_key, "the Local State AES key")?;
            decrypt_safe_storage(&envelope, &aes_key)
        }
    }
}

#[cfg(target_os = "windows")]
fn vscdb_state_db_path() -> Result<std::path::PathBuf, String> {
    let appdata = std::env::var("APPDATA").map_err(|_| "%APPDATA% not set".to_string())?;
    let path = std::path::PathBuf::from(&appdata)
        .join("Code")
        .join("User")
        .join("globalStorage")
        .join("state.vscdb");
    if path.exists() {
        return Ok(path);
    }
    let cursor = std::path::PathBuf::from(&appdata)
        .join("Cursor")
        .join("User")
        .join("globalStorage")
        .join("state.vscdb");
    if cursor.exists() {
        return Ok(cursor);
    }
    Err(format!(
        "state.vscdb not found at {} (also tried Cursor)",
        path.display()
    ))
}

#[cfg(target_os = "windows")]
fn vscdb_local_state_path(db_path: &std::path::Path) -> Result<std::path::PathBuf, String> {
    let install_root = db_path
        .parent() // globalStorage
        .and_then(|p| p.parent()) // User
        .and_then(|p| p.parent()) // Code
        .ok_or_else(|| {
            format!(
                "cannot locate Local State relative to {} — expected \
                 …\\Code\\User\\globalStorage\\state.vscdb",
                db_path.display()
            )
        })?;
    Ok(install_root.join("Local State"))
}

#[cfg(target_os = "windows")]
fn vscdb_read_secret(db_path: &std::path::Path, key: &str) -> Result<(Vec<u8>, String), String> {
    use rusqlite::OptionalExtension;
    let conn =
        rusqlite::Connection::open(db_path).map_err(|e| format!("cannot open state.vscdb: {e}"))?;
    let result: Option<(Vec<u8>, String)> = conn
        .query_row(
            "SELECT value, typeof(value) FROM ItemTable WHERE key = ?1",
            rusqlite::params![key],
            |row| {
                use rusqlite::types::ValueRef;
                let bytes = match row.get_ref(0)? {
                    ValueRef::Blob(b) => b.to_vec(),
                    ValueRef::Text(t) => t.to_vec(),
                    other => {
                        return Err(rusqlite::Error::InvalidColumnType(
                            0,
                            "value".to_string(),
                            other.data_type(),
                        ))
                    }
                };
                Ok((bytes, row.get::<_, String>(1)?))
            },
        )
        .optional()
        .map_err(|e| format!("query failed: {e}"))?;
    result.ok_or_else(|| {
        "key not found in state.vscdb — Server Manager credentials may not be stored yet"
            .to_string()
    })
}

#[cfg(target_os = "windows")]
fn current_windows_username() -> Option<String> {
    use windows::core::PWSTR;
    use windows::Win32::System::WindowsProgramming::GetUserNameW;
    let mut buf = vec![0u16; 257];
    let mut size = buf.len() as u32;
    let ok = unsafe { GetUserNameW(PWSTR(buf.as_mut_ptr()), &mut size) };
    if ok.is_err() {
        return None;
    }
    let len = size.saturating_sub(1) as usize;
    Some(String::from_utf16_lossy(&buf[..len]))
}

#[cfg(target_os = "windows")]
fn vscdb_dpapi_decrypt(ciphertext: &[u8], what: &str) -> Result<Vec<u8>, String> {
    use windows::Win32::Security::Cryptography::{CryptUnprotectData, CRYPT_INTEGER_BLOB};

    let mut input = CRYPT_INTEGER_BLOB {
        cbData: ciphertext.len() as u32,
        pbData: ciphertext.as_ptr() as *mut u8,
    };
    let mut output = CRYPT_INTEGER_BLOB {
        cbData: 0,
        pbData: std::ptr::null_mut(),
    };

    let ok = unsafe { CryptUnprotectData(&mut input, None, None, None, None, 0, &mut output) };

    if ok.is_err() {
        let err = std::io::Error::last_os_error();
        let current_user = current_windows_username().unwrap_or_else(|| "<unknown>".to_string());
        return Err(format!(
            "CryptUnprotectData failed on {what} ({} bytes): {err}.\n\
             This process is running as Windows user: {current_user}\n\
             DPAPI encrypts data to the user who wrote it. If this process runs as a \
             different user than VS Code, it cannot decrypt VS Code's stored secrets.\n\
             Fix: run VS Code and this tool as the same Windows user, or put the \
             password directly in .iris-agentic-dev.toml (see docs/connecting.md).",
            ciphertext.len()
        ));
    }

    let decrypted =
        unsafe { std::slice::from_raw_parts(output.pbData, output.cbData as usize).to_vec() };
    unsafe {
        windows::Win32::Foundation::LocalFree(windows::Win32::Foundation::HLOCAL(
            output.pbData as *mut _,
        ));
    }
    Ok(decrypted)
}

// ── check_config helpers ──────────────────────────────────────────────────────

/// Credential status / policy summary for a single server in check_config output.
pub struct ServerManagerCredentialEntry {
    pub server_name: String,
    /// `"resolved"`, `"not_configured"`, or `"error"`
    pub status: String,
    pub policy: Option<crate::iris::workspace_config::ConnectionPolicy>,
}

/// Credential status values
pub struct CredentialStatus;
impl CredentialStatus {
    pub const RESOLVED: &'static str = "resolved";
    pub const NOT_CONFIGURED: &'static str = "not_configured";
    pub const ERROR: &'static str = "error";
    /// OS keychain daemon not running or inaccessible (headless host, Remote SSH).
    /// The server definition is known but credentials cannot be read from the keychain.
    /// Use `.iris-agentic-dev.toml` with host/port/username/password as fallback.
    pub const KEYCHAIN_UNAVAILABLE: &'static str = "keychain_unavailable";
}

/// Build the `server_manager` section for `check_config` responses.
pub fn build_server_manager_config_json(
    profiles: &[ServerManagerProfile],
    active_server_name: Option<&str>,
    cred_entries: &[ServerManagerCredentialEntry],
) -> serde_json::Value {
    if profiles.is_empty() {
        return serde_json::json!({ "available": false });
    }

    let servers: Vec<serde_json::Value> = profiles
        .iter()
        .map(|p| {
            let cred = cred_entries.iter().find(|c| c.server_name == p.name);
            let cred_status = cred
                .map(|c| c.status.as_str())
                .unwrap_or(CredentialStatus::NOT_CONFIGURED);
            let active = active_server_name.map(|n| n == p.name).unwrap_or(false);
            let policy_json = cred
                .and_then(|c| c.policy.as_ref())
                .map(|pol| {
                    let template_str = pol.mcp_template.as_ref().map(|t| match t {
                        crate::iris::workspace_config::McpTemplate::Dev => "dev",
                        crate::iris::workspace_config::McpTemplate::Test => "test",
                        crate::iris::workspace_config::McpTemplate::Live => "live",
                    });
                    let data_policy_str = pol.data_policy.as_ref().map(|d| match d {
                        crate::iris::workspace_config::DataPolicy::Block => "block",
                        crate::iris::workspace_config::DataPolicy::Allow => "allow",
                        crate::iris::workspace_config::DataPolicy::Redact => "redact",
                    });
                    serde_json::json!({
                        "allow": pol.allow.as_ref().map(|cats| {
                            cats.iter().map(|c| c.as_str()).collect::<Vec<_>>()
                        }),
                        "mcp_template": template_str,
                        "data_policy": data_policy_str,
                    })
                })
                .unwrap_or(serde_json::Value::Null);

            serde_json::json!({
                "name": p.name,
                "host": p.host,
                "port": p.port,
                "active": active,
                "credential_status": cred_status,
                "policy": policy_json,
            })
        })
        .collect();

    serde_json::json!({
        "available": true,
        "servers": servers,
    })
}

// ── Policy gate ───────────────────────────────────────────────────────────────

/// Check whether a tool call is blocked by a per-connection policy.
///
/// Returns `Some(error_json)` when blocked, `None` when permitted.
/// Pure function — no I/O, no side effects.
/// Called before the role-gate in handler wiring.
pub fn policy_gate(
    tool_name: &str,
    server_name: &str,
    policy: Option<&crate::iris::workspace_config::ConnectionPolicy>,
) -> Option<serde_json::Value> {
    let policy = policy?;
    let allow = policy.allow.as_ref()?; // None = all permitted

    let category = tool_to_category(tool_name)?;
    if allow.contains(&category) {
        return None; // permitted
    }

    Some(serde_json::json!({
        "error_code": "POLICY_GATE",
        "policy_gate": true,
        "server_name": server_name,
        "blocked_category": category.as_str(),
        "allowed_categories": allow.iter().map(|c| c.as_str()).collect::<Vec<_>>(),
        "message": format!(
            "Tool '{}' is blocked by per-connection policy for server '{}'. \
             Category '{}' is not in the allowed list: [{}].",
            tool_name,
            server_name,
            category.as_str(),
            allow.iter().map(|c| c.as_str()).collect::<Vec<_>>().join(", ")
        ),
    }))
}

/// Map a tool name to its `ToolCategory`. Public for use by the policy gate layer.
pub fn tool_to_category_pub(
    tool_name: &str,
) -> Option<crate::iris::workspace_config::ToolCategory> {
    tool_to_category(tool_name)
}

/// Tool names deliberately exempt from `ToolCategory` — not an oversight.
///
/// `check_env_gate` and `policy_gate` both do `tool_to_category(tool_name)?`: a `None`
/// return means "not gated," not "blocked." Before 2026-08, that was true of every tool
/// nobody had gotten around to categorizing — 55 of the 90 real tools, discovered while
/// scoping a separate feature request. Two of those turned out to be live gaps in the
/// documented guarantee that `mcpTemplate = "live"`/`"test"` blocks `Execute`: an
/// uncategorized `iris_ws_exec` could run arbitrary ObjectScript over a WebSocket
/// terminal, and an uncategorized `iris_test`/`iris_coverage` could run test code,
/// completely bypassing the block that `iris_execute` itself already honored. Both are
/// categorized below now, along with the other 52.
///
/// `check_config` is the one tool that stays here on purpose: it makes zero IRIS calls
/// (it reports iad's own in-memory connection state), so there is no template or
/// per-connection policy it could possibly violate — forcing it into `Query` or `Admin`
/// would be less accurate than "not gated," not more.
///
/// `test_every_real_tool_has_a_category_or_is_exempt` (in
/// `tests/unit/test_tool_category_coverage.rs`) enforces that every tool in the real
/// registry is either mapped below or listed here — a new tool that is neither fails
/// that test immediately, so this list can't silently grow by omission again.
pub const INTENTIONALLY_UNCATEGORIZED_TOOLS: &[&str] = &["check_config"];

/// Map a tool name to its `ToolCategory`.
fn tool_to_category(tool_name: &str) -> Option<crate::iris::workspace_config::ToolCategory> {
    use crate::iris::workspace_config::ToolCategory;
    // Strip action suffix (e.g. "iris_source_control:commit" → "iris_source_control")
    let base = tool_name.split(':').next().unwrap_or(tool_name);
    Some(match base {
        "iris_compile" => ToolCategory::Compile,
        "iris_execute" => ToolCategory::Execute,
        "iris_query" => ToolCategory::Query,
        "iris_search" | "iris_symbols" | "iris_symbols_local" => ToolCategory::Search,
        "docs_introspect" | "iris_doc" => ToolCategory::Docs,
        "iris_source_control" => ToolCategory::SourceControl,
        "debug_capture_packet"
        | "debug_map_int_to_cls"
        | "debug_get_error_logs"
        | "debug_source_map"
        | "iris_debug" => ToolCategory::Debug,
        "iris_admin" | "iris_info" | "iris_containers" => ToolCategory::Admin,
        "skill_list" | "skill_describe" | "skill_search" | "skill_forget" | "skill_propose"
        | "skill_optimize" | "skill_share" | "agent_history" | "agent_stats"
        // 2026-08: agent_info and the skill/skill_community dispatchers (and their
        // Nostub-tier individual actions) join their already-categorized siblings above.
        | "agent_info" | "skill" | "skill_community" | "skill_community_install"
        | "skill_community_list" => ToolCategory::Skill,
        "kb_recall" | "kb_index"
        // 2026-08: the unified `kb` dispatcher joins its individual-action siblings.
        | "kb" => ToolCategory::Kb,
        // 052: get/list are Query; set/kill override to Execute in check_env_gate
        "iris_global" => ToolCategory::Query,
        // 053: iris_execute_method is Execute-gated (blocked on live/test templates)
        "iris_execute_method" => ToolCategory::Execute,
        // 056-interop-depth: all three are read-only
        "iris_message_body" | "iris_business_rule_info" | "iris_production_diff" => {
            ToolCategory::Query
        }
        // 059-tool-telemetry-benchmark: both read-only (query/export durable telemetry)
        "telemetry_query" | "telemetry_export_trace" => ToolCategory::Query,

        // ── 2026-08: the 54 tools below were uncategorized until now (see
        // INTENTIONALLY_UNCATEGORIZED_TOOLS above for the one deliberate exception,
        // check_config). Rule applied throughout: Execute/Compile for tools that run or
        // compile code (the two categories env_gate actually blocks on live/test, so
        // these are the ones worth getting right); Admin for tools that can create,
        // delete, or otherwise mutate server/namespace/database/credential/lookup/
        // production/container state; Query for read-only data/log/runtime-state
        // lookups; Docs for read-only code/schema/class-structure introspection. Where
        // a single tool mixes a safe default action with one risky one (e.g.
        // iris_lookup_manage's read actions vs. its destructive-gated set/delete;
        // iris_coverage's dry-run modes vs. its mode=run test execution), it gets the
        // riskier category rather than an per-action override — iris_global and
        // iris_query already show the alternative (see the action-aware overrides in
        // env_gate.rs) but that's more machinery than 54 tools' worth of nuance
        // currently justifies. Reclassify individually if one of these turns out wrong.

        // Executes code — the category env_gate actually blocks on live/test, so these
        // three matter most. iris_ws_exec runs arbitrary ObjectScript over a WebSocket
        // terminal (same risk as iris_execute, different transport); iris_test runs
        // %UnitTest suites; iris_coverage's mode=run does too (start→RunTest→stop→report)
        // even though its other modes (check/start/stop/report) are inert on their own.
        "iris_ws_exec" | "iris_test" | "iris_coverage" => ToolCategory::Execute,

        // Compiles/introduces new class code.
        "iris_generate_class" => ToolCategory::Compile,

        // Administrative mutation: server registry, namespaces, databases, credentials,
        // lookup tables, production topology/items, container lifecycle, destructive
        // global delete. All independently write/destructive-gated at the tool-impl
        // level already — this categorization is what makes a per-connection
        // `policy.<server>.allow` allowlist (e.g. `allow = ["query", "search"]` for a
        // "read-only browsing" connection) actually exclude them, which an uncategorized
        // tool could not be excluded from.
        "global_kill" | "iris_add_server" | "iris_remove_server" | "iris_import_servers"
        | "iris_namespace_create" | "iris_credential_manage" | "iris_lookup_manage"
        | "iris_lookup_transfer" | "iris_production" | "iris_production_item"
        | "iris_select_container" | "iris_start_sandbox" | "iris_ws_open" | "iris_ws_close" => {
            ToolCategory::Admin
        }

        // Read-only data, log, or runtime-state lookups — no code/schema structure, no
        // mutation.
        "capability_matrix" | "compare_document" | "compare_namespace" | "global_preview"
        | "hl7_schema_inspect" | "hl7_schema_list" | "iris_credential_list"
        | "iris_database_list" | "iris_database_stats" | "iris_get_log" | "iris_interop_query"
        | "iris_list_containers" | "iris_mirror_status" | "iris_namespace_list" | "iris_servers"
        | "iris_system_performance" | "iris_test_server" | "journal_search" | "mermaid_class"
        | "mermaid_production" | "my_access" | "query_audit_log" | "resolve_storage"
        | "stream_inspect" => ToolCategory::Query,

        // Read-only code/schema/class-structure introspection — same bucket as
        // docs_introspect/iris_doc above.
        "extract_message_map_routing" | "find_subclass_implementations" | "iris_doc_search"
        | "iris_generate" | "iris_generate_test" | "iris_macro" | "iris_table_info"
        | "resolve_dynamic_dispatch" => ToolCategory::Docs,

        _ => return None, // unknown tool — not gated
    })
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    static ENV_LOCK: Mutex<()> = Mutex::new(());

    fn make_profile(name: &str) -> ServerManagerProfile {
        ServerManagerProfile {
            name: name.to_string(),
            host: "localhost".to_string(),
            port: 52773,
            scheme: "http".to_string(),
            path_prefix: None,
            username: "_SYSTEM".to_string(),
            password_deprecated: None,
        }
    }

    #[test]
    fn select_server_empty_profiles_returns_ambiguous() {
        let result = select_server(&[]);
        assert!(matches!(result, Err(SmCredentialError::Ambiguous { .. })));
    }

    #[test]
    fn select_server_single_profile_returns_it() {
        let profiles = vec![make_profile("dev")];
        let p = select_server(&profiles).expect("single profile should be returned");
        assert_eq!(p.name, "dev");
    }

    #[test]
    fn select_server_multiple_no_env_returns_ambiguous() {
        let _guard = ENV_LOCK.lock().unwrap();
        std::env::remove_var("IRIS_SERVER_NAME");
        let profiles = vec![make_profile("dev"), make_profile("prod")];
        let result = select_server(&profiles);
        assert!(matches!(result, Err(SmCredentialError::Ambiguous { .. })));
    }

    #[test]
    fn select_server_multiple_env_matches() {
        let _guard = ENV_LOCK.lock().unwrap();
        std::env::set_var("IRIS_SERVER_NAME", "prod");
        let profiles = vec![make_profile("dev"), make_profile("prod")];
        let result = select_server(&profiles);
        std::env::remove_var("IRIS_SERVER_NAME");
        let p = result.expect("matching env var should return the profile");
        assert_eq!(p.name, "prod");
    }

    #[test]
    fn select_server_multiple_env_no_match_returns_ambiguous() {
        let _guard = ENV_LOCK.lock().unwrap();
        std::env::set_var("IRIS_SERVER_NAME", "nonexistent");
        let profiles = vec![make_profile("dev"), make_profile("prod")];
        let result = select_server(&profiles);
        std::env::remove_var("IRIS_SERVER_NAME");
        assert!(matches!(result, Err(SmCredentialError::Ambiguous { .. })));
    }

    /// Marked `#[ignore]` because it requires a live OS keychain (macOS Keychain,
    /// Windows Credential Manager, or a running Linux Secret Service daemon).
    /// Run with: `cargo test -- --ignored store_resolve_credential_roundtrip`
    #[test]
    #[ignore]
    fn store_resolve_credential_roundtrip() {
        init_platform_keystore();

        if store_credential("test-072-server", "_system", "test-pw-072").is_err() {
            eprintln!("store_resolve_credential_roundtrip: no keychain available, skipping");
            return;
        }

        let resolved = resolve_credential("test-072-server", "_system")
            .expect("resolve_credential should find the stored credential");

        assert_eq!(resolved, "test-pw-072");

        // Clean up: delete the entry from the keychain.
        let entry = keyring_core::Entry::new(
            SM_KEYCHAIN_SERVICE,
            "credentialProvider:test-072-server/_system",
        )
        .expect("keyring_core::Entry::new should succeed for cleanup");
        // keyring-core uses delete_credential().
        let _ = entry.delete_credential();
    }
}

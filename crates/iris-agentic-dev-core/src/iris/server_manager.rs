//! Server Manager connection discovery (044-servermanager-discovery).
//!
//! Reads IRIS server profiles from the VS Code Server Manager extension's
//! `intersystems.servers` key in VS Code's user `settings.json`, and resolves
//! credentials from the OS keychain using the same key format as Server Manager.

use serde::Deserialize;
use std::path::{Path, PathBuf};

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
}

impl std::fmt::Display for SmCredentialError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SmCredentialError::CredentialNotFound { server_name } => write!(
                f,
                "No credential found in OS keychain for Server Manager server '{server_name}'. \
                 Open VS Code → Server Manager → right-click the server → Reconnect."
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
/// No-op if the platform store is already initialized.
/// Tests bypass this by calling `keyring_core::set_default_store` directly.
pub fn init_platform_keystore() {
    // keyring::v1::Entry::new triggers Once-based platform store initialization.
    let _ = keyring::Entry::new("_init_", "_init_");
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

    match entry.get_password() {
        Ok(pw) => {
            tracing::debug!("SM credential resolved for '{server_name}'");
            Ok(pw)
        }
        Err(keyring_core::Error::NoEntry) => Err(SmCredentialError::CredentialNotFound {
            server_name: server_name.to_string(),
        }),
        Err(_) => {
            // NoStorageAccess covers headless Linux without a keychain daemon
            Err(SmCredentialError::CredentialNotFound {
                server_name: server_name.to_string(),
            })
        }
    }
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
        | "skill_optimize" | "skill_share" | "agent_history" | "agent_stats" => ToolCategory::Skill,
        "kb_recall" | "kb_index" => ToolCategory::Kb,
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
        _ => return None, // unknown tool — not gated
    })
}

//! Per-workspace IRIS connection config via `.iris-agentic-dev.toml`.
//!
//! Priority order: CLI flags > .iris-agentic-dev.toml > env vars > auto-discovery.

use crate::iris::connection::{DiscoverySource, IrisConnection};
use serde::Deserialize;
use std::path::PathBuf;

/// Parsed contents of `.iris-agentic-dev.toml`. All fields are optional.
#[derive(Debug, Deserialize, Default, Clone)]
pub struct WorkspaceConfig {
    pub container: Option<String>,
    pub namespace: Option<String>,
    pub host: Option<String>,
    #[serde(alias = "port")]
    pub web_port: Option<u16>,
    /// URL path prefix for the IRIS web gateway, e.g. "irisaicore" when the
    /// Atelier API is served at http://host:port/irisaicore/api/atelier/...
    /// Corresponds to intersystems.servers[x].webServer.pathPrefix in VS Code settings.
    pub web_prefix: Option<String>,
    /// URL scheme: "http" or "https". Defaults to "http".
    /// Set to "https" for TLS-protected IRIS web gateways.
    pub scheme: Option<String>,
    pub username: Option<String>,
    pub password: Option<String>,
    /// When true, skip HTTP/Atelier REST and use docker exec exclusively.
    /// Use for containers without a web server (e.g. community IRIS with no web gateway).
    /// Requires IRIS_CONTAINER to be set or container= in config.
    #[serde(default)]
    pub docker_only: bool,
    /// Tools to exclude from the MCP tool list at startup.
    /// Each entry is an exact tool name (e.g. "iris_source_control", "iris_admin").
    /// Useful for stripping tools irrelevant to your workflow to reduce noise.
    /// Takes effect regardless of toolset.
    #[serde(default)]
    pub disabled_tools: Vec<String>,
}

/// Connection role for fleet/operate mode instances.
#[derive(Debug, Deserialize, Clone, PartialEq, Default)]
#[serde(rename_all = "kebab-case")]
pub enum ConnectionRole {
    #[default]
    Workspace,
    Subject,
    ControlPlane,
}

/// Per-instance config block for `mode = "operate"` fleet configs.
#[derive(Debug, Deserialize, Default, Clone)]
#[serde(rename_all = "kebab-case")]
pub struct InstanceConfig {
    pub container: Option<String>,
    pub namespace: Option<String>,
    pub host: Option<String>,
    pub web_port: Option<u16>,
    pub username: Option<String>,
    pub password: Option<String>,
    #[serde(default)]
    pub role: ConnectionRole,
    pub memory_home: Option<String>,
    pub subject: Option<String>,
}

/// Environment template gate: controls which tool categories are available on a connection.
/// Default: `Dev` (all tools permitted).
#[derive(Debug, Deserialize, Clone, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase")]
pub enum McpTemplate {
    #[default]
    Dev,
    Test,
    Live,
}

/// PHI data policy: controls whether PHI-capable tools execute and how responses are handled.
/// Default: `Block` (PHI-capable tools blocked before any IRIS call).
#[derive(Debug, Deserialize, Clone, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase")]
pub enum DataPolicy {
    #[default]
    Block,
    Allow,
    /// PHI-capable tools permitted; PHI fields in responses replaced with `[REDACTED-PHI]`.
    /// HL7 v2 field-level redaction is a follow-on delivery.
    Redact,
}

/// Tool categories for per-connection policy gates (044-servermanager-discovery).
#[derive(Debug, Deserialize, Clone, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ToolCategory {
    Compile,
    Execute,
    Query,
    Search,
    Docs,
    SourceControl,
    Debug,
    Admin,
    Skill,
    Kb,
}

impl ToolCategory {
    pub fn as_str(&self) -> &'static str {
        match self {
            ToolCategory::Compile => "compile",
            ToolCategory::Execute => "execute",
            ToolCategory::Query => "query",
            ToolCategory::Search => "search",
            ToolCategory::Docs => "docs",
            ToolCategory::SourceControl => "source_control",
            ToolCategory::Debug => "debug",
            ToolCategory::Admin => "admin",
            ToolCategory::Skill => "skill",
            ToolCategory::Kb => "kb",
        }
    }
}

impl std::str::FromStr for ToolCategory {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "compile" => Ok(ToolCategory::Compile),
            "execute" => Ok(ToolCategory::Execute),
            "query" => Ok(ToolCategory::Query),
            "search" => Ok(ToolCategory::Search),
            "docs" => Ok(ToolCategory::Docs),
            "source_control" => Ok(ToolCategory::SourceControl),
            "debug" => Ok(ToolCategory::Debug),
            "admin" => Ok(ToolCategory::Admin),
            "skill" => Ok(ToolCategory::Skill),
            "kb" => Ok(ToolCategory::Kb),
            other => Err(format!("unknown tool category: '{other}'")),
        }
    }
}

fn deserialize_opt_mcp_template<'de, D>(de: D) -> Result<Option<McpTemplate>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    // Deserialize as a raw string first; return None for unknown variants instead of erroring.
    let s = match Option::<String>::deserialize(de)? {
        Some(s) => s,
        None => return Ok(None),
    };
    match s.as_str() {
        "dev" => Ok(Some(McpTemplate::Dev)),
        "test" => Ok(Some(McpTemplate::Test)),
        "live" => Ok(Some(McpTemplate::Live)),
        other => {
            tracing::warn!(value = other, "unknown mcpTemplate value — treating as Dev");
            Ok(None)
        }
    }
}

/// Raw TOML deserialization form of a policy block.
#[derive(Debug, Deserialize, Clone, Default)]
pub struct ConnectionPolicyRaw {
    pub allow: Option<Vec<ToolCategory>>,
    #[serde(
        rename = "mcpTemplate",
        deserialize_with = "deserialize_opt_mcp_template",
        default
    )]
    pub mcp_template: Option<McpTemplate>,
    #[serde(rename = "dataPolicy")]
    pub data_policy: Option<DataPolicy>,
    #[serde(rename = "globalBlocklist", default)]
    pub global_blocklist: Vec<String>,
    #[serde(rename = "dataPolicyKillAllowlist", default)]
    pub data_policy_kill_allowlist: Vec<String>,
}

/// Per-connection policy config from `[policy.<server-name>]` in `.iris-agentic-dev.toml`.
///
/// - `allow = None` → all categories permitted.
/// - `allow = Some([...])` → only listed categories permitted; all others blocked.
#[derive(Debug, Clone)]
pub struct ConnectionPolicy {
    /// The `[policy.<server-name>]` map key.
    pub server_name: String,
    /// Allowlist of permitted categories. `None` = all permitted.
    pub allow: Option<Vec<ToolCategory>>,
    /// Environment template gate. `None` → `Dev` (all tools permitted).
    pub mcp_template: Option<McpTemplate>,
    /// PHI data policy. `None` → `Block` (PHI-capable tools blocked by default).
    pub data_policy: Option<DataPolicy>,
    /// Per-connection additional global blocklist patterns (extends system blocklist).
    pub global_blocklist: Vec<String>,
    /// Patterns exempted from kill-operation blocklist check only.
    pub data_policy_kill_allowlist: Vec<String>,
}

/// Top-level fleet config. Wraps WorkspaceConfig for backward-compatible develop mode.
///
/// Parsing rule: load as FleetConfig.
/// - mode absent or "develop" → use `workspace` (flat fields); ignore `instance` map.
/// - mode = "operate" → use `instance` map; flat `workspace` fields are informational.
#[derive(Debug, Deserialize, Default, Clone)]
pub struct FleetConfig {
    pub mode: Option<String>,
    #[serde(default)]
    pub instance: std::collections::HashMap<String, InstanceConfig>,
    /// Per-connection policy blocks: `[policy.<server-name>]`
    #[serde(default)]
    pub policy: std::collections::HashMap<String, ConnectionPolicyRaw>,
    #[serde(flatten)]
    pub workspace: WorkspaceConfig,
    /// Resolved policies (populated after parsing).
    #[serde(skip)]
    pub policies: std::collections::HashMap<String, ConnectionPolicy>,
}

/// Resolve the workspace root path.
/// Priority: OBJECTSCRIPT_WORKSPACE env var > workspace_path arg > walk up from cwd.
///
/// When no explicit path is given, walks up from current_dir() looking for .iris-agentic-dev.toml
/// (git-style discovery). This ensures the config is found even when the MCP server is
/// launched from a parent directory (e.g. by an IDE that sets cwd to the home directory).
pub fn workspace_root(workspace_path: Option<&str>) -> PathBuf {
    if let Ok(ws) = std::env::var("OBJECTSCRIPT_WORKSPACE") {
        if !ws.is_empty() {
            return PathBuf::from(ws);
        }
    }
    if let Some(p) = workspace_path {
        if !p.is_empty() && p != "." {
            return PathBuf::from(p);
        }
    }
    // Walk up from current directory looking for .iris-agentic-dev.toml
    // (fall back to legacy .iris-dev.toml for backward compatibility)
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let mut dir = cwd.as_path();
    let mut legacy_root: Option<PathBuf> = None;
    loop {
        if dir.join(".iris-agentic-dev.toml").exists() {
            return dir.to_path_buf();
        }
        if legacy_root.is_none() && dir.join(".iris-dev.toml").exists() {
            legacy_root = Some(dir.to_path_buf());
        }
        match dir.parent() {
            Some(parent) => dir = parent,
            None => break,
        }
    }
    // If no .iris-agentic-dev.toml found but legacy .iris-dev.toml exists, use that dir.
    legacy_root.unwrap_or(cwd)
}

/// Load `.iris-agentic-dev.toml` from the resolved workspace root.
/// Returns `None` if the file does not exist (not an error).
/// Logs a warning and returns `None` on parse errors — never panics.
pub fn load_workspace_config(workspace_path: Option<&str>) -> Option<WorkspaceConfig> {
    let root = workspace_root(workspace_path);
    // Prefer new config name; fall back to legacy .iris-dev.toml for backward compatibility.
    let config_path = if root.join(".iris-agentic-dev.toml").exists() {
        root.join(".iris-agentic-dev.toml")
    } else if root.join(".iris-dev.toml").exists() {
        tracing::debug!(
            "Using legacy .iris-dev.toml (consider renaming to .iris-agentic-dev.toml)"
        );
        root.join(".iris-dev.toml")
    } else {
        return None;
    };

    match std::fs::read_to_string(&config_path) {
        Err(e) => {
            tracing::warn!(
                "Could not read .iris-agentic-dev.toml at {}: {}",
                config_path.display(),
                e
            );
            None
        }
        Ok(contents) => match toml::from_str::<WorkspaceConfig>(&contents) {
            Ok(cfg) => {
                tracing::debug!(
                    "Loaded .iris-agentic-dev.toml from {}",
                    config_path.display()
                );
                Some(cfg)
            }
            Err(e) => {
                tracing::warn!(
                    "Could not parse .iris-agentic-dev.toml at {}: {}",
                    config_path.display(),
                    e
                );
                None
            }
        },
    }
}

/// Load `.iris-agentic-dev.toml` as a `FleetConfig` (Amendment 001).
/// Returns `None` if the file does not exist or fails to parse (with a warning).
pub fn load_fleet_config(workspace_path: Option<&str>) -> Option<FleetConfig> {
    let root = workspace_root(workspace_path);
    let config_path = if root.join(".iris-agentic-dev.toml").exists() {
        root.join(".iris-agentic-dev.toml")
    } else if root.join(".iris-dev.toml").exists() {
        root.join(".iris-dev.toml")
    } else {
        return None;
    };

    match std::fs::read_to_string(&config_path) {
        Err(e) => {
            tracing::warn!("Could not read config at {}: {}", config_path.display(), e);
            None
        }
        Ok(contents) => match load_fleet_config_from_str(&contents) {
            Ok(cfg) => {
                tracing::debug!("Loaded fleet config from {}", config_path.display());
                Some(cfg)
            }
            Err(e) => {
                tracing::warn!("Could not parse config at {}: {}", config_path.display(), e);
                None
            }
        },
    }
}

/// Parse a `FleetConfig` from a TOML string.
/// Resolves `policy` blocks into the `policies` map post-parse.
pub fn load_fleet_config_from_str(contents: &str) -> Result<FleetConfig, toml::de::Error> {
    let mut cfg: FleetConfig = toml::from_str(contents)?;
    // Resolve raw policy blocks into ConnectionPolicy structs
    for (name, raw) in &cfg.policy {
        cfg.policies.insert(
            name.clone(),
            ConnectionPolicy {
                server_name: name.clone(),
                allow: raw.allow.clone(),
                mcp_template: raw.mcp_template.clone(),
                data_policy: raw.data_policy.clone(),
                global_blocklist: raw.global_blocklist.clone(),
                data_policy_kill_allowlist: raw.data_policy_kill_allowlist.clone(),
            },
        );
    }
    Ok(cfg)
}

/// Validate a FleetConfig: replace unknown `memory_home` keys with `"self"` (with a warning).
pub fn validate_fleet_config(cfg: &mut FleetConfig) {
    let known_keys: std::collections::HashSet<String> = cfg.instance.keys().cloned().collect();
    for (name, inst) in cfg.instance.iter_mut() {
        if let Some(ref mh) = inst.memory_home.clone() {
            if mh != "self" && !known_keys.contains(mh.as_str()) {
                tracing::warn!(
                    "instance '{}': memory-home '{}' is not a declared instance key — falling back to 'self'",
                    name, mh
                );
                inst.memory_home = Some("self".into());
            }
        }
    }
}

/// Role-gate check for subject instances (FR-019, FR-020).
///
/// Returns `Some(error_json)` when the call should be blocked, `None` when it should proceed.
///
/// - `role`: the connection role to check.
/// - `tool_name`: tool identifier, e.g. `"iris_compile"` or `"iris_source_control:commit"`.
///   For `iris_query`, pass `"iris_query:SELECT"` / `"iris_query:INSERT"` etc.
///   SELECT (and other read-only SQL) should be passed as `"iris_query:SELECT"` — not gated.
/// - `confirm`: whether the caller passed `confirm: true`.
/// - `instance_name`: the `[instance.*]` key (for the error message).
/// - `hard_block`: if `true`, no confirm path exists (used for source_control writes).
///
/// Pure function — no I/O, no side effects.
pub fn check_role_gate(
    role: &ConnectionRole,
    tool_name: &str,
    confirm: bool,
    instance_name: &str,
    hard_block: bool,
) -> Option<serde_json::Value> {
    // Workspace and ControlPlane are never gated
    if *role != ConnectionRole::Subject {
        return None;
    }

    // SELECT and other read-only queries are never gated
    if tool_name == "iris_query:SELECT"
        || tool_name == "iris_query:select"
        || tool_name.starts_with("iris_source_control:status")
        || tool_name.starts_with("iris_source_control:diff")
        || tool_name.starts_with("iris_source_control:log")
        || tool_name.starts_with("iris_source_control:list")
        || tool_name.starts_with("iris_source_control:get")
    {
        return None;
    }

    // Hard block: no confirm path (source_control writes).
    // confirm: true is silently accepted at the parameter level (shared field with soft-gate ops)
    // but has no effect here — confirm_ignored: true makes this explicit to agents.
    if hard_block {
        return Some(serde_json::json!({
            "error": "role_gate",
            "role_gate": true,
            "hard_block": true,
            "confirm_ignored": true,
            "instance": instance_name,
            "role": "subject",
            "message": format!(
                "Instance '{}' has role 'subject'. Source-control write operations are not permitted on subject instances. confirm: true has no effect on hard-blocked operations.",
                instance_name
            ),
        }));
    }

    // Soft gate: confirm=true bypasses
    if confirm {
        return None;
    }

    Some(serde_json::json!({
        "error": "role_gate",
        "role_gate": true,
        "instance": instance_name,
        "role": "subject",
        "required_confirmation": tool_name,
        "message": format!(
            "Instance '{}' has role 'subject'. Re-issue with confirm: true to proceed.",
            instance_name
        ),
    }))
}

/// Build the `workspace_config` JSON field for `iris_list_containers`.
///
/// - `mode = "operate"`: returns an object with `mode`, `instances` (each with role/memory_home/subject/running).
/// - develop mode or absent: returns the existing shape (found/path/container/namespace/running).
/// - no config file: returns `serde_json::Value::Null`.
pub fn build_workspace_config_json(
    workspace_path: Option<&str>,
    running_containers: &[serde_json::Value],
) -> serde_json::Value {
    let mut cfg = match load_fleet_config(workspace_path) {
        None => return serde_json::Value::Null,
        Some(c) => c,
    };
    validate_fleet_config(&mut cfg);

    let config_path = workspace_root(workspace_path)
        .join(".iris-agentic-dev.toml")
        .to_string_lossy()
        .to_string();

    if cfg.mode.as_deref() == Some("operate") {
        let mut instances = serde_json::Map::new();
        for (name, inst) in &cfg.instance {
            let memory_home = inst.memory_home.clone().unwrap_or_else(|| "self".into());
            let running = inst
                .container
                .as_deref()
                .map(|c| {
                    running_containers
                        .iter()
                        .any(|r| r["name"].as_str() == Some(c))
                })
                .unwrap_or(false);
            let role_str = match inst.role {
                ConnectionRole::Workspace => "workspace",
                ConnectionRole::Subject => "subject",
                ConnectionRole::ControlPlane => "control-plane",
            };
            instances.insert(
                name.clone(),
                serde_json::json!({
                    "role": role_str,
                    "memory_home": memory_home,
                    "subject": inst.subject,
                    "running": running,
                }),
            );
        }
        serde_json::json!({
            "found": true,
            "path": config_path,
            "mode": "operate",
            "instances": serde_json::Value::Object(instances),
        })
    } else {
        // Develop mode — return existing shape unchanged
        let container_name = cfg.workspace.container.as_deref().unwrap_or("");
        let running = !container_name.is_empty()
            && running_containers
                .iter()
                .any(|c| c["name"].as_str() == Some(container_name));
        serde_json::json!({
            "found": true,
            "path": config_path,
            "container": cfg.workspace.container,
            "namespace": cfg.workspace.namespace,
            "running": running,
        })
    }
}

/// Apply workspace config to set up the connection environment.
///
/// If `host` is specified: returns `Some(IrisConnection)` that will be passed directly
/// to `discover_iris()` as the explicit override.
///
/// If `container` is specified (but not host): sets `IRIS_CONTAINER` (and optionally
/// `IRIS_NAMESPACE`, `IRIS_USERNAME`, `IRIS_PASSWORD`) so the standard discovery cascade
/// picks up the container. Returns `None` to let discovery proceed normally.
///
/// If neither is specified: returns `None` — no connection info in the config.
pub fn workspace_config_to_connection(
    cfg: &WorkspaceConfig,
    namespace_default: &str,
) -> Option<IrisConnection> {
    // Export disabled_tools to env so IrisTools::with_registry_and_toolset picks them up.
    // Only set if the field is non-empty and env var not already set by user.
    if !cfg.disabled_tools.is_empty() && std::env::var("IRIS_DISABLED_TOOLS").is_err() {
        std::env::set_var("IRIS_DISABLED_TOOLS", cfg.disabled_tools.join(","));
    }

    // host + web_port → explicit HTTP/HTTPS connection (highest priority, no docker needed)
    if let Some(ref host) = cfg.host {
        let port = cfg.web_port.unwrap_or(52773);
        let scheme = cfg
            .scheme
            .clone()
            .or_else(|| std::env::var("IRIS_SCHEME").ok())
            .map(|s| s.trim_matches('/').to_string())
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| "http".to_string());
        let prefix = cfg
            .web_prefix
            .clone()
            .or_else(|| std::env::var("IRIS_WEB_PREFIX").ok())
            .map(|p| p.trim_matches('/').to_string())
            .filter(|p| !p.is_empty());
        let base_url = match prefix {
            Some(p) => format!("{}://{}:{}/{}", scheme, host, port, p),
            None => format!("{}://{}:{}", scheme, host, port),
        };
        let namespace = cfg
            .namespace
            .clone()
            .or_else(|| std::env::var("IRIS_NAMESPACE").ok())
            .unwrap_or_else(|| namespace_default.to_string());
        let username = cfg
            .username
            .clone()
            .or_else(|| std::env::var("IRIS_USERNAME").ok())
            .unwrap_or_else(|| "_SYSTEM".to_string());
        let password = cfg
            .password
            .clone()
            .or_else(|| std::env::var("IRIS_PASSWORD").ok())
            .unwrap_or_else(|| "SYS".to_string());
        // If container is also specified alongside host, update IRIS_CONTAINER so docker
        // exec tools (iris_execute fallback, iris_test, etc.) target the right container.
        if let Some(ref container) = cfg.container {
            std::env::set_var("IRIS_CONTAINER", container);
        }
        return Some(IrisConnection::new(
            base_url,
            namespace,
            username,
            password,
            DiscoverySource::EnvVar,
        ));
    }

    // container → inject into env so discover_iris() docker step picks it up
    if let Some(ref container) = cfg.container {
        std::env::set_var("IRIS_CONTAINER", container);
        let ns = cfg
            .namespace
            .clone()
            .or_else(|| std::env::var("IRIS_NAMESPACE").ok())
            .unwrap_or_else(|| namespace_default.to_string());
        let username = cfg
            .username
            .clone()
            .or_else(|| std::env::var("IRIS_USERNAME").ok())
            .unwrap_or_else(|| "_SYSTEM".to_string());
        let password = cfg
            .password
            .clone()
            .or_else(|| std::env::var("IRIS_PASSWORD").ok())
            .unwrap_or_else(|| "SYS".to_string());
        if let Some(ref ns_val) = cfg.namespace {
            std::env::set_var("IRIS_NAMESPACE", ns_val);
        }
        if let Some(ref user) = cfg.username {
            std::env::set_var("IRIS_USERNAME", user);
        }
        if let Some(ref pass) = cfg.password {
            std::env::set_var("IRIS_PASSWORD", pass);
        }
        if cfg.docker_only {
            // docker_only=true: skip HTTP entirely, use docker exec for all operations.
            // Return a connection with an unreachable URL — HTTP calls will fail fast,
            // triggering the docker exec fallback in iris_execute/iris_compile etc.
            return Some(IrisConnection::new(
                "http://127.0.0.1:1",
                ns,
                username,
                password,
                DiscoverySource::Docker {
                    container_name: container.clone(),
                },
            ));
        }
        return None; // discover_iris() will find the container via IRIS_CONTAINER
    }

    None
}

/// Apply workspace config to an existing explicit connection override.
///
/// If `explicit` is already set (from CLI flags), returns it unchanged.
/// Otherwise loads `.iris-agentic-dev.toml` from `workspace_path` and applies it:
/// - `host` config → returns `Some(IrisConnection)`
/// - `container` config → sets `IRIS_CONTAINER` env var, returns `None`
/// - no config / no relevant fields → returns `None`
pub fn apply_workspace_config(
    explicit: Option<IrisConnection>,
    workspace_path: Option<&str>,
    namespace: &str,
) -> Option<IrisConnection> {
    if explicit.is_some() {
        return explicit;
    }
    let cfg = load_workspace_config(workspace_path)?;
    explicit.or_else(|| workspace_config_to_connection(&cfg, namespace))
}

/// Generate starter `.iris-agentic-dev.toml` content with inline comments.
/// Used by `iris-dev init` and `check_config`.
pub fn generate_toml_content(container: &str, namespace: &str) -> String {
    format!(
        r#"# iris-agentic-dev workspace configuration
# Commit this file to share connection settings with your team.

# ── Native IRIS (no Docker) — Windows IIS or Linux Apache ──────────────────
# For IRIS installed directly on the host (not in Docker), uncomment and set:
# host = "localhost"
# web_port = 80       # IIS default (IRIS 2024.1+); use 52773 for pre-2024.1 Private Web Server
# web_prefix = ""     # URL path prefix, e.g. "iris" when Atelier is at /iris/api/atelier/
# namespace = "USER"
# scheme = "http"     # Use "https" for TLS-protected IRIS web gateways

# ── Docker container ────────────────────────────────────────────────────────
# For IRIS running in Docker, uncomment and set container name:
# NOTE: iris-agentic-dev requires the IRIS Atelier REST API. Three supported configurations:
#   1. Community images (iris-community, irishealth-community) — include private web server on port 52773
#   2. Enterprise + ISC Web Gateway container (intersystems/webgateway) — iris-agentic-dev auto-detects it
#   3. Enterprise standalone (intersystems/iris) — NOT supported, no Atelier REST available
#
# container = "{container}"

# Default IRIS namespace
namespace = "{namespace}"

# Credentials (optional)
# Use IRIS_USERNAME / IRIS_PASSWORD env vars instead of committing credentials.
# username = "_SYSTEM"
# password = "..."  # not recommended in committed files

# ── Tool filtering (optional) ────────────────────────────────────────────────
# Remove tools you don't use from the MCP tool list to reduce noise.
# disabled_tools = ["iris_source_control", "iris_admin", "iris_credential_manage"]
"#
    )
}

/// Generate starter `.iris-agentic-dev.toml` for `iris-dev init --mode operate`.
/// Produces one active `[instance.local]` block and a commented-out `[instance.subject]` block.
pub fn generate_operate_toml_content(local_container: &str, local_namespace: &str) -> String {
    format!(
        r#"# iris-agentic-dev fleet configuration (operate mode)
# Use this when managing multiple named IRIS instances.
# Commit this file to share connection settings with your team.
mode = "operate"

# ── Local / control-plane instance ──────────────────────────────────────────
[instance.local]
container = "{local_container}"
namespace = "{local_namespace}"
role = "workspace"            # "workspace" or "control-plane": no write gating

# ── Subject instance (e.g. a customer/production IRIS) ──────────────────────
# Uncomment and fill in to add a subject instance.
# [instance.subject]
# host = "subject-iris.example.com"
# web_port = 52773
# namespace = "USER"
# role = "subject"            # destructive ops require explicit confirm: true
# memory-home = "local"       # route AI memory writes to the 'local' instance
# subject = "MyCustomer"      # free-form label identifying this subject
"#,
        local_container = local_container,
        local_namespace = local_namespace,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use std::sync::Mutex;

    // Serialize tests that mutate global env vars to avoid parallel interference.
    static ENV_LOCK: Mutex<()> = Mutex::new(());

    // ── workspace_root tests ─────────────────────────────────────────────────

    #[test]
    fn workspace_root_env_var_overrides_all() {
        let _guard = ENV_LOCK.lock().unwrap();
        let dir = tempfile::tempdir().unwrap();
        std::env::set_var("OBJECTSCRIPT_WORKSPACE", dir.path().to_str().unwrap());
        let root = workspace_root(None);
        std::env::remove_var("OBJECTSCRIPT_WORKSPACE");
        assert_eq!(root, dir.path());
    }

    #[test]
    fn workspace_root_env_overrides_arg() {
        let _guard = ENV_LOCK.lock().unwrap();
        // When env is set, it takes priority over the arg
        let env_dir = tempfile::tempdir().unwrap();
        let arg_dir = tempfile::tempdir().unwrap();
        std::env::set_var("OBJECTSCRIPT_WORKSPACE", env_dir.path().to_str().unwrap());
        let root = workspace_root(Some(arg_dir.path().to_str().unwrap()));
        std::env::remove_var("OBJECTSCRIPT_WORKSPACE");
        assert_eq!(root, env_dir.path(), "env var must win over arg");
    }

    #[test]
    fn workspace_root_empty_arg_falls_through() {
        let _guard = ENV_LOCK.lock().unwrap();
        std::env::remove_var("OBJECTSCRIPT_WORKSPACE");
        let root = workspace_root(Some(""));
        // Falls through to cwd-walk — just check it doesn't panic and returns something
        assert!(root.is_absolute() || root.to_str() == Some("."));
    }

    #[test]
    fn workspace_root_dot_arg_falls_through() {
        let _guard = ENV_LOCK.lock().unwrap();
        std::env::remove_var("OBJECTSCRIPT_WORKSPACE");
        let root = workspace_root(Some("."));
        assert!(root.is_absolute() || root.to_str() == Some("."));
    }

    // ── load_workspace_config tests ──────────────────────────────────────────

    #[test]
    fn load_config_missing_file_returns_none() {
        let _guard = ENV_LOCK.lock().unwrap();
        std::env::remove_var("OBJECTSCRIPT_WORKSPACE");
        let dir = tempfile::tempdir().unwrap();
        std::env::set_var("OBJECTSCRIPT_WORKSPACE", dir.path().to_str().unwrap());
        let result = load_workspace_config(None);
        std::env::remove_var("OBJECTSCRIPT_WORKSPACE");
        assert!(result.is_none());
    }

    #[test]
    fn load_config_valid_toml_returns_some() {
        let _guard = ENV_LOCK.lock().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let mut f = std::fs::File::create(dir.path().join(".iris-agentic-dev.toml")).unwrap();
        writeln!(
            f,
            "host = \"localhost\"\nweb_port = 52773\nnamespace = \"USER\""
        )
        .unwrap();
        std::env::set_var("OBJECTSCRIPT_WORKSPACE", dir.path().to_str().unwrap());
        let result = load_workspace_config(None);
        std::env::remove_var("OBJECTSCRIPT_WORKSPACE");
        let cfg = result.expect("should load config");
        assert_eq!(cfg.host.as_deref(), Some("localhost"));
        assert_eq!(cfg.web_port, Some(52773));
    }

    #[test]
    fn load_config_port_alias_for_web_port() {
        let _guard = ENV_LOCK.lock().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let mut f = std::fs::File::create(dir.path().join(".iris-agentic-dev.toml")).unwrap();
        writeln!(
            f,
            "host = \"localhost\"\nport = 52774\nnamespace = \"USER\""
        )
        .unwrap();
        std::env::set_var("OBJECTSCRIPT_WORKSPACE", dir.path().to_str().unwrap());
        let result = load_workspace_config(None);
        std::env::remove_var("OBJECTSCRIPT_WORKSPACE");
        let cfg = result.expect("should load config");
        assert_eq!(
            cfg.web_port,
            Some(52774),
            "`port` should alias to `web_port`"
        );
    }

    #[test]
    fn load_config_invalid_toml_returns_none() {
        let _guard = ENV_LOCK.lock().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let mut f = std::fs::File::create(dir.path().join(".iris-agentic-dev.toml")).unwrap();
        writeln!(f, "this is not valid toml ={{{{").unwrap();
        std::env::set_var("OBJECTSCRIPT_WORKSPACE", dir.path().to_str().unwrap());
        let result = load_workspace_config(None);
        std::env::remove_var("OBJECTSCRIPT_WORKSPACE");
        assert!(result.is_none());
    }

    #[test]
    fn load_config_legacy_iris_dev_toml() {
        let _guard = ENV_LOCK.lock().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let mut f = std::fs::File::create(dir.path().join(".iris-dev.toml")).unwrap();
        writeln!(f, "host = \"myhost\"\nweb_port = 52773").unwrap();
        std::env::set_var("OBJECTSCRIPT_WORKSPACE", dir.path().to_str().unwrap());
        let result = load_workspace_config(None);
        std::env::remove_var("OBJECTSCRIPT_WORKSPACE");
        // Legacy file may or may not be picked up depending on priority; at minimum no panic
        let _ = result;
    }

    // ── workspace_config_to_connection tests ─────────────────────────────────

    #[test]
    fn config_with_host_returns_connection() {
        let cfg = WorkspaceConfig {
            host: Some("localhost".to_string()),
            web_port: Some(52773),
            namespace: Some("USER".to_string()),
            container: None,
            username: None,
            password: None,
            scheme: None,
            web_prefix: None,
            docker_only: false,
            disabled_tools: vec![],
        };
        let conn = workspace_config_to_connection(&cfg, "USER");
        assert!(conn.is_some(), "host config should produce connection");
    }

    #[test]
    fn config_with_container_only_returns_none_and_sets_env() {
        let _guard = ENV_LOCK.lock().unwrap();
        std::env::remove_var("IRIS_CONTAINER");
        let cfg = WorkspaceConfig {
            host: None,
            web_port: None,
            namespace: Some("MYNS".to_string()),
            container: Some("my-iris-container".to_string()),
            username: None,
            password: None,
            scheme: None,
            web_prefix: None,
            docker_only: false,
            disabled_tools: vec![],
        };
        let conn = workspace_config_to_connection(&cfg, "USER");
        let container_env = std::env::var("IRIS_CONTAINER").ok();
        std::env::remove_var("IRIS_CONTAINER");
        assert!(conn.is_none(), "container-only config should return None");
        assert_eq!(container_env.as_deref(), Some("my-iris-container"));
    }

    #[test]
    fn config_empty_returns_none() {
        let cfg = WorkspaceConfig {
            host: None,
            web_port: None,
            namespace: None,
            container: None,
            username: None,
            password: None,
            scheme: None,
            web_prefix: None,
            docker_only: false,
            disabled_tools: vec![],
        };
        let conn = workspace_config_to_connection(&cfg, "USER");
        assert!(conn.is_none());
    }

    #[test]
    fn workspace_root_arg_used_when_no_env() {
        let _guard = ENV_LOCK.lock().unwrap();
        std::env::remove_var("OBJECTSCRIPT_WORKSPACE");
        let dir = tempfile::tempdir().unwrap();
        let root = workspace_root(Some(dir.path().to_str().unwrap()));
        assert_eq!(root, dir.path());
    }

    #[test]
    fn workspace_root_legacy_toml_fallback() {
        let _guard = ENV_LOCK.lock().unwrap();
        std::env::remove_var("OBJECTSCRIPT_WORKSPACE");
        let dir = tempfile::tempdir().unwrap();
        std::fs::File::create(dir.path().join(".iris-dev.toml")).unwrap();
        std::env::set_var("OBJECTSCRIPT_WORKSPACE", dir.path().to_str().unwrap());
        // load_workspace_config will walk and find .iris-dev.toml
        let result = load_workspace_config(None);
        std::env::remove_var("OBJECTSCRIPT_WORKSPACE");
        // Legacy file loads fine (may be empty/default, doesn't error)
        let _ = result;
    }

    #[test]
    fn config_with_host_and_path_prefix_builds_prefixed_url() {
        let cfg = WorkspaceConfig {
            host: Some("my-host".to_string()),
            web_port: Some(80),
            namespace: Some("USER".to_string()),
            container: None,
            username: None,
            password: None,
            scheme: Some("http".to_string()),
            web_prefix: Some("iriscore".to_string()),
            docker_only: false,
            disabled_tools: vec![],
        };
        let conn = workspace_config_to_connection(&cfg, "USER").unwrap();
        assert!(
            conn.base_url.contains("iriscore"),
            "URL should include path prefix: {}",
            conn.base_url
        );
    }

    #[test]
    fn config_with_host_and_container_sets_iris_container_env() {
        let _guard = ENV_LOCK.lock().unwrap();
        std::env::remove_var("IRIS_CONTAINER");
        let cfg = WorkspaceConfig {
            host: Some("localhost".to_string()),
            web_port: Some(52773),
            namespace: None,
            container: Some("my-iris-container".to_string()),
            username: None,
            password: None,
            scheme: None,
            web_prefix: None,
            docker_only: false,
            disabled_tools: vec![],
        };
        let conn = workspace_config_to_connection(&cfg, "USER");
        let container_env = std::env::var("IRIS_CONTAINER").ok();
        std::env::remove_var("IRIS_CONTAINER");
        assert!(conn.is_some(), "host present → connection returned");
        assert_eq!(
            container_env.as_deref(),
            Some("my-iris-container"),
            "IRIS_CONTAINER must be set when container+host both specified"
        );
    }

    #[test]
    fn config_container_with_credentials_sets_env_vars() {
        let _guard = ENV_LOCK.lock().unwrap();
        std::env::remove_var("IRIS_CONTAINER");
        std::env::remove_var("IRIS_USERNAME");
        std::env::remove_var("IRIS_PASSWORD");
        std::env::remove_var("IRIS_NAMESPACE");
        let cfg = WorkspaceConfig {
            host: None,
            web_port: None,
            namespace: Some("MYNAMESPACE".to_string()),
            container: Some("cred-container".to_string()),
            username: Some("myuser".to_string()),
            password: Some("mypass".to_string()),
            scheme: None,
            web_prefix: None,
            docker_only: false,
            disabled_tools: vec![],
        };
        let conn = workspace_config_to_connection(&cfg, "USER");
        let ns_env = std::env::var("IRIS_NAMESPACE").ok();
        let user_env = std::env::var("IRIS_USERNAME").ok();
        let pass_env = std::env::var("IRIS_PASSWORD").ok();
        std::env::remove_var("IRIS_CONTAINER");
        std::env::remove_var("IRIS_USERNAME");
        std::env::remove_var("IRIS_PASSWORD");
        std::env::remove_var("IRIS_NAMESPACE");
        assert!(conn.is_none(), "container-only → no connection");
        assert_eq!(ns_env.as_deref(), Some("MYNAMESPACE"));
        assert_eq!(user_env.as_deref(), Some("myuser"));
        assert_eq!(pass_env.as_deref(), Some("mypass"));
    }

    #[test]
    fn config_docker_only_returns_unreachable_connection() {
        let _guard = ENV_LOCK.lock().unwrap();
        let cfg = WorkspaceConfig {
            host: None,
            web_port: None,
            namespace: Some("USER".to_string()),
            container: Some("docker-only-iris".to_string()),
            username: None,
            password: None,
            scheme: None,
            web_prefix: None,
            docker_only: true,
            disabled_tools: vec![],
        };
        let conn = workspace_config_to_connection(&cfg, "USER");
        std::env::remove_var("IRIS_CONTAINER");
        assert!(
            conn.is_some(),
            "docker_only=true should return a connection (with unreachable URL)"
        );
        let c = conn.unwrap();
        assert!(
            c.base_url.contains("127.0.0.1:1"),
            "docker_only URL must be unreachable: {}",
            c.base_url
        );
    }

    #[test]
    fn apply_workspace_config_returns_explicit_unchanged() {
        use crate::iris::connection::DiscoverySource;
        let explicit = IrisConnection::new(
            "http://explicit-host:52773",
            "EXPLICIT",
            "_SYSTEM",
            "SYS",
            DiscoverySource::EnvVar,
        );
        let result = apply_workspace_config(Some(explicit.clone()), None, "USER");
        assert!(result.is_some());
        assert_eq!(result.unwrap().base_url, explicit.base_url);
    }

    #[test]
    fn apply_workspace_config_loads_config_when_no_explicit() {
        let _guard = ENV_LOCK.lock().unwrap();
        std::env::remove_var("OBJECTSCRIPT_WORKSPACE");
        let dir = tempfile::tempdir().unwrap();
        let mut f = std::fs::File::create(dir.path().join(".iris-agentic-dev.toml")).unwrap();
        use std::io::Write;
        writeln!(f, "host = \"config-host\"\nweb_port = 52773").unwrap();
        std::env::set_var("OBJECTSCRIPT_WORKSPACE", dir.path().to_str().unwrap());
        let result = apply_workspace_config(None, None, "USER");
        std::env::remove_var("OBJECTSCRIPT_WORKSPACE");
        assert!(result.is_some(), "should load config and build connection");
        let c = result.unwrap();
        assert!(c.base_url.contains("config-host"));
    }

    // ── generate_toml_content tests ──────────────────────────────────────────

    #[test]
    fn toml_template_native_section_before_container() {
        let content = generate_toml_content("my-iris", "USER");
        let native_pos = content
            .find("Native IRIS")
            .expect("template must contain 'Native IRIS' section header");
        let container_pos = content
            .find("Docker container")
            .expect("template must contain 'Docker container' section header");
        assert!(
            native_pos < container_pos,
            "Native IRIS section must appear before Docker container section"
        );
    }

    #[test]
    fn toml_template_has_port_80_comment() {
        let content = generate_toml_content("my-iris", "USER");
        assert!(
            content.contains("web_port = 80"),
            "template must document port 80 as IIS default"
        );
    }

    #[test]
    fn toml_template_both_sections_commented_by_default() {
        let content = generate_toml_content("my-iris", "USER");
        // Neither host nor container should be active (uncommented) assignments
        for line in content.lines() {
            let trimmed = line.trim();
            if trimmed.starts_with('#') || trimmed.is_empty() {
                continue;
            }
            assert!(
                !trimmed.starts_with("host =") && !trimmed.starts_with("container ="),
                "host and container must be commented out in default template, found: {trimmed}"
            );
        }
    }
}

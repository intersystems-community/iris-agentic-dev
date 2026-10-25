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
    /// Validate the gateway's TLS certificate. Defaults to `true` (absent = validate).
    ///
    /// Set `false` for a self-signed cert that is not in the OS trust store. A cert from a
    /// locally-installed CA (mkcert and friends) needs nothing here — the HTTP client reads the
    /// OS trust store, so those verify normally.
    ///
    /// Applies to this file's own connection only, and `false` counts only from the operator's file
    /// or a project file the operator trusted (spec 141). It is not exported to the environment.
    /// `None` declares nothing: certificates are checked, unless the file names the server the
    /// launch settings name and `IRIS_TLS_VERIFY` says otherwise for it (#127).
    pub tls_verify: Option<bool>,
    pub username: Option<String>,
    pub password: Option<String>,
    /// When true, skip HTTP/Atelier REST and use docker exec exclusively.
    /// Use for containers without a web server (e.g. community IRIS with no web gateway).
    /// Requires IRIS_CONTAINER to be set or container= in config.
    #[serde(default)]
    pub docker_only: bool,
    /// Declare that this IRIS instance has no embedded web server (AI-branch builds,
    /// irishealth-ai:*, 2026.3+). Routes iris_execute and iris_compile to docker exec
    /// immediately without attempting Atelier REST. When the web port IS reachable
    /// (webgateway sidecar scenario), use host= instead and leave nopws=false.
    #[serde(default)]
    pub nopws: bool,
    /// Route docker exec commands through SSH to a remote Docker host.
    /// When set, every docker exec call runs through
    /// `ssh -o BatchMode=yes -o StrictHostKeyChecking=accept-new <ssh_host>`.
    /// Requires `container` to be set. See docs/connecting.md for security notes.
    pub ssh_host: Option<String>,
    /// Require a known host key for `ssh_host` (`StrictHostKeyChecking=yes`). By default a host
    /// seen for the first time is recorded (`accept-new`) and a changed key is refused. Setting
    /// this only tightens, so any file may set it.
    #[serde(default)]
    pub ssh_strict: bool,
    /// Tools to exclude from the MCP tool list at startup.
    /// Each entry is an exact tool name (e.g. "iris_source_control", "iris_admin").
    /// Useful for stripping tools irrelevant to your workflow to reduce noise.
    /// Takes effect regardless of toolset. Always wins over `enabled_tools` for any
    /// name in both.
    #[serde(default)]
    pub disabled_tools: Vec<String>,
    /// Allowlist: when non-empty, ONLY these tools remain in the MCP tool list at
    /// startup — everything else is pruned, regardless of toolset (075-modular-tool-
    /// install, FR-001). An empty list (the default) means "no allowlist" — the active
    /// `Toolset` preset applies as usual — not "expose zero tools." `disabled_tools`
    /// still wins for any name present in both (FR-002); a name here that doesn't
    /// match any real tool is silently ignored, same as `disabled_tools` today.
    #[serde(default)]
    pub enabled_tools: Vec<String>,
    /// Override the write-tools gate. When `false`, all tools that mutate IRIS state
    /// (iris_doc put/delete, iris_compile, etc.) are removed from the tool list.
    /// When unset, the gate is derived from SystemMode / namespace heuristics.
    pub write_tools_enabled: Option<bool>,
    /// When `true`, the destructive-tools gate is unlocked (tools that can delete data,
    /// drop globals, etc.). Requires `write_tools_enabled = true` — setting this `true`
    /// while `write_tools_enabled = false` is refused at startup with DESTRUCTIVE_REQUIRES_WRITES.
    pub destructive_tools_enabled: Option<bool>,
}

/// Connection role for fleet/operate mode instances.
///
/// A **workbench** is a server the agent builds on; a **subject** is a server under study, where
/// source-control writes are refused and other writes need confirmation. Parsing is lenient and
/// fails closed ([`parse_role`]): `workspace` is the pre-1.7 spelling of `workbench`,
/// `control-plane` was never gated and so reads as `workbench`, and any other value is `subject`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ConnectionRole {
    #[default]
    Workbench,
    Subject,
}

impl ConnectionRole {
    pub fn as_str(&self) -> &'static str {
        match self {
            ConnectionRole::Workbench => "workbench",
            ConnectionRole::Subject => "subject",
        }
    }
}

/// Read a role string. The second value is a note for `config_errors` when the spelling is
/// deprecated or not understood.
pub fn parse_role(raw: &str) -> (ConnectionRole, Option<String>) {
    match raw.trim().to_ascii_lowercase().as_str() {
        "workbench" => (ConnectionRole::Workbench, None),
        "subject" => (ConnectionRole::Subject, None),
        "workspace" => (
            ConnectionRole::Workbench,
            Some(
                "role \"workspace\" is now \"workbench\"; \"workspace\" is accepted through 1.7.x"
                    .to_string(),
            ),
        ),
        "control-plane" => (
            ConnectionRole::Workbench,
            Some(
                "role \"control-plane\" is not a role; it is treated as \"workbench\"".to_string(),
            ),
        ),
        other => (
            ConnectionRole::Subject,
            Some(format!(
                "role \"{other}\" is not a role; it is treated as \"subject\""
            )),
        ),
    }
}

impl<'de> Deserialize<'de> for ConnectionRole {
    fn deserialize<D: serde::Deserializer<'de>>(de: D) -> Result<Self, D::Error> {
        let raw = String::deserialize(de)?;
        Ok(parse_role(&raw).0)
    }
}

/// Per-instance config block for `mode = "operate"` fleet configs.
///
/// Connection fields match top-level `WorkspaceConfig` (`web_port`, `scheme`,
/// `web_prefix`) so SKILL.md / `iris-agentic-dev init --mode operate` snippets
/// work. `web-port`, `web-prefix`, and `memory-home` remain accepted aliases.
#[derive(Debug, Deserialize, Default, Clone)]
pub struct InstanceConfig {
    pub container: Option<String>,
    pub namespace: Option<String>,
    pub host: Option<String>,
    #[serde(alias = "port", alias = "web-port")]
    pub web_port: Option<u16>,
    /// URL path prefix for the IRIS web gateway (same meaning as top-level `web_prefix`).
    #[serde(alias = "web-prefix")]
    pub web_prefix: Option<String>,
    /// `"http"` or `"https"`. Defaults to `"http"` when building the pool URL.
    pub scheme: Option<String>,
    pub username: Option<String>,
    pub password: Option<String>,
    /// The restricted account the agent's code runs as on this server. Honoured from the operator
    /// file or a trusted workspace file only.
    pub service_username: Option<String>,
    pub service_password: Option<String>,
    #[serde(default)]
    pub role: ConnectionRole,
    #[serde(alias = "memory-home")]
    pub memory_home: Option<String>,
    pub subject: Option<String>,
    /// Check this instance's certificate? `false` turns it off for this instance only, and counts
    /// only from the operator's file or a project file the operator trusted.
    pub tls_verify: Option<bool>,
}

/// Where a fleet `[instance.*]` block is.
///
/// Defaults: host `localhost`, port `52773`, scheme `http`, no path prefix. A scheme that is not
/// `http` or `https` is an error.
pub fn instance_endpoint(
    inst: &InstanceConfig,
) -> Result<crate::iris::endpoint::Endpoint, crate::iris::endpoint::InvalidEndpoint> {
    crate::iris::endpoint::Endpoint::parse(
        inst.scheme.as_deref(),
        inst.host.as_deref().unwrap_or("localhost"),
        inst.web_port.unwrap_or(52773),
        inst.web_prefix.as_deref(),
    )
}

/// The Atelier base URL for a fleet `[instance.*]` block; the unreachable sentinel when the block
/// has no valid endpoint (see [`instance_endpoint`]).
pub fn instance_base_url(inst: &InstanceConfig) -> String {
    instance_endpoint(inst)
        .map(|e| e.base_url())
        .unwrap_or_else(|_| crate::iris::connection_pool::UNREACHABLE_BASE_URL.to_string())
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

/// Lenient and fail-closed (R1 D11): an unknown template reads as `live`, the strictest. The
/// notice for the operator is added by [`crate::iris::config_sources::value_notes`].
fn deserialize_opt_mcp_template<'de, D>(de: D) -> Result<Option<McpTemplate>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let s = match Option::<String>::deserialize(de)? {
        Some(s) => s,
        None => return Ok(None),
    };
    Ok(Some(match s.trim().to_ascii_lowercase().as_str() {
        "dev" => McpTemplate::Dev,
        "test" => McpTemplate::Test,
        _ => McpTemplate::Live,
    }))
}

/// Case-insensitive; an unknown value reads as `block`, the strictest.
fn deserialize_opt_data_policy<'de, D>(de: D) -> Result<Option<DataPolicy>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let s = match Option::<String>::deserialize(de)? {
        Some(s) => s,
        None => return Ok(None),
    };
    Ok(Some(match s.trim().to_ascii_lowercase().as_str() {
        "allow" => DataPolicy::Allow,
        "redact" => DataPolicy::Redact,
        _ => DataPolicy::Block,
    }))
}

/// Case-insensitive; a category iad does not know is dropped, which narrows the list.
fn deserialize_opt_allow<'de, D>(de: D) -> Result<Option<Vec<ToolCategory>>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let Some(items) = Option::<Vec<String>>::deserialize(de)? else {
        return Ok(None);
    };
    let mut out: Vec<ToolCategory> = Vec::new();
    for s in items {
        if let Ok(c) = s.trim().to_ascii_lowercase().parse::<ToolCategory>() {
            if !out.contains(&c) {
                out.push(c);
            }
        }
    }
    Ok(Some(out))
}

/// Raw TOML deserialization form of a policy block.
#[derive(Debug, Deserialize, Clone, Default)]
pub struct ConnectionPolicyRaw {
    #[serde(deserialize_with = "deserialize_opt_allow", default)]
    pub allow: Option<Vec<ToolCategory>>,
    #[serde(
        rename = "mcpTemplate",
        deserialize_with = "deserialize_opt_mcp_template",
        default
    )]
    pub mcp_template: Option<McpTemplate>,
    #[serde(
        rename = "dataPolicy",
        deserialize_with = "deserialize_opt_data_policy",
        default
    )]
    pub data_policy: Option<DataPolicy>,
    #[serde(rename = "globalBlocklist", default)]
    pub global_blocklist: Vec<String>,
    #[serde(rename = "dataPolicyKillAllowlist", default)]
    pub data_policy_kill_allowlist: Vec<String>,
    /// Emit a `%SYS.Audit` record for every tool call on this connection. Off by default.
    /// Requires the event definition to exist on the target IRIS instance — see
    /// `docs/agent-attribution.md` for the one-time setup command.
    #[serde(rename = "irisAudit", default)]
    pub iris_audit: bool,
}

/// Per-connection policy config from `[policy.<server-name>]` in `.iris-agentic-dev.toml`.
///
/// - `allow = None` → all categories permitted.
/// - `allow = Some([...])` → only listed categories permitted; all others blocked.
///
/// `Default` is the strict policy, and it is what `dispatch_gate` uses for a connection with no
/// `[policy.<server>]` section: every `Option` field's `None` already means the locked-down
/// choice (`dataPolicy=block`, `mcpTemplate=dev`), so an unconfigured connection gets the same
/// gates as a configured one rather than none at all.
#[derive(Debug, Clone, Default)]
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
    /// Emit a `%SYS.Audit` record for every tool call on this connection. Off by default.
    pub iris_audit: bool,
}

impl ConnectionPolicy {
    /// The policy that holds when a call is judged by both `self` and `other` (spec 141, R1 D4).
    ///
    /// Used when a pool member is the default connection under another name: `[policy.<name>]` alone
    /// would be a way around `[policy.default]`. The higher template, the stricter data policy, the
    /// intersection of `allow` and of the kill allowlist, the union of the blocklists and of
    /// `irisAudit`. The name stays `self`'s.
    pub fn stricter(&self, other: &ConnectionPolicy) -> ConnectionPolicy {
        fn rank_template(t: &McpTemplate) -> u8 {
            match t {
                McpTemplate::Dev => 0,
                McpTemplate::Test => 1,
                McpTemplate::Live => 2,
            }
        }
        fn rank_data(d: &DataPolicy) -> u8 {
            match d {
                DataPolicy::Allow => 0,
                DataPolicy::Redact => 1,
                DataPolicy::Block => 2,
            }
        }
        let template = match (&self.mcp_template, &other.mcp_template) {
            (Some(a), Some(b)) => Some(if rank_template(b) > rank_template(a) {
                b.clone()
            } else {
                a.clone()
            }),
            (Some(a), None) | (None, Some(a)) => Some(a.clone()),
            (None, None) => None,
        };
        // `None` is `Block`, the strictest.
        let data = match (&self.data_policy, &other.data_policy) {
            (Some(a), Some(b)) => Some(if rank_data(b) > rank_data(a) {
                b.clone()
            } else {
                a.clone()
            }),
            _ => Some(DataPolicy::Block),
        };
        let allow = match (&self.allow, &other.allow) {
            (Some(a), Some(b)) => Some(a.iter().filter(|c| b.contains(c)).cloned().collect()),
            (Some(a), None) | (None, Some(a)) => Some(a.clone()),
            (None, None) => None,
        };
        let mut blocklist = self.global_blocklist.clone();
        for g in &other.global_blocklist {
            if !blocklist.contains(g) {
                blocklist.push(g.clone());
            }
        }
        ConnectionPolicy {
            server_name: self.server_name.clone(),
            allow,
            mcp_template: template,
            data_policy: data,
            global_blocklist: blocklist,
            data_policy_kill_allowlist: self
                .data_policy_kill_allowlist
                .iter()
                .filter(|k| other.data_policy_kill_allowlist.contains(k))
                .cloned()
                .collect(),
            iris_audit: self.iris_audit || other.iris_audit,
        }
    }
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
    /// Values that parsed but were corrected or not understood (an unknown role, a deprecated
    /// spelling). Reported by `check_config`; never a reason to refuse the file.
    #[serde(skip)]
    pub errors: Vec<String>,
}

/// Resolve the workspace root path.
/// Priority: OBJECTSCRIPT_WORKSPACE env var > workspace_path arg > walk up from cwd.
///
/// The walk looks for `.iris-agentic-dev.toml` (git-style discovery), falling back to the legacy
/// `.iris-dev.toml`, and stops before the home directory: the file there is the operator file, which
/// [`crate::iris::config_sources::ConfigSnapshot`] always reads and never treats as a workspace file.
pub fn workspace_root(workspace_path: Option<&str>) -> PathBuf {
    crate::iris::config_sources::workspace_root(
        &crate::iris::config_sources::LoadInputs::from_process(workspace_path, None),
    )
}

fn snapshot_for(
    workspace_path: Option<&str>,
    config_file: Option<&std::path::Path>,
) -> crate::iris::config_sources::ConfigSnapshot {
    crate::iris::config_sources::ConfigSnapshot::load(
        &crate::iris::config_sources::LoadInputs::from_process(workspace_path, config_file),
    )
}

/// The workspace-layer file's connection settings, if there is such a file and it parsed.
/// The operator file is not a workspace file and is not returned here.
pub fn load_workspace_config(workspace_path: Option<&str>) -> Option<WorkspaceConfig> {
    load_workspace_config_with_path(workspace_path).map(|(cfg, _)| cfg)
}

/// Like [`load_workspace_config`] but also returns the path of the file that was loaded.
pub fn load_workspace_config_with_path(
    workspace_path: Option<&str>,
) -> Option<(WorkspaceConfig, std::path::PathBuf)> {
    let snap = snapshot_for(workspace_path, None);
    snap.connection_config
        .filter(|(_, _, kind)| *kind != crate::iris::config_sources::FileKind::Operator)
        .map(|(cfg, path, _)| (cfg, path))
}

/// Does the file name a server? `host` and `container` do, and so do `docker_only` and `nopws`,
/// which name the route (docker exec, never HTTP): a file that sets one must not let discovery
/// hand the call to an HTTP server. `web_port`, `namespace`, `scheme`, `web_prefix` and the
/// credentials shape a connection chosen elsewhere, so a file holding only those is not where the
/// connection came from (spec 141 T194; iris-interop-dev #410).
pub fn names_a_target(cfg: &WorkspaceConfig) -> bool {
    cfg.docker_only
        || cfg.nopws
        || [&cfg.host, &cfg.container]
            .into_iter()
            .any(|v| v.as_deref().is_some_and(|s| !s.trim().is_empty()))
}

/// [`names_a_target`] for the file at `path`; a file that cannot be read or parsed names nothing.
pub fn file_names_a_target(path: &std::path::Path) -> bool {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|c| toml::from_str::<WorkspaceConfig>(&c).ok())
        .is_some_and(|cfg| names_a_target(&cfg))
}

/// Whether a file of this kind chooses the connection, given whether `IRIS_HOST` is set. A
/// workspace-layer file does, over the environment (spec 140, iris-interop-dev #434). The operator
/// file is a personal default that a project without its own file reaches, so it yields to
/// `IRIS_HOST` and chooses only when that is unset. Its tool and gate settings apply either way.
pub fn file_chooses_connection(
    kind: crate::iris::config_sources::FileKind,
    host_env_set: bool,
) -> bool {
    kind != crate::iris::config_sources::FileKind::Operator || !host_env_set
}

/// `http` or `https`, lowercased, from a configured scheme (surrounding `/` and a trailing `:` are
/// tolerated); `None` for anything else, so a scheme cannot carry the URL to a host the loopback
/// check never read.
pub fn checked_scheme(raw: &str) -> Option<String> {
    crate::iris::endpoint::parse_scheme(raw).map(|s| s.as_str().to_string())
}

/// May the operator's `IRIS_USERNAME`/`IRIS_PASSWORD` go to `endpoint`, which a workspace file
/// named? Only when it is this machine or the server launch settings already name: the same scheme,
/// host, port and prefix, not just the same host. A file naming the operator's `https` server with
/// `scheme = "http"` would otherwise get the password in cleartext.
pub fn env_credentials_may_go_to(
    endpoint: &crate::iris::endpoint::Endpoint,
    launch: Option<&crate::iris::endpoint::Endpoint>,
) -> bool {
    endpoint.is_loopback() || launch.is_some_and(|l| l.same_server(endpoint))
}

/// The connection and gate declaration a snapshot gives the caller: the explicit connection if
/// there is one, otherwise the one the chosen file describes; the path of the file that chose it;
/// and the gates the files declare.
///
/// The gate declaration is returned rather than exported to the environment (085 FR-001): it
/// belongs to the config load that produced it, and the next load must be able to change it.
pub fn apply_snapshot(
    explicit: Option<IrisConnection>,
    snap: &crate::iris::config_sources::ConfigSnapshot,
    namespace: &str,
) -> (
    Option<IrisConnection>,
    Option<std::path::PathBuf>,
    crate::tools::write_gate::DeclaredGates,
) {
    let declared = snap.declared_gates();
    match &snap.connection_config {
        Some((cfg, path, _)) => {
            // Tool-list settings apply even when a connection is already established; an explicit
            // connection wins for connectivity.
            let conn_from_cfg = if snap.file_chooses_connection() {
                workspace_config_to_connection(cfg, namespace)
            } else {
                export_tool_lists(cfg);
                None
            };
            (explicit.or(conn_from_cfg), Some(path.clone()), declared)
        }
        None => (explicit, None, declared),
    }
}

/// [`apply_snapshot`] for the workspace found from `workspace_path`.
pub fn apply_workspace_config_with_path(
    explicit: Option<IrisConnection>,
    workspace_path: Option<&str>,
    namespace: &str,
) -> (
    Option<IrisConnection>,
    Option<std::path::PathBuf>,
    crate::tools::write_gate::DeclaredGates,
) {
    let snap = snapshot_for(workspace_path, None);
    snap.announce();
    apply_snapshot(explicit, &snap, namespace)
}

/// [`apply_snapshot`] for an explicit `--config <file>` (issue #111). If `explicit` is set (from
/// CLI flags) that takes precedence, but the file is still read for tool-list and gate settings.
pub fn apply_explicit_config_file(
    explicit: Option<IrisConnection>,
    config_path: &std::path::Path,
    namespace: &str,
) -> (
    Option<IrisConnection>,
    Option<std::path::PathBuf>,
    crate::tools::write_gate::DeclaredGates,
) {
    apply_snapshot(explicit, &snapshot_for(None, Some(config_path)), namespace)
}

/// The fleet config of the workspace-layer file found from `workspace_path`: its instances and
/// policies after layering with the operator file. `None` when there is no such file or it does not
/// parse.
///
/// `workspace_path` may be a workspace **directory** or the path to the config file itself (what
/// `load_pool` receives as `config_file`).
pub fn load_fleet_config(workspace_path: Option<&str>) -> Option<FleetConfig> {
    let snap = snapshot_for(workspace_path, None);
    snap.workspace_layer.is_some().then_some(snap.fleet)
}

/// Every instance both files define, operator file first, for the connection pool. Empty when
/// neither file exists.
pub fn merged_fleet(workspace_path: Option<&str>) -> FleetConfig {
    snapshot_for(workspace_path, None).fleet
}

/// Parse a `FleetConfig` from a TOML string.
/// Resolves `policy` blocks into the `policies` map post-parse.
pub fn load_fleet_config_from_str(contents: &str) -> Result<FleetConfig, toml::de::Error> {
    let mut cfg: FleetConfig = toml::from_str(contents)?;
    if let Ok(raw) = contents.parse::<toml::Table>() {
        crate::iris::config_sources::value_notes(&raw, &mut cfg.errors);
    }
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
                iris_audit: raw.iris_audit,
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
    // A workbench is never gated
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
            "success": false,
            "error_code": "SUBJECT_SCM_WRITE_BLOCKED",
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
        "success": false,
        "error_code": "SUBJECT_CONFIRM_REQUIRED",
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
                ConnectionRole::Workbench => "workbench",
                ConnectionRole::Subject => "subject",
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
            // Null when undeclared. check_config makes no network calls, so this is the only
            // place an operator sees that certificate validation is off before wondering why a
            // bad cert never complained.
            "tls_verify": cfg.workspace.tls_verify,
        })
    }
}

/// Export disabled_tools/enabled_tools to env so IrisTools::with_registry_and_toolset picks them
/// up. Only set if the field is non-empty and the variable is not already set by the user. Runs
/// for every loaded file, including a home-directory file that yields its connection to
/// `IRIS_HOST` (`file_chooses_connection`).
pub fn export_tool_lists(cfg: &WorkspaceConfig) {
    if !cfg.disabled_tools.is_empty() && std::env::var("IRIS_DISABLED_TOOLS").is_err() {
        std::env::set_var("IRIS_DISABLED_TOOLS", cfg.disabled_tools.join(","));
    }
    if !cfg.enabled_tools.is_empty() && std::env::var("IRIS_ENABLED_TOOLS").is_err() {
        std::env::set_var("IRIS_ENABLED_TOOLS", cfg.enabled_tools.join(","));
    }
}

/// The environment credentials the operator set that must not go to `host`, which a file named:
/// a variable the file does not override, when `endpoint` is neither this machine nor the server
/// launch settings name. `is_set` says which of the credential variables the operator set.
pub fn withheld_env_credentials(
    cfg: &WorkspaceConfig,
    endpoint: &crate::iris::endpoint::Endpoint,
    launch: Option<&crate::iris::endpoint::Endpoint>,
    is_set: &dyn Fn(&str) -> bool,
) -> Vec<&'static str> {
    if env_credentials_may_go_to(endpoint, launch) {
        return Vec::new();
    }
    [
        ("IRIS_USERNAME", &cfg.username),
        ("IRIS_PASSWORD", &cfg.password),
        ("IRIS_SERVICE_USERNAME", &None),
        ("IRIS_SERVICE_PASSWORD", &None),
    ]
    .into_iter()
    .filter(|(key, from_file)| from_file.is_none() && is_set(key))
    .map(|(key, _)| key)
    .collect()
}

/// The text of `credentials_withheld` and of its set-aside item.
pub fn credentials_withheld_note(
    withheld: &[&str],
    endpoint: &crate::iris::endpoint::Endpoint,
) -> String {
    format!(
        "{} not sent to {endpoint}: .iris-agentic-dev.toml names that server, and the \
         environment's credentials go only to this machine or to the server IRIS_HOST, \
         IRIS_SCHEME, IRIS_WEB_PORT and IRIS_WEB_PREFIX name. Put username and password in \
         the file, or point IRIS_HOST at {endpoint}.",
        withheld.join(" and ")
    )
}

/// What `cfg` hands to discovery when it names a container and no host (and so builds no
/// connection of its own): the container, with the namespace and account the file gives. Empty
/// for a file that names a host, runs docker only, or names no container.
///
/// The old `workspace_config_to_connection` wrote these into `IRIS_CONTAINER`, `IRIS_NAMESPACE`,
/// `IRIS_USERNAME` and `IRIS_PASSWORD`. They are data now; the process environment is read, never
/// written, so a file's credentials stay distinguishable from the launch settings and a reload
/// that removes the key removes the value.
pub fn container_discovery_hints(cfg: &WorkspaceConfig) -> crate::iris::discovery::DiscoveryHints {
    if cfg.host.is_some() || cfg.docker_only || cfg.nopws {
        return Default::default();
    }
    match cfg.container {
        Some(ref container) => crate::iris::discovery::DiscoveryHints {
            container: Some(container.clone()),
            namespace: cfg.namespace.clone(),
            username: cfg.username.clone(),
            password: cfg.password.clone(),
        },
        None => Default::default(),
    }
}

/// [`container_discovery_hints`] for the file the snapshot's connection comes from, when that file
/// chooses the connection.
pub fn snapshot_discovery_hints(
    snap: &crate::iris::config_sources::ConfigSnapshot,
) -> crate::iris::discovery::DiscoveryHints {
    match &snap.connection_config {
        Some((cfg, _, _)) if snap.file_chooses_connection() => container_discovery_hints(cfg),
        _ => Default::default(),
    }
}

/// [`snapshot_discovery_hints`] for the workspace found from `workspace_path`.
pub fn workspace_discovery_hints(
    workspace_path: Option<&str>,
) -> crate::iris::discovery::DiscoveryHints {
    snapshot_discovery_hints(&snapshot_for(workspace_path, None))
}

/// Apply workspace config to set up the connection environment.
///
/// If `host` is specified: returns `Some(IrisConnection)` that will be passed directly
/// to `discover_iris()` as the explicit override.
///
/// If `container` is specified (but not host): returns `None` to let discovery proceed, with the
/// container and account in [`container_discovery_hints`]. The environment is not written.
///
/// If neither is specified: returns `None` — no connection info in the config.
pub fn workspace_config_to_connection(
    cfg: &WorkspaceConfig,
    namespace_default: &str,
) -> Option<IrisConnection> {
    export_tool_lists(cfg);

    // The two gate keys are NOT exported to the environment (085 FR-001/FR-002). They used to be
    // written to IRIS_WRITE_TOOLS_ENABLED / IRIS_DESTRUCTIVE_TOOLS_ENABLED, and only when the
    // variable was absent — write-once per process, so a config edited from true to false kept
    // serving writes forever, and the second variable had no reader at all. The declaration now
    // travels as data: read it with `write_gate::DeclaredGates::from_config`, resolve it with
    // `write_gate::resolve_for_connection`. The contradictory combination is rejected by
    // `write_gate::validate_gate_config` before startup gets this far (exit 2), rather than by a
    // `return None` here that dropped the caller into the permissive namespace heuristic.

    // host + web_port → explicit HTTP/HTTPS connection (highest priority, no docker needed)
    if let Some(ref host) = cfg.host {
        let port = cfg.web_port.unwrap_or(52773);
        let scheme = cfg
            .scheme
            .clone()
            .or_else(|| std::env::var("IRIS_SCHEME").ok());
        let prefix = cfg
            .web_prefix
            .clone()
            .or_else(|| std::env::var("IRIS_WEB_PREFIX").ok());
        let endpoint = match crate::iris::endpoint::Endpoint::parse(
            scheme.as_deref(),
            host,
            port,
            prefix.as_deref(),
        ) {
            Ok(e) => e,
            Err(e) => {
                // The loopback check below reads the endpoint; a scheme such as
                // `http://elsewhere/#` would carry the URL past it.
                tracing::error!(
                    "INVALID_CONFIG: .iris-agentic-dev.toml: {e}; the file's connection is not used"
                );
                return None;
            }
        };
        let base_url = endpoint.base_url();
        let namespace = cfg
            .namespace
            .clone()
            .or_else(|| std::env::var("IRIS_NAMESPACE").ok())
            .unwrap_or_else(|| namespace_default.to_string());
        // The file came with the repository, so its host is the repository's choice: the
        // operator's env credentials go only to this machine or to IRIS_HOST's server.
        let launch = crate::iris::connection::launch_endpoint();
        let env_ok = env_credentials_may_go_to(&endpoint, launch.as_ref());
        let from_env = |key: &str| std::env::var(key).ok().filter(|_| env_ok);
        let withheld = withheld_env_credentials(cfg, &endpoint, launch.as_ref(), &|key| {
            std::env::var(key).is_ok_and(|v| !v.is_empty())
        });
        let username = cfg
            .username
            .clone()
            .or_else(|| from_env("IRIS_USERNAME"))
            .unwrap_or_else(|| "_SYSTEM".to_string());
        let password = cfg
            .password
            .clone()
            .or_else(|| from_env("IRIS_PASSWORD"))
            .unwrap_or_else(|| "SYS".to_string());
        // If container is also specified alongside host, the connection carries it so docker exec
        // tools (iris_execute fallback, iris_test, etc.) target the right container. It travels on
        // the connection, not in the environment. Use DiscoverySource::Docker so check_config
        // exposes the container name.
        let source = match cfg.container {
            Some(ref container) => DiscoverySource::Docker {
                container_name: container.clone(),
            },
            None => DiscoverySource::EnvVar,
        };
        // The file's own `tls_verify` decides its connection's certificate checks (and arrives
        // here only when the operator wrote it or trusted the file). Naming the server the launch
        // settings name gets the launch settings' say too.
        let tls = match cfg.tls_verify {
            Some(_) => crate::iris::http::TlsSetting::declared(
                cfg.tls_verify,
                crate::iris::http::TlsSource::ConfigFile,
            ),
            None if launch.as_ref().is_some_and(|l| l.same_server(&endpoint)) => {
                crate::iris::http::TlsSetting::from_launch_env()
            }
            None => crate::iris::http::TlsSetting::default(),
        };
        let mut conn =
            IrisConnection::new(base_url, namespace, username, password, source).with_tls(tls);
        conn.member.container = cfg.container.clone();
        if !withheld.is_empty() {
            let note = credentials_withheld_note(&withheld, &endpoint);
            tracing::warn!("{note}");
            conn.credentials_withheld = Some(note);
        }
        return Some(conn);
    }

    // container, no host: `container_discovery_hints` hands the container to discovery as data.
    // Nothing is written to the process environment.
    if let Some(ref container) = cfg.container {
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
        if cfg.docker_only || cfg.nopws {
            // docker_only=true or nopws=true: skip HTTP entirely, use docker exec.
            // Return a connection with an unreachable URL — HTTP calls fail fast,
            // triggering the docker exec fallback in iris_execute/iris_compile etc.
            let mut conn = IrisConnection::new(
                "http://127.0.0.1:1",
                ns,
                username,
                password,
                DiscoverySource::Docker {
                    container_name: container.clone(),
                },
            );
            conn.ssh_host = cfg.ssh_host.clone();
            conn.member.ssh_strict = cfg.ssh_strict;
            conn.member.container = Some(container.clone());
            return Some(conn);
        }
        return None; // discovery finds the container from `container_discovery_hints`
    }

    // docker_only=true with no container field — still route to docker exec sentinel so
    // the block-syntax guard fires before any HTTP attempt. IRIS_CONTAINER env var (if set)
    // will be picked up by execute() at call time.
    if cfg.docker_only || cfg.nopws {
        let ns = cfg
            .namespace
            .clone()
            .or_else(|| std::env::var("IRIS_NAMESPACE").ok())
            .unwrap_or_else(|| "USER".to_string());
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
        let container = std::env::var("IRIS_CONTAINER").ok().unwrap_or_default();
        let mut conn = IrisConnection::new(
            "http://127.0.0.1:1",
            ns,
            username,
            password,
            DiscoverySource::Docker {
                container_name: container,
            },
        );
        conn.ssh_host = cfg.ssh_host.clone();
        conn.member.ssh_strict = cfg.ssh_strict;
        return Some(conn);
    }

    None
}

/// Apply workspace config to an existing explicit connection override.
///
/// If `explicit` is already set (from CLI flags), returns it unchanged.
/// Otherwise loads `.iris-agentic-dev.toml` from `workspace_path` and applies it:
/// - `host` config → returns `Some(IrisConnection)`
/// - `container` config → returns `None`; [`container_discovery_hints`] carries the container
/// - no config / no relevant fields → returns `None`
pub fn apply_workspace_config(
    explicit: Option<IrisConnection>,
    workspace_path: Option<&str>,
    namespace: &str,
) -> Option<IrisConnection> {
    if explicit.is_some() {
        return explicit;
    }
    apply_workspace_config_with_path(None, workspace_path, namespace).0
}

/// Generate starter `.iris-agentic-dev.toml` content with inline comments.
/// Used by `iris-dev init` and `check_config`.
pub fn generate_toml_content(container: &str, namespace: &str) -> String {
    // TOML-quoted, so a value with a quote or a backslash still parses.
    let container_toml = toml::Value::String(container.to_string()).to_string();
    let namespace_toml = toml::Value::String(namespace.to_string()).to_string();
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
# container = {container_toml}

# Default IRIS namespace
namespace = {namespace_toml}

# Credentials (optional)
# Use IRIS_USERNAME / IRIS_PASSWORD env vars instead of committing credentials.
# username = "_SYSTEM"
# password = "..."  # not recommended in committed files

# ── Tool filtering (optional) ────────────────────────────────────────────────
# Remove tools you don't use from the MCP tool list to reduce noise.
# disabled_tools = ["iris_source_control", "iris_admin", "iris_credential_manage"]
# Or go the other way: expose ONLY a named subset, regardless of toolset.
# disabled_tools always wins for a name in both lists.
# enabled_tools = ["iris_query", "iris_search", "iris_symbols"]
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
role = "workbench"            # "workbench" (no write gating) or "subject"

# ── Subject instance (e.g. a customer/production IRIS) ──────────────────────
# Uncomment and fill in to add a subject instance.
# [instance.subject]
# host = "subject-iris.example.com"
# web_port = 52773
# scheme = "https"             # omit for http (default)
# web_prefix = ""              # gateway path prefix, if any
# namespace = "USER"
# username = "_SYSTEM"
# password = "SYS"
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

    /// Points `HOME` at an empty directory for a test, so the loaders do not read the developer's
    /// own `~/.iris-agentic-dev.toml` (the operator file is always read, spec 141). Hold it under
    /// `ENV_LOCK`.
    struct HomeGuard {
        _dir: tempfile::TempDir,
        old: Option<std::ffi::OsString>,
    }

    impl HomeGuard {
        fn new() -> Self {
            let dir = tempfile::tempdir().unwrap();
            let old = std::env::var_os("HOME");
            std::env::set_var("HOME", dir.path());
            HomeGuard { _dir: dir, old }
        }
    }

    impl Drop for HomeGuard {
        fn drop(&mut self) {
            match &self.old {
                Some(v) => std::env::set_var("HOME", v),
                None => std::env::remove_var("HOME"),
            }
        }
    }

    #[test]
    fn instance_base_url_https_with_prefix() {
        let inst = InstanceConfig {
            host: Some("ex.example.com".into()),
            web_port: Some(443),
            scheme: Some("https".into()),
            web_prefix: Some("/hspi/".into()),
            ..Default::default()
        };
        assert_eq!(instance_base_url(&inst), "https://ex.example.com:443/hspi");
    }

    #[test]
    fn instance_base_url_defaults() {
        let inst = InstanceConfig {
            host: Some("prod.example.com".into()),
            ..Default::default()
        };
        assert_eq!(instance_base_url(&inst), "http://prod.example.com:52773");
    }

    #[test]
    fn workspace_root_env_var_overrides_all() {
        let _guard = ENV_LOCK.lock().unwrap();
        let _home = HomeGuard::new();
        let dir = tempfile::tempdir().unwrap();
        std::env::set_var("OBJECTSCRIPT_WORKSPACE", dir.path().to_str().unwrap());
        let root = workspace_root(None);
        std::env::remove_var("OBJECTSCRIPT_WORKSPACE");
        assert_eq!(root, dir.path());
    }

    #[test]
    fn workspace_root_env_overrides_arg() {
        let _guard = ENV_LOCK.lock().unwrap();
        let _home = HomeGuard::new();
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
        let _home = HomeGuard::new();
        std::env::remove_var("OBJECTSCRIPT_WORKSPACE");
        let root = workspace_root(Some(""));
        // Falls through to cwd-walk — just check it doesn't panic and returns something
        assert!(root.is_absolute() || root.to_str() == Some("."));
    }

    #[test]
    fn workspace_root_dot_arg_falls_through() {
        let _guard = ENV_LOCK.lock().unwrap();
        let _home = HomeGuard::new();
        std::env::remove_var("OBJECTSCRIPT_WORKSPACE");
        let root = workspace_root(Some("."));
        assert!(root.is_absolute() || root.to_str() == Some("."));
    }

    // ── load_workspace_config tests ──────────────────────────────────────────

    #[test]
    fn load_config_missing_file_returns_none() {
        let _guard = ENV_LOCK.lock().unwrap();
        let _home = HomeGuard::new();
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
        let _home = HomeGuard::new();
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
        let _home = HomeGuard::new();
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
        let _home = HomeGuard::new();
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
        let _home = HomeGuard::new();
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
            tls_verify: None,
            web_prefix: None,
            docker_only: false,
            nopws: false,
            ssh_host: None,
            ssh_strict: false,
            disabled_tools: vec![],
            enabled_tools: vec![],
            write_tools_enabled: None,
            destructive_tools_enabled: None,
        };
        let conn = workspace_config_to_connection(&cfg, "USER");
        assert!(conn.is_some(), "host config should produce connection");
    }

    #[test]
    fn config_with_container_only_returns_none_and_hands_the_container_to_discovery() {
        let _guard = ENV_LOCK.lock().unwrap();
        let _home = HomeGuard::new();
        std::env::remove_var("IRIS_CONTAINER");
        let cfg = WorkspaceConfig {
            host: None,
            web_port: None,
            namespace: Some("MYNS".to_string()),
            container: Some("my-iris-container".to_string()),
            username: None,
            password: None,
            scheme: None,
            tls_verify: None,
            web_prefix: None,
            docker_only: false,
            nopws: false,
            ssh_host: None,
            ssh_strict: false,
            disabled_tools: vec![],
            enabled_tools: vec![],
            write_tools_enabled: None,
            destructive_tools_enabled: None,
        };
        let conn = workspace_config_to_connection(&cfg, "USER");
        assert!(conn.is_none(), "container-only config should return None");
        assert!(
            std::env::var("IRIS_CONTAINER").is_err(),
            "the environment is not written"
        );
        assert_eq!(
            container_discovery_hints(&cfg).container.as_deref(),
            Some("my-iris-container")
        );
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
            tls_verify: None,
            web_prefix: None,
            docker_only: false,
            nopws: false,
            ssh_host: None,
            ssh_strict: false,
            disabled_tools: vec![],
            enabled_tools: vec![],
            write_tools_enabled: None,
            destructive_tools_enabled: None,
        };
        let conn = workspace_config_to_connection(&cfg, "USER");
        assert!(conn.is_none());
    }

    #[test]
    fn workspace_root_arg_used_when_no_env() {
        let _guard = ENV_LOCK.lock().unwrap();
        let _home = HomeGuard::new();
        std::env::remove_var("OBJECTSCRIPT_WORKSPACE");
        let dir = tempfile::tempdir().unwrap();
        let root = workspace_root(Some(dir.path().to_str().unwrap()));
        assert_eq!(root, dir.path());
    }

    #[test]
    fn workspace_root_legacy_toml_fallback() {
        let _guard = ENV_LOCK.lock().unwrap();
        let _home = HomeGuard::new();
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
            tls_verify: None,
            web_prefix: Some("iriscore".to_string()),
            docker_only: false,
            nopws: false,
            ssh_host: None,
            ssh_strict: false,
            disabled_tools: vec![],
            enabled_tools: vec![],
            write_tools_enabled: None,
            destructive_tools_enabled: None,
        };
        let conn = workspace_config_to_connection(&cfg, "USER").unwrap();
        assert!(
            conn.base_url.contains("iriscore"),
            "URL should include path prefix: {}",
            conn.base_url
        );
    }

    #[test]
    fn config_with_host_and_container_puts_the_container_on_the_connection() {
        let _guard = ENV_LOCK.lock().unwrap();
        let _home = HomeGuard::new();
        std::env::remove_var("IRIS_CONTAINER");
        let cfg = WorkspaceConfig {
            host: Some("localhost".to_string()),
            web_port: Some(52773),
            namespace: None,
            container: Some("my-iris-container".to_string()),
            username: None,
            password: None,
            scheme: None,
            tls_verify: None,
            web_prefix: None,
            docker_only: false,
            nopws: false,
            ssh_host: None,
            ssh_strict: false,
            disabled_tools: vec![],
            enabled_tools: vec![],
            write_tools_enabled: None,
            destructive_tools_enabled: None,
        };
        let conn = workspace_config_to_connection(&cfg, "USER");
        assert!(std::env::var("IRIS_CONTAINER").is_err());
        let conn = conn.expect("host present → connection returned");
        assert_eq!(
            conn.default_container().as_deref(),
            Some("my-iris-container"),
            "the container must travel on the connection when container+host both specified"
        );
    }

    #[test]
    fn config_container_with_credentials_reaches_discovery_not_the_environment() {
        let _guard = ENV_LOCK.lock().unwrap();
        let _home = HomeGuard::new();
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
            tls_verify: None,
            web_prefix: None,
            docker_only: false,
            nopws: false,
            ssh_host: None,
            ssh_strict: false,
            disabled_tools: vec![],
            enabled_tools: vec![],
            write_tools_enabled: None,
            destructive_tools_enabled: None,
        };
        let conn = workspace_config_to_connection(&cfg, "USER");
        assert!(conn.is_none(), "container-only → no connection");
        for k in ["IRIS_NAMESPACE", "IRIS_USERNAME", "IRIS_PASSWORD"] {
            assert!(std::env::var(k).is_err(), "{k} must not be written");
        }
        let hints = container_discovery_hints(&cfg);
        assert_eq!(hints.namespace.as_deref(), Some("MYNAMESPACE"));
        assert_eq!(hints.username.as_deref(), Some("myuser"));
        assert_eq!(hints.password.as_deref(), Some("mypass"));
    }

    #[test]
    fn config_docker_only_returns_unreachable_connection() {
        let _guard = ENV_LOCK.lock().unwrap();
        let _home = HomeGuard::new();
        let cfg = WorkspaceConfig {
            host: None,
            web_port: None,
            namespace: Some("USER".to_string()),
            container: Some("docker-only-iris".to_string()),
            username: None,
            password: None,
            scheme: None,
            tls_verify: None,
            web_prefix: None,
            docker_only: true,
            nopws: false,
            ssh_host: None,
            ssh_strict: false,
            disabled_tools: vec![],
            enabled_tools: vec![],
            write_tools_enabled: None,
            destructive_tools_enabled: None,
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
        let _home = HomeGuard::new();
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
    fn apply_workspace_config_with_path_returns_path() {
        let _guard = ENV_LOCK.lock().unwrap();
        let _home = HomeGuard::new();
        std::env::remove_var("OBJECTSCRIPT_WORKSPACE");
        let dir = tempfile::tempdir().unwrap();
        let mut f = std::fs::File::create(dir.path().join(".iris-agentic-dev.toml")).unwrap();
        use std::io::Write;
        writeln!(f, "host = \"path-host\"\nweb_port = 52773").unwrap();
        std::env::set_var("OBJECTSCRIPT_WORKSPACE", dir.path().to_str().unwrap());
        let (conn, path, _declared) = apply_workspace_config_with_path(None, None, "USER");
        std::env::remove_var("OBJECTSCRIPT_WORKSPACE");
        assert!(conn.is_some(), "should build connection from config");
        assert!(conn.unwrap().base_url.contains("path-host"));
        let path = path.expect("should return config path");
        assert!(
            path.ends_with(".iris-agentic-dev.toml"),
            "returned path should point at the config file"
        );
    }

    /// With no config anywhere, an explicit connection passes through untouched
    /// and there is no config path to report.
    ///
    /// Point OBJECTSCRIPT_WORKSPACE at an empty directory rather than passing
    /// `None`: `workspace_root` walks up from the current directory, so on a
    /// developer machine with a `~/.iris-agentic-dev.toml` this test would
    /// otherwise pick up the real home-directory config and fail.
    #[test]
    fn apply_workspace_config_with_path_returns_none_path_when_no_config_exists() {
        use crate::iris::connection::DiscoverySource;
        let _guard = ENV_LOCK.lock().unwrap();
        let _home = HomeGuard::new();
        let empty = tempfile::tempdir().unwrap();
        std::env::set_var("OBJECTSCRIPT_WORKSPACE", empty.path().to_str().unwrap());

        let explicit = IrisConnection::new(
            "http://explicit:52773",
            "EXPLICIT",
            "_SYSTEM",
            "SYS",
            DiscoverySource::EnvVar,
        );
        let (conn, path, declared) =
            apply_workspace_config_with_path(Some(explicit.clone()), None, "USER");
        std::env::remove_var("OBJECTSCRIPT_WORKSPACE");

        assert!(conn.is_some());
        assert_eq!(conn.unwrap().base_url, explicit.base_url);
        assert!(path.is_none(), "no config found, so no path to report");
        assert_eq!(
            declared,
            crate::tools::write_gate::DeclaredGates::default(),
            "no config file means nothing was declared — an absent key is not a declaration"
        );
    }

    /// An explicit connection wins for connectivity, but the config path is
    /// still reported so the caller can watch that file for tool-list and gate
    /// settings. Pinned here because it reads as a contradiction otherwise.
    #[test]
    fn explicit_connection_still_reports_the_config_path_it_found() {
        use crate::iris::connection::DiscoverySource;
        use std::io::Write;
        let _guard = ENV_LOCK.lock().unwrap();
        let _home = HomeGuard::new();
        let dir = tempfile::tempdir().unwrap();
        let mut f = std::fs::File::create(dir.path().join(".iris-agentic-dev.toml")).unwrap();
        writeln!(
            f,
            "host = \"toml-host\"\nweb_port = 52773\nwrite_tools_enabled = false"
        )
        .unwrap();
        std::env::set_var("OBJECTSCRIPT_WORKSPACE", dir.path().to_str().unwrap());

        let explicit = IrisConnection::new(
            "http://explicit:52773",
            "EXPLICIT",
            "_SYSTEM",
            "SYS",
            DiscoverySource::EnvVar,
        );
        let (conn, path, declared) = apply_workspace_config_with_path(Some(explicit), None, "USER");
        std::env::remove_var("OBJECTSCRIPT_WORKSPACE");

        assert!(
            conn.unwrap().base_url.contains("explicit"),
            "explicit connection must win over the TOML host"
        );
        assert!(
            path.expect("config path should be reported")
                .ends_with(".iris-agentic-dev.toml"),
            "the loaded config path is what the caller watches for tool-list settings"
        );
        assert_eq!(
            declared.write_tools_enabled,
            Some(false),
            "the TOML gate declaration must survive the explicit connection winning \
             connectivity — that split is exactly what the env var used to hide"
        );
    }

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

    // ── load_workspace_config_with_path tests ────────────────────────────────

    #[test]
    fn load_config_with_path_missing_file_returns_none() {
        let _guard = ENV_LOCK.lock().unwrap();
        let _home = HomeGuard::new();
        let dir = tempfile::tempdir().unwrap();
        // No config file — with_path should return None
        let result = load_workspace_config_with_path(Some(dir.path().to_str().unwrap()));
        assert!(result.is_none(), "missing file should return None");
    }

    #[test]
    fn load_config_with_path_valid_toml_returns_some_with_path() {
        let _guard = ENV_LOCK.lock().unwrap();
        let _home = HomeGuard::new();
        let dir = tempfile::tempdir().unwrap();
        use std::io::Write;
        let mut f = std::fs::File::create(dir.path().join(".iris-agentic-dev.toml")).unwrap();
        writeln!(f, "host = \"with-path-host\"\nweb_port = 52773").unwrap();
        let result = load_workspace_config_with_path(Some(dir.path().to_str().unwrap()));
        let (cfg, path) = result.expect("should load config with path");
        assert_eq!(cfg.host, Some("with-path-host".to_string()));
        assert!(
            path.ends_with(".iris-agentic-dev.toml"),
            "returned path should end with config filename"
        );
    }

    #[test]
    fn load_config_with_path_legacy_toml_returns_some() {
        let _guard = ENV_LOCK.lock().unwrap();
        let _home = HomeGuard::new();
        let dir = tempfile::tempdir().unwrap();
        use std::io::Write;
        // Only legacy .iris-dev.toml exists — exercises the legacy branch in with_path
        let mut f = std::fs::File::create(dir.path().join(".iris-dev.toml")).unwrap();
        writeln!(f, "host = \"legacy-host\"\nweb_port = 52773").unwrap();
        let result = load_workspace_config_with_path(Some(dir.path().to_str().unwrap()));
        let (cfg, path) = result.expect("should load legacy config");
        assert_eq!(cfg.host, Some("legacy-host".to_string()));
        assert!(
            path.ends_with(".iris-dev.toml"),
            "returned path should end with legacy filename"
        );
    }

    #[test]
    fn load_config_with_path_invalid_toml_returns_none() {
        let _guard = ENV_LOCK.lock().unwrap();
        let _home = HomeGuard::new();
        let dir = tempfile::tempdir().unwrap();
        use std::io::Write;
        let mut f = std::fs::File::create(dir.path().join(".iris-agentic-dev.toml")).unwrap();
        writeln!(f, "this is not valid toml ={{{{").unwrap();
        let result = load_workspace_config_with_path(Some(dir.path().to_str().unwrap()));
        assert!(result.is_none(), "invalid TOML should return None");
    }

    // ── load_fleet_config_from_str ───────────────────────────────────────────

    #[test]
    fn load_fleet_config_from_str_parses_instance_snake_case() {
        let fleet = load_fleet_config_from_str(
            r#"mode = "operate"
[instance.prod]
host = "prod.example.com"
web_port = 52773
namespace = "MYAPP"
username = "admin"
password = "secret"
"#,
        )
        .unwrap();
        assert_eq!(fleet.mode.as_deref(), Some("operate"));
        let inst = &fleet.instance["prod"];
        assert_eq!(inst.host.as_deref(), Some("prod.example.com"));
        assert_eq!(inst.web_port, Some(52773));
        assert_eq!(inst.namespace.as_deref(), Some("MYAPP"));
        assert_eq!(inst.username.as_deref(), Some("admin"));
        assert_eq!(inst.password.as_deref(), Some("secret"));
    }

    #[test]
    fn load_fleet_config_from_str_parses_instance_kebab_aliases() {
        let fleet = load_fleet_config_from_str(
            r#"mode = "operate"
[instance.gw]
host = "gw.example.com"
web-port = 443
web-prefix = "/csp/sys"
scheme = "https"
memory-home = "local"
"#,
        )
        .unwrap();
        let inst = &fleet.instance["gw"];
        assert_eq!(inst.web_port, Some(443));
        assert_eq!(inst.web_prefix.as_deref(), Some("/csp/sys"));
        assert_eq!(inst.scheme.as_deref(), Some("https"));
        assert_eq!(inst.memory_home.as_deref(), Some("local"));
        assert_eq!(
            instance_base_url(inst),
            "https://gw.example.com:443/csp/sys"
        );
    }

    #[test]
    fn load_fleet_config_from_str_invalid_toml_returns_error() {
        let result = load_fleet_config_from_str("not valid toml = {{{{");
        assert!(result.is_err());
    }

    #[test]
    fn instance_base_url_scheme_strips_trailing_slashes() {
        let inst = InstanceConfig {
            host: Some("h.example.com".into()),
            web_port: Some(443),
            scheme: Some("https/".into()),
            ..Default::default()
        };
        assert_eq!(instance_base_url(&inst), "https://h.example.com:443");
    }

    #[test]
    fn instance_base_url_empty_scheme_falls_back_to_http() {
        let inst = InstanceConfig {
            host: Some("h.example.com".into()),
            web_port: Some(80),
            scheme: Some("".into()),
            ..Default::default()
        };
        assert_eq!(instance_base_url(&inst), "http://h.example.com:80");
    }

    #[test]
    fn instance_base_url_prefix_strips_surrounding_slashes() {
        let inst = InstanceConfig {
            host: Some("h.example.com".into()),
            web_port: Some(443),
            scheme: Some("https".into()),
            web_prefix: Some("/hspi/".into()),
            ..Default::default()
        };
        assert_eq!(instance_base_url(&inst), "https://h.example.com:443/hspi");
    }

    // ── write_tools_enabled / destructive_tools_enabled (#110, 085 T014) ─────
    //
    // These used to assert that `workspace_config_to_connection` exported
    // `IRIS_WRITE_TOOLS_ENABLED`, from a struct literal, after `remove_var` — so they only ever
    // exercised the clean-env branch while the #110 defect lived exclusively in the other one.
    // The config loader no longer touches the environment at all: it hands the declaration back
    // as data. Each test below parses a config *string* (constitution test layer 1, so a serde
    // silent-drop fails here) and asserts what the resolver does with the declaration.

    use crate::tools::write_gate::{resolve_declared, DeclaredGates, GateSource, OperatorEnvGates};

    /// Resolve a config string the way startup does, against a dev instance in USER — the context
    /// where inference would say `true`, so a declared `false` that is honoured is unambiguous.
    fn resolve_toml(toml_src: &str) -> crate::tools::write_gate::GateResolution {
        let cfg: WorkspaceConfig = toml::from_str(toml_src).expect("config string must parse");
        resolve_declared(
            &OperatorEnvGates::default(),
            DeclaredGates::from_config(&cfg),
            crate::iris::connection::SystemMode::Development,
            "USER",
        )
    }

    #[test]
    fn write_tools_enabled_false_is_honoured_over_inference() {
        let r = resolve_toml("host = \"localhost\"\nweb_port = 52773\nwrite_tools_enabled = false");
        assert!(
            !r.write_enabled,
            "a declared false must win over a Development instance in USER"
        );
        assert_eq!(r.write_source, GateSource::ConfigFile);
    }

    #[test]
    fn write_tools_enabled_true_is_attributed_to_the_config_file() {
        let r = resolve_toml("host = \"localhost\"\nweb_port = 52773\nwrite_tools_enabled = true");
        assert!(r.write_enabled);
        assert_eq!(
            r.write_source,
            GateSource::ConfigFile,
            "reported source must be the declaration, not the inference that agrees with it"
        );
    }

    #[test]
    fn write_tools_enabled_absent_falls_through_to_inference() {
        let r = resolve_toml("host = \"localhost\"\nweb_port = 52773");
        assert!(r.write_enabled, "Development in USER infers writes on");
        assert_eq!(
            r.write_source,
            GateSource::InferredSystemMode,
            "an absent key is not a declaration"
        );
    }

    /// The env exports are gone. Pinned as a test because the whole #110 defect was invisible
    /// process-global state, and a well-meaning re-export would restore it silently.
    #[test]
    fn config_load_does_not_export_the_gate_to_the_environment() {
        let _guard = ENV_LOCK.lock().unwrap();
        let _home = HomeGuard::new();
        std::env::remove_var("IRIS_WRITE_TOOLS_ENABLED");
        std::env::remove_var("IRIS_DESTRUCTIVE_TOOLS_ENABLED");
        let cfg: WorkspaceConfig = toml::from_str(
            "host = \"localhost\"\nweb_port = 52773\nwrite_tools_enabled = false\n\
             destructive_tools_enabled = false",
        )
        .unwrap();
        let _ = workspace_config_to_connection(&cfg, "USER");
        assert!(
            std::env::var("IRIS_WRITE_TOOLS_ENABLED").is_err(),
            "the gate must not travel through the environment (085 FR-001)"
        );
        assert!(
            std::env::var("IRIS_DESTRUCTIVE_TOOLS_ENABLED").is_err(),
            "the destructive key had no reader for its whole documented life; do not re-export it"
        );
    }

    /// `destructive = true` + `write = false` used to log `DESTRUCTIVE_REQUIRES_WRITES`, then
    /// `return None` *before* the env export — so the gate was never set, `is_write_allowed()` fell
    /// through to the USER-namespace heuristic, and the server came up with writes ON. It is now
    /// rejected as a config error, and the resolver clamps the tier regardless.
    #[test]
    fn destructive_requires_writes_is_rejected_and_never_widens() {
        let src = "host = \"localhost\"\nweb_port = 52773\nwrite_tools_enabled = false\n\
                   destructive_tools_enabled = true";
        let cfg: WorkspaceConfig = toml::from_str(src).unwrap();

        let err = crate::tools::write_gate::validate_gate_config(&cfg)
            .expect_err("the contradictory combination must be a config error");
        assert_eq!(
            err.code(),
            crate::tools::write_gate::ERR_DESTRUCTIVE_REQUIRES_WRITES
        );

        // Belt and braces: even if a caller ignores the error, resolution must not fail open.
        let r = resolve_toml(src);
        assert!(!r.write_enabled, "must not fall through to the heuristic");
        assert!(!r.destructive_enabled);
    }

    #[test]
    fn apply_explicit_config_file_sets_enabled_tools_env() {
        let _guard = ENV_LOCK.lock().unwrap();
        let _home = HomeGuard::new();
        std::env::remove_var("IRIS_ENABLED_TOOLS");
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(".iris-agentic-dev.toml");
        std::fs::write(
            &path,
            r#"host="localhost"
web_port=52773
enabled_tools=["iris_query","check_config"]
"#,
        )
        .unwrap();
        let (conn, cfg_path, _declared) = apply_explicit_config_file(None, &path, "USER");
        assert!(
            conn.is_some(),
            "should build connection from explicit config"
        );
        assert_eq!(cfg_path.unwrap(), std::fs::canonicalize(&path).unwrap());
        assert_eq!(
            std::env::var("IRIS_ENABLED_TOOLS").unwrap(),
            "iris_query,check_config"
        );
        std::env::remove_var("IRIS_ENABLED_TOOLS");
    }

    #[test]
    fn apply_explicit_config_file_missing_returns_explicit_unchanged() {
        use crate::iris::connection::DiscoverySource;
        let _guard = ENV_LOCK.lock().unwrap();
        let _home = HomeGuard::new();
        let explicit = IrisConnection::new(
            "http://cli-host:52773",
            "USER",
            "_SYSTEM",
            "SYS",
            DiscoverySource::EnvVar,
        );
        let (conn, path, declared) = apply_explicit_config_file(
            Some(explicit.clone()),
            std::path::Path::new("/nonexistent/path/no-such-file.toml"),
            "USER",
        );
        assert_eq!(conn.unwrap().base_url, explicit.base_url);
        assert!(path.is_none());
        assert_eq!(
            declared,
            crate::tools::write_gate::DeclaredGates::default(),
            "an unreadable config declares nothing"
        );
    }
}

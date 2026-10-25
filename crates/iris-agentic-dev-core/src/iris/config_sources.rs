//! Configuration sources, trust and the set-aside list (spec 141, R4a D1, D2, D4, D8).
//!
//! `ConfigSnapshot::load` reads the operator file every time, plus at most one workspace-layer
//! file, and answers every question the gates ask about configuration: which file chose the
//! connection, which loosening settings apply, what was set aside and why.
//!
//! Nothing here reads the process environment except [`LoadInputs::from_process`], so every rule
//! is testable from a temporary directory.

use crate::iris::operator_hold::{hold_loosening, Held};
use crate::iris::workspace_config::{
    checked_scheme, env_credentials_may_go_to, load_fleet_config_from_str, parse_role,
    ConnectionRole, FleetConfig, McpTemplate, WorkspaceConfig,
};
use crate::policy::effective::{
    effective_policy, Binding, EffectivePolicy, LaunchPolicy, PartSource, PolicyInputs, PolicyLayer,
};
use crate::tools::write_gate::{
    validate_declared_gates, DeclaredGates, GateConfigError, GateSource,
};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

/// The file name of the operator file and of a workspace file.
pub const CONFIG_FILE_NAME: &str = ".iris-agentic-dev.toml";
const LEGACY_FILE_NAME: &str = ".iris-dev.toml";

/// Version of the canonical form of a trust entry's `settings`. A bump lapses trust on purpose.
pub const TRUST_FORMAT: u32 = 1;

/// Keys only the operator file and launch settings may set (FR-035). Dotted for nested tables.
pub const OPERATOR_ONLY_KEYS: &[&str] = &[
    "trust",
    "exec_as_os_user",
    "transport.http_token",
    "local_skills",
    "audit_payloads",
    "telemetry_sink",
];

/// Does a `read_only` value (the file key or `IAD_READ_ONLY`) ask for strict read-only?
fn is_strict_value(v: &str) -> bool {
    v.trim().eq_ignore_ascii_case("strict")
}

/// Is `path` the operator file, `~/.iris-agentic-dev.toml`?
pub fn is_operator_file(path: &Path) -> bool {
    dirs::home_dir()
        .map(|h| h.join(CONFIG_FILE_NAME))
        .is_some_and(|op| canon(&op) == canon(path))
}

/// What a set-aside item tells the operator to run. A trust entry loosens, so a running server
/// takes it up at its next start (FR-043).
pub fn trust_command_hint(file: Option<&Path>) -> String {
    match file {
        Some(p) => format!(
            "run `iris-agentic-dev trust {}` in a terminal, then restart iad",
            p.display()
        ),
        None => "run `iris-agentic-dev trust` in a terminal, then restart iad".to_string(),
    }
}

/// Everything one load needs from the process, so tests can pass it without touching the
/// environment.
#[derive(Debug, Clone, Default)]
pub struct LoadInputs {
    /// `--config <file>`.
    pub config_flag: Option<PathBuf>,
    /// `--workspace <dir>` when it is not the default `.`.
    pub workspace_flag: Option<String>,
    /// `OBJECTSCRIPT_WORKSPACE`.
    pub env_workspace: Option<String>,
    pub cwd: PathBuf,
    pub home: Option<PathBuf>,
    /// `IRIS_HOST`, for the credential rule that decides where operator loosening may follow.
    pub host_env: Option<String>,
    /// The server `IRIS_HOST`, `IRIS_SCHEME`, `IRIS_WEB_PORT` and `IRIS_WEB_PREFIX` name. When it
    /// is `None` and `host_env` is set, the server is `host_env` on the default port.
    pub launch_endpoint: Option<crate::iris::endpoint::Endpoint>,
    /// Which of `IRIS_USERNAME`, `IRIS_PASSWORD`, `IRIS_SERVICE_USERNAME` and
    /// `IRIS_SERVICE_PASSWORD` the operator set, when the caller (a test) says; `None` reads the
    /// process environment.
    pub env_credentials_set: Option<Vec<String>>,
    /// `IRIS_WRITE_TOOLS_ENABLED`.
    pub launch_write: Option<bool>,
    /// `IRIS_DESTRUCTIVE_TOOLS_ENABLED`.
    pub launch_destructive: Option<bool>,
    /// `IAD_COMPILE_TIME_GATE`: `Some(true)` is on everywhere, `Some(false)` is off for the default
    /// server.
    pub launch_compile_gate: Option<bool>,
    /// `IAD_READ_ONLY=strict` (spec 141 R5 C15).
    pub launch_read_only_strict: bool,
    /// The operator file as the running server applies it. A reload passes the table of the load
    /// before; a loosening edit since then is held back (spec 141 FR-043). `None` at a start.
    pub operator_baseline: Option<toml::Table>,
}

impl LoadInputs {
    /// The inputs of this process. `workspace_flag` is the `--workspace` value; `.` and the empty
    /// string mean it was not given.
    pub fn from_process(workspace_flag: Option<&str>, config_flag: Option<&Path>) -> Self {
        let env_gates = crate::tools::write_gate::operator_env_gates();
        Self {
            config_flag: config_flag.map(Path::to_path_buf),
            workspace_flag: workspace_flag
                .filter(|p| !p.is_empty() && *p != ".")
                .map(str::to_string),
            env_workspace: std::env::var("OBJECTSCRIPT_WORKSPACE")
                .ok()
                .filter(|s| !s.is_empty()),
            cwd: std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
            home: dirs::home_dir(),
            host_env: crate::iris::connection::host_from_env(),
            launch_endpoint: crate::iris::connection::launch_endpoint(),
            env_credentials_set: None,
            launch_write: env_gates.write_tools_enabled,
            launch_destructive: env_gates.destructive_enabled,
            launch_compile_gate: std::env::var("IAD_COMPILE_TIME_GATE")
                .ok()
                .and_then(|v| LaunchPolicy::compile_gate_from_env_value(&v)),
            launch_read_only_strict: std::env::var("IAD_READ_ONLY")
                .is_ok_and(|v| is_strict_value(&v)),
            operator_baseline: None,
        }
    }

    /// The server launch settings name, if they name one.
    fn launch_server(&self) -> Option<crate::iris::endpoint::Endpoint> {
        self.launch_endpoint.clone().or_else(|| {
            self.host_env
                .as_deref()
                .and_then(|h| crate::iris::endpoint::Endpoint::parse(None, h, 52773, None).ok())
        })
    }

    fn launch_policy(&self) -> LaunchPolicy {
        LaunchPolicy {
            write_tools: self.launch_write,
            destructive_tools: self.launch_destructive,
            compile_gate: self.launch_compile_gate,
        }
    }
}

/// Which kind of file a [`LoadedFile`] is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileKind {
    Operator,
    Workspace,
    Named,
}

impl FileKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            FileKind::Operator => "operator_file",
            FileKind::Workspace => "workspace_file",
            FileKind::Named => "named_config_file",
        }
    }
}

/// What named a [`FileKind::Named`] or workspace file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NamedBy {
    ConfigFlag,
    ObjectscriptWorkspace,
    WorkspaceFlag,
}

impl NamedBy {
    pub fn as_str(&self) -> &'static str {
        match self {
            NamedBy::ConfigFlag => "--config",
            NamedBy::ObjectscriptWorkspace => "OBJECTSCRIPT_WORKSPACE",
            NamedBy::WorkspaceFlag => "--workspace",
        }
    }
}

/// One configuration file that was read and parsed as TOML.
#[derive(Debug, Clone)]
pub struct LoadedFile {
    /// Canonical path.
    pub path: PathBuf,
    pub kind: FileKind,
    pub named_by: Option<NamedBy>,
    pub table: toml::Table,
}

/// A configuration file that could not be read or parsed (R5 C11).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileError {
    pub path: PathBuf,
    pub line: Option<usize>,
    pub column: Option<usize>,
    pub message: String,
}

impl FileError {
    /// `<file>:<line>:<column>: <message>`
    pub fn display(&self) -> String {
        match (self.line, self.column) {
            (Some(l), Some(c)) => format!("{}:{l}:{c}: {}", self.path.display(), self.message),
            _ => format!("{}: {}", self.path.display(), self.message),
        }
    }
}

/// One loosening setting that differs from what was trusted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Change {
    pub setting: String,
    pub was: Option<String>,
    pub now: Option<String>,
}

/// Whether a workspace-layer file's loosening settings are trusted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TrustState {
    /// No workspace-layer file, or it holds no loosening settings.
    NotNeeded,
    Trusted,
    Untrusted,
    Lapsed {
        changed: Vec<Change>,
    },
}

impl TrustState {
    pub fn as_str(&self) -> &'static str {
        match self {
            TrustState::NotNeeded => "not_needed",
            TrustState::Trusted => "trusted",
            TrustState::Untrusted => "untrusted",
            TrustState::Lapsed { .. } => "lapsed",
        }
    }

    /// Do the file's loosening settings apply?
    pub fn is_trusted(&self) -> bool {
        matches!(self, TrustState::Trusted)
    }
}

/// A `[[trust]]` entry of the operator file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrustEntry {
    pub path: String,
    pub format: u32,
    pub settings: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SetAsideKind {
    UntrustedSetting,
    OperatorOnly,
    CredentialWithheld,
    ShadowedServer,
    /// An edit to the operator file that loosens, held until the next start (FR-043).
    HeldForRestart,
}

impl SetAsideKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            SetAsideKind::UntrustedSetting => "untrusted_setting",
            SetAsideKind::OperatorOnly => "operator_only",
            SetAsideKind::CredentialWithheld => "credential_withheld",
            SetAsideKind::ShadowedServer => "shadowed_server",
            SetAsideKind::HeldForRestart => "held_for_restart",
        }
    }
}

/// A setting iad read and did not act on (R4a D8).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SetAsideItem {
    pub id: String,
    /// The policy part this item belongs to (`mcp_template`, `allow`, `data_policy`,
    /// `kill_allowlist`, `write_tools`, `destructive_tools`, `role`, `compile_gate`), when it has one.
    pub part: Option<&'static str>,
    pub kind: SetAsideKind,
    pub setting: String,
    pub server: Option<String>,
    pub file: Option<String>,
    pub reason: String,
    pub lift: String,
}

impl SetAsideItem {
    pub fn to_json(&self) -> serde_json::Value {
        serde_json::json!({
            "id": self.id,
            "kind": self.kind.as_str(),
            "setting": self.setting,
            "server": self.server,
            "file": self.file,
            "reason": self.reason,
            "lift": self.lift,
        })
    }
}

/// Operator-only settings, read from the operator file (launch forms are merged by the caller).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct OperatorSettings {
    pub exec_as_os_user: bool,
    /// Servers whose `[policy.<name>]` section sets `execAsOsUser = true`.
    pub exec_as_os_user_servers: Vec<String>,
    pub http_token: Option<String>,
    pub local_skills: bool,
    pub audit_payloads: bool,
    /// `telemetry_sink = "iris"` adds the IRIS global to the local telemetry file (R5 C16).
    pub telemetry_sink: crate::telemetry::TelemetrySink,
}

/// The configuration of one load.
#[derive(Debug, Clone, Default)]
pub struct ConfigSnapshot {
    /// Where the operator file is or would be.
    pub operator_path: Option<PathBuf>,
    pub operator_file: Option<LoadedFile>,
    pub workspace_layer: Option<LoadedFile>,
    /// The workspace-layer file's trust state; `None` when there is no such file.
    pub workspace_trust: Option<TrustState>,
    pub trust_entries: Vec<TrustEntry>,
    pub operator: OperatorSettings,
    /// Files that could not be parsed. Any entry makes the snapshot invalid.
    pub errors: Vec<FileError>,
    /// Values iad corrected or did not understand, from files that did parse.
    pub config_errors: Vec<String>,
    pub set_aside: Vec<SetAsideItem>,
    /// Loosening edits to the operator file held back since the server started.
    pub held: Vec<Held>,
    /// Instances and policies of both files, after layering. Policies are the effective ones.
    pub fleet: FleetConfig,
    /// The file that chose the connection, with its settings after the trust filter.
    pub connection_config: Option<(WorkspaceConfig, PathBuf, FileKind)>,
    /// Effective policy per server name, `default` first.
    pub policies: BTreeMap<String, EffectivePolicy>,
    /// What the files declare about the two gates, without launch settings.
    declared: DeclaredGates,
    /// The layers each server's policy was built from, so it can be built again once a connection
    /// says what the system mode implies.
    layers: BTreeMap<String, StoredLayers>,
    /// What asked for `read_only = "strict"`: a file path or `IAD_READ_ONLY`. It tightens, so any
    /// source may set it and none can lift another's (R5 C15).
    read_only_strict: Option<String>,
    /// The directory host paths are confined to (spec 141 T212, `tools/host_fs.rs`); `None` when
    /// no project directory is known, and then every host path is refused.
    pub project_root: Option<PathBuf>,
    /// The directory iad watches for a new project file when there is none.
    watch_dir: Option<PathBuf>,
    /// The workspace-layer candidate, even when it did not parse.
    workspace_candidate: Option<PathBuf>,
    host_env_set: bool,
}

/// One server's policy inputs.
#[derive(Debug, Clone)]
struct StoredLayers {
    is_default: bool,
    operator: Option<PolicyLayer>,
    workspace: Option<PolicyLayer>,
    binding: Binding,
    launch: LaunchPolicy,
}

/// What [`ConfigSnapshot::load`] found at a candidate path.
enum Read {
    Missing,
    Failed(FileError),
    Ok(LoadedFile),
}

fn canon(p: &Path) -> PathBuf {
    std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf())
}

fn line_col(contents: &str, offset: usize) -> (usize, usize) {
    let offset = offset.min(contents.len());
    let before = &contents[..offset];
    let line = before.matches('\n').count() + 1;
    let col = before
        .rsplit('\n')
        .next()
        .map(|l| l.chars().count())
        .unwrap_or(0)
        + 1;
    (line, col)
}

fn read_file(path: &Path, kind: FileKind, named_by: Option<NamedBy>) -> Read {
    let cpath = canon(path);
    let contents = match std::fs::read_to_string(path) {
        Ok(c) => c,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Read::Missing,
        Err(e) => {
            return Read::Failed(FileError {
                path: cpath,
                line: None,
                column: None,
                message: format!("cannot be read: {e}"),
            })
        }
    };
    let fail = |e: &toml::de::Error| {
        let (line, column) = e
            .span()
            .map(|s| line_col(&contents, s.start))
            .map(|(l, c)| (Some(l), Some(c)))
            .unwrap_or((None, None));
        Read::Failed(FileError {
            path: cpath.clone(),
            line,
            column,
            message: e.message().to_string(),
        })
    };
    let table = match contents.parse::<toml::Table>() {
        Ok(t) => t,
        Err(e) => return fail(&e),
    };
    // Types as well as syntax: `web_port = "abc"` is a broken file, not a missing key.
    if let Err(e) = toml::from_str::<WorkspaceConfig>(&contents) {
        return fail(&e);
    }
    if let Err(e) = load_fleet_config_from_str(&contents) {
        return fail(&e);
    }
    Read::Ok(LoadedFile {
        path: cpath,
        kind,
        named_by,
        table,
    })
}

/// A directory or a file path named by a flag, as the file it means.
fn resolve_named(p: &Path) -> Option<PathBuf> {
    let is_toml = p
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("toml"));
    if p.is_file() || (is_toml && !p.is_dir()) {
        return Some(p.to_path_buf());
    }
    [CONFIG_FILE_NAME, LEGACY_FILE_NAME]
        .iter()
        .map(|n| p.join(n))
        .find(|c| c.exists())
}

/// The workspace-layer file in D1's order with its kind and what named it, and the directory to
/// watch for one when there is none.
type Candidate = (
    Option<(PathBuf, FileKind, Option<NamedBy>)>,
    Option<PathBuf>,
);

fn candidate(inputs: &LoadInputs, home: Option<&Path>) -> Candidate {
    let rel = |p: &Path| {
        if p.is_absolute() {
            p.to_path_buf()
        } else {
            inputs.cwd.join(p)
        }
    };
    if let Some(cfg) = &inputs.config_flag {
        let p = rel(cfg);
        return (
            Some((p.clone(), FileKind::Named, Some(NamedBy::ConfigFlag))),
            p.parent().map(Path::to_path_buf),
        );
    }
    for (value, kind, by) in [
        (
            inputs.env_workspace.as_deref(),
            FileKind::Named,
            NamedBy::ObjectscriptWorkspace,
        ),
        (
            inputs.workspace_flag.as_deref(),
            FileKind::Workspace,
            NamedBy::WorkspaceFlag,
        ),
    ] {
        if let Some(v) = value.filter(|v| !v.is_empty()) {
            let dir = rel(Path::new(v));
            return (
                resolve_named(&dir).map(|p| (p, kind, Some(by))),
                Some(if dir.is_dir() || !dir.exists() {
                    dir
                } else {
                    dir.parent().map(Path::to_path_buf).unwrap_or(dir)
                }),
            );
        }
    }

    let cwd = canon(&inputs.cwd);
    let found = walk(&cwd, home);
    let watch = if home.is_some_and(|h| cwd == h) || cwd == Path::new("/") {
        None
    } else {
        Some(cwd)
    };
    (found.map(|f| (f, FileKind::Workspace, None)), watch)
}

/// The nearest `.iris-agentic-dev.toml` walking up from `cwd`, falling back to the nearest legacy
/// `.iris-dev.toml`. The walk stops before `home`, so the operator file is never found by it.
fn walk(cwd: &Path, home: Option<&Path>) -> Option<PathBuf> {
    let mut dir = cwd;
    let mut legacy: Option<PathBuf> = None;
    loop {
        if home.is_some_and(|h| dir == h) {
            break;
        }
        let new = dir.join(CONFIG_FILE_NAME);
        if new.is_file() {
            return Some(new);
        }
        let old = dir.join(LEGACY_FILE_NAME);
        if legacy.is_none() && old.is_file() {
            legacy = Some(old);
        }
        match dir.parent() {
            Some(p) => dir = p,
            None => break,
        }
    }
    legacy
}

/// The directory a load looks in: `OBJECTSCRIPT_WORKSPACE`, then `--workspace`, then the directory of
/// the file the walk finds, then the working directory.
pub fn workspace_root(inputs: &LoadInputs) -> PathBuf {
    if let Some(v) = [
        inputs.env_workspace.as_deref(),
        inputs.workspace_flag.as_deref(),
    ]
    .into_iter()
    .flatten()
    .find(|v| !v.is_empty())
    {
        return PathBuf::from(v);
    }
    let home = inputs.home.as_deref().map(canon);
    walk(&canon(&inputs.cwd), home.as_deref())
        .and_then(|f| f.parent().map(Path::to_path_buf))
        .unwrap_or_else(|| inputs.cwd.clone())
}

// ── Loosening settings and the fingerprint (D2) ────────────────────────────────────────────────

fn q(s: &str) -> String {
    toml::Value::String(s.to_string()).to_string()
}

fn q_list(items: &[String]) -> String {
    format!(
        "[{}]",
        items.iter().map(|s| q(s)).collect::<Vec<_>>().join(", ")
    )
}

fn str_of(v: &toml::Value) -> Option<String> {
    v.as_str().map(|s| s.trim().to_string())
}

fn port_of(t: &toml::Table) -> u16 {
    ["web_port", "web-port", "port"]
        .iter()
        .find_map(|k| t.get(*k).and_then(|v| v.as_integer()))
        .and_then(|p| u16::try_from(p).ok())
        .unwrap_or(52773)
}

/// Where the server a file's table describes is. A scheme or prefix the table leaves out is the
/// launch settings' (`IRIS_SCHEME` and `IRIS_WEB_PREFIX` fill them in when the connection is
/// built); `None` when the table's address is not a valid endpoint.
fn endpoint_of(
    t: &toml::Table,
    launch: Option<&crate::iris::endpoint::Endpoint>,
) -> Option<crate::iris::endpoint::Endpoint> {
    let host = t
        .get("host")
        .and_then(str_of)
        .unwrap_or_else(|| "localhost".to_string());
    let scheme = t
        .get("scheme")
        .and_then(str_of)
        .or_else(|| launch.map(|l| l.scheme().as_str().to_string()));
    let prefix = ["web_prefix", "web-prefix"]
        .iter()
        .find_map(|k| t.get(*k).and_then(str_of))
        .or_else(|| launch.and_then(|l| l.prefix().map(str::to_string)));
    crate::iris::endpoint::Endpoint::parse(scheme.as_deref(), &host, port_of(t), prefix.as_deref())
        .ok()
}

fn address_of(t: &toml::Table) -> String {
    let host = t
        .get("host")
        .and_then(str_of)
        .unwrap_or_else(|| "localhost".to_string());
    let scheme = t
        .get("scheme")
        .and_then(str_of)
        .and_then(|s| checked_scheme(&s))
        .unwrap_or_else(|| "http".to_string());
    let prefix = ["web_prefix", "web-prefix"]
        .iter()
        .find_map(|k| t.get(*k).and_then(str_of));
    format!(
        "{scheme}://{host}:{}{}",
        port_of(t),
        crate::iris::connection_pool::web_prefix_path_part(prefix.as_deref())
    )
}

/// Lines of the default connection's address, when the file defines one.
fn default_address_lines(t: &toml::Table) -> Vec<String> {
    let mut v = Vec::new();
    let has_host = t.get("host").and_then(str_of).is_some();
    let container = t.get("container").and_then(str_of);
    if has_host {
        v.push(format!("connection = {}", q(&address_of(t))));
    }
    if let Some(c) = &container {
        v.push(format!("connection.container = {}", q(c)));
    }
    if has_host || container.is_some() {
        if let Some(ns) = t.get("namespace").and_then(str_of) {
            v.push(format!("connection.namespace = {}", q(&ns)));
        }
    }
    v
}

fn instance_address_lines(name: &str, t: &toml::Table) -> Vec<String> {
    let mut v = vec![format!("instance.{name}.address = {}", q(&address_of(t)))];
    if let Some(c) = t.get("container").and_then(str_of) {
        v.push(format!("instance.{name}.container = {}", q(&c)));
    }
    if let Some(ns) = t.get("namespace").and_then(str_of) {
        v.push(format!("instance.{name}.namespace = {}", q(&ns)));
    }
    v
}

/// The loosening settings of a parsed file as sorted, deduplicated canonical lines, each address
/// the settings apply to included (R4a D2).
pub fn loosening_settings(table: &toml::Table) -> Vec<String> {
    let mut lines: Vec<(String, String)> = Vec::new(); // (dotted key, value text)
    let mut push = |k: String, v: String| lines.push((k, v));

    let flag = |t: &toml::Table, k: &str| t.get(k).and_then(|v| v.as_bool());
    for (k, loosens_when) in [
        ("write_tools_enabled", true),
        ("destructive_tools_enabled", true),
        ("compile_time_gate", false),
        ("tls_verify", false),
    ] {
        if flag(table, k) == Some(loosens_when) {
            push(k.to_string(), loosens_when.to_string());
        }
    }
    for k in ["ssh_host", "service_username"] {
        if let Some(v) = table.get(k).and_then(str_of).filter(|v| !v.is_empty()) {
            push(k.to_string(), q(&v));
        }
    }

    if let Some(policies) = table.get("policy").and_then(|p| p.as_table()) {
        for (name, p) in policies {
            let Some(p) = p.as_table() else { continue };
            let key = |k: &str| format!("policy.{name}.{k}");
            for (k, loosens_when) in [
                ("writeTools", true),
                ("destructiveTools", true),
                ("compileTimeGate", false),
            ] {
                if flag(p, k) == Some(loosens_when) {
                    push(key(k), loosens_when.to_string());
                }
            }
            if let Some(t) = p.get("mcpTemplate").and_then(str_of) {
                let t = t.to_ascii_lowercase();
                if t == "dev" || t == "test" {
                    push(key("mcpTemplate"), q(&t));
                }
            }
            if let Some(items) = p.get("allow").and_then(|v| v.as_array()) {
                let mut cats: Vec<String> = items
                    .iter()
                    .filter_map(str_of)
                    .map(|s| s.to_ascii_lowercase())
                    .filter(|s| !s.is_empty())
                    .collect();
                cats.sort();
                cats.dedup();
                if !cats.is_empty() {
                    push(key("allow"), q_list(&cats));
                }
            }
            if let Some(d) = p.get("dataPolicy").and_then(str_of) {
                let d = d.to_ascii_lowercase();
                if d == "allow" || d == "redact" {
                    push(key("dataPolicy"), q(&d));
                }
            }
            if let Some(items) = p.get("dataPolicyKillAllowlist").and_then(|v| v.as_array()) {
                let mut pats: Vec<String> = items
                    .iter()
                    .filter_map(str_of)
                    .filter(|s| !s.is_empty())
                    .collect();
                pats.sort();
                pats.dedup();
                if !pats.is_empty() {
                    push(key("dataPolicyKillAllowlist"), q_list(&pats));
                }
            }
        }
    }

    if let Some(instances) = table.get("instance").and_then(|p| p.as_table()) {
        for (name, i) in instances {
            let Some(i) = i.as_table() else { continue };
            let key = |k: &str| format!("instance.{name}.{k}");
            if let Some(r) = i.get("role").and_then(str_of) {
                if parse_role(&r).0 == ConnectionRole::Workbench {
                    push(key("role"), q("workbench"));
                }
            }
            if let Some(v) = i
                .get("service_username")
                .and_then(str_of)
                .filter(|v| !v.is_empty())
            {
                push(key("service_username"), q(&v));
            }
            if let Some(v) = i.get("ssh_host").and_then(str_of).filter(|v| !v.is_empty()) {
                push(key("ssh_host"), q(&v));
            }
            if flag(i, "tls_verify") == Some(false) {
                push(key("tls_verify"), "false".to_string());
            }
        }
    }

    // The address each setting applies to. The default server's row is the top level plus
    // `policy.default`; every other server is `instance.<name>`/`policy.<name>`.
    let mut out: Vec<String> = Vec::new();
    let default_row = |k: &str| {
        !k.contains('.') || k.starts_with("policy.default.") || k.starts_with("instance.default.")
    };
    if lines.iter().any(|(k, _)| default_row(k)) {
        out.extend(default_address_lines(table));
    }
    if let Some(instances) = table.get("instance").and_then(|p| p.as_table()) {
        for (name, i) in instances {
            let Some(i) = i.as_table() else { continue };
            let p1 = format!("policy.{name}.");
            let p2 = format!("instance.{name}.");
            if lines
                .iter()
                .any(|(k, _)| k.starts_with(&p1) || k.starts_with(&p2))
            {
                out.extend(instance_address_lines(name, i));
            }
        }
    }
    if lines.is_empty() {
        return Vec::new();
    }
    out.extend(lines.into_iter().map(|(k, v)| format!("{k} = {v}")));
    out.sort();
    out.dedup();
    out
}

fn split_line(line: &str) -> (&str, &str) {
    line.split_once(" = ").unwrap_or((line, ""))
}

fn same_path(entry: &str, path: &Path) -> bool {
    let e = Path::new(entry);
    e == path || canon(e) == canon(path)
}

/// The `[[trust]]` entries of the operator file's table.
pub fn trust_entries(operator: &toml::Table) -> Vec<TrustEntry> {
    operator
        .get("trust")
        .and_then(|t| t.as_array())
        .map(|a| {
            a.iter()
                .filter_map(|e| {
                    let e = e.as_table()?;
                    Some(TrustEntry {
                        path: e.get("path")?.as_str()?.to_string(),
                        format: e
                            .get("format")
                            .and_then(|f| f.as_integer())
                            .and_then(|f| u32::try_from(f).ok())
                            .unwrap_or(0),
                        settings: e
                            .get("settings")
                            .and_then(|s| s.as_array())
                            .map(|s| {
                                s.iter()
                                    .filter_map(|x| x.as_str().map(str::to_string))
                                    .collect()
                            })
                            .unwrap_or_default(),
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

/// Trust state of the file at `path` holding `table`, against the operator file's entries.
pub fn trust_state(path: &Path, table: &toml::Table, entries: &[TrustEntry]) -> TrustState {
    let current = loosening_settings(table);
    if current.is_empty() {
        return TrustState::NotNeeded;
    }
    let Some(entry) = entries.iter().find(|e| same_path(&e.path, path)) else {
        return TrustState::Untrusted;
    };
    if entry.format != TRUST_FORMAT {
        return TrustState::Lapsed {
            changed: vec![Change {
                setting: "format".to_string(),
                was: Some(entry.format.to_string()),
                now: Some(TRUST_FORMAT.to_string()),
            }],
        };
    }
    let mut trusted = entry.settings.clone();
    trusted.sort();
    trusted.dedup();
    if trusted == current {
        return TrustState::Trusted;
    }
    let was: BTreeMap<&str, &str> = trusted.iter().map(|l| split_line(l)).collect();
    let now: BTreeMap<&str, &str> = current.iter().map(|l| split_line(l)).collect();
    let keys: BTreeSet<&str> = was.keys().chain(now.keys()).copied().collect();
    let changed = keys
        .into_iter()
        .filter(|k| was.get(k) != now.get(k))
        .map(|k| Change {
            setting: k.to_string(),
            was: was.get(k).map(|v| v.to_string()),
            now: now.get(k).map(|v| v.to_string()),
        })
        .collect();
    TrustState::Lapsed { changed }
}

// ── Operator-only keys (D4) ────────────────────────────────────────────────────────────────────

fn lookup<'a>(table: &'a toml::Table, dotted: &str) -> Option<&'a toml::Value> {
    let mut parts = dotted.split('.');
    let mut cur = table.get(parts.next()?)?;
    for p in parts {
        cur = cur.get(p)?;
    }
    Some(cur)
}

/// Operator-only keys present in a workspace-layer file's raw table, as set-aside items.
/// A key set to `false` is skipped: it asks for nothing.
pub fn operator_only_items(table: &toml::Table, file: &Path) -> Vec<SetAsideItem> {
    let per_server = policy_flag_servers(table, "execAsOsUser")
        .into_iter()
        .map(|name| SetAsideItem {
            id: format!("workspace:policy.{name}.execAsOsUser"),
            part: None,
            kind: SetAsideKind::OperatorOnly,
            setting: format!("[policy.{name}] execAsOsUser = true"),
            server: Some(name),
            file: Some(file.to_string_lossy().into_owned()),
            reason: "only the operator can set this; iad does not read it from a project file"
                .to_string(),
            lift: "the operator sets it; `iris-agentic-dev trust --show` says where".to_string(),
        });
    OPERATOR_ONLY_KEYS
        .iter()
        .filter_map(|key| {
            let v = lookup(table, key)?;
            if v.as_bool() == Some(false) {
                return None;
            }
            // The local file is what every setting of it gets without the opt-in.
            if *key == "telemetry_sink"
                && v.as_str().and_then(crate::telemetry::TelemetrySink::parse)
                    != Some(crate::telemetry::TelemetrySink::Iris)
            {
                return None;
            }
            // The token is a secret and this item is shown to the agent: never print the value.
            let setting = match *key {
                "trust" => "[[trust]]".to_string(),
                "transport.http_token" => "transport.http_token = <set>".to_string(),
                k => format!("{k} = {v}"),
            };
            Some(SetAsideItem {
                id: format!("workspace:{key}"),
                part: None,
                kind: SetAsideKind::OperatorOnly,
                setting,
                server: None,
                file: Some(file.to_string_lossy().into_owned()),
                reason: "only the operator can set this; iad does not read it from a project file"
                    .to_string(),
                lift: "the operator sets it; `iris-agentic-dev trust --show` says where"
                    .to_string(),
            })
        })
        .chain(per_server)
        .collect()
}

/// The `[policy.<name>]` sections of `table` that set `key = true`, by name.
fn policy_flag_servers(table: &toml::Table, key: &str) -> Vec<String> {
    table
        .get("policy")
        .and_then(|p| p.as_table())
        .map(|policies| {
            policies
                .iter()
                .filter(|(_, v)| v.get(key).and_then(|b| b.as_bool()) == Some(true))
                .map(|(name, _)| name.clone())
                .collect()
        })
        .unwrap_or_default()
}

/// Operator-only settings from the operator file's raw table.
pub fn operator_settings(table: &toml::Table) -> OperatorSettings {
    let b = |k: &str| table.get(k).and_then(|v| v.as_bool()).unwrap_or(false);
    OperatorSettings {
        exec_as_os_user: b("exec_as_os_user"),
        exec_as_os_user_servers: policy_flag_servers(table, "execAsOsUser"),
        http_token: lookup(table, "transport.http_token")
            .and_then(|v| v.as_str())
            .map(str::to_string)
            .filter(|s| !s.is_empty()),
        local_skills: b("local_skills"),
        audit_payloads: b("audit_payloads"),
        telemetry_sink: table
            .get("telemetry_sink")
            .and_then(|v| v.as_str())
            .and_then(crate::telemetry::TelemetrySink::parse)
            .unwrap_or_default(),
    }
}

// ── The snapshot ───────────────────────────────────────────────────────────────────────────────

/// Names with anything to say about them, `default` first.
fn server_names(tables: &[&toml::Table]) -> Vec<String> {
    let mut names: BTreeSet<String> = BTreeSet::new();
    for t in tables {
        for section in ["policy", "instance"] {
            if let Some(s) = t.get(section).and_then(|s| s.as_table()) {
                names.extend(s.keys().cloned());
            }
        }
    }
    names.remove("default");
    std::iter::once("default".to_string())
        .chain(names)
        .collect()
}

/// Notes on values that parsed but were corrected: role spellings, an unknown template, a category
/// that does not exist.
pub fn value_notes(table: &toml::Table, out: &mut Vec<String>) {
    if let Some(v) = table.get("read_only") {
        let understood = v
            .as_str()
            .is_some_and(|s| is_strict_value(s) || s.trim().eq_ignore_ascii_case("off"));
        if !understood {
            out.push(format!(
                "read_only {v} is not \"strict\" or \"off\"; it is ignored, and read-only mode is off"
            ));
        }
    }
    if let Some(v) = table.get("telemetry_sink") {
        if v.as_str()
            .and_then(crate::telemetry::TelemetrySink::parse)
            .is_none()
        {
            out.push(format!(
                "telemetry_sink {v} is not \"local\" or \"iris\"; it is ignored, and telemetry stays in the local file"
            ));
        }
    }
    if let Some(instances) = table.get("instance").and_then(|i| i.as_table()) {
        for (name, i) in instances {
            if let Some(r) = i.get("role").and_then(|r| r.as_str()) {
                if let Some(note) = parse_role(r).1 {
                    out.push(format!("[instance.{name}] {note}"));
                }
            }
        }
    }
    if let Some(policies) = table.get("policy").and_then(|p| p.as_table()) {
        for (name, p) in policies {
            if let Some(t) = p.get("mcpTemplate").and_then(|v| v.as_str()) {
                if !["dev", "test", "live"].contains(&t.trim().to_ascii_lowercase().as_str()) {
                    out.push(format!(
                        "[policy.{name}] mcpTemplate \"{t}\" is not dev, test or live; it is treated as live"
                    ));
                }
            }
            if let Some(d) = p.get("dataPolicy").and_then(|v| v.as_str()) {
                if !["block", "redact", "allow"].contains(&d.trim().to_ascii_lowercase().as_str()) {
                    out.push(format!(
                        "[policy.{name}] dataPolicy \"{d}\" is not block, redact or allow; it is treated as block"
                    ));
                }
            }
            if let Some(items) = p.get("allow").and_then(|v| v.as_array()) {
                for c in items.iter().filter_map(|v| v.as_str()) {
                    if c.trim()
                        .to_ascii_lowercase()
                        .parse::<crate::iris::workspace_config::ToolCategory>()
                        .is_err()
                    {
                        out.push(format!(
                            "[policy.{name}] allow: \"{c}\" is not a tool category; it is ignored"
                        ));
                    }
                }
            }
        }
    }
}

/// The file as the snapshot holds it, not as it reads on disk now: a held edit to the operator
/// file must not reach a reader that goes back to the path.
fn table_text(file: &LoadedFile) -> String {
    toml::to_string(&file.table).unwrap_or_default()
}

fn fleet_of(file: &LoadedFile) -> FleetConfig {
    load_fleet_config_from_str(&table_text(file)).unwrap_or_default()
}

/// The set-aside items for loosening edits held until the next start.
fn held_items(held: &[Held], operator_file: Option<&Path>) -> Vec<SetAsideItem> {
    held.iter()
        .map(|h| SetAsideItem {
            id: h.id.clone(),
            part: None,
            kind: SetAsideKind::HeldForRestart,
            setting: h.setting.clone(),
            server: None,
            file: operator_file.map(|p| p.to_string_lossy().into_owned()),
            reason: "this edit loosens a setting, and a running server applies only edits that \
                     tighten"
                .to_string(),
            lift: "restart iad".to_string(),
        })
        .collect()
}

impl ConfigSnapshot {
    pub fn load(inputs: &LoadInputs) -> ConfigSnapshot {
        let mut snap = ConfigSnapshot {
            host_env_set: inputs.host_env.is_some(),
            ..Default::default()
        };
        let home = inputs.home.as_deref().map(canon);
        snap.project_root = crate::tools::host_fs::project_root(inputs);

        // The operator file: read whether or not a workspace file exists.
        let operator_path = home.as_ref().map(|h| h.join(CONFIG_FILE_NAME));
        snap.operator_path = operator_path.clone();
        let mut operator_unreadable = false;
        if let Some(p) = &operator_path {
            match read_file(p, FileKind::Operator, None) {
                Read::Ok(mut f) => {
                    if let Some(base) = &inputs.operator_baseline {
                        let (table, held) = hold_loosening(base, std::mem::take(&mut f.table));
                        f.table = table;
                        snap.held = held;
                    }
                    snap.operator_file = Some(f);
                }
                Read::Failed(e) => {
                    operator_unreadable = true;
                    snap.errors.push(e);
                }
                Read::Missing => {
                    // A deleted file loosens whatever it tightened.
                    if let Some(base) = &inputs.operator_baseline {
                        let (table, held) = hold_loosening(base, toml::Table::new());
                        if !held.is_empty() {
                            snap.held = held;
                            snap.operator_file = Some(LoadedFile {
                                path: p.clone(),
                                kind: FileKind::Operator,
                                named_by: None,
                                table,
                            });
                        }
                    }
                    let legacy = home.as_ref().map(|h| h.join(LEGACY_FILE_NAME));
                    if legacy.is_some_and(|l| l.exists()) {
                        snap.config_errors.push(
                            "the legacy file ~/.iris-dev.toml is no longer read; rename it to \
                             .iris-agentic-dev.toml in the home directory"
                                .to_string(),
                        );
                    }
                }
            }
        }
        snap.set_aside
            .extend(held_items(&snap.held, operator_path.as_deref()));
        if let Some(op) = &snap.operator_file {
            snap.trust_entries = trust_entries(&op.table);
            snap.operator = operator_settings(&op.table);
            value_notes(&op.table, &mut snap.config_errors);
        }

        // The workspace-layer file: at most one.
        let (cand, watch_dir) = candidate(inputs, home.as_deref());
        snap.watch_dir = watch_dir;
        if let Some((path, kind, by)) = cand {
            let same_as_operator = operator_path
                .as_ref()
                .is_some_and(|op| path.exists() && canon(&path) == canon(op));
            if !same_as_operator {
                snap.workspace_candidate = Some(canon(&path));
                match read_file(&path, kind, by) {
                    Read::Ok(f) => snap.workspace_layer = Some(f),
                    Read::Failed(e) => snap.errors.push(e),
                    Read::Missing => snap
                        .config_errors
                        .push(format!("{} does not exist", path.display())),
                }
            }
        }

        // Trust.
        if let Some(ws) = &snap.workspace_layer {
            snap.workspace_trust = Some(if operator_unreadable {
                // The trust entries cannot be read, so nothing is trusted.
                match loosening_settings(&ws.table).is_empty() {
                    true => TrustState::NotNeeded,
                    false => TrustState::Untrusted,
                }
            } else {
                trust_state(&ws.path, &ws.table, &snap.trust_entries)
            });
            value_notes(&ws.table, &mut snap.config_errors);
        } else if snap.workspace_candidate.is_some() {
            snap.workspace_trust = Some(TrustState::Untrusted);
        }

        snap.read_only_strict = [snap.operator_file.as_ref(), snap.workspace_layer.as_ref()]
            .into_iter()
            .flatten()
            .find(|f| {
                f.table
                    .get("read_only")
                    .and_then(|v| v.as_str())
                    .is_some_and(is_strict_value)
            })
            .map(|f| f.path.display().to_string())
            .or_else(|| {
                inputs
                    .launch_read_only_strict
                    .then(|| "IAD_READ_ONLY".to_string())
            });

        snap.build(inputs);
        snap
    }

    fn build(&mut self, inputs: &LoadInputs) {
        let trust = self.trust();
        let op_table = self.operator_file.as_ref().map(|f| f.table.clone());
        let ws_table = self.workspace_layer.as_ref().map(|f| f.table.clone());
        let op_path = self.operator_file.as_ref().map(|f| f.path.clone());
        let ws_path = self.workspace_layer.as_ref().map(|f| f.path.clone());
        let tables: Vec<&toml::Table> = [op_table.as_ref(), ws_table.as_ref()]
            .into_iter()
            .flatten()
            .collect();
        let names = server_names(&tables);

        // Operator-only keys in the workspace-layer file, whatever its trust state.
        if let (Some(t), Some(p)) = (&ws_table, &ws_path) {
            self.set_aside.extend(operator_only_items(t, p));
        }

        let launch_srv = inputs.launch_server();

        // Instances: operator-side names first; a workspace name that is taken is dropped.
        let mut fleet = FleetConfig::default();
        let op_fleet = self.operator_file.as_ref().map(fleet_of);
        let ws_fleet = self.workspace_layer.as_ref().map(fleet_of);
        if let Some(f) = &op_fleet {
            fleet.instance.extend(f.instance.clone());
        }
        let mut shadowed: BTreeSet<String> = BTreeSet::new();
        if let Some(f) = &ws_fleet {
            for (name, inst) in &f.instance {
                if fleet.instance.contains_key(name) {
                    shadowed.insert(name.clone());
                    self.set_aside.push(SetAsideItem {
                        id: format!("workspace:instance.{name}"),
                        part: None,
                        kind: SetAsideKind::ShadowedServer,
                        setting: format!("[instance.{name}]"),
                        server: Some(name.clone()),
                        file: ws_path.as_ref().map(|p| p.to_string_lossy().into_owned()),
                        reason: format!(
                            "the workspace file redefines server {name}, which the operator \
                             file already defines"
                        ),
                        lift: "rename the server in the workspace file".to_string(),
                    });
                } else {
                    let mut inst = inst.clone();
                    // Loosening settings count from a trusted file only (they are listed in
                    // `set_aside` otherwise): turning certificate checks off, and choosing the
                    // account the agent's code runs as. The service-account drop is reported by
                    // `connection_loosening_items`, which lists `service_username`.
                    if !trust.is_trusted() {
                        if inst.tls_verify == Some(false) {
                            inst.tls_verify = None;
                        }
                        inst.service_username = None;
                        inst.service_password = None;
                    }
                    fleet.instance.insert(name.clone(), inst);
                }
            }
        }
        fleet.mode = ws_fleet
            .as_ref()
            .and_then(|f| f.mode.clone())
            .or_else(|| op_fleet.as_ref().and_then(|f| f.mode.clone()));

        // The workspace layer's connection settings, after the trust filter.
        let chosen = self
            .workspace_layer
            .clone()
            .or_else(|| self.operator_file.clone());
        let ws_chooses_default = ws_table
            .as_ref()
            .is_some_and(|t| t.get("host").is_some() || t.get("container").is_some());
        let ws_host = ws_table
            .as_ref()
            .and_then(|t| t.get("host").and_then(str_of));
        let default_binding = match (&ws_table, ws_chooses_default) {
            (Some(_), true) if trust.is_trusted() => Binding::TrustedFileChosen,
            (Some(t), true) => Binding::WorkspaceChosen {
                may_receive_credentials: ws_host
                    .as_deref()
                    .map(|_| {
                        endpoint_of(t, launch_srv.as_ref())
                            .is_some_and(|e| env_credentials_may_go_to(&e, launch_srv.as_ref()))
                    })
                    .unwrap_or(true),
            },
            _ => Binding::OperatorSide,
        };

        // The effective policy of every server.
        let launch = inputs.launch_policy();
        let mut declared_items: Vec<SetAsideItem> = Vec::new();
        let mut policies = BTreeMap::new();
        let mut declared = DeclaredGates::default();
        for name in &names {
            let is_default = name == "default";
            let op_layer = op_table
                .as_ref()
                .map(|t| PolicyLayer::from_table(t, name, is_default));
            let ws_layer = ws_table
                .as_ref()
                .map(|t| PolicyLayer::from_table(t, name, is_default));
            let binding = if is_default {
                default_binding
            } else {
                // A server only the workspace file defines is the workspace's choice.
                let ws_defines = ws_table
                    .as_ref()
                    .and_then(|t| t.get("instance")?.get(name))
                    .and_then(|i| i.as_table());
                match ws_defines {
                    Some(i)
                        if !shadowed.contains(name)
                            && !op_fleet
                                .as_ref()
                                .is_some_and(|f| f.instance.contains_key(name)) =>
                    {
                        if trust.is_trusted() {
                            Binding::TrustedFileChosen
                        } else {
                            Binding::WorkspaceChosen {
                                may_receive_credentials: endpoint_of(i, launch_srv.as_ref())
                                    .is_some_and(|e| {
                                        env_credentials_may_go_to(&e, launch_srv.as_ref())
                                    }),
                            }
                        }
                    }
                    _ => Binding::OperatorSide,
                }
            };
            let launch_for = if is_default {
                launch
            } else {
                LaunchPolicy {
                    compile_gate: launch.compile_gate.filter(|on| *on),
                    ..Default::default()
                }
            };
            self.layers.insert(
                name.clone(),
                StoredLayers {
                    is_default,
                    operator: op_layer.clone(),
                    workspace: ws_layer.clone(),
                    binding,
                    launch: launch_for,
                },
            );
            let eff = effective_policy(&PolicyInputs {
                server: name,
                is_default,
                operator: op_layer.as_ref(),
                workspace: ws_layer.as_ref(),
                trust: &trust,
                binding,
                launch: launch_for,
                system_template: None,
                operator_file: op_path.as_deref(),
                workspace_file: ws_path.as_deref(),
            });
            if is_default {
                // The file-only declaration the write gates resolve against launch settings.
                let files_only = effective_policy(&PolicyInputs {
                    launch: LaunchPolicy::default(),
                    server: name,
                    is_default,
                    operator: op_layer.as_ref(),
                    workspace: ws_layer.as_ref(),
                    trust: &trust,
                    binding,
                    system_template: None,
                    operator_file: op_path.as_deref(),
                    workspace_file: ws_path.as_deref(),
                });
                let gate_source = |from: PartSource| match from {
                    PartSource::OperatorFile => Some(GateSource::OperatorFile),
                    PartSource::TrustedFile | PartSource::WorkspaceFile => {
                        Some(GateSource::ConfigFile)
                    }
                    _ => None,
                };
                declared = DeclaredGates {
                    write_tools_enabled: files_only.write_tools.value,
                    destructive_tools_enabled: files_only.destructive_tools.value,
                    write_source: gate_source(files_only.write_tools.from),
                    destructive_source: gate_source(files_only.destructive_tools.from),
                    withhold_launch_loosening: binding.withholds_launch_loosening(),
                    read_only_strict: false,
                };
            }
            declared_items.extend(eff.set_aside.iter().cloned());
            if let Some(inst) = fleet.instance.get_mut(name) {
                inst.role = eff.role.value;
            }
            fleet
                .policies
                .insert(name.clone(), eff.to_connection_policy());
            policies.insert(name.clone(), eff);
        }
        for item in declared_items {
            if !self.set_aside.iter().any(|i| i.id == item.id) {
                self.set_aside.push(item);
            }
        }

        // Settings that never reach a policy part: certificate checks, SSH, service accounts.
        if let (Some(t), Some(p)) = (&ws_table, &ws_path) {
            if !trust.is_trusted() {
                self.set_aside
                    .extend(connection_loosening_items(t, p, &trust));
            }
        }

        // Invalid snapshot: nothing is trusted, both gates close.
        if self.is_invalid() {
            declared = DeclaredGates {
                write_tools_enabled: Some(false),
                destructive_tools_enabled: Some(false),
                write_source: Some(GateSource::FailClosed),
                destructive_source: Some(GateSource::FailClosed),
                withhold_launch_loosening: false,
                read_only_strict: false,
            };
        }
        // Set once, after both branches: strict closes the gates whatever the files declared, and
        // an invalid snapshot is closed already.
        declared.read_only_strict = self.read_only_strict.is_some();
        self.declared = declared;
        self.policies = policies;

        // Fleet extras.
        if let Some(f) = chosen {
            // `fleet.workspace` describes the file as written; `connection_config` is what applies.
            if let Ok(raw) = toml::Value::Table(f.table.clone()).try_into::<WorkspaceConfig>() {
                fleet.workspace = raw;
            }
            if let Some(cfg) = filtered_workspace_config(&f, &trust, f.kind == FileKind::Operator) {
                if !self.is_invalid() {
                    self.connection_config = Some((cfg, f.path.clone(), f.kind));
                }
            }
        }
        self.fleet = fleet;

        // Credentials the file's host does not get.
        if let Some((cfg, path, kind)) = &self.connection_config {
            let launch = inputs.launch_server();
            let file = match kind {
                FileKind::Operator => self.operator_file.as_ref(),
                _ => self.workspace_layer.as_ref(),
            };
            let endpoint = file.and_then(|f| endpoint_of(&f.table, launch.as_ref()));
            if let (Some(_), Some(endpoint)) = (&cfg.host, endpoint) {
                let withheld = crate::iris::workspace_config::withheld_env_credentials(
                    cfg,
                    &endpoint,
                    launch.as_ref(),
                    &|key| match &inputs.env_credentials_set {
                        Some(set) => set.iter().any(|k| k == key),
                        None => std::env::var(key).is_ok_and(|v| !v.is_empty()),
                    },
                );
                if !withheld.is_empty() {
                    self.set_aside.push(SetAsideItem {
                        id: "default:credentials".to_string(),
                        part: None,
                        kind: SetAsideKind::CredentialWithheld,
                        setting: withheld.join(" and "),
                        server: Some("default".to_string()),
                        file: Some(path.to_string_lossy().into_owned()),
                        reason: crate::iris::workspace_config::credentials_withheld_note(
                            &withheld, &endpoint,
                        ),
                        lift: "put username and password in the file, or point IRIS_HOST \
                               (scheme, port, prefix) at the file's server"
                            .to_string(),
                    });
                }
            }
        }
    }

    /// Do the scratch-class tools stay off? (R5 C15.)
    pub fn read_only_strict(&self) -> bool {
        self.read_only_strict.is_some()
    }

    /// The file or launch setting that asked for strict read-only.
    pub fn read_only_source(&self) -> Option<String> {
        self.read_only_strict.clone()
    }

    pub fn is_invalid(&self) -> bool {
        !self.errors.is_empty()
    }

    /// The workspace-layer file's trust state (`NotNeeded` when there is none).
    pub fn trust(&self) -> TrustState {
        self.workspace_trust
            .clone()
            .unwrap_or(TrustState::NotNeeded)
    }

    /// The policy a call on `server` is judged by. A server nobody wrote anything about gets the
    /// strict defaults.
    pub fn effective_policy(&self, server: &str) -> EffectivePolicy {
        self.policies
            .get(server)
            .cloned()
            .unwrap_or_else(|| EffectivePolicy {
                server: server.to_string(),
                ..Default::default()
            })
    }

    /// [`Self::effective_policy`] once the server's system mode is known: `system_template` is what
    /// it implies, and an untrusted file's template is compared with that instead of with `live`.
    pub fn effective_policy_for(
        &self,
        server: &str,
        system_template: Option<McpTemplate>,
    ) -> EffectivePolicy {
        let Some(l) = self.layers.get(server) else {
            return self.effective_policy(server);
        };
        effective_policy(&PolicyInputs {
            server,
            is_default: l.is_default,
            operator: l.operator.as_ref(),
            workspace: l.workspace.as_ref(),
            trust: &self.trust(),
            binding: l.binding,
            launch: l.launch,
            system_template,
            operator_file: self.operator_file.as_ref().map(|f| f.path.as_path()),
            workspace_file: self.workspace_layer.as_ref().map(|f| f.path.as_path()),
        })
    }

    /// The set-aside list as it stands for a server whose policy was built with
    /// [`Self::effective_policy_for`]: the template items come from `policy`, because whether a
    /// template was set aside depends on the system mode.
    pub fn set_aside_for(&self, server: &str, policy: &EffectivePolicy) -> Vec<SetAsideItem> {
        let mut v: Vec<SetAsideItem> = self
            .set_aside
            .iter()
            .filter(|i| !(i.part == Some("mcp_template") && i.server.as_deref() == Some(server)))
            .cloned()
            .collect();
        v.extend(
            policy
                .set_aside
                .iter()
                .filter(|i| i.part == Some("mcp_template"))
                .cloned(),
        );
        v
    }

    /// The gate declaration of the default server, after trust and layering. Launch settings are
    /// not in it: they are read first when the gate is resolved.
    pub fn declared_gates(&self) -> DeclaredGates {
        self.declared
    }

    /// Does either file have a `[policy.<server>]` block?
    pub fn has_policy_section(&self, server: &str) -> bool {
        [&self.operator_file, &self.workspace_layer]
            .into_iter()
            .flatten()
            .any(|f| {
                f.table
                    .get("policy")
                    .and_then(|p| p.get(server))
                    .is_some_and(|p| p.is_table())
            })
    }

    /// A contradictory gate declaration: one file that sets `destructive` on and `write` off.
    /// Judged per file, as written. Across files the destructive tier simply cannot outlive a
    /// closed write gate (`resolve_declared` clamps it), so a project file cannot make iad refuse
    /// to start by disagreeing with the operator file.
    ///
    /// Strict read-only with `irisAudit = true` on any server is the one cross-file case: the audit
    /// emission compiles a class on the server, so the two settings cannot both hold (R5 C15).
    pub fn gate_error(&self) -> Option<GateConfigError> {
        [&self.operator_file, &self.workspace_layer]
            .into_iter()
            .flatten()
            .find_map(|f| {
                let flag = |k: &str| f.table.get(k).and_then(|v| v.as_bool());
                validate_declared_gates(&DeclaredGates {
                    write_tools_enabled: flag("write_tools_enabled"),
                    destructive_tools_enabled: flag("destructive_tools_enabled"),
                    ..Default::default()
                })
                .err()
            })
            .or_else(|| {
                (self.read_only_strict() && self.policies.values().any(|p| p.audit.value))
                    .then_some(GateConfigError::ReadOnlyStrictWithAudit)
            })
    }

    /// Does the file in `connection_config` choose the connection? Only a file that names a host or
    /// a container can (T194). The operator file yields to `IRIS_HOST`; a workspace-layer file does
    /// not (spec 140).
    pub fn file_chooses_connection(&self) -> bool {
        match &self.connection_config {
            Some((cfg, _, kind)) => {
                crate::iris::workspace_config::names_a_target(cfg)
                    && crate::iris::workspace_config::file_chooses_connection(
                        *kind,
                        self.host_env_set,
                    )
            }
            None => false,
        }
    }

    /// The path of the file in `connection_config` when it was read and names no server.
    pub fn file_naming_no_target(&self) -> Option<&Path> {
        self.connection_config
            .as_ref()
            .filter(|(cfg, _, _)| !crate::iris::workspace_config::names_a_target(cfg))
            .map(|(_, path, _)| path.as_path())
    }

    /// The path iad watches and tells the agent about. Never the operator file.
    pub fn config_watch_path(&self) -> Option<PathBuf> {
        if let Some(p) = &self.workspace_candidate {
            return Some(p.clone());
        }
        self.watch_dir.as_ref().map(|d| d.join(CONFIG_FILE_NAME))
    }

    /// The one line iad writes to stderr when it set settings aside (FR-042), or `None`.
    pub fn startup_notice(&self) -> Option<String> {
        if self.set_aside.is_empty() {
            return None;
        }
        let n = self.set_aside.len();
        let why = match (&self.workspace_trust, &self.workspace_layer) {
            (Some(TrustState::Untrusted), Some(f)) => {
                format!("the workspace file {} is not trusted", f.path.display())
            }
            (Some(TrustState::Lapsed { .. }), Some(f)) => {
                format!("trust in {} lapsed", f.path.display())
            }
            _ => "see check_config for each reason".to_string(),
        };
        Some(format!(
            "iris-agentic-dev: {n} setting{} set aside ({why}); check_config lists them, and \
             `iris-agentic-dev trust --show` explains them",
            if n == 1 { "" } else { "s" }
        ))
    }

    /// Print [`Self::startup_notice`] to stderr, once per process. Plain `eprintln!`, not tracing:
    /// `RUST_LOG` can silence tracing, and an operator who opens a repository and finds writes
    /// off should not have to know that.
    pub fn announce(&self) {
        static ONCE: std::sync::Once = std::sync::Once::new();
        if let Some(line) = self.startup_notice() {
            ONCE.call_once(|| eprintln!("{line}"));
        }
    }

    /// The `config_sources` object of `check_config`.
    pub fn sources_json(&self) -> serde_json::Value {
        let operator = serde_json::json!({
            "path": self.operator_path.as_ref().map(|p| p.to_string_lossy().into_owned()),
            "loaded": self.operator_file.is_some(),
        });
        let workspace = match (&self.workspace_layer, &self.workspace_candidate) {
            (Some(f), _) => {
                let trust = self.trust();
                let mut o = serde_json::json!({
                    "path": f.path.to_string_lossy(),
                    "kind": match f.kind { FileKind::Named => "named_config_file", _ => "workspace_file" },
                    "named_by": f.named_by.map(|b| b.as_str()),
                    "trust": trust.as_str(),
                });
                if let TrustState::Lapsed { changed } = &trust {
                    o["changed"] = serde_json::json!(changed
                        .iter()
                        .map(|c| serde_json::json!({
                            "setting": c.setting, "was": c.was, "now": c.now
                        }))
                        .collect::<Vec<_>>());
                }
                if !matches!(trust, TrustState::Trusted | TrustState::NotNeeded) {
                    o["lift"] = serde_json::json!(trust_command_hint(Some(&f.path)));
                }
                o
            }
            (None, Some(p)) => serde_json::json!({
                "path": p.to_string_lossy(),
                "kind": "workspace_file",
                "named_by": null,
                "trust": "untrusted",
            }),
            _ => serde_json::Value::Null,
        };
        serde_json::json!({ "operator_file": operator, "workspace_file": workspace })
    }
}

impl Binding {
    fn withholds_launch_loosening(&self) -> bool {
        matches!(
            self,
            Binding::WorkspaceChosen {
                may_receive_credentials: false
            }
        )
    }
}

/// The file's connection settings with the ones that need trust removed.
fn filtered_workspace_config(
    f: &LoadedFile,
    trust: &TrustState,
    is_operator: bool,
) -> Option<WorkspaceConfig> {
    let mut cfg: WorkspaceConfig = toml::Value::Table(f.table.clone()).try_into().ok()?;
    // The gates travel as `DeclaredGates`, resolved with trust; the config never carries them on.
    cfg.write_tools_enabled = None;
    cfg.destructive_tools_enabled = None;
    if !is_operator && !trust.is_trusted() {
        if cfg.tls_verify == Some(false) {
            cfg.tls_verify = None;
        }
        cfg.ssh_host = None;
    }
    Some(cfg)
}

/// Loosening settings that have no policy part: set aside with the file unchanged.
fn connection_loosening_items(
    table: &toml::Table,
    file: &Path,
    trust: &TrustState,
) -> Vec<SetAsideItem> {
    let reason = match trust {
        TrustState::Lapsed { changed } => format!(
            "trust lapsed: {}",
            changed
                .iter()
                .map(|c| c.setting.clone())
                .collect::<Vec<_>>()
                .join(", ")
        ),
        _ => "the file is not trusted".to_string(),
    };
    let item = |key: String, setting: String, server: Option<String>| SetAsideItem {
        id: format!("workspace:{key}"),
        part: None,
        kind: SetAsideKind::UntrustedSetting,
        setting,
        server,
        file: Some(file.to_string_lossy().into_owned()),
        reason: reason.clone(),
        lift: trust_command_hint(Some(file)),
    };
    let mut out = Vec::new();
    for (key, line) in loosening_pairs_for_connection(table) {
        let server = key
            .strip_prefix("instance.")
            .and_then(|r| r.split('.').next())
            .map(str::to_string)
            .or_else(|| Some("default".to_string()));
        out.push(item(key.clone(), line, server));
    }
    out
}

/// `tls_verify = false`, `ssh_host` and `service_username`, top level and per instance.
fn loosening_pairs_for_connection(table: &toml::Table) -> Vec<(String, String)> {
    let mut v = Vec::new();
    if table.get("tls_verify").and_then(|x| x.as_bool()) == Some(false) {
        v.push(("tls_verify".to_string(), "tls_verify = false".to_string()));
    }
    for k in ["ssh_host", "service_username"] {
        if let Some(s) = table.get(k).and_then(str_of).filter(|s| !s.is_empty()) {
            v.push((k.to_string(), format!("{k} = {}", q(&s))));
        }
    }
    if let Some(instances) = table.get("instance").and_then(|i| i.as_table()) {
        for (name, i) in instances {
            let Some(i) = i.as_table() else { continue };
            if i.get("tls_verify").and_then(|x| x.as_bool()) == Some(false) {
                let k = format!("instance.{name}.tls_verify");
                v.push((k.clone(), format!("{k} = false")));
            }
            for f in ["ssh_host", "service_username"] {
                if let Some(s) = i.get(f).and_then(str_of).filter(|s| !s.is_empty()) {
                    let k = format!("instance.{name}.{f}");
                    v.push((k.clone(), format!("{k} = {}", q(&s))));
                }
            }
        }
    }
    v
}

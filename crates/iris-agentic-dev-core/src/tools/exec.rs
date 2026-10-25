//! Running the agent's code as a named identity (spec 141, R2).
//!
//! Every tool that runs code the agent wrote (`iris_execute`, `iris_query` writes, `iris_global`
//! set/kill, `iris_execute_method`, `global_kill`, `iris_test`, `iris_coverage` run/start and the
//! WebSocket terminal) reaches IRIS through this module. A handler never picks a connection, a
//! client or a container: [`IrisTools::exec_context`] reads them from the call's resolved target
//! (R1) and [`run_code`] runs the code as the identity that target resolved to, or refuses.
//!
//! Three paths exist and each names who ran:
//!
//! - **Atelier (HTTP)**: Basic auth with the identity's credentials, through a cookie jar of that
//!   identity's own. The generated class returns `$USERNAME` ahead of the output.
//! - **Terminal (`docker exec`, SSH)**: `iris session` takes no user name under OS
//!   authentication, so the first stdin line drops to the service account inside IRIS
//!   ([`build_login_guard`]). It ends the process when the drop fails, and prints an `IAD-ID:`
//!   marker that [`split_identity_marker`] reads back.
//! - **WebSocket terminal**: the service account's own credentials; `$USERNAME` is probed once
//!   after the prompt.

use crate::iris::config_sources::ConfigSnapshot;
use crate::iris::connection::{code_may_have_run, IrisConnection, TerminalTarget};
use serde_json::{json, Map, Value};
use std::sync::Arc;
use std::time::Duration;

// ── codes ─────────────────────────────────────────────────────────────────────────────────────

pub const ERR_SERVICE_ACCOUNT_LOGIN_FAILED: &str = "SERVICE_ACCOUNT_LOGIN_FAILED";
pub const ERR_SERVICE_ACCOUNT_IDENTITY_MISMATCH: &str = "SERVICE_ACCOUNT_IDENTITY_MISMATCH";
pub const ERR_SERVICE_ACCOUNT_TERMINAL_DENIED: &str = "SERVICE_ACCOUNT_TERMINAL_DENIED";
pub const ERR_SERVICE_ACCOUNT_HTTP_DENIED: &str = "SERVICE_ACCOUNT_HTTP_DENIED";
pub const ERR_SERVICE_ACCOUNT_WITHHELD: &str = "SERVICE_ACCOUNT_WITHHELD";
pub const ERR_SERVICE_ACCOUNT_UNCONFIGURED: &str = "SERVICE_ACCOUNT_UNCONFIGURED_FOR_SERVER";
pub const ERR_SERVICE_ACCOUNT_INVALID: &str = "SERVICE_ACCOUNT_INVALID";
pub const ERR_DOCKER_TERMINAL_LOGIN_REQUIRED: &str = "DOCKER_TERMINAL_LOGIN_REQUIRED";
pub const ERR_NO_DOCKER_ROUTE_FOR_SERVER: &str = "NO_DOCKER_ROUTE_FOR_SERVER";
pub const ERR_SESSION_IDENTITY_CHANGED: &str = "SESSION_IDENTITY_CHANGED";
pub const ERR_SSH_HOST_KEY_MISMATCH: &str = "SSH_HOST_KEY_MISMATCH";

/// The tools whose code is the agent's. The pipeline resolves a service account for these only; a
/// read, a compile or a document write runs as the connection's own user.
pub const SERVICE_ACCOUNT_TOOLS: &[&str] = &[
    "iris_execute",
    "iris_query",
    "iris_global",
    "iris_execute_method",
    "global_kill",
    "iris_test",
    "iris_coverage",
    "iris_ws_open",
    "iris_ws_exec",
];

fn arg<'a>(args: &'a Value, keys: &[&str]) -> Option<&'a str> {
    keys.iter()
        .find_map(|k| args.get(*k).and_then(Value::as_str))
        .map(str::trim)
}

/// Does this call run the agent's code, so that it must run as the service account?
///
/// `iris_query` does when it writes or forces; `iris_global` when it sets or kills;
/// `iris_coverage` when it starts or runs the agent's test path. Both spellings of the action
/// argument are read.
pub fn needs_service_account(tool: &str, args: &Value) -> bool {
    let action = arg(args, &["action", "mode"]).map(str::to_ascii_lowercase);
    let action = action.as_deref().unwrap_or("");
    match tool {
        "iris_execute"
        | "iris_execute_method"
        | "global_kill"
        | "iris_test"
        | "iris_ws_open"
        | "iris_ws_exec" => true,
        "iris_query" => {
            action == "write" || args.get("force").and_then(Value::as_bool) == Some(true)
        }
        "iris_global" => matches!(action, "set" | "kill"),
        "iris_coverage" => matches!(action, "run" | "start"),
        _ => false,
    }
}

// ── identity ──────────────────────────────────────────────────────────────────────────────────

/// Where the identity that ran came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IdentitySource {
    /// `IRIS_SERVICE_USERNAME` in the operator's environment.
    ServiceAccountEnv,
    /// The server's own `service_username` (`servers.json`, a trusted `[instance.*]` block).
    ServiceAccountServer,
    /// The connection's own account, or the container's OS user when no account is set.
    Primary,
    /// The container's OS user, because the operator opted in and the login guard could not work.
    OsUserOptIn,
}

impl IdentitySource {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::ServiceAccountEnv => "service_account_env",
            Self::ServiceAccountServer => "service_account_server",
            Self::Primary => "primary",
            Self::OsUserOptIn => "os_user_opt_in",
        }
    }
}

/// Which kind of target a call has, for choosing its service account.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IdentityTarget {
    /// The default connection.
    Default,
    /// The pool member built from `IRIS_HOST` (`_env`).
    EnvMember,
    /// A pool member at the default connection's address.
    AliasOfDefault,
    /// Any other pool member.
    Member,
}

#[derive(Debug, Clone, Copy)]
pub struct IdentityInputs<'a> {
    /// `IRIS_SERVICE_USERNAME`.
    pub env_account: Option<&'a str>,
    /// `credentials_withheld` names the service account: a workspace file chose the host.
    pub withheld: bool,
    pub target: IdentityTarget,
    /// The member's own `service_username`.
    pub own_account: Option<&'a str>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IdentityDecision {
    Primary,
    Service {
        user: String,
        source: IdentitySource,
    },
    Refused {
        code: &'static str,
    },
}

/// R2 section 3, as a table.
///
/// The env account belongs to the default connection and to what is the same instance. A member
/// with its own account uses it. A member with none is refused while the env account is set,
/// because falling back to the member's primary user would defeat the operator's intent, and
/// sending the env account to a host it was never paired with would leak its password.
pub fn decide_identity(i: &IdentityInputs<'_>) -> IdentityDecision {
    match i.target {
        IdentityTarget::Member | IdentityTarget::AliasOfDefault if i.own_account.is_some() => {
            IdentityDecision::Service {
                user: i.own_account.unwrap_or_default().to_string(),
                source: IdentitySource::ServiceAccountServer,
            }
        }
        IdentityTarget::Member => {
            if i.env_account.is_some() {
                IdentityDecision::Refused {
                    code: ERR_SERVICE_ACCOUNT_UNCONFIGURED,
                }
            } else {
                IdentityDecision::Primary
            }
        }
        _ => match i.env_account {
            None => IdentityDecision::Primary,
            Some(_) if i.withheld => IdentityDecision::Refused {
                code: ERR_SERVICE_ACCOUNT_WITHHELD,
            },
            Some(user) => IdentityDecision::Service {
                user: user.to_string(),
                source: IdentitySource::ServiceAccountEnv,
            },
        },
    }
}

/// A user name that can be spliced into one terminal line: not empty, no control character. A
/// newline would add a stdin line ahead of the login guard.
pub fn validate_service_username(user: &str) -> Result<(), String> {
    if user.trim().is_empty() || user.chars().any(char::is_control) {
        return Err(format!(
            "{ERR_SERVICE_ACCOUNT_INVALID}: a service account user name must be non-empty and \
             hold no control character"
        ));
    }
    Ok(())
}

// ── the opt-in to run as the container's OS user ──────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OptInSource {
    Env,
    HomeFile,
}

impl OptInSource {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Env => "env",
            Self::HomeFile => "home_file",
        }
    }
}

fn truthy(v: &str) -> bool {
    matches!(v.trim().to_ascii_lowercase().as_str(), "1" | "true" | "yes")
}

/// Is running as the container's OS user allowed for `server`? Only the operator's environment
/// (`IAD_EXEC_AS_OS_USER`, passed in as `env`) and home-directory file can say so (FR-008); the
/// snapshot already holds nothing a workspace file set.
pub fn os_user_opt_in(
    snap: &ConfigSnapshot,
    server: &str,
    env: Option<&str>,
) -> Option<OptInSource> {
    if env.is_some_and(truthy) {
        return Some(OptInSource::Env);
    }
    (snap.operator.exec_as_os_user
        || snap
            .operator
            .exec_as_os_user_servers
            .iter()
            .any(|s| s == server))
    .then_some(OptInSource::HomeFile)
}

// ── the terminal: guard and marker ────────────────────────────────────────────────────────────

/// What a terminal call writes first when no service account is set, so that every response can
/// name the OS-authenticated identity.
pub const IDENTITY_PROBE_LINE: &str = "Write \"IAD-ID:\",$USERNAME,!";

/// What a guarded terminal call writes after the agent's code. A session the terminal ended with
/// "Access Denied" never reaches it, which tells that from code that ran and printed the same text.
pub const END_PROBE_LINE: &str = "Write \"IAD-END\",!";

const ID_PREFIX: &str = "IAD-ID:";
const END_PREFIX: &str = "IAD-END";

/// The first stdin line of a terminal call that must run as `user`.
///
/// It logs in as `user` with the one-argument `$SYSTEM.Security.Login`, which needs no password
/// (the OS-authenticated caller holds the permission it checks), ends the process when that fails
/// or lands on another user, and prints the marker. One line, no braces: the terminal runs each
/// line on its own. The process cannot climb back: `Login("irisowner")` from it returns 0 (T-R2-L02).
pub fn build_login_guard(user: &str) -> String {
    let user_lit = crate::objectscript::os_lit_inner(user);
    format!(
        "Set %iad=$SYSTEM.Security.Login(\"{user_lit}\") Halt:'%iad  \
         Halt:$ZCVT($USERNAME,\"U\")'=$ZCVT(\"{user_lit}\",\"U\")  Kill %iad  Write \"IAD-ID:\",$USERNAME,!"
    )
}

/// `{}` block syntax cannot run in a terminal. `iris_execute` checks before any container is
/// reached; the terminal runs one line at a time and answers a block with `<SYNTAX>`.
fn block_syntax_refusal(code: &str, opts: &RunOpts<'_>) -> Option<ExecError> {
    (opts.check_block_syntax && crate::tools::write_gate::contains_terminal_block_syntax(code))
        .then(|| {
            ExecError::new(
                crate::tools::write_gate::ERR_TERMINAL_SYNTAX_UNSUPPORTED,
                "Code contains block-syntax (`{...}`) that is not supported in terminal (docker \
                 exec) mode. Rewrite in classic form (If cond Write x) or use iris_doc + \
                 iris_compile to write a .mac routine.",
            )
        })
}

/// A failure that says who would have run the code, or that the login was refused: not one that a
/// fallback to another path may paper over.
pub fn is_identity_failure(code: &str) -> bool {
    matches!(
        code,
        ERR_SERVICE_ACCOUNT_LOGIN_FAILED
            | ERR_SERVICE_ACCOUNT_IDENTITY_MISMATCH
            | ERR_SERVICE_ACCOUNT_TERMINAL_DENIED
            | ERR_DOCKER_TERMINAL_LOGIN_REQUIRED
            | ERR_SERVICE_ACCOUNT_HTTP_DENIED
    )
}

/// Why a terminal call's output cannot be trusted to have run as the identity asked for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MarkerError {
    /// No marker: the login failed and the process ended before the agent's code.
    LoginFailed,
    /// The terminal asks for a user name and password.
    TerminalLoginRequired,
    /// The login worked and the terminal then refused the session. Measured on 2026.2: the account
    /// needs `%Development:USE`, or the terminal prints "Access Denied" after the guard.
    TerminalDenied,
    /// The marker names another user.
    Mismatch { ran_as: String },
}

impl MarkerError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::LoginFailed => ERR_SERVICE_ACCOUNT_LOGIN_FAILED,
            Self::TerminalLoginRequired => ERR_DOCKER_TERMINAL_LOGIN_REQUIRED,
            Self::TerminalDenied => ERR_SERVICE_ACCOUNT_TERMINAL_DENIED,
            Self::Mismatch { .. } => ERR_SERVICE_ACCOUNT_IDENTITY_MISMATCH,
        }
    }

    pub fn message(&self, expected: Option<&str>) -> String {
        let who = expected.unwrap_or("the service account");
        match self {
            Self::LoginFailed => format!(
                "The terminal could not log in as {who}, so the code did not run. The account must \
                 exist, be enabled, and the container's OS user must be allowed to log in as it."
            ),
            Self::TerminalLoginRequired => "The container's terminal asks for a user name and \
                password, which iad does not send, so the code did not run. Enable operating-system \
                authentication for %Service_Terminal, or use the Atelier REST path."
                .to_string(),
            Self::TerminalDenied => format!(
                "The terminal logged in as {who} and then refused the session, so the code did not \
                 run. The account needs %Development:USE (and read access to the namespace's \
                 databases) to use the terminal."
            ),
            Self::Mismatch { ran_as } => format!(
                "The terminal reports it ran as {ran_as}, not as {who}."
            ),
        }
    }
}

impl std::fmt::Display for MarkerError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.code(), self.message(None))
    }
}

impl std::error::Error for MarkerError {}

/// The terminal output with the identity lines taken out, and who the marker named.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IdentityMarker {
    pub user: Option<String>,
    pub rest: String,
}

/// Read the `IAD-ID:` marker out of a terminal call's output.
///
/// With `expected` set (a service account), the marker must be there and must name that user (case
/// is not significant: IRIS user names are case-insensitive), or the call failed before the agent's
/// code. With none, a marker is reported when there is one. Only the first marker counts: the guard
/// is the first line sent, so it is the first thing written, and output that imitates the marker
/// further down does not pick the identity.
pub fn split_identity_marker(
    raw: &str,
    expected: Option<&str>,
) -> Result<IdentityMarker, MarkerError> {
    let lines: Vec<&str> = raw.lines().collect();
    let marker_at = lines.iter().position(|l| l.trim().starts_with(ID_PREFIX));
    let ended = lines.iter().any(|l| l.trim() == END_PREFIX);
    let wants_login = raw.contains("Username:") || raw.contains("Password:");
    let denied_text = lines.iter().any(|l| l.trim() == "Access Denied");

    let Some(at) = marker_at else {
        if wants_login || (denied_text && expected.is_some()) {
            return Err(MarkerError::TerminalLoginRequired);
        }
        return match expected {
            Some(_) => Err(MarkerError::LoginFailed),
            None => Ok(IdentityMarker {
                user: None,
                rest: raw.to_string(),
            }),
        };
    };
    let user = lines[at].trim()[ID_PREFIX.len()..].trim().to_string();
    if let Some(want) = expected {
        if !user.eq_ignore_ascii_case(want) {
            return Err(MarkerError::Mismatch { ran_as: user });
        }
        if denied_text && !ended {
            return Err(MarkerError::TerminalDenied);
        }
    }
    let rest = lines
        .iter()
        .enumerate()
        .filter(|(i, l)| *i != at && l.trim() != END_PREFIX)
        .map(|(_, l)| *l)
        .collect::<Vec<_>>()
        .join("\n");
    Ok(IdentityMarker {
        user: Some(user),
        rest,
    })
}

// ── routes ────────────────────────────────────────────────────────────────────────────────────

/// Where the docker leg of a target runs. Resolved once per target (R2 section 2); `IRIS_CONTAINER`
/// is read there and nowhere below.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExecRoute {
    /// HTTP only. A failure before the code runs is reported, never run elsewhere.
    Http,
    /// HTTP first, then a docker exec in `container` for a failure before the code ran.
    HttpThenDocker {
        container: String,
        ssh_host: Option<String>,
    },
    /// `docker_only`, or a NoPWS build: no HTTP leg.
    DockerOnly {
        container: String,
        ssh_host: Option<String>,
    },
    /// `docker_only` with no container named.
    DockerOnlyNoContainer,
}

impl ExecRoute {
    pub fn container(&self) -> Option<&str> {
        match self {
            Self::HttpThenDocker { container, .. } | Self::DockerOnly { container, .. } => {
                Some(container)
            }
            _ => None,
        }
    }

    pub fn ssh_host(&self) -> Option<&str> {
        match self {
            Self::HttpThenDocker { ssh_host, .. } | Self::DockerOnly { ssh_host, .. } => {
                ssh_host.as_deref()
            }
            _ => None,
        }
    }

    pub fn path(&self) -> ExecPath {
        if self.ssh_host().is_some() {
            ExecPath::DockerExecSsh
        } else {
            ExecPath::DockerExecLocal
        }
    }

    /// `check_config`'s name for the route.
    pub fn label(&self) -> &'static str {
        match self {
            Self::Http => "http",
            Self::HttpThenDocker { .. } => "http_then_docker",
            Self::DockerOnly { .. } | Self::DockerOnlyNoContainer => "docker_only",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RouteTarget {
    /// The default connection and `_env`: both read `IRIS_CONTAINER`.
    DefaultOrEnv,
    /// Any other pool member: only its own `container`.
    Member,
}

fn non_blank(v: Option<&str>) -> Option<&str> {
    v.map(str::trim).filter(|s| !s.is_empty())
}

/// R2 section 2. A named server never inherits `IRIS_CONTAINER` (FR-009).
pub fn resolve_route(
    target: RouteTarget,
    env_container: Option<&str>,
    own_container: Option<&str>,
    docker_only: bool,
    ssh_host: Option<&str>,
) -> ExecRoute {
    let container = match target {
        RouteTarget::DefaultOrEnv => non_blank(env_container).or(non_blank(own_container)),
        RouteTarget::Member => non_blank(own_container),
    };
    let ssh_host = non_blank(ssh_host).map(str::to_string);
    match (container, docker_only) {
        (Some(c), true) => ExecRoute::DockerOnly {
            container: c.to_string(),
            ssh_host,
        },
        (Some(c), false) => ExecRoute::HttpThenDocker {
            container: c.to_string(),
            ssh_host,
        },
        (None, true) => ExecRoute::DockerOnlyNoContainer,
        (None, false) => ExecRoute::Http,
    }
}

/// The route of a connection: `docker_only` is the `http://127.0.0.1:1` sentinel, and a NoPWS build
/// has no HTTP leg either. The container (a file's, else `IRIS_CONTAINER`) is read here, once per
/// target, and nowhere below.
pub fn route_for_connection(conn: &IrisConnection, target: RouteTarget) -> ExecRoute {
    let docker_only =
        conn.base_url == "http://127.0.0.1:1" || conn.base_url.starts_with("http://127.0.0.1:1/");
    let no_pws = conn
        .version
        .as_deref()
        .is_some_and(|v| v.contains("2026.2.0AI"));
    // The container a file named for this connection, else `IRIS_CONTAINER`: both are "the default
    // connection's own", and only the default target reads either (a pool member never inherits).
    let env_container = conn.default_container();
    resolve_route(
        target,
        env_container.as_deref(),
        conn.member.container.as_deref(),
        crate::tools::nopws::nopws_is_active(docker_only, no_pws),
        conn.ssh_host.as_deref(),
    )
}

/// What to do about an HTTP execution that failed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FallbackDecision {
    /// The query was sent, so the code ran or is running: report the error as it is.
    CodeMayHaveRun,
    /// The service account is not allowed to use the Atelier API or to write the scratch class.
    /// A missing privilege is a setup error, and a terminal run would hide it (and drop block
    /// syntax and `&sql`).
    ServiceAccountHttpDenied,
    /// Run it through the route's docker leg, guarded.
    Docker,
    /// There is no docker leg. `no_docker_route` says `IRIS_CONTAINER` is set and was not used
    /// because the target is a named server.
    HttpFailed { no_docker_route: bool },
}

fn http_denied(msg: &str) -> bool {
    let lower = msg.to_ascii_lowercase();
    lower.contains("http 401") || lower.contains("http 403") || lower.contains("<protect>")
}

pub fn fallback_decision(
    service_identity: bool,
    err: &anyhow::Error,
    route: &ExecRoute,
    named_server: bool,
    env_container_set: bool,
) -> FallbackDecision {
    if code_may_have_run(err) {
        return FallbackDecision::CodeMayHaveRun;
    }
    if service_identity && http_denied(&err.to_string()) {
        return FallbackDecision::ServiceAccountHttpDenied;
    }
    match route {
        ExecRoute::HttpThenDocker { .. } | ExecRoute::DockerOnly { .. } => FallbackDecision::Docker,
        ExecRoute::Http | ExecRoute::DockerOnlyNoContainer => FallbackDecision::HttpFailed {
            no_docker_route: named_server && env_container_set,
        },
    }
}

// ── the report ────────────────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExecPath {
    Atelier,
    DockerExecLocal,
    DockerExecSsh,
    WsTerminal,
}

impl ExecPath {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Atelier => "atelier",
            Self::DockerExecLocal => "docker_exec_local",
            Self::DockerExecSsh => "docker_exec_ssh",
            Self::WsTerminal => "ws_terminal",
        }
    }
}

/// The identity that ran the code, as the response reports it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IdentityReport {
    /// What IRIS reported when `verified`, else the user name iad sent.
    pub user: String,
    pub source: IdentitySource,
    pub path: ExecPath,
    /// `user` is what IRIS said ran the code (`$USERNAME`), not what iad asked for.
    pub verified: bool,
    pub server: String,
}

impl IdentityReport {
    pub fn to_json(&self) -> Value {
        json!({
            "user": self.user,
            "source": self.source.as_str(),
            "path": self.path.as_str(),
            "verified": self.verified,
            "server": self.server,
        })
    }
}

// ── running ───────────────────────────────────────────────────────────────────────────────────

/// Everything a run needs, read from the call's resolved target.
#[derive(Clone)]
pub struct ExecContext {
    pub conn: Arc<IrisConnection>,
    pub client: Arc<reqwest::Client>,
    /// The service account's user name; `None` runs as the connection's own user.
    pub expected: Option<String>,
    pub source: IdentitySource,
    /// The pool name, `default` for the default connection.
    pub server: String,
    pub route: ExecRoute,
    pub opt_in: Option<OptInSource>,
    /// The call named a server.
    pub named_server: bool,
    pub env_container_set: bool,
    pub ssh_strict: bool,
}

pub struct RunOpts<'a> {
    pub timeout: Duration,
    /// Scratch class prefix: `IrisDevTmp.<prefix><id>`.
    pub prefix: &'a str,
    /// The code for the terminal leg, when it differs from the HTTP code (`&sql` translated). `None`
    /// means the terminal leg runs the same code; see `terminal` to switch the leg off.
    pub terminal_code: Option<&'a str>,
    /// Whether a docker leg is allowed at all. A tool that never had one keeps it that way.
    pub terminal: bool,
    /// Refuse `{}` block syntax before the terminal leg (`iris_execute` only: iad-written code is
    /// written for the terminal).
    pub check_block_syntax: bool,
}

impl<'a> RunOpts<'a> {
    pub fn http_only(timeout: Duration, prefix: &'a str) -> Self {
        Self {
            timeout,
            prefix,
            terminal_code: None,
            terminal: false,
            check_block_syntax: false,
        }
    }

    pub fn with_terminal(timeout: Duration, prefix: &'a str) -> Self {
        Self {
            terminal: true,
            ..Self::http_only(timeout, prefix)
        }
    }
}

#[derive(Debug, Clone)]
pub struct ExecOutcome {
    pub output: String,
    pub identity: IdentityReport,
}

/// A refusal or a failure, in the shape the handlers return.
#[derive(Debug, Clone)]
pub struct ExecError {
    pub code: String,
    pub message: String,
    pub extra: Map<String, Value>,
    pub identity: Option<Box<IdentityReport>>,
}

impl ExecError {
    pub fn new(code: &str, message: impl Into<String>) -> Self {
        Self {
            code: code.to_string(),
            message: message.into(),
            extra: Map::new(),
            identity: None,
        }
    }

    fn with(mut self, key: &str, value: Value) -> Self {
        self.extra.insert(key.to_string(), value);
        self
    }

    pub fn to_json(&self) -> Value {
        let mut body = json!({
            "success": false,
            "error_code": self.code,
            "error": self.message,
        });
        for (k, v) in &self.extra {
            body[k] = v.clone();
        }
        if let Some(id) = &self.identity {
            body["identity"] = id.to_json();
        }
        body
    }
}

impl std::fmt::Display for ExecError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.code, self.message)
    }
}

impl std::error::Error for ExecError {}

impl ExecContext {
    fn report(
        &self,
        user: Option<String>,
        path: ExecPath,
        source: IdentitySource,
    ) -> IdentityReport {
        let verified = user.is_some();
        IdentityReport {
            user: user
                .or_else(|| self.expected.clone())
                .unwrap_or_else(|| self.conn.username.clone()),
            source,
            path,
            verified,
            server: self.server.clone(),
        }
    }

    /// The Atelier leg: one generated class, run as this context's identity.
    pub async fn generate(
        &self,
        code: &str,
        ns: &str,
        prefix: &str,
    ) -> anyhow::Result<(String, IdentityReport)> {
        let (out, ran_as) = self
            .conn
            .execute_via_generator_identified(code, ns, &self.client, prefix)
            .await?;
        // The connection has already refused an answer from another user (`expect_user`).
        Ok((out, self.report(ran_as, ExecPath::Atelier, self.source)))
    }

    fn denied_error(&self, http_err: &str) -> ExecError {
        let who = self.expected.as_deref().unwrap_or("the service account");
        let mut e = ExecError::new(
            ERR_SERVICE_ACCOUNT_HTTP_DENIED,
            format!(
                "The Atelier REST API refused {who} ({http_err}). To run code over HTTP the \
                 account needs %Development:USE (the /api/atelier web application's resource) \
                 with password authentication on that application, read access to the \
                 namespace's databases, and write access to the database that holds the \
                 IrisDevTmp package; map IrisDevTmp to a scratch database so the application's \
                 code database can stay read-only. iad did not fall back to docker exec: that \
                 would run the code as the container's OS user."
            ),
        );
        e.extra.insert("http_error".into(), json!(http_err));
        e.identity = Some(Box::new(self.report(None, ExecPath::Atelier, self.source)));
        e
    }

    /// Run `code` as this context's identity.
    ///
    /// HTTP first unless the route is `DockerOnly`; a failure before the code ran goes to the
    /// route's docker leg when [`fallback_decision`] says so. Every outcome names the identity.
    pub async fn run(
        &self,
        code: &str,
        ns: &str,
        opts: &RunOpts<'_>,
    ) -> Result<ExecOutcome, ExecError> {
        self.run_unreported(code, ns, opts)
            .await
            .map_err(|e| self.named(e))
    }

    /// An error that does not yet say who the code would have run as says so: the user iad sent,
    /// unverified, over the path the route would have used.
    fn named(&self, mut e: ExecError) -> ExecError {
        if e.identity.is_none() {
            let path = match self.route {
                ExecRoute::DockerOnly { .. } => self.route.path(),
                _ => ExecPath::Atelier,
            };
            e.identity = Some(Box::new(self.report(None, path, self.source)));
        }
        e
    }

    async fn run_unreported(
        &self,
        code: &str,
        ns: &str,
        opts: &RunOpts<'_>,
    ) -> Result<ExecOutcome, ExecError> {
        match &self.route {
            ExecRoute::DockerOnly { .. } if opts.terminal => {
                return self.terminal_leg(code, ns, opts, None).await;
            }
            ExecRoute::DockerOnlyNoContainer if opts.terminal => {
                // The syntax guard answers first, as it always has: it needs no container.
                if let Some(e) = block_syntax_refusal(code, opts) {
                    return Err(e);
                }
                return Err(ExecError::new(
                    crate::tools::nopws::NOPWS_NO_CONTAINER,
                    "docker_only or nopws=true requires a container name. Set IRIS_CONTAINER env \
                     var or add container = \"<name>\" to .iris-agentic-dev.toml.",
                ));
            }
            _ => {}
        }

        let attempt =
            tokio::time::timeout(opts.timeout, self.generate(code, ns, opts.prefix)).await;
        let err = match attempt {
            Err(_) => {
                return Err(ExecError::new(
                    "TIMEOUT",
                    format!("execution timed out after {}s", opts.timeout.as_secs()),
                ))
            }
            Ok(Ok((output, identity))) => return Ok(ExecOutcome { output, identity }),
            Ok(Err(e)) => e,
        };
        if let Some(c) = err.downcast_ref::<crate::iris::connection::CodeMayHaveRun>() {
            if let Some(text) =
                c.0.strip_prefix(&format!("{ERR_SERVICE_ACCOUNT_IDENTITY_MISMATCH}: "))
            {
                let mut e = ExecError::new(ERR_SERVICE_ACCOUNT_IDENTITY_MISMATCH, text);
                e.identity = Some(Box::new(self.report(None, ExecPath::Atelier, self.source)));
                return Err(e);
            }
        }

        let http_err = err.to_string();
        match fallback_decision(
            self.expected.is_some(),
            &err,
            &self.route,
            self.named_server,
            self.env_container_set,
        ) {
            FallbackDecision::CodeMayHaveRun => Err(ExecError::new(
                crate::tools::ERR_QUERY_FAILED_AFTER_RUN,
                format!(
                    "{http_err}. The code ran on IRIS, or is still running, so iad did not run it \
                     again through docker exec."
                ),
            )
            .with("execution_path", json!("atelier"))),
            FallbackDecision::ServiceAccountHttpDenied => Err(self.denied_error(&http_err)),
            FallbackDecision::Docker if opts.terminal => {
                self.terminal_leg(code, ns, opts, Some(http_err)).await
            }
            FallbackDecision::Docker => Err(ExecError::new("EXECUTION_FAILED", http_err)),
            FallbackDecision::HttpFailed { no_docker_route } if opts.terminal => {
                let message = if no_docker_route {
                    format!(
                        "iris_execute: HTTP/Atelier execution failed ({http_err}). \
                         {ERR_NO_DOCKER_ROUTE_FOR_SERVER}: IRIS_CONTAINER is set but names the \
                         default connection's container, and `{}` is a named server with no \
                         container of its own, so the code was not run there. Give the server a \
                         `container` to allow a docker fallback.",
                        self.server
                    )
                } else {
                    format!(
                        "iris_execute: HTTP/Atelier execution failed ({http_err}). No docker \
                         fallback available (IRIS_CONTAINER not set). Verify the Atelier REST \
                         endpoint is reachable and the credentials have \
                         %Service_Object:USE.{}",
                        crate::tools::DOCKER_REQUIRED_HINT
                    )
                };
                let mut e = ExecError::new("HTTP_EXECUTION_FAILED", message)
                    .with("http_error", json!(http_err));
                if no_docker_route {
                    e = e.with("no_docker_route", json!(ERR_NO_DOCKER_ROUTE_FOR_SERVER));
                }
                Err(e)
            }
            FallbackDecision::HttpFailed { .. } => {
                Err(ExecError::new("EXECUTION_FAILED", http_err))
            }
        }
    }

    /// Run `code` in the target's container first and over Atelier only if the container cannot be
    /// used. `RunTest` needs the filesystem and a real terminal, so a target with a container
    /// prefers it. A refusal about the identity (the login failed, the terminal wants a password,
    /// another user ran) is final: falling back would hide it.
    pub async fn run_docker_first(
        &self,
        code: &str,
        ns: &str,
        opts: &RunOpts<'_>,
    ) -> Result<ExecOutcome, ExecError> {
        if self.route.container().is_none() {
            return self.run(code, ns, opts).await;
        }
        self.run_docker_first_unreported(code, ns, opts)
            .await
            .map_err(|e| self.named(e))
    }

    async fn run_docker_first_unreported(
        &self,
        code: &str,
        ns: &str,
        opts: &RunOpts<'_>,
    ) -> Result<ExecOutcome, ExecError> {
        let docker = match self.terminal_leg(code, ns, opts, None).await {
            Ok(o) => return Ok(o),
            Err(e) if e.code == "TIMEOUT" || is_identity_failure(&e.code) => return Err(e),
            Err(e) => e,
        };
        match tokio::time::timeout(opts.timeout, self.generate(code, ns, opts.prefix)).await {
            Ok(Ok((output, identity))) => Ok(ExecOutcome { output, identity }),
            Ok(Err(http)) => Err(ExecError::new(
                "DOCKER_REQUIRED",
                format!(
                    "IRIS_CONTAINER set but docker exec failed ({}) and HTTP fallback also failed \
                     ({http}).{}",
                    docker.message,
                    crate::tools::DOCKER_REQUIRED_HINT
                ),
            )),
            Err(_) => Err(ExecError::new(
                "TIMEOUT",
                format!("execution timed out after {}s", opts.timeout.as_secs()),
            )),
        }
    }

    /// The terminal leg: `docker exec` or SSH to the route's container, guarded.
    async fn terminal_leg(
        &self,
        code: &str,
        ns: &str,
        opts: &RunOpts<'_>,
        http_err: Option<String>,
    ) -> Result<ExecOutcome, ExecError> {
        let terminal_code = opts.terminal_code.unwrap_or(code);
        if let Some(e) = block_syntax_refusal(terminal_code, opts) {
            return Err(e);
        }
        let (Some(container), ssh_host) = (self.route.container(), self.route.ssh_host()) else {
            return Err(ExecError::new(
                crate::tools::nopws::NOPWS_NO_CONTAINER,
                "no container to run in",
            ));
        };
        let path = self.route.path();
        let mut login = self.expected.clone();
        let mut source = self.source;
        for round in 0..2 {
            let target = TerminalTarget {
                container: container.to_string(),
                ssh_host: ssh_host.map(str::to_string),
                login: login.clone(),
                ssh_strict: self.ssh_strict,
            };
            let attempt = tokio::time::timeout(
                opts.timeout,
                self.conn.execute_terminal(terminal_code, ns, &target),
            )
            .await;
            let result = match attempt {
                Err(_) => {
                    return Err(ExecError::new(
                        "TIMEOUT",
                        format!("execution timed out after {}s", opts.timeout.as_secs()),
                    ))
                }
                Ok(r) => r,
            };
            match result {
                Ok(out) => {
                    let identity = IdentityReport {
                        user: out
                            .ran_as
                            .clone()
                            .or_else(|| login.clone())
                            .unwrap_or_else(|| self.conn.username.clone()),
                        source,
                        path,
                        verified: out.ran_as.is_some(),
                        server: self.server.clone(),
                    };
                    return Ok(ExecOutcome {
                        output: out.text,
                        identity,
                    });
                }
                Err(e) => {
                    if let Some(m) = e.downcast_ref::<MarkerError>() {
                        let guard_failed =
                            matches!(m, MarkerError::LoginFailed | MarkerError::TerminalDenied);
                        if round == 0 && guard_failed && self.opt_in.is_some() && login.is_some() {
                            login = None;
                            source = IdentitySource::OsUserOptIn;
                            continue;
                        }
                        let mut err = ExecError::new(m.code(), m.message(self.expected.as_deref()));
                        err.identity = Some(Box::new(IdentityReport {
                            user: self.expected.clone().unwrap_or_default(),
                            source,
                            path,
                            verified: false,
                            server: self.server.clone(),
                        }));
                        return Err(err.with("execution_path", json!(path.as_str())));
                    }
                    return Err(self.terminal_failure(&e, path, ssh_host, http_err.as_deref()));
                }
            }
        }
        unreachable!("the loop returns on its second round")
    }

    fn terminal_failure(
        &self,
        e: &anyhow::Error,
        path: ExecPath,
        ssh_host: Option<&str>,
        http_err: Option<&str>,
    ) -> ExecError {
        let msg = e.to_string();
        let mut out = if let Some(detail) = msg.strip_prefix(&format!(
            "{}: ",
            crate::iris::connection::ERR_CONTAINER_UNREACHABLE
        )) {
            ExecError::new(crate::iris::connection::ERR_CONTAINER_UNREACHABLE, detail)
        } else if msg.starts_with(ERR_SSH_HOST_KEY_MISMATCH) {
            ExecError::new(ERR_SSH_HOST_KEY_MISMATCH, msg.clone())
        } else if let Some(host) = ssh_host {
            ExecError::new(
                "SSH_EXEC_FAILED",
                format!(
                    "ssh {host} docker exec failed: {msg}. Verify SSH connectivity and that \
                     Docker is running on the remote host."
                ),
            )
        } else {
            ExecError::new("EXECUTION_FAILED", msg)
        };
        out.extra
            .insert("execution_path".into(), json!(path.as_str()));
        if let Some(h) = http_err {
            out.extra.insert("http_error".into(), json!(h));
        }
        out
    }
}

// ── check_config ──────────────────────────────────────────────────────────────────────────────

/// One pool member's identity, for `check_config`.
pub struct MemberIdentity {
    pub name: String,
    /// `{user, source}` or `{refused: <code>}`.
    pub decision: Value,
    pub route: ExecRoute,
}

/// The `identity` object of `check_config` (R2 section 4): the account, the opt-in and where it
/// was set (or ignored), the user each path would run as, each server, and the tools covered.
pub fn identity_block(
    snap: &ConfigSnapshot,
    env_account: Option<&str>,
    env_opt_in: Option<&str>,
    default_route: &ExecRoute,
    members: &[MemberIdentity],
) -> Value {
    let opt_in = os_user_opt_in(snap, "default", env_opt_in);
    let ignored: Vec<Value> = snap
        .set_aside
        .iter()
        .filter(|i| i.setting.contains("exec_as_os_user") || i.setting.contains("execAsOsUser"))
        .map(|i| {
            let key = if i.setting.contains("execAsOsUser") {
                "execAsOsUser"
            } else {
                "exec_as_os_user"
            };
            json!({"file": i.file, "key": key})
        })
        .collect();
    let svc = env_account.map(str::to_string);
    let user = svc
        .clone()
        .unwrap_or_else(|| "the connection's own user".to_string());
    let http = (!matches!(
        default_route,
        ExecRoute::DockerOnly { .. } | ExecRoute::DockerOnlyNoContainer
    ))
    .then(|| {
        json!({
            "user": user,
            "login": "not probed",
            "usable": "unknown",
            "note": "the first iris_execute reports any missing privilege",
        })
    });
    let via = if svc.is_some() {
        "Security.Login guard"
    } else {
        "os authentication"
    };
    let docker_local = default_route
        .container()
        .filter(|_| default_route.ssh_host().is_none())
        .map(|c| json!({"container": c, "user": user, "via": via}));
    let docker_ssh = default_route.ssh_host().map(
        |h| json!({"host": h, "container": default_route.container(), "user": user, "via": via}),
    );
    let servers: Vec<Value> = members
        .iter()
        .map(|m| {
            let mut v = m.decision.clone();
            v["name"] = json!(m.name);
            v["route"] = json!(if m.route.container().is_some() {
                m.route.label()
            } else {
                "http"
            });
            v
        })
        .collect();
    json!({
        "service_account": {
            "configured": svc.is_some(),
            "user": svc,
            "source": svc.as_ref().map(|_| "service_account_env"),
        },
        "os_user_opt_in": {
            "enabled": opt_in.is_some(),
            "source": opt_in.map(OptInSource::as_str),
            "ignored": ignored,
        },
        "paths": {
            "http": http,
            "docker_local": docker_local,
            "docker_ssh": docker_ssh,
            "ws_terminal": {"user": user},
        },
        "servers": servers,
        "tools": SERVICE_ACCOUNT_TOOLS,
    })
}

impl ExecContext {
    /// A context that runs as `conn`'s own user, for a caller that holds a connection and client
    /// and no resolved call: tests, and tools outside the pipeline. The route is the default
    /// connection's (`IRIS_CONTAINER`).
    pub fn primary(conn: Arc<IrisConnection>, client: Arc<reqwest::Client>) -> Self {
        Self {
            ssh_strict: conn.member.ssh_strict,
            route: route_for_connection(&conn, RouteTarget::DefaultOrEnv),
            conn,
            client,
            expected: None,
            source: IdentitySource::Primary,
            server: "default".to_string(),
            opt_in: None,
            named_server: false,
            env_container_set: crate::iris::connection::container_from_env().is_some(),
        }
    }

    /// The report for a tool that runs its code through a helper taking `(connection, client)`
    /// and so sees no `$USERNAME`. A service account's connection refuses an answer from another
    /// user, so success on one is verified; the primary's is not checked.
    pub fn helper_report(&self) -> IdentityReport {
        IdentityReport {
            user: self
                .expected
                .clone()
                .unwrap_or_else(|| self.conn.username.clone()),
            source: self.source,
            path: ExecPath::Atelier,
            verified: self.expected.is_some(),
            server: self.server.clone(),
        }
    }
}

/// Add `identity` to a handler's structured result. A result without structured content, or whose
/// content is not an object, is returned as it is.
pub fn attach_identity(
    res: Result<rmcp::model::CallToolResult, rmcp::ErrorData>,
    identity: &IdentityReport,
) -> Result<rmcp::model::CallToolResult, rmcp::ErrorData> {
    let r = res?;
    let Some(Value::Object(mut body)) = r.structured_content.clone() else {
        return Ok(r);
    };
    body.insert("identity".to_string(), identity.to_json());
    Ok(if r.is_error == Some(true) {
        rmcp::model::CallToolResult::structured_error(Value::Object(body))
    } else {
        rmcp::model::CallToolResult::structured(Value::Object(body))
    })
}

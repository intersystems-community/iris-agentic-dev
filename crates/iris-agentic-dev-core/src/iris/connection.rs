//! IRIS connection types and Atelier REST API fingerprinting.

use std::fmt;

/// How this process is being driven, as reported to IRIS in the `User-Agent` header.
///
/// An operator asking "was that an agent or a person?" has only the request itself to go on:
/// Atelier REST arrives through the Web Gateway, so every caller looks like `CSPa24.so` from
/// the gateway's IP, and `$Username` is whatever credential was configured. The header is the
/// one field the caller controls and the access log records.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CallerMode {
    /// Long-running MCP server — an agent session.
    #[default]
    Mcp,
    /// One-shot `iris-agentic-dev tool|exec …` dispatch — a script, hook, or CI step.
    Cli,
}

impl CallerMode {
    fn marker(self) -> &'static str {
        match self {
            CallerMode::Mcp => "mcp",
            CallerMode::Cli => "cli",
        }
    }
}

tokio::task_local! {
    /// The MCP client that initiated this tool call, captured from `context.client_info()`
    /// at `call_tool` entry. `(name, version)` when the client sent `clientInfo` at
    /// `initialize`; `None` when it did not or when we are not inside a tool call.
    ///
    /// Task-scoped, not global: the HTTP transport clones one `IrisTools` across sessions,
    /// so a global would let session A's peer bleed into session B's `User-Agent` (R3).
    pub static MCP_PEER: Option<(String, String)>;
}

/// The MCP client that identified itself for the current tool call.
///
/// Returns `None` outside a `call_tool` scope, and `None` when the client sent no
/// `clientInfo`. Reads from the task-local set by `call_tool`.
pub fn mcp_peer() -> Option<(String, String)> {
    MCP_PEER.try_with(|p| p.clone()).unwrap_or(None)
}

/// Process-wide caller mode, set once by the binary before it opens any connection.
static CALLER_MODE: std::sync::OnceLock<CallerMode> = std::sync::OnceLock::new();

/// Record how this process is being driven. First call wins; later calls are ignored, so a
/// library embedding this cannot have its mode changed out from under it mid-run.
pub fn set_caller_mode(mode: CallerMode) {
    let _ = CALLER_MODE.set(mode);
}

/// The caller mode recorded by [`set_caller_mode`], defaulting to [`CallerMode::Mcp`].
pub fn caller_mode() -> CallerMode {
    *CALLER_MODE.get().unwrap_or(&CallerMode::Mcp)
}

/// Longest `IRIS_AGENT_LABEL` that reaches IRIS. The label lands in every access-log line,
/// so it is capped rather than trusted.
const MAX_LABEL_LEN: usize = 64;

/// The `User-Agent` sent on every IRIS-facing request.
///
/// Format: `iris-agentic-dev/<version> (<mode>[; <label>][; <client>/<version>])`.
///
/// Operators filter on this to tell agent traffic from a developer's IDE traffic — see
/// `docs/agent-attribution.md`. `IRIS_AGENT_LABEL` is a free-text tag (which agent, which
/// team, which pipeline) that is stripped of non-ASCII and control characters and
/// length-capped. The MCP client part (`<name>/<version>`) is appended when a peer
/// identified itself at `initialize`; it is read from the task-local set by `call_tool` and
/// cleaned by [`clean_peer`] first.
pub fn user_agent(mode: CallerMode) -> String {
    // The MCP client identity lets an operator distinguish "claude-code" from "cursor" from a
    // custom harness, rather than knowing only that some agent acted.
    user_agent_with_peer(mode, mcp_peer().as_ref())
}

/// Like [`user_agent`] but accepts an explicit MCP peer rather than reading the task-local.
/// Used by `iris_audit` to build `EventData` outside of a task-local scope.
pub fn user_agent_with_peer(mode: CallerMode, peer: Option<&(String, String)>) -> String {
    let mut ua = format!(
        "iris-agentic-dev/{} ({}",
        env!("CARGO_PKG_VERSION"),
        mode.marker()
    );
    if let Some(label) = sanitized_agent_label() {
        ua.push_str("; ");
        ua.push_str(&label);
    }
    if let Some((name, version)) = peer.and_then(clean_peer) {
        ua.push_str("; ");
        ua.push_str(&name);
        ua.push('/');
        ua.push_str(&version);
    }
    ua.push(')');
    ua
}

/// Longest client name and version that reach IRIS.
const MAX_PEER_NAME_LEN: usize = 64;
const MAX_PEER_VERSION_LEN: usize = 32;

/// The MCP client's `clientInfo` reduced to what is safe in a header and in an audit event's
/// `key=value` text (spec 141 T221). The client chose both strings and iad passes them on, so
/// they get the `IRIS_AGENT_LABEL` treatment and a little more: only ASCII letters, digits and
/// `. _ + -` survive, every other character (non-ASCII, controls, whitespace, and the `; ( ) / = "`
/// the User-Agent and the event text use as separators) becomes `_`, runs of `_` collapse, and the
/// name and version are capped. A client that cleans to nothing is left out rather than printed as
/// `/`. A version that cleans to nothing is `unknown`. Clean input is returned unchanged.
pub fn clean_peer((name, version): &(String, String)) -> Option<(String, String)> {
    let name = clean_peer_part(name, MAX_PEER_NAME_LEN);
    if name.is_empty() {
        return None;
    }
    let mut version = clean_peer_part(version, MAX_PEER_VERSION_LEN);
    if version.is_empty() {
        version = "unknown".to_string();
    }
    Some((name, version))
}

fn clean_peer_part(raw: &str, max: usize) -> String {
    let mut out = String::with_capacity(raw.len().min(max));
    for c in raw.chars() {
        let c = if c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '+' | '-') {
            c
        } else {
            '_'
        };
        if c == '_' && out.ends_with('_') {
            continue;
        }
        out.push(c);
    }
    let trimmed = out.trim_matches('_');
    trimmed.chars().take(max).collect()
}

/// `IRIS_AGENT_LABEL` reduced to something safe to put in a header: control characters
/// (CR/LF above all — a bare CRLF in a header value is request splitting) become spaces,
/// Non-ASCII is stripped (DP-446307), control chars become spaces, runs of whitespace collapse,
/// and the result is capped at [`MAX_LABEL_LEN`].
fn sanitized_agent_label() -> Option<String> {
    let raw = std::env::var("IRIS_AGENT_LABEL").ok()?;
    // Strip non-ASCII (DP-446307: a single Unicode char anywhere in %SYS.Audit::Export throws
    // <ILLEGAL VALUE>) and replace control chars with space; collapse runs of whitespace.
    let cleaned: String = raw
        .chars()
        .filter(|c| c.is_ascii())
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    let cleaned = cleaned.trim();
    if cleaned.is_empty() {
        return None;
    }
    Some(match cleaned.char_indices().nth(MAX_LABEL_LEN) {
        Some((cut, _)) => cleaned[..cut].to_string(),
        None => cleaned.to_string(),
    })
}

/// Decide whether to skip TLS certificate validation, given the two env var values.
///
/// Takes the values rather than reading the environment so the truth table can be tested without
/// `set_var`. `http::TlsSetting::from_launch_env` is the env-reading form.
///
/// `IRIS_INSECURE` is checked first and wins. `IRIS_INSECURE=false` is not an instruction to
/// validate, only the absence of one, so `IRIS_TLS_VERIFY` still gets to speak — a shell that
/// exports the first variable for tidiness must not silently override a `tls_verify = false`
/// config. Anything unrecognised means validate: `IRIS_INSECURE=yes` is a typo, not consent, and
/// failing closed produces a legible handshake error instead of silently unvalidated TLS.
///
/// Every reader of the two variables comes through here, by way of `http::TlsSetting`. Four
/// hand-copied versions of this decision used to exist and two of them read only `IRIS_INSECURE`,
/// so `IRIS_TLS_VERIFY=false` was honoured for a single `iris_doc` get and ignored for a batch get
/// and for every websocket (#127).
///
/// The parameters are named for what they hold rather than after the variables themselves: the
/// tool-name gate reads a bare snake-cased `iris_…` identifier in a signature as a reference to a
/// tool the router does not have, since it recognises `fn` and `let` declarations but not a
/// parameter list.
pub fn tls_insecure(insecure_var: Option<&str>, tls_verify_var: Option<&str>) -> bool {
    if let Some(v) = insecure_var {
        if v == "true" || v == "1" {
            return true;
        }
    }
    matches!(tls_verify_var, Some("false") | Some("0"))
}

/// Whether the connected IRIS instance is a production (Live) system.
/// Detected at probe time via `^%SYS("SystemMode")` SQL query.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SystemMode {
    Live,        // "LIVE" or "FAILOVER" — lock write tools
    Development, // "DEVELOPMENT" — allow write tools
    Test,        // "TEST" — allow write tools
    /// The query ran and `^%SYS("SystemMode")` holds nothing: a fresh instance. Judged like
    /// `Unknown` by the write gates; the environment template reads it as dev (R4a D6).
    Unset,
    #[default]
    Unknown, // could not read it — apply namespace heuristic
}

/// Read `^%SYS("SystemMode")`. The Management Portal stores `LIVE`, `TEST`, `DEVELOPMENT` or
/// `FAILOVER` (Config.Startup's SystemMode); this matched `Live` exactly, so a real Live instance
/// read as Unknown and had its writes left to the namespace heuristic. A failover member is a
/// production system too.
pub fn parse_system_mode(raw: &str) -> SystemMode {
    match raw.trim().to_ascii_uppercase().as_str() {
        "LIVE" | "FAILOVER" => SystemMode::Live,
        "DEVELOPMENT" => SystemMode::Development,
        "TEST" => SystemMode::Test,
        _ => SystemMode::Unknown,
    }
}

/// Read the answer of the `SystemMode` query: `Unset` when the query ran and returned no value,
/// `Unknown` when it did not answer (HTTP error, an account without `%SYS` read, no `result`).
pub fn parse_system_mode_response(status: u16, body: &str) -> SystemMode {
    if !(200..300).contains(&status) {
        return SystemMode::Unknown;
    }
    let Ok(v) = serde_json::from_str::<serde_json::Value>(body) else {
        return SystemMode::Unknown;
    };
    let Some(rows) = v["result"]["content"].as_array() else {
        return SystemMode::Unknown;
    };
    let value = rows
        .first()
        .and_then(|r| r["Value"].as_str())
        .map(str::trim)
        .unwrap_or("");
    if value.is_empty() {
        return SystemMode::Unset;
    }
    parse_system_mode(value)
}

/// Which version of the Atelier REST API to use.
#[derive(Debug, Clone, PartialEq)]
pub enum AtelierVersion {
    V8,
    V7,
    V2,
    V1,
}

impl AtelierVersion {
    pub fn version_str(&self) -> &'static str {
        match self {
            AtelierVersion::V8 => "v8",
            AtelierVersion::V7 => "v7",
            AtelierVersion::V2 => "v2",
            AtelierVersion::V1 => "v1",
        }
    }

    /// Returns true if this version supports WebSocket terminal sessions (v7+).
    pub fn supports_ws_terminal(&self) -> bool {
        matches!(self, AtelierVersion::V7 | AtelierVersion::V8)
    }
}

/// A resolved connection to a running IRIS instance via Atelier REST API.
/// T011: manual Debug impl redacts `password` (P1/FR-022).
#[derive(Clone)]
pub struct IrisConnection {
    /// Base URL e.g. "http://localhost:52773" or "http://localhost:80/prefix"
    pub base_url: String,
    pub namespace: String,
    pub username: String,
    pub password: String,
    pub version: Option<String>,
    pub atelier_version: AtelierVersion,
    pub source: DiscoverySource,
    pub port_superserver: Option<u16>,
    /// Detected at probe time — controls write-tool availability (issue #26).
    pub system_mode: SystemMode,
    /// Remote SSH host for docker exec routing (FR-007/101-nopws-connectivity).
    /// When set, docker exec commands are prefixed with
    /// `ssh -o StrictHostKeyChecking=no <ssh_host>`.
    pub ssh_host: Option<String>,
    /// Why the operator's `IRIS_USERNAME`/`IRIS_PASSWORD` were not sent to this server: a
    /// workspace file named a host that is neither this machine nor `IRIS_HOST`'s server.
    pub credentials_withheld: Option<String>,
    /// Whether this server's certificate is checked, and what said so. Set where the connection
    /// is built from its source; `IrisConnection::new` validates.
    pub tls: super::http::TlsSetting,
    /// What is particular to a pool member for running the agent's code (spec 141, R2).
    pub member: Box<MemberExec>,
}

/// A pool member's own restricted account, container and SSH strictness. Boxed: most connections
/// carry none of it, and `IrisConnection` is moved by value through the discovery code.
#[derive(Clone, Default)]
pub struct MemberExec {
    /// This server's own restricted account for the agent's code. The default connection's comes
    /// from `IRIS_SERVICE_USERNAME`, not from here.
    pub service_username: Option<String>,
    pub service_password: Option<String>,
    /// This server's own container for a docker exec fallback. The default connection's comes from
    /// `IRIS_CONTAINER`; a named server never inherits it (FR-009).
    pub container: Option<String>,
    /// Require a known SSH host key (`StrictHostKeyChecking=yes`) instead of accepting a new one.
    pub ssh_strict: bool,
    /// Set on a service account's connection: the user every generated class must report running
    /// as (`$USERNAME`). A different answer is an error, whichever tool made the call.
    pub expect_user: Option<String>,
}

/// T011: Manual Debug implementation — never prints the password.
impl fmt::Debug for IrisConnection {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("IrisConnection")
            .field("base_url", &self.base_url)
            .field("namespace", &self.namespace)
            .field("username", &self.username)
            .field("password", &"[redacted]")
            .field("version", &self.version)
            .field("atelier_version", &self.atelier_version)
            .field("source", &self.source)
            .field("port_superserver", &self.port_superserver)
            .field("system_mode", &self.system_mode)
            .field("ssh_host", &self.ssh_host)
            .field("credentials_withheld", &self.credentials_withheld)
            .field("tls", &self.tls)
            .field("service_username", &self.member.service_username)
            .field(
                "service_password",
                &self.member.service_password.as_ref().map(|_| "[redacted]"),
            )
            .field("container", &self.member.container)
            .finish()
    }
}

#[derive(Debug, Clone)]
pub enum DiscoverySource {
    LocalhostScan {
        port: u16,
    },
    Docker {
        container_name: String,
    },
    VsCodeSettings,
    EnvVar,
    ExplicitFlag,
    /// Discovered via VS Code Server Manager settings.json + OS keychain (044).
    ServerManager {
        server_name: String,
    },
}

/// Structured result from a document compile operation.
#[derive(Debug)]
pub struct CompileResult {
    pub errors: Vec<String>,
    pub console: Vec<String>,
}

impl CompileResult {
    pub fn success(&self) -> bool {
        self.errors.is_empty()
    }
}

/// `IRIS_CONTAINER`, unless it is unset or blank. A blank value is a common way to switch the
/// docker-exec path off; read as a name it sent `docker exec ''`, whose error hid the real reason
/// a call failed.
pub fn container_from_env() -> Option<String> {
    non_blank(std::env::var("IRIS_CONTAINER").ok())
}

impl IrisConnection {
    /// The container this connection's docker leg runs in: the one a configuration file named for
    /// it (`member.container`), else `IRIS_CONTAINER`. A file's container is data on the
    /// connection, and outranks the launch environment as the rest of the file's connection does.
    pub fn default_container(&self) -> Option<String> {
        non_blank(self.member.container.clone()).or_else(container_from_env)
    }
}

/// `IRIS_HOST`, unless it is unset or blank.
/// The server the launch settings name: `IRIS_HOST` with `IRIS_SCHEME`, `IRIS_WEB_PORT` and
/// `IRIS_WEB_PREFIX`. `None` when `IRIS_HOST` is unset or the settings are not a valid endpoint.
pub fn launch_endpoint() -> Option<super::endpoint::Endpoint> {
    let host = host_from_env()?;
    let port = std::env::var("IRIS_WEB_PORT")
        .ok()
        .and_then(|p| p.parse::<u16>().ok())
        .unwrap_or(52773);
    super::endpoint::Endpoint::parse(
        std::env::var("IRIS_SCHEME").ok().as_deref(),
        &host,
        port,
        std::env::var("IRIS_WEB_PREFIX").ok().as_deref(),
    )
    .ok()
}

pub fn host_from_env() -> Option<String> {
    non_blank(std::env::var("IRIS_HOST").ok())
}

/// `v` when it holds something other than whitespace.
pub fn non_blank(v: Option<String>) -> Option<String> {
    v.filter(|s| !s.trim().is_empty())
}

/// What the generator class puts in front of its result; see `build_exec_class`.
const RESULT_MARK: char = '\u{2}';
/// After [`RESULT_MARK`]: the code left the device at `$PRINCIPAL`, and the rest of its output is
/// in Atelier's `console` lines.
const PRINCIPAL_MARK: char = '\u{4}';

/// The captured output from a generator's `/action/query` answer.
///
/// A non-2xx status, a body that is not JSON, or errors in Atelier's `status` are errors. The old
/// code read any of them as `Ok("")`: `%UnitTest.Manager`, when `messages.log` grows mid-test,
/// writes its own error text ahead of the HTTP headers, and the call came back as an empty
/// success. Atelier does not always hand the column back as a JSON string: when the captured
/// output parses as JSON on its own (an array, an object, a number, a bare `true`) IRIS emits it
/// structurally, so it is re-serialized rather than read as empty.
pub fn query_output(status: u16, body: &str) -> anyhow::Result<String> {
    query_output_identified(status, body).map(|(out, _)| out)
}

/// What the generator puts between the leading mark and the output: `$USERNAME`, then this.
const IDENTITY_MARK: char = '\u{3}';

/// [`query_output`], and the user name IRIS reported as having run the code.
///
/// The user is `None` when the answer carries no identity, which an executor class built before
/// this field existed would produce. The output is split at the first separator, so a separator
/// inside the output stays in the output.
pub fn query_output_identified(
    status: u16,
    body: &str,
) -> anyhow::Result<(String, Option<String>)> {
    let head: String = body.chars().take(300).collect();
    if !(200..300).contains(&status) {
        anyhow::bail!("Atelier answered the generator query with HTTP {status}: {head}");
    }
    let parsed: serde_json::Value = serde_json::from_str(body).map_err(|_| {
        anyhow::anyhow!(
            "Atelier's answer to the generator query is not JSON, so the run's output is lost. \
             Something wrote straight to the HTTP connection; %UnitTest.Manager does this when \
             messages.log grows during a test. The answer began: {head}"
        )
    })?;
    let result = &parsed["result"]["content"][0]["result"];
    // An error in Atelier's status with no result means the query failed. With a result, the
    // result is the answer: some calls report a status error alongside output they did produce
    // (resolve_dynamic_dispatch gets ERROR #5035 "Parsing error" with a usable result).
    if let Some(errors) = parsed["status"]["errors"]
        .as_array()
        .filter(|e| !e.is_empty() && result.is_null())
    {
        let text: Vec<String> = errors
            .iter()
            .map(|e| {
                e["error"]
                    .as_str()
                    .map(str::to_string)
                    .unwrap_or_else(|| e.to_string())
            })
            .collect();
        anyhow::bail!(
            "Atelier reported the generator query failed: {}",
            text.join("; ")
        );
    }
    Ok(match result {
        serde_json::Value::String(s) => {
            let marked = s.strip_prefix(RESULT_MARK);
            // `$USERNAME`, then IDENTITY_MARK, then the output; the output may itself open with
            // PRINCIPAL_MARK when the code left the device at `$PRINCIPAL`.
            let (user, rest) = match marked.and_then(|m| m.split_once(IDENTITY_MARK)) {
                Some((user, rest)) => (Some(user.to_string()), rest),
                None => (None, marked.unwrap_or(s)),
            };
            let at_principal = rest.strip_prefix(PRINCIPAL_MARK);
            let mut out = at_principal.unwrap_or(rest).replace('\x01', "\n");
            // The code moved the device to $PRINCIPAL; Atelier carries what it wrote after
            // that in `console`, so the output is the capture followed by those lines.
            if at_principal.is_some() {
                for line in parsed["console"].as_array().into_iter().flatten() {
                    out.push_str(line.as_str().unwrap_or_default());
                    out.push('\n');
                }
            }
            (out, user)
        }
        serde_json::Value::Null => (String::new(), None),
        other => (other.to_string(), None),
    })
}

/// The generator's query was sent, so the code ran or is still running. The error keeps its text;
/// a caller that sees it through [`code_may_have_run`] must neither retry nor run the code another
/// way. 1.6.3 turned failed query answers into errors, and `iris_execute` then re-ran the code
/// through docker exec, so a run whose output overflowed the string limit ran twice.
#[derive(Debug)]
pub struct CodeMayHaveRun(pub String);

impl std::fmt::Display for CodeMayHaveRun {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for CodeMayHaveRun {}

/// The generator's query was sent and did not answer within the HTTP timeout. IRIS may still be
/// running the code, so it is never retried, and a caller can tell it from a refused connection
/// (spec 141 T174: `journal_search` reports it as a timeout, not as `IRIS_UNREACHABLE`).
#[derive(Debug)]
pub struct RequestTimedOut(pub String);

impl std::fmt::Display for RequestTimedOut {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for RequestTimedOut {}

pub fn code_may_have_run(e: &anyhow::Error) -> bool {
    e.downcast_ref::<CodeMayHaveRun>().is_some() || e.downcast_ref::<RequestTimedOut>().is_some()
}

/// True when `e` is an HTTP timeout: a generator query that did not answer in time, or any other
/// request that reqwest timed out.
pub fn is_request_timeout(e: &anyhow::Error) -> bool {
    e.downcast_ref::<RequestTimedOut>().is_some()
        || e.downcast_ref::<reqwest::Error>()
            .is_some_and(reqwest::Error::is_timeout)
}

/// May a failed generator attempt be tried again? Only on a transient network error or a 5xx
/// before the code ran; 4xx are client errors (e.g. 401 auth). The extra patterns cover cold-start
/// failures on the first call: DNS not warm, TLS handshake, connection reset, or a half-open pooled
/// connection surfacing as EOF / broken pipe.
pub fn generator_retryable(e: &anyhow::Error) -> bool {
    if code_may_have_run(e) {
        return false;
    }
    let msg = e.to_string();
    let lower = msg.to_ascii_lowercase();
    msg.contains("HTTP 5")
        || [
            "error sending request",
            "connection refused",
            "connection reset",
            "connection closed",
            "broken pipe",
            "unexpected end of file",
            "eof",
            "dns",
            "handshake",
            "timed out",
            "timeout",
        ]
        .iter()
        .any(|p| lower.contains(p))
}

/// Did the run report a failure instead of the code's own output?
///
/// Four prefixes mean failure. [`IrisConnection::execute_via_generator`] itself emits
/// `ERROR: <exception>` from the `Catch` block, `ERROR($ZERROR): <code>` when the body wrote
/// nothing but left `$ZERROR` set, and `ERROR($DEVICE): <detail>` when the body left the current
/// device pointing somewhere other than the capture file, which means its output went where we
/// cannot read it. Tool-generated ObjectScript adds its own `ERROR:<sentinel>` on a bad `%Status`.
///
/// Callers that hand-rolled `starts_with("ERROR:")` caught the first and last but were blind to
/// the parenthesized shapes, and reported the IRIS error as a successful result.
///
/// This is the only permitted way to decide that generator output is a failure. Adding a shape
/// here fixes every caller at once; hand-rolling one fixes nothing anywhere else.
pub fn is_generator_error(out: &str) -> bool {
    generator_error_message(out).is_some()
}

/// The failure message, with whichever prefix matched removed — or `None` on real output.
///
/// Every site that used to write `if let Some(msg) = out.strip_prefix("ERROR:")` wants this. That
/// hand-rolled form matches the two unparenthesized shapes and is blind to `ERROR($ZERROR):` and
/// `ERROR($DEVICE):`, so those fell through to the success path: `dict.rs` parsed the error text as
/// JSON and returned `[]`, `admin.rs` reported `flag_cleared: true` for a password change that
/// never ran.
///
/// Returning the message rather than a bool is what makes the single-source rule adoptable — a
/// caller needs the text for its own error code, and if getting it means re-doing the prefix work
/// by hand then the shared check buys nothing.
pub fn generator_error_message(out: &str) -> Option<&str> {
    let t = out.trim_start();
    for prefix in [
        "ERROR($ZERROR): ",
        "ERROR($DEVICE): ",
        "ERROR: ",
        // Tool-generated ObjectScript writes `ERROR:<CODE>:<text>` with no space. Last, so the
        // spaced and parenthesized shapes are stripped in full before this catches the rest.
        "ERROR:",
    ] {
        if let Some(rest) = t.strip_prefix(prefix) {
            return Some(rest);
        }
    }
    None
}

/// A value that must reach IRIS and nothing else: a password to store, a new password to set.
///
/// `Debug` prints `[redacted]`, so a `tracing` line or an `{:?}` in an error cannot show it. There
/// is no `Display`. The one reader is [`encode_args_header`], through [`Secret::expose`].
pub struct Secret(String);

impl Secret {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for Secret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Secret([redacted])")
    }
}

/// Request header that carries the arguments of [`IrisConnection::execute_via_generator_with_args`].
///
/// Not a SQL parameter: with `%System/%SQL` auditing on, IRIS writes a bound parameter into
/// `%SYS.Audit` (T-R4b-L02, spec 141). A header is exposed where `Authorization` already is, and
/// the audit does not record it.
pub const IAD_ARGS_HEADER: &str = "X-IAD-Args";

/// A snippet and the secrets it reads as `tIadArgs.%Get(n)`. The snippet holds no value.
#[derive(Debug)]
pub struct GeneratorCall {
    pub code: String,
    pub args: Vec<Secret>,
}

/// The `X-IAD-Args` value for `args`: a JSON array of strings, base64-encoded. Base64 because a
/// password may hold a quote, a newline or a non-ASCII byte that a header value cannot.
pub fn encode_args_header(args: &[Secret]) -> String {
    use base64::Engine as _;
    let values: Vec<&str> = args.iter().map(Secret::expose).collect();
    let json = serde_json::to_vec(&values).expect("a list of strings always serialises");
    base64::engine::general_purpose::STANDARD.encode(json)
}

impl IrisConnection {
    pub fn new(
        base_url: impl Into<String>,
        namespace: impl Into<String>,
        username: impl Into<String>,
        password: impl Into<String>,
        source: DiscoverySource,
    ) -> Self {
        Self {
            base_url: base_url.into(),
            namespace: namespace.into(),
            username: username.into(),
            password: password.into(),
            version: None,
            atelier_version: AtelierVersion::V1,
            source,
            port_superserver: None,
            system_mode: SystemMode::Unknown,
            ssh_host: None,
            credentials_withheld: None,
            tls: super::http::TlsSetting::default(),
            member: Box::default(),
        }
    }

    /// This connection with the certificate setting its source gave it.
    pub fn with_tls(mut self, tls: super::http::TlsSetting) -> Self {
        if !tls.verify {
            // The logout at exit must reach this server the way the session did.
            super::csp_session::note_unchecked_origin(&self.base_url);
        }
        self.tls = tls;
        self
    }

    /// Where this server is, parsed from `base_url`.
    pub fn endpoint(&self) -> Result<super::endpoint::Endpoint, super::endpoint::InvalidEndpoint> {
        super::endpoint::Endpoint::from_base_url(&self.base_url)
    }

    /// A client for `kind` that checks this server's certificate as its own setting says.
    pub fn client(&self, kind: super::http::ClientKind) -> anyhow::Result<reqwest::Client> {
        super::http::client_for(&self.tls, kind)
    }

    /// Returns true if write-capable calls are allowed against *this* connection, judged from the
    /// operator's environment and the instance's own `SystemMode` / namespace.
    ///
    /// Delegates to [`crate::tools::write_gate::resolve_declared`] so the precedence chain has one
    /// implementation (085 FR-019). A connection knows nothing about a config file, so nothing is
    /// declared here — callers that hold a `.iris-agentic-dev.toml` declaration must resolve
    /// through `write_gate::resolve_for_connection`, which is what `ConnectionState` does. Reading
    /// the declaration back out of an environment variable is the #110 defect.
    pub fn is_write_allowed(&self) -> bool {
        crate::tools::write_gate::resolve_declared(
            crate::tools::write_gate::operator_env_gates(),
            crate::tools::write_gate::DeclaredGates::default(),
            self.system_mode,
            &self.namespace,
        )
        .write_enabled
    }

    /// Build the full Atelier REST URL for a given path suffix.
    pub fn atelier_url(&self, path: &str) -> String {
        format!(
            "{}/api/atelier{}",
            self.base_url.trim_end_matches('/'),
            path
        )
    }

    /// Build a versioned Atelier URL using the detected API version and the connection namespace.
    pub fn atelier_url_versioned(&self, path: &str) -> String {
        self.versioned_ns_url(&self.namespace.clone(), path)
    }

    /// Build a versioned Atelier URL for an explicit namespace.
    pub fn versioned_ns_url(&self, namespace: &str, path: &str) -> String {
        let v = self.atelier_version.version_str();
        // URL-encode namespace so %SYS becomes %25SYS in the path component
        let ns_encoded = urlencoding::encode(namespace);
        self.atelier_url(&format!("/{}/{}{}", v, ns_encoded, path))
    }

    /// Probe this connection: fetch IRIS version, Atelier API level, and SystemMode.
    pub async fn probe(&mut self) {
        let client = match self.client(super::http::ClientKind::Probe) {
            Ok(c) => c,
            Err(_) => return,
        };

        let url = self.atelier_url("/");
        if let Ok(resp) = client
            .get(&url)
            .basic_auth(&self.username, Some(&self.password))
            .send()
            .await
        {
            let status = resp.status();
            if status.is_success() {
                if let Ok(body) = crate::iris::http::capped_json::<serde_json::Value>(resp).await {
                    tracing::debug!("Atelier root response: {}", body);
                    let content = &body["result"]["content"];
                    self.version = content["version"].as_str().map(|v| v.to_string());
                    self.atelier_version = match content["api"].as_u64() {
                        Some(v) if v >= 8 => AtelierVersion::V8,
                        Some(v) if v >= 7 => AtelierVersion::V7,
                        Some(v) if v >= 2 => AtelierVersion::V2,
                        _ => AtelierVersion::V1,
                    };
                }
            } else {
                tracing::debug!("Atelier root probe got HTTP {}", status);
            }
        }

        // Detect SystemMode via SQL against %SYS global (issue #26).
        // One extra round-trip at startup; result cached for session lifetime.
        let mode = self.detect_system_mode(&client).await;
        self.system_mode = mode;
        tracing::info!(
            host = %self.base_url,
            version = ?self.version,
            system_mode = ?self.system_mode,
            write_allowed = self.is_write_allowed(),
            "iris-agentic-dev: connection probed"
        );
    }

    /// Query `^%SYS("SystemMode")` to detect whether this is a Live instance.
    pub(crate) async fn detect_system_mode(&self, client: &reqwest::Client) -> SystemMode {
        let url = self.versioned_ns_url("%SYS", "/action/query");
        let resp = client
            .post(&url)
            .basic_auth(&self.username, Some(&self.password))
            .json(&serde_json::json!({
                "query": "SELECT Value FROM %Library.Global_Get('%SYS', '^%SYS(\"SystemMode\")')"
            }))
            .send()
            .await;
        match resp {
            Ok(r) => {
                let status = r.status().as_u16();
                let body = crate::iris::http::text_or_default(r).await;
                parse_system_mode_response(status, &body)
            }
            Err(_) => SystemMode::Unknown,
        }
    }

    /// Execute ObjectScript code via the write-compile-query cycle (pure HTTP, no docker).
    /// FR-023: retries up to 3 times with 100/200/400ms backoff on network errors or HTTP 5xx.
    pub async fn execute_via_generator(
        &self,
        code: &str,
        namespace: &str,
        client: &reqwest::Client,
    ) -> anyhow::Result<String> {
        self.execute_via_generator_identified(code, namespace, client, "IrisDevRun")
            .await
            .map(|(out, _)| out)
    }

    /// [`Self::execute_via_generator`], and the user name IRIS reported as having run the code.
    /// The scratch class is named `IrisDevTmp.<prefix><id>`.
    pub async fn execute_via_generator_identified(
        &self,
        code: &str,
        namespace: &str,
        client: &reqwest::Client,
        prefix: &str,
    ) -> anyhow::Result<(String, Option<String>)> {
        self.execute_generator_with(code, namespace, client, prefix, &[])
            .await
    }

    /// [`Self::execute_via_generator`] for a snippet that needs a secret. The snippet reads
    /// `tIadArgs.%Get(n)`; the values travel in the `X-IAD-Args` request header and are never in
    /// the class source, the SQL text or a SQL parameter (FR-020).
    pub async fn execute_via_generator_with_args(
        &self,
        call: &GeneratorCall,
        namespace: &str,
        client: &reqwest::Client,
    ) -> anyhow::Result<String> {
        self.execute_generator_with(&call.code, namespace, client, "IrisDevRun", &call.args)
            .await
            .map(|(out, _)| out)
    }

    /// [`Self::execute_via_generator`] with the scratch class named `IrisDevTmp.<prefix><id>`.
    pub async fn execute_via_generator_prefixed(
        &self,
        code: &str,
        namespace: &str,
        client: &reqwest::Client,
        prefix: &str,
    ) -> anyhow::Result<String> {
        self.execute_via_generator_identified(code, namespace, client, prefix)
            .await
            .map(|(out, _)| out)
    }

    async fn execute_generator_with(
        &self,
        code: &str,
        namespace: &str,
        client: &reqwest::Client,
        prefix: &str,
        args: &[Secret],
    ) -> anyhow::Result<(String, Option<String>)> {
        let delays = [
            std::time::Duration::from_millis(100),
            std::time::Duration::from_millis(200),
            std::time::Duration::from_millis(400),
        ];
        let mut last_err = anyhow::anyhow!("no attempts made");

        for (attempt, delay) in delays.iter().enumerate() {
            match self
                .execute_via_generator_once(code, namespace, client, prefix, args)
                .await
            {
                Ok(output) => {
                    if attempt > 0 {
                        tracing::info!(
                            "execute_via_generator succeeded on attempt {}",
                            attempt + 1
                        );
                    }
                    return Ok(output);
                }
                Err(e) => {
                    if !generator_retryable(&e) || attempt == delays.len() - 1 {
                        return Err(e);
                    }
                    // Transient on cold-start (private web server still warming up) — debug only;
                    // the success path logs at info so a recovery is still visible when needed.
                    tracing::debug!(
                        "execute_via_generator attempt {} failed ({}), retrying in {:?}",
                        attempt + 1,
                        e,
                        delay
                    );
                    last_err = e;
                    tokio::time::sleep(*delay).await;
                }
            }
        }
        Err(last_err)
    }

    /// Single attempt of execute_via_generator (no retry logic).
    async fn execute_via_generator_once(
        &self,
        code: &str,
        namespace: &str,
        client: &reqwest::Client,
        prefix: &str,
        args: &[Secret],
    ) -> anyhow::Result<(String, Option<String>)> {
        let id: String = uuid::Uuid::new_v4()
            .simple()
            .to_string()
            .chars()
            .take(12)
            .collect();
        // Use a dedicated scratch package (IrisDevTmp) rather than User.* to avoid
        // polluting the user's application namespace with transient executor classes.
        let class_name = format!("IrisDevTmp.{}{}", prefix, id);
        let doc_name = format!("{}.cls", class_name);
        // SQL proc name: for non-User packages, IRIS SQL schema = package name (no "SQL" prefix).
        // IrisDevTmp.IrisDevRunXXX → SQL schema IrisDevTmp, proc IrisDevTmp.IrisDevRunXXX_Execute.
        // (The SQLUser prefix is a historical special case only for the User package.)
        let sql_func = format!("{}_Execute", class_name);
        let content = Self::build_exec_class(&class_name, code, !args.is_empty());

        // Compute the per-request User-Agent now (inside call_tool task scope so mcp_peer()
        // returns the connected client's name/version). This overrides the static default
        // set on the reqwest::Client at build time (which predates any tool call).
        let ua = user_agent(caller_mode());

        // Armed before the PUT: a future dropped while the request is in flight cannot know whether
        // IRIS stored the class, and deleting one that is not there costs a 404.
        let mut scratch = ScratchGuard::new(self, &doc_name, namespace);

        // 1. PUT the class document
        let put_url = self.versioned_ns_url(
            namespace,
            &format!("/doc/{}", urlencoding::encode(&doc_name)),
        );
        let put_resp = client
            .put(&put_url)
            .header(reqwest::header::USER_AGENT, &ua)
            .basic_auth(&self.username, Some(&self.password))
            .json(&serde_json::json!({"enc": false, "content": content}))
            .send()
            .await?;
        if !put_resp.status().is_success() {
            scratch.disarm();
            anyhow::bail!("PUT doc failed: HTTP {}", put_resp.status());
        }

        // 2. Compile
        let compile_url = self.versioned_ns_url(namespace, "/action/compile?flags=cuk");
        let compile_resp = client
            .post(&compile_url)
            .header(reqwest::header::USER_AGENT, &ua)
            .basic_auth(&self.username, Some(&self.password))
            .json(&serde_json::json!([doc_name]))
            .send()
            .await;
        // The PUT stored the class, so every exit from here on removes it.
        let compile_resp = match compile_resp {
            Ok(r) => r,
            Err(e) => {
                scratch.release(self, client).await;
                return Err(e.into());
            }
        };
        if !compile_resp.status().is_success() {
            scratch.release(self, client).await;
            anyhow::bail!("compile HTTP {}", compile_resp.status());
        }
        let compile_body: serde_json::Value =
            crate::iris::http::json_or_default(compile_resp).await;
        let has_errors = compile_body["result"]["log"]
            .as_array()
            .map(|entries| {
                entries.iter().any(|e| {
                    e["type"]
                        .as_str()
                        .map(|t| t.eq_ignore_ascii_case("error"))
                        .unwrap_or(false)
                })
            })
            .unwrap_or(false);
        if has_errors {
            scratch.release(self, client).await;
            anyhow::bail!("compile errors: {:?}", compile_body["result"]["log"]);
        }

        // 3. Query via SQL
        // "output" is a reserved word in IRIS SQL — use "result" as the column alias.
        let sql = format!("SELECT {}() AS result", sql_func);
        let query_url = self.versioned_ns_url(namespace, "/action/query");
        let mut query_req = client
            .post(&query_url)
            .header(reqwest::header::USER_AGENT, &ua)
            .basic_auth(&self.username, Some(&self.password))
            .json(&serde_json::json!({"query": sql}));
        if !args.is_empty() {
            query_req = query_req.header(IAD_ARGS_HEADER, encode_args_header(args));
        }
        let query_resp = query_req.send().await;
        // The class is compiled by now, so a failed send must still remove it before returning.
        // Past a connect failure the request may have reached IRIS, so the code may have run.
        let query_resp = match query_resp {
            Ok(r) => r,
            Err(e) => {
                scratch.release(self, client).await;
                if e.is_connect() {
                    return Err(e.into());
                }
                if e.is_timeout() {
                    return Err(RequestTimedOut(e.to_string()).into());
                }
                return Err(CodeMayHaveRun(e.to_string()).into());
            }
        };
        let status = query_resp.status().as_u16();
        let body = crate::iris::http::text_or_default(query_resp).await;

        // 4. Delete the temp class; a failure is logged, the output still returns
        scratch.release(self, client).await;

        let (out, ran_as) = query_output_identified(status, &body)
            .map_err(|e| anyhow::Error::from(CodeMayHaveRun(e.to_string())))?;
        if let (Some(want), Some(got)) = (&self.member.expect_user, &ran_as) {
            if !want.eq_ignore_ascii_case(got) {
                return Err(CodeMayHaveRun(format!(
                    "{}: IRIS reports the code ran as {got}, not as the service account {want}. \
                     The request named {want}, but IRIS answered from another login, which \
                     happens when a session cookie outranks the credentials. The code has \
                     already run.",
                    crate::tools::exec::ERR_SERVICE_ACCOUNT_IDENTITY_MISMATCH
                ))
                .into());
            }
        }
        Ok((out, ran_as))
    }

    /// Build the `.cls` source lines for the temp executor class.
    ///
    /// Callers that inspect the returned string for failure must use
    /// [`is_generator_error`] — this method emits two different error prefixes.
    ///
    /// Uses a plain ClassMethod (not CodeMode=objectgenerator) so that user code is
    /// compiled directly through the IRIS macro preprocessor. This means $$$macros and
    /// #include / #define directives in user code work as expected, matching what a real
    /// .cls compilation provides.
    fn build_exec_class(class_name: &str, code: &str, with_args: bool) -> Vec<String> {
        let mut lines: Vec<String> = vec![
            "Include %occInclude".into(),
            "".into(),
            format!("Class {} [ Final ]", class_name),
            "{".into(),
            "".into(),
            "ClassMethod Execute() As %String [ SqlProc ]".into(),
            "{".into(),
            // Capture all output to a temp file so we can return it as a string via SQL.
            // #56: use %Library.File.TempFilename() for platform-portable temp path.
            "  Set tmpfile = ##class(%Library.File).TempFilename(\"txt\")".into(),
            "  Set savedIO = $IO".into(),
            "  Open tmpfile:(\"WNS\":/IOT=\"UTF8\"):5".into(),
            "  If '$TEST { Quit \"ERROR: output capture unavailable\" }".into(),
            // `capdev` is the name IRIS gives the capture device, which is what `$IO` reports
            // later. It is not always the string TempFilename returned: native Windows IRIS
            // lower-cases it (#139) and IRIS rewrites paths such as `/tmp/./x`. Same line as the
            // `Use`, so user code still starts at Execute+7 in IRIS error positions.
            "  Use tmpfile  Set capdev = $IO".into(),
            "  Try {".into(),
        ];
        if with_args {
            // The arguments come from the request, not from the class: see `IAD_ARGS_HEADER`. A
            // missing header fails `%FromJSON` and is reported by the Catch below, never run.
            lines.extend([
                "    Set tIadRaw = $Get(%request.CgiEnvs(\"HTTP_X_IAD_ARGS\"))".into(),
                "    Set tIadArgs = [].%FromJSON($ZConvert($System.Encryption.Base64Decode(tIadRaw),\"I\",\"UTF8\"))".into(),
                "    Kill tIadRaw".into(),
            ]);
        }
        for line in code.lines() {
            lines.push(format!("    {}", line));
        }
        // Nothing after the snippet needs the arguments, and a local left behind is what an error
        // trap that dumps variables would write out.
        let (kill_after, kill_in_catch): (&[&str], &[&str]) = if with_args {
            (&["    Kill tIadArgs"], &["    Kill tIadArgs,tIadRaw"])
        } else {
            (&[], &[])
        };
        lines.extend(kill_after.iter().map(|l| l.to_string()));
        lines.extend([
            "    Write !".into(), // sentinel ensures output always ends with \n (IDEV-3)
            "  } Catch ex {".into(),
        ]);
        lines.extend(kill_in_catch.iter().map(|l| l.to_string()));
        lines.extend([
            "    Write \"ERROR: \",ex.DisplayString(),!".into(),
            "  }".into(),
            // Which device is current now that the body has run? Routines like
            // run^SystemPerformance select their own device and do not put it back, which
            // sends every later Write somewhere we never read. That used to surface as an
            // empty result the caller reported as success (the 1.3.0 pbuttons bug).
            "  Set devio = $IO".into(),
            // Snapshot $ZERROR now, before Close/Use/stream operations below can clobber
            // it. This captures non-exception errors (e.g. an OPEN failure that sets
            // $ZERROR without throwing) so we can surface them if the body produced no
            // output — but WITHOUT writing to tmpfile yet (see the out="" test below).
            "  Set ze = $ZError".into(),
            "  Close tmpfile".into(),
            "  Use savedIO".into(),
            // Read captured output from temp file and return it.
            "  Set out = \"\"".into(),
            "  Set stream = ##class(%Stream.FileCharacter).%New()".into(),
            "  Set stream.TranslateTable = \"UTF8\"".into(),
            "  Set sc = stream.LinkToFile(tmpfile)".into(),
            "  If $$$ISOK(sc) {".into(),
            "    While 'stream.AtEnd { Set out = out _ stream.ReadLine() _ $Char(10) }".into(),
            "  }".into(),
            "  Do ##class(%Library.File).Delete(tmpfile)".into(),
            // Only surface a non-exception $ZERROR when the body produced NO output.
            // A residual like <ENDOFFILE> is often left as a benign side effect of an
            // SCM provider's internal Read even when the operation fully succeeded;
            // appending it to a non-empty result corrupted otherwise-valid output.
            "  If (out=\"\") && (ze'=\"\") && (ze'=\",\") { Set out = \"ERROR($ZERROR): \"_ze_$Char(10) }"
                .into(),
            // Whatever we captured is partial when the device moved, so refuse the whole
            // result rather than hand back a plausible-looking fragment. The exception is a move
            // to $PRINCIPAL (`Use 0`, `Use $PRINCIPAL`, closing the current device): Atelier
            // returns what was written after it in the answer's `console`, so `$Char(4)` tells
            // `query_output` to append that and nothing is lost.
            "  If devio'=capdev { If devio=$PRINCIPAL { Set out = $Char(4)_out } Else { Set out = \"ERROR($DEVICE): the called code left the current device set to \"\"\"_devio_\"\"\" instead of the capture file, so its output was written elsewhere and is lost. Snapshot and restore around the call: Set io=$IO / <call> / Use io\"_$Char(10) } }"
                .into(),
            // $Char(2) first, so Atelier never sees text that looks like JSON: it hands such a
            // column back parsed, and fails with ERROR #5035 when the JSON is not valid (an output
            // of `[1,2`). `query_output` strips it. `$USERNAME` comes next, then $Char(3): the
            // identity IRIS ran the code as, read inside the process that ran it (spec 141, R2).
            "  Quit $Char(2)_$USERNAME_$Char(3)_out".into(),
            "}".into(),
            "".into(),
            "}".into(),
        ]);
        lines
    }

    /// Delete an Atelier document and say what the reply amounted to ([`delete_outcome`]).
    /// `Err` is a failure to ask at all; a document IRIS refused to delete is `Ok(Refused(..))`.
    async fn delete_doc(
        &self,
        doc_name: &str,
        namespace: &str,
        client: &reqwest::Client,
    ) -> anyhow::Result<DeleteOutcome> {
        let url = self.versioned_ns_url(
            namespace,
            &format!("/doc/{}", urlencoding::encode(doc_name)),
        );
        let resp = client
            .delete(&url)
            .basic_auth(&self.username, Some(&self.password))
            .send()
            .await?;
        let status = resp.status();
        let body = crate::iris::http::text_or_default(resp).await;
        Ok(delete_outcome(status, &body))
    }

    /// Delete the executor's scratch class; the caller's result stands. Returns whether the class is
    /// gone. One that stays is logged and handed to the call in progress as a warning
    /// ([`note_scratch_left`]).
    async fn delete_scratch(
        &self,
        doc_name: &str,
        namespace: &str,
        client: &reqwest::Client,
    ) -> bool {
        let why = match self.delete_doc(doc_name, namespace, client).await {
            Ok(DeleteOutcome::Gone) => return true,
            Ok(outcome) => outcome.why(),
            Err(e) => e.to_string(),
        };
        tracing::warn!("scratch class not removed, it stays in {namespace}: {why}");
        note_scratch_left(doc_name, namespace, &why);
        false
    }

    /// Execute ObjectScript code via docker exec (iris session stdin).
    ///
    /// LIMITATION: IRIS terminal sessions wrap stdin at ~80 columns when code is
    /// sent as a single line. For code longer than ~80 characters, callers with
    /// an HTTP client should use execute_via_generator() instead — it compiles
    /// user code into a temp class with no line-length restriction.
    ///
    /// This method is preserved for environments without Atelier REST access, and for code iad
    /// wrote itself: it runs as the container's OS user in `IRIS_CONTAINER`, read fresh on each
    /// call. Code the agent wrote goes through `tools::exec`, which names the container and the
    /// identity.
    pub async fn execute(&self, code: &str, namespace: &str) -> anyhow::Result<String> {
        let container = self
            .default_container()
            .ok_or_else(|| anyhow::anyhow!("DOCKER_REQUIRED"))?;
        let target = TerminalTarget {
            container,
            ssh_host: None,
            login: None,
            ssh_strict: self.member.ssh_strict,
        };
        self.execute_terminal(code, namespace, &target)
            .await
            .map(|o| o.text)
    }

    /// FR-007 (101-nopws-connectivity): [`Self::execute`] on a remote container, through SSH.
    pub async fn execute_ssh(
        &self,
        code: &str,
        namespace: &str,
        ssh_host: &str,
    ) -> anyhow::Result<String> {
        let container = self
            .default_container()
            .ok_or_else(|| anyhow::anyhow!("DOCKER_REQUIRED"))?;
        let target = TerminalTarget {
            container,
            ssh_host: Some(ssh_host.to_string()),
            login: None,
            ssh_strict: self.member.ssh_strict,
        };
        self.execute_terminal(code, namespace, &target)
            .await
            .map(|o| o.text)
    }

    /// Run `code` in an `iris session` in `target.container`, locally or through SSH.
    ///
    /// The first stdin line names the identity: with `target.login` it drops to that account and
    /// halts when it cannot (`exec::build_login_guard`); without, it only prints `$USERNAME`. The
    /// answer is read back, the identity lines are taken out, and a guarded call whose output
    /// shows the login failed, the terminal wanted a password, or another user ran is an error
    /// that holds a [`MarkerError`]. No password goes through argv or stdin.
    pub async fn execute_terminal(
        &self,
        code: &str,
        namespace: &str,
        target: &TerminalTarget,
    ) -> anyhow::Result<TerminalOutput> {
        use tokio::io::AsyncWriteExt;

        let eol = if target.ssh_host.is_some() {
            "\r\n"
        } else {
            "\n"
        };
        let mut child = match &target.ssh_host {
            None => {
                validate_container_name(&target.container).map_err(|e| anyhow::anyhow!(e))?;
                let mut c = tokio::process::Command::new("docker");
                // `--` ends docker's options: whatever follows is the container and the command.
                c.args([
                    "exec",
                    "-i",
                    "--",
                    &target.container,
                    "iris",
                    "session",
                    "IRIS",
                    "-U",
                    namespace,
                ]);
                c
            }
            Some(host) => {
                let remote_cmd = build_ssh_remote_command(&target.container, namespace, host)
                    .map_err(|e| anyhow::anyhow!(e))?;
                let mut c = tokio::process::Command::new("ssh");
                c.args(ssh_args(host, &remote_cmd, target.ssh_strict));
                c
            }
        };
        let mut child = child
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            // Kept, not discarded: when the daemon refuses the exec, its stderr is the only
            // account of why, and dropping it is what turned "no such container" into an empty
            // success. See [`docker_exec_failure`].
            .stderr(std::process::Stdio::piped())
            .spawn()
            .map_err(|e| {
                anyhow::anyhow!(
                    "{} not available: {e}",
                    if target.ssh_host.is_some() {
                        "ssh"
                    } else {
                        "docker"
                    }
                )
            })?;

        if let Some(mut stdin) = child.stdin.take() {
            use crate::tools::exec::{build_login_guard, END_PROBE_LINE, IDENTITY_PROBE_LINE};
            let first = match &target.login {
                Some(user) => build_login_guard(user),
                None => IDENTITY_PROBE_LINE.to_string(),
            };
            let _ = stdin.write_all(format!("{first}{eol}").as_bytes()).await;
            let _ = stdin.write_all(code.as_bytes()).await;
            if target.login.is_some() {
                let _ = stdin
                    .write_all(format!("{eol}{END_PROBE_LINE}").as_bytes())
                    .await;
            }
            let halt = if target.ssh_host.is_some() {
                "Halt"
            } else {
                "halt"
            };
            let _ = stdin
                .write_all(format!("{eol}{halt}{eol}").as_bytes())
                .await;
        }

        let label = if target.ssh_host.is_some() {
            "ssh docker exec"
        } else {
            "docker exec"
        };
        let output =
            tokio::time::timeout(std::time::Duration::from_secs(30), child.wait_with_output())
                .await
                .map_err(|_| anyhow::anyhow!("{label} timed out after 30s"))??;

        // Non-zero exit with output is IRIS reporting a problem in the code, and the caller reads
        // that text (`is_generator_error` classifies it). Non-zero exit with nothing to show means
        // the exec itself did not happen.
        if !output.status.success() && output.stdout.is_empty() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            if target.ssh_host.is_some() {
                if ssh_host_key_failure(&stderr) {
                    return Err(anyhow::anyhow!(
                        "{}: the host key of {} does not match the one on record, so nothing ran. \
                         {}",
                        crate::tools::exec::ERR_SSH_HOST_KEY_MISMATCH,
                        target.ssh_host.as_deref().unwrap_or_default(),
                        stderr.trim()
                    ));
                }
                return Err(anyhow::anyhow!("ssh exec failed: {stderr}"));
            }
            return Err(anyhow::anyhow!(docker_exec_failure(
                &target.container,
                output.status.code(),
                &stderr,
            )));
        }

        let raw = String::from_utf8_lossy(&output.stdout).to_string();
        let marker = crate::tools::exec::split_identity_marker(&raw, target.login.as_deref())
            .map_err(anyhow::Error::new)?;
        Ok(TerminalOutput {
            text: strip_iris_banner(&marker.rest),
            ran_as: marker.user,
        })
    }

    /// FR-004: Run a SQL query via the Atelier query endpoint.
    /// Takes an explicit `namespace` parameter rather than always using `self.namespace`.
    pub async fn query(
        &self,
        sql: &str,
        params: Vec<serde_json::Value>,
        namespace: &str,
        client: &reqwest::Client,
    ) -> anyhow::Result<serde_json::Value> {
        let url = self.versioned_ns_url(namespace, "/action/query");
        let resp = client
            .post(&url)
            .basic_auth(&self.username, Some(&self.password))
            .json(&serde_json::json!({"query": sql, "parameters": params}))
            .send()
            .await?;
        // Check status before decoding: on auth failure IRIS/WebGateway returns an HTML
        // challenge page, not JSON. Decoding that first turned every 401/403 into a
        // "error decoding response body" parse failure that pointed at the wrong
        // subsystem entirely — this is the first error many new users ever see, since an
        // unconfigured or freshly started container is 401 by default.
        let status = resp.status();
        if status == reqwest::StatusCode::UNAUTHORIZED || status == reqwest::StatusCode::FORBIDDEN {
            anyhow::bail!(
                "authentication failed (HTTP {}) for user '{}' at {}\n\
                 hint: check credentials, or whether the account requires a password change",
                status.as_u16(),
                self.username,
                url
            );
        }
        if !status.is_success() {
            anyhow::bail!("query HTTP {}", status);
        }
        let body: serde_json::Value =
            crate::iris::http::capped_json::<serde_json::Value>(resp).await?;
        // Surface Atelier-level errors returned as 200 OK with status.errors in body.
        if let Some(errs) = body["status"]["errors"].as_array() {
            if !errs.is_empty() {
                let msg = errs[0]["error"].as_str().unwrap_or("Atelier query error");
                anyhow::bail!("{}", msg);
            }
        }
        Ok(body)
    }

    /// Compile a document via POST /action/compile. Returns structured errors and console output.
    /// Used by both the CLI `compile` command and the MCP `iris_compile` tool.
    pub async fn compile_document(
        &self,
        doc_name: &str,
        namespace: &str,
        flags: &str,
        client: &reqwest::Client,
    ) -> anyhow::Result<CompileResult> {
        let compile_url = self.versioned_ns_url(
            namespace,
            &format!("/action/compile?flags={}", urlencoding::encode(flags)),
        );
        let resp = client
            .post(&compile_url)
            .basic_auth(&self.username, Some(&self.password))
            .json(&serde_json::json!([doc_name]))
            .send()
            .await?;
        if !resp.status().is_success() {
            anyhow::bail!("compile HTTP {}", resp.status());
        }
        let body: serde_json::Value = crate::iris::http::json_or_default(resp).await;
        let console: Vec<String> = body["console"]
            .as_array()
            .map(|a| {
                a.iter()
                    .filter_map(|v| v.as_str().map(|s| s.to_string()))
                    .collect()
            })
            .unwrap_or_default();
        let mut errors: Vec<String> = vec![];
        if let Some(se) = body["status"]["errors"].as_array() {
            for e in se {
                if let Some(msg) = e["error"].as_str() {
                    errors.push(msg.to_string());
                }
            }
        }
        for line in &console {
            if let Some(rest) = line.trim().strip_prefix("ERROR ") {
                let rest = rest.to_string();
                if errors.iter().all(|e| !e.contains(&rest)) {
                    errors.push(rest);
                }
            }
        }
        Ok(CompileResult { errors, console })
    }

    /// A tool-call client for the connection the launch settings describe: `IRIS_INSECURE` and
    /// `IRIS_TLS_VERIFY` apply to it and to nothing else. A client for a connection of its own is
    /// `conn.client(ClientKind::Call)`.
    pub fn http_client() -> anyhow::Result<reqwest::Client> {
        super::http::client_for(
            &super::http::TlsSetting::from_launch_env(),
            super::http::ClientKind::Call,
        )
    }

    /// A tool-call client for a connection whose certificate setting is `tls`, with the cookie jar
    /// of identity `key` (`csp_session::store_for_identity`). A service account's requests must not
    /// carry the primary's CSP cookie: IRIS reads the cookie before the `Authorization` header and
    /// would run the code as the primary.
    pub fn http_client_for_identity(
        key: &str,
        tls: &super::http::TlsSetting,
    ) -> anyhow::Result<reqwest::Client> {
        super::http::client_with_store(
            tls,
            super::http::ClientKind::Call,
            super::csp_session::store_for_identity(key),
        )
    }

    /// Test accessor for build_exec_class. Exposed for integration tests.
    #[doc(hidden)]
    pub fn build_exec_class_for_test(class_name: &str, _tmpfile: &str, code: &str) -> Vec<String> {
        Self::build_exec_class(class_name, code, false)
    }

    /// As [`Self::build_exec_class_for_test`], for a call that carries `X-IAD-Args`.
    #[doc(hidden)]
    pub fn build_exec_class_with_args_for_test(class_name: &str, code: &str) -> Vec<String> {
        Self::build_exec_class(class_name, code, true)
    }
}

/// Error code for a `docker exec` that never reached IRIS.
///
/// The local docker path sent stderr to `/dev/null` and ignored the exit status, so a
/// `docker exec` into a container that does not exist — which fails in the daemon, before IRIS
/// is involved at all — came back as `Ok("")` and `iris_execute` reported
/// `{"success": true, "output": ""}`. Every test pinned to a container name the machine does
/// not have therefore passed without executing anything, which is how the CI e2e job ran
/// `test_iris_execute_docker_exec_fallback` green against `iris-dev-iris`, a container that
/// only exists on one laptop.
pub const ERR_CONTAINER_UNREACHABLE: &str = "CONTAINER_UNREACHABLE";

/// Explain a failed `docker exec` in terms of what to do about it.
///
/// `exit_code` is `None` when the process died on a signal. `stderr` is passed through whenever
/// this function has no specific advice for it — a message the daemon wrote is worth more than
/// a guess.
pub fn docker_exec_failure(container: &str, exit_code: Option<i32>, stderr: &str) -> String {
    let detail = stderr.trim();
    let advice = if detail.contains("No such container") {
        format!(
            "docker has no container named '{container}'. Check `docker ps --filter name={container}`, \
             then point IRIS_CONTAINER (or the `container` key in .iris-agentic-dev.toml) at a \
             container that is running."
        )
    } else if detail.contains("is not running")
        || detail.contains("is paused")
        || detail.contains("is restarting")
    {
        format!(
            "container '{container}' exists but is not running: start it with \
             `docker start {container}`."
        )
    } else {
        let exit = match exit_code {
            Some(code) => format!("exit {code}"),
            None => "killed by signal".to_string(),
        };
        let said = if detail.is_empty() {
            "and wrote nothing to stderr".to_string()
        } else {
            format!("and said: {detail}")
        };
        format!(
            "`docker exec` into '{container}' failed ({exit}) {said}. IRIS was never reached, \
             so nothing ran."
        )
    };
    format!("{ERR_CONTAINER_UNREACHABLE}: {advice}")
}

/// FR-006: Strip IRIS session banner and prompt lines from docker exec stdout.
///
/// IRIS session output looks like:
///   Copyright (c) 2024 InterSystems Corporation
///   All rights reserved.
///   IRIS for UNIX ... 2024.1 ...
///   USER>
///   <code output lines>
///   USER>
///
/// We strip banner lines and bare prompt lines (lines that are ONLY a prompt, no content).
/// Lines that start with a prompt prefix but have content after it are kept.
pub fn strip_iris_banner(output: &str) -> String {
    let mut result_lines: Vec<&str> = Vec::new();
    // Banner-text rules (below) only apply before the first prompt is seen — after that,
    // a line like "IRIS for UNIX ..." is legitimate `Write $ZVersion` output, not the
    // connect-time banner, and must not be stripped (regression: this previously made
    // test_execute_zversion's `Write $ZVersion` output disappear entirely).
    let mut seen_prompt = false;

    for line in output.lines() {
        let trimmed = line.trim();

        if !seen_prompt
            && (trimmed.starts_with("Copyright")
                || trimmed.contains("InterSystems Corporation")
                || trimmed.starts_with("All rights reserved")
                || trimmed.starts_with("IRIS for ")
                || trimmed.starts_with("Cache for ")
                || trimmed.starts_with("Ensemble for ")
                // IRIS 2026.2+ prints "Node: <hostname>, Instance: IRIS" on session connect.
                // Without this, its embedded ':' gets misparsed as a name:code pair by
                // callers like parse_status_response (e.g. "Node" became the production name).
                || (trimmed.starts_with("Node: ") && trimmed.contains(", Instance:")))
        {
            continue;
        }

        // Strip bare prompt-only lines: lines that are just "USER>", "IRIS>", "%SYS>", etc.
        // A bare prompt line has no content beyond the prompt token.
        if is_bare_prompt_line(trimmed) {
            seen_prompt = true;
            continue;
        }

        result_lines.push(line);
    }

    // Remove leading blank lines
    while result_lines
        .first()
        .map(|l: &&str| l.trim().is_empty())
        .unwrap_or(false)
    {
        result_lines.remove(0);
    }
    // Remove trailing blank lines
    while result_lines
        .last()
        .map(|l: &&str| l.trim().is_empty())
        .unwrap_or(false)
    {
        result_lines.pop();
    }

    result_lines.join("\n")
}

/// Returns true if the line is purely an IRIS session prompt with no following content.
/// Examples: "USER>", "IRIS>", "%SYS>", "USER 1S1>" (continuation), "USER> " (trailing space).
///
/// IRIS terminal continuation prompts have the format "NS LineNumSDepth>" e.g. "USER 1S1>",
/// "USER 2S1>", "%SYS 1S1>" — these appear when multi-line code is sent via stdin.
fn is_bare_prompt_line(s: &str) -> bool {
    let s = s.trim_end();
    if !s.ends_with('>') {
        return false;
    }
    let token = &s[..s.len() - 1];
    // Primary prompt: "USER", "IRIS", "%SYS" etc.
    // Check the namespace part (before optional space + continuation suffix)
    let (ns_part, rest) = match token.find(' ') {
        None => (token, ""),
        Some(pos) => (&token[..pos], &token[pos + 1..]),
    };
    let ns = ns_part.strip_prefix('%').unwrap_or(ns_part);
    // Namespace must be non-empty alphanumeric+underscore, reasonable length
    if ns.is_empty() || ns.len() > 16 || !ns.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
    {
        return false;
    }
    // If there's a continuation suffix, it must match the pattern \d+S\d+ (e.g. "1S1", "12S2")
    if rest.is_empty() {
        return true;
    }
    let mut digits = rest;
    while digits.starts_with(|c: char| c.is_ascii_digit()) {
        digits = &digits[1..];
    }
    if !digits.starts_with('S') {
        return false;
    }
    let digits = &digits[1..];
    !digits.is_empty() && digits.chars().all(|c| c.is_ascii_digit())
}

/// What a DELETE of an Atelier document achieved.
///
/// HTTP 200 is not the answer on its own. A locked or checked-out document is refused with a 200
/// and the diagnosis in `status.errors` (ERROR #5845), which `iris_doc` has read that way for a
/// long time. The scratch-class delete read neither half, so a refused delete and a completed one
/// were the same event, and a class that could not be removed (a namespace under source control
/// that locks new documents) was neither reported nor tried again. The shape follows
/// iris-interop-dev's `delete_outcome`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DeleteOutcome {
    /// The document is not there: a clean 2xx, or a 404, which is the same result by another route.
    Gone,
    /// IRIS answered 2xx and refused in the reply body, in its own words.
    Refused(String),
    /// A non-2xx that is not a 404. Nothing is known about the document.
    HttpError(u16),
}

impl DeleteOutcome {
    /// Only `Gone` lets the scratch guard stand down: on anything else the class may still be
    /// compiled in the namespace.
    pub fn is_gone(&self) -> bool {
        matches!(self, Self::Gone)
    }

    /// Text for a log line or a warning. Every variant names itself, so none reads as "deleted".
    pub fn why(&self) -> String {
        match self {
            Self::Gone => "deleted".to_string(),
            Self::Refused(e) => format!("IRIS refused the delete: {e}"),
            Self::HttpError(s) => format!("the delete answered HTTP {s}"),
        }
    }
}

/// Read a DELETE reply. It takes the body text because a 200 can carry a refusal.
pub fn delete_outcome(status: reqwest::StatusCode, body: &str) -> DeleteOutcome {
    // 404 before `is_success`: an absent document is what the delete wanted.
    if status.as_u16() == 404 {
        return DeleteOutcome::Gone;
    }
    if !status.is_success() {
        return DeleteOutcome::HttpError(status.as_u16());
    }
    // A 2xx whose body is not JSON counts as done: only the shapes known to carry a refusal are
    // treated as one. There are two, both measured on 2026.2. A locked document puts its reason in
    // `status.errors`. A source-control class that refuses in `OnBeforeDelete` leaves `status.errors`
    // empty and puts it in `result.status` (and the console): the reply is a 200 either way.
    let parsed: serde_json::Value = serde_json::from_str(body).unwrap_or_default();
    if let Some(first) = parsed["status"]["errors"]
        .as_array()
        .and_then(|errs| errs.first())
    {
        return DeleteOutcome::Refused(
            first["error"]
                .as_str()
                .unwrap_or("delete refused with no text")
                .to_string(),
        );
    }
    match parsed["result"]["status"].as_str() {
        Some(text) if text.trim_start().starts_with("ERROR") => {
            DeleteOutcome::Refused(text.trim().to_string())
        }
        _ => DeleteOutcome::Gone,
    }
}

tokio::task_local! {
    /// Scratch classes the call in progress could not remove, as warning texts. Set by
    /// [`collecting_scratch_left`] around a tool call so that `execute_via_generator`, which many
    /// tools reach, can report a class that stays without every caller carrying it.
    static SCRATCH_LEFT: std::sync::Arc<std::sync::Mutex<Vec<String>>>;
}

/// Run `fut`, and return its output with the warnings for every scratch class it left behind.
pub async fn collecting_scratch_left<F: std::future::Future>(fut: F) -> (F::Output, Vec<String>) {
    let left = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let out = SCRATCH_LEFT.scope(left.clone(), fut).await;
    let warnings = std::mem::take(&mut *left.lock().unwrap_or_else(|e| e.into_inner()));
    (out, warnings)
}

/// Record that scratch class `doc_name` stays in `namespace`. A no-op outside a call.
fn note_scratch_left(doc_name: &str, namespace: &str, why: &str) {
    let _ = SCRATCH_LEFT.try_with(|left| {
        left.lock().unwrap_or_else(|e| e.into_inner()).push(format!(
            "The scratch class {doc_name} stays in namespace {namespace}: {why}. It is safe to \
             delete; this call's result is not affected."
        ));
    });
}

/// Removes an executor's scratch class even when the executing future never reaches its own delete.
///
/// A dropped future runs none of the deletes in `execute_via_generator_once`. That happens when a
/// tokio runtime shuts down with the call in flight (in-process tests, `main` returning) and when an
/// MCP client cancels a slow `iris_execute`; one full test run left 125 classes in USER that way.
/// `Drop` cannot await, and the runtime it ran on may be the one going away, so the delete runs on
/// its own thread with its own runtime and client, and the drop waits for it.
struct ScratchGuard {
    url: String,
    username: String,
    password: String,
    doc_name: String,
    namespace: String,
    tls: super::http::TlsSetting,
    armed: bool,
}

impl ScratchGuard {
    fn new(conn: &IrisConnection, doc_name: &str, namespace: &str) -> Self {
        Self {
            url: conn.versioned_ns_url(
                namespace,
                &format!("/doc/{}", urlencoding::encode(doc_name)),
            ),
            username: conn.username.clone(),
            password: conn.password.clone(),
            doc_name: doc_name.to_string(),
            namespace: namespace.to_string(),
            tls: conn.tls,
            armed: true,
        }
    }

    /// Delete on the caller's runtime. Disarmed only once IRIS says the class is gone, so a drop
    /// during the delete, or a delete IRIS refused, still gets one more attempt from `Drop`.
    async fn release(&mut self, conn: &IrisConnection, client: &reqwest::Client) {
        if conn
            .delete_scratch(&self.doc_name, &self.namespace, client)
            .await
        {
            self.armed = false;
        }
    }

    /// IRIS never stored the class.
    fn disarm(&mut self) {
        self.armed = false;
    }
}

impl Drop for ScratchGuard {
    fn drop(&mut self) {
        if !self.armed {
            return;
        }
        let url = std::mem::take(&mut self.url);
        let user = std::mem::take(&mut self.username);
        let pass = std::mem::take(&mut self.password);
        let doc = std::mem::take(&mut self.doc_name);
        let namespace = std::mem::take(&mut self.namespace);
        let tls = self.tls;
        let worker = std::thread::spawn(move || {
            let Ok(rt) = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
            else {
                return;
            };
            rt.block_on(async move {
                let Ok(client) = super::http::client_for(&tls, super::http::ClientKind::Call)
                else {
                    return;
                };
                let sent = client
                    .delete(&url)
                    .basic_auth(&user, Some(&pass))
                    .timeout(std::time::Duration::from_secs(5))
                    .send()
                    .await;
                match sent {
                    Ok(r) => {
                        let status = r.status();
                        let body = crate::iris::http::text_or_default(r).await;
                        let outcome = delete_outcome(status, &body);
                        if !outcome.is_gone() {
                            tracing::warn!(
                                "scratch class {doc} not removed after cancel, it stays in \
                                 {namespace}: {}",
                                outcome.why()
                            );
                        }
                    }
                    Err(e) => tracing::warn!(
                        "scratch class {doc} not removed after cancel, it stays in {namespace}: {e}"
                    ),
                }
            });
        });
        let _ = worker.join();
    }
}

/// Is `container` a container name? Docker's own rule: letters, digits, `_ . -`, starting with a
/// letter or digit. Both `docker exec` paths check it, the local one and the one through SSH, so a
/// name that docker would read as an option (`--privileged`, `-v/:/host`) never gets that far
/// (spec 141 T221).
pub fn validate_container_name(container: &str) -> Result<(), String> {
    let ok = !container.is_empty()
        && container
            .chars()
            .next()
            .is_some_and(|c| c.is_ascii_alphanumeric())
        && container
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | '-'));
    if ok {
        Ok(())
    } else {
        Err(format!(
            "INVALID_CONTAINER: '{container}' is not a container name (letters, digits, _ . -, \
             starting with a letter or digit)"
        ))
    }
}

/// The command `execute_ssh` hands to the remote shell. The container and namespace are checked
/// against their own syntax and single-quoted, and a host that starts with `-` is refused, since ssh
/// reads it as an option (`-oProxyCommand=...` runs a local command). Before this the parts went into
/// the remote shell unquoted, outside every IRIS gate (137).
pub fn build_ssh_remote_command(
    container: &str,
    namespace: &str,
    ssh_host: &str,
) -> Result<String, String> {
    validate_container_name(container)?;
    if let Err(e) = crate::objectscript::validate_namespace(namespace) {
        return Err(e["message"]
            .as_str()
            .unwrap_or("INVALID_NAMESPACE")
            .to_string());
    }
    if ssh_host.is_empty() || ssh_host.starts_with('-') || ssh_host.contains(char::is_whitespace) {
        return Err(format!("INVALID_SSH_HOST: '{ssh_host}' is not a host name"));
    }
    Ok(format!(
        "docker exec -i '{container}' iris session IRIS -U '{namespace}'"
    ))
}

/// Where a terminal call runs and as whom.
#[derive(Debug, Clone)]
pub struct TerminalTarget {
    pub container: String,
    /// Run the `docker exec` on this host, through SSH.
    pub ssh_host: Option<String>,
    /// Drop to this account before the code runs; `None` runs as the OS-authenticated user.
    pub login: Option<String>,
    pub ssh_strict: bool,
}

/// What a terminal call printed, with the identity lines taken out.
#[derive(Debug, Clone)]
pub struct TerminalOutput {
    pub text: String,
    /// The user the terminal reported (`$USERNAME`), when it printed the marker.
    pub ran_as: Option<String>,
}

/// The arguments for `ssh` to run `remote_cmd` on `host`.
///
/// `BatchMode=yes` makes a question the user cannot answer (a new host key, a password) a failure
/// instead of a hang. The host key is checked: a new one is recorded (`accept-new`), a changed one
/// refuses, and `strict` refuses a new one too. `StrictHostKeyChecking=no` accepted any key, which
/// let anyone on the path pose as the host the agent's code runs on.
pub fn ssh_args(host: &str, remote_cmd: &str, strict: bool) -> Vec<String> {
    let check = if strict { "yes" } else { "accept-new" };
    vec![
        "-o".into(),
        "BatchMode=yes".into(),
        "-o".into(),
        format!("StrictHostKeyChecking={check}"),
        host.to_string(),
        remote_cmd.to_string(),
    ]
}

/// Did `ssh` stop on the host key?
fn ssh_host_key_failure(stderr: &str) -> bool {
    stderr.contains("Host key verification failed")
        || stderr.contains("REMOTE HOST IDENTIFICATION HAS CHANGED")
}

#[cfg(test)]
mod system_mode_tests {
    use super::*;

    fn conn(namespace: &str, mode: SystemMode) -> IrisConnection {
        let mut c = IrisConnection::new(
            "http://localhost:52773",
            namespace,
            "_SYSTEM",
            "SYS",
            DiscoverySource::EnvVar,
        );
        c.system_mode = mode;
        c
    }

    // T005 — SystemMode parsing
    #[test]
    fn debug_impl_redacts_password() {
        let c = conn("USER", SystemMode::Unknown);
        let debug_str = format!("{c:?}");
        assert!(
            debug_str.contains("[redacted]"),
            "password should be redacted: {debug_str}"
        );
        assert!(
            !debug_str.contains("password: \"SYS\""),
            "raw password should not appear: {debug_str}"
        );
        assert!(
            debug_str.contains("IrisConnection"),
            "should be an IrisConnection: {debug_str}"
        );
    }

    #[test]
    fn system_mode_live_from_string() {
        // Simulates what detect_system_mode maps "Live" to
        assert_eq!(SystemMode::Live, SystemMode::Live);
        assert_ne!(SystemMode::Live, SystemMode::Unknown);
    }

    #[test]
    fn system_mode_default_is_unknown() {
        assert_eq!(SystemMode::default(), SystemMode::Unknown);
    }

    #[test]
    fn system_mode_development_ne_live() {
        assert_ne!(SystemMode::Development, SystemMode::Live);
    }

    // ── The inference chain (085 T013) ───────────────────────────────────────
    //
    // These used to mutate `IRIS_ALLOW_PROD` / `IRIS_WRITE_TOOLS_ENABLED` and call
    // `is_write_allowed()`. The operator environment is now a process-start snapshot behind a
    // `OnceLock`, so `set_var` inside a test is a no-op against whatever the first test in the
    // binary captured — a test that passes or fails depending on execution order. They drive the
    // pure resolver with an explicit `OperatorEnvGates` instead, which is what that parameter is
    // for.
    use crate::tools::write_gate::{resolve_declared, DeclaredGates, GateSource, OperatorEnvGates};

    /// Nothing declared, nothing in the operator's environment: pure inference.
    fn inferred(ns: &str, mode: SystemMode) -> bool {
        resolve_declared(
            &OperatorEnvGates::default(),
            DeclaredGates::default(),
            mode,
            ns,
        )
        .write_enabled
    }

    #[test]
    fn write_blocked_for_live() {
        assert!(!inferred("USER", SystemMode::Live));
    }

    #[test]
    fn write_allowed_for_development() {
        assert!(inferred("USER", SystemMode::Development));
    }

    #[test]
    fn write_allowed_for_test_mode() {
        assert!(inferred("USER", SystemMode::Test));
    }

    #[test]
    fn write_blocked_for_unknown_with_prod_namespace() {
        assert!(!inferred("PROD", SystemMode::Unknown));
        assert!(!inferred("PRODUCTION", SystemMode::Unknown));
        assert!(!inferred("LIVE", SystemMode::Unknown));
        assert!(!inferred("PRD", SystemMode::Unknown));
    }

    #[test]
    fn write_allowed_for_unknown_with_dev_namespace() {
        assert!(inferred("USER", SystemMode::Unknown));
        assert!(inferred("DEV", SystemMode::Unknown));
        assert!(inferred("MYAPP", SystemMode::Unknown));
    }

    #[test]
    fn iris_allow_prod_overrides_live_mode() {
        let op = OperatorEnvGates {
            allow_prod: true,
            ..Default::default()
        };
        let r = resolve_declared(&op, DeclaredGates::default(), SystemMode::Live, "USER");
        assert!(r.write_enabled);
        assert_eq!(r.write_source, GateSource::LegacyAllowProd);
    }

    #[test]
    fn is_write_allowed_logic_direct() {
        assert!(!inferred("LIVE", SystemMode::Unknown));
        assert!(!inferred("PROD", SystemMode::Live));
        assert!(inferred("DEV", SystemMode::Development));
    }

    /// `is_write_allowed()` must be the resolver, not a second copy of the chain. Compares
    /// against the resolver under whatever the process snapshot happens to be, so it asserts
    /// delegation without depending on the environment the test binary inherited.
    #[test]
    fn is_write_allowed_delegates_to_resolver() {
        for (ns, mode) in [
            ("USER", SystemMode::Development),
            ("USER", SystemMode::Live),
            ("PROD", SystemMode::Unknown),
            ("MYAPP", SystemMode::Unknown),
        ] {
            let expected = resolve_declared(
                crate::tools::write_gate::operator_env_gates(),
                DeclaredGates::default(),
                mode,
                ns,
            )
            .write_enabled;
            assert_eq!(
                conn(ns, mode).is_write_allowed(),
                expected,
                "is_write_allowed disagreed with the resolver for {ns}/{mode:?}"
            );
        }
    }

    #[test]
    fn is_production_namespace_case_insensitive() {
        use crate::tools::write_gate::is_production_namespace;
        assert!(is_production_namespace("prod"));
        assert!(is_production_namespace("PROD"));
        assert!(is_production_namespace("Production"));
        assert!(is_production_namespace("LIVE"));
        assert!(is_production_namespace("live"));
        assert!(is_production_namespace("PRD"));
        assert!(!is_production_namespace("USER"));
        assert!(!is_production_namespace("DEV"));
        assert!(!is_production_namespace("MYAPP"));
    }

    // The #110 override: an operator-set `IRIS_WRITE_TOOLS_ENABLED` beats SystemMode either way.
    #[test]
    fn operator_env_0_blocks_writes_on_dev_instance() {
        let op = OperatorEnvGates {
            write_tools_enabled: Some(false),
            ..Default::default()
        };
        let r = resolve_declared(
            &op,
            DeclaredGates::default(),
            SystemMode::Development,
            "USER",
        );
        assert!(
            !r.write_enabled,
            "IRIS_WRITE_TOOLS_ENABLED=0 must block even Development mode"
        );
        assert_eq!(r.write_source, GateSource::OperatorEnv);
    }

    #[test]
    fn operator_env_1_allows_writes_on_live_instance() {
        let op = OperatorEnvGates {
            write_tools_enabled: Some(true),
            ..Default::default()
        };
        let r = resolve_declared(&op, DeclaredGates::default(), SystemMode::Live, "USER");
        assert!(
            r.write_enabled,
            "IRIS_WRITE_TOOLS_ENABLED=1 must allow even Live mode"
        );
        assert_eq!(r.write_source, GateSource::OperatorEnv);
    }

    #[test]
    fn operator_env_absent_falls_through_to_system_mode() {
        assert!(!inferred("USER", SystemMode::Live));
        assert!(inferred("USER", SystemMode::Development));
    }
}

#[cfg(test)]
mod pure_fn_tests {
    use super::*;

    fn make_conn() -> IrisConnection {
        IrisConnection::new(
            "http://localhost:52773",
            "USER",
            "_SYSTEM",
            "SYS",
            DiscoverySource::EnvVar,
        )
    }

    // ── versioned_ns_url ──────────────────────────────────────────────────────
    #[test]
    fn versioned_ns_url_contains_namespace() {
        let c = make_conn();
        let url = c.versioned_ns_url("USER", "/docnames/CLS");
        assert!(url.contains("USER"), "{url}");
        assert!(url.contains("docnames"), "{url}");
    }

    #[test]
    fn versioned_ns_url_encodes_percent_sys() {
        let c = make_conn();
        let url = c.versioned_ns_url("%SYS", "/action/query");
        assert!(url.contains("%25SYS") || url.contains("%SYS"), "{url}");
        assert!(url.contains("action/query"), "{url}");
    }

    #[test]
    fn versioned_ns_url_different_namespaces() {
        let c = make_conn();
        let url1 = c.versioned_ns_url("MYNS", "/foo");
        let url2 = c.versioned_ns_url("OTHERNS", "/foo");
        assert_ne!(url1, url2);
    }

    // ── docker_exec_failure ──────────────────────────────────────────────────

    /// Verbatim stderr from `docker exec -i iad-no-such-container-xyz iris session IRIS -U USER`.
    const NO_SUCH_CONTAINER: &str =
        "Error response from daemon: No such container: iad-no-such-container-xyz\n";

    #[test]
    fn docker_exec_failure_names_the_container_and_how_to_check_it() {
        let msg = docker_exec_failure("iris-dev-iris", Some(1), NO_SUCH_CONTAINER);
        assert!(msg.starts_with(ERR_CONTAINER_UNREACHABLE), "{msg}");
        // Which container, and the one command that answers "so is it there or not".
        assert!(msg.contains("iris-dev-iris"), "{msg}");
        assert!(msg.contains("docker ps"), "{msg}");
        // Where the name came from, so the fix is findable.
        assert!(msg.contains("IRIS_CONTAINER"), "{msg}");
    }

    #[test]
    fn docker_exec_failure_distinguishes_stopped_from_absent() {
        let stopped = docker_exec_failure(
            "iris-dev-iris",
            Some(126),
            "Error response from daemon: Container abc123 is not running\n",
        );
        assert!(stopped.contains("not running"), "{stopped}");
        assert!(
            stopped.contains("docker start iris-dev-iris"),
            "a stopped container has a one-command fix; say it: {stopped}"
        );
        // The absent case must not tell the operator to start a container that does not exist.
        let absent = docker_exec_failure("iris-dev-iris", Some(1), NO_SUCH_CONTAINER);
        assert!(!absent.contains("docker start"), "{absent}");
    }

    #[test]
    fn docker_exec_failure_keeps_the_exit_code_when_stderr_says_nothing() {
        let msg = docker_exec_failure("iris-e2e", Some(127), "   \n");
        assert!(msg.contains("127"), "{msg}");
        assert!(msg.contains("iris-e2e"), "{msg}");
    }

    #[test]
    fn docker_exec_failure_survives_a_signal_kill_with_no_exit_code() {
        let msg = docker_exec_failure("iris-e2e", None, "killed");
        assert!(msg.starts_with(ERR_CONTAINER_UNREACHABLE), "{msg}");
        assert!(msg.contains("killed"), "{msg}");
    }

    #[test]
    fn docker_exec_failure_passes_unrecognized_stderr_through() {
        // Anything the daemon says that this function does not have a special case for still has
        // to reach the caller — guessing is what produced `success: true, output: ""`.
        let msg = docker_exec_failure(
            "iris-e2e",
            Some(1),
            "permission denied while trying to connect to the Docker daemon socket",
        );
        assert!(msg.contains("permission denied"), "{msg}");
    }

    // ── strip_iris_banner ────────────────────────────────────────────────────
    #[test]
    fn strip_iris_banner_empty_input() {
        assert_eq!(strip_iris_banner("").trim(), "");
    }

    #[test]
    fn strip_iris_banner_only_prompts() {
        let raw = "USER>\nUSER>\n";
        let stripped = strip_iris_banner(raw);
        assert!(
            stripped.trim().is_empty(),
            "only prompts → empty: {stripped:?}"
        );
    }

    #[test]
    fn strip_iris_banner_strips_continuation_prompts() {
        // IRIS multi-line stdin produces "USER 1S1>", "USER 2S1>" continuation prompts
        let raw = "USER>\nUSER 1S1>\nUSER 2S1>\nOK|ready\nUSER 3S1>\nUSER>\n";
        let stripped = strip_iris_banner(raw);
        assert_eq!(
            stripped.trim(),
            "OK|ready",
            "continuation prompts must be stripped: {stripped:?}"
        );
    }

    #[test]
    fn strip_iris_banner_output_without_banner() {
        let raw = "42\n";
        let stripped = strip_iris_banner(raw);
        assert_eq!(stripped.trim(), "42");
    }

    // ── build_exec_class / build_exec_class_for_test ──────────────────────────
    #[test]
    fn build_exec_class_contains_class_name() {
        let lines = IrisConnection::build_exec_class_for_test(
            "IrisDevTmp.IrisDevRuntest123",
            "/tmp/test.txt",
            "Write 1",
        );
        assert!(
            lines
                .iter()
                .any(|l| l.contains("IrisDevTmp.IrisDevRuntest123")),
            "class name must appear in generated source"
        );
    }

    #[test]
    fn build_exec_class_contains_user_code() {
        let lines = IrisConnection::build_exec_class_for_test(
            "TestClass",
            "/tmp/t.txt",
            "Write \"hello world\"",
        );
        assert!(
            lines.iter().any(|l| l.contains("hello world")),
            "user code must be embedded"
        );
    }

    #[test]
    fn build_exec_class_has_write_sentinel() {
        let lines = IrisConnection::build_exec_class_for_test("T", "/tmp/t.txt", "Write 1");
        let has_sentinel = lines.iter().any(|l| l.trim() == "Write !");
        assert!(has_sentinel, "sentinel Write ! must be present");
    }

    #[test]
    fn build_exec_class_has_include_occinclude() {
        let lines = IrisConnection::build_exec_class_for_test("T", "/tmp/t.txt", "Write 1");
        assert!(
            lines.iter().any(|l| l.trim() == "Include %occInclude"),
            "Include %occInclude must be present so $$$macros resolve in user code"
        );
    }

    #[test]
    fn build_exec_class_is_plain_classmethod_not_generator() {
        let lines = IrisConnection::build_exec_class_for_test("T", "/tmp/t.txt", "Write 1");
        let joined = lines.join("\n");
        assert!(
            !joined.contains("CodeMode = objectgenerator"),
            "user code method must be plain ClassMethod, not objectgenerator"
        );
        assert!(
            joined.contains("SqlProc"),
            "method must still be SqlProc so it can be called via SQL"
        );
    }
}

#[cfg(test)]
mod additional_tests {
    use super::*;

    fn make_conn() -> IrisConnection {
        IrisConnection::new(
            "http://localhost:52773",
            "USER",
            "_SYSTEM",
            "SYS",
            DiscoverySource::EnvVar,
        )
    }

    fn make_conn_v8() -> IrisConnection {
        let mut c = make_conn();
        c.atelier_version = AtelierVersion::V8;
        c
    }

    // ── AtelierVersion::version_str ───────────────────────────────────────────

    #[test]
    fn atelier_version_str_v1() {
        assert_eq!(AtelierVersion::V1.version_str(), "v1");
    }

    #[test]
    fn atelier_version_str_v2() {
        assert_eq!(AtelierVersion::V2.version_str(), "v2");
    }

    #[test]
    fn atelier_version_str_v8() {
        assert_eq!(AtelierVersion::V8.version_str(), "v8");
    }

    // ── atelier_url ───────────────────────────────────────────────────────────

    #[test]
    fn atelier_url_strips_trailing_slash() {
        let mut c = make_conn();
        c.base_url = "http://localhost:52773/".to_string();
        let url = c.atelier_url("/");
        assert!(
            !url.contains("//api"),
            "double slash must not appear: {url}"
        );
        assert!(url.contains("/api/atelier"), "{url}");
    }

    #[test]
    fn atelier_url_no_trailing_slash_on_base() {
        let c = make_conn();
        let url = c.atelier_url("/");
        assert_eq!(url, "http://localhost:52773/api/atelier/");
    }

    // ── versioned_ns_url with V8 ──────────────────────────────────────────────

    #[test]
    fn versioned_ns_url_uses_v8_when_set() {
        let c = make_conn_v8();
        let url = c.versioned_ns_url("USER", "/docnames/CLS");
        assert!(url.contains("/v8/"), "URL must include v8 segment: {url}");
    }

    #[test]
    fn versioned_ns_url_default_is_v1() {
        let c = make_conn();
        let url = c.versioned_ns_url("USER", "/docnames/CLS");
        assert!(url.contains("/v1/"), "URL must include v1 segment: {url}");
    }

    #[test]
    fn versioned_ns_url_path_appended_after_namespace() {
        let c = make_conn();
        let url = c.versioned_ns_url("USER", "/action/query");
        // Structure: .../v1/USER/action/query
        assert!(url.ends_with("/action/query"), "{url}");
    }

    // ── atelier_url_versioned uses self.namespace ─────────────────────────────

    #[test]
    fn atelier_url_versioned_uses_self_namespace() {
        let mut c = make_conn();
        c.namespace = "MYNS".to_string();
        let url = c.atelier_url_versioned("/action/query");
        assert!(url.contains("MYNS"), "self.namespace must appear: {url}");
    }

    // ── CompileResult::success ────────────────────────────────────────────────

    #[test]
    fn compile_result_success_when_no_errors() {
        let r = CompileResult {
            errors: vec![],
            console: vec!["Compiled.".into()],
        };
        assert!(r.success());
    }

    #[test]
    fn compile_result_failure_when_errors_present() {
        let r = CompileResult {
            errors: vec!["ERROR line 1".into()],
            console: vec![],
        };
        assert!(!r.success());
    }

    // ── is_bare_prompt_line ───────────────────────────────────────────────────

    #[test]
    fn bare_prompt_user_is_stripped() {
        assert!(is_bare_prompt_line("USER>"));
    }

    #[test]
    fn bare_prompt_percent_sys_is_stripped() {
        assert!(is_bare_prompt_line("%SYS>"));
    }

    #[test]
    fn bare_prompt_with_trailing_space_is_stripped() {
        // is_bare_prompt_line receives trimmed input from strip_iris_banner,
        // but the function itself also trims_end internally.
        assert!(is_bare_prompt_line("USER>   "));
    }

    #[test]
    fn line_with_content_after_prompt_is_not_bare() {
        // "USER> 42" should NOT be treated as a bare prompt line
        assert!(!is_bare_prompt_line("USER> 42"));
    }

    #[test]
    fn non_prompt_line_is_not_bare() {
        assert!(!is_bare_prompt_line("42"));
        assert!(!is_bare_prompt_line(""));
        assert!(!is_bare_prompt_line("hello world"));
    }

    // ── strip_iris_banner — additional edge cases ─────────────────────────────

    #[test]
    fn strip_iris_banner_removes_copyright_line() {
        let raw = "Copyright (c) 2024 InterSystems Corporation\n42\n";
        let stripped = strip_iris_banner(raw);
        assert!(!stripped.contains("Copyright"), "{stripped:?}");
        assert_eq!(stripped.trim(), "42");
    }

    #[test]
    fn strip_iris_banner_removes_node_instance_line() {
        // IRIS 2026.2+ prints this on every `iris session` connect. Its embedded ':'
        // previously got misparsed as a name:code pair by callers like
        // interop::parse_status_response (production name came back as "Node").
        let raw = "\nNode: de17f22ad88c, Instance: IRIS\n\nUSER>\nIrisDevTest.CoverageProduction:1\n\nUSER>\n";
        let stripped = strip_iris_banner(raw);
        assert!(!stripped.contains("Node:"), "{stripped:?}");
        assert_eq!(stripped.trim(), "IrisDevTest.CoverageProduction:1");
    }

    #[test]
    fn strip_iris_banner_keeps_iris_for_line_after_first_prompt() {
        // Regression: `Write $ZVersion` legitimately outputs a string starting with
        // "IRIS for UNIX ..." — this must NOT be treated as the connect-time banner
        // just because it shares the same text prefix. Only strip banner-shaped text
        // that appears before the first prompt is seen.
        let raw = "\nNode: de17f22ad88c, Instance: IRIS\n\nUSER>\nIRIS for UNIX (Ubuntu Server LTS for ARM64 Containers) 2026.2.0L\n\nUSER>\n";
        let stripped = strip_iris_banner(raw);
        assert_eq!(
            stripped.trim(),
            "IRIS for UNIX (Ubuntu Server LTS for ARM64 Containers) 2026.2.0L"
        );
    }

    #[test]
    fn strip_iris_banner_keeps_content_lines_unchanged() {
        let raw = "USER>\nhello\nworld\nUSER>\n";
        let stripped = strip_iris_banner(raw);
        assert_eq!(stripped, "hello\nworld");
    }

    #[test]
    fn strip_iris_banner_no_prompts_at_all() {
        let raw = "line one\nline two\n";
        let stripped = strip_iris_banner(raw);
        assert_eq!(stripped, "line one\nline two");
    }

    #[test]
    fn strip_iris_banner_all_banner_gives_empty() {
        let raw =
            "Copyright (c) 2024 InterSystems Corporation\nAll rights reserved.\nIRIS for UNIX\n";
        let stripped = strip_iris_banner(raw);
        assert!(
            stripped.trim().is_empty(),
            "all banner → empty: {stripped:?}"
        );
    }

    #[test]
    fn strip_iris_banner_trims_leading_and_trailing_blank_lines() {
        let raw = "USER>\n\nhello\n\nUSER>\n";
        let stripped = strip_iris_banner(raw);
        assert_eq!(stripped.trim(), "hello");
    }

    // ── is_retryable_error classification logic ───────────────────────────────

    /// Helper to classify error messages as retryable or not.
    fn classify_error_as_retryable(msg: &str) -> bool {
        super::generator_retryable(&anyhow::anyhow!("{msg}"))
    }

    #[test]
    fn classify_http_5xx_as_retryable() {
        assert!(classify_error_as_retryable(
            "HTTP 500 Internal Server Error"
        ));
        assert!(classify_error_as_retryable("HTTP 502 Bad Gateway"));
        assert!(classify_error_as_retryable("HTTP 503 Service Unavailable"));
        assert!(classify_error_as_retryable("HTTP 504 Gateway Timeout"));
    }

    #[test]
    fn classify_http_4xx_as_not_retryable() {
        assert!(!classify_error_as_retryable("HTTP 401 Unauthorized"));
        assert!(!classify_error_as_retryable("HTTP 403 Forbidden"));
        assert!(!classify_error_as_retryable("HTTP 404 Not Found"));
        assert!(!classify_error_as_retryable("HTTP 400 Bad Request"));
    }

    #[test]
    fn classify_connection_refused_as_retryable() {
        assert!(classify_error_as_retryable("connection refused"));
        assert!(classify_error_as_retryable("Connection refused"));
        assert!(classify_error_as_retryable("error: connection refused"));
    }

    #[test]
    fn classify_connection_reset_as_retryable() {
        assert!(classify_error_as_retryable("connection reset"));
        assert!(classify_error_as_retryable("Connection reset by peer"));
    }

    #[test]
    fn classify_connection_closed_as_retryable() {
        assert!(classify_error_as_retryable("connection closed"));
        assert!(classify_error_as_retryable(
            "Connection closed unexpectedly"
        ));
    }

    #[test]
    fn classify_broken_pipe_as_retryable() {
        assert!(classify_error_as_retryable("broken pipe"));
        assert!(classify_error_as_retryable("Broken pipe"));
    }

    #[test]
    fn classify_eof_as_retryable() {
        assert!(classify_error_as_retryable("eof"));
        assert!(classify_error_as_retryable("EOF"));
        assert!(classify_error_as_retryable("unexpected end of file"));
        assert!(classify_error_as_retryable("Unexpected End of File"));
    }

    #[test]
    fn classify_dns_as_retryable() {
        assert!(classify_error_as_retryable("dns lookup failed"));
        assert!(classify_error_as_retryable("DNS resolution error"));
    }

    #[test]
    fn classify_handshake_as_retryable() {
        assert!(classify_error_as_retryable("tls handshake failed"));
        assert!(classify_error_as_retryable("Handshake error"));
    }

    #[test]
    fn classify_timeout_as_retryable() {
        assert!(classify_error_as_retryable("timed out"));
        assert!(classify_error_as_retryable("timeout"));
        assert!(classify_error_as_retryable("Timed Out"));
        assert!(classify_error_as_retryable("TIMEOUT"));
    }

    #[test]
    fn classify_error_sending_request_as_retryable() {
        assert!(classify_error_as_retryable("error sending request"));
        assert!(classify_error_as_retryable(
            "Error sending request to server"
        ));
    }

    #[test]
    fn classify_generic_error_as_not_retryable() {
        // Generic errors that don't match any retryable pattern → not retryable
        assert!(!classify_error_as_retryable("invalid json response"));
        assert!(!classify_error_as_retryable("query execution failed"));
        assert!(!classify_error_as_retryable(
            "compilation error: unexpected token"
        ));
    }

    #[test]
    fn classify_case_insensitivity() {
        // Most patterns are checked against lowercase version, so case variations work
        assert!(classify_error_as_retryable("Connection Refused"));
        assert!(classify_error_as_retryable("CONNECTION CLOSED"));
        assert!(classify_error_as_retryable("Error Sending Request"));
    }

    #[test]
    fn classify_combined_message() {
        // Error messages with multiple keywords — retryable if ANY keyword matches
        assert!(classify_error_as_retryable(
            "Failed to send request: connection refused after DNS lookup"
        ));
        assert!(classify_error_as_retryable(
            "HTTP 503 Service Unavailable: timeout waiting for backend"
        ));
    }

    #[test]
    fn classify_empty_string_as_not_retryable() {
        assert!(!classify_error_as_retryable(""));
    }

    #[test]
    fn classify_whitespace_only_as_not_retryable() {
        assert!(!classify_error_as_retryable("   "));
    }
}

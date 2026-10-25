//! The one place a call's target server is resolved and every gate runs on it (spec 141, R1).
//!
//! Until 1.7.0 each handler picked its own connection and ran whichever gates it carried against
//! the default connection's policy, whatever `server` said. [`IrisTools::dispatch_gated`] resolves
//! the target once, builds a [`ResolvedCall`] holding that server's connection, effective policy,
//! role, write gates and identity, runs every gate on it, and hands the handler the result through
//! a task-local whose accessors fail closed.
//!
//! Two tails share the pipeline. The MCP server's tail is the tool router; the CLI's and the
//! tests' is [`IrisTools::call_gated`], a match on the tool name. The CLI cannot build the
//! `RequestContext` the router needs, so the router and the name match are two tails of one
//! function, not two copies of the gates.

use crate::elicitation::Origin;
use crate::iris::config_sources::{ConfigSnapshot, SetAsideItem};
use crate::iris::connection::{DiscoverySource, IrisConnection, SystemMode};
use crate::iris::workspace_config::{
    check_role_gate, ConnectionPolicy, ConnectionRole, McpTemplate, ToolCategory,
};
use crate::policy::effective::{implied_template, EffectivePolicy};
use crate::tools::exec::{
    self, decide_identity, needs_service_account, os_user_opt_in, ExecRoute, IdentityDecision,
    IdentityInputs, IdentitySource, IdentityTarget, OptInSource, RouteTarget,
};
use crate::tools::write_gate::{
    self, classify, effective_category, member_needs_mode, operator_env_gates, tool_class,
    GateResolution, MemberDeclaration, TargetKind, TargetSource, WriteClass,
};
use crate::tools::{err_result, resolve_namespace, IrisTools};
use rmcp::model::CallToolResult;
use rmcp::ErrorData as McpError;
use serde_json::{json, Map, Value};
use std::future::Future;
use std::sync::Arc;

/// The error code of an accessor read outside a pipeline scope.
pub const ERR_TARGET_NOT_RESOLVED: &str = "TARGET_NOT_RESOLVED";

/// An elicitation id answered through a tool other than the one that opened it.
pub const ERR_ELICITATION_NOT_FOUND: &str = "ELICITATION_NOT_FOUND";

/// An elicitation id answered with a `server` other than the one that opened it.
pub const ERR_ELICITATION_SERVER_MISMATCH: &str = "ELICITATION_SERVER_MISMATCH";

/// Who runs the call on the target (R1 D9; R2 owns the execution paths).
#[derive(Clone)]
pub enum ExecIdentity {
    /// The connection's own account.
    Primary,
    /// The restricted service account, with its own HTTP client and cookie jar.
    Service {
        conn: Arc<IrisConnection>,
        client: Arc<reqwest::Client>,
        user: String,
        source: IdentitySource,
    },
    /// A service account is configured and unusable for this target; the call must be refused.
    Refused { code: &'static str, reason: String },
}

/// One server a call acts on, with everything the gates judge it by.
#[derive(Clone)]
pub struct ResolvedServer {
    /// The pool name; for the default connection the Server Manager name, else `default`.
    pub key: String,
    pub is_default: bool,
    /// `None` when the server is not connected. The gates still answer (085 FR-012).
    pub conn: Option<Arc<IrisConnection>>,
    /// The namespace this call acts in: the argument, else the connection's.
    pub namespace: String,
    /// The policy the gates read; never absent. An undeclared template is resolved from the
    /// server's system mode.
    pub policy: ConnectionPolicy,
    /// The same policy with where each part came from, for refusals and `check_config`.
    pub effective: EffectivePolicy,
    pub set_aside: Vec<SetAsideItem>,
    pub role: ConnectionRole,
    /// The `[instance.*]` key, for messages.
    pub instance: String,
    pub gates: GateResolution,
    pub identity: ExecIdentity,
    /// Where the docker leg of this target runs (R2 section 2).
    pub route: ExecRoute,
    /// The operator allowed running as the container's OS user when the login guard cannot work.
    pub os_user_opt_in: Option<OptInSource>,
    /// A `[policy.<key>]` section exists or the template is not `dev`: the server is audited.
    pub audited: bool,
}

/// What the pipeline resolved for one call.
#[derive(Clone)]
pub struct ResolvedCall {
    pub tool: String,
    /// The classification, or the pending entry's on an elicitation resume.
    pub class: WriteClass,
    pub categories: Vec<ToolCategory>,
    pub kind: TargetKind,
    pub primary: ResolvedServer,
    /// The second server of `compare_*`.
    pub others: Vec<ResolvedServer>,
    pub config: Arc<ConfigSnapshot>,
}

impl ResolvedServer {
    /// The server's data policy as the tools that scrub or refuse bulk data read it.
    pub fn data_policy_name(&self) -> &'static str {
        use crate::iris::workspace_config::DataPolicy;
        match self.policy.data_policy {
            Some(DataPolicy::Allow) => "allow",
            Some(DataPolicy::Redact) => "redact",
            _ => "block",
        }
    }
}

/// The stricter of a server's data policy and the one the caller named. A caller can narrow what
/// the server allows and never widen it; a missing or unknown value is `block`.
pub fn narrow_data_policy(server: &'static str, caller: Option<&str>) -> &'static str {
    fn rank(p: &str) -> u8 {
        match p {
            "allow" => 0,
            "redact" => 1,
            _ => 2,
        }
    }
    let caller = caller.unwrap_or("block");
    if rank(caller) > rank(server) {
        match rank(caller) {
            1 => "redact",
            _ => "block",
        }
    } else {
        server
    }
}

impl ResolvedCall {
    /// Every server the call acts on, the primary first.
    pub fn servers(&self) -> impl Iterator<Item = &ResolvedServer> {
        std::iter::once(&self.primary).chain(self.others.iter())
    }
}

tokio::task_local! {
    /// The call in progress. Set by `dispatch_gated` around the tail.
    pub(crate) static CALL_TARGET: Arc<ResolvedCall>;
}

/// The tool, server and class of the call in progress, for an elicitation entry it opens.
pub fn current_origin() -> Option<Origin> {
    CALL_TARGET
        .try_with(|t| Origin {
            tool: t.tool.clone(),
            target_key: if t.primary.is_default {
                None
            } else {
                Some(t.primary.key.clone())
            },
            class: t.class,
        })
        .ok()
}

/// The compile-time gate's switch for the target server (R4a D9) and where its value came from.
#[derive(Debug, Clone)]
pub struct CompileGate {
    /// `true` is the default: documents are scanned for code that runs while they compile.
    pub on: bool,
    /// `<from> <file>`, as a put or compile response reports it.
    pub source: String,
}

impl CompileGate {
    fn of(server: &ResolvedServer) -> Self {
        let part = &server.effective.compile_gate;
        let source = match &part.file {
            Some(f) => format!("{} {}", part.from.as_str(), f.to_string_lossy()),
            None => part.from.as_str().to_string(),
        };
        Self {
            on: part.value,
            source,
        }
    }

    /// Say in `response` that this call ran with the gate off (spec edge case 5). An on gate adds
    /// nothing.
    pub fn note_if_off(&self, response: &mut Value) {
        if self.on {
            return;
        }
        response["compile_time_gate"] = json!("off");
        response["compile_time_gate_source"] = json!(self.source);
    }
}

/// The compile-time gate switch of the call in progress. Fails closed outside a pipeline scope.
pub fn compile_gate() -> Result<CompileGate, McpError> {
    CALL_TARGET
        .try_with(|t| CompileGate::of(&t.primary))
        .map_err(|_| not_resolved())
}

/// The error a handler sees when it reads its target outside a pipeline scope.
fn not_resolved() -> McpError {
    McpError::internal_error(
        format!(
            "{ERR_TARGET_NOT_RESOLVED}: no call is in progress. A handler read its target outside \
             the pipeline, or from a spawned task that does not carry the scope."
        ),
        None,
    )
}

/// A [`McpError`] seen from [`IrisTools::call_gated`], which reports errors as text.
pub(crate) struct GatedError(pub String);

impl From<McpError> for GatedError {
    fn from(e: McpError) -> Self {
        GatedError(format!("{e:?}"))
    }
}

/// Why resolution stopped before any gate ran.
enum Early {
    /// A refusal, sent to the caller as an error result.
    Refused(CallToolResult),
    /// A protocol-level error, sent as is (an unknown server).
    Error(McpError),
}

impl From<McpError> for Early {
    fn from(e: McpError) -> Self {
        Early::Error(e)
    }
}

fn input_refusal(code: &str, message: String) -> Early {
    match err_result(json!({ "success": false, "error_code": code, "error": message })) {
        Ok(r) => Early::Refused(r),
        Err(e) => Early::Error(e),
    }
}

/// The body of the `READ_ONLY_STRICT` refusal. `from` names the file or launch setting that asked
/// for strict read-only.
fn read_only_strict_body(tool: &str, from: &str) -> Value {
    json!({
        "success": false,
        "error_code": write_gate::ERR_READ_ONLY_STRICT,
        "error": format!(
            "{tool} runs through a scratch class that iad compiles on the server, and read-only \
             mode is strict ({from}), so it is off. Tools that compile nothing still work."
        ),
        "tool": tool,
        "read_only_from": from,
    })
}

/// Two URLs name one instance when scheme, host, port and prefix agree; `localhost`, `127.0.0.1`
/// and `::1` are one host.
pub fn same_instance(a: &str, b: &str) -> bool {
    fn norm(u: &str) -> Option<String> {
        let url = reqwest::Url::parse(u.trim()).ok()?;
        let host = match url.host_str()? {
            "localhost" | "127.0.0.1" | "[::1]" | "::1" => "localhost".to_string(),
            h => h.to_ascii_lowercase(),
        };
        Some(format!(
            "{}://{}:{}{}",
            url.scheme(),
            host,
            url.port_or_known_default().unwrap_or(0),
            url.path().trim_end_matches('/')
        ))
    }
    match (norm(a), norm(b)) {
        (Some(x), Some(y)) => x == y,
        _ => a.trim_end_matches('/') == b.trim_end_matches('/'),
    }
}

/// The role and `[instance.*]` key of a connection (R1 D5).
///
/// In operate mode the connection is matched against the declared instances by container name or
/// host, then by namespace; `(Workbench, "")` when nothing matches, there is no fleet, or the mode
/// is not `operate`. The role is the effective one: a workspace file that has not been trusted
/// cannot make a subject server a workbench. `namespace` is the one the call acts in, which is
/// not always the connection's own.
pub fn resolve_role(
    snap: &ConfigSnapshot,
    conn: Option<&IrisConnection>,
    namespace: &str,
) -> (ConnectionRole, String) {
    let fleet = &snap.fleet;
    if snap.operator_file.is_none() && snap.workspace_layer.is_none() {
        return (ConnectionRole::Workbench, String::new());
    }
    if fleet.mode.as_deref() != Some("operate") {
        return (ConnectionRole::Workbench, String::new());
    }
    let Some(iris) = conn else {
        return (ConnectionRole::Workbench, String::new());
    };

    // Active container name from DiscoverySource or IRIS_CONTAINER env var fallback.
    let active_container = match &iris.source {
        DiscoverySource::Docker { container_name } => Some(container_name.clone()),
        _ => iris.default_container(),
    };
    let active_ns = namespace.to_uppercase();

    // Two-pass match to handle shared-gateway fleets (#114):
    //   Pass 1: host (or container) AND namespace — most specific.
    //   Pass 2: host (or container) only, ignoring namespace — fallback for
    //           instances that omit namespace in their config.
    // This prevents a subject-role entry on the same host from shadowing a workbench-role entry
    // that also declares the active namespace.
    for pass in 0..2u8 {
        for (name, inst) in &fleet.instance {
            let host_matches = if let Some(ref ic) = inst.container {
                active_container.as_deref() == Some(ic.as_str())
            } else {
                inst.host
                    .as_deref()
                    .map(|h| iris.base_url.contains(&format!("://{h}:")))
                    .unwrap_or(false)
            };
            if !host_matches {
                continue;
            }
            let ns_matches = inst
                .namespace
                .as_deref()
                .map(|ns| ns.to_uppercase() == active_ns)
                .unwrap_or(false);
            let matches = if pass == 0 {
                ns_matches && inst.namespace.is_some()
            } else {
                true
            };
            if matches {
                return (inst.role, name.clone());
            }
        }
    }
    (ConnectionRole::Workbench, String::new())
}

/// The write gates of a pool member that is the default connection under another name.
///
/// It is the same instance, so the operator's launch settings and the default's own resolution
/// apply to it; the member can only close them further, with `[policy.<name>] writeTools = false`
/// or `destructiveTools = false`.
fn alias_gates(default: GateResolution, decl: &MemberDeclaration) -> GateResolution {
    let mut g = default;
    if g.write_source == write_gate::GateSource::ReadOnlyStrict {
        return g;
    }
    if decl.write_tools == Some(false) {
        g.write_enabled = false;
        g.write_source = write_gate::GateSource::PolicyDeclaration;
    }
    if decl.destructive_tools == Some(false) {
        g.destructive_enabled = false;
        g.destructive_source = write_gate::GateSource::PolicyDeclaration;
    }
    g.destructive_enabled = g.destructive_enabled && g.write_enabled;
    g
}

fn non_blank_env(name: &str) -> Option<String> {
    std::env::var(name).ok().filter(|v| !v.trim().is_empty())
}

/// `IAD_EXEC_AS_OS_USER`, the operator's environment opt-in to run as the container's OS user.
fn exec_as_os_user_env() -> Option<String> {
    non_blank_env("IAD_EXEC_AS_OS_USER")
}

/// What a refused identity says. The code is the one the caller sees; the text names how to lift it.
fn refusal_reason(code: &str, server: &str, withheld: Option<&str>) -> String {
    match code {
        exec::ERR_SERVICE_ACCOUNT_UNCONFIGURED => format!(
            "IRIS_SERVICE_USERNAME is set, and server `{server}` has no service account of its \
             own, so this tool will not run as that server's primary user."
        ),
        exec::ERR_SERVICE_ACCOUNT_WITHHELD => format!(
            "This tool runs as the service account, and {}",
            withheld.unwrap_or("the service account's credentials were withheld")
        ),
        other => other.to_string(),
    }
}

/// Categories the environment template can block. A server whose template nothing declared needs
/// its system mode read before a call in one of them can be judged.
fn template_matters(categories: &[ToolCategory]) -> bool {
    categories.iter().any(|c| {
        matches!(
            c,
            ToolCategory::Execute | ToolCategory::Compile | ToolCategory::SourceControl
        )
    })
}

/// The keys of an argument map that are safe to put in an audit entry: names and places, never
/// code, content or passwords.
fn audit_params(args: &Map<String, Value>, namespace: &str) -> Value {
    let mut out = Map::new();
    out.insert("namespace".to_string(), json!(namespace));
    for key in [
        "mode",
        "action",
        "target",
        "name",
        "document",
        "global",
        "global_name",
        "globalName",
        "class",
        "method",
        "pattern",
    ] {
        if let Some(Value::String(s)) = args.get(key) {
            out.insert(key.to_string(), json!(s));
        }
    }
    Value::Object(out)
}

impl IrisTools {
    /// The resolved call. Fails closed outside a pipeline scope.
    ///
    /// A handler that spawns a task must pass what it needs in; the scope does not follow a
    /// `tokio::spawn`. A handler that calls another IRIS tool must go through the pipeline again.
    pub fn target(&self) -> Result<Arc<ResolvedCall>, McpError> {
        CALL_TARGET.try_with(Arc::clone).map_err(|_| not_resolved())
    }

    /// The primary target's connection, or `IRIS_UNREACHABLE`.
    pub fn target_conn(&self) -> Result<Arc<IrisConnection>, McpError> {
        let t = self.target()?;
        t.primary
            .conn
            .clone()
            .ok_or_else(crate::tools::iris_unreachable)
    }

    /// The primary target's connection when it has one. For the tools that answer without IRIS
    /// (or that report its absence), where [`Self::target_conn`] would refuse.
    pub fn target_conn_opt(&self) -> Result<Option<Arc<IrisConnection>>, McpError> {
        Ok(self.target()?.primary.conn.clone())
    }

    /// The connection and HTTP client to run privileged code with: the primary target's own, or its
    /// service account's with a client of its own, or the refusal.
    pub fn target_exec(&self) -> Result<(Arc<IrisConnection>, Arc<reqwest::Client>), McpError> {
        let t = self.target()?;
        match &t.primary.identity {
            ExecIdentity::Refused { reason, .. } => {
                Err(McpError::invalid_request(reason.clone(), None))
            }
            ExecIdentity::Service { conn, client, .. } => Ok((conn.clone(), client.clone())),
            ExecIdentity::Primary => {
                let conn = t
                    .primary
                    .conn
                    .clone()
                    .ok_or_else(crate::tools::iris_unreachable)?;
                Ok((conn, Arc::clone(&self.client)))
            }
        }
    }

    /// The connection of the pool member `name` names, for a tool whose `name` may also be an ad-hoc
    /// host (`iris_test_server`): a name the pool does not hold is `SERVER_NOT_FOUND`, never the
    /// default connection the pipeline falls back to for it.
    pub fn target_pool_member(&self, name: &str) -> Result<Arc<IrisConnection>, McpError> {
        if self.target()?.kind == TargetKind::Local {
            return Err(crate::iris::connection_pool::server_not_found(name));
        }
        self.target_conn()
    }

    /// The two connections of a `compare_*` call: `server_a` and `server_b`, both resolved and
    /// gated by the pipeline.
    pub fn target_pair(&self) -> Result<(Arc<IrisConnection>, Arc<IrisConnection>), McpError> {
        let t = self.target()?;
        let conn = |s: &ResolvedServer| s.conn.clone().ok_or_else(crate::tools::iris_unreachable);
        let b = t.others.first().ok_or_else(not_resolved)?;
        Ok((conn(&t.primary)?, conn(b)?))
    }

    /// Everything [`exec::ExecContext::run`] needs, from the call's resolved target: who runs the
    /// code, over which connection and client, and where the docker leg is. A refused identity, a
    /// server that is not connected and a read outside a pipeline scope are errors in the shape the
    /// handlers return.
    pub fn exec_context(&self) -> Result<exec::ExecContext, exec::ExecError> {
        self.exec_context_as(true)
    }

    /// [`Self::exec_context`] that runs as the connection's own user whatever identity the call
    /// resolved: for the code iad wrote itself within a call that also runs the agent's (the
    /// monitor stop and report of `iris_test` with coverage).
    pub fn primary_exec_context(&self) -> Result<exec::ExecContext, exec::ExecError> {
        self.exec_context_as(false)
    }

    fn exec_context_as(&self, with_identity: bool) -> Result<exec::ExecContext, exec::ExecError> {
        let t = self
            .target()
            .map_err(|e| exec::ExecError::new(ERR_TARGET_NOT_RESOLVED, e.message.to_string()))?;
        let s = &t.primary;
        let conn = s.conn.clone().ok_or_else(|| {
            exec::ExecError::new("IRIS_UNREACHABLE", format!("{} is not connected.", s.key))
        })?;
        let identity = if with_identity {
            &s.identity
        } else {
            &ExecIdentity::Primary
        };
        let (conn, client, expected, source) = match identity {
            ExecIdentity::Refused { code, reason } => {
                return Err(exec::ExecError::new(code, reason.clone()));
            }
            ExecIdentity::Service {
                conn,
                client,
                user,
                source,
            } => (conn.clone(), client.clone(), Some(user.clone()), *source),
            ExecIdentity::Primary => (
                conn,
                Arc::clone(&self.client),
                None,
                IdentitySource::Primary,
            ),
        };
        Ok(exec::ExecContext {
            ssh_strict: conn.member.ssh_strict,
            conn,
            client,
            expected,
            source,
            server: s.key.clone(),
            route: s.route.clone(),
            opt_in: s.os_user_opt_in,
            named_server: !s.is_default,
            env_container_set: crate::iris::connection::container_from_env().is_some(),
        })
    }

    /// `check_config`'s `identity` object (R2 section 4): the service account, the OS-user opt-in,
    /// the user each path would run as and each server's. `http_login` is the answer of
    /// [`Self::service_login_probe`].
    pub fn identity_report_json(
        &self,
        snap: &ConfigSnapshot,
        default: Option<Arc<IrisConnection>>,
        http_login: Option<String>,
    ) -> Value {
        let env_account = non_blank_env("IRIS_SERVICE_USERNAME");
        let default_route = default
            .as_deref()
            .map(|c| exec::route_for_connection(c, RouteTarget::DefaultOrEnv))
            .unwrap_or(ExecRoute::Http);
        let pool = self.pool_ref();
        let members: Vec<exec::MemberIdentity> = pool
            .names()
            .into_iter()
            .filter_map(|name| {
                let c = pool.peek(Some(name)).ok()?;
                let target = if name == "_env" {
                    IdentityTarget::EnvMember
                } else if default
                    .as_deref()
                    .is_some_and(|d| same_instance(&d.base_url, &c.base_url))
                {
                    IdentityTarget::AliasOfDefault
                } else {
                    IdentityTarget::Member
                };
                let decision = match decide_identity(&IdentityInputs {
                    env_account: env_account.as_deref(),
                    withheld: c
                        .credentials_withheld
                        .as_deref()
                        .is_some_and(|n| n.contains("IRIS_SERVICE_USERNAME")),
                    target,
                    own_account: c.member.service_username.as_deref(),
                }) {
                    IdentityDecision::Primary => {
                        json!({"user": c.username, "source": IdentitySource::Primary.as_str()})
                    }
                    IdentityDecision::Service { user, source } => {
                        json!({"user": user, "source": source.as_str()})
                    }
                    IdentityDecision::Refused { code } => json!({"refused": code}),
                };
                let route_target = if matches!(target, IdentityTarget::Member) {
                    RouteTarget::Member
                } else {
                    RouteTarget::DefaultOrEnv
                };
                Some(exec::MemberIdentity {
                    name: name.to_string(),
                    decision,
                    route: exec::route_for_connection(&c, route_target),
                })
            })
            .collect();
        let mut block = exec::identity_block(
            snap,
            env_account.as_deref(),
            exec_as_os_user_env().as_deref(),
            &default_route,
            &members,
        );
        if let Some(login) = http_login {
            if block["paths"]["http"].is_object() {
                block["paths"]["http"]["login"] = json!(login);
            }
        }
        block
    }

    /// One `GET /api/atelier/` with the service account's credentials, through a client with no
    /// cookie jar and no redirects: does the account log in to the Atelier API? It sends no code
    /// and writes nothing. `None` when no service account is set or the default route has no HTTP
    /// leg.
    pub async fn service_login_probe(&self) -> Option<String> {
        let user = non_blank_env("IRIS_SERVICE_USERNAME")?;
        let conn = self.connection.lock().unwrap().iris.clone()?;
        if conn.credentials_withheld.is_some() {
            return Some("withheld".to_string());
        }
        if matches!(
            exec::route_for_connection(&conn, RouteTarget::DefaultOrEnv),
            ExecRoute::DockerOnly { .. } | ExecRoute::DockerOnlyNoContainer
        ) {
            return None;
        }
        let password = std::env::var("IRIS_SERVICE_PASSWORD").unwrap_or_default();
        let client = conn.client(crate::iris::http::ClientKind::OwnProbe).ok()?;
        let reply = client
            .get(conn.atelier_url("/"))
            .basic_auth(&user, Some(&password))
            .send()
            .await;
        Some(match reply {
            Ok(r) if r.status().is_success() => "ok".to_string(),
            Ok(r) => format!(
                "denied (HTTP {}): the account cannot log in to /api/atelier. It needs \
                 %Development:USE (the application's resource) and password authentication on the \
                 application",
                r.status().as_u16()
            ),
            Err(_) => "unreachable".to_string(),
        })
    }

    /// The namespace to act in: the argument when given, else the target's.
    pub fn target_ns(&self, requested: Option<&str>) -> Result<String, McpError> {
        let t = self.target()?;
        Ok(resolve_namespace(requested, &t.primary.namespace).to_string())
    }

    /// The tool, with `args`, through every gate and then `tail` (R1 D1, D8).
    ///
    /// `tail` receives the normalized arguments and runs inside the scope [`Self::target`] reads.
    pub(crate) async fn dispatch_gated<R, E, F, Fut>(
        &self,
        name: &str,
        args: Option<Map<String, Value>>,
        tail: F,
    ) -> Result<R, E>
    where
        R: From<CallToolResult>,
        E: From<McpError>,
        F: FnOnce(Option<Map<String, Value>>) -> Fut,
        Fut: Future<Output = Result<R, E>>,
    {
        // Reload before resolving the gate, not after. A gate resolved ahead of dispatch would
        // answer from the previous load and let one more write through after the operator turned
        // writes off. FR-002 has to hold on the very next call, in both directions.
        self.check_reload().await;
        if let Some(invalid) = self.config_invalid(name) {
            self.record_refused(name);
            return invalid.map(Into::into).map_err(Into::into);
        }
        let mut args = args;
        if let Some(a) = args.as_mut() {
            write_gate::normalize_dispatch_args(a);
        }
        let empty = Map::new();
        let arg_map = args.as_ref().unwrap_or(&empty);

        let resolved = match self.resolve_call(name, arg_map).await {
            Ok(r) => r,
            Err(Early::Refused(result)) => {
                self.record_refused(name);
                return Ok(result.into());
            }
            Err(Early::Error(e)) => return Err(e.into()),
        };

        // The write/destructive gate comes first: it is the security answer, and it outranks the
        // usability ones below. A caller with writes disabled who also misspells a key on
        // `global_kill` is told the gate refused, not that they made a typo.
        for server in resolved.servers() {
            if let Some(Ok(refusal)) =
                write_gate::gate_for_class(name, resolved.class, &server.gates)
            {
                self.record_refused(name);
                return Ok(self.classify_on(server, refusal).into());
            }
        }

        // Strict read-only (R5 C15): a tool that can compile a scratch class on the server is
        // refused, whatever its write class. It comes after the write gate so a caller with writes
        // off hears about that first, and before the usability checks below.
        if let Some(from) = resolved.config.read_only_source() {
            if write_gate::is_scratch(name) {
                self.record_call(name, false);
                return self
                    .refusal_on(&resolved.primary, read_only_strict_body(name, &from))
                    .map(Into::into)
                    .map_err(Into::into);
            }
        }

        // Unknown-parameter check (113 FR-015/FR-016): ahead of the handler, so a rejected call
        // never enters it and nothing half-applies. One site for every tool, off the advertised
        // schema.
        if let Some(accepted) = self.accepted_params.get(name) {
            let unknown = crate::tools::param_check::unknown_keys(Some(arg_map), &accepted.names);
            if !unknown.is_empty() {
                self.record_refused(name);
                return err_result(json!({
                    "success": false,
                    "error_code": crate::tools::param_check::ERR_UNKNOWN_PARAMETER,
                    "error": crate::tools::param_check::unknown_parameter_message(
                        name, &unknown, &accepted.names,
                    ),
                }))
                .map(Into::into)
                .map_err(Into::into);
            }
        }

        // Namespace check (137). Every tool that takes a namespace splices it into generated code
        // (`New $NAMESPACE Set $NAMESPACE="…"`, `zn`, a URL), and `resolve_namespace` accepts any
        // non-empty string.
        for (key, value) in arg_map {
            if key != "namespace" && !key.ends_with("_namespace") {
                continue;
            }
            let Some(ns) = value.as_str().filter(|s| !s.is_empty()) else {
                continue;
            };
            if let Err(e) = crate::objectscript::validate_namespace(ns) {
                self.record_refused(name);
                return err_result(e).map(Into::into).map_err(Into::into);
            }
        }

        // The policy gates, in R1 D8's order, on every IRIS server the call acts on. They read the
        // whole argument map, not a handler's subset of it.
        if resolved.kind == TargetKind::Iris {
            let full = Value::Object(arg_map.clone());
            for server in resolved.servers() {
                if let Some(refusal) = self.policy_refusal_on(server, &resolved, &full) {
                    self.audit_call(name, server, arg_map, "blocked", Some("policy"));
                    self.record_refused(name);
                    return Ok(refusal.into());
                }
            }
        }
        self.audit_call(name, &resolved.primary, arg_map, "allowed", None);

        // T019: warn once when the connection is docker_only so operators know that HTTP headers
        // (including User-Agent) cannot be set on this transport.
        if resolved.primary.conn.as_ref().is_some_and(|i| {
            i.base_url == "http://127.0.0.1:1" || i.base_url.starts_with("http://127.0.0.1:1/")
        }) && !self
            .docker_only_attr_warned
            .swap(true, std::sync::atomic::Ordering::Relaxed)
        {
            tracing::warn!(
                "attribution unavailable: docker_only connection uses docker exec rather than \
                 HTTP, so no User-Agent header is sent to IRIS. To make agent traffic \
                 identifiable, use an HTTP connection."
            );
        }

        // An IRIS answer over the size cap is recorded by whichever reader met it, and most readers
        // fall back to an empty value. The call's answer is then untrustworthy, so a successful one
        // becomes `RESPONSE_TOO_LARGE` (spec 141 T221).
        let oversize = Arc::new(std::sync::Mutex::new(None));
        let out = crate::iris::http::OVERSIZE
            .scope(
                oversize.clone(),
                CALL_TARGET.scope(Arc::new(resolved), tail(args)),
            )
            .await;
        let too_large = oversize.lock().ok().and_then(|mut c| c.take());
        match (out, too_large) {
            (Ok(_), Some(e)) => {
                self.record_refused(name);
                err_result(json!({
                    "success": false,
                    "error_code": "RESPONSE_TOO_LARGE",
                    "error": e.to_string(),
                    "limit_bytes": e.limit,
                }))
                .map(Into::into)
                .map_err(Into::into)
            }
            (out, _) => out,
        }
    }

    /// Run `tool` by name through the pipeline. The CLI and the tests use this; the MCP server's
    /// tail is the router.
    pub async fn call_gated(&self, tool: &str, params: Value) -> Result<CallToolResult, String> {
        if self.strict_toolset && !self.accepted_params.contains_key(tool) {
            return Err(format!("unknown tool: {tool}"));
        }
        let args = match params {
            Value::Object(m) => Some(m),
            Value::Null => None,
            other => {
                return Err(format!(
                    "bad params for {tool}: expected an object, got {other}"
                ))
            }
        };
        self.dispatch_gated(tool, args, |args| async move {
            let params = args.map(Value::Object).unwrap_or_else(|| json!({}));
            let (answer, scratch_left) = crate::iris::connection::collecting_scratch_left(
                self.dispatch_by_name(tool, params),
            )
            .await;
            answer
                .map(|result| super::with_warnings(result, scratch_left))
                .map_err(GatedError)
        })
        .await
        .map_err(|e: GatedError| e.0)
    }

    // ── refusals ────────────────────────────────────────────────────────────────────────────

    /// A gate's refusal result for `server`, with the `refusal` object the contract describes.
    fn classify_on(&self, server: &ResolvedServer, result: CallToolResult) -> CallToolResult {
        let Some(body) = result.structured_content.clone() else {
            return result;
        };
        self.refusal_on(server, body).unwrap_or(result)
    }

    /// A gate's body as an error result judged on `server`: the body names the server, and gains
    /// `refusal {kind, server, lift, set_aside}`.
    pub(crate) fn refusal_on(
        &self,
        server: &ResolvedServer,
        gate: Value,
    ) -> Result<CallToolResult, McpError> {
        let body = match gate {
            Value::Object(mut o) => {
                o.entry("server_name").or_insert(json!(server.key));
                Value::Object(o)
            }
            other => other,
        };
        let refused =
            crate::policy::refusal::classify_refusal(body, &server.key, &server.set_aside);
        // The recipe goes to the operator's stderr, not into the result the agent reads.
        if let Some(remedy) = crate::policy::refusal::refusal_remedy(
            &refused,
            self.snapshot().operator_path.as_deref(),
            &server.set_aside,
        ) {
            crate::policy::refusal::announce_remedy(&remedy);
        }
        err_result(refused)
    }

    /// The policy gates of one server, as a refusal result: environment template, bulk PHI, system
    /// blocklist, PHI names, `allow`, then the role gate.
    fn policy_refusal_on(
        &self,
        server: &ResolvedServer,
        call: &ResolvedCall,
        args: &Value,
    ) -> Option<CallToolResult> {
        let refuse = |gate: Value| self.refusal_on(server, gate).ok();
        if let Err(gate) = crate::policy::gate::dispatch_gate_for(
            &call.tool,
            &server.key,
            &server.policy,
            &call.categories,
            args,
        ) {
            return refuse(gate);
        }
        if let Some(gate) = crate::iris::server_manager::policy_gate_for(
            &call.tool,
            &server.key,
            &server.policy,
            &call.categories,
        ) {
            return refuse(gate);
        }
        self.role_refusal(server, call, args).and_then(refuse)
    }

    /// The role gate (R1 D5): a write or destructive call on a subject server needs `confirm`; a
    /// source-control write is refused outright; a read-mode `iris_query` that is not a `SELECT`
    /// or `WITH` (a `CALL`) needs `confirm` too.
    fn role_refusal(
        &self,
        server: &ResolvedServer,
        call: &ResolvedCall,
        args: &Value,
    ) -> Option<Value> {
        if server.role != ConnectionRole::Subject {
            return None;
        }
        let confirm = args.get("confirm").and_then(|v| v.as_bool()) == Some(true);
        let writes = call.class != WriteClass::ReadOnly;
        match call.tool.as_str() {
            "iris_source_control" if writes => check_role_gate(
                &server.role,
                "iris_source_control:commit",
                confirm,
                &server.instance,
                true,
            ),
            "iris_query" => {
                let first = args
                    .get("query")
                    .and_then(|v| v.as_str())
                    .and_then(|q| q.split_whitespace().next())
                    .unwrap_or("")
                    .to_uppercase();
                let read_statement = first.is_empty() || first == "SELECT" || first == "WITH";
                if writes || !read_statement {
                    check_role_gate(
                        &server.role,
                        "iris_query:INSERT",
                        confirm,
                        &server.instance,
                        false,
                    )
                } else {
                    None
                }
            }
            tool if writes => check_role_gate(&server.role, tool, confirm, &server.instance, false),
            _ => None,
        }
    }

    /// One audit entry per decision, against the server the call acts on. The `%SYS.Audit`
    /// emission goes to that server's connection, not the default's (R1 D8.9).
    fn audit_call(
        &self,
        tool: &str,
        server: &ResolvedServer,
        args: &Map<String, Value>,
        status: &str,
        gate: Option<&str>,
    ) {
        let policy = server.audited.then_some(&server.policy);
        self.write_audit_entry_on(
            tool,
            &server.key,
            server.conn.clone(),
            policy,
            status,
            gate,
            None,
            audit_params(args, &server.namespace),
        );
    }

    // ── resolution ──────────────────────────────────────────────────────────────────────────

    /// Find the servers a call acts on and judge each by its own policy, role and gates (R1 D3).
    async fn resolve_call(
        &self,
        tool: &str,
        args: &Map<String, Value>,
    ) -> Result<ResolvedCall, Early> {
        let snap = self.snapshot();
        let row = tool_class(tool);
        let (source, mut kind) = row
            .map(|r| (r.target, r.kind))
            .unwrap_or((TargetSource::Arg("server"), TargetKind::Iris));
        let mut class = classify(tool, Some(args)).unwrap_or(WriteClass::Write);
        let text = |k: &str| {
            args.get(k)
                .and_then(|v| v.as_str())
                .map(str::trim)
                .filter(|s| !s.is_empty())
        };

        // `None` is the default connection.
        let mut keys: Vec<Option<String>> = Vec::new();
        let mut ns_arg: Option<String> = text("namespace").map(str::to_string);
        match source {
            TargetSource::Arg(a) => keys.push(text(a).map(str::to_string)),
            TargetSource::Args(list) => {
                for a in list {
                    keys.push(text(a).map(str::to_string));
                }
            }
            TargetSource::Name(n) => match text(n) {
                // Only a call that names a server loads the pool (and may open the keychain).
                Some(name) if self.pool_ref().names().contains(&name) => {
                    keys.push(Some(name.to_string()))
                }
                _ => {
                    // An ad-hoc host, not a pool member: there is no server policy to judge.
                    keys.push(None);
                    kind = TargetKind::Local;
                }
            },
            TargetSource::Session => {
                let token = text("session");
                if let Some((_, ns, _)) =
                    token.and_then(crate::iris::ws_session::WsSessionPool::parse_token)
                {
                    ns_arg = Some(ns);
                }
                keys.push(token.and_then(|t| self.ws_pool.target_of(t)).flatten());
            }
            TargetSource::Default => keys.push(None),
            TargetSource::ArgOrPending(a) => {
                let named = text(a).map(str::to_string);
                let pending =
                    text("elicitation_id").and_then(|id| self.elicitation_store.lookup(id));
                match pending {
                    Some(p) => {
                        if p.origin.tool != tool {
                            return Err(input_refusal(
                                ERR_ELICITATION_NOT_FOUND,
                                "Elicitation session expired or not found".to_string(),
                            ));
                        }
                        if named != p.origin.target_key {
                            return Err(input_refusal(
                                ERR_ELICITATION_SERVER_MISMATCH,
                                format!(
                                    "This elicitation was opened on {}. Answer it with {}.",
                                    p.origin
                                        .target_key
                                        .as_deref()
                                        .unwrap_or("the default server"),
                                    match &p.origin.target_key {
                                        Some(k) => format!("server = \"{k}\""),
                                        None => "no `server` argument".to_string(),
                                    }
                                ),
                            ));
                        }
                        class = p.origin.class;
                        keys.push(p.origin.target_key);
                    }
                    None => keys.push(named),
                }
            }
        }

        let categories = effective_category(tool, Some(args), class);
        let wants_service = needs_service_account(tool, &Value::Object(args.clone()));
        let mut servers = Vec::new();
        for key in &keys {
            let server = match key {
                None => self.resolve_default(&snap, ns_arg.as_deref(), wants_service),
                Some(name) => {
                    self.resolve_member(
                        &snap,
                        name,
                        ns_arg.as_deref(),
                        class,
                        &categories,
                        wants_service,
                    )
                    .await?
                }
            };
            servers.push(server);
        }
        let mut it = servers.into_iter();
        let primary = it.next().expect("a call has a primary server");
        Ok(ResolvedCall {
            tool: tool.to_string(),
            class,
            categories,
            kind,
            primary,
            others: it.collect(),
            config: snap,
        })
    }

    /// The default connection, judged on the namespace the call acts in.
    fn resolve_default(
        &self,
        snap: &Arc<ConfigSnapshot>,
        ns_arg: Option<&str>,
        wants_service: bool,
    ) -> ResolvedServer {
        let (conn, state_gates, declared) = {
            let c = self.connection.lock().unwrap();
            (c.iris.clone(), c.gates, c.declared)
        };
        let conn_ns = conn
            .as_ref()
            .map(|c| c.namespace.clone())
            .unwrap_or_else(|| "USER".to_string());
        let namespace = resolve_namespace(ns_arg, &conn_ns).to_string();
        let mode = conn.as_ref().map(|c| c.system_mode).unwrap_or_default();

        // A Server Manager connection is judged by its own name; every other source by `default`.
        let key = match conn.as_ref().map(|c| &c.source) {
            Some(DiscoverySource::ServerManager { server_name }) => server_name.clone(),
            _ => "default".to_string(),
        };
        let implied = implied_template(mode, true, &namespace).0;
        let effective = snap.effective_policy_for(&key, Some(implied.clone()));
        let set_aside = snap.set_aside_for(&key, &effective);
        let mut policy = effective.to_connection_policy();
        if policy.mcp_template.is_none() {
            policy.mcp_template = Some(implied);
        }
        let audited =
            snap.has_policy_section(&key) || policy.mcp_template != Some(McpTemplate::Dev);

        let (role, instance) = resolve_role(snap, conn.as_deref(), &namespace);

        // The connection's gates were resolved against its own namespace. When the system mode
        // could not decide, the namespace the call acts in does (R1 5.1).
        let gates =
            if matches!(mode, SystemMode::Unknown | SystemMode::Unset) && namespace != conn_ns {
                write_gate::resolve_declared(operator_env_gates(), declared, mode, &namespace)
            } else {
                state_gates
            };

        let (identity, route, os_user_opt_in) = match &conn {
            None => (ExecIdentity::Primary, ExecRoute::Http, None),
            Some(c) => (
                self.identity_for(c, &key, IdentityTarget::Default, wants_service),
                exec::route_for_connection(c, RouteTarget::DefaultOrEnv),
                os_user_opt_in(snap, &key, exec_as_os_user_env().as_deref()),
            ),
        };
        ResolvedServer {
            key,
            is_default: true,
            conn,
            namespace,
            policy,
            effective,
            set_aside,
            role,
            instance,
            gates,
            identity,
            route,
            os_user_opt_in,
            audited,
        }
    }

    /// The identity `conn` runs the agent's code as (R2 section 3), for a call that runs it.
    ///
    /// The service account's connection is `conn` with its credentials, and its HTTP client has a
    /// cookie jar of its own: IRIS reads the session cookie before the `Authorization` header, so
    /// a jar shared with the primary would run the code as the primary.
    fn identity_for(
        &self,
        conn: &Arc<IrisConnection>,
        key: &str,
        target: IdentityTarget,
        wants_service: bool,
    ) -> ExecIdentity {
        if !wants_service {
            return ExecIdentity::Primary;
        }
        let env_account = non_blank_env("IRIS_SERVICE_USERNAME");
        let withheld_note = conn
            .credentials_withheld
            .as_deref()
            .filter(|n| n.contains("IRIS_SERVICE_USERNAME"));
        let decision = decide_identity(&IdentityInputs {
            env_account: env_account.as_deref(),
            withheld: withheld_note.is_some(),
            target,
            own_account: conn.member.service_username.as_deref(),
        });
        let (user, source) = match decision {
            IdentityDecision::Primary => return ExecIdentity::Primary,
            IdentityDecision::Refused { code } => {
                return ExecIdentity::Refused {
                    code,
                    reason: refusal_reason(code, key, withheld_note),
                };
            }
            IdentityDecision::Service { user, source } => (user, source),
        };
        if let Err(e) = exec::validate_service_username(&user) {
            return ExecIdentity::Refused {
                code: exec::ERR_SERVICE_ACCOUNT_INVALID,
                reason: e,
            };
        }
        let password = match source {
            IdentitySource::ServiceAccountEnv => {
                std::env::var("IRIS_SERVICE_PASSWORD").unwrap_or_default()
            }
            _ => conn.member.service_password.clone().unwrap_or_default(),
        };
        let mut svc = (**conn).clone();
        svc.username = user.clone();
        svc.password = password;
        svc.member.expect_user = Some(user.clone());
        let client = match self.identity_client(&conn.base_url, &user, &conn.tls) {
            Ok(c) => c,
            Err(e) => {
                return ExecIdentity::Refused {
                    code: "SERVICE_ACCOUNT_CLIENT_FAILED",
                    reason: format!("could not build the service account's HTTP client: {e}"),
                }
            }
        };
        ExecIdentity::Service {
            conn: Arc::new(svc),
            client,
            user,
            source,
        }
    }

    /// The HTTP client of one service account on one server, built once. Its cookie jar is that
    /// identity's own and is registered for logout.
    fn identity_client(
        &self,
        base_url: &str,
        user: &str,
        tls: &crate::iris::http::TlsSetting,
    ) -> anyhow::Result<Arc<reqwest::Client>> {
        let key = format!(
            "{}|{}",
            base_url.trim_end_matches('/'),
            user.to_ascii_lowercase()
        );
        let mut clients = self.identity_clients.lock().unwrap();
        if let Some(c) = clients.get(&key) {
            return Ok(c.clone());
        }
        let client = Arc::new(IrisConnection::http_client_for_identity(&key, tls)?);
        clients.insert(key, client.clone());
        Ok(client)
    }

    /// A pool member. Its policy is its own `[policy.<name>]` (the strict default when absent), its
    /// write gates are its own (R1 D6), and its role is its `[instance.<name>]`'s.
    async fn resolve_member(
        &self,
        snap: &Arc<ConfigSnapshot>,
        name: &str,
        ns_arg: Option<&str>,
        class: WriteClass,
        categories: &[ToolCategory],
        wants_service: bool,
    ) -> Result<ResolvedServer, Early> {
        let pool = self.pool_ref();
        let conn = pool.get(Some(name))?;
        let namespace = resolve_namespace(ns_arg, &conn.namespace).to_string();
        // The default connection under another name: its system mode is already known, and it is
        // held to the stricter of the two policies and gates below.
        let default_conn = self.connection.lock().unwrap().iris.clone();
        let alias_of_default = default_conn
            .as_ref()
            .filter(|d| same_instance(&d.base_url, &conn.base_url))
            .cloned();

        let declared = snap.effective_policy(name);
        let decl = MemberDeclaration {
            top_level: snap.declared_gates(),
            write_tools: declared.write_tools.value,
            destructive_tools: declared.destructive_tools.value,
        };
        // The member is asked for its system mode only when the answer decides something: its write
        // gate (a write nobody declared), or its template (a call the template can block, with no
        // template declared). Reads never probe.
        let needs_gate_mode =
            class != WriteClass::ReadOnly && member_needs_mode(operator_env_gates(), &decl);
        let needs_template_mode =
            declared.mcp_template.value.is_none() && template_matters(categories);
        let mode = if let Some(d) = &alias_of_default {
            d.system_mode
        } else if needs_gate_mode || needs_template_mode {
            pool.system_mode(name).await
        } else {
            pool.cached_system_mode(name).unwrap_or_default()
        };

        let implied = implied_template(mode, false, &namespace).0;
        let effective =
            snap.effective_policy_for(name, (mode != SystemMode::Unknown).then(|| implied.clone()));
        let set_aside = snap.set_aside_for(name, &effective);
        let mut policy = effective.to_connection_policy();
        if policy.mcp_template.is_none() {
            policy.mcp_template = Some(implied);
        }
        let mut gates = write_gate::resolve_member(operator_env_gates(), &decl, mode, &namespace);

        let (mut role, instance) = if snap.fleet.mode.as_deref() == Some("operate")
            && snap.fleet.instance.contains_key(name)
        {
            (effective.role.value, name.to_string())
        } else {
            resolve_role(snap, Some(&conn), &namespace)
        };

        // `_env`, or a Server Manager profile with the default's address, is held to the stricter
        // of the two, or `server = "_env"` would be a way around `[policy.default]` and the
        // default's write gate.
        if alias_of_default.is_some() {
            let default = self.resolve_default(snap, ns_arg, false);
            policy = policy.stricter(&default.policy);
            gates = alias_gates(default.gates, &decl);
            if default.role == ConnectionRole::Subject {
                role = ConnectionRole::Subject;
            }
        }
        let audited =
            snap.has_policy_section(name) || policy.mcp_template != Some(McpTemplate::Dev);

        let id_target = if name == "_env" {
            IdentityTarget::EnvMember
        } else if alias_of_default.is_some() {
            IdentityTarget::AliasOfDefault
        } else {
            IdentityTarget::Member
        };
        let identity = self.identity_for(&conn, name, id_target, wants_service);
        let route = exec::route_for_connection(
            &conn,
            if matches!(id_target, IdentityTarget::Member) {
                RouteTarget::Member
            } else {
                RouteTarget::DefaultOrEnv
            },
        );
        let opt_in = os_user_opt_in(snap, name, exec_as_os_user_env().as_deref());

        Ok(ResolvedServer {
            key: name.to_string(),
            is_default: false,
            conn: Some(conn),
            namespace,
            policy,
            effective,
            set_aside,
            role,
            instance,
            gates,
            identity,
            route,
            os_user_opt_in: opt_in,
            audited,
        })
    }
}

//! Write and destructive gate resolution, classification, and enforcement.
//!
//! The gate used to travel through a process-global environment variable
//! (`IRIS_WRITE_TOOLS_ENABLED`), exported from the config loader only when the variable was
//! not already set. That made it write-once per process: an operator who edited
//! `write_tools_enabled` from `true` to `false` kept seeing `true` forever, and two readers
//! interpreted an absent variable with opposite defaults. Enforcement was a four-line preamble
//! copy-pasted into six handlers, so every write-capable tool added since was ungated by
//! omission.
//!
//! This module replaces both halves of that with data:
//!
//! - [`resolve_gates`] is a pure function. No `std::env` reads, no IO, no clock. Precedence is
//!   the [`GateSource`] variant order, and the operator's environment arrives as an
//!   [`OperatorEnvGates`] parameter, which is what makes the "operator already set it" branch
//!   reachable from a test.
//! - [`CLASSIFICATION`] is a const table naming every registered tool and its [`WriteClass`],
//!   with per-action overrides for tools that both read and write. Completeness is asserted by
//!   test against `IrisTools::registered_tool_names()`, so a new tool cannot ship unclassified.
//!
//! Enforcement happens once, in `ServerHandler::call_tool`, before anything touches IRIS.
//!
//! Spec: `specs/085-write-gate-integrity/`.

use crate::iris::connection::SystemMode;
use crate::iris::workspace_config::WorkspaceConfig;
use rmcp::{model::CallToolResult, ErrorData as McpError};
use std::sync::OnceLock;

// ── Error codes ──────────────────────────────────────────────────────────────

/// A write-capable call was refused because the write gate is off.
///
/// Same string as `admin_tools::ERR_WRITE_GATE`, which the six deleted in-handler guards used.
/// Kept identical so the reporter's published probes keep matching (Principle V).
pub const ERR_WRITE_GATE: &str = "WRITE_TOOLS_DISABLED";

/// A destructive-tier call was refused: writes are on, the tier is off.
///
/// Documented since v1.0.0 (`docs/tools.md`) and, until this feature, never present in source.
pub const ERR_DESTRUCTIVE_GATE: &str = "DESTRUCTIVE_TOOLS_DISABLED";

/// `destructive_tools_enabled = true` declared with `write_tools_enabled = false`.
///
/// Previously logged while the server started with writes *enabled*. Now a startup rejection.
pub const ERR_DESTRUCTIVE_REQUIRES_WRITES: &str = "DESTRUCTIVE_REQUIRES_WRITES";
/// The refusal code of `read_only = "strict"`, for a scratch-class tool and for a closed gate.
pub const ERR_READ_ONLY_STRICT: &str = "READ_ONLY_STRICT";
pub const ERR_READ_ONLY_STRICT_WITH_AUDIT: &str = "READ_ONLY_STRICT_WITH_AUDIT";

// ── 1. GateSource ────────────────────────────────────────────────────────────

/// Which input decided a gate's value.
///
/// Variant order **is** the precedence order (FR-003). Reported by `check_config` as
/// `write_tools_source` / `destructive_tools_source` so a future mismatch is one field lookup
/// rather than a four-round issue.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize, schemars::JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum GateSource {
    /// `read_only = "strict"` (any file) or `IAD_READ_ONLY=strict`: both gates are closed on every
    /// server, whatever else would have opened them (spec 141 T203, Q18).
    ReadOnlyStrict,
    /// Operator exported the gate env var before the process began.
    OperatorEnv,
    /// Declared in a workspace file the operator trusted, or in the tightening half of one.
    ConfigFile,
    /// Declared in the operator file, `~/.iris-agentic-dev.toml`.
    OperatorFile,
    /// `IRIS_ALLOW_PROD` set — the issue #26 override.
    LegacyAllowProd,
    /// Nothing declared; IRIS `SystemMode` decided.
    InferredSystemMode,
    /// Nothing declared and `SystemMode` unknown; the namespace decided.
    InferredNamespace,
    /// Nothing declared and nothing infers this gate; the documented default applied.
    ///
    /// The destructive tier is never inferred from `SystemMode` or the namespace — it is off
    /// until declared. Reporting that as [`GateSource::FailClosed`] would tell an operator
    /// something failed when nothing did.
    InferredDefault,
    /// Resolution could not be trusted, so the gate was forced off (FR-005).
    ///
    /// An unparseable config, or the [`GateResolution`] invariant clamp.
    FailClosed,
    /// A pool member's `[policy.<name>] writeTools` / `destructiveTools` (spec 141, R1 D6).
    PolicyDeclaration,
    /// A pool member with no declaration whose `SystemMode` could not be read: write-closed until
    /// the operator names the member (spec 141, R1 D6, R5 C10).
    UnprobedMember,
}

impl GateSource {
    /// The `snake_case` wire value, without going through serde.
    pub fn as_str(&self) -> &'static str {
        match self {
            GateSource::ReadOnlyStrict => "read_only_strict",
            GateSource::OperatorEnv => "operator_env",
            GateSource::ConfigFile => "config_file",
            GateSource::OperatorFile => "operator_file",
            GateSource::LegacyAllowProd => "legacy_allow_prod",
            GateSource::InferredSystemMode => "inferred_system_mode",
            GateSource::InferredNamespace => "inferred_namespace",
            GateSource::InferredDefault => "inferred_default",
            GateSource::FailClosed => "fail_closed",
            GateSource::PolicyDeclaration => "policy_declaration",
            GateSource::UnprobedMember => "unprobed_member",
        }
    }
}

// ── 2. GateResolution ────────────────────────────────────────────────────────

/// The resolved gate answer for one connection context.
///
/// Replaced wholesale on config reload rather than mutated, which is what makes a config edit
/// take effect in *both* directions (FR-002).
///
/// **Invariant (FR-018)**: `destructive_enabled == true` implies `write_enabled == true`.
/// [`resolve_gates`] enforces it rather than trusting callers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
pub struct GateResolution {
    pub write_enabled: bool,
    pub write_source: GateSource,
    pub destructive_enabled: bool,
    pub destructive_source: GateSource,
}

impl GateResolution {
    /// Both gates closed by `read_only = "strict"`: the answer on every server, whatever a file,
    /// the operator's environment or the legacy override declared (spec 141 T203, Q18).
    pub fn read_only_strict() -> Self {
        Self {
            write_enabled: false,
            write_source: GateSource::ReadOnlyStrict,
            destructive_enabled: false,
            destructive_source: GateSource::ReadOnlyStrict,
        }
    }

    /// Everything off, attributed to fail-closed. Used when no resolution is available at all.
    pub fn fail_closed() -> Self {
        Self {
            write_enabled: false,
            write_source: GateSource::FailClosed,
            destructive_enabled: false,
            destructive_source: GateSource::FailClosed,
        }
    }
}

/// A snapshot of what the *operator* set in the environment, as distinct from what the process
/// set later while loading a config.
///
/// Conflating those two is the #110 defect: the old code exported the config value into
/// `IRIS_WRITE_TOOLS_ENABLED` only when the variable was absent, so it could never tell "the
/// operator asked for this" from "we wrote this on the first config load".
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct OperatorEnvGates {
    pub write_tools_enabled: Option<bool>,
    pub destructive_enabled: Option<bool>,
    pub allow_prod: bool,
}

impl OperatorEnvGates {
    /// `"1"` or case-insensitive `"true"` → `true`; any other present value → `false`;
    /// absent → `None`.
    fn parse(value: Result<String, std::env::VarError>) -> Option<bool> {
        value
            .ok()
            .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
    }

    /// Read the three variables out of the process environment.
    ///
    /// Called once, through [`operator_env_gates`]. Not public: a second read after the process
    /// has begun could see a variable the process itself set, which is the bug.
    fn from_env() -> Self {
        Self {
            write_tools_enabled: Self::parse(std::env::var("IRIS_WRITE_TOOLS_ENABLED")),
            destructive_enabled: Self::parse(std::env::var("IRIS_DESTRUCTIVE_TOOLS_ENABLED")),
            allow_prod: Self::parse(std::env::var("IRIS_ALLOW_PROD")).unwrap_or(false),
        }
    }
}

static OPERATOR_ENV_GATES: OnceLock<OperatorEnvGates> = OnceLock::new();

/// The process-start snapshot of the operator's environment.
///
/// Captured on first call. Every later call returns the same snapshot, so a variable the process
/// exports afterwards cannot be mistaken for an operator declaration.
pub fn operator_env_gates() -> &'static OperatorEnvGates {
    OPERATOR_ENV_GATES.get_or_init(OperatorEnvGates::from_env)
}

/// Seed the snapshot explicitly, for tests and for the binary's startup capture.
///
/// Returns `false` if it was already captured. Tests that need a specific operator environment
/// should pass an [`OperatorEnvGates`] straight to [`resolve_gates`] instead — that is the whole
/// point of the parameter, and it needs no process-wide state.
pub fn init_operator_env_gates(gates: OperatorEnvGates) -> bool {
    OPERATOR_ENV_GATES.set(gates).is_ok()
}

/// What a `.iris-agentic-dev.toml` declares about the two gates.
///
/// Lifted out of [`WorkspaceConfig`] so the declaration can travel with a `ConnectionState` after
/// the config value itself is out of scope. The gate has to be re-resolved whenever the namespace
/// or `SystemMode` changes — `iris_select_container` is the case that matters — and re-resolving
/// without the declaration would silently drop back to inference. That is what the env var used to
/// paper over.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct DeclaredGates {
    pub write_tools_enabled: Option<bool>,
    pub destructive_tools_enabled: Option<bool>,
    /// Which file decided each gate; `None` reads as [`GateSource::ConfigFile`].
    pub write_source: Option<GateSource>,
    pub destructive_source: Option<GateSource>,
    /// The default server's address came from a workspace file the operator has not trusted, so a
    /// looser gate in launch settings does not apply to it (R4a D5, binding rule 2).
    pub withhold_launch_loosening: bool,
    /// `read_only = "strict"` in either file, or `IAD_READ_ONLY=strict` in launch settings. It
    /// only tightens, so any file may set it. Resolution closes both gates on every server before
    /// it looks at anything else.
    pub read_only_strict: bool,
}

impl DeclaredGates {
    pub fn from_config(cfg: &WorkspaceConfig) -> Self {
        Self {
            write_tools_enabled: cfg.write_tools_enabled,
            destructive_tools_enabled: cfg.destructive_tools_enabled,
            ..Default::default()
        }
    }

    /// `None` config → nothing declared. An absent key is not a declaration (FR-001).
    pub fn from_config_opt(cfg: Option<&WorkspaceConfig>) -> Self {
        cfg.map(Self::from_config).unwrap_or_default()
    }
}

/// Resolve both gates from a parsed config. Pure: no `std::env` reads, no IO, no clock.
///
/// Precedence follows the [`GateSource`] variant order. The `SystemMode` / namespace chain at the
/// end is the issue #26 behavior moved here unchanged (FR-019) — this feature changes where that
/// decision lives and whether it is reported, not what it decides.
pub fn resolve_gates(
    operator: &OperatorEnvGates,
    cfg: Option<&WorkspaceConfig>,
    system_mode: SystemMode,
    namespace: &str,
) -> GateResolution {
    resolve_declared(
        operator,
        DeclaredGates::from_config_opt(cfg),
        system_mode,
        namespace,
    )
}

/// [`resolve_gates`] against a declaration that has outlived its config file. Same function; the
/// config-taking form exists so tests can start from a TOML string (FR-022).
pub fn resolve_declared(
    operator: &OperatorEnvGates,
    declared: DeclaredGates,
    system_mode: SystemMode,
    namespace: &str,
) -> GateResolution {
    if declared.read_only_strict {
        return GateResolution::read_only_strict();
    }
    // A looser launch setting does not follow a server a workspace file chose.
    let launch_write = operator
        .write_tools_enabled
        .filter(|w| !(*w && declared.withhold_launch_loosening));
    let launch_destructive = operator
        .destructive_enabled
        .filter(|d| !(*d && declared.withhold_launch_loosening));
    let (write_enabled, write_source) = if let Some(w) = launch_write {
        (w, GateSource::OperatorEnv)
    } else if let Some(w) = declared.write_tools_enabled {
        (w, declared.write_source.unwrap_or(GateSource::ConfigFile))
    } else if operator.allow_prod {
        (true, GateSource::LegacyAllowProd)
    } else {
        match system_mode {
            SystemMode::Live => (false, GateSource::InferredSystemMode),
            SystemMode::Development | SystemMode::Test => (true, GateSource::InferredSystemMode),
            SystemMode::Unknown | SystemMode::Unset => (
                !is_production_namespace(namespace),
                GateSource::InferredNamespace,
            ),
        }
    };

    let (declared_destructive, declared_source) = if let Some(d) = launch_destructive {
        (d, GateSource::OperatorEnv)
    } else if let Some(d) = declared.destructive_tools_enabled {
        (
            d,
            declared
                .destructive_source
                .unwrap_or(GateSource::ConfigFile),
        )
    } else {
        // Never inferred from SystemMode or namespace — off until declared (spec 073).
        (false, GateSource::InferredDefault)
    };

    // The invariant, enforced here rather than trusted of callers: the destructive tier is a
    // subset of the write gate, so writes off closes it regardless of what was declared. The
    // contradictory *declaration* is rejected earlier by validate_gate_config; this is the belt
    // to that suspenders, and it is what makes US7 scenario 3 hold.
    let (destructive_enabled, destructive_source) = if declared_destructive && !write_enabled {
        (false, GateSource::FailClosed)
    } else {
        (declared_destructive, declared_source)
    };

    GateResolution {
        write_enabled,
        write_source,
        destructive_enabled,
        destructive_source,
    }
}

/// What decides a pool member's gates besides the operator's environment (spec 141, R1 D6).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct MemberDeclaration {
    /// The default connection's top-level `write_tools_enabled` / `destructive_tools_enabled`, as
    /// the files declare them. Only a `false` reaches a member: a key that names no server cannot
    /// raise one.
    pub top_level: DeclaredGates,
    /// `[policy.<name>] writeTools`, after the trust filter.
    pub write_tools: Option<bool>,
    /// `[policy.<name>] destructiveTools`, after the trust filter.
    pub destructive_tools: Option<bool>,
}

/// Resolve the gates of a pool member (spec 141, R1 D6). Pure.
///
/// `system_mode` is what a probe of the member answered, or [`SystemMode::Unknown`] when it was
/// not probed or the probe failed. Precedence, first match wins:
///
/// 1. the operator's `IRIS_WRITE_TOOLS_ENABLED=0` closes every member; `=1` and `IRIS_ALLOW_PROD`
///    raise only the default connection;
/// 2. a top-level `write_tools_enabled = false` closes every member; `= true` raises none;
/// 3. `[policy.<name>] writeTools`;
/// 4. the member's `SystemMode`: Live closes, Development and Test open;
/// 5. a mode the probe could not read (`Unknown`) closes (`UnprobedMember`), whatever the namespace
///    says;
/// 6. an instance whose mode was read and is empty (`Unset`, a fresh install) is judged by the
///    namespace the call acts in, as the default connection is: a production-looking name closes.
///
/// The destructive tier is never inferred and is always a subset of the write gate (FR-018).
pub fn resolve_member(
    operator: &OperatorEnvGates,
    decl: &MemberDeclaration,
    system_mode: SystemMode,
    namespace: &str,
) -> GateResolution {
    if decl.top_level.read_only_strict {
        return GateResolution::read_only_strict();
    }
    let (write_enabled, write_source) = if operator.write_tools_enabled == Some(false) {
        (false, GateSource::OperatorEnv)
    } else if decl.top_level.write_tools_enabled == Some(false) {
        (
            false,
            decl.top_level
                .write_source
                .unwrap_or(GateSource::ConfigFile),
        )
    } else if let Some(w) = decl.write_tools {
        (w, GateSource::PolicyDeclaration)
    } else {
        match system_mode {
            SystemMode::Live => (false, GateSource::InferredSystemMode),
            SystemMode::Development | SystemMode::Test => (true, GateSource::InferredSystemMode),
            SystemMode::Unset => (
                !is_production_namespace(namespace),
                GateSource::InferredNamespace,
            ),
            SystemMode::Unknown => (false, GateSource::UnprobedMember),
        }
    };

    let (declared_destructive, declared_source) = if operator.destructive_enabled == Some(false) {
        (false, GateSource::OperatorEnv)
    } else if decl.top_level.destructive_tools_enabled == Some(false) {
        (
            false,
            decl.top_level
                .destructive_source
                .unwrap_or(GateSource::ConfigFile),
        )
    } else if let Some(d) = decl.destructive_tools {
        (d, GateSource::PolicyDeclaration)
    } else {
        (false, GateSource::InferredDefault)
    };
    let (destructive_enabled, destructive_source) = if declared_destructive && !write_enabled {
        (false, GateSource::FailClosed)
    } else {
        (declared_destructive, declared_source)
    };
    GateResolution {
        write_enabled,
        write_source,
        destructive_enabled,
        destructive_source,
    }
}

/// Whether [`resolve_member`] needs to know the member's `SystemMode`: nothing before the
/// inference step decided the write gate.
pub fn member_needs_mode(operator: &OperatorEnvGates, decl: &MemberDeclaration) -> bool {
    !decl.top_level.read_only_strict
        && operator.write_tools_enabled != Some(false)
        && decl.top_level.write_tools_enabled != Some(false)
        && decl.write_tools.is_none()
}

/// Resolve for a connection that may not exist.
///
/// FR-012: the answer must not depend on connectivity. A live connection supplies the two
/// inference inputs (`SystemMode`, namespace); without one, resolution proceeds from the same
/// declaration against `SystemMode::Unknown` and `namespace`. The old disconnected path instead
/// re-read the environment with an `unwrap_or(true)` default, so an unreachable server answered
/// permissively no matter what the config said.
pub fn resolve_for_connection(
    declared: DeclaredGates,
    iris: Option<&crate::iris::connection::IrisConnection>,
    namespace: &str,
) -> GateResolution {
    let (system_mode, namespace) = match iris {
        Some(c) => (c.system_mode, c.namespace.as_str()),
        None => (SystemMode::Unknown, namespace),
    };
    resolve_declared(operator_env_gates(), declared, system_mode, namespace)
}

/// Namespaces treated as production when `SystemMode` is unknown.
///
/// Moved from `iris/connection.rs` so the whole inference chain has one implementation.
pub fn is_production_namespace(ns: &str) -> bool {
    matches!(
        ns.to_uppercase().as_str(),
        "PROD" | "PRODUCTION" | "LIVE" | "PRD"
    )
}

// ── Startup validation ───────────────────────────────────────────────────────

/// Why a gate configuration is unusable.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum GateConfigError {
    /// `destructive_tools_enabled = true` with `write_tools_enabled = false`.
    #[error("DESTRUCTIVE_REQUIRES_WRITES: destructive_tools_enabled = true requires write_tools_enabled = true — the destructive tier is a subset of the write gate and can never take effect with writes off")]
    DestructiveRequiresWrites,
    /// `read_only = "strict"` (or `IAD_READ_ONLY=strict`) with `irisAudit = true` on any server.
    #[error("READ_ONLY_STRICT_WITH_AUDIT: read_only = \"strict\" (or IAD_READ_ONLY=strict) cannot be combined with irisAudit = true — the audit emission compiles a class on the server, and strict read-only means iad writes nothing to it. Remove one of the two settings")]
    ReadOnlyStrictWithAudit,
}

impl GateConfigError {
    pub fn code(&self) -> &'static str {
        match self {
            GateConfigError::DestructiveRequiresWrites => ERR_DESTRUCTIVE_REQUIRES_WRITES,
            GateConfigError::ReadOnlyStrictWithAudit => ERR_READ_ONLY_STRICT_WITH_AUDIT,
        }
    }
}

/// Reject a gate configuration that can never do what it says. Pure.
///
/// The old code logged this and returned `None` from `workspace_config_to_connection`, which
/// skipped the env export below it and dropped the caller into the permissive namespace
/// inference — so the configuration documented as "refused to start" started with writes
/// *enabled*. The caller now exits 2 (FR-005, FR-006).
pub fn validate_gate_config(cfg: &WorkspaceConfig) -> Result<(), GateConfigError> {
    validate_declared_gates(&DeclaredGates::from_config(cfg))
}

/// The same check against the declaration alone.
///
/// `mcp.rs` gets a [`DeclaredGates`] back from the config loader and never holds the
/// `WorkspaceConfig` it came from, so the startup check has to be reachable from the declaration.
/// One implementation, two entry points — a second copy of the condition is how the reload path
/// and the startup path would come to disagree.
pub fn validate_declared_gates(declared: &DeclaredGates) -> Result<(), GateConfigError> {
    if declared.destructive_tools_enabled == Some(true)
        && declared.write_tools_enabled == Some(false)
    {
        return Err(GateConfigError::DestructiveRequiresWrites);
    }
    Ok(())
}

// ── 3. Tool classification ───────────────────────────────────────────────────

/// What gate a call requires.
///
/// `Destructive` is a *subset* of `Write`, not a sibling: a destructive tool with writes off is
/// refused with [`ERR_WRITE_GATE`], not [`ERR_DESTRUCTIVE_GATE`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WriteClass {
    ReadOnly,
    Write,
    Destructive,
}

/// Where a call's target server comes from (spec 141, R1 D3).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TargetSource {
    /// The named argument; absent or blank means the default connection.
    Arg(&'static str),
    /// Two named arguments, both gated (`compare_*`); the first is the primary.
    Args(&'static [&'static str]),
    /// The argument names a pool member. A name that is not a member is an ad-hoc host and the call
    /// has no IRIS policy to judge (`iris_test_server`).
    Name(&'static str),
    /// The server recorded in the `WsSessionPool` entry the `session` argument names.
    Session,
    /// The named argument, except that a call with an `elicitation_id` is judged on the server, tool
    /// and class its pending entry recorded.
    ArgOrPending(&'static str),
    /// The default connection; the tool has no `server` parameter.
    Default,
}

/// Whether a tool's policy gates apply (spec 141, R1 D7).
///
/// `Local` tools do not act on an IRIS server's data or code (the server registry, the skill and
/// knowledge tools, the docs-site search), so the template, `allow`, `dataPolicy` and role gates
/// have nothing to judge. The write/destructive gate still applies to them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TargetKind {
    Iris,
    Local,
}

/// One row of [`CLASSIFICATION`].
pub struct ToolClass {
    /// Registered tool name, exactly as `tools/list` advertises it.
    pub tool: &'static str,
    /// Per-action overrides, matched case-insensitively against the call's `action` or `mode`
    /// argument. Empty for tools with a single class.
    pub actions: &'static [(&'static str, WriteClass)],
    /// Applies when `actions` is empty or no action matches.
    ///
    /// For a dispatcher whose write actions are the majority, this is the write tier and the
    /// read actions are the overrides, so an unrecognised action is gated. `iris_doc` and
    /// `iris_query` are the two documented exceptions: their read path is the default call and
    /// their write actions are an explicit, closed set (data-model.md §3).
    pub default: WriteClass,
    /// Where the target server comes from. Every row names one; the completeness test checks the
    /// column against the advertised schema.
    pub target: TargetSource,
    pub kind: TargetKind,
    /// The tool can reach `execute_via_generator`, which PUTs, compiles and deletes an
    /// `IrisDevTmp.*` class on the target (spec 141, R5 C14). That is how iad reads over HTTP,
    /// since Atelier has no execute endpoint, so a tool that only reads can still write a class to
    /// the server. `read_only = "strict"` refuses and hides these tools. `readOnlyHint` is not
    /// derived from it: the scratch class is removed after the call and changes nothing the user
    /// can observe.
    pub scratch: bool,
}

impl ToolClass {
    /// The tool can compile a scratch class on the target.
    const fn scratch(mut self) -> Self {
        self.scratch = true;
        self
    }

    /// The tool has no `server` parameter and acts on the default connection.
    const fn on_default(mut self) -> Self {
        self.target = TargetSource::Default;
        self
    }

    /// The tool does not act on an IRIS server's data or code.
    const fn local(mut self) -> Self {
        self.kind = TargetKind::Local;
        self
    }

    /// A local tool with no `server` parameter at all.
    const fn local_default(self) -> Self {
        self.on_default().local()
    }

    const fn target(mut self, target: TargetSource) -> Self {
        self.target = target;
        self
    }
}

const fn row(
    tool: &'static str,
    actions: &'static [(&'static str, WriteClass)],
    default: WriteClass,
) -> ToolClass {
    ToolClass {
        tool,
        actions,
        default,
        target: TargetSource::Arg("server"),
        kind: TargetKind::Iris,
        scratch: false,
    }
}

const fn ro(tool: &'static str) -> ToolClass {
    row(tool, &[], WriteClass::ReadOnly)
}

const fn wr(tool: &'static str) -> ToolClass {
    row(tool, &[], WriteClass::Write)
}

const fn de(tool: &'static str) -> ToolClass {
    row(tool, &[], WriteClass::Destructive)
}

const fn mixed(
    tool: &'static str,
    actions: &'static [(&'static str, WriteClass)],
    default: WriteClass,
) -> ToolClass {
    row(tool, actions, default)
}

/// Every registered tool and the gate it requires (FR-007).
///
/// This is the single source of truth for enforcement *and* for the completeness test, which
/// compares it against `IrisTools::registered_tool_names()` in both directions. A tool added
/// without an entry fails CI; an entry whose tool was renamed fails CI too, because a stale
/// entry silently stops matching and that is how a gate quietly disappears.
///
/// The write set covers every tool the reporter verified ungated against the released 1.2.6
/// binary — `iris_ws_exec`, `iris_global` set/kill, `iris_lookup_manage` set/delete,
/// `iris_execute_method` — and every tool that was ungated by code reading (FR-013).
pub const CLASSIFICATION: &[ToolClass] = &[
    // ── read-only ────────────────────────────────────────────────────────────
    ro("agent_history").local_default(),
    ro("agent_info").local_default(),
    ro("agent_stats").local_default(),
    ro("capability_matrix").scratch(),
    ro("check_config").local_default(),
    ro("compare_document").target(TargetSource::Args(&["server_a", "server_b"])),
    ro("compare_namespace").target(TargetSource::Args(&["server_a", "server_b"])),
    ro("debug_capture_packet").on_default(),
    ro("debug_get_error_logs").on_default(),
    ro("debug_map_int_to_cls").on_default().scratch(),
    ro("debug_source_map").on_default().scratch(),
    ro("docs_introspect").scratch(),
    ro("extract_message_map_routing").scratch(),
    ro("find_subclass_implementations").scratch(),
    ro("global_preview").scratch(),
    ro("hl7_schema_inspect").scratch(),
    ro("hl7_schema_list").scratch(),
    ro("iris_business_rule_info").scratch(),
    ro("iris_credential_list").on_default(),
    ro("iris_database_list").scratch(),
    ro("iris_database_stats").scratch(),
    ro("iris_debug").scratch(),
    ro("iris_doc_search").local_default(),
    ro("iris_get_log"),
    ro("iris_info"),
    ro("iris_interop_query"),
    ro("iris_list_containers").local_default(),
    ro("iris_macro"),
    ro("iris_message_body").scratch(),
    ro("iris_mirror_status").scratch(),
    ro("iris_namespace_list").scratch(),
    ro("iris_production_diff").scratch(),
    ro("iris_search"),
    ro("iris_servers").local_default(),
    ro("iris_symbols"),
    ro("iris_symbols_local").local(),
    ro("iris_check"),
    mixed(
        "iris_system_performance",
        &[
            ("start", WriteClass::Write),
            ("status", WriteClass::ReadOnly),
            ("last_runid", WriteClass::ReadOnly),
            ("list_profiles", WriteClass::ReadOnly),
            // Both writers touch ^IRIS.SystemPerformance("profile") and nothing else, and a
            // deleted profile is one add_profile away from being back — recoverable, so Write
            // rather than Destructive.
            ("add_profile", WriteClass::Write),
            ("delete_profile", WriteClass::Write),
            ("list_runs", WriteClass::ReadOnly),
            ("report", WriteClass::ReadOnly),
        ],
        WriteClass::ReadOnly,
    )
    .scratch(),
    ro("iris_table_info").scratch(),
    ro("iris_test_server").target(TargetSource::Name("name")),
    // Opening and closing a terminal session mutates nothing. Everything a session can do
    // goes through iris_ws_exec, which is where the gate belongs.
    ro("iris_ws_close").target(TargetSource::Session),
    ro("iris_ws_open"),
    ro("journal_search").scratch(),
    ro("kb_recall").local_default(),
    ro("mermaid_class"),
    ro("mermaid_production"),
    ro("my_access").scratch(),
    ro("query_audit_log"),
    ro("resolve_dynamic_dispatch").scratch(),
    ro("resolve_storage"),
    ro("skill_community_list").local_default(),
    ro("skill_describe").local_default(),
    ro("skill_list").local_default(),
    ro("skill_search").local_default(),
    ro("stream_inspect").scratch(),
    ro("telemetry_export_trace").on_default(),
    ro("telemetry_query").on_default(),
    // ── write ────────────────────────────────────────────────────────────────
    // Scratch: the superclass walk before a compile runs ObjectScript through a scratch class
    // (FR-015a).
    wr("iris_compile").scratch(),
    wr("iris_execute").scratch(),
    // Verified ungated against 1.2.6 with the gate provably active.
    wr("iris_execute_method").scratch(),
    wr("iris_generate"),
    // Stores the generated class and compiles it, superclass walk included.
    wr("iris_generate_class").scratch(),
    wr("iris_generate_test"),
    // Local server registry, same class of state as iris_remove_server.
    wr("iris_add_server").local_default(),
    wr("iris_import_servers").local_default(),
    // Swaps the live pool for one built from the files on disk, which activates every server
    // `iris_add_server`/`iris_import_servers` or an edited file declared since startup. The
    // registry writers are writes; the call that puts their output to use is one too (spec 141
    // T219).
    wr("iris_reload_pool").local_default(),
    wr("iris_production_item").scratch(),
    wr("iris_select_container").local_default(),
    wr("iris_start_sandbox").local_default(),
    // Executes test classes in IRIS, so it runs arbitrary application code.
    wr("iris_test").scratch(),
    // The complete bypass of the iris_execute gate: open a session, then run anything.
    wr("iris_ws_exec").target(TargetSource::Session),
    wr("kb_index").local_default(),
    wr("skill_community_install").local_default(),
    wr("skill_optimize").local_default(),
    wr("skill_propose").local_default(),
    wr("skill_share").local_default(),
    // ── destructive tier (spec 073, ☠ in docs/tools.md) ──────────────────────
    de("global_kill").scratch(),
    de("iris_namespace_create").scratch(),
    // Local state, not IRIS — the enforcement test asserts the saved server survives.
    de("iris_remove_server").local_default(),
    de("skill_forget").local_default(),
    // ── per-action ───────────────────────────────────────────────────────────
    // Mostly listings; the seven create/update/delete actions are the destructive tier.
    mixed(
        "iris_admin",
        &[
            ("check_permission", WriteClass::ReadOnly),
            ("database_status", WriteClass::ReadOnly),
            ("get_webapp", WriteClass::ReadOnly),
            ("journal_search", WriteClass::ReadOnly),
            ("list_databases", WriteClass::ReadOnly),
            ("list_namespaces", WriteClass::ReadOnly),
            ("list_roles", WriteClass::ReadOnly),
            ("list_user_roles", WriteClass::ReadOnly),
            ("list_users", WriteClass::ReadOnly),
            ("list_webapps", WriteClass::ReadOnly),
            ("namespace_mappings", WriteClass::ReadOnly),
            ("view_locks", WriteClass::ReadOnly),
            ("view_processes", WriteClass::ReadOnly),
            // 099: fresh container setup — Write tier (not Destructive)
            ("clear_password_change_flag", WriteClass::Write),
            ("unlock_user", WriteClass::Write),
            ("fresh_container_setup", WriteClass::Write),
            // 097: mirror management — add is Write, failover falls through to Destructive default
            ("mirror_add_async", WriteClass::Write),
        ],
        WriteClass::Destructive,
    )
    .scratch(),
    mixed(
        "iris_containers",
        &[("list", WriteClass::ReadOnly)],
        WriteClass::Write,
    )
    .local_default(),
    // run/start/stop execute test code in IRIS. `check` starts the line monitor to see that it
    // can start and stops it again, so it changes instance state and is a write (spec 141 T219);
    // only `report`, which reads the counters back, is a read.
    mixed(
        "iris_coverage",
        &[("report", WriteClass::ReadOnly)],
        WriteClass::Write,
    )
    .scratch(),
    mixed(
        "iris_credential_manage",
        &[
            ("get", WriteClass::ReadOnly),
            ("list", WriteClass::ReadOnly),
            ("list_tables", WriteClass::ReadOnly),
        ],
        WriteClass::Destructive,
    )
    .on_default()
    .scratch(),
    // The four write modes are a closed set; every other mode is a read (data-model.md §3).
    mixed(
        "iris_doc",
        &[
            ("put", WriteClass::Write),
            ("delete", WriteClass::Write),
            ("insert", WriteClass::Write),
            ("delete_lines", WriteClass::Write),
        ],
        WriteClass::ReadOnly,
    )
    .target(TargetSource::ArgOrPending("server")),
    // kill is the same operation global_kill performs, so it carries the same tier. Classifying
    // it lower would leave the destructive gate reachable through a dispatcher, which is the
    // shape of the defect this feature exists to remove.
    mixed(
        "iris_global",
        &[
            ("get", WriteClass::ReadOnly),
            ("list", WriteClass::ReadOnly),
            ("set", WriteClass::Write),
            ("kill", WriteClass::Destructive),
        ],
        WriteClass::Write,
    )
    .scratch(),
    // export reads a table out to XML; the spec's tier list names the other actions.
    mixed(
        "iris_lookup_manage",
        &[
            ("get", WriteClass::ReadOnly),
            ("list_keys", WriteClass::ReadOnly),
            ("list_tables", WriteClass::ReadOnly),
            ("export", WriteClass::ReadOnly),
        ],
        WriteClass::Destructive,
    )
    .on_default()
    .scratch(),
    mixed(
        "iris_lookup_transfer",
        &[
            ("export", WriteClass::ReadOnly),
            // `%Import` clears a non-empty table first, so an import replaces a whole table: at
            // least as destructive as the single-row delete `iris_lookup_manage` already gates
            // there (spec 141 R5 C18).
            ("import", WriteClass::Destructive),
        ],
        WriteClass::Write,
    )
    .on_default()
    .scratch(),
    mixed(
        "iris_production",
        &[
            ("check", WriteClass::ReadOnly),
            ("status", WriteClass::ReadOnly),
            ("get_autostart", WriteClass::ReadOnly),
        ],
        WriteClass::Write,
    )
    .scratch(),
    // Reads are the default call; only mode="write" mutates (data-model.md §3).
    mixed(
        "iris_query",
        &[("write", WriteClass::Write)],
        WriteClass::ReadOnly,
    ),
    mixed(
        "iris_source_control",
        &[
            ("status", WriteClass::ReadOnly),
            ("menu", WriteClass::ReadOnly),
        ],
        WriteClass::Write,
    )
    .target(TargetSource::ArgOrPending("server"))
    .scratch(),
    mixed(
        "kb",
        &[
            ("recall", WriteClass::ReadOnly),
            ("history", WriteClass::ReadOnly),
            ("stats", WriteClass::ReadOnly),
        ],
        WriteClass::Write,
    )
    .local_default(),
    mixed(
        "skill",
        &[
            ("list", WriteClass::ReadOnly),
            ("describe", WriteClass::ReadOnly),
            ("search", WriteClass::ReadOnly),
            ("forget", WriteClass::Destructive),
        ],
        WriteClass::Write,
    )
    .local_default(),
    mixed(
        "skill_community",
        &[("list", WriteClass::ReadOnly)],
        WriteClass::Write,
    )
    .local_default(),
];

/// Put `mode` and `action` in the form the handlers read them, trimmed and `to_lowercase`d,
/// before any gate looks at them. Every gate then reads the value the handler will act on: the
/// write gate classified `mode: " put"` as read-only and `iris_doc` wrote, and the policy gates
/// compare exactly (security review, 2026-10-22).
///
/// Also trims the server arguments, so the gates and the handler look up the same name, and reads
/// `confirmed` (the spelling `iris_execute` and `iris_ws_exec` took) as `confirm`, so the role
/// gate sees one name.
pub fn normalize_dispatch_args(args: &mut serde_json::Map<String, serde_json::Value>) {
    for key in ["mode", "action"] {
        if let Some(serde_json::Value::String(v)) = args.get_mut(key) {
            *v = v.trim().to_lowercase();
        }
    }
    for key in ["server", "server_a", "server_b"] {
        if let Some(serde_json::Value::String(v)) = args.get_mut(key) {
            let trimmed = v.trim();
            if trimmed.len() != v.len() {
                *v = trimmed.to_string();
            }
        }
    }
    if args.get("confirmed").and_then(|v| v.as_bool()) == Some(true) {
        args.insert("confirm".to_string(), serde_json::Value::Bool(true));
    }
    // The two camelCase parameters have snake_case spellings, like every other parameter on the
    // surface (spec 141 R5 C21). Renamed here so the gates and the handler read one name; the
    // camelCase spelling wins when a caller sends both.
    for (snake, camel) in [
        ("acknowledge_phi", "acknowledgePhi"),
        ("data_policy", "dataPolicy"),
    ] {
        if let Some(v) = args.remove(snake) {
            args.entry(camel.to_string()).or_insert(v);
        }
    }
}

/// Look up the class for a call.
///
/// Returns `None` for a tool with no `CLASSIFICATION` entry. Callers must treat that as
/// "unknown, therefore gated" — but it should be unreachable: the completeness test asserts
/// every registered tool has an entry.
pub fn classify(
    tool: &str,
    args: Option<&serde_json::Map<String, serde_json::Value>>,
) -> Option<WriteClass> {
    let entry = CLASSIFICATION.iter().find(|e| e.tool == tool)?;
    // `force` skips `iris_query`'s read-only SQL check, so a forced read runs what a write runs and
    // is judged as one (spec 141, T-R4a-U22).
    if tool == "iris_query"
        && args
            .and_then(|a| a.get("force"))
            .and_then(|v| v.as_bool())
            .unwrap_or(false)
    {
        return Some(WriteClass::Write);
    }
    // `Diff` shows a document's difference from the repository and changes nothing; every other
    // menu id, known or not, is a write (spec 141 T205).
    if tool == "iris_source_control" && args.is_some_and(is_scm_diff) {
        return Some(WriteClass::ReadOnly);
    }
    if !entry.actions.is_empty() {
        if let Some(args) = args {
            // `action` and `mode` are the two argument names the tools use for this; `iris_doc`
            // accepts `action` as an alias for `mode`. Read the way the handlers read it, trimmed
            // and then `to_lowercase`d: an exact match let mode="PUT" through and then write, a
            // case-only match let " put" through, and `to_lowercase` maps the Kelvin sign to `k`.
            for key in ["action", "mode"] {
                if let Some(v) = args.get(key).and_then(|v| v.as_str()) {
                    let v = v.trim().to_lowercase();
                    if let Some((_, class)) = entry.actions.iter().find(|(a, _)| *a == v) {
                        return Some(*class);
                    }
                }
            }
        }
    }
    Some(entry.default)
}

/// Is this `iris_source_control` call `execute` of the menu id `Diff`?
///
/// The id is matched exactly as the handler hands it to the provider (`%SourceMenu,<id>`): `Diff`
/// or `%Diff`, no trimming and no case folding. A spelling the provider might read as something
/// else stays a write. The action is read the way [`classify`] reads every action.
fn is_scm_diff(args: &serde_json::Map<String, serde_json::Value>) -> bool {
    let executes = ["action", "mode"].iter().any(|k| {
        args.get(*k)
            .and_then(|v| v.as_str())
            .is_some_and(|v| v.trim().to_lowercase() == "execute")
    });
    executes
        && args
            .get("action_id")
            .and_then(|v| v.as_str())
            .is_some_and(|id| matches!(id, "Diff" | "%Diff"))
}

/// Can the tool compile a scratch class on its target? `read_only = "strict"` refuses it and
/// hides it from `tools/list` (spec 141, R5 C15).
pub fn is_scratch(tool: &str) -> bool {
    tool_class(tool).is_some_and(|e| e.scratch)
}

/// The row of [`CLASSIFICATION`] for a tool.
pub fn tool_class(tool: &str) -> Option<&'static ToolClass> {
    CLASSIFICATION.iter().find(|e| e.tool == tool)
}

/// The categories a call has to be permitted in, for the environment template and `allow`
/// (spec 141, R1 D7, R4a). One place where an action changes a tool's category; every gate reads
/// the answer. A template blocks the call when it blocks any of them and `allow` has to list all
/// of them.
///
/// - `iris_global` set/kill, `global_kill` and an `iris_query` write or `force`: `Execute`.
/// - `iris_doc` write modes (with or without `compile`): `Compile`, because writing class source is
///   how code reaches the compiler. Reads stay `Docs`, so `allow = ["docs"]` keeps them.
/// - Any other write or destructive call whose category is `Query`, `Docs`, `Search` or `Debug`:
///   `Admin`, so `allow = ["query"]` no longer lets a write through.
/// - `iris_compile` with `force_writable`: `Compile` and `Admin`, since it runs
///   `%Library.EnsembleMgr.EnableNamespace` as the connection's account.
///
/// `class` is the call's class: the classification, or the pending entry's on an elicitation
/// resume, which carries no `mode`. A tool without a category (`check_config`) has none.
pub fn effective_category(
    tool: &str,
    args: Option<&serde_json::Map<String, serde_json::Value>>,
    class: WriteClass,
) -> Vec<crate::iris::workspace_config::ToolCategory> {
    use crate::iris::workspace_config::ToolCategory;
    let Some(base) = crate::iris::server_manager::tool_to_category_pub(tool) else {
        return Vec::new();
    };
    let writes = class != WriteClass::ReadOnly;
    let flag = |key: &str| {
        args.and_then(|a| a.get(key))
            .and_then(|v| v.as_bool())
            .unwrap_or(false)
    };
    let primary = match tool {
        "global_kill" => ToolCategory::Execute,
        "iris_global" if writes => ToolCategory::Execute,
        "iris_query" if writes || flag("force") => ToolCategory::Execute,
        "iris_doc" if writes => ToolCategory::Compile,
        _ if writes
            && matches!(
                base,
                ToolCategory::Query
                    | ToolCategory::Docs
                    | ToolCategory::Search
                    | ToolCategory::Debug
            ) =>
        {
            ToolCategory::Admin
        }
        _ => base,
    };
    let mut categories = vec![primary];
    if tool == "iris_compile" && flag("force_writable") {
        categories.push(ToolCategory::Admin);
    }
    categories
}

/// The write/destructive gate for one server, called by the pipeline before anything touches IRIS.
///
/// `Some(refusal)` means the call must not proceed. `None` means it may.
///
/// An unclassified tool fails closed as `Write`: a tool that reaches dispatch without an entry
/// is a bug the completeness test should have caught, and the safe reading of a bug is "this
/// might write".
///
/// `gates` is the target server's own resolution (spec 141, R1 D6): the default connection's
/// [`resolve_declared`], or a pool member's [`resolve_member`]. The pipeline calls this once per
/// server a call acts on.
pub fn gate_check(
    tool: &str,
    args: Option<&serde_json::Map<String, serde_json::Value>>,
    gates: &GateResolution,
) -> Option<Result<CallToolResult, McpError>> {
    let class = classify(tool, args).unwrap_or(WriteClass::Write);
    gate_for_class(tool, class, gates)
}

/// [`gate_check`] for a call whose class is already known: the pipeline's, which on an elicitation
/// resume is the pending entry's and not the (mode-less) resume call's own.
pub fn gate_for_class(
    tool: &str,
    class: WriteClass,
    gates: &GateResolution,
) -> Option<Result<CallToolResult, McpError>> {
    match class {
        WriteClass::ReadOnly => None,
        WriteClass::Write | WriteClass::Destructive => {
            // Write is checked first, so a destructive tool with writes off reports the write
            // gate — the more fundamental refusal, and the one whose remedy comes first.
            // The text says what decided the gate, never how an operator would change it: an agent
            // that reads the edit makes it (spec 141 FR-039, 2026-10-24). The remedy goes to stderr
            // and `iris-agentic-dev trust --show`.
            if !gates.write_enabled {
                if gates.write_source == GateSource::ReadOnlyStrict {
                    return Some(strict_refusal(tool, "write_tools"));
                }
                let why = match gates.write_source {
                    GateSource::UnprobedMember => {
                        " This server's SystemMode could not be read, so it starts write-closed \
                         until the operator declares it."
                    }
                    GateSource::PolicyDeclaration => {
                        " A policy declaration for this server turns writes off."
                    }
                    _ => "",
                };
                return Some(refusal(
                    ERR_WRITE_GATE,
                    &format!(
                        "{tool} is write-capable and write tools are disabled (source: {}).{why}",
                        gates.write_source.as_str()
                    ),
                ));
            }
            // The destructive tier is off until declared, and a declaration can only narrow a
            // write gate that is on: it is a subset of it.
            if class == WriteClass::Destructive && !gates.destructive_enabled {
                if gates.destructive_source == GateSource::ReadOnlyStrict {
                    return Some(strict_refusal(tool, "destructive_tools"));
                }
                return Some(refusal(
                    ERR_DESTRUCTIVE_GATE,
                    &format!(
                        "{tool} is a destructive tool and the destructive tier is disabled \
                         (source: {}).",
                        gates.destructive_source.as_str()
                    ),
                ));
            }
            None
        }
    }
}

/// The refusal of a gate that `read_only = "strict"` closed. It carries `READ_ONLY_STRICT`, the
/// code the scratch-class refusal uses, so the agent hears one answer for the one setting, and
/// `gate` says which gate refused: `write_tools` or `destructive_tools`.
fn strict_refusal(tool: &str, gate: &str) -> Result<CallToolResult, McpError> {
    crate::tools::err_result(serde_json::json!({
        "success": false,
        "error_code": ERR_READ_ONLY_STRICT,
        "error": format!(
            "{tool} is refused: read-only mode is strict, so the write and destructive gates are \
             closed on every server, whatever else declares them open. Reads still work."
        ),
        "tool": tool,
        "gate": gate,
        "gate_source": GateSource::ReadOnlyStrict.as_str(),
    }))
}

/// Returns `true` when the code string contains a literal kill-of-a-global expression:
/// `Kill ^`, `KILL ^`, `k ^`, or any case variant of `kill` followed by optional whitespace
/// and a `^` character.
///
/// This is defense-in-depth against inadvertent sloppiness — a well-intentioned model writing
/// `Kill ^Foo` literally without considering the destructive gate. It does NOT catch indirect
/// vectors (`Kill @var`, `Xecute`, `##class` dispatch, `&sql`). The spec says so plainly and
/// the error message repeats it, so callers cannot mistake this check for a comprehensive block.
///
/// False positives (e.g. a kill in a comment line) are acceptable: blocking a comment that
/// looks like a kill is safer than missing a kill that looks like a comment.
pub fn contains_global_kill(code: &str) -> bool {
    // Scan each line for a `kill` keyword (case-insensitive) or its single-letter
    // abbreviation `k`, followed by an optional postconditional and then `^`. Searches within
    // the line so comment lines containing a kill expression are also caught — a false
    // positive beats a false negative.
    for line in code.lines() {
        let lower = line.to_ascii_lowercase();
        let bytes = lower.as_bytes();
        // Check every position where 'k' occurs.
        for (i, _) in lower.match_indices('k') {
            // The keyword has to stand on its own. Without this the trailing `k` of a routine
            // label made `Do check^MyRtn` and `Set x=$$lock^MyLocks` look like `k ^MyRtn` —
            // the same one-meaning-of-caret mistake that made the code-edit gate reject
            // `$$run^SystemPerformance`.
            if i > 0 {
                let prev = bytes[i - 1];
                if prev.is_ascii_alphanumeric() || prev == b'%' || prev == b'_' || prev == b'.' {
                    continue;
                }
            }
            let rest = &lower[i..];
            let after = if let Some(stripped) = rest.strip_prefix("kill") {
                stripped
            } else {
                // Single-letter `k`. `lower` is lowercased, so if `rest` began with "kill" the
                // strip above would have taken it — no "did we land mid-keyword" check is
                // reachable here, and there used to be one that no test could ever enter.
                &rest[1..]
            };
            // A postconditional sits between the keyword and its argument: `Kill:$D(x) ^Foo`.
            // Skipping to the whitespace that ends it keeps those from walking through.
            let after = if after.starts_with(':') {
                match after.find([' ', '\t']) {
                    Some(sp) => &after[sp..],
                    None => continue,
                }
            } else {
                after
            };
            let trimmed = after.trim_start_matches([' ', '\t']);
            if kill_args_name_a_global(trimmed) {
                return true;
            }
        }
    }
    false
}

/// Whether a `Kill` argument list names a global in any position. The list runs to the first
/// unquoted space; arguments are split on unquoted, unbracketed commas. Only looking at the first
/// argument let `Kill x,^Y` through (137).
fn kill_args_name_a_global(args: &str) -> bool {
    let (mut depth, mut in_str, mut at_arg_start) = (0i32, false, true);
    for c in args.chars() {
        if in_str {
            if c == '"' {
                in_str = false;
            }
            continue;
        }
        match c {
            '"' => in_str = true,
            // A space right after a comma is a syntax error in ObjectScript, but reading on costs
            // nothing and a false positive beats a miss.
            ' ' | '\t' if depth == 0 && at_arg_start => continue,
            ' ' | '\t' if depth == 0 => return false,
            '(' => depth += 1,
            ')' => depth -= 1,
            ',' if depth == 0 => {
                at_arg_start = true;
                continue;
            }
            '^' if at_arg_start && depth == 0 => return true,
            _ => {}
        }
        if !c.is_whitespace() {
            at_arg_start = false;
        }
    }
    false
}

/// Error code returned when block-syntax code is submitted to the docker exec (terminal) path.
///
/// The docker exec path pipes code into `iris session` stdin, which processes input line-by-line
/// in terminal mode. Block syntax (`{}`) is not supported there — it causes a raw `<SYNTAX>`
/// error with no diagnostic. This error code surfaces before any docker exec is invoked so
/// agents get an actionable message and the `.mac` + `iris_compile` escape hatch.
pub const ERR_TERMINAL_SYNTAX_UNSUPPORTED: &str = "TERMINAL_SYNTAX_UNSUPPORTED";

/// Returns `true` when the code string contains `{}` block syntax that is not supported
/// in IRIS terminal (docker exec) mode.
///
/// The IRIS terminal interpreter is line-by-line. Block syntax — `If cond { ... }`,
/// `For ... { }`, etc. — causes a `<SYNTAX>` error with no explanation. This function
/// detects the pattern before any docker exec call so the caller can return an actionable
/// error.
///
/// **Detection rule**: a line (after left-trimming whitespace) starts with a
/// block-introducing keyword token AND that line contains a `{` that is NOT inside a
/// double-quoted string literal.
///
/// Block-introducing keywords detected: `If`/`I`, `Else`/`E`, `For`/`F`, `While`,
/// `Do`, `Try`, `Catch`. Note: `W` is NOT included — in ObjectScript `W` abbreviates
/// `Write`, not `While`. `While` has no single-letter abbreviation in terminal mode.
/// `ElseIf` is NOT included — it is not a terminal-mode keyword.
///
/// **False positives** (e.g. `{` on a line that starts with `If` but is inside a string
/// at the outer level) are conservatively safe — blocking code that looks like block
/// syntax is safer than missing real block syntax.
/// **False negatives** are not acceptable on the listed keywords.
pub fn contains_terminal_block_syntax(code: &str) -> bool {
    for line in code.lines() {
        let trimmed = line.trim_start();
        if trimmed.is_empty() {
            continue;
        }

        // Check whether this line starts with a block-introducing keyword.
        if !line_starts_with_block_keyword(trimmed) {
            continue;
        }

        // The line starts with a block keyword. Now check whether it contains a `{`
        // that is not inside a double-quoted string literal.
        if line_contains_unquoted_brace(trimmed) {
            return true;
        }
    }
    false
}

/// Returns `true` if the trimmed line starts with a block-introducing keyword followed
/// by a non-alphanumeric character (i.e., the keyword is not a prefix of a longer word).
///
/// Keywords (case-insensitive):
/// - `If` / `I` (abbreviation)
/// - `Else` / `E` (abbreviation)
/// - `For` / `F` (abbreviation)
/// - `While` (no single-letter abbreviation — `W` is `Write` in ObjectScript)
/// - `Do` (no single-letter abbreviation — `D` could start other names)
/// - `Try`
/// - `Catch`
/// - `ElseIf`
///
/// `ElseIf` was excluded on the grounds that it is "not a terminal-mode keyword", and the test
/// that was supposed to cover it asserted nothing at all. `ElseIf x=1 { ... }` is braced syntax
/// like any other: `else` does not match because `If` follows it without a separator, so the
/// line went through undetected. False negatives are the failure this check exists to prevent,
/// so the keyword is in the list.
fn line_starts_with_block_keyword(trimmed: &str) -> bool {
    // Pairs of (keyword_bytes, min_separator_after).
    // A keyword must be followed by end-of-string, a space, tab, or `(`.
    for kw in &[
        "if", "else", "elseif", "for", "while", "do", "try", "catch",
        // Single-letter abbreviations:
        "i", "e",
        "f",
        // Note: "w" excluded (it's Write), "d" excluded (ambiguous with other names that
        // start with D). "Do" is detected via the full form above.
    ] {
        let kw_len = kw.len();
        if trimmed.len() < kw_len {
            continue;
        }
        let prefix = &trimmed[..kw_len];
        if !prefix.eq_ignore_ascii_case(kw) {
            continue;
        }
        // Keyword matches — verify it ends at a word boundary.
        let after = &trimmed[kw_len..];
        if after.is_empty()
            || after.starts_with(' ')
            || after.starts_with('\t')
            || after.starts_with('(')
        {
            return true;
        }
    }
    false
}

/// Returns `true` if the line contains a `{` character that is not inside a double-quoted
/// string literal. ObjectScript strings use `""` to escape a literal double-quote inside
/// a string; no backslash escaping.
fn line_contains_unquoted_brace(line: &str) -> bool {
    let bytes = line.as_bytes();
    let len = bytes.len();
    let mut in_string = false;
    let mut i = 0;

    while i < len {
        let b = bytes[i];
        if in_string {
            if b == b'"' {
                // ObjectScript: `""` inside a string is an escaped double-quote.
                if i + 1 < len && bytes[i + 1] == b'"' {
                    i += 2;
                    continue;
                }
                in_string = false;
            }
            i += 1;
            continue;
        }
        // Not in a string.
        if b == b'"' {
            in_string = true;
            i += 1;
            continue;
        }
        if b == b'{' {
            return true;
        }
        i += 1;
    }
    false
}

/// The refusal envelope: a normal tool result in the existing `err_json` shape, not an
/// `McpError`, so the reporter's probes keep parsing the same response shape (Principle V).
///
/// Goes through `crate::tools::err_result` rather than building a `CallToolResult` by hand, so
/// the refusal cannot drift from every other error this server emits.
fn refusal(code: &str, msg: &str) -> Result<CallToolResult, McpError> {
    crate::tools::err_result(serde_json::json!({
        "success": false,
        "error_code": code,
        "error": msg,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The four row constructors are `const fn` used only from the `CLASSIFICATION` initialiser, so
    /// they are evaluated at compile time and never execute at runtime. Nothing outside this module
    /// can call them, and nothing asserted what they build — yet `de` returning `Write` by mistake
    /// would silently drop the destructive tier on all seven ☠ tools while every gate test still
    /// passed. These calls are the only runtime check that each constructor picks the class its
    /// name claims.
    #[test]
    fn the_row_constructors_build_the_class_their_name_claims() {
        let r = ro("iris_info");
        assert_eq!(r.tool, "iris_info");
        assert_eq!(r.default, WriteClass::ReadOnly);
        assert!(r.actions.is_empty(), "a single-class row has no overrides");

        assert_eq!(wr("iris_compile").default, WriteClass::Write);
        assert_eq!(de("global_kill").default, WriteClass::Destructive);
    }

    /// The four builder methods are `const fn` too, called only from the table initialiser. Each
    /// sets exactly the field its name claims and leaves the others alone: `local()` flips the kind
    /// without moving the target, `on_default()` moves the target without touching the kind.
    #[test]
    fn the_builder_methods_set_only_the_field_they_name() {
        let base = ro("iris_info");
        assert_eq!(base.target, TargetSource::Arg("server"));
        assert_eq!(base.kind, TargetKind::Iris);
        assert!(!base.scratch);

        let scratch = ro("iris_info").scratch();
        assert!(scratch.scratch);
        assert_eq!(scratch.kind, TargetKind::Iris);

        let on_default = ro("iris_info").on_default();
        assert_eq!(on_default.target, TargetSource::Default);
        assert_eq!(on_default.kind, TargetKind::Iris);

        let local = ro("iris_info").local();
        assert_eq!(local.kind, TargetKind::Local);
        assert_eq!(local.target, TargetSource::Arg("server"));

        let local_default = ro("iris_info").local_default();
        assert_eq!(local_default.kind, TargetKind::Local);
        assert_eq!(local_default.target, TargetSource::Default);

        let named = ro("iris_info").target(TargetSource::Session);
        assert_eq!(named.target, TargetSource::Session);
        assert_eq!(named.kind, TargetKind::Iris);
    }

    /// A top-level `destructive_tools_enabled = false` closes the destructive tier of every
    /// member, and the file that declared it is the source reported. A policy section that says
    /// `destructiveTools = true` does not reopen it; with no recorded source the report falls back
    /// to the config file.
    #[test]
    fn a_top_level_destructive_false_closes_every_member_and_names_its_file() {
        let operator = OperatorEnvGates::default();
        let member = |source: Option<GateSource>| MemberDeclaration {
            top_level: DeclaredGates {
                destructive_tools_enabled: Some(false),
                destructive_source: source,
                ..DeclaredGates::default()
            },
            write_tools: Some(true),
            destructive_tools: Some(true),
        };

        let with_source = resolve_member(
            &operator,
            &member(Some(GateSource::LegacyAllowProd)),
            SystemMode::Development,
            "USER",
        );
        assert!(with_source.write_enabled, "the write tier is not touched");
        assert!(!with_source.destructive_enabled);
        assert_eq!(with_source.destructive_source, GateSource::LegacyAllowProd);

        let without_source =
            resolve_member(&operator, &member(None), SystemMode::Development, "USER");
        assert!(!without_source.destructive_enabled);
        assert_eq!(without_source.destructive_source, GateSource::ConfigFile);
    }

    /// `confirmed` (the spelling `iris_execute` and `iris_ws_exec` took) is read as `confirm`
    /// when it is `true`, and only then; a `false` or non-boolean `confirmed` adds nothing, so a
    /// caller cannot confirm by sending the word.
    #[test]
    fn confirmed_true_becomes_confirm_and_nothing_else_does() {
        let run = |v: serde_json::Value| {
            let mut args = serde_json::Map::new();
            args.insert("confirmed".to_string(), v);
            normalize_dispatch_args(&mut args);
            args
        };
        assert_eq!(
            run(serde_json::json!(true)).get("confirm"),
            Some(&serde_json::Value::Bool(true))
        );
        assert!(!run(serde_json::json!(false)).contains_key("confirm"));
        assert!(!run(serde_json::json!("true")).contains_key("confirm"));
    }

    /// `mixed` must pass both the override table and the default through untouched: the overrides
    /// are what keep `iris_doc(get)` readable with writes off, and the default is what gates an
    /// action nobody listed.
    #[test]
    fn mixed_carries_its_overrides_and_default_through() {
        const ACTIONS: &[(&str, WriteClass)] = &[("get", WriteClass::ReadOnly)];
        let m = mixed("iris_doc", ACTIONS, WriteClass::Write);
        assert_eq!(m.tool, "iris_doc");
        assert_eq!(m.default, WriteClass::Write);
        assert_eq!(m.actions.len(), 1);
        assert_eq!(m.actions[0], ("get", WriteClass::ReadOnly));
    }

    /// Every refusal message ends in one of these strings, and `check_config` reports them, so a
    /// wrong arm here misdirects an operator to the wrong remedy. Asserted exhaustively — the match
    /// in `as_str` is total, so a new variant added without a wire value fails to compile, but a
    /// new variant wired to the *wrong* existing string would not.
    #[test]
    fn every_gate_source_has_its_documented_wire_value() {
        let cases = [
            (GateSource::OperatorEnv, "operator_env"),
            (GateSource::ConfigFile, "config_file"),
            (GateSource::LegacyAllowProd, "legacy_allow_prod"),
            (GateSource::InferredSystemMode, "inferred_system_mode"),
            (GateSource::InferredNamespace, "inferred_namespace"),
            (GateSource::InferredDefault, "inferred_default"),
            (GateSource::FailClosed, "fail_closed"),
        ];
        for (source, wire) in cases {
            assert_eq!(source.as_str(), wire, "{source:?}");
        }
    }

    // ── contains_terminal_block_syntax tests ─────────────────────────────────

    /// Guard fires on `If cond { ... }` — the classic block-syntax form that terminal mode rejects.
    /// This also validates US1 AC#2: the guard fires on the problematic input but NOT on the
    /// terminal-compatible variant without braces.
    #[test]
    fn test_contains_terminal_block_syntax_if_block_fires() {
        assert!(
            contains_terminal_block_syntax("If x=1 { Write 1 }"),
            "If...{{ }} must trigger the guard"
        );
        assert!(
            !contains_terminal_block_syntax("If x=1 Write 1"),
            "classic terminal form must NOT trigger the guard"
        );
    }

    /// `For` with block syntax fires; plain `For` loop does not.
    #[test]
    fn test_contains_terminal_block_syntax_for_block_fires() {
        assert!(
            contains_terminal_block_syntax("For i=1:1:10 { Write i,! }"),
            "For...{{ }} must trigger"
        );
        assert!(
            !contains_terminal_block_syntax("For i=1:1:10  Write i,!"),
            "For without braces must NOT trigger"
        );
    }

    /// `While` with block syntax fires.
    #[test]
    fn test_contains_terminal_block_syntax_while_block_fires() {
        assert!(
            contains_terminal_block_syntax("While cond { }"),
            "While...{{ }} must trigger"
        );
    }

    /// `{` inside a double-quoted string must NOT trigger.
    #[test]
    fn test_contains_terminal_block_syntax_brace_in_string_no_fire() {
        assert!(
            !contains_terminal_block_syntax(r#"Set x="{hello}""#),
            "brace inside string literal must not trigger"
        );
    }

    /// `{` inside a global subscript must NOT trigger.
    #[test]
    fn test_contains_terminal_block_syntax_brace_in_subscript_no_fire() {
        assert!(
            !contains_terminal_block_syntax(r#"Set ^Global("{")=1"#),
            "brace inside global subscript must not trigger"
        );
    }

    /// Plain code with no braces — no fire.
    #[test]
    fn test_contains_terminal_block_syntax_no_braces_no_fire() {
        assert!(
            !contains_terminal_block_syntax("Set x=1\nWrite x"),
            "code with no braces must not trigger"
        );
    }

    /// Empty string — no fire.
    #[test]
    fn test_contains_terminal_block_syntax_empty_no_fire() {
        assert!(
            !contains_terminal_block_syntax(""),
            "empty string must not trigger"
        );
    }

    /// `{` in what looks like a comment line — conservative: guard fires (false positive
    /// is safe, false negative is not).
    #[test]
    fn test_contains_terminal_block_syntax_comment_like_fires_conservatively() {
        // A bare `{` that follows a keyword — guard fires even if it looks like a comment.
        assert!(
            contains_terminal_block_syntax("If 1 {  // comment style"),
            "brace after If in comment-like line should still trigger (conservative)"
        );
    }

    /// `Else` (abbrev `E`) and `Try`/`Catch` with block syntax fire.
    #[test]
    fn test_contains_terminal_block_syntax_else_try_catch_fire() {
        assert!(
            contains_terminal_block_syntax("Else { Write 0 }"),
            "Else...{{ }} must trigger"
        );
        assert!(
            contains_terminal_block_syntax("Try { Set x=1 }"),
            "Try...{{ }} must trigger"
        );
        assert!(
            contains_terminal_block_syntax("Catch e { Write e }"),
            "Catch...{{ }} must trigger"
        );
    }

    /// `Do` with block syntax fires.
    #[test]
    fn test_contains_terminal_block_syntax_do_block_fires() {
        assert!(
            contains_terminal_block_syntax("Do { Write 1 }"),
            "Do...{{ }} must trigger"
        );
    }

    /// `ElseIf` is a detection keyword. It was not, and this test asserted nothing while
    /// claiming to document that — so a braced `ElseIf` reached the terminal undetected.
    #[test]
    fn test_contains_terminal_block_syntax_elseif_fires() {
        assert!(
            contains_terminal_block_syntax("ElseIf x=1 { Write 1 }"),
            "braced ElseIf is block syntax the terminal cannot take a line at a time"
        );
        assert!(
            !contains_terminal_block_syntax("ElseIfy=1"),
            "a name that merely starts with the keyword is not the keyword"
        );
    }

    // ── contains_terminal_block_syntax edge-case tests ────────────────────────

    /// Single-letter abbreviations `I` (If), `E` (Else), `F` (For) fire.
    /// Note: `W` is NOT in the list — in ObjectScript, `W` abbreviates `Write`, not `While`.
    /// `While` has no single-letter abbreviation in terminal mode.
    #[test]
    fn test_contains_terminal_block_syntax_abbreviations_fire() {
        assert!(
            contains_terminal_block_syntax("I x=1 { Write 1 }"),
            "abbreviated If (I) must trigger"
        );
        assert!(
            contains_terminal_block_syntax("E { Write 0 }"),
            "abbreviated Else (E) must trigger"
        );
        assert!(
            contains_terminal_block_syntax("F i=1:1:10 { Write i }"),
            "abbreviated For (F) must trigger"
        );
        // `W` is Write in ObjectScript — must NOT trigger the block-syntax guard.
        // Use a line that starts with `W` followed by whitespace and a brace — if W were
        // treated as While, this would falsely trigger.
        assert!(
            !contains_terminal_block_syntax("W x"),
            "W (Write) without brace — must NOT trigger"
        );
    }

    /// The snapshot is captured once per process and cannot be re-seeded. Whether *this* test wins
    /// the race to seed it depends on which other test in this binary touched a connection first,
    /// so both outcomes are asserted rather than assuming one: what matters is that a second seed
    /// never takes, because that is what stops a variable the process exported from being read back
    /// as an operator declaration (the #110 defect).
    #[test]
    fn the_operator_snapshot_is_captured_at_most_once() {
        let first = init_operator_env_gates(OperatorEnvGates::default());
        assert!(
            !init_operator_env_gates(OperatorEnvGates {
                write_tools_enabled: Some(true),
                destructive_enabled: Some(true),
                allow_prod: true,
            }),
            "a second seed must be refused"
        );
        if first {
            assert_eq!(operator_env_gates().write_tools_enabled, None);
            assert_eq!(operator_env_gates().destructive_enabled, None);
            assert!(!operator_env_gates().allow_prod);
        }
    }
}

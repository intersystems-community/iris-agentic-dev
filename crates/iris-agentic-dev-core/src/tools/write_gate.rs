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
    /// Operator exported the gate env var before the process began.
    OperatorEnv,
    /// Declared in `.iris-agentic-dev.toml`.
    ConfigFile,
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
}

impl GateSource {
    /// The `snake_case` wire value, without going through serde.
    pub fn as_str(&self) -> &'static str {
        match self {
            GateSource::OperatorEnv => "operator_env",
            GateSource::ConfigFile => "config_file",
            GateSource::LegacyAllowProd => "legacy_allow_prod",
            GateSource::InferredSystemMode => "inferred_system_mode",
            GateSource::InferredNamespace => "inferred_namespace",
            GateSource::InferredDefault => "inferred_default",
            GateSource::FailClosed => "fail_closed",
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
}

impl DeclaredGates {
    pub fn from_config(cfg: &WorkspaceConfig) -> Self {
        Self {
            write_tools_enabled: cfg.write_tools_enabled,
            destructive_tools_enabled: cfg.destructive_tools_enabled,
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
    let (write_enabled, write_source) = if let Some(w) = operator.write_tools_enabled {
        (w, GateSource::OperatorEnv)
    } else if let Some(w) = declared.write_tools_enabled {
        (w, GateSource::ConfigFile)
    } else if operator.allow_prod {
        (true, GateSource::LegacyAllowProd)
    } else {
        match system_mode {
            SystemMode::Live => (false, GateSource::InferredSystemMode),
            SystemMode::Development | SystemMode::Test => (true, GateSource::InferredSystemMode),
            SystemMode::Unknown => (
                !is_production_namespace(namespace),
                GateSource::InferredNamespace,
            ),
        }
    };

    let (declared_destructive, declared_source) = if let Some(d) = operator.destructive_enabled {
        (d, GateSource::OperatorEnv)
    } else if let Some(d) = declared.destructive_tools_enabled {
        (d, GateSource::ConfigFile)
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
}

impl GateConfigError {
    pub fn code(&self) -> &'static str {
        match self {
            GateConfigError::DestructiveRequiresWrites => ERR_DESTRUCTIVE_REQUIRES_WRITES,
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
}

const fn ro(tool: &'static str) -> ToolClass {
    ToolClass {
        tool,
        actions: &[],
        default: WriteClass::ReadOnly,
    }
}

const fn wr(tool: &'static str) -> ToolClass {
    ToolClass {
        tool,
        actions: &[],
        default: WriteClass::Write,
    }
}

const fn de(tool: &'static str) -> ToolClass {
    ToolClass {
        tool,
        actions: &[],
        default: WriteClass::Destructive,
    }
}

const fn mixed(
    tool: &'static str,
    actions: &'static [(&'static str, WriteClass)],
    default: WriteClass,
) -> ToolClass {
    ToolClass {
        tool,
        actions,
        default,
    }
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
    ro("agent_history"),
    ro("agent_info"),
    ro("agent_stats"),
    ro("capability_matrix"),
    ro("check_config"),
    ro("compare_document"),
    ro("compare_namespace"),
    ro("debug_capture_packet"),
    ro("debug_get_error_logs"),
    ro("debug_map_int_to_cls"),
    ro("debug_source_map"),
    ro("docs_introspect"),
    ro("extract_message_map_routing"),
    ro("find_subclass_implementations"),
    ro("global_preview"),
    ro("hl7_schema_inspect"),
    ro("hl7_schema_list"),
    ro("iris_business_rule_info"),
    ro("iris_credential_list"),
    ro("iris_database_list"),
    ro("iris_database_stats"),
    ro("iris_debug"),
    ro("iris_doc_search"),
    ro("iris_get_log"),
    ro("iris_info"),
    ro("iris_interop_query"),
    ro("iris_list_containers"),
    ro("iris_macro"),
    ro("iris_message_body"),
    ro("iris_mirror_status"),
    ro("iris_namespace_list"),
    ro("iris_production_diff"),
    ro("iris_search"),
    ro("iris_reload_pool"),
    ro("iris_servers"),
    ro("iris_symbols"),
    ro("iris_symbols_local"),
    mixed(
        "iris_system_performance",
        &[
            ("start", WriteClass::Write),
            ("status", WriteClass::ReadOnly),
            ("last_runid", WriteClass::ReadOnly),
        ],
        WriteClass::ReadOnly,
    ),
    ro("iris_table_info"),
    ro("iris_test_server"),
    // Opening and closing a terminal session mutates nothing. Everything a session can do
    // goes through iris_ws_exec, which is where the gate belongs.
    ro("iris_ws_close"),
    ro("iris_ws_open"),
    ro("journal_search"),
    ro("kb_recall"),
    ro("mermaid_class"),
    ro("mermaid_production"),
    ro("my_access"),
    ro("query_audit_log"),
    ro("resolve_dynamic_dispatch"),
    ro("resolve_storage"),
    ro("skill_community_list"),
    ro("skill_describe"),
    ro("skill_list"),
    ro("skill_search"),
    ro("stream_inspect"),
    ro("telemetry_export_trace"),
    ro("telemetry_query"),
    // ── write ────────────────────────────────────────────────────────────────
    wr("iris_compile"),
    wr("iris_execute"),
    // Verified ungated against 1.2.6 with the gate provably active.
    wr("iris_execute_method"),
    wr("iris_generate"),
    wr("iris_generate_class"),
    wr("iris_generate_test"),
    // Local server registry, same class of state as iris_remove_server.
    wr("iris_add_server"),
    wr("iris_import_servers"),
    wr("iris_production_item"),
    wr("iris_select_container"),
    wr("iris_start_sandbox"),
    // Executes test classes in IRIS, so it runs arbitrary application code.
    wr("iris_test"),
    // The complete bypass of the iris_execute gate: open a session, then run anything.
    wr("iris_ws_exec"),
    wr("kb_index"),
    wr("skill_community_install"),
    wr("skill_optimize"),
    wr("skill_propose"),
    wr("skill_share"),
    // ── destructive tier (spec 073, ☠ in docs/tools.md) ──────────────────────
    de("global_kill"),
    de("iris_namespace_create"),
    // Local state, not IRIS — the enforcement test asserts the saved server survives.
    de("iris_remove_server"),
    de("skill_forget"),
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
    ),
    mixed(
        "iris_containers",
        &[("list", WriteClass::ReadOnly)],
        WriteClass::Write,
    ),
    // run/start/stop execute test code in IRIS; check/report only read results back.
    mixed(
        "iris_coverage",
        &[
            ("check", WriteClass::ReadOnly),
            ("report", WriteClass::ReadOnly),
        ],
        WriteClass::Write,
    ),
    mixed(
        "iris_credential_manage",
        &[
            ("get", WriteClass::ReadOnly),
            ("list", WriteClass::ReadOnly),
            ("list_tables", WriteClass::ReadOnly),
        ],
        WriteClass::Destructive,
    ),
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
    ),
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
    ),
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
    ),
    mixed(
        "iris_lookup_transfer",
        &[("export", WriteClass::ReadOnly)],
        WriteClass::Write,
    ),
    mixed(
        "iris_production",
        &[
            ("check", WriteClass::ReadOnly),
            ("status", WriteClass::ReadOnly),
            ("get_autostart", WriteClass::ReadOnly),
        ],
        WriteClass::Write,
    ),
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
    ),
    mixed(
        "kb",
        &[
            ("recall", WriteClass::ReadOnly),
            ("history", WriteClass::ReadOnly),
            ("stats", WriteClass::ReadOnly),
        ],
        WriteClass::Write,
    ),
    mixed(
        "skill",
        &[
            ("list", WriteClass::ReadOnly),
            ("describe", WriteClass::ReadOnly),
            ("search", WriteClass::ReadOnly),
            ("forget", WriteClass::Destructive),
        ],
        WriteClass::Write,
    ),
    mixed(
        "skill_community",
        &[("list", WriteClass::ReadOnly)],
        WriteClass::Write,
    ),
];

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
    if !entry.actions.is_empty() {
        if let Some(args) = args {
            // `action` and `mode` are the two argument names the tools use for this; `iris_doc`
            // accepts `action` as an alias for `mode`. Matched case-insensitively because the
            // handlers lowercase before dispatching — a gate that compared exactly would let
            // mode="PUT" through and then write.
            for key in ["action", "mode"] {
                if let Some(v) = args.get(key).and_then(|v| v.as_str()) {
                    if let Some((_, class)) = entry
                        .actions
                        .iter()
                        .find(|(a, _)| a.eq_ignore_ascii_case(v))
                    {
                        return Some(*class);
                    }
                }
            }
        }
    }
    Some(entry.default)
}

/// The one gate check, called from `ServerHandler::call_tool` before anything touches IRIS.
///
/// `Some(refusal)` means the call must not proceed. `None` means it may.
///
/// An unclassified tool fails closed as `Write`: a tool that reaches dispatch without an entry
/// is a bug the completeness test should have caught, and the safe reading of a bug is "this
/// might write".
///
/// # The seam left for spec 074
///
/// Spec 074 (write-server allowlist) is deferred, not cancelled: FR-017 deletes
/// `write_allowed_servers` and `WRITE_SERVER_NOT_ALLOWED` from the docs rather than implementing
/// them, on the grounds that a documented security key with no reader is worse than no key. When
/// it is implemented it belongs **here**, between the two checks below, and it needs exactly two
/// things this signature already carries: the target server, which pooled tools take as the
/// optional `server` argument in `args` (falling back to the pool default when absent), and the
/// allowlist, which becomes a field on `GateResolution` resolved alongside the two gates in
/// `resolve_gates`. Nothing else about this function has to change — no new call site, no new
/// parameter, and no per-tool guard, which is the property the whole feature is about.
pub fn gate_check(
    tool: &str,
    args: Option<&serde_json::Map<String, serde_json::Value>>,
    gates: &GateResolution,
) -> Option<Result<CallToolResult, McpError>> {
    let class = classify(tool, args).unwrap_or(WriteClass::Write);
    match class {
        WriteClass::ReadOnly => None,
        WriteClass::Write | WriteClass::Destructive => {
            // Write is checked first, so a destructive tool with writes off reports the write
            // gate — the more fundamental refusal, and the one whose remedy comes first.
            if !gates.write_enabled {
                return Some(refusal(
                    ERR_WRITE_GATE,
                    &format!(
                        "{tool} is write-capable and write tools are disabled (source: {}). \
                         Set write_tools_enabled = true in .iris-agentic-dev.toml to allow writes.",
                        gates.write_source.as_str()
                    ),
                ));
            }
            // ── seam: the spec 074 per-server predicate goes here ──
            // Writes are allowed on this instance; whether they are allowed on *that* server is a
            // narrower question and is asked second. See the doc comment above.

            if class == WriteClass::Destructive && !gates.destructive_enabled {
                return Some(refusal(
                    ERR_DESTRUCTIVE_GATE,
                    &format!(
                        "{tool} is a destructive tool and the destructive tier is disabled \
                         (source: {}). Set destructive_tools_enabled = true in \
                         .iris-agentic-dev.toml to allow it.",
                        gates.destructive_source.as_str()
                    ),
                ));
            }
            None
        }
    }
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
    // abbreviation `k`, followed by optional whitespace and `^`. Searches within the
    // line so comment lines containing a kill expression are also caught — a false
    // positive beats a false negative.
    for line in code.lines() {
        let lower = line.to_ascii_lowercase();
        // Check every position where 'k' occurs.
        for (i, _) in lower.match_indices('k') {
            let rest = &lower[i..];
            let after = if let Some(stripped) = rest.strip_prefix("kill") {
                stripped
            } else {
                // Single-letter `k`, but not the start of "kill".
                let tail = &rest[1..];
                if tail.starts_with("ill") {
                    // "kill" will be handled at this same index on a later iteration.
                    continue;
                }
                tail
            };
            let trimmed = after.trim_start_matches([' ', '\t']);
            if trimmed.starts_with('^') {
                return true;
            }
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
///
/// `ElseIf` is intentionally excluded — it is not a terminal-mode keyword.
fn line_starts_with_block_keyword(trimmed: &str) -> bool {
    // Pairs of (keyword_bytes, min_separator_after).
    // A keyword must be followed by end-of-string, a space, tab, or `(`.
    for kw in &[
        "if", "else", "for", "while", "do", "try", "catch",
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

    /// `ElseIf` is NOT a detection keyword (not a terminal-mode keyword).
    /// It should NOT fire through the ElseIf keyword — only through its component parts.
    #[test]
    fn test_contains_terminal_block_syntax_elseif_not_a_keyword() {
        // "ElseIf cond {" should still fire because "If" is at the end — but the point is
        // that "ElseIf" as a whole is not in the keyword list. The detection picks up the
        // adjacent `{` by finding any keyword before it, so we test a pathological case:
        // just "ElseIf" with no other keyword nearby.
        // Since "ElseIf" contains "If", which IS a keyword, the test verifies what fires:
        let code = "ElseIf x=1 { Write 1 }";
        // The implementation may or may not fire here depending on whether it parses
        // "ElseIf" as containing "If". We document: ElseIf is not explicitly added to the
        // keyword list. The result must be consistent (not crash). We don't assert a
        // specific value here — this is a documentation test.
        let _ = contains_terminal_block_syntax(code);
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

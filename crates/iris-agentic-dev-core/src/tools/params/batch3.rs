//! Batch 3: `iris_admin`, one struct for 26 parameters across 25 actions.
//!
//! Flat and almost entirely optional, per research decision 4. JSON Schema can express "these six
//! fields matter when `action` is `create_user`" with `if`/`then`, but schemars will not derive that
//! from a flat struct, and hand-writing it for 26 fields across 25 actions produces a schema nobody
//! maintains. The per-action requirement tables in `docs/tools.md` stay the normative statement of
//! which fields each action needs, and `tests/unit/test_docs_contract.rs` keeps checking them
//! against the handler.
//!
//! `action` is the only required parameter in this whole feature: with it missing there is no action
//! to dispatch to, and a schema a client can read beats the generic INVALID_ACTION list it used to
//! get.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// The `time_range` filter of `action = "journal_search"`.
///
/// `journal_search_impl` reads this as `time_range.get("from")` / `.get("to")`, both ISO 8601
/// strings, and converts them to IRIS timestamps. Declaring it as an untyped object would advertise
/// nothing; declaring it as a named type would advertise a `$ref` a client has to resolve, so it is
/// inlined at the field.
#[derive(Debug, Clone, Default, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
// A container-level attribute in schemars 1.2, not a field one: every reference to this type is
// inlined, so the tool's schema carries the two fields instead of a `$ref` into `$defs`.
#[schemars(inline)]
pub struct JournalTimeRange {
    /// Lower bound, ISO 8601 (`2026-09-06T00:00:00Z`). Records before it are skipped.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub from: Option<String>,
    /// Upper bound, ISO 8601. Records after it are skipped.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub to: Option<String>,
}

/// Parameters of `iris_admin`.
#[derive(Debug, Clone, Default, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct IrisAdminParams {
    /// Which administration action to run. The enum is the dispatch table itself — any value
    /// outside it reaches the catch-all arm and comes back as `INVALID_ACTION`.
    #[schemars(extend("enum" = [
        "list_namespaces",
        "list_databases",
        "list_users",
        "list_roles",
        "list_webapps",
        "list_user_roles",
        "get_webapp",
        "check_permission",
        "create_user",
        "update_user",
        "delete_user",
        "create_namespace",
        "delete_namespace",
        "create_webapp",
        "delete_webapp",
        "view_locks",
        "view_processes",
        "journal_search",
        "namespace_mappings",
        "database_status",
        "clear_password_change_flag",
        "unlock_user",
        "fresh_container_setup",
        "mirror_add_async",
        "mirror_failover",
    ]))]
    pub action: String,
    /// Route this call to a named registered IRIS instance. If omitted, uses the default connection.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub server: Option<String>,

    // ── web applications ──────────────────────────────────────────────────────
    /// `list_webapps`: keep only applications of this type (`REST`, `CSP`). Matched
    /// case-insensitively against the type IRIS reports, so a value outside the set filters
    /// everything out rather than erroring.
    #[schemars(extend("enum" = ["REST", "CSP"]))]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub r#type: Option<String>,
    /// Web application path, e.g. `/api/atelier`. Used by `get_webapp`, `create_webapp`,
    /// `delete_webapp`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    /// `create_webapp`: the REST dispatch class the application routes to.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dispatch_class: Option<String>,

    // ── users, roles, permissions ─────────────────────────────────────────────
    /// The account to act on. Required by the user actions and by `unlock_user`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub username: Option<String>,
    /// Password for `create_user`, or the current password for `clear_password_change_flag` and
    /// `fresh_container_setup`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub password: Option<String>,
    /// `clear_password_change_flag` / `fresh_container_setup`: the replacement password. Omit to keep
    /// the current one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub new_password: Option<String>,
    /// Display name for `create_user` / `update_user`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub full_name: Option<String>,
    /// Comma-separated role names for `create_user` / `update_user`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub roles: Option<String>,
    /// Whether the user or web application is enabled.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub enabled: Option<bool>,
    /// `check_permission`: the resource to test, e.g. `%DB_IRISSYS`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resource: Option<String>,
    /// `check_permission`: the privilege to test. Defaults to `USE`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub permission: Option<String>,

    // ── namespaces and databases ──────────────────────────────────────────────
    /// Namespace name for `create_namespace` / `delete_namespace`, or database name for
    /// `database_status`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// `create_namespace`: existing database to hold routines and classes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub code_database: Option<String>,
    /// `create_namespace`: existing database to hold globals.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data_database: Option<String>,
    /// Namespace filter or target, depending on the action.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub namespace: Option<String>,

    // ── journal search ────────────────────────────────────────────────────────
    /// `journal_search`: global reference pattern. `*` and `?` are stripped before the search, which
    /// matches substrings.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub global_pattern: Option<String>,
    /// `journal_search`: time window. At least one of `global_pattern` or `time_range` is required.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub time_range: Option<JournalTimeRange>,
    /// `journal_search`: cap on returned records. Defaults to 100, capped at 1000.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_records: Option<u64>,

    // ── mirroring ─────────────────────────────────────────────────────────────
    /// `mirror_add_async`: name of the mirror set to join.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mirror_name: Option<String>,
    /// `mirror_add_async`: hostname of the primary member.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub primary_host: Option<String>,
    /// `mirror_add_async`: superserver port of the primary member. Defaults to 2188.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub primary_port: Option<u64>,
    /// `mirror_add_async`: instance name on the primary host. Defaults to `IRIS`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub instance_name: Option<String>,
    /// `mirror_add_async`: 0 = DR, 1 = read-only reporting, 2 = read-write reporting. Defaults to 0.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub async_member_type: Option<u64>,

    /// `mirror_failover`: must be `true`. The failover is irreversible without manual recovery.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub confirm: Option<bool>,
}

//! Batch 4: the read-only administration tools — SystemPerformance control, mirror status, journal
//! search, audit log, and the two access-inspection tools.
//!
//! Nineteen parameter slots, seventeen of them plain strings. The two that are not are the point of
//! the batch: `journal_search.max_entries` and `query_audit_log.limit` are read with `as_u64()` and
//! clamped into a `u32`, so a caller sending `"50"` got the default 100 and no explanation. They are
//! declared `Option<u32>` here, which turns that silent discard into a refusal.
//!
//! Nothing in this batch is `required`. `iris_system_performance.mode` is documented as required and
//! is deliberately optional in the schema: the handler reads it with `unwrap_or("")` and answers a
//! missing value with `unknown mode ''; valid values: ` and the whole list. Requiring it would
//! trade that sentence for a serde deserialization failure.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Parameters of `iris_system_performance`.
#[derive(Debug, Clone, Default, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct IrisSystemPerformanceParams {
    /// `start` (begin a profile run), `status` (poll one), `last_runid` (the most recent),
    /// `list_profiles`, `add_profile`, `delete_profile`, `list_runs` (completed runs, newest
    /// first), or `report` (locate the HTML a completed run wrote).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(extend("enum" = [
        "start", "status", "last_runid", "list_profiles", "add_profile", "delete_profile",
        "list_runs", "report"
    ]))]
    pub mode: Option<String>,
    /// `mode=status`: the run to poll. `waittime^SystemPerformance` answers an unknown ID with
    /// `-2^no such runid`. `mode=report`: the run to locate; omit for the newest completed run.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub run_id: Option<String>,
    /// `mode=start`: profile to run, default `test` (5 minutes). IRIS ships `test`, `30mins`,
    /// `4hours`, `8hours`, `12hours`, `24hours`, and an instance may define its own — any name of
    /// letters, digits and underscore is accepted, which is why this is not an enum.
    /// `mode=add_profile` / `delete_profile`: the profile to create or remove. IRIS silently
    /// strips characters outside that set, so anything else is rejected here.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub profile: Option<String>,
    /// `mode=add_profile`: what the profile is for. Shows up in `list_profiles` and in the
    /// Management Portal. Required.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// `mode=add_profile`: seconds between samples. The shipped profiles use 1 to 60.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub interval_seconds: Option<i64>,
    /// `mode=add_profile`: how many samples to take. `interval_seconds * sample_count` is how
    /// long the run lasts.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sample_count: Option<i64>,
    /// Route this call to a named registered IRIS instance. If omitted, uses the default connection.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub server: Option<String>,
}

/// Parameters of `iris_mirror_status`.
#[derive(Debug, Clone, Default, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct IrisMirrorStatusParams {
    /// Route this call to a named registered IRIS instance. If omitted, uses the default connection.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub server: Option<String>,
}

/// Parameters of `journal_search`.
///
/// `start`, `end` and `global_pattern` are declared as the handler reads them, which is all this
/// feature changes. They do not currently filter: the generated scan compares timestamps with
/// ObjectScript's numeric `<`, and it skips records with `Continue`, which never advances the
/// cursor. See `specs/113-typed-tool-schemas/parameter-audit.md`.
#[derive(Debug, Clone, Default, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct JournalSearchParams {
    /// Lower bound on the record timestamp, ISO 8601.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub start: Option<String>,
    /// Upper bound on the record timestamp, ISO 8601.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub end: Option<String>,
    /// Substring the global reference must contain, e.g. `^MyApp`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub global_pattern: Option<String>,
    /// Cap on returned records. Defaults to 100, clamped to 1–500.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_entries: Option<u32>,
    /// Route this call to a named registered IRIS instance. If omitted, uses the default connection.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub server: Option<String>,
}

/// Parameters of `query_audit_log`.
#[derive(Debug, Clone, Default, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct QueryAuditLogParams {
    /// Exact match on the `Username` column — not a substring match.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub user: Option<String>,
    /// Exact match on the `EventType` column, e.g. `%Login`. `Login` will not find `LoginFailure`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub event_type: Option<String>,
    /// Lower bound on `UTCTimeStamp`, inclusive.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub start: Option<String>,
    /// Upper bound on `UTCTimeStamp`, inclusive.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub end: Option<String>,
    /// Max rows. Defaults to 100, clamped to 1–500.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit: Option<u32>,
    /// Route this call to a named registered IRIS instance. If omitted, uses the default connection.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub server: Option<String>,
}

/// Parameters of `my_access`.
#[derive(Debug, Clone, Default, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MyAccessParams {
    /// Route this call to a named registered IRIS instance. If omitted, uses the default connection.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub server: Option<String>,
}

/// Parameters of `capability_matrix`.
#[derive(Debug, Clone, Default, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CapabilityMatrixParams {
    /// Account to look up. Defaults to the connected user.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub user: Option<String>,
    /// Route this call to a named registered IRIS instance. If omitted, uses the default connection.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub server: Option<String>,
}

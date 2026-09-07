//! Batch 5: the interoperability tools — the log/queue/message dispatcher, production item control,
//! production diff, message body reader, and business rule inspection.
//!
//! Thirty-three parameter slots, and the widest range of shapes in the feature: two integers, one
//! boolean, one array of strings, one string map, one nested object with its own required field, and
//! two parameters that accept an integer *or* a decimal string. `iris_interop_query` carries fifteen
//! of the thirty-three on its own, which is the clearest single case for this whole feature — a
//! fifteen-parameter tool whose entire contract lived in one paragraph of English.
//!
//! `session_id` and `since_id` use [`StringOrI64`](super::StringOrI64) because their handler reads
//! `as_i64().or_else(|| as_str()?.parse().ok())`. Declaring `integer` would reject
//! `session_id: "12345"`, which works today.
//!
//! Nothing here is `required`. `iris_message_body.message_id` is the closest call: the handler
//! answers a missing one with `INVALID_PARAMS: message_id is required`, and that sentence beats a
//! serde failure naming a Rust struct (FR-004).

use std::collections::BTreeMap;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::StringOrI64;

/// The `search_table` filter of `iris_interop_query`.
///
/// This mirrors `crate::tools::interop::SearchTableFilter`, which the handler deserializes the same
/// object into. Two structs rather than one because the handler's copy is `Deserialize`-only and
/// this one has to be `Serialize` for the round-trip through `p`; adding `Serialize` there would
/// have put a schema concern into the interop module for no gain.
///
/// `prop` has no default and no `Option`, so a filter without it fails to deserialize — which is
/// the one place in this batch where `required` appears in the advertised schema.
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
// Inlined rather than referenced: schemars would otherwise emit `{"$ref": "#/$defs/..."}` for the
// `search_table` property, and a client that does not follow `$ref` sees a parameter with no shape
// at all — the same blindness this feature removes, one level down.
#[schemars(inline)]
pub struct SearchTableParam {
    /// Indexed Search Table property to match, e.g. `PatientID`. An unknown name comes back as
    /// `SEARCH_PROP_NOT_FOUND` listing what the extent does have.
    pub prop: String,
    /// Exact value to match. Exactly one of `value` or `value_like` is required.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value: Option<String>,
    /// SQL `LIKE` pattern to match, e.g. `MRN12%`. Exactly one of `value` or `value_like`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value_like: Option<String>,
    /// Message body class to restrict the search to.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub class: Option<String>,
    /// Search Table extent to search. Defaults to `EnsLib.HL7.SearchTable`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub extent: Option<String>,
}

/// Parameters of `iris_interop_query`.
#[derive(Debug, Clone, Default, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct IrisInteropQueryParams {
    /// Which sub-query to run: `logs` (Event Log), `queues` (queue depths), or `messages`
    /// (`Ens.MessageHeader`). Defaults to `logs`; anything else is `INVALID_ACTION`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(extend("enum" = ["logs", "queues", "messages"]))]
    pub what: Option<String>,
    /// Config item name to narrow `logs` and `messages` to.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub component: Option<String>,
    /// `what=logs`: comma-separated severities, default `error,warning`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub log_type: Option<String>,
    /// Row cap, default 50.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit: Option<u32>,
    /// Interop namespace. Omit to use the connection's.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub namespace: Option<String>,
    /// `what=messages`: match on `SourceConfigName`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    /// `what=messages`: match on `TargetConfigName`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target: Option<String>,
    /// `what=messages`: match on `MessageBodyClassName`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message_class: Option<String>,
    /// `what=messages`: one session's messages. A number or a decimal string.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<StringOrI64>,
    /// `what=messages`: tail after this header ID. A number or a decimal string.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub since_id: Option<StringOrI64>,
    /// `what=messages`: body class to join, e.g. `Ens.StringContainer`. Its SQL table name is
    /// resolved server-side.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub body_class: Option<String>,
    /// `what=messages`: SQL predicate on the joined body table, e.g. `StringValue LIKE 'A%'`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub body_where: Option<String>,
    /// `what=messages`: body-table columns to add to each row.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub body_select: Option<Vec<String>>,
    /// `what=messages`: search an indexed Search Table field instead of the body table.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub search_table: Option<SearchTableParam>,
    /// Route this call to a named registered IRIS instance. If omitted, uses the default connection.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub server: Option<String>,
}

/// Parameters of `iris_production_item`.
#[derive(Debug, Clone, Default, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct IrisProductionItemParams {
    /// `enable`, `disable`, `get_settings`, or `set_settings`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(extend("enum" = ["enable", "disable", "get_settings", "set_settings"]))]
    pub action: Option<String>,
    /// Exact config item name, as it appears in the production definition.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub item: Option<String>,
    /// Interop namespace. Omit to use the connection's.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub namespace: Option<String>,
    /// `action=set_settings`: setting name to value. Values are strings — the handler reads
    /// `as_str()` on each and skips anything else, so a number here is dropped.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub settings: Option<BTreeMap<String, String>>,
    /// Route this call to a named registered IRIS instance. If omitted, uses the default connection.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub server: Option<String>,
}

/// Parameters of `iris_production_diff`.
#[derive(Debug, Clone, Default, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct IrisProductionDiffParams {
    /// Production to diff. Defaults to the running one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub production: Option<String>,
    /// Interop namespace. Omit to use the connection's.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub namespace: Option<String>,
    /// Route this call to a named registered IRIS instance. If omitted, uses the default connection.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub server: Option<String>,
}

/// Parameters of `iris_message_body`.
///
/// `acknowledgePhi` and `dataPolicy` are camelCase, unlike every other parameter on the surface.
/// They are renamed rather than corrected: this feature declares what the tools accept, and
/// snake_case aliases would be new capability.
#[derive(Debug, Clone, Default, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct IrisMessageBodyParams {
    /// `Ens.MessageHeader` ID whose body to read.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message_id: Option<String>,
    /// Interop namespace. Omit to use the connection's.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub namespace: Option<String>,
    /// Body bytes to return, default 65536, clamped to 1048576.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_bytes: Option<u32>,
    /// Required with `dataPolicy=allow`: acknowledges that the body may contain PHI.
    #[serde(
        rename = "acknowledgePhi",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub acknowledge_phi: Option<bool>,
    /// `block` (default), `allow`, or `redact`. The server's own policy is applied first, so this
    /// cannot widen what a `.iris-agentic-dev.toml` has restricted.
    #[serde(
        rename = "dataPolicy",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub data_policy: Option<String>,
    /// Route this call to a named registered IRIS instance. If omitted, uses the default connection.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub server: Option<String>,
}

/// Parameters of `iris_business_rule_info`.
#[derive(Debug, Clone, Default, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct IrisBusinessRuleInfoParams {
    /// `list` for every rule set, or `get` for one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(extend("enum" = ["list", "get"]))]
    pub action: Option<String>,
    /// `action=get`: the rule set to describe.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rule_name: Option<String>,
    /// Interop namespace. Omit to use the connection's.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub namespace: Option<String>,
    /// Route this call to a named registered IRIS instance. If omitted, uses the default connection.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub server: Option<String>,
}

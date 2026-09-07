//! Batch 6: Ensemble credentials and lookup tables.
//!
//! Fifteen slots, every one a plain optional string — the simplest batch in the feature and the one
//! where the missing schema cost the most, because three of the four tools write. A caller who typed
//! `tbl` instead of `table` got `TABLE_NOT_FOUND: Table not found:` with an empty name, and a caller
//! who typed `val` instead of `value` set the entry to the empty string.
//!
//! None of the four declares `server`. Their handlers call `self.iris_arc()` rather than resolving a
//! connection from the pool, so there is no instance selection to advertise; recorded as F6 in
//! `specs/113-typed-tool-schemas/parameter-audit.md` and guarded by
//! `server_is_absent_from_every_tool_in_this_batch`.
//!
//! Nothing is `required`, including the parameters the prose calls required. Each handler already
//! answers a missing one with a sentence naming the action that needed it — `get requires key`,
//! `import requires xml` — which beats a serde error naming a Rust struct (FR-004).

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Parameters of `iris_credential_list`.
#[derive(Debug, Clone, Default, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct IrisCredentialListParams {
    /// Interop namespace to list credentials from. Omit to use the connection's.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub namespace: Option<String>,
}

/// Parameters of `iris_credential_manage`.
#[derive(Debug, Clone, Default, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct IrisCredentialManageParams {
    /// `create`, `update`, or `delete`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(extend("enum" = ["create", "update", "delete"]))]
    pub action: Option<String>,
    /// Credential ID, as it appears in the Credentials page of the Management Portal.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    /// Username to store. Required for `create`, optional for `update`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub username: Option<String>,
    /// Password to store. Required for `create`, optional for `update`. Never returned by any tool.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub password: Option<String>,
    /// Interop namespace. Omit to use the connection's.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub namespace: Option<String>,
}

/// Parameters of `iris_lookup_manage`.
#[derive(Debug, Clone, Default, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct IrisLookupManageParams {
    /// `get`, `set`, `delete`, `list_keys`, or `list_tables`. The first, fourth and fifth are
    /// read-only; `set` and `delete` need the destructive tier, and so does any value this list does
    /// not contain.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(extend("enum" = ["get", "set", "delete", "list_keys", "list_tables"]))]
    pub action: Option<String>,
    /// Lookup table name. Required for everything except `list_tables`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub table: Option<String>,
    /// Entry key. Required for `get`, `set`, and `delete`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub key: Option<String>,
    /// Value to store. Required for `set`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value: Option<String>,
    /// Interop namespace. Omit to use the connection's.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub namespace: Option<String>,
}

/// Parameters of `iris_lookup_transfer`.
#[derive(Debug, Clone, Default, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct IrisLookupTransferParams {
    /// `export` (read-only) or `import` (write-gated).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(extend("enum" = ["export", "import"]))]
    pub action: Option<String>,
    /// Lookup table name to export from or import into.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub table: Option<String>,
    /// `action=import`: the `<lookupTable>` XML to load, in the form `export` returns.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub xml: Option<String>,
    /// Interop namespace. Omit to use the connection's.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub namespace: Option<String>,
}

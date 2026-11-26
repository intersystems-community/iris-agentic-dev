//! Batch 8: SQL storage maps.
//!
//! One tool with nine actions, all declared in one struct with `#[schemars(extend(...))]`.
//! Every param is optional and unused params are caught by the handler.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Parameters of `iris_storage_map`.
#[derive(Debug, Clone, Default, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct IrisStorageMapParams {
    /// Action: read, diagram, uses, compare, preview, propose, check, edit, replace.
    #[schemars(extend("enum" = ["read", "diagram", "uses", "compare", "preview", "propose", "check", "edit", "replace"]))]
    pub action: String,
    /// Fully qualified class name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub class: Option<String>,
    /// Map name; defaults to all maps or master map.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub map: Option<String>,
    /// Global name for `uses` and `propose` actions.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub global: Option<String>,
    /// Index globals to link to the master, for `propose` (at most 8, plain global names).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub index_globals: Option<Vec<String>>,
    /// Property names by field position, for `propose` on a global.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub field_names: Option<Vec<String>>,
    /// Other namespaces to search for `uses`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub namespaces: Option<Vec<String>>,
    /// Map version from `read` (for edit and replace).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    /// Path operations for `edit`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ops: Option<Vec<Value>>,
    /// SqlMap model for `replace`, `check`, `preview`.
    #[schemars(extend("type" = "object"))]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<Value>,
    /// Confirm removal for `replace`.
    #[serde(default)]
    pub confirm_removal: bool,
    /// Seed the ID counter.
    #[serde(default)]
    pub seed_counter: bool,
    /// Build indexes after write.
    #[serde(default)]
    pub build_indexes: bool,
    /// Include values in data samples.
    #[serde(default)]
    pub include_values: bool,
    /// Acknowledge PHI for data reads.
    #[serde(default, rename = "acknowledgePhi")]
    pub acknowledge_phi: bool,
    /// Maximum nodes for `compare` and `propose`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_nodes: Option<u32>,
    /// Maximum rows for `preview`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_rows: Option<u32>,
    /// Namespace for the operation; defaults to target namespace.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub namespace: Option<String>,
    /// Route this call to a named registered IRIS instance. If omitted, uses the default connection.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub server: Option<String>,
    /// Confirmation flag for subject-server verification.
    #[serde(default)]
    pub confirm: bool,
}

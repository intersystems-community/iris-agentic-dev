//! Batch 1: the comparison pair and the global preview/kill pair.
//!
//! `comparison_tools` and `admin_tools` already have structs called `CompareDocumentParams`,
//! `CompareNamespaceParams`, `GlobalPreviewParams` and `GlobalKillParams`, and those are *not* these.
//! They are argument bundles for the `_impl` functions and carry an `Arc<IrisConnection>` and an
//! `Arc<reqwest::Client>` — neither of which can derive `Deserialize` or `JsonSchema`, and neither of
//! which a caller could ever send. The structs below are the wire contract; the handler still builds
//! the impl struct from them plus the resolved connection.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Parameters of `compare_document`.
///
/// All four are optional because all four are read with `unwrap_or("")` today. Omitting `server_a`
/// produces `SERVER_NOT_FOUND: '' in pool. Use iris_servers to list available instances.`, which
/// tells a caller what to do next; a `required` declaration would replace it with a serde
/// deserialization error naming a Rust struct.
#[derive(Debug, Clone, Default, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CompareDocumentParams {
    /// Document to compare, e.g. `MyApp.Foo.cls` or `MyRoutine.mac`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub document: Option<String>,
    /// Name of the first registered instance (`iris_servers` lists them).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub server_a: Option<String>,
    /// Name of the second registered instance.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub server_b: Option<String>,
    /// Namespace to read the document from on both instances. Defaults to `server_a`'s namespace.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub namespace: Option<String>,
}

/// Parameters of `compare_namespace`.
#[derive(Debug, Clone, Default, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CompareNamespaceParams {
    /// Namespace to compare on both instances. Defaults to `server_a`'s namespace.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub namespace: Option<String>,
    /// Name of the first registered instance.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub server_a: Option<String>,
    /// Name of the second registered instance.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub server_b: Option<String>,
}

/// Parameters of `global_preview`.
#[derive(Debug, Clone, Default, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GlobalPreviewParams {
    /// Global name, with or without the leading `^`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub global: Option<String>,
    /// Route this call to a named registered IRIS instance. If omitted, uses the default connection.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub server: Option<String>,
    /// Maximum entries to preview. Default 20, clamped to 1–100.
    ///
    /// Read as an integer today, so `"count": "50"` is currently discarded in favour of the default.
    /// Declaring the type turns that silent discard into a rejection (FR-008).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub count: Option<u32>,
}

/// Parameters of `global_kill`.
#[derive(Debug, Clone, Default, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GlobalKillParams {
    /// Global name, with or without the leading `^`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub global: Option<String>,
    /// Route this call to a named registered IRIS instance. If omitted, uses the default connection.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub server: Option<String>,
    /// Token from `global_preview`, valid five minutes.
    ///
    /// Optional on purpose: omitting it must keep producing `CONFIRM_REQUIRED`, which names the
    /// preview call to make, rather than a deserialization error.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub confirm_token: Option<String>,
}

//! Batch 7: HL7 schema browsing, the Mermaid generators, storage resolution, and stream inspection.
//!
//! Twenty-one slots, the last of the thirty-one tools that advertised an open object. One integer —
//! `mermaid_class.depth`, read with `as_u64()` and clamped to 5 — and twenty strings.
//!
//! `stream_inspect` takes `oid`, `namespace` and `server`, and nothing else. It shipped documented
//! with a `max_chars` that no code read, which is the bug that named this feature; the struct below
//! is the fix, and `stream_inspect_declares_exactly_three_parameters` in
//! `tests/binary/schema_batch7.rs` is what keeps it fixed.
//!
//! Nothing is `required`. `mermaid_class.class` and `hl7_schema_inspect.schema` come closest — both
//! default to the empty string and fail downstream — but the handlers' own answers name the class or
//! report the missing HL7 library, which beats a serde error naming a Rust struct (FR-004).

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Parameters of `hl7_schema_list`.
#[derive(Debug, Clone, Default, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Hl7SchemaListParams {
    /// Namespace to list HL7 schemas from. Omit to use the connection's.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub namespace: Option<String>,
    /// Route this call to a named registered IRIS instance. If omitted, uses the default connection.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub server: Option<String>,
}

/// Parameters of `hl7_schema_inspect`.
#[derive(Debug, Clone, Default, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Hl7SchemaInspectParams {
    /// HL7 schema name, e.g. `2.5`. Without it the tool has nothing to inspect.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub schema: Option<String>,
    /// Segment to describe, e.g. `MSH`. Omit to list the schema's message structures instead.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub segment: Option<String>,
    /// Namespace to read the schema from. Omit to use the connection's.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub namespace: Option<String>,
    /// Route this call to a named registered IRIS instance. If omitted, uses the default connection.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub server: Option<String>,
}

/// Parameters of `mermaid_class`.
#[derive(Debug, Clone, Default, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MermaidClassParams {
    /// Fully qualified class name to diagram.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub class: Option<String>,
    /// Superclass levels to walk, default 3, clamped to 5. The answer echoes the value actually
    /// used, so a request for 9 comes back as 5.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub depth: Option<u32>,
    /// Namespace to resolve the class in. Omit to use the connection's.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub namespace: Option<String>,
    /// Route this call to a named registered IRIS instance. If omitted, uses the default connection.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub server: Option<String>,
}

/// Parameters of `mermaid_production`.
#[derive(Debug, Clone, Default, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MermaidProductionParams {
    /// Full production class name. A name with no production behind it draws a single node rather
    /// than erroring.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub production: Option<String>,
    /// Interop namespace. Omit to use the connection's.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub namespace: Option<String>,
    /// Route this call to a named registered IRIS instance. If omitted, uses the default connection.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub server: Option<String>,
}

/// Parameters of `resolve_storage`.
#[derive(Debug, Clone, Default, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ResolveStorageParams {
    /// Fully qualified class name whose global maps to resolve. A class with no compiled storage
    /// comes back with an empty list, not an error.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub class: Option<String>,
    /// Namespace to resolve the class in. Omit to use the connection's.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub namespace: Option<String>,
    /// Route this call to a named registered IRIS instance. If omitted, uses the default connection.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub server: Option<String>,
}

/// Parameters of `stream_inspect`.
///
/// Three fields, and no fourth. See the module doc: this is the tool whose documentation promised a
/// `max_chars` the code never read.
#[derive(Debug, Clone, Default, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct StreamInspectParams {
    /// Stream OID — an integer as a string. Tried against `%Stream.GlobalCharacter` first, then
    /// `%Stream.GlobalBinary`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub oid: Option<String>,
    /// Namespace holding the stream. Omit to use the connection's.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub namespace: Option<String>,
    /// Route this call to a named registered IRIS instance. If omitted, uses the default connection.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub server: Option<String>,
}

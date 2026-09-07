//! Per-tool parameter structs for the tools that used to take `AnyParams`.
//!
//! # Why these exist
//!
//! `AnyParams` was a newtype around `serde_json::Value` with a hand-written `JsonSchema` impl
//! emitting `{"type": "object"}`. A tool declared as `Parameters<AnyParams>` therefore advertised
//! no `properties`, no types, no `required`, and no enums — 31 of 81 tools, reading 132 parameter
//! slots across 71 distinct names, documented only in English prose inside the tool description.
//! Two consequences, both shipped: an agent had to parse prose to find a parameter name, and a
//! misspelled or invented key was silently ignored rather than rejected (`stream_inspect` was
//! documented with a `max_chars` that no code read, so a caller asking for 10,000 characters got
//! everything).
//!
//! # The conversion shape
//!
//! Each handler keeps its body byte-identical. The signature takes a typed struct, and the first
//! line of the body re-serializes it back into a `serde_json::Value` named `p`, so every existing
//! `p.get("key").and_then(|v| v.as_str())` line still compiles and still means the same thing:
//!
//! ```ignore
//! async fn iris_namespace_list(
//!     &self,
//!     Parameters(params): Parameters<IrisNamespaceListParams>,
//! ) -> Result<CallToolResult, McpError> {
//!     let p = serde_json::to_value(&params).unwrap_or(serde_json::Value::Null);
//!     // ... unchanged ...
//! }
//! ```
//!
//! That is not laziness. It keeps two genuinely independent sources for the invariant the tests
//! check: the advertised schema comes from the struct, and the read set comes from the `p.get`
//! literals in the body. If the handler destructured typed fields instead, both sides would derive
//! from the same struct and "the schema matches what the handler reads" would be a tautology.
//!
//! Every optional field carries `#[serde(default, skip_serializing_if = "Option::is_none")]`. The
//! `skip_serializing_if` half matters more than it looks: without it an omitted parameter
//! round-trips as `Value::Null` rather than being absent, so `p.get("x").is_some()` flips from
//! false to true and any presence check in a handler changes behaviour.
//!
//! # Enums
//!
//! Closed value sets stay `Option<String>` with `#[schemars(extend("enum" = [...]))]` rather than
//! becoming Rust enums, so the handler's own validation keeps producing its specific error
//! (`unknown mode 'x'; valid values: start, status, last_runid`). A Rust enum would move rejection
//! into serde and replace that message with a generic deserialization failure.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

pub mod batch1;
pub mod batch2;
pub mod batch3;
pub mod batch4;
pub mod batch5;
pub mod batch6;
pub mod batch7;

/// An integer that may arrive as a JSON number or as a decimal string.
///
/// Two parameters need this: `iris_interop_query`'s `session_id` and `since_id`. Their handlers
/// read `as_i64().or_else(|| as_str().and_then(|s| s.parse().ok()))`, so both `12345` and `"12345"`
/// work today and declaring either type alone would reject calls that currently succeed.
///
/// Everything else numeric on the surface is read as `as_u64()` only, and gets a plain
/// `Option<u32>` — a string there is discarded today, so declaring it correctly turns a silent
/// discard into a rejection. That is the one intentional behaviour change in this feature.
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
#[serde(untagged)]
// schemars cannot express an untagged integer-or-string, so the type array is set directly.
#[schemars(extend("type" = ["integer", "string"]))]
// Inlined so the two-type declaration lands on the property itself; behind a `$ref` a client that
// does not resolve `$defs` sees a parameter with no type.
#[schemars(inline)]
pub enum StringOrI64 {
    Int(i64),
    Str(String),
}

impl StringOrI64 {
    /// The integer value, or `None` for a string that is not a decimal integer.
    ///
    /// `None` rather than an error, because that is exactly what the handlers do today: an
    /// unparseable string falls through to the parameter's default.
    pub fn as_i64(&self) -> Option<i64> {
        match self {
            Self::Int(i) => Some(*i),
            Self::Str(s) => s.trim().parse().ok(),
        }
    }
}

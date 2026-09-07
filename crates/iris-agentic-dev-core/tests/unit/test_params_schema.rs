//! Shared parameter-type behaviour: `StringOrI64` and the closed `NoParams`.
//!
//! These are the two pieces every conversion batch leans on, so they are asserted once here
//! rather than re-asserted per batch.

use iris_agentic_dev_core::tools::params::StringOrI64;
use schemars::schema_for;

/// `iris_interop_query` reads `session_id` and `since_id` with
/// `as_i64().or_else(|| as_str()?.parse().ok())`, so both `12345` and `"12345"` work today.
/// Declaring either one alone would break the other — the one case where a single JSON type
/// is not enough (research decision 3).
#[test]
fn string_or_i64_accepts_both_json_forms() {
    #[derive(serde::Deserialize)]
    struct Probe {
        since_id: StringOrI64,
    }

    // Parsed from JSON *strings*, not struct literals — a struct literal would not exercise
    // serde at all, which is the #110 pattern.
    let from_int: Probe = serde_json::from_str(r#"{"since_id": 12345}"#).expect("integer form");
    let from_str: Probe = serde_json::from_str(r#"{"since_id": "12345"}"#).expect("string form");

    assert_eq!(from_int.since_id.as_i64(), Some(12345));
    assert_eq!(from_str.since_id.as_i64(), Some(12345));
    assert_eq!(from_int.since_id.as_i64(), from_str.since_id.as_i64());
}

/// A non-numeric string is not an error at the type level — the handler's own
/// `as_str()?.parse().ok()` returns `None` today and the parameter is ignored. Preserving that
/// exactly is FR-004; turning it into a deserialization error would be a behaviour change.
#[test]
fn string_or_i64_keeps_unparseable_strings_as_none() {
    let v: StringOrI64 = serde_json::from_str(r#""not-a-number""#).expect("still deserializes");
    assert_eq!(v.as_i64(), None);
}

/// The schema must advertise both types. schemars has no built-in for an untagged
/// integer-or-string, so this is a `#[schemars(extend(...))]` override and worth pinning:
/// if the override is dropped the schema silently narrows to one branch.
#[test]
fn string_or_i64_advertises_both_types() {
    let schema = serde_json::to_value(schema_for!(StringOrI64)).expect("schema serializes");
    let ty = schema
        .get("type")
        .unwrap_or_else(|| panic!("StringOrI64 schema has no `type`: {schema}"));
    let types: Vec<&str> = ty
        .as_array()
        .unwrap_or_else(|| panic!("`type` must be an array of two, got {ty}"))
        .iter()
        .map(|v| v.as_str().expect("each type entry is a string"))
        .collect();
    assert_eq!(types, vec!["integer", "string"]);
}

/// `NoParams` is what the genuinely no-argument tools deserialize into. Without
/// `deny_unknown_fields` it accepts anything, which is the open surface this feature removes —
/// and it must still emit a `properties` key so `tools/list` shows an empty object rather than
/// nothing at all.
#[test]
fn no_params_is_closed_and_declares_empty_properties() {
    let schema = serde_json::to_value(schema_for!(iris_agentic_dev_core::tools::NoParams))
        .expect("schema serializes");

    assert_eq!(
        schema.get("additionalProperties"),
        Some(&serde_json::json!(false)),
        "NoParams must reject unknown fields; schema was {schema}"
    );
    assert!(
        schema.get("properties").is_some(),
        "NoParams must emit a `properties` key (empty object) so a client sees a declared \
         parameter set rather than an unspecified one; schema was {schema}"
    );
    assert_eq!(
        schema
            .get("properties")
            .and_then(serde_json::Value::as_object)
            .map(serde_json::Map::len),
        Some(0),
        "NoParams takes no parameters"
    );
}

/// Passing a key to a no-argument tool is a caller error, and this is where it becomes one.
#[test]
fn no_params_rejects_a_stray_key() {
    let err = serde_json::from_str::<iris_agentic_dev_core::tools::NoParams>(r#"{"server":"x"}"#)
        .expect_err("a stray key must be rejected, not ignored");
    assert!(
        err.to_string().contains("server"),
        "the error must name the offending key, got: {err}"
    );
}

//! The schema-only call suite: its validator, and the inventory it has to cover (113 T036b/T037).
//!
//! SC-007 asks whether the advertised schema is enough on its own. Answering it needs two things
//! the `jira_bugs` suite cannot supply: a task per converted tool that is one call rather than a
//! code repair, and a way to judge a call without making it. Six of the thirty-one mutate state
//! (`global_kill`, `iris_admin`, `iris_namespace_create`, `iris_credential_manage`,
//! `iris_lookup_manage`, `iris_lookup_transfer`), so "run it and see" is not available. The
//! judgement is therefore made against the schema the server advertises: the same four checks the
//! server itself applies — unknown keys, required keys, declared types, declared enums — plus the
//! parameters the request cannot be satisfied without.
//!
//! `validate_call` is the pure half and is tested here. The listing is read from a spawned server
//! in `tests/binary/schema_task_coverage.rs`, and the model is asked for calls in
//! `tests/integration/sc007_schema_only.rs`.

use iris_agentic_dev_core::benchmark::schema_tasks::{
    load_schema_tasks, validate_call, CallVerdict,
};
use serde_json::json;

/// The thirty-one tools this feature converted, spelled out rather than derived.
///
/// Derived from the source it is meant to police, this list would agree with a mistake: a tool
/// dropped from the conversion would also drop out of the inventory and the gap would read as
/// coverage. The names come from the spec's own list.
const CONVERTED: &[&str] = &[
    "capability_matrix",
    "compare_document",
    "compare_namespace",
    "global_kill",
    "global_preview",
    "hl7_schema_inspect",
    "hl7_schema_list",
    "iris_admin",
    "iris_business_rule_info",
    "iris_containers",
    "iris_credential_list",
    "iris_credential_manage",
    "iris_database_list",
    "iris_database_stats",
    "iris_interop_query",
    "iris_lookup_manage",
    "iris_lookup_transfer",
    "iris_message_body",
    "iris_mirror_status",
    "iris_namespace_create",
    "iris_namespace_list",
    "iris_production_diff",
    "iris_production_item",
    "iris_system_performance",
    "journal_search",
    "mermaid_class",
    "mermaid_production",
    "my_access",
    "query_audit_log",
    "resolve_storage",
    "stream_inspect",
];

/// A stand-in for the shape the server really advertises: nullable parameters as `anyOf` with a
/// `null` branch, which is what `#[derive(JsonSchema)]` emits for `Option<T>`. A validator written
/// against a flat `"type": "string"` would accept everything here by finding no type at all.
fn sample_schema() -> serde_json::Value {
    json!({
        "type": "object",
        "additionalProperties": false,
        "required": ["mode"],
        "properties": {
            "mode": {
                "anyOf": [
                    {"type": "string", "enum": ["start", "status", "last_runid"]},
                    {"type": "null"}
                ]
            },
            "count": {
                "anyOf": [{"type": "integer", "format": "uint32"}, {"type": "null"}]
            },
            "server": {
                "anyOf": [{"type": "string"}, {"type": "null"}]
            }
        }
    })
}

fn reasons(verdict: &CallVerdict) -> String {
    match verdict {
        CallVerdict::Accepted => "accepted".to_string(),
        CallVerdict::Rejected(rs) => rs.join(" | "),
    }
}

#[test]
fn a_call_using_only_declared_keys_is_accepted() {
    let verdict = validate_call(
        &sample_schema(),
        &json!({"mode": "start", "count": 20}),
        &["mode".to_string()],
    );
    assert_eq!(verdict, CallVerdict::Accepted, "{}", reasons(&verdict));
}

#[test]
fn an_undeclared_key_is_rejected_and_named() {
    // The `max_chars` shape: a plausible parameter that exists only in someone's head.
    let verdict = validate_call(
        &sample_schema(),
        &json!({"mode": "start", "max_chars": 10}),
        &[],
    );
    let rs = reasons(&verdict);
    assert!(rs.contains("max_chars"), "{rs}");
    assert_ne!(verdict, CallVerdict::Accepted);
}

#[test]
fn every_undeclared_key_is_reported_not_just_the_first() {
    // A model that guessed two parameters should learn about both in one round trip; reporting one
    // at a time is what makes an agent loop.
    let verdict = validate_call(
        &sample_schema(),
        &json!({"mode": "start", "max_chars": 10, "verbose": true}),
        &[],
    );
    let rs = reasons(&verdict);
    assert!(rs.contains("max_chars") && rs.contains("verbose"), "{rs}");
}

#[test]
fn a_missing_required_key_is_rejected() {
    let verdict = validate_call(&sample_schema(), &json!({"server": "dev"}), &[]);
    let rs = reasons(&verdict);
    assert!(rs.contains("mode"), "{rs}");
}

#[test]
fn a_wrong_type_is_rejected_even_through_anyof() {
    let verdict = validate_call(
        &sample_schema(),
        &json!({"mode": "start", "count": "20"}),
        &[],
    );
    let rs = reasons(&verdict);
    assert!(rs.contains("count"), "{rs}");
    assert!(rs.contains("integer"), "{rs}");
}

#[test]
fn a_value_outside_a_declared_enum_is_rejected() {
    let verdict = validate_call(&sample_schema(), &json!({"mode": "begin"}), &[]);
    let rs = reasons(&verdict);
    assert!(rs.contains("begin"), "{rs}");
    assert!(
        rs.contains("start"),
        "the accepted values belong in the reason: {rs}"
    );
}

#[test]
fn null_is_accepted_where_the_schema_allows_it() {
    // `Option<T>` declares a null branch, and a model that fills every property with an explicit
    // null is doing something the server accepts. Rejecting it here would fail a call the server
    // would have answered.
    let verdict = validate_call(
        &sample_schema(),
        &json!({"mode": "start", "server": null}),
        &[],
    );
    assert_eq!(verdict, CallVerdict::Accepted, "{}", reasons(&verdict));
}

#[test]
fn a_call_missing_a_parameter_the_request_needs_is_rejected() {
    // The check that keeps the measurement honest. `global_preview` with no `global` deserializes
    // fine and answers with an error about the empty global — accepted by the server, useless as an
    // answer to "show me the first entries of ^Foo".
    let verdict = validate_call(
        &sample_schema(),
        &json!({"mode": "start"}),
        &["mode".to_string(), "count".to_string()],
    );
    let rs = reasons(&verdict);
    assert!(rs.contains("count"), "{rs}");
}

#[test]
fn a_non_object_argument_is_rejected() {
    let verdict = validate_call(&sample_schema(), &json!("mode=start"), &[]);
    assert_ne!(verdict, CallVerdict::Accepted);
}

#[test]
fn every_converted_tool_has_a_task() {
    let tasks = load_schema_tasks().expect("the embedded schema-call suite must parse");
    let covered: std::collections::BTreeSet<&str> = tasks.iter().map(|t| t.tool.as_str()).collect();
    let missing: Vec<&&str> = CONVERTED
        .iter()
        .filter(|t| !covered.contains(**t))
        .collect();
    assert!(
        missing.is_empty(),
        "SC-007 is measured per tool, so a tool with no task is a gap and not a pass. Missing: \
         {missing:?}"
    );
}

#[test]
fn no_task_names_a_tool_outside_the_conversion() {
    let tasks = load_schema_tasks().expect("the embedded schema-call suite must parse");
    let stray: Vec<&str> = tasks
        .iter()
        .map(|t| t.tool.as_str())
        .filter(|t| !CONVERTED.contains(t))
        .collect();
    assert!(
        stray.is_empty(),
        "these tasks name tools this feature did not convert, so their results do not bear on \
         SC-007: {stray:?}"
    );
}

#[test]
fn each_task_is_one_tool_asked_once() {
    let tasks = load_schema_tasks().expect("the embedded schema-call suite must parse");
    let mut seen = std::collections::BTreeSet::new();
    for task in &tasks {
        assert!(
            seen.insert(task.tool.clone()),
            "two tasks target `{}` — the per-tool pass/fail table cannot have two rows for one \
             tool",
            task.tool
        );
        assert!(
            !task.request.trim().is_empty(),
            "`{}` has an empty request, so the model would be asked nothing",
            task.tool
        );
        assert!(
            !task.expect_keys.is_empty(),
            "`{}` expects no parameters, so an empty `{{}}` would pass and the task would measure \
             nothing",
            task.tool
        );
    }
}

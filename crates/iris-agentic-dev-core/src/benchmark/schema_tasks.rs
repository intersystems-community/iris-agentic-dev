//! One-call tasks per converted tool, and the schema-only judgement of a call (113 T036b/T037).
//!
//! The `jira_bugs` suite scores a code repair: give the model a broken class, let it propose a fix,
//! compile it, run the unit test. Nothing in that shape asks whether a tool's parameters can be
//! discovered, and none of the thirty-one tools this feature converted appears in it. SC-007 needs a
//! different unit of work — one request, one call, judged on whether the call would be accepted.
//!
//! The judgement is made against the advertised schema rather than by making the call. Six of the
//! thirty-one mutate state (`global_kill`, `iris_admin`, `iris_namespace_create`,
//! `iris_credential_manage`, `iris_lookup_manage`, `iris_lookup_transfer`), and a benchmark that
//! deletes a global to score a point is not a benchmark anyone will run twice. Checking the schema
//! loses nothing that matters here: `call_tool` decides acceptance from the same four facts
//! (unknown keys, required keys, declared types, declared enums), so a call this module accepts is a
//! call the server accepts.
//!
//! One check goes beyond the server: `expect_keys`, the parameters the request cannot be answered
//! without. Every converted tool takes all its parameters optionally, by design — FR-004 keeps the
//! downstream errors (`SERVER_NOT_FOUND`, `CONFIRM_REQUIRED`) that requiring them would replace with
//! a deserialization failure. So `global_preview {}` deserializes cleanly and answers with an error
//! about the empty global. Accepted by the server, and no answer at all to "preview `^Foo`". Without
//! `expect_keys` a model could score 31 out of 31 by sending `{}` every time.

use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

/// A single request, the tool that answers it, and the parameters the answer needs.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct SchemaTask {
    /// The tool the request is about. The model is not asked to choose a tool — SC-007 measures
    /// whether one tool's schema is legible, and tool selection is a different question with its own
    /// failure modes.
    pub tool: String,
    /// What a developer wants, in the words they would use. No parameter names, no JSON: naming the
    /// parameters in the request would hand over the answer the schema is supposed to supply.
    pub request: String,
    /// Parameters whose absence makes the call useless even though the server would accept it.
    pub expect_keys: Vec<String>,
}

/// `tasks/schema_calls/*.json` embedded at compile time.
///
/// `include_str!` rather than a directory read for the reason `EMBEDDED_TASKS` gives: a path baked
/// from `CARGO_MANIFEST_DIR` points at the build machine, so a released binary reading it finds
/// nothing.
macro_rules! schema_task {
    ($name:literal) => {
        include_str!(concat!("tasks/schema_calls/", $name))
    };
}

const EMBEDDED_SCHEMA_TASKS: &[&str] = &[
    schema_task!("capability_matrix.json"),
    schema_task!("compare_document.json"),
    schema_task!("compare_namespace.json"),
    schema_task!("global_kill.json"),
    schema_task!("global_preview.json"),
    schema_task!("hl7_schema_inspect.json"),
    schema_task!("hl7_schema_list.json"),
    schema_task!("iris_admin.json"),
    schema_task!("iris_business_rule_info.json"),
    schema_task!("iris_containers.json"),
    schema_task!("iris_credential_list.json"),
    schema_task!("iris_credential_manage.json"),
    schema_task!("iris_database_list.json"),
    schema_task!("iris_database_stats.json"),
    schema_task!("iris_interop_query.json"),
    schema_task!("iris_lookup_manage.json"),
    schema_task!("iris_lookup_transfer.json"),
    schema_task!("iris_message_body.json"),
    schema_task!("iris_mirror_status.json"),
    schema_task!("iris_namespace_create.json"),
    schema_task!("iris_namespace_list.json"),
    schema_task!("iris_production_diff.json"),
    schema_task!("iris_production_item.json"),
    schema_task!("iris_system_performance.json"),
    schema_task!("journal_search.json"),
    schema_task!("mermaid_class.json"),
    schema_task!("mermaid_production.json"),
    schema_task!("my_access.json"),
    schema_task!("query_audit_log.json"),
    schema_task!("resolve_storage.json"),
    schema_task!("stream_inspect.json"),
];

/// The embedded schema-call suite, in file order.
pub fn load_schema_tasks() -> anyhow::Result<Vec<SchemaTask>> {
    EMBEDDED_SCHEMA_TASKS
        .iter()
        .enumerate()
        .map(|(i, contents)| {
            serde_json::from_str::<SchemaTask>(contents)
                .map_err(|e| anyhow::anyhow!("failed to parse embedded schema task #{i}: {e}"))
        })
        .collect()
}

/// Whether a proposed call would be accepted, and if not, why in the caller's terms.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub enum CallVerdict {
    Accepted,
    /// Every problem found, not the first — a model that guessed two parameters should hear about
    /// both, and a per-tool result table is more useful with the full reason in it.
    Rejected(Vec<String>),
}

/// The types a property declares, flattened through `anyOf` and through `"type": [...]`.
///
/// `Option<T>` reflects as `anyOf: [{type: T}, {type: null}]`, so a validator that reads only a
/// top-level `"type"` finds nothing on almost every parameter on this surface and silently accepts
/// every value it is handed.
fn declared_types(prop: &serde_json::Value) -> BTreeSet<String> {
    let mut types = BTreeSet::new();
    let mut collect = |v: &serde_json::Value| match v.get("type") {
        Some(serde_json::Value::String(s)) => {
            types.insert(s.clone());
        }
        Some(serde_json::Value::Array(items)) => {
            for item in items {
                if let Some(s) = item.as_str() {
                    types.insert(s.to_string());
                }
            }
        }
        _ => {}
    };
    collect(prop);
    if let Some(branches) = prop.get("anyOf").and_then(|b| b.as_array()) {
        for branch in branches {
            collect(branch);
        }
    }
    types
}

/// The values a property declares, flattened the same way. Empty means "not an enum".
fn declared_enum(prop: &serde_json::Value) -> Vec<String> {
    let mut values = Vec::new();
    let mut collect = |v: &serde_json::Value| {
        if let Some(items) = v.get("enum").and_then(|e| e.as_array()) {
            for item in items {
                if let Some(s) = item.as_str() {
                    values.push(s.to_string());
                }
            }
        }
    };
    collect(prop);
    if let Some(branches) = prop.get("anyOf").and_then(|b| b.as_array()) {
        for branch in branches {
            collect(branch);
        }
    }
    values
}

/// The JSON Schema type name for a value, as the schema would spell it.
fn json_type(value: &serde_json::Value) -> &'static str {
    match value {
        serde_json::Value::Null => "null",
        serde_json::Value::Bool(_) => "boolean",
        serde_json::Value::Number(n) if n.is_i64() || n.is_u64() => "integer",
        serde_json::Value::Number(_) => "number",
        serde_json::Value::String(_) => "string",
        serde_json::Value::Array(_) => "array",
        serde_json::Value::Object(_) => "object",
    }
}

/// Judges `args` against `schema`, plus the parameters `expect_keys` says the request needs.
///
/// The four schema checks mirror the server's: unknown keys (`additionalProperties: false`), missing
/// `required`, declared type, declared enum. `expect_keys` is the fifth and is not the server's
/// business — see the module docs for why a call the server accepts can still be a wrong answer.
pub fn validate_call(
    schema: &serde_json::Value,
    args: &serde_json::Value,
    expect_keys: &[String],
) -> CallVerdict {
    let Some(args) = args.as_object() else {
        return CallVerdict::Rejected(vec![format!(
            "arguments must be a JSON object, got {}",
            json_type(args)
        )]);
    };
    let empty = serde_json::Map::new();
    let props = schema
        .get("properties")
        .and_then(|p| p.as_object())
        .unwrap_or(&empty);
    let mut problems = Vec::new();

    let declared: BTreeSet<&str> = props.keys().map(String::as_str).collect();
    for key in args.keys() {
        if !declared.contains(key.as_str()) {
            problems.push(format!(
                "unknown parameter `{key}` — accepted: {}",
                declared.iter().copied().collect::<Vec<&str>>().join(", ")
            ));
        }
    }

    if let Some(required) = schema.get("required").and_then(|r| r.as_array()) {
        for name in required.iter().filter_map(|r| r.as_str()) {
            if !args.contains_key(name) {
                problems.push(format!("missing required parameter `{name}`"));
            }
        }
    }

    for (key, value) in args {
        let Some(prop) = props.get(key) else {
            continue; // already reported as unknown
        };
        let types = declared_types(prop);
        if !types.is_empty() && !types.contains(json_type(value)) {
            problems.push(format!(
                "`{key}` is {} but the schema declares {}",
                json_type(value),
                types.iter().cloned().collect::<Vec<String>>().join(" or ")
            ));
            continue; // an enum check on a wrongly typed value adds noise, not information
        }
        let values = declared_enum(prop);
        if let Some(s) = value.as_str() {
            if !values.is_empty() && !values.iter().any(|v| v == s) {
                problems.push(format!(
                    "`{key}` = `{s}` is not one of {}",
                    values.join(", ")
                ));
            }
        }
    }

    for name in expect_keys {
        if !args.contains_key(name) {
            problems.push(format!(
                "`{name}` is missing, and the request cannot be answered without it"
            ));
        }
    }

    if problems.is_empty() {
        CallVerdict::Accepted
    } else {
        CallVerdict::Rejected(problems)
    }
}

/// Per-tool outcome of one SC-007 run, in the order the report table wants them.
#[derive(Debug, Clone, Serialize)]
pub struct SchemaTaskResult {
    pub tool: String,
    pub accepted: bool,
    /// The call the model proposed, kept verbatim so a failure can be read without re-running.
    pub call: serde_json::Value,
    pub reasons: Vec<String>,
}

/// Builds the per-tool table from a set of results, sorted by tool name.
pub fn summarize(results: &[SchemaTaskResult]) -> BTreeMap<String, bool> {
    results
        .iter()
        .map(|r| (r.tool.clone(), r.accepted))
        .collect()
}

/// The prompt the model answers: the tool's advertised schema and the request, nothing else.
///
/// Deliberately spare. Every sentence of guidance here is a sentence the tool's own schema did not
/// have to carry, and the measurement is about the schema.
pub fn build_call_prompt(task: &SchemaTask, schema: &serde_json::Value) -> String {
    format!(
        "# Tool\n\n{}\n\n# Its input schema\n\n```json\n{}\n```\n\n# Request\n\n{}\n\n\
         Respond with ONLY the JSON arguments object for one call to this tool. No prose, no \
         markdown fences, no tool name — just the object.",
        task.tool,
        serde_json::to_string_pretty(schema).unwrap_or_else(|_| schema.to_string()),
        task.request
    )
}

/// Pulls the arguments object out of a model response.
///
/// Models add fences and preambles despite instructions, and some answer with
/// `{"name": …, "arguments": {…}}` because that is what a tool call looks like in their training
/// data. Unwrapping that is not leniency about the schema — the arguments inside still face every
/// check — it is refusing to score a formatting habit as a schema failure.
pub fn extract_call(response: &str) -> Option<serde_json::Value> {
    let stripped = response.replace("```json", "").replace("```", "");
    let start = stripped.find('{')?;
    let end = stripped.rfind('}')?;
    if end < start {
        return None;
    }
    let value: serde_json::Value = serde_json::from_str(&stripped[start..=end]).ok()?;
    match value.get("arguments") {
        Some(args) if args.is_object() => Some(args.clone()),
        _ => Some(value),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_call_reads_a_bare_object() {
        let call = extract_call(r#"{"global": "^Foo"}"#).expect("must parse");
        assert_eq!(call["global"], "^Foo");
    }

    #[test]
    fn extract_call_strips_fences_and_preamble() {
        let call =
            extract_call("Sure!\n```json\n{\"global\": \"^Foo\"}\n```\n").expect("must parse");
        assert_eq!(call["global"], "^Foo");
    }

    #[test]
    fn extract_call_unwraps_a_tool_call_envelope() {
        let call = extract_call(r#"{"name": "global_preview", "arguments": {"global": "^Foo"}}"#)
            .expect("must parse");
        assert_eq!(call, serde_json::json!({"global": "^Foo"}));
    }

    #[test]
    fn extract_call_returns_none_on_prose() {
        assert!(extract_call("I would call global_preview with the global name.").is_none());
    }

    #[test]
    fn build_call_prompt_carries_the_schema_and_not_the_answer() {
        let task = SchemaTask {
            tool: "global_preview".to_string(),
            request: "Show me the first few entries of the audit global".to_string(),
            expect_keys: vec!["global".to_string()],
        };
        let schema = serde_json::json!({"properties": {"global": {"type": "string"}}});
        let prompt = build_call_prompt(&task, &schema);
        assert!(prompt.contains("global_preview"));
        assert!(prompt.contains("\"global\""));
        assert!(prompt.contains("audit global"));
    }
}

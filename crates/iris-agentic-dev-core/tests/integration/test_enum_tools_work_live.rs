//! E7, against `iris-dev-iris`: every read-only tool answers a call with real arguments.
//!
//! The suite had tests for the gates and for the wiring of each tool, and none that asked whether a
//! tool still works. Three tools failed on every HTTP call for months (`skill` list and search, `kb`
//! recall and `iris_debug` `source_map` built ObjectScript that does not compile) and nothing noticed.
//!
//! This test enumerates the tools. It takes every row of the classification table in
//! `write_gate.rs`, keeps the read-only ones (and, for a dispatcher, each read-only action, mode or
//! `what` the schema or the table names), and calls each with the arguments in [`cases`], which point
//! at things that exist on the server. An answer fails when it is an execution or compile failure
//! (`IRIS_EXECUTE_ERROR`, `EXECUTION_FAILED`, `COMPILE_FAILED`) or its text holds `not JSON`,
//! `<SYNTAX>`, `<UNDEFINED>` or `ERROR #5`, unless [`EXPECTED_ERRORS`] names that tool, variant and
//! code with a reason.
//!
//! A tool or variant with no case fails the test, and so does a case that is not read-only. A new
//! read-only tool is added to [`cases`] in the commit that adds it.
//!
//! Serial, `#[ignore]`: run with
//! `lockf -k /tmp/iad-141-iris.lock cargo test --features testing --test integration enum_tools_work -- --ignored --test-threads=1`.

use super::live_rig_171::{rig, Rig, Where};
use iris_agentic_dev_core::testing::ThrowawayIris;
use iris_agentic_dev_core::tools::write_gate::{classify, tool_class, WriteClass, CLASSIFICATION};
use iris_agentic_dev_core::tools::Toolset;
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};

/// Codes that say the tool's own ObjectScript or compile step failed.
const FORBIDDEN_CODES: &[&str] = &["IRIS_EXECUTE_ERROR", "EXECUTION_FAILED", "COMPILE_FAILED"];

/// Text that says IRIS rejected generated code, or that the answer was lost on the way back.
const FORBIDDEN_TEXT: &[&str] = &["not JSON", "<SYNTAX>", "<UNDEFINED>", "ERROR #5"];

/// `(tool, variant, code, reason)`: a forbidden code or text the call may still answer with.
/// `variant` is the call's `action`, `mode` or `what`, or `*` for every call of the tool. `code` is
/// a forbidden code, or `text` for a forbidden text. Each row says why the answer is right.
const EXPECTED_ERRORS: &[(&str, &str, &str, &str)] = &[];

struct Case {
    tool: &'static str,
    args: Value,
}

fn c(tool: &'static str, args: Value) -> Case {
    Case { tool, args }
}

/// Real arguments for every read-only call. Names that exist on a stock `iris-community` are used:
/// `%Library.String`, `%Dictionary.ClassDefinition`, the `USER` namespace, `_SYSTEM`.
fn cases() -> Vec<Case> {
    vec![
        c("agent_history", json!({})),
        c("agent_info", json!({"what": "stats"})),
        c("agent_info", json!({"what": "history"})),
        c("agent_stats", json!({})),
        c("capability_matrix", json!({})),
        c("check_config", json!({})),
        c(
            "compare_document",
            json!({"document": "%Library.String.cls", "namespace": "USER"}),
        ),
        c("compare_namespace", json!({"namespace": "USER"})),
        c("debug_capture_packet", json!({})),
        c("debug_get_error_logs", json!({})),
        c(
            "debug_map_int_to_cls",
            json!({"routine": "%Library.String.1", "offset": 1}),
        ),
        c("debug_source_map", json!({"cls_name": "%Library.String"})),
        c("docs_introspect", json!({"class_name": "%Library.String"})),
        c(
            "extract_message_map_routing",
            json!({"class_name": "%Library.String"}),
        ),
        c(
            "find_subclass_implementations",
            json!({"base_classes": ["%Library.DataType"], "method_name": "IsValid", "limit": 5}),
        ),
        c("global_preview", json!({"global": "^SKILLS", "count": 3})),
        c("hl7_schema_inspect", json!({})),
        c("hl7_schema_list", json!({})),
        c("iris_business_rule_info", json!({"action": "list"})),
        c(
            "iris_business_rule_info",
            json!({"action": "get", "rule_name": "No.Such.Rule"}),
        ),
        c("iris_credential_list", json!({})),
        c("iris_database_list", json!({})),
        c("iris_database_stats", json!({})),
        // `db` is a database directory, not a name.
        c(
            "iris_database_stats",
            json!({"db": "/usr/irissys/mgr/user/"}),
        ),
        c(
            "iris_debug",
            json!({"action": "map_int", "error_string": "<DIVIDE>IsValid+2^%Library.String.1"}),
        ),
        c("iris_debug", json!({"action": "error_logs"})),
        c("iris_debug", json!({"action": "capture"})),
        c(
            "iris_debug",
            json!({"action": "source_map", "class_name": "%Library.String"}),
        ),
        c("iris_doc_search", json!({"query": "ClassMethod"})),
        c("iris_get_log", json!({})),
        c("iris_list_containers", json!({})),
        c("iris_info", json!({"what": "documents"})),
        c("iris_info", json!({"what": "modified"})),
        c("iris_info", json!({"what": "namespace"})),
        c("iris_info", json!({"what": "metadata"})),
        c("iris_info", json!({"what": "jobs"})),
        c("iris_info", json!({"what": "csp_apps"})),
        c("iris_info", json!({"what": "csp_debug"})),
        c(
            "iris_info",
            json!({"what": "sa_schema", "name": "http://www.intersystems.com/deepsee"}),
        ),
        c("iris_interop_query", json!({"what": "logs"})),
        c("iris_interop_query", json!({"what": "queues"})),
        c("iris_interop_query", json!({"what": "messages"})),
        c("iris_macro", json!({"action": "list"})),
        c(
            "iris_macro",
            json!({"action": "signature", "name": "$$$OK"}),
        ),
        c("iris_macro", json!({"action": "location", "name": "$$$OK"})),
        c(
            "iris_macro",
            json!({"action": "definition", "name": "$$$OK"}),
        ),
        c("iris_macro", json!({"action": "expand", "name": "$$$OK"})),
        c("iris_message_body", json!({"message_id": "1"})),
        c("iris_mirror_status", json!({})),
        c("iris_namespace_list", json!({})),
        c("iris_production_diff", json!({})),
        c("iris_search", json!({"query": "IsValid", "limit": 5})),
        c("iris_servers", json!({})),
        c(
            "iris_symbols",
            json!({"query": "Library.String", "limit": 5}),
        ),
        c(
            "iris_symbols_local",
            json!({"query": "main", "workspace_path": "."}),
        ),
        c(
            "iris_table_info",
            json!({"table": "%Dictionary.ClassDefinition"}),
        ),
        c("iris_test_server", json!({"name": "no-such-server"})),
        c("iris_ws_open", json!({})),
        c("iris_ws_close", json!({"session": "$OPENED"})),
        c("journal_search", json!({"max_entries": 5})),
        c("kb_recall", json!({"query": "e7-no-such-term"})),
        c("mermaid_class", json!({"class": "%Library.String"})),
        c("mermaid_production", json!({})),
        c("my_access", json!({})),
        c("query_audit_log", json!({"limit": 3})),
        c(
            "resolve_dynamic_dispatch",
            json!({"method_name": "IsValid"}),
        ),
        c(
            "resolve_storage",
            json!({"class": "%Dictionary.ClassDefinition"}),
        ),
        c("skill_community_list", json!({})),
        c("skill_describe", json!({"name": "objectscript-guardrails"})),
        c("skill_list", json!({})),
        c("skill_search", json!({"query": "guardrails"})),
        c("stream_inspect", json!({})),
        c("telemetry_export_trace", json!({})),
        c("telemetry_query", json!({"limit": 3})),
        // iris_system_performance: the read modes.
        c("iris_system_performance", json!({"mode": "status"})),
        c("iris_system_performance", json!({"mode": "last_runid"})),
        c("iris_system_performance", json!({"mode": "list_profiles"})),
        c("iris_system_performance", json!({"mode": "list_runs"})),
        c(
            "iris_system_performance",
            json!({"mode": "report", "run_id": "no-such-run"}),
        ),
        // iris_admin: the read actions.
        c(
            "iris_admin",
            json!({"action": "check_permission", "username": "_SYSTEM", "resource": "%DB_USER", "permission": "R"}),
        ),
        c(
            "iris_admin",
            json!({"action": "database_status", "name": "USER"}),
        ),
        c(
            "iris_admin",
            json!({"action": "get_webapp", "name": "/api/atelier"}),
        ),
        c(
            "iris_admin",
            json!({"action": "journal_search", "max_records": 5}),
        ),
        c("iris_admin", json!({"action": "list_databases"})),
        c("iris_admin", json!({"action": "list_namespaces"})),
        c("iris_admin", json!({"action": "list_roles"})),
        c(
            "iris_admin",
            json!({"action": "list_user_roles", "username": "_SYSTEM"}),
        ),
        c("iris_admin", json!({"action": "list_users"})),
        c("iris_admin", json!({"action": "list_webapps"})),
        c(
            "iris_admin",
            json!({"action": "namespace_mappings", "namespace": "USER"}),
        ),
        c("iris_admin", json!({"action": "view_locks"})),
        c("iris_admin", json!({"action": "view_processes"})),
        c("iris_containers", json!({"action": "list"})),
        c("iris_coverage", json!({"mode": "report"})),
        c(
            "iris_credential_manage",
            json!({"action": "get", "id": "no-such-credential"}),
        ),
        c("iris_credential_manage", json!({"action": "list"})),
        c("iris_credential_manage", json!({"action": "list_tables"})),
        // iris_doc: the read modes.
        c(
            "iris_doc",
            json!({"mode": "get", "name": "%Library.String.cls"}),
        ),
        c(
            "iris_doc",
            json!({"mode": "head", "name": "%Library.String.cls"}),
        ),
        c(
            "iris_doc",
            json!({"mode": "fragment", "name": "%Library.String.cls", "start": 1, "end": 5}),
        ),
        c(
            "iris_doc",
            json!({"mode": "compiled", "name": "%Library.String.cls"}),
        ),
        c(
            "iris_doc",
            json!({"mode": "list", "pattern": "%Library.Str*.cls"}),
        ),
        c(
            "iris_global",
            json!({"action": "get", "global_name": "^SKILLS"}),
        ),
        c(
            "iris_global",
            json!({"action": "list", "global_name": "^SKILLS"}),
        ),
        c(
            "iris_lookup_manage",
            json!({"action": "get", "table": "No.Such.Table", "key": "k"}),
        ),
        c(
            "iris_lookup_manage",
            json!({"action": "list_keys", "table": "No.Such.Table"}),
        ),
        c("iris_lookup_manage", json!({"action": "list_tables"})),
        c(
            "iris_lookup_manage",
            json!({"action": "export", "table": "No.Such.Table"}),
        ),
        c(
            "iris_lookup_transfer",
            json!({"action": "export", "table": "No.Such.Table"}),
        ),
        c("iris_production", json!({"action": "check"})),
        c("iris_production", json!({"action": "status"})),
        c("iris_production", json!({"action": "get_autostart"})),
        c(
            "iris_query",
            json!({"mode": "read", "query": "SELECT 1 AS One"}),
        ),
        c(
            "iris_query",
            json!({"mode": "explain", "query": "SELECT 1 AS One"}),
        ),
        c(
            "iris_query",
            json!({"mode": "count", "table": "%Dictionary.ClassDefinition"}),
        ),
        c(
            "iris_source_control",
            json!({"action": "status", "document": "%Library.String.cls"}),
        ),
        c(
            "iris_source_control",
            json!({"action": "menu", "document": "%Library.String.cls"}),
        ),
        c(
            "kb",
            json!({"action": "recall", "query": "e7-no-such-term"}),
        ),
        c("kb", json!({"action": "history"})),
        c("kb", json!({"action": "stats"})),
        c("skill", json!({"action": "list"})),
        c(
            "skill",
            json!({"action": "describe", "name": "objectscript-guardrails"}),
        ),
        c("skill", json!({"action": "search", "query": "guardrails"})),
        c("skill_community", json!({"action": "list"})),
    ]
}

/// The `action`, `mode` or `what` a call carries (lower-cased), or `""`.
fn variant_of(args: &Value) -> String {
    for key in ["action", "mode", "what"] {
        if let Some(v) = args.get(key).and_then(Value::as_str) {
            return v.trim().to_lowercase();
        }
    }
    String::new()
}

fn as_map(args: &Value) -> serde_json::Map<String, Value> {
    args.as_object().cloned().unwrap_or_default()
}

/// The enum values the schema gives to property `key`, if it has an enum.
fn enum_values(schema: &Value, key: &str) -> Vec<String> {
    let Some(p) = schema.get("properties").and_then(|p| p.get(key)) else {
        return vec![];
    };
    let mut found = vec![];
    let mut take = |v: &Value| {
        if let Some(a) = v.get("enum").and_then(Value::as_array) {
            found.extend(a.iter().filter_map(|x| x.as_str().map(str::to_lowercase)));
        }
    };
    take(p);
    if let Some(any) = p.get("anyOf").and_then(Value::as_array) {
        any.iter().for_each(&mut take);
    }
    found
}

/// Every variant a tool needs a case for: the values of its `action`, `mode` and `what` enums that
/// the classification reads as read-only, the read-only actions the table names, and the bare call
/// when it is read-only and the tool has no enum to choose from.
fn required_variants(tool: &str, schema: &Value) -> BTreeSet<String> {
    let mut need = BTreeSet::new();
    let mut had_enum = false;
    for key in ["action", "mode", "what"] {
        for v in enum_values(schema, key) {
            had_enum = true;
            let args = json!({ key: v });
            if classify(tool, Some(&as_map(&args))) == Some(WriteClass::ReadOnly) {
                need.insert(v);
            }
        }
    }
    if let Some(row) = tool_class(tool) {
        for (a, class) in row.actions {
            if *class == WriteClass::ReadOnly {
                need.insert(a.to_string());
            }
        }
    }
    if !had_enum && classify(tool, Some(&as_map(&json!({})))) == Some(WriteClass::ReadOnly) {
        need.insert(String::new());
    }
    need
}

fn allowed(tool: &str, variant: &str, code: &str) -> Option<&'static str> {
    EXPECTED_ERRORS
        .iter()
        .find(|(t, v, k, _)| *t == tool && (*v == "*" || *v == variant) && *k == code)
        .map(|(_, _, _, why)| *why)
}

/// What is wrong with an answer, if anything.
fn flaws(tool: &str, variant: &str, answer: &Value, text: &str) -> Vec<String> {
    let mut out = vec![];
    if let Some(code) = answer.get("error_code").and_then(Value::as_str) {
        if FORBIDDEN_CODES.contains(&code) && allowed(tool, variant, code).is_none() {
            out.push(format!("error_code {code}"));
        }
    }
    for t in FORBIDDEN_TEXT {
        if text.contains(t) && allowed(tool, variant, "text").is_none() {
            out.push(format!("text holds `{t}`"));
        }
    }
    out
}

/// The answer a call gave, as JSON when it is, and the text it carried.
async fn ask(r: &Rig, tool: &str, args: Value) -> (Value, String) {
    match r.tools.call_gated(tool, args).await {
        Ok(res) => {
            let text = res
                .content
                .iter()
                .filter_map(|c| c.as_text().map(|t| t.text.clone()))
                .collect::<Vec<_>>()
                .join("\n");
            let json = res
                .structured_content
                .clone()
                .or_else(|| serde_json::from_str(&text).ok())
                .unwrap_or(Value::Null);
            let text = if text.is_empty() {
                json.to_string()
            } else {
                text
            };
            (json, text)
        }
        Err(e) => (json!({"error": e.to_string()}), e.to_string()),
    }
}

async fn sweep(at: &Where) -> usize {
    let merged = rig(at, Toolset::Merged).await;
    let baseline = rig(at, Toolset::Baseline).await;
    let in_merged = merged.tools.registered_tool_names();
    let in_baseline = baseline.tools.registered_tool_names();
    let mut schemas: BTreeMap<String, Value> = BTreeMap::new();
    for e in baseline
        .tools
        .tool_catalogue()
        .into_iter()
        .chain(merged.tools.tool_catalogue())
    {
        schemas.insert(e.name, e.input_schema);
    }
    let registered: BTreeSet<String> = in_merged.union(&in_baseline).cloned().collect();

    let mut problems: Vec<String> = vec![];
    let all_cases = cases();

    // Every registered tool has a classification row, and the table names no tool twice.
    for name in &registered {
        if tool_class(name).is_none() {
            problems.push(format!("{name}: registered with no classification row"));
        }
    }
    let rows: BTreeSet<&str> = CLASSIFICATION.iter().map(|r| r.tool).collect();
    assert_eq!(rows.len(), CLASSIFICATION.len(), "a tool has two rows");

    // A case is read-only, names a registered tool, and is not a stray.
    for k in &all_cases {
        if !registered.contains(k.tool) {
            problems.push(format!(
                "{}: a case for a tool that is not registered",
                k.tool
            ));
        }
        if classify(k.tool, Some(&as_map(&k.args))) != Some(WriteClass::ReadOnly) {
            problems.push(format!(
                "{}: the case {} is not read-only; the sweep never calls a write",
                k.tool, k.args
            ));
        }
    }

    // Every read-only tool and variant has a case.
    for name in &registered {
        let Some(schema) = schemas.get(name) else {
            continue;
        };
        for variant in required_variants(name, schema) {
            let covered = all_cases
                .iter()
                .any(|k| k.tool == name && variant_of(&k.args) == variant);
            if !covered {
                problems.push(format!(
                    "{name}: no case for read-only variant {:?}; add one to cases()",
                    variant
                ));
            }
        }
    }

    // Call each case where its tool is registered (the default toolset first).
    let mut called = 0;
    let mut opened: Option<String> = None;
    for k in &all_cases {
        if !registered.contains(k.tool)
            || classify(k.tool, Some(&as_map(&k.args))) != Some(WriteClass::ReadOnly)
        {
            continue;
        }
        let r = if in_merged.contains(k.tool) {
            &merged
        } else {
            &baseline
        };
        let mut args = k.args.clone();
        if args.get("session").and_then(Value::as_str) == Some("$OPENED") {
            match &opened {
                Some(s) => args["session"] = json!(s),
                None => {
                    problems.push(format!("{}: iris_ws_open gave no session to close", k.tool));
                    continue;
                }
            }
        }
        let (answer, text) = ask(r, k.tool, args.clone()).await;
        called += 1;
        if k.tool == "iris_ws_open" {
            opened = answer
                .get("session")
                .or_else(|| answer.get("session_id"))
                .and_then(Value::as_str)
                .map(str::to_string);
        }
        let variant = variant_of(&args);
        for f in flaws(k.tool, &variant, &answer, &text) {
            problems.push(format!(
                "{} {}: {f}: {}",
                k.tool,
                args,
                text.chars().take(300).collect::<String>()
            ));
        }
    }
    assert!(
        problems.is_empty(),
        "{} problem(s) in the sweep of {called} calls:\n{}",
        problems.len(),
        problems.join("\n")
    );
    called
}

#[tokio::test]
#[ignore = "requires live IRIS (iris-dev-iris)"]
async fn every_read_only_tool_works_on_iris_dev_iris() {
    let n = sweep(&Where::live()).await;
    assert!(n >= 100, "the sweep made only {n} calls");
}

/// The same sweep on the throwaway container: 2026.2, and the version `IAD_THROWAWAY_IMAGE` names.
#[tokio::test]
#[ignore = "requires Docker (starts the throwaway IRIS)"]
async fn every_read_only_tool_works_on_the_throwaway() {
    let Some(t) = ThrowawayIris::start() else {
        return;
    };
    let n = sweep(&Where::throwaway(&t)).await;
    assert!(n >= 100, "the sweep made only {n} calls");
}

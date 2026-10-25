//! Spec 141 R1 D12: the guards that keep the pipeline the only place a target is resolved.
//!
//! - no handler resolves its own connection or gates;
//! - every row of `CLASSIFICATION` names where the target comes from, and the column agrees with
//!   the schema the tool advertises;
//! - an accessor read outside a pipeline scope fails closed;
//! - `force` is judged by what it runs, and `force_writable` needs two categories.

use crate::support_gate_141::*;
use iris_agentic_dev_core::iris::workspace_config::ToolCategory;
use iris_agentic_dev_core::tools::target::ERR_TARGET_NOT_RESOLVED;
use iris_agentic_dev_core::tools::write_gate::{
    effective_category, TargetKind, TargetSource, WriteClass, CLASSIFICATION,
};
use iris_agentic_dev_core::tools::{IrisTools, Toolset};
use serde_json::json;

/// The body of every `#[tool]` method under `src/tools`. rustfmt puts methods at four spaces, so a
/// body ends at the first line that is exactly `    }`.
fn tool_bodies() -> Vec<(String, String, String)> {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/tools");
    let mut out = Vec::new();
    for entry in std::fs::read_dir(&root).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().and_then(|e| e.to_str()) != Some("rs") {
            continue;
        }
        let text = std::fs::read_to_string(&path).unwrap();
        let lines: Vec<&str> = text.lines().collect();
        let mut i = 0;
        while i < lines.len() {
            if lines[i].starts_with("    #[tool(") || lines[i] == "    #[tool]" {
                let mut j = i;
                while j < lines.len()
                    && !lines[j].starts_with("    async fn ")
                    && !lines[j].starts_with("    fn ")
                {
                    j += 1;
                }
                let name = lines[j]
                    .trim_start()
                    .trim_start_matches("async ")
                    .trim_start_matches("fn ")
                    .split('(')
                    .next()
                    .unwrap_or("")
                    .to_string();
                let mut k = j;
                while k < lines.len() && lines[k] != "    }" {
                    k += 1;
                }
                out.push((
                    path.file_name().unwrap().to_string_lossy().to_string(),
                    name,
                    lines[j..=k.min(lines.len() - 1)].join("\n"),
                ));
                i = k;
            }
            i += 1;
        }
    }
    out
}

#[test]
fn the_scan_finds_the_tool_methods() {
    let bodies = tool_bodies();
    assert!(
        bodies.len() > 60,
        "the scan found {} #[tool] methods; it is reading the wrong thing",
        bodies.len()
    );
    assert!(bodies.iter().any(|(_, n, _)| n == "iris_execute"));
}

/// Every `#[tool]` body takes its target from the pipeline accessors.
#[test]
fn no_tool_body_resolves_its_own_target_or_runs_its_own_gate() {
    const FORBIDDEN: &[&str] = &[
        "resolve_server(",
        "get_iris_reloaded(",
        "get_iris_for_exec_with_client(",
        "iris_arc(",
        "connection.lock().unwrap().iris",
        "pool_ref().get(",
        "dispatch_gate(",
        "policy_gate(",
        "active_server_manager_policy(",
        "instance_role(",
        "policy_refusal(",
    ];
    let mut found = Vec::new();
    for (file, name, body) in tool_bodies() {
        for f in FORBIDDEN {
            if body.contains(f) {
                found.push(format!("{file}: {name} uses {f}"));
            }
        }
    }
    assert!(
        found.is_empty(),
        "a handler that picks its own server is the defect spec 141 removes:\n{}",
        found.join("\n")
    );
}

fn schema_props(tools: &IrisTools, tool: &str) -> Vec<String> {
    tools
        .tool_input_schema(tool)
        .and_then(|s| s["properties"].as_object().cloned())
        .map(|m| m.keys().cloned().collect())
        .unwrap_or_default()
}

#[test]
fn every_classification_row_names_a_target_that_agrees_with_the_advertised_schema() {
    for toolset in [Toolset::Baseline, Toolset::Nostub, Toolset::Merged] {
        let rig = rig_with("", toolset, &[]);
        let names = rig.tools.registered_tool_names();
        let mut checked = 0;
        for row in CLASSIFICATION.iter().filter(|r| names.contains(r.tool)) {
            let props = schema_props(&rig.tools, row.tool);
            let has = |p: &str| props.iter().any(|x| x == p);
            match row.target {
                TargetSource::Arg(a) => assert!(has(a), "{} names Arg({a}) but advertises {props:?}", row.tool),
                TargetSource::ArgOrPending(a) => {
                    assert!(has(a), "{} advertises {props:?}", row.tool);
                    assert!(has("elicitation_id"), "{} resumes by elicitation_id", row.tool);
                }
                TargetSource::Args(list) => {
                    for a in list {
                        assert!(has(a), "{} names {a} but advertises {props:?}", row.tool);
                    }
                }
                TargetSource::Name(n) => assert!(has(n), "{} advertises {props:?}", row.tool),
                TargetSource::Session => assert!(has("session"), "{} advertises {props:?}", row.tool),
                TargetSource::Default => assert!(
                    !has("server"),
                    "{} takes a `server` argument but its row says Default, so the argument would be ignored by the gates",
                    row.tool
                ),
            }
            checked += 1;
        }
        assert!(checked > 50, "{toolset:?}: only {checked} rows checked");
    }
}

#[test]
fn a_local_tool_is_one_that_does_not_act_on_an_iris_servers_data() {
    // The set is fixed on purpose: a tool joins it only by a decision, not by a default.
    const LOCAL: &[&str] = &[
        "agent_history",
        "agent_info",
        "agent_stats",
        "check_config",
        "iris_add_server",
        "iris_containers",
        "iris_doc_search",
        "iris_import_servers",
        "iris_list_containers",
        "iris_reload_pool",
        "iris_remove_server",
        "iris_select_container",
        "iris_servers",
        "iris_start_sandbox",
        "iris_symbols_local",
        "kb",
        "kb_index",
        "kb_recall",
        "skill",
        "skill_community",
        "skill_community_install",
        "skill_community_list",
        "skill_describe",
        "skill_forget",
        "skill_list",
        "skill_optimize",
        "skill_propose",
        "skill_search",
        "skill_share",
    ];
    let mut local: Vec<&str> = CLASSIFICATION
        .iter()
        .filter(|r| r.kind == TargetKind::Local)
        .map(|r| r.tool)
        .collect();
    local.sort();
    assert_eq!(local, LOCAL);
}

#[test]
fn accessors_read_outside_a_pipeline_scope_fail_closed() {
    let rig = rig("");
    let t = &rig.tools;
    let err = |e: rmcp::ErrorData| e.message.to_string();
    assert!(err(t.target().err().unwrap()).contains(ERR_TARGET_NOT_RESOLVED));
    assert!(err(t.target_conn().err().unwrap()).contains(ERR_TARGET_NOT_RESOLVED));
    assert!(err(t.target_exec().err().unwrap()).contains(ERR_TARGET_NOT_RESOLVED));
    assert!(err(t.target_ns(Some("USER")).err().unwrap()).contains(ERR_TARGET_NOT_RESOLVED));
}

#[tokio::test]
async fn accessors_read_inside_a_call_return_the_calls_target() {
    // `check_config` is local-kind and reads the default; any tool that reaches its handler has a
    // scope. `iris_get_log` is cheap and local to the process.
    let rig = rig("");
    let r = rig
        .tools
        .call_gated("check_config", json!({}))
        .await
        .unwrap();
    let body = body_of(&r);
    assert_eq!(body["connected"], json!(true), "{body}");
}

// ── T-R4a-U22 ───────────────────────────────────────────────────────────────────────────────────

fn cats(tool: &str, args: serde_json::Value, class: WriteClass) -> Vec<ToolCategory> {
    effective_category(tool, args.as_object(), class)
}

#[test]
fn effective_category_moves_an_action_to_the_category_of_what_it_runs() {
    use ToolCategory::*;
    assert_eq!(
        cats(
            "iris_global",
            json!({"action": "get"}),
            WriteClass::ReadOnly
        ),
        vec![Query]
    );
    assert_eq!(
        cats("iris_global", json!({"action": "set"}), WriteClass::Write),
        vec![Execute]
    );
    assert_eq!(
        cats(
            "iris_global",
            json!({"action": "kill"}),
            WriteClass::Destructive
        ),
        vec![Execute]
    );
    assert_eq!(
        cats("global_kill", json!({}), WriteClass::Destructive),
        vec![Execute]
    );
    assert_eq!(
        cats("iris_query", json!({"mode": "read"}), WriteClass::ReadOnly),
        vec![Query]
    );
    assert_eq!(
        cats("iris_query", json!({"mode": "write"}), WriteClass::Write),
        vec![Execute]
    );
    // `force` skips the read-only SQL check, so a forced read runs what a write runs.
    assert_eq!(
        cats(
            "iris_query",
            json!({"mode": "read", "force": true}),
            WriteClass::ReadOnly
        ),
        vec![Execute]
    );
    assert_eq!(
        cats("iris_doc", json!({"mode": "get"}), WriteClass::ReadOnly),
        vec![Docs]
    );
    assert_eq!(
        cats("iris_doc", json!({"mode": "put"}), WriteClass::Write),
        vec![Compile]
    );
    // A resume carries no mode; the pending entry's class decides.
    assert_eq!(
        cats(
            "iris_doc",
            json!({"elicitation_id": "x"}),
            WriteClass::Write
        ),
        vec![Compile]
    );
    assert_eq!(
        cats(
            "iris_system_performance",
            json!({"action": "start"}),
            WriteClass::Write
        ),
        vec![Admin]
    );
    assert_eq!(
        cats("iris_generate_test", json!({}), WriteClass::Write),
        vec![Admin]
    );
    assert_eq!(
        cats("check_config", json!({}), WriteClass::ReadOnly),
        Vec::<ToolCategory>::new()
    );
}

#[test]
fn force_writable_needs_both_compile_and_admin() {
    use ToolCategory::*;
    assert_eq!(
        cats(
            "iris_compile",
            json!({"target": "A.cls"}),
            WriteClass::Write
        ),
        vec![Compile]
    );
    assert_eq!(
        cats(
            "iris_compile",
            json!({"target": "A.cls", "force_writable": true}),
            WriteClass::Write
        ),
        vec![Compile, Admin]
    );
}

#[tokio::test]
async fn force_writable_with_allow_compile_alone_is_refused_naming_admin() {
    let rig = rig("[policy.default]\nallow = [\"compile\"]\n");
    let r = rig
        .tools
        .call_gated(
            "iris_compile",
            json!({"target": "Sentinel141.cls", "force_writable": true}),
        )
        .await
        .unwrap();
    let body = body_of(&r);
    assert_eq!(code_of(&body), "POLICY_GATE", "{body}");
    assert_eq!(body["blocked_category"], json!("admin"), "{body}");
}

#[tokio::test]
async fn query_force_without_writes_is_refused_by_the_write_gate_in_both_server_spellings() {
    // `force` runs what a write runs; with writes off on the target it is refused by the write gate
    // however the server is spelled.
    let rig = rig_with(
        "write_tools_enabled = false\n",
        Toolset::Merged,
        &[("prod", UNROUTABLE)],
    );
    for server in [None, Some("prod"), Some(" prod ")] {
        let mut args = json!({"query": "SELECT 1", "mode": "read", "force": true});
        if let Some(s) = server {
            args["server"] = json!(s);
        }
        let r = rig.tools.call_gated("iris_query", args).await.unwrap();
        let body = body_of(&r);
        assert_eq!(
            code_of(&body),
            "WRITE_TOOLS_DISABLED",
            "server={server:?}: a forced read is a write to the gate. Body: {body}"
        );
    }
}

//! Spec 141 T219: `iris_coverage` `check` and `iris_reload_pool` change state, so they are writes.
//!
//! `check` starts the line monitor to see whether it can start, and stops it again when none was
//! running; `iris_reload_pool` swaps the connection pool for one built from the files on disk,
//! which activates hosts nobody had approved a call against. Both were classed read-only, so the
//! write gate, `read_only = "strict"` and a Live instance's closed gate let them through.

use crate::support_gate_141::*;
use iris_agentic_dev_core::iris::connection::SystemMode;
use iris_agentic_dev_core::iris::workspace_config::{McpTemplate, ToolCategory};
use iris_agentic_dev_core::testing::gated;
use iris_agentic_dev_core::tools::write_gate::{
    classify, gate_check, resolve_declared, tool_class, OperatorEnvGates, WriteClass,
    ERR_WRITE_GATE,
};
use serde_json::{json, Value};

fn args(v: Value) -> serde_json::Map<String, Value> {
    v.as_object().expect("an object").clone()
}

// ── the table ───────────────────────────────────────────────────────────────────────────────────

#[test]
fn coverage_check_is_a_write_in_every_spelling_and_report_still_reads() {
    for (key, value) in [
        ("mode", "check"),
        ("mode", "CHECK"),
        ("mode", " check "),
        ("action", "check"),
        ("action", "Check"),
    ] {
        let a = args(json!({ key: value }));
        assert_eq!(
            classify("iris_coverage", Some(&a)),
            Some(WriteClass::Write),
            "{key}={value:?}"
        );
    }
    // The control: report only reads the monitor's counters back, and says so.
    for key in ["mode", "action"] {
        let a = args(json!({ key: "report" }));
        assert_eq!(
            classify("iris_coverage", Some(&a)),
            Some(WriteClass::ReadOnly),
            "{key}=report"
        );
    }
    // stop, start and run stay writes, and so does a call with no mode at all.
    for mode in ["stop", "start", "run"] {
        let a = args(json!({ "mode": mode }));
        assert_eq!(
            classify("iris_coverage", Some(&a)),
            Some(WriteClass::Write),
            "mode={mode}"
        );
    }
    assert_eq!(classify("iris_coverage", None), Some(WriteClass::Write));
}

#[test]
fn reload_pool_is_a_write_that_acts_on_the_default_connection() {
    assert_eq!(classify("iris_reload_pool", None), Some(WriteClass::Write));
    let row = tool_class("iris_reload_pool").expect("a row");
    assert_eq!(row.default, WriteClass::Write);
    // It has no server parameter and touches no IRIS data: the gate that judges it is the default
    // connection's, the one the pool is rebuilt around.
    assert!(
        !iris_agentic_dev_core::tools::write_gate::is_scratch("iris_reload_pool"),
        "reloading the pool compiles nothing on a server"
    );
}

// ── the gate of each kind of instance ───────────────────────────────────────────────────────────

#[test]
fn a_live_instance_refuses_both_and_a_development_instance_allows_both() {
    let calls = [
        ("iris_coverage", Some(args(json!({"mode": "check"})))),
        ("iris_coverage", Some(args(json!({"action": "check"})))),
        ("iris_reload_pool", None),
    ];
    for (mode, expect_refusal) in [
        (SystemMode::Live, true),
        (SystemMode::Test, false),
        (SystemMode::Development, false),
    ] {
        let gates = resolve_declared(
            &OperatorEnvGates::default(),
            Default::default(),
            mode,
            "USER",
        );
        for (tool, a) in &calls {
            let refused = gate_check(tool, a.as_ref(), &gates).is_some();
            // Test and Development open the write gate by inference; Live closes it.
            assert_eq!(
                refused, expect_refusal,
                "{tool} {a:?} on {mode:?}: {gates:?}"
            );
        }
    }
}

// ── the environment template ────────────────────────────────────────────────────────────────────

#[test]
fn coverage_check_is_blocked_by_the_live_and_test_templates_and_open_in_dev() {
    // `check` was inert by classification but Execute by category, so the templates already
    // blocked it; this pins that the reclassification did not move it somewhere they do not look.
    for key in ["mode", "action"] {
        let params = json!({ key: "check" });
        for template in [McpTemplate::Live, McpTemplate::Test] {
            let blocked = gated::env("iris_coverage", &template, "prod", &params)
                .unwrap_or_else(|| panic!("{key}: {template:?} must block coverage check"));
            assert_eq!(blocked["error_code"], "ENV_GATE_BLOCKED", "{blocked}");
            assert_eq!(blocked["blocked_category"], "execute", "{blocked}");
        }
        assert!(
            gated::env("iris_coverage", &McpTemplate::Dev, "dev", &params).is_none(),
            "{key}: dev blocks nothing"
        );
    }
}

#[test]
fn reload_pool_is_an_admin_call_so_a_query_only_allow_list_no_longer_admits_it() {
    use iris_agentic_dev_core::iris::workspace_config::ConnectionPolicy;
    // It was a Query tool, which `allow = ["query"]` (a read-only browsing connection) admits.
    let policy = ConnectionPolicy {
        allow: Some(vec![ToolCategory::Query]),
        ..Default::default()
    };
    let blocked = gated::policy("iris_reload_pool", "dev", Some(&policy))
        .expect("a query-only connection must not reload the pool");
    assert_eq!(blocked["blocked_category"], "admin", "{blocked}");

    let policy = ConnectionPolicy {
        allow: Some(vec![ToolCategory::Admin]),
        ..Default::default()
    };
    assert!(
        gated::policy("iris_reload_pool", "dev", Some(&policy)).is_none(),
        "the control: an allow list naming admin admits it"
    );
}

// ── through the pipeline ────────────────────────────────────────────────────────────────────────

/// Writes off on the default connection, nothing listening anywhere: a call that got past the
/// gate would fail with a connection error, which is never the answer these tests accept.
const WRITES_OFF: &str = "write_tools_enabled = false\n";

fn assert_write_gate(label: &str, body: &Value) {
    assert_eq!(
        code_of(body),
        ERR_WRITE_GATE,
        "{label}: expected the write gate's refusal. Body: {body}"
    );
    let text = body.to_string();
    assert!(
        !text.contains("error sending request") && code_of(body) != "IRIS_UNREACHABLE",
        "{label}: the call reached the network: {body}"
    );
}

#[tokio::test]
async fn coverage_check_and_reload_pool_meet_the_write_gate_in_every_spelling() {
    let rig = rig(WRITES_OFF);
    for (key, value) in [
        ("mode", "check"),
        ("mode", "CHECK"),
        ("mode", " check "),
        ("action", "check"),
    ] {
        let r = rig
            .tools
            .call_gated("iris_coverage", json!({ key: value }))
            .await
            .unwrap();
        assert_write_gate(&format!("{key}={value:?}"), &body_of(&r));
    }
    for server in ["dev", " dev "] {
        let r = rig
            .tools
            .call_gated("iris_coverage", json!({"mode": "check", "server": server}))
            .await
            .unwrap();
        assert_write_gate(&format!("server={server:?}"), &body_of(&r));
    }
    let r = rig
        .tools
        .call_gated("iris_reload_pool", json!({}))
        .await
        .unwrap();
    assert_write_gate("iris_reload_pool", &body_of(&r));
}

#[tokio::test]
async fn read_only_strict_refuses_coverage_check_and_reload_pool() {
    let rig = rig("read_only = \"strict\"\nwrite_tools_enabled = true\n");
    for (tool, params) in [
        ("iris_coverage", json!({"mode": "check"})),
        ("iris_coverage", json!({"action": "check"})),
        ("iris_reload_pool", json!({})),
    ] {
        let r = rig.tools.call_gated(tool, params.clone()).await.unwrap();
        let b = body_of(&r);
        assert_eq!(code_of(&b), "READ_ONLY_STRICT", "{tool} {params}: {b}");
    }
}

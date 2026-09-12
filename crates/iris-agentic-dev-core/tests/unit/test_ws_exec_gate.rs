//! `iris_ws_exec` and the code-edit hard-block (119, issue #137).
//!
//! `iris_ws_exec` runs arbitrary ObjectScript in a persistent terminal session and reached
//! `WsSessionPool::exec` without consulting `dispatch_gate` at all. Step `[0]` also branched on
//! three literal tool names, so even a handler that called the gate would have been waved through.
//!
//! Two things are asserted here. The verdicts, which are the fix. And the declaration the fix reads
//! — `CODE_EXEC_TOOLS` — checked against every tool in the router that takes a `code` parameter, so
//! the next arbitrary-execution tool cannot ship missing from it the way this one did.

use iris_agentic_dev_core::iris::workspace_config::{ConnectionPolicy, DataPolicy, McpTemplate};
use iris_agentic_dev_core::policy::gate::{dispatch_gate, CODE_EXEC_TOOLS};
use iris_agentic_dev_core::testing::{params_type, struct_fields};
use iris_agentic_dev_core::tools::IrisTools;

fn policy_dev() -> ConnectionPolicy {
    ConnectionPolicy {
        server_name: "iris-dev".to_string(),
        allow: None,
        mcp_template: Some(McpTemplate::Dev),
        data_policy: Some(DataPolicy::Allow),
        global_blocklist: vec![],
        data_policy_kill_allowlist: vec![],
        iris_audit: false,
    }
}

fn ws_exec(code: &str) -> Result<(), serde_json::Value> {
    let policy = policy_dev();
    dispatch_gate(
        "iris_ws_exec",
        "iris-dev",
        Some(&policy),
        &serde_json::json!({ "session": "ws-1", "code": code }),
    )
}

fn error_code(err: &serde_json::Value) -> &str {
    err.get("error_code")
        .and_then(|c| c.as_str())
        .unwrap_or_else(|| panic!("gate error carries no error_code: {err}"))
}

/// Payloads that must be refused, whichever arbitrary-execution tool carries them.
///
/// One per branch of `check_objectscript_code_edit`: the code-management API tokens, an editable
/// `%Dictionary.*Definition` class, and a direct write to a code-storage global.
const CODE_EDIT_PAYLOADS: &[(&str, &str)] = &[
    ("$system.OBJ delete", "Do $system.OBJ.Delete(\"MyApp.Foo\")"),
    (
        "$system.OBJ compile",
        "Do $system.OBJ.Compile(\"MyApp.Foo\",\"ck\")",
    ),
    (
        "class definition",
        "Set c=##class(%Dictionary.ClassDefinition).%OpenId(\"MyApp.Foo\")",
    ),
    (
        "routine manager",
        "Do ##class(%RoutineMgr).Delete(\"x.mac\")",
    ),
    ("code-storage global", "Set ^oddDEF(\"MyApp.Foo\",1)=\"\""),
];

/// FR-001: every payload `iris_execute` refuses, `iris_ws_exec` refuses too.
#[test]
fn ws_exec_is_blocked_on_the_code_edit_surface() {
    for (label, code) in CODE_EDIT_PAYLOADS {
        let err = ws_exec(code).expect_err(&format!(
            "iris_ws_exec must refuse {label}: `{code}` reaches the editable-code surface"
        ));
        assert_eq!(
            error_code(&err),
            "CODE_EDIT_BLOCKED",
            "wrong error code for {label}: {err}"
        );
    }
}

/// FR-001, stated as parity rather than as a second copy of the expected verdicts.
///
/// The two tools share one gate, so their answers have to agree payload for payload. Asserting the
/// agreement catches a future branch that gates one and not the other, which a per-tool assertion
/// would not.
#[test]
fn ws_exec_and_execute_return_the_same_verdict() {
    let policy = policy_dev();
    for (label, code) in CODE_EDIT_PAYLOADS.iter().chain(BENIGN_PAYLOADS.iter()) {
        let execute = dispatch_gate(
            "iris_execute",
            "iris-dev",
            Some(&policy),
            &serde_json::json!({ "code": code }),
        );
        let ws = ws_exec(code);
        assert_eq!(
            execute.is_err(),
            ws.is_err(),
            "iris_execute and iris_ws_exec disagree on {label} (`{code}`): \
             execute={execute:?} ws_exec={ws:?}"
        );
    }
}

/// ObjectScript that touches no code and must keep working. A gate that blocks these is a gate
/// nobody can leave on.
const BENIGN_PAYLOADS: &[(&str, &str)] = &[
    ("version", "write $zversion,!"),
    ("local set", "Set x=1 Write x,!"),
    ("ordinary global", "Set ^MyApp.Counter=1"),
    (
        "compiled introspection",
        "Set c=##class(%Dictionary.CompiledClass).%OpenId(\"MyApp.Foo\")",
    ),
];

/// FR-002.
#[test]
fn ws_exec_permits_ordinary_objectscript() {
    for (label, code) in BENIGN_PAYLOADS {
        assert!(
            ws_exec(code).is_ok(),
            "iris_ws_exec must permit {label}: `{code}` edits no code"
        );
    }
}

/// FR-008 as far as a unit test can state it: the gate reads the `code` parameter and nothing else,
/// so it can answer before a session token means anything. The token here is not a real session.
#[test]
fn ws_exec_is_gated_without_a_valid_session() {
    let policy = policy_dev();
    let err = dispatch_gate(
        "iris_ws_exec",
        "iris-dev",
        Some(&policy),
        &serde_json::json!({ "session": "not-a-session", "code": "Do $system.OBJ.Delete(\"X\")" }),
    )
    .expect_err("the gate must not need a live session to refuse");
    assert_eq!(error_code(&err), "CODE_EDIT_BLOCKED");
}

/// FR-003.
#[test]
fn code_exec_tools_declares_both_arbitrary_execution_tools() {
    assert!(
        CODE_EXEC_TOOLS.contains(&"iris_execute"),
        "iris_execute must be declared: {CODE_EXEC_TOOLS:?}"
    );
    assert!(
        CODE_EXEC_TOOLS.contains(&"iris_ws_exec"),
        "iris_ws_exec must be declared — this is issue #137: {CODE_EXEC_TOOLS:?}"
    );
}

/// FR-004: the guard.
///
/// Any tool whose params struct has a `code` field takes caller-supplied ObjectScript, and step
/// `[0]` reads `CODE_EXEC_TOOLS` to decide whether to scan it. A tool with the field and no
/// declaration is the #137 bug, and this fails on it at the next `cargo test` rather than at the
/// next report.
///
/// The tool list comes from `IrisTools::registered_tool_names()` — the router's own registry, built
/// by the same `#[tool_router]` expansion that dispatches calls. Checking the constant against a
/// list written in this file would prove only that I typed the same thing twice.
#[test]
fn every_tool_with_a_code_parameter_is_declared() {
    let tools = IrisTools::new(None).expect("IrisTools::new");
    let registered = tools.registered_tool_names();
    assert!(
        registered.contains("iris_ws_exec"),
        "the router does not register iris_ws_exec, so this test is checking nothing"
    );

    let mut with_code: Vec<String> = Vec::new();
    for tool in registered.iter() {
        // `iris_query` sends SQL through `query`, not `code`; it has its own branch in step [0].
        if struct_fields(&params_type(tool)).contains("code") {
            with_code.push(tool.clone());
        }
    }
    with_code.sort();
    assert!(
        !with_code.is_empty(),
        "the scan found no tool with a `code` parameter, so it is broken — iris_execute has one"
    );
    for tool in &with_code {
        assert!(
            CODE_EXEC_TOOLS.contains(&tool.as_str()),
            "`{tool}` takes a `code` parameter but is not in CODE_EXEC_TOOLS, so dispatch_gate \
             step [0] never scans it. Add it to the constant in src/policy/gate.rs. \
             Declared: {CODE_EXEC_TOOLS:?}; found with a code parameter: {with_code:?}"
        );
    }
}

/// The constant must not grow entries that are not tools. A typo there disables the gate for the
/// tool it was meant to name, silently — and no other test would notice, because a name that
/// matches nothing simply never fires.
///
/// Walked against the router's registry, not against a literal, for the same reason as above.
#[test]
fn code_exec_tools_names_only_registered_tools() {
    let tools = IrisTools::new(None).expect("IrisTools::new");
    let registered = tools.registered_tool_names();
    for declared in CODE_EXEC_TOOLS {
        assert!(
            registered.contains(*declared),
            "CODE_EXEC_TOOLS names `{declared}`, which the router does not register. \
             Registered count: {}",
            registered.len()
        );
    }
}

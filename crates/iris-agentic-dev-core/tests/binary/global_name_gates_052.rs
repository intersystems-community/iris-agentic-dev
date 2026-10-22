//! Layer 2 for the global-name gates (spec 052, amendment 2026-10-10).
//!
//! `tests/unit/test_global_name_gates_052.rs` pins what the gate decides. These pin that the decision
//! is wired into the three tools that take a global name, by spawning the binary and calling them.
//!
//! No IRIS is needed, and that is the assertion: every refusal here comes from a gate that runs before
//! the first request. Both tiers are enabled on purpose. With them off, `call_tool` refuses
//! `global_kill` and `iris_global` writes first, and these tests would pass for the wrong reason.

use iris_agentic_dev_core::testing::{answer_text, require_iad_binary, McpSession};

fn tiers_on() -> Vec<(String, String)> {
    vec![
        ("IRIS_WRITE_TOOLS_ENABLED".to_string(), "true".to_string()),
        (
            "IRIS_DESTRUCTIVE_TOOLS_ENABLED".to_string(),
            "true".to_string(),
        ),
    ]
}

fn body(answer: &serde_json::Value) -> serde_json::Value {
    let result = answer
        .get("result")
        .unwrap_or_else(|| panic!("expected a result, got: {}", answer_text(answer)));
    if let Some(sc) = result.get("structuredContent") {
        return sc.clone();
    }
    let text = result
        .get("content")
        .and_then(|c| c.get(0))
        .and_then(|c| c.get("text"))
        .and_then(|t| t.as_str())
        .unwrap_or_else(|| panic!("no structuredContent and no text: {}", answer_text(answer)));
    serde_json::from_str(text).unwrap_or_else(|e| panic!("body is not JSON ({e}): {text}"))
}

fn code_of(answer: &serde_json::Value) -> String {
    body(answer)
        .get("error_code")
        .and_then(|c| c.as_str())
        .unwrap_or("")
        .to_string()
}

const GATE_CODES: [&str; 3] = [
    "SYSTEM_BLOCKLIST",
    "PHI_GATE_BLOCKED",
    "INVALID_GLOBAL_NAME",
];

/// One binary, many calls: spawning is the slow part.
#[test]
#[ignore = "spawns the built binary; run with --include-ignored"]
fn iris_global_refuses_each_spelling_of_a_system_global() {
    let Some(_bin) = require_iad_binary() else {
        return;
    };
    let mut session = McpSession::start(&tiers_on());
    for action in ["get", "list", "kill", "set"] {
        for name in [
            "oddDEF",
            "^oddDEF",
            "ROUTINE",
            "^ROUTINE",
            "^routine",
            "%SYS",
            "^%SYS.Foo",
        ] {
            let answer = session.call(
                "iris_global",
                &serde_json::json!({"action": action, "global_name": name, "subscripts": ["IadGate052"], "value": "x"}),
            );
            assert_eq!(
                code_of(&answer),
                "SYSTEM_BLOCKLIST",
                "{action} {name} must be refused before any IRIS call — {}",
                answer_text(&answer)
            );
        }
    }
}

#[test]
#[ignore = "spawns the built binary; run with --include-ignored"]
fn iris_global_refuses_each_spelling_of_a_phi_global() {
    let Some(_bin) = require_iad_binary() else {
        return;
    };
    let mut session = McpSession::start(&tiers_on());
    for name in ["PAPMIPatient", "^PAPMIPatient", "^paadm1", "^MRADM"] {
        let answer = session.call(
            "iris_global",
            &serde_json::json!({"action": "get", "global_name": name, "subscripts": ["1"]}),
        );
        assert_eq!(
            code_of(&answer),
            "PHI_GATE_BLOCKED",
            "{name} — {}",
            answer_text(&answer)
        );
    }
}

#[test]
#[ignore = "spawns the built binary; run with --include-ignored"]
fn iris_global_refuses_a_name_that_is_not_a_name() {
    let Some(_bin) = require_iad_binary() else {
        return;
    };
    let mut session = McpSession::start(&tiers_on());
    for name in [
        "oddDEF(\"IadGate052\")",
        "^^ROUTINE",
        "ROUTINE ",
        "^|\"%SYS\"|ROUTINE",
        "Foo)  Set ^IadGate052(1)=1  Set x=(1",
        "",
    ] {
        let answer = session.call(
            "iris_global",
            &serde_json::json!({"action": "get", "global_name": name}),
        );
        assert_eq!(
            code_of(&answer),
            "INVALID_GLOBAL_NAME",
            "{name:?} — {}",
            answer_text(&answer)
        );
    }
}

/// The hole the live probe found: the gate read `global_name` only, and these two take `global`.
#[test]
#[ignore = "spawns the built binary; run with --include-ignored"]
fn global_preview_and_global_kill_are_gated_like_iris_global() {
    let Some(_bin) = require_iad_binary() else {
        return;
    };
    let mut session = McpSession::start(&tiers_on());
    for (name, want) in [
        ("oddDEF", "SYSTEM_BLOCKLIST"),
        ("^oddDEF", "SYSTEM_BLOCKLIST"),
        ("ROUTINE", "SYSTEM_BLOCKLIST"),
        ("^ROUTINE", "SYSTEM_BLOCKLIST"),
        ("PAPMIPatient", "PHI_GATE_BLOCKED"),
        ("^PAPMIPatient", "PHI_GATE_BLOCKED"),
        ("oddDEF(\"x\")", "INVALID_GLOBAL_NAME"),
    ] {
        let preview = session.call(
            "global_preview",
            &serde_json::json!({"global": name, "count": 1}),
        );
        assert_eq!(
            code_of(&preview),
            want,
            "global_preview {name} — {}",
            answer_text(&preview)
        );
        assert!(
            body(&preview).get("confirm_token").is_none(),
            "a refused preview must not mint a token — {}",
            answer_text(&preview)
        );
        let kill = session.call(
            "global_kill",
            &serde_json::json!({"global": name, "confirm_token": "never-issued"}),
        );
        assert_eq!(
            code_of(&kill),
            want,
            "global_kill {name} — {}",
            answer_text(&kill)
        );
    }
}

/// The gate must not become a wall: an application global in either spelling goes on to IRIS, and
/// with no IRIS here it fails there, not at a gate.
#[test]
#[ignore = "spawns the built binary; run with --include-ignored"]
fn an_application_global_passes_the_gate_in_both_spellings() {
    let Some(_bin) = require_iad_binary() else {
        return;
    };
    let mut session = McpSession::start(&tiers_on());
    for name in [
        "IadGate052.Probe",
        "^IadGate052.Probe",
        "MyApp.Data",
        "SYSCONFIG",
    ] {
        let answer = session.call(
            "iris_global",
            &serde_json::json!({"action": "get", "global_name": name}),
        );
        let code = code_of(&answer);
        assert!(
            !GATE_CODES.contains(&code.as_str()),
            "{name} is an application global and must reach IRIS; got {code} — {}",
            answer_text(&answer)
        );
    }
}

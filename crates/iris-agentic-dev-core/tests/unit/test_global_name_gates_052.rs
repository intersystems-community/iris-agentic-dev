//! Spec 052, amendment 2026-10-10 (FR-015 to FR-021): the gate and the executor must see one name.
//!
//! `iris_global` stripped a leading `^` and spliced the rest into ObjectScript, while gates [3] and
//! [4] compared the string as typed. `^ROUTINE` wrote to a code-storage global that `ROUTINE`
//! refused; `oddDEF("x")` slipped past an exact-match rule; `global_preview` and `global_kill` carry
//! the name in `global`, which the gate never read. Every test below runs a name in both forms or
//! walks the table of things that are not names, because a gate test that passes only bare names is
//! how this shipped.

use iris_agentic_dev_core::iris::audit_log::scrub_params;
use iris_agentic_dev_core::iris::workspace_config::{ConnectionPolicy, DataPolicy, McpTemplate};
use iris_agentic_dev_core::policy::data_policy_gate::check_phi_name_gate;
use iris_agentic_dev_core::policy::gate::dispatch_gate;
use iris_agentic_dev_core::policy::patterns::{PHI_NAME_PATTERNS, SYSTEM_BLOCKLIST};
use iris_agentic_dev_core::policy::system_blocklist_gate::check_system_blocklist;
use iris_agentic_dev_core::tools::global::canonical_global_name;

fn policy(blocklist: &[&str], kill_allow: &[&str]) -> ConnectionPolicy {
    ConnectionPolicy {
        server_name: "iris-dev".to_string(),
        allow: None,
        mcp_template: Some(McpTemplate::Dev),
        data_policy: Some(DataPolicy::Block),
        global_blocklist: blocklist.iter().map(|s| s.to_string()).collect(),
        data_policy_kill_allowlist: kill_allow.iter().map(|s| s.to_string()).collect(),
        iris_audit: false,
    }
}

fn both_forms(name: &str) -> [String; 2] {
    [name.to_string(), format!("^{name}")]
}

fn code(r: Result<(), serde_json::Value>) -> String {
    r.expect_err("the call must be refused")["error_code"]
        .as_str()
        .unwrap_or("")
        .to_string()
}

// ── the canonical name (FR-015, FR-016) ──────────────────────────────────────────────────────

#[test]
fn canonical_name_strips_exactly_one_caret() {
    for (raw, want) in [
        ("MyApp", "MyApp"),
        ("^MyApp", "MyApp"),
        ("%SYS", "%SYS"),
        ("^%SYS", "%SYS"),
        ("MyApp.Data", "MyApp.Data"),
        ("Ens.MessageHeaderD", "Ens.MessageHeaderD"),
        ("a", "a"),
        ("A1.b2", "A1.b2"),
        // IRIS accepts these: a lone `%`, a doubled dot, a trailing digit after a dot.
        ("%", "%"),
        ("A..B", "A..B"),
        ("A.1", "A.1"),
    ] {
        assert_eq!(canonical_global_name(raw).as_deref(), Ok(want), "{raw}");
    }
}

#[test]
fn canonical_name_accepts_128_characters_and_refuses_129() {
    let ok = "A".repeat(128);
    assert_eq!(canonical_global_name(&ok).as_deref(), Ok(ok.as_str()));
    assert_eq!(
        canonical_global_name(&format!("^{ok}")).as_deref(),
        Ok(ok.as_str())
    );
    let too_long = "A".repeat(129);
    assert!(canonical_global_name(&too_long).is_err());
    assert!(canonical_global_name(&format!("^{too_long}")).is_err());
}

/// Everything here reached the executor, or would have, as part of a generated ObjectScript line.
#[test]
fn canonical_name_refuses_what_is_not_a_name() {
    for raw in [
        "",
        "^",
        "^^ROUTINE",
        "^^",
        "ROUTINE(\"x\")",
        "ROUTINE(",
        "oddDEF\"",
        "oddDEF(\"x\")",
        "^oddDEF(\"x\")",
        // IRIS answers `<SYNTAX>` for a name that ends in a dot, so the gate refuses it too.
        "A.",
        "^A1.",
        "%.",
        "Ab.Cd.",
        "ROUTINE ",
        " ROUTINE",
        "^ ROUTINE",
        "ROUTINE\t",
        "ROUTINE\n",
        "ROUTINE\0",
        "ROUTINE;",
        "A,B",
        "A B",
        "A-B",
        "A_B",
        "A:B",
        "A/B",
        "^|\"%SYS\"|ROUTINE",
        "^[\"dir\"]ROUTINE",
        "|\"%SYS\"|ROUTINE",
        "$ROUTINE",
        "^$ROUTINE",
        "1Abc",
        ".Abc",
        "@x",
        "Foo)  Set ^ROUTINE(1)=1  Set x=(1",
        // Non-ASCII: a fullwidth caret is not a caret, and `ſ` upper-cases to `S`, so a loose
        // comparison would read `%ſYS` as `%SYS`.
        "\u{ff3e}ROUTINE",
        "%\u{17f}YS",
        "Fo\u{f6}",
    ] {
        let err = canonical_global_name(raw).expect_err(&format!("{raw:?} is not a global name"));
        assert_eq!(err["error_code"], "INVALID_GLOBAL_NAME", "{raw:?}");
        assert_eq!(err["success"], false, "{raw:?}");
        assert!(
            err["message"].as_str().unwrap_or("").contains("global"),
            "the message names the problem: {err}"
        );
    }
}

// ── gate [3], both forms (FR-015, FR-017) ────────────────────────────────────────────────────

#[test]
fn every_system_blocklist_entry_blocks_both_forms() {
    for pattern in SYSTEM_BLOCKLIST {
        // A name the pattern is certain to match: the pattern itself without `^` and `*`.
        let stem = pattern.trim_start_matches('^').trim_end_matches('*');
        for name in both_forms(stem) {
            assert!(
                check_system_blocklist(&name, &[], &[], false, "s").is_some(),
                "{pattern} must block {name:?}"
            );
        }
    }
}

#[test]
fn the_reported_names_are_blocked_in_both_forms() {
    for stem in [
        "ROUTINE",
        "oddDEF",
        "rMAC",
        "%SYS",
        "%Dictionary.Foo",
        "Ens.Config.Item",
    ] {
        for name in both_forms(stem) {
            assert!(
                check_system_blocklist(&name, &[], &[], false, "s").is_some(),
                "{name}"
            );
        }
    }
}

#[test]
fn blocklist_case_is_ignored_in_both_forms() {
    for name in ["routine", "^routine", "ROUTINE", "^Routine", "^ODDdef"] {
        assert!(
            check_system_blocklist(name, &[], &[], false, "s").is_some(),
            "{name}"
        );
    }
}

#[test]
fn application_globals_are_not_blocked_in_either_form() {
    for stem in [
        "MyApp",
        "MyApp.Data",
        "SYSCONFIG",
        "IRIS.SystemPerformance",
        "Routines",
        "USERS",
    ] {
        for name in both_forms(stem) {
            assert!(
                check_system_blocklist(&name, &[], &[], false, "s").is_none(),
                "{name} is an application global and must stay usable"
            );
        }
    }
}

/// Operator-authored entries, written either way, block either form of the name.
#[test]
fn custom_blocklist_entries_work_with_and_without_a_caret() {
    for entry in ["^Secret*", "Secret*", "^Vault", "Vault"] {
        let custom = vec![entry.to_string()];
        let stem = entry.trim_start_matches('^').trim_end_matches('*');
        let probe = if entry.ends_with('*') {
            format!("{stem}Data")
        } else {
            stem.to_string()
        };
        for name in both_forms(&probe) {
            let hit = check_system_blocklist(&name, &custom, &[], false, "s");
            assert!(hit.is_some(), "entry {entry:?} must block {name:?}");
            assert_eq!(hit.unwrap()["error_code"], "SYSTEM_BLOCKLIST");
        }
        for name in both_forms("Unrelated") {
            assert!(check_system_blocklist(&name, &custom, &[], false, "s").is_none());
        }
    }
}

#[test]
fn the_kill_allowlist_matches_either_form_but_only_for_a_kill() {
    let allow = vec!["^SYS.Scratch*".to_string()];
    // `^SYS.Scratch...` is under the system rule `^SYS.*`; the allowlist exempts a kill only.
    for name in both_forms("SYS.ScratchTemp") {
        assert!(
            check_system_blocklist(&name, &[], &allow, true, "s").is_none(),
            "{name} kill"
        );
        assert!(
            check_system_blocklist(&name, &[], &allow, false, "s").is_some(),
            "{name} write"
        );
    }
    let bare = vec!["SYS.Scratch*".to_string()];
    for name in both_forms("SYS.ScratchTemp") {
        assert!(
            check_system_blocklist(&name, &[], &bare, true, "s").is_none(),
            "{name}"
        );
    }
}

/// Every code-storage global found by enumerating `^$GLOBAL` in USER and %SYS on 2026.2 (FR-022).
/// A write to any of them edits compiled code or routine source.
const CODE_STORAGE_SEEN: &[&str] = &[
    "ROUTINE",
    "rBACKUP",
    "rINC",
    "rINCSAVE",
    "rINDEX",
    "rINDEXCLASS",
    "rINDEXEXT",
    "rINDEXSQL",
    "rMAC",
    "rMACSAVE",
    "rMAP",
    "rOBJ",
    "oddCOM",
    "oddDEF",
    "oddDEP",
    "oddEXT",
    "oddEXTR",
    "oddMAP",
    "oddMETA",
    "oddPKG",
    "oddPROC",
    "oddPROJECT",
    "oddPYTHON",
    "oddSQL",
    "oddStudioDocument",
    "oddStudioMenu",
    "oddTSQL",
    "oddXML",
];

#[test]
fn every_code_storage_global_seen_on_a_live_instance_is_blocked_in_both_forms() {
    for stem in CODE_STORAGE_SEEN {
        for name in both_forms(stem) {
            assert!(
                check_system_blocklist(&name, &[], &[], false, "s").is_some(),
                "{name} holds code or routine source and must be on the blocklist"
            );
        }
    }
}

/// The exact names must not turn into prefixes: application globals that merely start the same way
/// stay usable. This is the reason `^SYS*` was narrowed to `^SYS` and `^SYS.*`.
#[test]
fn application_globals_that_start_like_code_storage_are_not_blocked() {
    for stem in [
        "oddJobs", "oddments", "rMACHINE", "rINCOME", "rINDEXED", "ROUTINES", "rObjects",
    ] {
        for name in both_forms(stem) {
            assert!(
                check_system_blocklist(&name, &[], &[], false, "s").is_none(),
                "{name} is an application global"
            );
        }
    }
}

// ── gate [4], both forms ─────────────────────────────────────────────────────────────────────

#[test]
fn every_phi_pattern_blocks_both_forms_until_acknowledged() {
    for pattern in PHI_NAME_PATTERNS {
        let stem = pattern.trim_start_matches('^').trim_end_matches('*');
        let name_stem = format!("{stem}123");
        for name in both_forms(&name_stem) {
            let hit = check_phi_name_gate(&name, false, "s");
            assert!(hit.is_some(), "{pattern} must flag {name:?}");
            assert_eq!(hit.unwrap()["error_code"], "PHI_GATE_BLOCKED");
            assert!(
                check_phi_name_gate(&name, true, "s").is_none(),
                "{name} acknowledged"
            );
        }
    }
}

#[test]
fn non_phi_names_are_not_flagged_in_either_form() {
    for stem in ["MyAppData", "OEM", "PAPM", "Orders"] {
        for name in both_forms(stem) {
            // `^OE*` and `^ORDER*` are real prefixes, so only names clear of every prefix belong here.
            let flagged = check_phi_name_gate(&name, false, "s").is_some();
            let expected = PHI_NAME_PATTERNS.iter().any(|p| {
                let prefix = p
                    .trim_start_matches('^')
                    .trim_end_matches('*')
                    .to_uppercase();
                stem.to_uppercase().starts_with(&prefix)
            });
            assert_eq!(flagged, expected, "{name}");
        }
    }
}

// ── dispatch_gate reads every key, canonicalizes, and refuses invalid names (FR-016, FR-018) ──

#[test]
fn dispatch_gate_blocks_the_system_global_in_every_key_and_form() {
    let p = policy(&[], &[]);
    for key in ["global_name", "globalName", "global"] {
        for name in both_forms("oddDEF") {
            let params = serde_json::json!({ key: name, "action": "get" });
            assert_eq!(
                code(dispatch_gate("iris_global", "iris-dev", Some(&p), &params)),
                "SYSTEM_BLOCKLIST",
                "key {key}, name {name}"
            );
        }
    }
}

#[test]
fn global_preview_and_global_kill_pass_through_gates_3_and_4() {
    let p = policy(&[], &[]);
    for tool in ["global_preview", "global_kill"] {
        for name in both_forms("oddDEF") {
            let params = serde_json::json!({ "global": name, "confirm_token": "t" });
            assert_eq!(
                code(dispatch_gate(tool, "iris-dev", Some(&p), &params)),
                "SYSTEM_BLOCKLIST",
                "{tool} {name}"
            );
        }
        for name in both_forms("PAPMIPatient") {
            let params = serde_json::json!({ "global": name });
            assert_eq!(
                code(dispatch_gate(tool, "iris-dev", Some(&p), &params)),
                "PHI_GATE_BLOCKED",
                "{tool} {name}"
            );
        }
        for name in both_forms("MyApp.Data") {
            let params = serde_json::json!({ "global": name });
            assert!(
                dispatch_gate(tool, "iris-dev", Some(&p), &params).is_ok(),
                "{tool} {name}"
            );
        }
    }
}

#[test]
fn an_invalid_name_is_refused_before_the_blocklist_looks_at_it() {
    let p = policy(&[], &[]);
    for key in ["global_name", "global"] {
        for raw in [
            "oddDEF(\"x\")",
            "^^oddDEF",
            "ROUTINE ",
            "^|\"%SYS\"|ROUTINE",
            "",
        ] {
            let params = serde_json::json!({ key: raw, "action": "get" });
            assert_eq!(
                code(dispatch_gate("iris_global", "iris-dev", Some(&p), &params)),
                "INVALID_GLOBAL_NAME",
                "key {key}, raw {raw:?}"
            );
        }
    }
}

#[test]
fn a_tool_with_no_global_param_is_untouched() {
    let p = policy(&[], &[]);
    let params = serde_json::json!({ "query": "SELECT 1" });
    assert!(dispatch_gate("iris_query", "iris-dev", Some(&p), &params).is_ok());
    // A missing name on a tool that carries one is the handler's `MISSING_PARAMS`, not a gate error.
    let params = serde_json::json!({ "action": "list" });
    assert!(dispatch_gate("iris_global", "iris-dev", Some(&p), &params).is_ok());
}

#[test]
fn no_policy_still_applies_the_name_rules() {
    for name in both_forms("oddDEF") {
        let params = serde_json::json!({ "global_name": name, "action": "get" });
        assert_eq!(
            code(dispatch_gate("iris_global", "s", None, &params)),
            "SYSTEM_BLOCKLIST",
            "{name}"
        );
    }
    let params = serde_json::json!({ "global": "oddDEF(\"x\")" });
    assert_eq!(
        code(dispatch_gate("global_preview", "s", None, &params)),
        "INVALID_GLOBAL_NAME"
    );
}

#[test]
fn a_custom_blocklist_entry_blocks_global_preview_in_both_forms() {
    let p = policy(&["^Vault*"], &[]);
    for name in both_forms("VaultKeys") {
        let params = serde_json::json!({ "global": name });
        assert_eq!(
            code(dispatch_gate(
                "global_preview",
                "iris-dev",
                Some(&p),
                &params
            )),
            "SYSTEM_BLOCKLIST",
            "{name}"
        );
    }
}

// ── the environment template (FR-023) ────────────────────────────────────────────────────────

fn with_template(template: McpTemplate) -> ConnectionPolicy {
    ConnectionPolicy {
        mcp_template: Some(template),
        ..policy(&[], &[])
    }
}

#[test]
fn global_kill_is_an_execute_operation_on_live_and_test_like_iris_global_kill() {
    for template in [McpTemplate::Live, McpTemplate::Test] {
        let p = with_template(template);
        let kill = serde_json::json!({ "global": "MyApp.Data", "confirm_token": "t" });
        assert_eq!(
            code(dispatch_gate("global_kill", "s", Some(&p), &kill)),
            "ENV_GATE_BLOCKED"
        );
        let via_iris_global = serde_json::json!({ "global_name": "MyApp.Data", "action": "kill" });
        assert_eq!(
            code(dispatch_gate(
                "iris_global",
                "s",
                Some(&p),
                &via_iris_global
            )),
            "ENV_GATE_BLOCKED",
            "the two spellings of one operation must agree"
        );
    }
    let dev = with_template(McpTemplate::Dev);
    let kill = serde_json::json!({ "global": "MyApp.Data", "confirm_token": "t" });
    assert!(dispatch_gate("global_kill", "s", Some(&dev), &kill).is_ok());
}

#[test]
fn global_preview_is_a_read_on_every_template() {
    for template in [McpTemplate::Dev, McpTemplate::Test, McpTemplate::Live] {
        let p = with_template(template);
        let params = serde_json::json!({ "global": "MyApp.Data", "count": 1 });
        assert!(dispatch_gate("global_preview", "s", Some(&p), &params).is_ok());
    }
}

// ── the audit scrub (FR-020) ─────────────────────────────────────────────────────────────────

#[test]
fn the_audit_scrub_redacts_a_phi_name_in_global_and_global_name_with_or_without_a_caret() {
    for key in ["global_name", "global"] {
        for name in both_forms("PAPMIPatient") {
            let scrubbed = scrub_params(serde_json::json!({ key: name, "action": "get" }));
            assert_eq!(scrubbed[key], "[REDACTED-PHI]", "{key} {name}");
        }
        let kept = scrub_params(serde_json::json!({ key: "MyApp.Data" }));
        assert_eq!(kept[key], "MyApp.Data");
    }
}

// ── the handlers validate on their own (FR-019, FR-021) ──────────────────────────────────────
//
// A caller that reaches a handler without `dispatch_gate` (a test, a new entry point, a refactor
// that moves the gate) must still not be able to interpolate an invalid name. No IRIS is needed:
// these fail before the first request, which is the assertion.

use iris_agentic_dev_core::iris::connection::{DiscoverySource, IrisConnection};
use iris_agentic_dev_core::tools::admin_tools::{
    global_kill_impl, global_preview_impl, ConfirmEntry, GlobalKillParams, GlobalPreviewParams,
};
use iris_agentic_dev_core::tools::global::{handle_iris_global, IrisGlobalParams};
use std::collections::HashMap;
use std::sync::Arc;

fn unreachable_conn() -> Arc<IrisConnection> {
    // Port 1 refuses connections at once. If a handler got past validation it would fail with a
    // transport error, never with INVALID_GLOBAL_NAME.
    Arc::new(IrisConnection::new(
        "http://127.0.0.1:1".to_string(),
        "USER",
        "u".to_string(),
        "p".to_string(),
        DiscoverySource::EnvVar,
    ))
}

fn global_params(action: &str, name: &str) -> IrisGlobalParams {
    IrisGlobalParams {
        action: action.to_string(),
        global_name: name.to_string(),
        subscripts: None,
        value: Some("x".to_string()),
        namespace: None,
        subtree: None,
        max_nodes: None,
        max_subscripts: None,
        acknowledge_phi: None,
        server: None,
    }
}

#[tokio::test]
async fn iris_global_refuses_an_invalid_name_even_if_the_gate_passed() {
    let conn = unreachable_conn();
    let client = reqwest::Client::new();
    for action in ["get", "set", "kill", "list"] {
        for raw in ["oddDEF(\"x\")", "^^ROUTINE", "A B", ""] {
            let out = handle_iris_global(&conn, &client, &global_params(action, raw), Ok(())).await;
            assert_eq!(
                out["error_code"], "INVALID_GLOBAL_NAME",
                "{action} {raw:?}: {out}"
            );
        }
    }
}

#[tokio::test]
async fn global_preview_refuses_an_invalid_name_and_mints_no_token() {
    let tokens = tokio::sync::Mutex::new(HashMap::<String, ConfirmEntry>::new());
    let out = global_preview_impl(
        GlobalPreviewParams {
            global: "oddDEF(\"x\")".to_string(),
            server: None,
            count: 1,
            iris: unreachable_conn(),
            client: Arc::new(reqwest::Client::new()),
        },
        &tokens,
    )
    .await
    .expect("a refusal is a tool result, not a protocol error");
    let body = out.structured_content.clone().unwrap_or_default();
    assert_eq!(body["error_code"], "INVALID_GLOBAL_NAME", "{body}");
    assert!(body.get("confirm_token").is_none(), "{body}");
    assert!(
        tokens.lock().await.is_empty(),
        "a refused name must not leave a usable token"
    );
}

#[tokio::test]
async fn global_kill_refuses_an_invalid_name_before_it_checks_the_token() {
    let tokens = tokio::sync::Mutex::new(HashMap::<String, ConfirmEntry>::new());
    let out = global_kill_impl(
        GlobalKillParams {
            global: "ROUTINE(\"x\")".to_string(),
            server: None,
            confirm_token: "anything".to_string(),
            iris: unreachable_conn(),
            client: Arc::new(reqwest::Client::new()),
        },
        &tokens,
    )
    .await
    .expect("a refusal is a tool result");
    let body = out.structured_content.clone().unwrap_or_default();
    assert_eq!(body["error_code"], "INVALID_GLOBAL_NAME", "{body}");
}

/// A token minted for `^MyApp` must not authorize a kill of the same global spelled another way, and
/// the spelling must not change which global the token was minted for.
#[tokio::test]
async fn a_token_is_bound_to_the_canonical_name() {
    let tokens = tokio::sync::Mutex::new(HashMap::<String, ConfirmEntry>::new());
    tokens.lock().await.insert(
        "tok".to_string(),
        ConfirmEntry {
            global: "MyApp".to_string(),
            server: None,
            issued_at: std::time::Instant::now(),
        },
    );
    // `^MyApp` and `MyApp` are the same global: the token is accepted for either spelling, so the
    // kill gets past the token check and fails at the transport (port 1), not at CONFIRM_MISMATCH.
    for spelling in ["MyApp", "^MyApp"] {
        let out = global_kill_impl(
            GlobalKillParams {
                global: spelling.to_string(),
                server: None,
                confirm_token: "tok".to_string(),
                iris: unreachable_conn(),
                client: Arc::new(reqwest::Client::new()),
            },
            &tokens,
        )
        .await;
        let body = out
            .ok()
            .and_then(|r| r.structured_content)
            .unwrap_or_default();
        assert_ne!(body["error_code"], "CONFIRM_MISMATCH", "{spelling}: {body}");
        assert_ne!(
            body["error_code"], "INVALID_GLOBAL_NAME",
            "{spelling}: {body}"
        );
        // The token is single use; put it back for the next spelling.
        tokens.lock().await.insert(
            "tok".to_string(),
            ConfirmEntry {
                global: "MyApp".to_string(),
                server: None,
                issued_at: std::time::Instant::now(),
            },
        );
    }
}

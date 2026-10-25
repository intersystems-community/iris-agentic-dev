//! Spec 141 T207: every operator limit in `REFUSAL_CODES` has an edit that lifts it, and the
//! catalogue `trust --show` prints lists them.
//!
//! `refusal_remedy` answers `None` for a code `edit_for` does not know, and the operator then gets
//! no line on stderr for a refusal the agent was told is theirs to lift. Nothing failed when
//! `SERVICE_ACCOUNT_TERMINAL_DENIED` was added to the list without a rule; this does.

use iris_agentic_dev_core::policy::refusal::{
    classify_refusal, refusal_remedy, remedy_catalogue, REFUSAL_CODES,
};
use serde_json::json;
use std::path::Path;

/// A body for `code`, with the fields a gate sets that decide the kind.
fn body(code: &str) -> serde_json::Value {
    json!({"success": false, "error_code": code, "error": "x", "blocked_category": "execute"})
}

#[test]
fn every_operator_limit_has_a_remedy_on_the_default_and_on_a_named_server() {
    let mut limits = 0;
    for code in REFUSAL_CODES {
        for server in ["default", "prod"] {
            let out = classify_refusal(body(code), server, &[]);
            if out["refusal"]["kind"] != "operator_limit" {
                continue;
            }
            limits += 1;
            let remedy = refusal_remedy(&out, None, &[])
                .unwrap_or_else(|| panic!("{code} on {server}: an operator limit with no remedy"));
            assert!(remedy.line.contains(code), "{}", remedy.line);
            assert!(
                remedy.line.contains("~/.iris-agentic-dev.toml"),
                "{}",
                remedy.line
            );
        }
    }
    assert!(limits >= 30, "only {limits} operator limits were checked");
}

#[test]
fn the_remedy_names_the_edit_for_a_named_server_and_for_the_default() {
    let on_default = classify_refusal(body("WRITE_TOOLS_DISABLED"), "default", &[]);
    let line = refusal_remedy(&on_default, None, &[]).unwrap().line;
    assert!(line.contains("`write_tools_enabled = true`"), "{line}");

    let on_prod = classify_refusal(body("DESTRUCTIVE_TOOLS_DISABLED"), "prod", &[]);
    let line = refusal_remedy(&on_prod, Some(Path::new("/h/.iad.toml")), &[])
        .unwrap()
        .line;
    assert!(
        line.contains("[policy.prod] destructiveTools = true"),
        "{line}"
    );
    assert!(line.contains("/h/.iad.toml"), "{line}");

    let compile = classify_refusal(body("COMPILE_TIME_EXEC_BLOCKED"), "prod", &[]);
    let line = refusal_remedy(&compile, None, &[]).unwrap().line;
    assert!(
        line.contains("[policy.prod] compileTimeGate = false"),
        "{line}"
    );
}

#[test]
fn an_acknowledgement_and_a_fixed_rule_have_no_remedy_for_the_operator() {
    for code in [
        "PHI_ACK_REQUIRED",
        "SQL_WRITE_BLOCKED",
        "STORED_SOURCE_UNREADABLE",
    ] {
        let out = classify_refusal(body(code), "default", &[]);
        assert!(out.get("refusal").is_some(), "{code}");
        assert!(refusal_remedy(&out, None, &[]).is_none(), "{code}");
    }
    assert!(refusal_remedy(&body("SQL_ERROR"), None, &[]).is_none());
}

#[test]
fn the_catalogue_lists_each_limit_once_per_server_kind_and_ends_with_the_file() {
    let lines = remedy_catalogue(None);
    for code in [
        "WRITE_TOOLS_DISABLED",
        "COMPILE_TIME_EXEC_BLOCKED",
        "ENV_GATE_BLOCKED",
        "READ_ONLY_STRICT",
        "ADMIN_WRITE_DISABLED",
        "SERVICE_ACCOUNT_UNCONFIGURED_FOR_SERVER",
    ] {
        assert!(
            lines.iter().any(|l| l.starts_with(code)),
            "{code} is missing from the catalogue: {lines:?}"
        );
    }
    let unique: std::collections::BTreeSet<&String> = lines.iter().collect();
    assert_eq!(unique.len(), lines.len(), "a line is repeated");
    assert!(
        lines.last().unwrap().contains("~/.iris-agentic-dev.toml"),
        "{lines:?}"
    );
    let named = remedy_catalogue(Some(Path::new("/h/op.toml")));
    assert!(named.last().unwrap().contains("/h/op.toml"));
}

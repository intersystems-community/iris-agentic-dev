//! Spec 141 FR-037, FR-038: the template a system mode implies, how the answer to the system-mode
//! query is read, and the role names.

use iris_agentic_dev_core::iris::connection::{parse_system_mode_response, SystemMode};
use iris_agentic_dev_core::iris::workspace_config::{
    load_fleet_config_from_str, parse_role, ConnectionRole, McpTemplate,
};
use iris_agentic_dev_core::policy::effective::{implied_template, PartSource};
use std::path::Path;

// T-R4a-U17
#[test]
fn a_system_mode_implies_a_template() {
    let t = |mode, is_default, ns| implied_template(mode, is_default, ns).0;
    assert_eq!(t(SystemMode::Live, true, "USER"), McpTemplate::Live);
    assert_eq!(t(SystemMode::Live, false, "USER"), McpTemplate::Live);
    assert_eq!(t(SystemMode::Test, true, "USER"), McpTemplate::Test);
    assert_eq!(t(SystemMode::Development, true, "USER"), McpTemplate::Dev);
    // A fresh instance has no system mode: development.
    assert_eq!(t(SystemMode::Unset, true, "USER"), McpTemplate::Dev);
    assert_eq!(t(SystemMode::Unset, false, "USER"), McpTemplate::Dev);

    // Could not read it: the default server falls back to the namespace inference...
    assert_eq!(t(SystemMode::Unknown, true, "USER"), McpTemplate::Dev);
    assert_eq!(t(SystemMode::Unknown, true, "PROD"), McpTemplate::Live);
    assert_eq!(
        t(SystemMode::Unknown, true, "production"),
        McpTemplate::Live
    );
    // ...and a pool member is judged like a Live instance.
    assert_eq!(t(SystemMode::Unknown, false, "USER"), McpTemplate::Live);

    for mode in [
        SystemMode::Live,
        SystemMode::Test,
        SystemMode::Development,
        SystemMode::Unset,
        SystemMode::Unknown,
    ] {
        assert_eq!(
            implied_template(mode, true, "USER").1,
            PartSource::SystemMode,
            "an implied template is reported as coming from the system mode"
        );
    }
}

// T-R4a-U18
#[test]
fn the_system_mode_answer_parser_reads_bodies_captured_from_live_iris() {
    let load = |name: &str| -> (u16, String) {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/system_mode_141")
            .join(name);
        let v: serde_json::Value = serde_json::from_str(
            &std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display())),
        )
        .unwrap();
        (
            v["status"].as_u64().expect("status") as u16,
            v["body"].as_str().expect("body").to_string(),
        )
    };

    let (st, body) = load("as_system.json");
    assert_eq!(st, 200);
    assert!(
        body.contains("\"Value\":\"\""),
        "the fixture is the unset answer: {body}"
    );
    assert_eq!(parse_system_mode_response(st, &body), SystemMode::Unset);

    let (st, body) = load("as_no_roles.json");
    assert_eq!(st, 401);
    assert_eq!(parse_system_mode_response(st, &body), SystemMode::Unknown);
}

#[test]
fn the_system_mode_answer_parser_edge_cases() {
    let ok =
        |content: &str| format!(r#"{{"status":{{"errors":[]}},"result":{{"content":{content}}}}}"#);
    // An empty result set is also "nothing is set".
    assert_eq!(
        parse_system_mode_response(200, &ok("[]")),
        SystemMode::Unset
    );
    assert_eq!(
        parse_system_mode_response(200, &ok(r#"[{"Value":"  "}]"#)),
        SystemMode::Unset
    );
    assert_eq!(
        parse_system_mode_response(200, &ok(r#"[{"Value":"LIVE"}]"#)),
        SystemMode::Live
    );
    assert_eq!(
        parse_system_mode_response(200, &ok(r#"[{"Value":"FAILOVER"}]"#)),
        SystemMode::Live
    );
    assert_eq!(
        parse_system_mode_response(200, &ok(r#"[{"Value":"Test"}]"#)),
        SystemMode::Test
    );
    // Unreadable: no `result`, a non-2xx status, or a body that is not JSON.
    assert_eq!(
        parse_system_mode_response(200, r#"{"status":{"errors":[{"error":"x"}]}}"#),
        SystemMode::Unknown
    );
    assert_eq!(
        parse_system_mode_response(403, &ok(r#"[{"Value":"LIVE"}]"#)),
        SystemMode::Unknown
    );
    assert_eq!(
        parse_system_mode_response(200, "<html>"),
        SystemMode::Unknown
    );
    assert_eq!(parse_system_mode_response(200, ""), SystemMode::Unknown);
    // A value the Portal never writes is not Unset: something is there and iad does not know it.
    assert_eq!(
        parse_system_mode_response(200, &ok(r#"[{"Value":"Staging"}]"#)),
        SystemMode::Unknown
    );
}

// T-R4a-U19
#[test]
fn roles_parse_leniently_and_fail_closed() {
    for raw in ["workbench", "Workbench", " WORKBENCH "] {
        assert_eq!(
            parse_role(raw),
            (ConnectionRole::Workbench, None),
            "{raw:?}"
        );
    }
    for raw in ["subject", "Subject"] {
        assert_eq!(parse_role(raw), (ConnectionRole::Subject, None), "{raw:?}");
    }
    let (role, note) = parse_role("WORKSPACE");
    assert_eq!(role, ConnectionRole::Workbench);
    let note = note.expect("a deprecation note");
    assert!(
        note.contains("workspace") && note.contains("workbench"),
        "{note}"
    );

    let (role, note) = parse_role("control-plane");
    assert_eq!(role, ConnectionRole::Workbench);
    assert!(note.expect("a note").contains("control-plane"));

    let (role, note) = parse_role("contrl-plane");
    assert_eq!(role, ConnectionRole::Subject, "a typo fails closed");
    assert!(note.expect("a note").contains("contrl-plane"));
}

#[test]
fn a_fleet_file_reports_role_notes_in_errors_and_keeps_every_instance() {
    let fleet = load_fleet_config_from_str(
        r#"mode = "operate"
[instance.a]
host = "a.example"
role = "workspace"
[instance.b]
host = "b.example"
role = "control-plane"
[instance.c]
host = "c.example"
role = "contrl-plane"
[instance.d]
host = "d.example"
role = "Subject"
"#,
    )
    .expect("a fleet with odd roles still parses");
    assert_eq!(fleet.instance["a"].role, ConnectionRole::Workbench);
    assert_eq!(fleet.instance["b"].role, ConnectionRole::Workbench);
    assert_eq!(fleet.instance["c"].role, ConnectionRole::Subject);
    assert_eq!(fleet.instance["d"].role, ConnectionRole::Subject);
    assert_eq!(fleet.errors.len(), 3, "{:?}", fleet.errors);
    for (needle, name) in [
        ("workspace", "a"),
        ("control-plane", "b"),
        ("contrl-plane", "c"),
    ] {
        assert!(
            fleet
                .errors
                .iter()
                .any(|e| e.contains(needle) && e.contains(&format!("instance.{name}"))),
            "no note for instance.{name} ({needle}): {:?}",
            fleet.errors
        );
    }
}

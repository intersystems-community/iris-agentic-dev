//! Spec 141 FR-039, FR-040: every gate refusal carries `refusal {kind, server, lift, set_aside}`.
//!
//! The bodies come from the real gate functions. The schema in
//! `docs/contracts/refusal.schema.json` is checked by hand, because no
//! JSON-Schema crate is in the lock file (constitution VII); a test below keeps the hand check and
//! the schema file in step.

use iris_agentic_dev_core::iris::config_sources::{
    ConfigSnapshot, LoadInputs, SetAsideItem, CONFIG_FILE_NAME,
};
use iris_agentic_dev_core::iris::workspace_config::{
    check_role_gate, ConnectionPolicy, ConnectionRole, McpTemplate, ToolCategory,
};
use iris_agentic_dev_core::policy::code_edit_gate::{
    check_compile_time_code_mode, check_objectscript_code_edit,
};
use iris_agentic_dev_core::policy::data_policy_gate::{check_bulk_phi_gate, check_phi_name_gate};
use iris_agentic_dev_core::policy::effective::EffectivePolicy;
use iris_agentic_dev_core::policy::refusal::{classify_refusal, RefusalKind, REFUSAL_CODES};
use iris_agentic_dev_core::policy::system_blocklist_gate::check_system_blocklist;
use iris_agentic_dev_core::testing::gated;
use serde_json::{json, Value};
use std::path::Path;

/// The refusal schema, by hand: required keys, the kind enum, and the `lift` shape per kind.
fn assert_matches_schema(body: &Value) {
    assert_eq!(body["success"], json!(false), "{body}");
    assert!(
        body["error_code"].as_str().is_some_and(|c| !c.is_empty()
            && c.chars()
                .all(|ch| ch.is_ascii_uppercase() || ch.is_ascii_digit() || ch == '_')),
        "{body}"
    );
    assert!(
        body["error"].as_str().is_some_and(|e| !e.is_empty()),
        "{body}"
    );
    let r = &body["refusal"];
    assert!(r.is_object(), "no refusal object: {body}");
    let obj = r.as_object().unwrap();
    for key in obj.keys() {
        assert!(
            ["kind", "server", "lift", "set_aside"].contains(&key.as_str()),
            "unexpected refusal key {key}"
        );
    }
    let kind = r["kind"].as_str().expect("kind");
    assert!(
        [
            "acknowledgement_required",
            "operator_limit",
            "untrusted_setting",
            "fixed_rule"
        ]
        .contains(&kind),
        "{kind}"
    );
    assert!(r["server"].is_string(), "{body}");
    let lift = &r["lift"];
    match kind {
        "fixed_rule" => assert!(lift.is_null(), "{body}"),
        "acknowledgement_required" => {
            let arg = lift["argument"]
                .as_str()
                .unwrap_or_else(|| panic!("{body}"));
            assert!(["confirm", "acknowledgePhi"].contains(&arg), "{arg}");
            assert_eq!(lift.as_object().unwrap().len(), 1, "{body}");
        }
        "operator_limit" | "untrusted_setting" => {
            assert!(lift["setting"].is_string(), "{body}");
            assert_eq!(
                lift.as_object().unwrap().len(),
                1,
                "the file and the edit are not the agent's: {body}"
            );
        }
        _ => unreachable!(),
    }
    if kind == "untrusted_setting" {
        assert!(r["set_aside"].is_string(), "{body}");
    } else {
        assert!(
            r.get("set_aside").is_none(),
            "set_aside is for untrusted settings only: {body}"
        );
    }
}

fn kind_of(body: &Value) -> &str {
    body["refusal"]["kind"]
        .as_str()
        .unwrap_or_else(|| panic!("no kind: {body}"))
}

fn snapshot(op: &str, ws: &str) -> (ConfigSnapshot, tempfile::TempDir) {
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path().join("home");
    let proj = tmp.path().join("proj");
    std::fs::create_dir_all(&home).unwrap();
    std::fs::create_dir_all(&proj).unwrap();
    if !op.is_empty() {
        std::fs::write(home.join(CONFIG_FILE_NAME), op).unwrap();
    }
    std::fs::write(proj.join(CONFIG_FILE_NAME), ws).unwrap();
    let snap = ConfigSnapshot::load(&LoadInputs {
        cwd: std::fs::canonicalize(&proj).unwrap(),
        home: Some(std::fs::canonicalize(&home).unwrap()),
        ..Default::default()
    });
    (snap, tmp)
}

fn classify(body: Value, snap: &ConfigSnapshot, server: &str) -> Value {
    classify_refusal(body, server, &snap.set_aside)
}

// T-R4a-U20
#[test]
fn a_template_an_untrusted_file_would_have_lifted_is_an_untrusted_setting() {
    let (snap, _t) = snapshot(
        "[policy.default]\nmcpTemplate = \"live\"\n",
        "[policy.default]\nmcpTemplate = \"dev\"\n",
    );
    let body = gated::env("iris_execute", &McpTemplate::Live, "default", &json!({}))
        .expect("live blocks execute");
    let out = classify(body, &snap, "default");
    assert_matches_schema(&out);
    assert_eq!(kind_of(&out), "untrusted_setting");
    assert_eq!(out["refusal"]["server"], "default");
    assert_eq!(
        out["refusal"]["set_aside"],
        "workspace:policy.default.mcpTemplate"
    );
    assert_eq!(
        out["refusal"]["lift"]["setting"],
        "[policy.default] mcpTemplate"
    );
    // The way to lift it is in the set-aside item, not in the refusal.
    let item = snap
        .set_aside
        .iter()
        .find(|i| i.id == "workspace:policy.default.mcpTemplate")
        .expect("the set-aside item the refusal points at");
    assert!(item.lift.contains("iris-agentic-dev trust"), "{item:?}");
    // The gate's own fields stay where they were.
    assert_eq!(out["error_code"], "ENV_GATE_BLOCKED");
    assert_eq!(out["env_gate_blocked"], true);
    assert_eq!(out["server_name"], "default");
}

#[test]
fn a_template_the_file_itself_declared_is_an_operator_limit() {
    // The untrusted file says `live` (it tightens, so it applies) and no looser value was set aside.
    let (snap, _t) = snapshot("", "[policy.default]\nmcpTemplate = \"live\"\n");
    let body = gated::env("iris_execute", &McpTemplate::Live, "default", &json!({})).unwrap();
    let out = classify(body, &snap, "default");
    assert_matches_schema(&out);
    assert_eq!(kind_of(&out), "operator_limit");
    assert_eq!(
        out["refusal"]["lift"]["setting"],
        "[policy.default] mcpTemplate"
    );

    // Nothing declared anywhere: the system mode chose, and only the operator can declare one.
    let (snap, _t) = snapshot("", "namespace = \"USER\"\n");
    let body = gated::env("iris_execute", &McpTemplate::Live, "default", &json!({})).unwrap();
    let out = classify(body, &snap, "default");
    assert_eq!(kind_of(&out), "operator_limit");
    assert_eq!(
        out["refusal"]["lift"].as_object().unwrap().len(),
        1,
        "{out}"
    );
}

#[test]
fn the_built_in_blocklist_is_a_fixed_rule_and_a_custom_pattern_an_operator_limit() {
    let (snap, _t) = snapshot(
        "[policy.default]\nglobalBlocklist = [\"^Mine*\"]\n",
        "x = 1\n",
    );
    let built_in = check_system_blocklist("^oddDEF", &[], &[], false, "default").unwrap();
    let out = classify(built_in, &snap, "default");
    assert_matches_schema(&out);
    assert_eq!(kind_of(&out), "fixed_rule");
    assert!(out["refusal"]["lift"].is_null());
    assert!(
        !out["remediation"].as_str().unwrap_or("").is_empty(),
        "the remediation text stays"
    );

    let custom =
        check_system_blocklist("^MineThing", &["^Mine*".to_string()], &[], false, "default")
            .unwrap();
    let out = classify(custom, &snap, "default");
    assert_matches_schema(&out);
    assert_eq!(kind_of(&out), "operator_limit");
    assert_eq!(
        out["refusal"]["lift"]["setting"],
        "[policy.default] globalBlocklist"
    );
}

#[test]
fn acknowledgements_fixed_rules_and_the_role_gate() {
    let (snap, _t) = snapshot("", "x = 1\n");

    let phi = check_phi_name_gate("^PAPMI", false, "default").expect("phi name blocks");
    let out = classify(phi, &snap, "default");
    assert_matches_schema(&out);
    assert_eq!(kind_of(&out), "acknowledgement_required");
    assert_eq!(out["refusal"]["lift"]["argument"], "acknowledgePhi");

    let edit = check_objectscript_code_edit("Do ##class(%SYSTEM.OBJ).Delete(\"A.B\")", "default")
        .expect("code edit blocks");
    let out = classify(edit, &snap, "default");
    assert_matches_schema(&out);
    assert_eq!(kind_of(&out), "fixed_rule");

    // The role gate: soft block asks for `confirm`, hard block is a fixed rule, and both now carry
    // an error_code and a `success: false` like every other gate.
    let soft = check_role_gate(
        &ConnectionRole::Subject,
        "iris_compile",
        false,
        "prod",
        false,
    )
    .expect("subject write blocks");
    assert_eq!(soft["error_code"], "SUBJECT_CONFIRM_REQUIRED");
    assert_eq!(soft["success"], false);
    assert_eq!(
        soft["error"], "role_gate",
        "`error` stays for 1.7.x clients"
    );
    assert!(soft["message"].as_str().unwrap().contains("confirm: true"));
    let out = classify(soft, &snap, "prod");
    assert_matches_schema(&out);
    assert_eq!(kind_of(&out), "acknowledgement_required");
    assert_eq!(out["refusal"]["lift"]["argument"], "confirm");
    assert_eq!(out["refusal"]["server"], "prod");

    let hard = check_role_gate(
        &ConnectionRole::Subject,
        "iris_source_control:commit",
        true,
        "prod",
        true,
    )
    .expect("subject scm write blocks");
    assert_eq!(hard["error_code"], "SUBJECT_SCM_WRITE_BLOCKED");
    let out = classify(hard, &snap, "prod");
    assert_matches_schema(&out);
    assert_eq!(kind_of(&out), "fixed_rule");
}

#[test]
fn an_untrusted_data_policy_and_an_untrusted_allow_list_point_at_their_items() {
    let (snap, _t) = snapshot(
        "",
        "[policy.default]\ndataPolicy = \"allow\"\nallow = [\"query\"]\n",
    );
    let bulk = check_bulk_phi_gate(
        "journal_search",
        &iris_agentic_dev_core::iris::workspace_config::DataPolicy::Block,
        "default",
    )
    .unwrap();
    let out = classify(bulk, &snap, "default");
    assert_matches_schema(&out);
    assert_eq!(kind_of(&out), "untrusted_setting");
    assert_eq!(
        out["refusal"]["set_aside"],
        "workspace:policy.default.dataPolicy"
    );

    // `allow = ["query"]` narrows (it applies), so the compile that it blocks is an operator limit.
    let policy = ConnectionPolicy {
        server_name: "default".into(),
        allow: Some(vec![ToolCategory::Query]),
        ..Default::default()
    };
    let blocked = gated::policy("iris_compile", "default", Some(&policy)).unwrap();
    assert_eq!(
        blocked["success"], false,
        "POLICY_GATE gains success: false"
    );
    assert!(
        blocked["error"].as_str().is_some_and(|e| !e.is_empty()),
        "and an `error`"
    );
    let out = classify(blocked, &snap, "default");
    assert_matches_schema(&out);
    assert_eq!(kind_of(&out), "operator_limit");
}

#[test]
fn a_body_that_is_not_a_refusal_is_returned_unchanged() {
    let (snap, _t) = snapshot("", "x = 1\n");
    for code in [
        "SQL_ERROR",
        "UNKNOWN_PARAMETER",
        "SERVER_NOT_FOUND",
        "METHOD_RETURNED_ERROR",
        "CONFIG_INVALID",
    ] {
        let body = json!({"success": false, "error_code": code, "error": "x"});
        let out = classify(body.clone(), &snap, "default");
        assert_eq!(out, body, "{code} is not a refusal");
        assert!(out.get("refusal").is_none());
    }
}

// T-R4a-U21
#[test]
fn every_refusal_code_gets_a_refusal_and_nothing_else_does() {
    assert!(REFUSAL_CODES.len() >= 20, "{REFUSAL_CODES:?}");
    let (snap, _t) = snapshot("", "x = 1\n");
    for code in REFUSAL_CODES {
        let body = json!({"success": false, "error_code": code, "error": "x"});
        let out = classify(body, &snap, "default");
        assert_matches_schema(&out);
        assert_eq!(out["error_code"], *code, "the code is kept");
    }

    // The registry names exactly the codes the policy layer emits.
    let src_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let re = regex::Regex::new(r#""error_code":\s*"([A-Z][A-Z0-9_]+)""#).unwrap();
    let mut emitted = std::collections::BTreeSet::new();
    let files = [
        "policy/env_gate.rs",
        "policy/data_policy_gate.rs",
        "policy/system_blocklist_gate.rs",
        "policy/code_edit_gate.rs",
        "iris/server_manager.rs",
        "iris/workspace_config.rs",
    ];
    for f in files {
        let text = std::fs::read_to_string(src_dir.join(f)).unwrap();
        let text = text.split("#[cfg(test)]").next().unwrap().to_string();
        let before = emitted.len();
        for c in re.captures_iter(&text) {
            emitted.insert(c[1].to_string());
        }
        assert!(
            emitted.len() > before,
            "{f}: the scan found no error_code literal"
        );
    }
    for code in &emitted {
        assert!(
            REFUSAL_CODES.contains(&code.as_str()) || code == "CONFIG_INVALID",
            "{code} is emitted by a gate and is not in REFUSAL_CODES"
        );
    }
    for non in [
        "SQL_ERROR",
        "UNKNOWN_PARAMETER",
        "SERVER_NOT_FOUND",
        "MISSING_PARAMS",
        "NAMESPACE_NOT_FOUND",
        "METHOD_RETURNED_ERROR",
        "CONFIG_INVALID",
        "DOCKER_REQUIRED",
    ] {
        assert!(!REFUSAL_CODES.contains(&non), "{non} is not a refusal");
    }
}

#[test]
fn the_hand_check_and_the_schema_file_agree() {
    let schema: Value = serde_json::from_str(
        &std::fs::read_to_string(
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/contracts/refusal.schema.json"),
        )
        .expect("the contract file"),
    )
    .unwrap();
    let kinds: Vec<&str> = schema["properties"]["refusal"]["properties"]["kind"]["enum"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap())
        .collect();
    let ours: Vec<&str> = [
        RefusalKind::AcknowledgementRequired,
        RefusalKind::OperatorLimit,
        RefusalKind::UntrustedSetting,
        RefusalKind::FixedRule,
    ]
    .iter()
    .map(|k| k.as_str())
    .collect();
    assert_eq!(kinds, ours);
}

// FR-040: `force` is never an acknowledgement; `compile-time` refusals leave as errors too.
#[test]
fn the_compile_time_gate_refusal_is_a_refusal() {
    let (snap, _t) = snapshot("", "x = 1\n");
    let body = check_compile_time_code_mode(
        "Class A.B { Method M() [ CodeMode = objectgenerator ] { } }",
        "A.B.cls",
    )
    .expect("an object generator is refused");
    assert_eq!(body["error_code"], "COMPILE_TIME_EXEC_BLOCKED");
    let out = classify(body, &snap, "default");
    assert_matches_schema(&out);
    assert_eq!(kind_of(&out), "operator_limit");
    assert_eq!(out["refusal"]["lift"]["setting"], "compile_time_gate");
}

// T-R4a-U24
#[test]
fn set_aside_ids_are_stable_and_unique() {
    let ws = "write_tools_enabled = true\ndestructive_tools_enabled = true\n\
exec_as_os_user = true\n[policy.default]\ndataPolicy = \"allow\"\nmcpTemplate = \"dev\"\n";
    let ids = |snap: &ConfigSnapshot| -> Vec<String> {
        snap.set_aside
            .iter()
            .map(|i: &SetAsideItem| i.id.clone())
            .collect()
    };
    let (a, _t1) = snapshot("", ws);
    let (b, _t2) = snapshot("", ws);
    assert_eq!(
        ids(&a),
        ids(&b),
        "the same file gives the same ids in the same order"
    );
    let mut sorted = ids(&a);
    sorted.sort();
    sorted.dedup();
    assert_eq!(sorted.len(), ids(&a).len(), "ids are unique: {:?}", ids(&a));
    for expected in [
        "workspace:write_tools_enabled",
        "workspace:destructive_tools_enabled",
        "workspace:exec_as_os_user",
        "workspace:policy.default.dataPolicy",
        "workspace:policy.default.mcpTemplate",
    ] {
        assert!(
            ids(&a).iter().any(|i| i == expected),
            "{expected} missing: {:?}",
            ids(&a)
        );
    }
    let _ = EffectivePolicy::default();
}

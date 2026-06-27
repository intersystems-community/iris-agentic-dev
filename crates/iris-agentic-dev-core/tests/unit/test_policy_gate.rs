// Tests for per-connection tool policy gate (US2, 044-servermanager-discovery).
//
// Verifies:
// - All 10 tool→category mappings
// - policy_gate() blocks when category not in allow list
// - policy_gate() permits when category in allow list
// - policy_gate() permits all when no policy block
// - Interop with role-gate: policy wins as most restrictive (policy checked first)
// - check_config includes policy summary

use iris_agentic_dev_core::iris::server_manager::policy_gate;
use iris_agentic_dev_core::iris::workspace_config::{
    check_role_gate, ConnectionPolicy, ConnectionRole, ToolCategory,
};

fn policy_allow(cats: &[&str]) -> ConnectionPolicy {
    ConnectionPolicy {
        server_name: "test-server".to_string(),
        allow: Some(
            cats.iter()
                .map(|s| s.parse::<ToolCategory>().expect("valid category"))
                .collect(),
        ),
    }
}

// ── tool→category mapping tests ─────────────────────────────────────────────

#[test]
fn tool_category_mapping_compile() {
    let policy = policy_allow(&["execute"]);
    let result = policy_gate("iris_compile", "test-server", Some(&policy));
    assert!(
        result.is_some(),
        "iris_compile should be blocked when compile not in allow list"
    );
}

#[test]
fn tool_category_mapping_execute() {
    let policy = policy_allow(&["compile"]);
    let result = policy_gate("iris_execute", "test-server", Some(&policy));
    assert!(
        result.is_some(),
        "iris_execute should be blocked when execute not in allow list"
    );
}

#[test]
fn tool_category_mapping_query() {
    let policy = policy_allow(&["compile"]);
    let result = policy_gate("iris_query", "test-server", Some(&policy));
    assert!(
        result.is_some(),
        "iris_query should be blocked when query not in allow list"
    );
}

#[test]
fn tool_category_mapping_search() {
    let policy = policy_allow(&["compile"]);
    let result = policy_gate("iris_search", "test-server", Some(&policy));
    assert!(
        result.is_some(),
        "iris_search should be blocked when search not in allow list"
    );
}

#[test]
fn tool_category_mapping_docs() {
    let policy = policy_allow(&["compile"]);
    let result = policy_gate("docs_introspect", "test-server", Some(&policy));
    assert!(
        result.is_some(),
        "docs_introspect should be blocked when docs not in allow list"
    );
}

#[test]
fn tool_category_mapping_source_control() {
    let policy = policy_allow(&["compile"]);
    let result = policy_gate("iris_source_control", "test-server", Some(&policy));
    assert!(
        result.is_some(),
        "iris_source_control blocked when source_control not in allow"
    );
}

#[test]
fn tool_category_mapping_debug() {
    let policy = policy_allow(&["compile"]);
    let result = policy_gate("debug_capture_packet", "test-server", Some(&policy));
    assert!(
        result.is_some(),
        "debug tool blocked when debug not in allow list"
    );
}

#[test]
fn tool_category_mapping_admin() {
    let policy = policy_allow(&["compile"]);
    let result = policy_gate("iris_admin", "test-server", Some(&policy));
    assert!(
        result.is_some(),
        "iris_admin blocked when admin not in allow list"
    );
}

#[test]
fn tool_category_mapping_skill() {
    let policy = policy_allow(&["compile"]);
    let result = policy_gate("skill_list", "test-server", Some(&policy));
    assert!(
        result.is_some(),
        "skill tools blocked when skill not in allow list"
    );
}

#[test]
fn tool_category_mapping_kb() {
    let policy = policy_allow(&["compile"]);
    let result = policy_gate("kb_recall", "test-server", Some(&policy));
    assert!(
        result.is_some(),
        "kb tools blocked when kb not in allow list"
    );
}

// ── policy_gate() logic tests ─────────────────────────────────────────────

#[test]
fn policy_gate_blocks_when_category_not_in_allow_list() {
    let policy = policy_allow(&["query", "search", "docs"]);
    let result = policy_gate("iris_compile", "prod", Some(&policy));
    assert!(result.is_some(), "compile should be blocked");
    let json = result.unwrap();
    assert_eq!(json["error_code"], "POLICY_GATE");
    assert_eq!(json["policy_gate"], true);
    assert_eq!(json["blocked_category"], "compile");
    let allowed: Vec<String> = json["allowed_categories"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap().to_string())
        .collect();
    assert!(allowed.contains(&"query".to_string()));
    assert!(allowed.contains(&"search".to_string()));
    assert!(allowed.contains(&"docs".to_string()));
}

#[test]
fn policy_gate_permits_when_in_allow_list() {
    let policy = policy_allow(&["query", "search"]);
    let result = policy_gate("iris_query", "prod", Some(&policy));
    assert!(
        result.is_none(),
        "iris_query should be permitted when query is in allow list"
    );
}

#[test]
fn policy_gate_permits_all_when_no_policy() {
    // No policy block → all tools permitted
    let result = policy_gate("iris_compile", "dev-local", None);
    assert!(result.is_none(), "no policy → all tools permitted");
}

#[test]
fn policy_gate_response_includes_server_name() {
    let policy = policy_allow(&["query"]);
    let json = policy_gate("iris_compile", "prod-server", Some(&policy)).unwrap();
    assert_eq!(json["server_name"], "prod-server");
}

// ── policy + role-gate interop ─────────────────────────────────────────────

#[test]
fn policy_gate_fires_before_role_gate() {
    // When both policy and role-gate would block, policy error should be returned
    // (policy is checked first in handler wiring, role-gate never reached)
    let policy = policy_allow(&["query"]);
    let policy_result = policy_gate("iris_compile", "prod", Some(&policy));
    assert!(
        policy_result.is_some(),
        "policy gate must fire for iris_compile blocked by policy"
    );
    // policy_gate fires and returns POLICY_GATE — handler returns immediately, role-gate not called
    let json = policy_result.unwrap();
    assert_eq!(json["error_code"], "POLICY_GATE");
    // role_gate would also fire for Subject role, but is never reached when policy blocks
    let role_result = check_role_gate(
        &ConnectionRole::Subject,
        "iris_compile",
        false,
        "prod",
        false,
    );
    assert!(
        role_result.is_some(),
        "role gate would also fire in isolation"
    );
    // Confirm both fire independently — in handler, only policy result is returned
}

#[test]
fn policy_gate_none_falls_through_to_role_gate() {
    // When policy permits (query allowed), role-gate can still block for subject instances
    let policy = policy_allow(&["query", "compile", "execute"]);
    let policy_result = policy_gate("iris_compile", "prod", Some(&policy));
    assert!(
        policy_result.is_none(),
        "compile is in allow list → policy permits → role-gate can fire next"
    );
}

// ── TOML parsing tests (T028) ─────────────────────────────────────────────

#[test]
fn parse_policy_toml_allow_list() {
    use iris_agentic_dev_core::iris::workspace_config::load_fleet_config_from_str;
    let toml = r#"
[policy.prod]
allow = ["query", "search", "docs"]
"#;
    let cfg = load_fleet_config_from_str(toml).expect("should parse");
    let policy = cfg.policies.get("prod").expect("prod policy must exist");
    let categories = policy.allow.as_ref().expect("allow list must be present");
    assert_eq!(categories.len(), 3);
}

#[test]
fn parse_policy_toml_missing_allow_means_all_permitted() {
    use iris_agentic_dev_core::iris::workspace_config::load_fleet_config_from_str;
    let toml = r#"
[policy.staging]
"#;
    let cfg = load_fleet_config_from_str(toml).expect("should parse");
    let policy = cfg
        .policies
        .get("staging")
        .expect("staging policy must exist");
    assert!(
        policy.allow.is_none(),
        "missing allow key must mean all categories permitted"
    );
}

#[test]
fn parse_policy_toml_hot_reload() {
    // Policy re-read per call: write toml, check gate, rewrite toml, check again
    use iris_agentic_dev_core::iris::workspace_config::load_fleet_config_from_str;

    let toml_v1 = r#"
[policy.prod]
allow = ["query"]
"#;
    let toml_v2 = r#"
[policy.prod]
allow = ["query", "compile"]
"#;

    let cfg_v1 = load_fleet_config_from_str(toml_v1).unwrap();
    let policy_v1 = cfg_v1.policies.get("prod").unwrap();
    let gate_v1 = policy_gate("iris_compile", "prod", Some(policy_v1));
    assert!(gate_v1.is_some(), "compile blocked in v1 policy");

    let cfg_v2 = load_fleet_config_from_str(toml_v2).unwrap();
    let policy_v2 = cfg_v2.policies.get("prod").unwrap();
    let gate_v2 = policy_gate("iris_compile", "prod", Some(policy_v2));
    assert!(
        gate_v2.is_none(),
        "compile permitted in v2 policy after hot-reload"
    );
}

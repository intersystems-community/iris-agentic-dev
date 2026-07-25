// Unit tests for skills_tools.rs — param deserialization and skills_namespace logic.
// No IRIS connection needed.

use iris_agentic_dev_core::tools::skills_tools::{
    AgentInfoParams, KbParams, SkillCommunityParams, SkillParams,
};

/// `OBJECTSCRIPT_SKILLMCP_NAMESPACE` is process-global, but `cargo test` runs the tests in a
/// binary on parallel threads. Two tests mutating it concurrently means one reads the other's
/// value and fails — which is why this file only ever passed under `--test-threads=1`.
/// Serialize every test that touches the var. Poison is recovered because a panicking test
/// holding the guard must not cascade into unrelated failures.
static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

// ── SkillParams ───────────────────────────────────────────────────────────────

#[test]
fn test_skill_params_action_only() {
    let p: SkillParams = serde_json::from_str(r#"{"action": "list"}"#).unwrap();
    assert_eq!(p.action, "list");
    assert!(p.name.is_none());
    assert!(p.query.is_none());
}

#[test]
fn test_skill_params_with_name() {
    let p: SkillParams =
        serde_json::from_str(r#"{"action": "describe", "name": "compile-workflow"}"#).unwrap();
    assert_eq!(p.action, "describe");
    assert_eq!(p.name.as_deref(), Some("compile-workflow"));
}

#[test]
fn test_skill_params_with_query() {
    let p: SkillParams =
        serde_json::from_str(r#"{"action": "search", "query": "compile"}"#).unwrap();
    assert_eq!(p.action, "search");
    assert_eq!(p.query.as_deref(), Some("compile"));
}

#[test]
fn test_skill_params_missing_action_fails() {
    let result: Result<SkillParams, _> = serde_json::from_str(r#"{"name": "some-skill"}"#);
    assert!(result.is_err(), "action is required");
}

// ── SkillCommunityParams ──────────────────────────────────────────────────────

#[test]
fn test_skill_community_params_list() {
    let p: SkillCommunityParams = serde_json::from_str(r#"{"action": "list"}"#).unwrap();
    assert_eq!(p.action, "list");
    assert!(p.package.is_none());
}

#[test]
fn test_skill_community_params_install() {
    let p: SkillCommunityParams =
        serde_json::from_str(r#"{"action": "install", "package": "objectscript-basics"}"#).unwrap();
    assert_eq!(p.action, "install");
    assert_eq!(p.package.as_deref(), Some("objectscript-basics"));
}

// ── KbParams ──────────────────────────────────────────────────────────────────

#[test]
fn test_kb_params_defaults() {
    let p: KbParams = serde_json::from_str(r#"{"action": "recall", "query": "compile"}"#).unwrap();
    assert_eq!(p.action, "recall");
    assert_eq!(p.query.as_deref(), Some("compile"));
    assert_eq!(p.top_k, 5); // default_top_k
    assert!(p.path.is_none());
}

#[test]
fn test_kb_params_index_with_path() {
    let p: KbParams =
        serde_json::from_str(r#"{"action": "index", "path": "/workspace/docs"}"#).unwrap();
    assert_eq!(p.action, "index");
    assert_eq!(p.path.as_deref(), Some("/workspace/docs"));
    assert_eq!(p.top_k, 5);
}

#[test]
fn test_kb_params_custom_top_k() {
    let p: KbParams =
        serde_json::from_str(r#"{"action": "recall", "query": "test", "top_k": 10}"#).unwrap();
    assert_eq!(p.top_k, 10);
}

#[test]
fn test_kb_params_missing_action_fails() {
    let result: Result<KbParams, _> = serde_json::from_str(r#"{"query": "test"}"#);
    assert!(result.is_err(), "action is required");
}

// ── AgentInfoParams ───────────────────────────────────────────────────────────

#[test]
fn test_agent_info_params_defaults() {
    let p: AgentInfoParams = serde_json::from_str(r#"{"what": "stats"}"#).unwrap();
    assert_eq!(p.what, "stats");
    assert_eq!(p.limit, 20); // default_limit
}

#[test]
fn test_agent_info_params_history_with_custom_limit() {
    let p: AgentInfoParams = serde_json::from_str(r#"{"what": "history", "limit": 50}"#).unwrap();
    assert_eq!(p.what, "history");
    assert_eq!(p.limit, 50);
}

#[test]
fn test_agent_info_params_missing_what_fails() {
    let result: Result<AgentInfoParams, _> = serde_json::from_str(r#"{"limit": 5}"#);
    assert!(result.is_err(), "what is required");
}

// ── skills_namespace env var ──────────────────────────────────────────────────

#[test]
fn test_skills_namespace_default() {
    let _g = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    // Remove env var if set, verify fallback
    std::env::remove_var("OBJECTSCRIPT_SKILLMCP_NAMESPACE");
    let ns = iris_agentic_dev_core::tools::skills_tools::skills_namespace();
    assert_eq!(ns, "USER");
}

#[test]
fn test_skills_namespace_env_override() {
    let _g = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    std::env::set_var("OBJECTSCRIPT_SKILLMCP_NAMESPACE", "SKILLS");
    let ns = iris_agentic_dev_core::tools::skills_tools::skills_namespace();
    // Read before restoring so a failure here cannot leak the var to other tests.
    std::env::remove_var("OBJECTSCRIPT_SKILLMCP_NAMESPACE");
    assert_eq!(ns, "SKILLS");
}

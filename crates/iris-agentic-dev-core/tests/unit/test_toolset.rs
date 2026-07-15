// T015–T027: Toolset unit tests.
// Tests for Nostub and Merged toolset configurations.
// Written FIRST — must FAIL until T017–T033 are implemented.

use iris_agentic_dev_core::tools::{IrisTools, Toolset};

// ── Toolset::from_str ────────────────────────────────────────────────────────

#[test]
fn test_toolset_from_str_baseline() {
    assert_eq!(Toolset::from_str("baseline"), Toolset::Baseline);
    assert_eq!(Toolset::from_str(""), Toolset::Baseline);
    assert_eq!(Toolset::from_str("unknown"), Toolset::Baseline);
}

#[test]
fn test_toolset_from_str_nostub() {
    assert_eq!(Toolset::from_str("nostub"), Toolset::Nostub);
    assert_eq!(Toolset::from_str("NOSTUB"), Toolset::Nostub);
}

#[test]
fn test_toolset_from_str_merged() {
    assert_eq!(Toolset::from_str("merged"), Toolset::Merged);
    assert_eq!(Toolset::from_str("MERGED"), Toolset::Merged);
}

// ── T015: Nostub — stub tools absent ────────────────────────────────────────

/// iris_symbols_local is now a real tool (025-symbols-local-ts) — must be present in nostub.
#[test]
fn test_nostub_excludes_iris_symbols_local() {
    let tools = IrisTools::new_with_toolset(None, Toolset::Nostub).expect("IrisTools::new");
    let names = tools.registered_tool_names();
    assert!(
        names.contains("iris_symbols_local"),
        "iris_symbols_local must be registered in nostub toolset (no longer a stub). Found symbols tools: {:?}",
        names
            .iter()
            .filter(|n| n.contains("symbol"))
            .collect::<Vec<_>>()
    );
}

/// skill tool must not expose propose/optimize/share actions in nostub (FR-005).
#[test]
fn test_nostub_skill_excludes_stub_actions() {
    let tools = IrisTools::new_with_toolset(None, Toolset::Nostub).expect("IrisTools::new");
    let names = tools.registered_tool_names();
    for stub_action in &["skill_propose", "skill_optimize", "skill_share"] {
        assert!(
            !names.contains(*stub_action),
            "{} must not be registered in nostub toolset",
            stub_action
        );
    }
}

/// skill_community must not expose install action in nostub (FR-006).
#[test]
fn test_nostub_skill_community_excludes_install() {
    let tools = IrisTools::new_with_toolset(None, Toolset::Nostub).expect("IrisTools::new");
    let names = tools.registered_tool_names();
    assert!(
        !names.contains("skill_community_install"),
        "skill_community_install must not be registered in nostub toolset"
    );
}

/// Nostub must preserve all non-stub tools (not accidentally remove real ones).
#[test]
fn test_nostub_preserves_core_tools() {
    let tools = IrisTools::new_with_toolset(None, Toolset::Nostub).expect("IrisTools::new");
    let names = tools.registered_tool_names();
    for required in &[
        "iris_compile",
        "iris_execute",
        "iris_doc",
        "iris_query",
        "iris_symbols",
        "docs_introspect",
        "iris_search",
        "iris_info",
    ] {
        assert!(
            names.contains(*required),
            "Core tool {} must still be registered in nostub toolset",
            required
        );
    }
}

/// Nostub should have exactly 4 fewer tools than baseline
/// (skill_propose + skill_optimize + skill_share + skill_community_install = 4 stubs removed).
/// iris_symbols_local is no longer a stub (025-symbols-local-ts).
#[test]
fn test_nostub_tool_count() {
    let baseline = IrisTools::new_with_toolset(None, Toolset::Baseline)
        .expect("baseline IrisTools")
        .registered_tool_names()
        .len();
    let nostub = IrisTools::new_with_toolset(None, Toolset::Nostub)
        .expect("nostub IrisTools")
        .registered_tool_names()
        .len();
    assert_eq!(
        nostub,
        baseline - 4,
        "Nostub should have exactly 4 fewer tools than baseline (got baseline={}, nostub={})",
        baseline,
        nostub
    );
}

// ── T020–T027: Merged — parity stubs (full parity tests require live IRIS) ──

/// iris_debug must be registered in merged toolset (FR-007).
#[test]
fn test_merged_registers_iris_debug() {
    let tools = IrisTools::new_with_toolset(None, Toolset::Merged).expect("IrisTools::new");
    let names = tools.registered_tool_names();
    assert!(
        names.contains("iris_debug"),
        "iris_debug must be registered in merged toolset. Found tools: {:?}",
        names
            .iter()
            .filter(|n| n.contains("debug"))
            .collect::<Vec<_>>()
    );
}

/// iris_production must be registered in merged toolset (FR-008).
#[test]
fn test_merged_registers_iris_production() {
    let tools = IrisTools::new_with_toolset(None, Toolset::Merged).expect("IrisTools::new");
    let names = tools.registered_tool_names();
    assert!(
        names.contains("iris_production"),
        "iris_production must be registered in merged toolset"
    );
}

/// iris_interop_query must be registered in merged toolset (FR-009).
#[test]
fn test_merged_registers_iris_interop_query() {
    let tools = IrisTools::new_with_toolset(None, Toolset::Merged).expect("IrisTools::new");
    let names = tools.registered_tool_names();
    assert!(
        names.contains("iris_interop_query"),
        "iris_interop_query must be registered in merged toolset"
    );
}

/// iris_containers must be registered in merged toolset (FR-010).
#[test]
fn test_merged_registers_iris_containers() {
    let tools = IrisTools::new_with_toolset(None, Toolset::Merged).expect("IrisTools::new");
    let names = tools.registered_tool_names();
    assert!(
        names.contains("iris_containers"),
        "iris_containers must be registered in merged toolset"
    );
}

/// agent_info must NOT be registered in merged toolset (FR-011).
#[test]
fn test_merged_excludes_agent_info() {
    let tools = IrisTools::new_with_toolset(None, Toolset::Merged).expect("IrisTools::new");
    let names = tools.registered_tool_names();
    assert!(
        !names.contains("agent_info"),
        "agent_info must not be registered in merged toolset"
    );
}

/// Merged must exclude all original debug tools (replaced by iris_debug).
#[test]
fn test_merged_excludes_original_debug_tools() {
    let tools = IrisTools::new_with_toolset(None, Toolset::Merged).expect("IrisTools::new");
    let names = tools.registered_tool_names();
    for replaced in &[
        "debug_capture_packet",
        "debug_get_error_logs",
        "debug_map_int_to_cls",
        "debug_source_map",
    ] {
        assert!(
            !names.contains(*replaced),
            "{} must not be registered in merged toolset (replaced by iris_debug)",
            replaced
        );
    }
}

/// Merged must exclude all original interop production tools (replaced by iris_production).
#[test]
fn test_merged_excludes_original_interop_production_tools() {
    let tools = IrisTools::new_with_toolset(None, Toolset::Merged).expect("IrisTools::new");
    let names = tools.registered_tool_names();
    for replaced in &[
        "interop_production_status",
        "interop_production_start",
        "interop_production_stop",
        "interop_production_update",
        "interop_production_needs_update",
        "interop_production_recover",
    ] {
        assert!(
            !names.contains(*replaced),
            "{} must not be registered in merged toolset (replaced by iris_production)",
            replaced
        );
    }
}

/// Merged must have exactly 38 tools (added iris_message_body, iris_business_rule_info,
/// iris_production_diff in 056-interop-depth).
#[test]
fn test_merged_tool_count_is_23() {
    let tools = IrisTools::new_with_toolset(None, Toolset::Merged).expect("IrisTools::new");
    let count = tools.registered_tool_names().len();
    assert_eq!(
        count, 40,
        "Merged toolset must have exactly 40 tools (38 + telemetry_query + telemetry_export_trace from 059-tool-telemetry-benchmark), got {}",
        count
    );
    // iris_get_log must be registered in Merged (027-progressive-disclosure)
    assert!(
        tools.registered_tool_names().contains("iris_get_log"),
        "iris_get_log must appear in Merged toolset"
    );
    // iris_execute_method must be registered in Merged (053-doc-depth)
    assert!(
        tools
            .registered_tool_names()
            .contains("iris_execute_method"),
        "iris_execute_method must appear in Merged toolset"
    );
}

/// iris_get_log must NOT be registered in Baseline or Nostub (027-progressive-disclosure).
#[test]
fn test_iris_get_log_absent_from_baseline_and_nostub() {
    let baseline = IrisTools::new_with_toolset(None, Toolset::Baseline).expect("IrisTools::new");
    assert!(
        !baseline.registered_tool_names().contains("iris_get_log"),
        "iris_get_log must NOT appear in Baseline toolset"
    );
    let nostub = IrisTools::new_with_toolset(None, Toolset::Nostub).expect("IrisTools::new");
    assert!(
        !nostub.registered_tool_names().contains("iris_get_log"),
        "iris_get_log must NOT appear in Nostub toolset"
    );
}

// ── IRIS_DISABLED_TOOLS env-var filtering ─────────────────────────────────────

// Serialize tests that set/remove env vars
static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

#[test]
fn test_disabled_tools_env_removes_named_tool() {
    let _lock = ENV_LOCK.lock().unwrap();
    std::env::set_var("IRIS_DISABLED_TOOLS", "iris_source_control");
    let tools = IrisTools::new_with_toolset(None, Toolset::Baseline).expect("IrisTools::new");
    let names = tools.registered_tool_names();
    std::env::remove_var("IRIS_DISABLED_TOOLS");
    assert!(
        !names.contains("iris_source_control"),
        "iris_source_control must be absent when in IRIS_DISABLED_TOOLS"
    );
}

#[test]
fn test_disabled_tools_env_removes_multiple_tools() {
    let _lock = ENV_LOCK.lock().unwrap();
    std::env::set_var("IRIS_DISABLED_TOOLS", "iris_admin,iris_credential_manage");
    let tools = IrisTools::new_with_toolset(None, Toolset::Nostub).expect("IrisTools::new");
    let names = tools.registered_tool_names();
    std::env::remove_var("IRIS_DISABLED_TOOLS");
    assert!(!names.contains("iris_admin"), "iris_admin must be absent");
    assert!(
        !names.contains("iris_credential_manage"),
        "iris_credential_manage must be absent"
    );
}

#[test]
fn test_disabled_tools_env_empty_string_removes_nothing() {
    let _lock = ENV_LOCK.lock().unwrap();
    std::env::set_var("IRIS_DISABLED_TOOLS", "");
    let tools = IrisTools::new_with_toolset(None, Toolset::Baseline).expect("IrisTools::new");
    let names = tools.registered_tool_names();
    std::env::remove_var("IRIS_DISABLED_TOOLS");
    // Core tools must still be present
    assert!(
        names.contains("iris_execute"),
        "iris_execute must remain when disabled list is empty"
    );
    assert!(
        names.contains("iris_query"),
        "iris_query must remain when disabled list is empty"
    );
}

#[test]
fn test_disabled_tools_env_ignores_whitespace() {
    let _lock = ENV_LOCK.lock().unwrap();
    std::env::set_var(
        "IRIS_DISABLED_TOOLS",
        " iris_source_control , iris_compile ",
    );
    let tools = IrisTools::new_with_toolset(None, Toolset::Baseline).expect("IrisTools::new");
    let names = tools.registered_tool_names();
    std::env::remove_var("IRIS_DISABLED_TOOLS");
    assert!(
        !names.contains("iris_source_control"),
        "whitespace-padded name must still be removed"
    );
    assert!(
        !names.contains("iris_compile"),
        "whitespace-padded name must still be removed"
    );
}

#[test]
fn test_disabled_tools_env_unknown_name_is_ignored() {
    let _lock = ENV_LOCK.lock().unwrap();
    std::env::set_var("IRIS_DISABLED_TOOLS", "nonexistent_tool");
    // Should not panic — remove_route on a name that doesn't exist must be a no-op
    let result = IrisTools::new_with_toolset(None, Toolset::Baseline);
    std::env::remove_var("IRIS_DISABLED_TOOLS");
    assert!(
        result.is_ok(),
        "unknown disabled tool name must not cause construction to fail"
    );
}

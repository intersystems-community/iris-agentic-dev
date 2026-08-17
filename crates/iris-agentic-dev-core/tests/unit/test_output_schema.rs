//! Regression test: tools with a declared output schema (076-interface-modernization,
//! User Story 1) actually advertise it via `list_tools`, and it stays declared regardless
//! of which toolset (Baseline/Nostub/Merged) is active.
//!
//! This is a static-router check, not a live-IRIS one — it never calls a tool, so it
//! belongs alongside the other pure-logic tests in this file's neighbors
//! (`test_toolset.rs`, `test_tool_category_coverage.rs`), no container required.

use iris_agentic_dev_core::tools::{IrisTools, Toolset};

/// Every tool given an `output_schema` attribute so far — see
/// `crates/iris-agentic-dev-core/src/tools/output_schemas.rs` for the response shapes and the
/// reasoning behind which tools are (and are not yet) covered.
const TOOLS_WITH_DECLARED_OUTPUT_SCHEMA: &[&str] = &[
    "iris_servers",
    "skill_list",
    "skill_community_list",
    "skill_forget",
    "agent_stats",
    "agent_history",
    "kb_recall",
    "iris_symbols",
    "iris_symbols_local",
    "docs_introspect",
    "debug_map_int_to_cls",
    "debug_source_map",
    "iris_ws_open",
    "iris_ws_exec",
    "iris_ws_close",
    // batch 2
    "debug_capture_packet",
    "debug_get_error_logs",
    "iris_add_server",
    "iris_remove_server",
    "iris_test_server",
    "iris_import_servers",
    "global_kill",
    "iris_namespace_list",
    "iris_database_list",
    "iris_namespace_create",
    "iris_database_stats",
    "my_access",
    "capability_matrix",
    "hl7_schema_list",
    "journal_search",
    // batch 3
    "compare_document",
    "compare_namespace",
    "global_preview",
    "query_audit_log",
    "stream_inspect",
    "hl7_schema_inspect",
    "mermaid_class",
    "mermaid_production",
    "skill_propose",
    "skill_optimize",
    "skill_share",
    "skill_community_install",
    "telemetry_query",
    "telemetry_export_trace",
    "iris_credential_list",
    // batch 4
    "resolve_dynamic_dispatch",
    "find_subclass_implementations",
    "skill_describe",
    "skill_search",
    "iris_get_log",
    "agent_info",
    "kb",
    "kb_index",
    "iris_credential_manage",
    "iris_lookup_manage",
    "iris_lookup_transfer",
];

/// Tools legitimately absent from the Merged toolset entirely (not "present but missing a
/// schema"), so excluded from the Merged-only check — either consolidated into `iris_debug`
/// (the debug_* quartet), pruned as stub tools for any non-Baseline toolset (the skill_*
/// quartet — see `with_registry_and_toolset`'s `stubs_to_remove`), or replaced by `iris_containers`
/// in Merged (`agent_info`, per the same block's `merged_replaced` list).
const MERGED_REMOVED: &[&str] = &[
    "debug_map_int_to_cls",
    "debug_source_map",
    "debug_capture_packet",
    "debug_get_error_logs",
    "skill_propose",
    "skill_optimize",
    "skill_share",
    "skill_community_install",
    "agent_info",
];

/// The mirror image of `MERGED_REMOVED`: tools that exist only in Merged, not Baseline/Nostub
/// (`with_registry_and_toolset`'s `merged_only` removal list) — excluded from the Baseline-only
/// check for the same reason, just the opposite direction.
const BASELINE_REMOVED: &[&str] = &["iris_get_log"];

#[test]
fn test_declared_tools_advertise_output_schema_in_baseline() {
    let tools = IrisTools::new_with_toolset(None, Toolset::Baseline).expect("IrisTools::new");
    for name in TOOLS_WITH_DECLARED_OUTPUT_SCHEMA
        .iter()
        .filter(|n| !BASELINE_REMOVED.contains(n))
    {
        assert!(
            tools.tool_declares_output_schema(name),
            "'{name}' should declare a non-null output_schema in Baseline's list_tools"
        );
    }
}

#[test]
fn test_declared_tools_advertise_output_schema_in_merged() {
    let tools = IrisTools::new_with_toolset(None, Toolset::Merged).expect("IrisTools::new");
    for name in TOOLS_WITH_DECLARED_OUTPUT_SCHEMA
        .iter()
        .filter(|n| !MERGED_REMOVED.contains(n))
    {
        assert!(
            tools.tool_declares_output_schema(name),
            "'{name}' should declare a non-null output_schema in Merged's list_tools too"
        );
    }
}

/// `MERGED_REMOVED` tools must be legitimately absent from Merged (not silently missing a
/// schema) — confirms the exclusion is real, not papering over a bug.
#[test]
fn test_merged_removed_tools_are_absent_from_merged_router() {
    let merged = IrisTools::new_with_toolset(None, Toolset::Merged)
        .expect("IrisTools::new")
        .registered_tool_names();
    for name in MERGED_REMOVED {
        assert!(
            !merged.contains(*name),
            "'{name}' was expected to be absent from Merged, but is still present"
        );
    }
}

/// Mirror of the above for `BASELINE_REMOVED` — `iris_get_log` must be genuinely Merged-only,
/// not just skipped in the Baseline check because someone assumed it without verifying.
#[test]
fn test_baseline_removed_tools_are_absent_from_baseline_router() {
    let baseline = IrisTools::new_with_toolset(None, Toolset::Baseline)
        .expect("IrisTools::new")
        .registered_tool_names();
    for name in BASELINE_REMOVED {
        assert!(
            !baseline.contains(*name),
            "'{name}' was expected to be Merged-only, but is present in Baseline"
        );
    }
}

#[test]
fn test_a_tool_without_a_declared_schema_reports_false_not_a_panic() {
    // iris_compile hasn't been given an output_schema yet (spec 076 US1 batch 2+) — confirms
    // the accessor distinguishes "no schema" from "not found" without special-casing either.
    let tools = IrisTools::new_with_toolset(None, Toolset::Baseline).expect("IrisTools::new");
    assert!(!tools.tool_declares_output_schema("iris_compile"));
    assert!(!tools.tool_declares_output_schema("not_a_real_tool_name"));
}

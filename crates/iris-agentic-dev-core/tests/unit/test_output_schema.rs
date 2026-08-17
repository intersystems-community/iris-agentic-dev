//! Regression test: tools with a declared output schema (076-interface-modernization,
//! User Story 1) actually advertise it via `list_tools`, and it stays declared regardless
//! of which toolset (Baseline/Nostub/Merged) is active.
//!
//! This is a static-router check, not a live-IRIS one — it never calls a tool, so it
//! belongs alongside the other pure-logic tests in this file's neighbors
//! (`test_toolset.rs`, `test_tool_category_coverage.rs`), no container required.
//!
//! As of batch 22 (`check_config`), this list covers all 90 registered tools — User
//! Story 1 is complete. The batch-by-batch grouping below is left as-is rather than
//! flattened, since it's the same order-of-discovery record `output_schemas.rs`'s own
//! comment headers keep.

use iris_agentic_dev_core::tools::{IrisTools, Toolset};

/// Every tool given an `output_schema` attribute — see
/// `crates/iris-agentic-dev-core/src/tools/output_schemas.rs` for the response shapes and the
/// reasoning behind each one's design. All 90 registered tools are covered.
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
    // batch 5
    "iris_list_containers",
    "iris_select_container",
    "iris_start_sandbox",
    "iris_generate_class",
    "iris_generate_test",
    "resolve_storage",
    "iris_info",
    "iris_table_info",
    "iris_doc_search",
    "iris_message_body",
    "iris_business_rule_info",
    "iris_production_diff",
    // batch 6
    "iris_execute_method",
    "iris_macro",
    "iris_debug",
    "iris_generate",
    "skill",
    "skill_community",
    // batch 7
    "iris_query",
    // batch 8
    "iris_compile",
    // batch 9
    "iris_test",
    // batch 10
    "iris_execute",
    // batch 11
    "iris_doc",
    // batch 12
    "iris_coverage",
    // batch 13
    "iris_global",
    // batch 14
    "iris_source_control",
    // batch 15
    "iris_containers",
    // batch 16
    "iris_interop_query",
    // batch 17
    "iris_production_item",
    // batch 18
    "iris_production",
    // batch 19
    "iris_admin",
    // batch 20
    "extract_message_map_routing",
    // batch 21
    "iris_search",
    // batch 22 — the 90th and last tool
    "check_config",
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
    "iris_list_containers",
    "iris_select_container",
    "iris_start_sandbox",
];

/// The mirror image of `MERGED_REMOVED`: tools that exist only in Merged, not Baseline/Nostub
/// (`with_registry_and_toolset`'s `merged_only` removal list) — excluded from the Baseline-only
/// check for the same reason, just the opposite direction.
const BASELINE_REMOVED: &[&str] = &[
    "iris_get_log",
    "iris_message_body",
    "iris_business_rule_info",
    "iris_production_diff",
    "iris_execute_method",
    "iris_debug",
    "iris_global",
    "iris_containers",
    "iris_admin",
];

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
    // Every real, registered tool now has a declared output schema — batch 22
    // (`check_config`) was the last one without it, and picking a fresh "known
    // undeclared" example each batch (iris_compile → iris_test → iris_execute →
    // check_config) finally ran out of tools. There is no longer a genuine
    // "registered but undeclared" case to assert against, so this narrows to the
    // accessor's not-found path alone: a name that was never a real tool must report
    // `false`, not panic or silently default to `true`.
    let tools = IrisTools::new_with_toolset(None, Toolset::Baseline).expect("IrisTools::new");
    assert!(!tools.tool_declares_output_schema("not_a_real_tool_name"));
}

#[test]
fn test_all_ninety_tools_are_declared() {
    // User Story 1 (076-interface-modernization) closes here: every one of the 90
    // registered tools across both toolsets has a real `output_schema` attribute. If this
    // fails, either a new tool was added without a schema, or this count needs updating —
    // check `output_schemas.rs`'s batch history before assuming either.
    assert_eq!(
        TOOLS_WITH_DECLARED_OUTPUT_SCHEMA.len(),
        90,
        "expected exactly 90 declared tools"
    );
}

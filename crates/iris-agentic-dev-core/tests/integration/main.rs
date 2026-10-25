//! Aggregated integration-test target.
//!
//! Tests here require a real IRIS instance. They share one process now, so a test that sets an
//! env var can be seen by the next one — `--test-threads=1` is mandatory for this target.
//!
//! These 54 files were 54 separate `[[test]]` targets. Cargo runs test binaries one after another,
//! so each was a process spawn charged to every run. As modules of one binary they spawn once.
//!
//! Add a file here and add its `mod` line below, or it will not run.

// Runs before `main`, so before any test thread: no test in this binary reaches the developer's
// real keychain or home directory. See `testing::isolate_process`.
#[ctor::ctor]
fn isolate_the_test_process() {
    iris_agentic_dev_core::testing::isolate_process();
}

mod enum_rejection;
mod nopws_101;
mod params_batch1;
mod params_batch2;
mod params_batch3;
mod params_batch4;
mod params_batch5;
mod params_batch6;
mod params_batch7;
mod rejection_order;
mod sc007_schema_only;
mod subclass_impl_live;
mod support_identity_141;
mod test_admin_api_guardrail_live;
mod test_admin_e2e;
mod test_aihub_139_live;
mod test_attribution_live;
mod test_benchmark_live;
mod test_cmd_live;
mod test_codemode_gate_live;
mod test_comparison_e2e;
mod test_compile_cmd;
mod test_compile_gate_141_live;
mod test_compile_widen_141_live;
mod test_content_skills_130_live;
mod test_coverage_diag_141_live;
mod test_coverage_dispatch_141_live;
mod test_coverage_live;
mod test_creds_141_live;
mod test_delete_callback_oid_live;
mod test_discovery_docker_live;
mod test_dispatch_formal_spec_live;
mod test_dispatch_gate_handlers;
mod test_doc_live;
mod test_doc_search_live;
mod test_e2e;
mod test_e2e_all_tools;
mod test_environment_restriction_live;
mod test_exec_sql_131_live;
mod test_execute_runs_once_live;
mod test_fresh_container_setup_live;
mod test_gate_enforcement_live;
mod test_generate_class_141_live;
mod test_generator_device_live;
mod test_generator_false_success_live;
mod test_global_name_gates_052_live;
mod test_global_value_141_live;
mod test_handlers_live;
mod test_hints_replay_129_live;
mod test_host_paths_141_live;
mod test_identity_baseline_141_live;
mod test_identity_paths_141_live;
mod test_injection_137_live;
mod test_interop_depth_live;
mod test_interop_ui_136_live;
mod test_iris_admin_observability_live;
mod test_iris_audit_live;
mod test_iris_doc_depth_live;
mod test_iris_global_live;
mod test_iris_test_e2e;
mod test_isolation_141;
mod test_journal_redact_141_live;
mod test_journal_term_141_live;
mod test_journal_walk_141_live;
mod test_live_reload_e2e;
mod test_login_guard_141_live;
mod test_lookup_empty_141_live;
mod test_low_items_141_live;
mod test_mcp_iris;
mod test_mdx_131_live;
mod test_mdx_142_samples_bi_live;
mod test_message_body_redact_141_live;
mod test_mirror_and_freespace;
mod test_query_stats_139_live;
mod test_release_160_live;
mod test_retry;
mod test_role_gate_e2e;
mod test_scm;
mod test_scm_action_codes_141_live;
mod test_scratch_cancel_130;
mod test_scratch_cleanup_141_live;
mod test_search_live;
mod test_server_pool_e2e;
mod test_service_account_http_141_live;
mod test_setup_skills_141_live;
mod test_skill_facts_127_live;
mod test_skill_install_e2e;
mod test_skill_symbols_live;
mod test_sm_e2e;
mod test_sql_lexer_141_live;
mod test_sql_param_audit_141_live;
mod test_sql_power_live;
mod test_sql_safety_e2e;
mod test_sql_translate_e2e;
mod test_stream_inspect_141_live;
mod test_superclass_141_live;
mod test_system_mode_bodies_141_live;
mod test_system_mode_live;
mod test_target_gates_141_live;
mod test_telemetry_live;
mod test_telemetry_sink_141_live;
mod test_terminal_compat_096;
mod test_throwaway_141_live;
mod test_todo_example_live;
mod test_tool_fixes_132_live;
mod test_trace_export_live;
mod test_unit_test_root_141_live;
mod test_unittest_device_race_live;
mod test_web_prefix_live;
mod test_ws_e2e;
mod test_ws_exec_gate_live;

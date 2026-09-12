//! Aggregated unit-test target.
//!
//! These 99 files were 99 separate `[[test]]` targets. Cargo never runs test binaries in
//! parallel, so each one cost a process spawn: a warm run of the non-live suite spent ~170s of
//! its 254s starting processes rather than running tests, and a no-op build spent 44s just
//! walking the target list. As modules of one binary they spawn once and the in-binary tests run
//! on all cores.
//!
//! Add a file here and add its `mod` line below, or it will not run — the aggregator is the only
//! thing that pulls these in.

mod handler_keys;
mod test_audit_log;
mod test_audit_phi_scrub;
mod test_benchmark_run_task_errors;
mod test_benchmark_scoring;
mod test_benchmark_task_loading;
mod test_bundled_skills;
mod test_cargo_config;
mod test_cli_dispatch_helpers;
mod test_code_edit_gate_unit;
mod test_compile_params;
mod test_connection_fixes;
mod test_coverage_gaps;
mod test_coverage_unit;
mod test_coverage_wave3;
mod test_data_policy_gate;
mod test_dict_unit;
mod test_discovery_probe_unit;
mod test_discovery_unit;
mod test_dispatch_gate;
mod test_dml_sql_unit;
mod test_doc_params;
mod test_doc_search_unit;
mod test_doc_unit2;
mod test_docs_contract;
mod test_elicitation;
mod test_elicitation_sweep;
mod test_enum_contract;
mod test_env_gate;
mod test_fresh_container_setup_unit;
mod test_gate_check;
mod test_gate_classification;
mod test_gate_resolution;
mod test_gate_uncovered_paths;
mod test_generate_unit;
mod test_generator_false_success;
mod test_info_unit;
mod test_interop_depth_unit;
mod test_iris_admin_observability_unit;
mod test_iris_audit;
mod test_iris_doc_depth_unit;
mod test_iris_global_unit;
mod test_iris_production_params;
mod test_iris_test_http;
mod test_lift_math;
mod test_list_tools_pagination;
mod test_live_reload;
mod test_llm_usage;
mod test_lockfile_sync;
mod test_mcp_peer_identity;
mod test_no_tracked_local_config;
mod test_nopws_detection;
mod test_nopws_execute;
mod test_output_schema;
mod test_output_schema_shapes;
mod test_param_rejection;
mod test_params_schema;
mod test_perf_monitoring;
mod test_policy_audit_config;
mod test_policy_gate;
mod test_policy_patterns;
mod test_probe_server_offline;
mod test_reload_pool;
mod test_role_gate;
mod test_role_gate_handlers;
mod test_schema_tasks;
mod test_scm_escaping;
mod test_scm_unit;
mod test_search_unit;
mod test_server_entry_plaintext;
mod test_server_manager;
mod test_skill_discovery_tools;
mod test_skill_frontmatter;
mod test_skill_install;
mod test_skill_manifest_sync;
mod test_skills_mod_unit;
mod test_skills_unit;
mod test_skills_unit_gaps;
mod test_sql_power_unit;
mod test_sql_safety;
mod test_sql_translate;
mod test_structured_content;
mod test_suppress_description;
mod test_system_blocklist;
mod test_telemetry_local_io;
mod test_telemetry_prune;
mod test_telemetry_query_filter;
mod test_telemetry_redact;
mod test_telemetry_types;
mod test_test_target_layout;
mod test_testing_helpers;
mod test_tls_trust_127;
mod test_tool_catalogue;
mod test_tool_category_coverage;
mod test_tools_fixes;
mod test_tools_mod_unit;
mod test_toolset;
mod test_trace_export;
mod test_user_agent;
mod test_vscode_payload;
mod test_workflow_yaml;
mod test_workspace_config;
mod test_workspace_config_paths;
mod test_ws_exec_gate;

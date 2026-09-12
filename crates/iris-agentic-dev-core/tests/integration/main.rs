//! Aggregated integration-test target.
//!
//! Tests here require a real IRIS instance. They share one process now, so a test that sets an
//! env var can be seen by the next one — `--test-threads=1` is mandatory for this target.
//!
//! These 54 files were 54 separate `[[test]]` targets. Cargo runs test binaries one after another,
//! so each was a process spawn charged to every run. As modules of one binary they spawn once.
//!
//! Add a file here and add its `mod` line below, or it will not run.

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
mod test_admin_e2e;
mod test_attribution_live;
mod test_benchmark_live;
mod test_cmd_live;
mod test_codemode_gate_live;
mod test_comparison_e2e;
mod test_compile_cmd;
mod test_coverage_live;
mod test_discovery_docker_live;
mod test_dispatch_gate_handlers;
mod test_doc_live;
mod test_doc_search_live;
mod test_e2e;
mod test_e2e_all_tools;
mod test_environment_restriction_live;
mod test_fresh_container_setup_live;
mod test_gate_enforcement_live;
mod test_generator_device_live;
mod test_generator_false_success_live;
mod test_handlers_live;
mod test_interop_depth_live;
mod test_iris_admin_observability_live;
mod test_iris_audit_live;
mod test_iris_doc_depth_live;
mod test_iris_global_live;
mod test_iris_test_e2e;
mod test_live_reload_e2e;
mod test_mcp_iris;
mod test_mirror_and_freespace;
mod test_retry;
mod test_role_gate_e2e;
mod test_scm;
mod test_search_live;
mod test_server_pool_e2e;
mod test_skill_install_e2e;
mod test_sm_e2e;
mod test_sql_power_live;
mod test_sql_safety_e2e;
mod test_sql_translate_e2e;
mod test_telemetry_live;
mod test_terminal_compat_096;
mod test_trace_export_live;
mod test_web_prefix_live;
mod test_ws_e2e;
mod test_ws_exec_gate_live;

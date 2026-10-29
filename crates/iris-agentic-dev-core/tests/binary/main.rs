//! Aggregated binary-invocation-test target.
//!
//! Tests here spawn `iris-agentic-dev` as a subprocess and talk JSON-RPC over stdio. No IRIS needed.
//!
//! These 14 files were 14 separate `[[test]]` targets. Cargo runs test binaries one after another,
//! so each was a process spawn charged to every run. As modules of one binary they spawn once.
//!
//! Add a file here and add its `mod` line below, or it will not run.

// Runs before `main`, so before any test thread: no test in this binary reaches the developer's
// real keychain or home directory. See `testing::isolate_process`.
#[ctor::ctor]
fn isolate_the_test_process() {
    iris_agentic_dev_core::testing::isolate_process();
}

mod check_ascot;
mod cli_discovery;
mod coding_pack_129;
mod gate_normalize_163;
mod global_name_gates_052;
mod include_scratch_130;
mod invalid_params;
mod macro_130;
mod nopws_101;
mod pool_reload_hint;
mod rejection_message;
mod sa_schema_131;
mod schema_batch1;
mod schema_batch2;
mod schema_batch3;
mod schema_batch4;
mod schema_batch5;
mod schema_batch6;
mod schema_batch7;
mod schema_census;
mod schema_task_coverage;
mod servers_json_web_prefix;
mod support_mcp_141;
mod suppress_description;
mod test_audit_mode_binary_141;
mod test_clean_cwd_141;
mod test_config_source_141;
mod test_creds_binary_141;
mod test_generate_class_141;
mod test_identity_binary_141;
mod test_install_lock_binary_141;
mod test_operator_latch_141;
mod test_read_only_strict_141;
mod test_scm_opt_ins_141;
mod test_server_probe_141;
mod test_skill_override_141;
mod test_target_gates_141;
mod test_telemetry_sink_binary_141;
mod test_trust_binary_141;
mod tls_trust_127;
mod ws_exec_gate;

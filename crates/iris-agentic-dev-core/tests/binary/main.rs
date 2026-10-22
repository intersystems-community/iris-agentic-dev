//! Aggregated binary-invocation-test target.
//!
//! Tests here spawn `iris-agentic-dev` as a subprocess and talk JSON-RPC over stdio. No IRIS needed.
//!
//! These 14 files were 14 separate `[[test]]` targets. Cargo runs test binaries one after another,
//! so each was a process spawn charged to every run. As modules of one binary they spawn once.
//!
//! Add a file here and add its `mod` line below, or it will not run.

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
mod suppress_description;
mod tls_trust_127;
mod ws_exec_gate;

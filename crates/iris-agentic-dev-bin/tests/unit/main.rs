//! Aggregated unit-test target for the binary crate.
//!
//! These 10 files were 10 separate `[[test]]` targets. Cargo never runs test binaries in parallel,
//! so each one cost a process spawn charged to every run — the same tax that put 85% of a warm
//! core-crate run into starting processes rather than running tests. As modules of one binary they
//! spawn once and the in-binary tests run on all cores.
//!
//! Add a file here and add its `mod` line below, or it will not run — the aggregator is the only
//! thing that pulls these in. The core crate's `tests/unit/test_test_target_layout.rs` reaches
//! across into this crate and fails when a file is missing its line.

// Runs before `main`, so before any test thread: no test in this binary reaches the developer's
// real keychain or home directory. See `testing::isolate_process`.
#[ctor::ctor]
fn isolate_the_test_process() {
    iris_agentic_dev_core::testing::isolate_process();
}

mod test_compile_args;
mod test_connection_args;
mod test_doc_args;
mod test_eval_session_binary;
mod test_exec_args;
mod test_exit_flush;
mod test_http_guard_141;
mod test_plugin_manifest_version;
mod test_plugin_version_check;
mod test_query_tsv;
mod test_skill_catalogue_141;
mod test_tool_dispatch;
mod test_trust_editor_141;
mod test_tsv;
mod test_workflow_files;

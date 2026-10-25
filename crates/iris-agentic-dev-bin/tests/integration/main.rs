//! Aggregated integration-test target for the binary crate.
//!
//! These 14 files were 14 separate `[[test]]` targets. Cargo runs test binaries one after another,
//! so each was a process spawn charged to every run whether or not any of its tests were selected.
//! As modules of one binary they spawn once.
//!
//! Most of these spawn `iris-agentic-dev` as a subprocess or talk to the live `iris-dev-iris`
//! container, so they stay serial: run them with `--test-threads=1`. Tests in the same target now
//! share a process, which means a `set_var` in one is visible to the next.
//!
//! Add a file here and add its `mod` line below, or it will not run — the aggregator is the only
//! thing that pulls these in.

// Runs before `main`, so before any test thread: no test in this binary reaches the developer's
// real keychain or home directory. See `testing::isolate_process`.
#[ctor::ctor]
fn isolate_the_test_process() {
    iris_agentic_dev_core::testing::isolate_process();
}

mod binary_093_reload_pool;
mod binary_095_add_server_plaintext;
mod binary_096_terminal_compat;
mod binary_098_server_probe;
mod binary_099_fresh_container;
mod binary_140_config_precedence;
mod support_141;
mod test_attribution_stdio;
mod test_cli_gates_141;
mod test_compile_flags_141;
mod test_compile_live;
mod test_csp_logout_132;
mod test_doc_cli_examples;
mod test_doc_live;
mod test_exec_live;
mod test_host_paths_141;
mod test_http_transport;
mod test_mcp_binary_config;
mod test_mcp_sigterm_130;
mod test_no_eager_keychain_141;
mod test_query_live;
mod test_reporter_repro;
mod test_session_stores_141;
mod test_skill_install_cli;
mod test_tool_live;
mod test_transport_141;
mod test_trust_cli_141;
mod test_ws_cli_guard;

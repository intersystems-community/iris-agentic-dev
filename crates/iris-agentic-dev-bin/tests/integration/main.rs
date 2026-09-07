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

mod binary_093_reload_pool;
mod binary_095_add_server_plaintext;
mod binary_096_terminal_compat;
mod binary_098_server_probe;
mod binary_099_fresh_container;
mod test_attribution_stdio;
mod test_compile_live;
mod test_doc_live;
mod test_exec_live;
mod test_http_transport;
mod test_mcp_binary_config;
mod test_query_live;
mod test_reporter_repro;
mod test_tool_live;

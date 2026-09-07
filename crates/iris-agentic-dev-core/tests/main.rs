//! Aggregated target for the test files that sit directly in `tests/`.
//!
//! These 15 files were 15 separate `[[test]]` targets. Cargo runs test binaries one after
//! another, so each was a process spawn charged to every run. As modules of one binary they spawn
//! once.
//!
//! `autotests = false` in Cargo.toml is what makes this work: without it cargo would also pick each
//! file up as its own target and compile everything twice. That flag means a new file in `tests/`
//! is invisible until it appears both here and — for nothing, since this target covers them all —
//! nowhere else. Add the `mod` line below or the file does not run.

mod admin_e2e_tests;
mod admin_unit_tests;
mod cli_dispatch_integration;
mod discovery_tests;
mod docker_discovery_e2e;
mod interop_e2e_tests;
mod interop_unit_tests;
mod log_store_tests;
mod manifest_tests;
mod mcp_handshake;
mod progressive_disclosure_integration;
mod skills_tests;
mod symbols_local_tests;
mod vscode_config_tests;
mod xdata_flow_tests;

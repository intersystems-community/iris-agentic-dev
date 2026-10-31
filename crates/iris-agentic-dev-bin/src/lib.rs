// Re-export cmd modules for integration/unit test access.
pub mod cmd;

// The unit tests inside `src/` (for example `cmd::mcp::tests`) run in their own binary, which no
// aggregator's `main.rs` reaches, so they isolate before `main` too (spec 141 T183): without it
// they read the developer's real config dir and keychain.
#[cfg(all(test, feature = "testing"))]
#[ctor::ctor]
fn isolate_the_lib_test_process() {
    iris_agentic_dev_core::testing::isolate_process();
}

// Without `--features testing` the core crate's `testing` module is not compiled (the release
// binary must not carry it), but `cargo clippy --all-targets` and a bare `cargo test` still build
// these tests. They get the two settings of `isolate_process` that keep a test off the developer's
// files and keychain: an empty `HOME` and `IAD_NO_KEYCHAIN=1`.
#[cfg(all(test, not(feature = "testing")))]
#[ctor::ctor]
fn isolate_the_lib_test_process() {
    let home = std::env::temp_dir().join(format!("iad-bin-lib-test-{}", std::process::id()));
    let _ = std::fs::create_dir_all(&home);
    std::env::set_var("HOME", &home);
    std::env::set_var("USERPROFILE", &home);
    std::env::set_var("IAD_NO_KEYCHAIN", "1");
}

/// Exit the process once pending telemetry writes finish (at most
/// `telemetry::EXIT_FLUSH_TIMEOUT`). Use this instead of `std::process::exit`, which drops a write
/// mid-cycle and leaves its `IrisDevTmp` scratch class in USER (130 round 4). It also ends the
/// process's CSP sessions, as `main` does (132 B1).
pub fn exit(code: i32) -> ! {
    iris_agentic_dev_core::telemetry::flush_before_exit();
    iris_agentic_dev_core::iris::csp_session::logout_blocking(std::time::Duration::from_secs(3));
    std::process::exit(code)
}

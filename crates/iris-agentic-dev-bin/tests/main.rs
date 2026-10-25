//! Aggregated target for the binary crate's test files that sit directly in `tests/`.
//!
//! These 2 files were 2 separate `[[test]]` targets. Cargo runs test binaries one after another, so
//! each was a process spawn charged to every run. As modules of one binary they spawn once.
//!
//! `autotests = false` in Cargo.toml is what makes this work: without it cargo would also pick each
//! file up as its own target and compile everything twice. That flag also means a new file in
//! `tests/` is invisible until its `mod` line appears below.

// Runs before `main`, so before any test thread: no test in this binary reaches the developer's
// real keychain or home directory. See `testing::isolate_process`.
#[ctor::ctor]
fn isolate_the_test_process() {
    iris_agentic_dev_core::testing::isolate_process();
}

mod benchmark_relocated_binary_tests;
mod plugin_dispatch_tests;

//! Test-support helpers, compiled only under the `testing` feature.
//!
//! # Why this module exists
//!
//! Five `#[ignore]` tests in `nopws_101` shipped in 1.3.0 having never executed a single
//! assertion. Each one opened with:
//!
//! ```text
//! let binary = std::env::var("IAD_BINARY")
//!     .unwrap_or_else(|_| "./target/debug/iris-agentic-dev".to_string());
//! if !std::path::Path::new(&binary).exists() {
//!     eprintln!("IAD_BINARY not found at {binary}, skipping");
//!     return;
//! }
//! ```
//!
//! Two independent faults compounded. The default path is *relative*, and a test binary's working
//! directory is the crate root, not the workspace root — so `./target/debug/iris-agentic-dev` never
//! resolves in a workspace. And the miss is reported by returning `Ok`, so a test that ran nothing
//! is indistinguishable in the summary from a test that verified everything. CI does not set
//! `IAD_BINARY`, so all five took the skip branch on every run, for months, printing `ok`.
//!
//! The fix is structural, not a better default: resolution must not depend on the working
//! directory, and a missing prerequisite must be loud unless an operator explicitly asked for
//! quiet.

use std::path::{Path, PathBuf};

/// Absolute path to the workspace `target/` directory.
///
/// `CARGO_MANIFEST_DIR` is the crate root (`.../crates/iris-agentic-dev-core`), which cargo sets at
/// compile time, so this is correct no matter where the test process is started from. Walking up to
/// the directory holding the workspace `Cargo.toml` keeps it correct if a crate is ever nested
/// deeper.
fn workspace_root() -> PathBuf {
    let mut dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    while dir.pop() {
        if dir.join("Cargo.toml").exists() && dir.join("crates").is_dir() {
            return dir;
        }
    }
    // A single-crate checkout: the manifest dir *is* the root.
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

/// Where the `iris-agentic-dev` binary is expected to be.
///
/// `IAD_BINARY` wins when set. A relative `IAD_BINARY` is resolved against the workspace root
/// rather than the process working directory, because CI passes `./target/debug/iris-agentic-dev`
/// and the two differ. Otherwise both `debug` and `release` profiles are tried, so a
/// `cargo test --release` run finds its own binary.
pub fn iad_binary_path() -> PathBuf {
    if let Ok(v) = std::env::var("IAD_BINARY") {
        let p = PathBuf::from(&v);
        return if p.is_absolute() {
            p
        } else {
            workspace_root().join(p)
        };
    }
    let root = workspace_root();
    let debug = root.join("target/debug/iris-agentic-dev");
    if debug.exists() {
        return debug;
    }
    root.join("target/release/iris-agentic-dev")
}

/// The binary, or a panic naming what to run.
///
/// Panicking is the point. A binary-invocation test with no binary has verified nothing, and the
/// only honest outcomes are "ran" and "failed". `IAD_ALLOW_SKIP=1` opts into the old behaviour for
/// a developer who knowingly wants the rest of a suite to run; it returns `None` and prints why, so
/// the skip appears in the log as a deliberate choice rather than as a pass.
///
/// ```text
/// let Some(bin) = require_iad_binary() else { return };
/// ```
pub fn require_iad_binary() -> Option<PathBuf> {
    let path = iad_binary_path();
    if path.exists() {
        return Some(path);
    }
    if std::env::var("IAD_ALLOW_SKIP").is_ok() {
        eprintln!(
            "SKIP (IAD_ALLOW_SKIP set): no iris-agentic-dev binary at {}",
            path.display()
        );
        return None;
    }
    panic!(
        "no iris-agentic-dev binary at {}\n\
         This test spawns the binary; without it the test asserts nothing, so it fails instead of \
         passing quietly.\n\
         Build it:      cargo build -p iris-agentic-dev\n\
         Or point at one: IAD_BINARY=/abs/path/to/iris-agentic-dev\n\
         Or opt into skipping deliberately: IAD_ALLOW_SKIP=1",
        path.display()
    );
}

/// Every process-environment variable that changes what the server does.
///
/// Kept as one list so a spawn helper can clear all of them in a single call. `std::env::var` in
/// `crates/*/src/` is the source of truth; a var read there and absent here is a bleed waiting to
/// happen, which is what `antipatterns.sh env-pinning` checks.
pub const BEHAVIOR_ENV_VARS: &[&str] = &[
    // connection
    "IRIS_HOST",
    "IRIS_WEB_PORT",
    "IRIS_SCHEME",
    "IRIS_WEB_PREFIX",
    "IRIS_NAMESPACE",
    "IRIS_USERNAME",
    "IRIS_PASSWORD",
    "IRIS_CONTAINER",
    "IRIS_SERVICE_USERNAME",
    "IRIS_SERVICE_PASSWORD",
    "IRIS_SERVER_NAME",
    "IRIS_INSECURE",
    "IRIS_TLS_VERIFY",
    // gates
    "IRIS_WRITE_TOOLS_ENABLED",
    "IRIS_DESTRUCTIVE_TOOLS_ENABLED",
    "IRIS_ALLOW_PROD",
    "IRIS_ADMIN_TOOLS",
    "IRIS_SCM_ALLOW_CHECKIN",
    // tool surface
    "IRIS_ENABLED_TOOLS",
    "IRIS_DISABLED_TOOLS",
    "IRIS_TOOLSET",
    "IRIS_NO_SKILLS",
    "IRIS_LIST_TOOLS_PAGE_SIZE",
    // skills
    "OBJECTSCRIPT_LEARNING",
    "IRIS_AGENTIC_DEV_SKILLS_DIR",
    "OBJECTSCRIPT_SKILLMCP_NAMESPACE",
    // config discovery
    "OBJECTSCRIPT_WORKSPACE",
    // attribution
    "IRIS_AGENT_LABEL",
    // disclosure thresholds
    "IRIS_INLINE_SEARCH",
    "IRIS_INLINE_COMPILE",
    "IRIS_INLINE_ERROR_LOGS",
    "IRIS_INLINE_INFO",
    "IRIS_LOG_STORE_MAX",
    "IRIS_LOG_TTL_MINUTES",
    // timeouts
    "IRIS_SEARCH_SYNC_TIMEOUT",
    "OBJECTSCRIPT_TEST_TIMEOUT",
    "IRIS_GENERATE_TIMEOUT",
    // generation
    "IRIS_GENERATE_CLASS_MODEL",
    "OPENAI_API_KEY",
    "OPENAI_BASE_URL",
    "ANTHROPIC_API_KEY",
    "ANTHROPIC_BASE_URL",
    // skill fetch
    "GITHUB_RAW_BASE_URL",
    "GITHUB_API_BASE_URL",
    // eval envelope identity
    "GAUNTLET_RUN_ID",
    "GAUNTLET_TASK_ID",
    "GAUNTLET_CONDITION",
];

/// A `Command` for the binary with every behavior-changing variable removed.
///
/// The deny-list approach every existing spawn site uses is unmaintainable: a new `std::env::var`
/// in `src/` instantly becomes a bleed at ~60 call sites, and the CI e2e job sets nine of these at
/// job level, so the same test means different things in the `test` job and the `e2e-tests` job.
/// Start from nothing and add back only what the test is about; then the test's meaning is written
/// down in the test.
///
/// `PATH`, `HOME` and the profiler's `LLVM_PROFILE_FILE` are left alone — the child needs them to
/// run at all, and `HOME` isolation is a per-test decision.
pub fn clean_command(bin: &Path) -> std::process::Command {
    let mut cmd = std::process::Command::new(bin);
    for var in BEHAVIOR_ENV_VARS {
        cmd.env_remove(var);
    }
    cmd
}

/// `clean_command` plus the `mcp` subcommand.
///
/// The MCP server is `iris-agentic-dev mcp`. Spawning the bare binary prints the usage banner and
/// exits 2, which a test reading stdout for JSON-RPC sees as empty output — the second half of the
/// `nopws_101` failure. Going through this helper makes forgetting it impossible.
pub fn clean_mcp_command(bin: &Path) -> std::process::Command {
    let mut cmd = clean_command(bin);
    cmd.arg("mcp");
    cmd
}

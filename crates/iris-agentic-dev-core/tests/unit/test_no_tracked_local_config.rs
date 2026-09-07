//! `.iris-agentic-dev.toml` at the repo root must never be tracked by git.
//!
//! It is machine-local connection config: container name, host, web port. `load_workspace_config`
//! walks up from cwd to find it, and per FR-006 it takes precedence over environment variables.
//! So a tracked copy silently overrides whatever the environment says.
//!
//! That shipped once. Commit ef38627 added the file to pin the local `iris-dev-iris` web port
//! 52780. CI sets `IRIS_CONTAINER: iris-e2e`, but the checkout now carried
//! `container = "iris-dev-iris"`, which won on precedence — so nine integration tests failed on CI
//! with `docker has no container named 'iris-dev-iris'` while the whole suite stayed green locally,
//! because locally that container is real.
//!
//! The asymmetry is the dangerous part: nothing about the local run can reveal it. Only this guard
//! can.

use std::path::PathBuf;
use std::process::Command;

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("crates/<crate> has two ancestors")
        .to_path_buf()
}

#[test]
fn the_workspace_config_is_not_tracked_by_git() {
    let root = repo_root();
    let out = Command::new("git")
        .args(["ls-files", "--", ".iris-agentic-dev.toml"])
        .current_dir(&root)
        .output()
        .expect("git ls-files runs");

    // No git (a source tarball, a vendored build) means nothing to check.
    if !out.status.success() {
        return;
    }

    let tracked = String::from_utf8_lossy(&out.stdout);
    assert!(
        tracked.trim().is_empty(),
        ".iris-agentic-dev.toml is tracked by git: {}\n\
         It is machine-local config and it outranks env vars, so a tracked copy points CI at \
         whatever container this developer happens to run. Run \
         `git rm --cached .iris-agentic-dev.toml` and keep it in .gitignore.",
        tracked.trim()
    );
}

#[test]
fn gitignore_covers_the_workspace_config() {
    let root = repo_root();
    let out = Command::new("git")
        .args(["check-ignore", "-q", ".iris-agentic-dev.toml"])
        .current_dir(&root)
        .output()
        .expect("git check-ignore runs");

    // 0 = ignored, 1 = not ignored, 128 = no git. Only 1 is a failure.
    assert_ne!(
        out.status.code(),
        Some(1),
        ".iris-agentic-dev.toml is not gitignored — the next `git add -A` re-commits it"
    );
}

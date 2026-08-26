//! Spec 085 US6 (FR-029): `Cargo.lock` agrees with the workspace manifests.
//!
//! Why this is a test and not just a CI flag. `build.rs` embeds
//! `git describe --tags --always --dirty` in `SERVER_VERSION`, and cargo reconciles a stale
//! lockfile *during resolution* — before any build script runs. So a lockfile that disagrees with
//! the manifests makes the tree dirty by the time `build.rs` looks at it, and every published
//! 1.2.6 asset advertised `1.2.6+v1.2.6-dirty`. `check_config`'s own description tells operators
//! to read `server_version` to tell an official build from a fork, so the drift was not cosmetic.
//!
//! `cargo metadata --locked` resolves the graph and refuses to rewrite the lockfile, without
//! compiling anything, which makes this cheap enough to keep in the unit suite.

use std::path::{Path, PathBuf};
use std::process::Command;

/// The workspace root. `CARGO_MANIFEST_DIR` is `<root>/crates/iris-agentic-dev-core`.
fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("CARGO_MANIFEST_DIR should be <root>/crates/<crate>")
        .to_path_buf()
}

/// The `cargo` that is running this test, so the check uses the same toolchain as the build.
fn cargo() -> String {
    std::env::var("CARGO").unwrap_or_else(|_| "cargo".to_string())
}

#[test]
fn cargo_lock_is_in_sync_with_the_workspace_manifests() {
    let root = repo_root();
    assert!(
        root.join("Cargo.lock").is_file(),
        "no Cargo.lock at {} — the lockfile is committed on purpose; a missing one means every \
         build resolves fresh and the version string is unreproducible",
        root.display()
    );

    let out = Command::new(cargo())
        .args(["metadata", "--locked", "--format-version", "1"])
        .current_dir(&root)
        .output()
        .expect("failed to run cargo metadata");

    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr);
        panic!(
            "Cargo.lock disagrees with the workspace manifests (cargo metadata --locked exited \
             {:?}). Cargo would rewrite the lockfile during resolution, which marks the tree dirty \
             before build.rs runs and appends `-dirty` to SERVER_VERSION on every release asset. \
             Run `cargo metadata` (no --locked) to reconcile it and commit the result.\n\
             cargo stderr:\n{}",
            out.status.code(),
            stderr.trim()
        );
    }
}

/// The lockfile has to cover the whole workspace, not just the crate the test happens to live in.
///
/// Member *paths* come from the root manifest and package *names* from each member's own manifest —
/// the two differ here (`crates/iris-agentic-dev-bin` publishes as `iris-agentic-dev`), and
/// hardcoding either list means a member added later is silently unchecked.
#[test]
fn the_lockfile_covers_every_workspace_member() {
    let root = repo_root();
    let lock = std::fs::read_to_string(root.join("Cargo.lock")).expect("read Cargo.lock");
    let ws = std::fs::read_to_string(root.join("Cargo.toml")).expect("read workspace Cargo.toml");

    // `members = [ "crates/a", "crates/b" ]` — the quoted paths in that array, nothing else.
    let members: Vec<String> = ws
        .split_once("members = [")
        .and_then(|(_, rest)| rest.split_once(']'))
        .map(|(list, _)| {
            list.split('"')
                .skip(1)
                .step_by(2)
                .map(str::to_string)
                .collect()
        })
        .expect("workspace Cargo.toml should declare a members array");
    assert!(
        members.len() >= 2,
        "parsed {} workspace member(s) from Cargo.toml — the parser has stopped reading the \
         manifest and this test is asserting nothing",
        members.len()
    );

    for member in members {
        let manifest = std::fs::read_to_string(root.join(&member).join("Cargo.toml"))
            .unwrap_or_else(|e| {
                panic!("workspace member {member} has no readable Cargo.toml: {e}")
            });
        // The first `name = "..."` under [package]; later ones belong to [[test]]/[[bin]] targets.
        let pkg = manifest
            .lines()
            .find_map(|l| l.strip_prefix("name = \""))
            .and_then(|r| r.split('"').next())
            .unwrap_or_else(|| panic!("{member}/Cargo.toml declares no package name"));
        assert!(
            lock.contains(&format!("name = \"{pkg}\"")),
            "Cargo.lock has no entry for workspace member {member} (package {pkg}) — the --locked \
             check above cannot see drift in a crate the lockfile does not list"
        );
    }
}

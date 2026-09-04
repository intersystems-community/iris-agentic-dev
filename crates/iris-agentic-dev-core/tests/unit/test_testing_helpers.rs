//! Tests for `core::testing` — the helpers that decide where the binary is and what the
//! child process inherits.
//!
//! These are the two things the `nopws_101` failure turned on: a relative `IAD_BINARY`
//! resolved against the wrong directory, and a spawn that inherited the CI job's gate
//! variables. Nine call sites now depend on `iad_binary_path` and `clean_command` being
//! right, and until this file existed nothing asserted either of them.
//!
//! Not an inline `mod tests`: assertion-message lines inside a `#[cfg(test)]` module only
//! execute when a test fails, so they read as permanently uncovered and cap the measured
//! coverage of the file they live in.

use iris_agentic_dev_core::testing::{
    clean_command, clean_mcp_command, iad_binary_path, require_iad_binary, BEHAVIOR_ENV_VARS,
};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard, OnceLock};

/// `IAD_BINARY` and `IAD_ALLOW_SKIP` are process-global. Serialize the tests that write
/// them and put the previous values back, so a harness that set `IAD_BINARY` for the whole
/// run still sees it afterwards.
struct EnvGuard {
    _lock: MutexGuard<'static, ()>,
    binary: Option<String>,
    allow_skip: Option<String>,
}

impl EnvGuard {
    fn new() -> Self {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        let lock = LOCK
            .get_or_init(|| Mutex::new(()))
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let guard = EnvGuard {
            _lock: lock,
            binary: std::env::var("IAD_BINARY").ok(),
            allow_skip: std::env::var("IAD_ALLOW_SKIP").ok(),
        };
        std::env::remove_var("IAD_BINARY");
        std::env::remove_var("IAD_ALLOW_SKIP");
        guard
    }
}

impl Drop for EnvGuard {
    fn drop(&mut self) {
        match &self.binary {
            Some(v) => std::env::set_var("IAD_BINARY", v),
            None => std::env::remove_var("IAD_BINARY"),
        }
        match &self.allow_skip {
            Some(v) => std::env::set_var("IAD_ALLOW_SKIP", v),
            None => std::env::remove_var("IAD_ALLOW_SKIP"),
        }
    }
}

/// A path that is absolute and certain not to exist.
fn missing_absolute_path() -> PathBuf {
    PathBuf::from("/nonexistent-iad-binary-for-tests/iris-agentic-dev")
}

#[test]
fn absolute_iad_binary_is_returned_unchanged() {
    let _g = EnvGuard::new();
    let want = missing_absolute_path();
    std::env::set_var("IAD_BINARY", &want);
    assert_eq!(iad_binary_path(), want);
}

/// The `nopws_101` bug in one assertion. CI passes `./target/debug/iris-agentic-dev`, and a
/// workspace member's test process runs with the member directory as its cwd, so resolving
/// against the cwd points at `crates/<member>/target/debug/…` — which never exists.
#[test]
fn relative_iad_binary_resolves_against_the_workspace_root_not_the_cwd() {
    let _g = EnvGuard::new();
    std::env::set_var("IAD_BINARY", "./target/debug/iris-agentic-dev");
    let resolved = iad_binary_path();

    assert!(
        resolved.is_absolute(),
        "a relative IAD_BINARY must come back absolute, got {}",
        resolved.display()
    );
    assert!(
        resolved.ends_with("target/debug/iris-agentic-dev"),
        "the relative part must survive resolution, got {}",
        resolved.display()
    );

    let root = resolved
        .parent()
        .and_then(Path::parent)
        .and_then(Path::parent)
        .expect("resolved path has three ancestors");
    assert!(
        root.join("Cargo.toml").is_file() && root.join("crates").is_dir(),
        "must resolve against the workspace root, got {}",
        root.display()
    );
    assert!(
        !root.ends_with("iris-agentic-dev-core"),
        "resolving against the crate directory is the bug, got {}",
        root.display()
    );
}

#[test]
fn unset_iad_binary_falls_back_to_a_workspace_target_path() {
    let _g = EnvGuard::new();
    let resolved = iad_binary_path();

    assert!(resolved.is_absolute(), "fallback must be absolute");
    let tail = resolved
        .strip_prefix(
            resolved
                .parent()
                .and_then(Path::parent)
                .and_then(Path::parent)
                .expect("three ancestors"),
        )
        .expect("tail under workspace root");
    assert!(
        tail == Path::new("target/debug/iris-agentic-dev")
            || tail == Path::new("target/release/iris-agentic-dev"),
        "fallback must name one of the two profiles, got {}",
        tail.display()
    );
}

#[test]
fn require_returns_the_path_when_the_binary_exists() {
    let _g = EnvGuard::new();
    // The test binary itself is the one file guaranteed to be on disk right now.
    let existing = std::env::current_exe().expect("current_exe");
    std::env::set_var("IAD_BINARY", &existing);
    assert_eq!(require_iad_binary(), Some(existing));
}

/// A skip is only honest when someone asked for it, and it has to say so.
#[test]
fn require_returns_none_when_allow_skip_is_set_and_the_binary_is_missing() {
    let _g = EnvGuard::new();
    std::env::set_var("IAD_BINARY", missing_absolute_path());
    std::env::set_var("IAD_ALLOW_SKIP", "1");
    assert_eq!(require_iad_binary(), None);
}

/// The whole point of the helper: no binary means the test verified nothing, so it fails
/// rather than printing `ok`.
#[test]
#[should_panic(expected = "no iris-agentic-dev binary at")]
fn require_panics_when_the_binary_is_missing() {
    let _g = EnvGuard::new();
    std::env::set_var("IAD_BINARY", missing_absolute_path());
    require_iad_binary();
}

#[test]
fn clean_command_removes_every_behavior_var() {
    let cmd = clean_command(Path::new("/bin/true"));
    let removed: Vec<&str> = cmd
        .get_envs()
        .filter(|(_, v)| v.is_none())
        .filter_map(|(k, _)| k.to_str())
        .collect();

    for var in BEHAVIOR_ENV_VARS {
        assert!(
            removed.contains(var),
            "{var} is in BEHAVIOR_ENV_VARS but clean_command did not remove it"
        );
    }
}

#[test]
fn clean_mcp_command_passes_the_mcp_subcommand() {
    let cmd = clean_mcp_command(Path::new("/bin/true"));
    let args: Vec<&str> = cmd.get_args().filter_map(|a| a.to_str()).collect();
    assert_eq!(
        args,
        vec!["mcp"],
        "spawning the bare binary prints usage and exits 2"
    );
}

/// A duplicate is harmless at runtime and a sign the list was edited by hand twice.
#[test]
fn behavior_env_vars_has_no_duplicates() {
    let mut seen = std::collections::HashSet::new();
    for var in BEHAVIOR_ENV_VARS {
        assert!(
            seen.insert(*var),
            "{var} appears twice in BEHAVIOR_ENV_VARS"
        );
    }
    assert!(
        seen.contains("IRIS_WRITE_TOOLS_ENABLED") && seen.contains("IRIS_HOST"),
        "the list must cover the gate and connection variables the CI job sets"
    );
}

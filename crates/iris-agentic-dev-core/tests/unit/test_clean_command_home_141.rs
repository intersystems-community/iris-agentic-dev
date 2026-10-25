//! `clean_command` and `clean_mcp_command` isolate the child's home directory (spec 141, T001).
//!
//! The developer's own `~/.iris-agentic-dev.toml` is the operator file. It sets
//! `destructive_tools_enabled = true`, so a spawned test that inherits `HOME` reads that file and
//! its gate answers differ from CI. Both helpers must hand the child a fresh, empty `HOME` and
//! `USERPROFILE`, and must not reuse the parent's.
//!
//! Not an inline `mod tests`: see `test_testing_helpers.rs` for why.

use iris_agentic_dev_core::testing::{clean_command, clean_mcp_command};
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::Command;

/// The value the command sets for `var`, or `None` when it leaves the variable alone.
fn env_value(cmd: &Command, var: &str) -> Option<PathBuf> {
    cmd.get_envs()
        .find(|(k, _)| *k == var)
        .and_then(|(_, v)| v.map(PathBuf::from))
}

fn assert_fresh_home(home: &Path) {
    assert!(
        home.is_dir(),
        "the child's HOME {} must be an existing directory",
        home.display()
    );
    assert!(
        !home.join(".iris-agentic-dev.toml").exists(),
        "the child's HOME {} must hold no operator file",
        home.display()
    );
}

#[test]
fn clean_command_sets_home_to_a_fresh_directory() {
    let cmd = clean_command(Path::new("/bin/sh"));
    let home = env_value(&cmd, "HOME")
        .expect("clean_command must set HOME explicitly, not inherit the developer's");
    assert_ne!(
        Some(home.clone()),
        std::env::var_os("HOME").map(PathBuf::from),
        "the child's HOME must not be the developer's HOME"
    );
    assert_fresh_home(&home);
}

#[test]
fn clean_command_sets_userprofile_to_the_same_fresh_directory() {
    let cmd = clean_command(Path::new("/bin/sh"));
    let home = env_value(&cmd, "HOME").expect("HOME must be set");
    let profile = env_value(&cmd, "USERPROFILE")
        .expect("clean_command must set USERPROFILE too, for the Windows home lookup");
    assert_eq!(
        profile, home,
        "USERPROFILE must match HOME so both platform lookups see the same empty directory"
    );
}

#[test]
fn each_command_gets_its_own_home_directory() {
    let first = env_value(&clean_command(Path::new("/bin/sh")), "HOME").expect("HOME");
    let second = env_value(&clean_command(Path::new("/bin/sh")), "HOME").expect("HOME");
    assert_ne!(
        first, second,
        "two spawns must not share one home directory, or one test's files leak into the next"
    );
}

#[test]
fn clean_mcp_command_sets_the_same_fresh_home() {
    let cmd = clean_mcp_command(Path::new("/bin/sh"));
    let home = env_value(&cmd, "HOME").expect("clean_mcp_command must set HOME");
    let profile = env_value(&cmd, "USERPROFILE").expect("clean_mcp_command must set USERPROFILE");
    assert_fresh_home(&home);
    assert_eq!(profile, home);
}

/// The child sees the directory the command names, and it is still there when the child runs.
#[test]
#[cfg(unix)]
fn child_process_sees_the_fresh_home() {
    let mut cmd = clean_command(Path::new("/bin/sh"));
    cmd.arg("-c")
        .arg(r#"printf '%s|%s' "$HOME" "$USERPROFILE""#);
    let out = cmd.output().expect("must run /bin/sh");
    assert!(out.status.success(), "sh failed: {:?}", out.status);
    let seen = String::from_utf8(out.stdout).expect("utf-8 env");
    let (home, profile) = seen.split_once('|').expect("HOME|USERPROFILE");
    let expected = env_value(&cmd, "HOME").expect("HOME");
    assert_eq!(
        OsString::from(home),
        expected.clone().into_os_string(),
        "the child must inherit the HOME the command was built with"
    );
    assert_eq!(profile, home, "USERPROFILE must match HOME in the child");
    assert_fresh_home(Path::new(home));
}

/// T181. A spawned process inherits the test's cwd, and the configuration walk goes up from the
/// cwd: a checkout under another checkout (a worktree) finds the outer `.iris-agentic-dev.toml`,
/// which outranks `IRIS_*`. Both helpers name a fresh empty working directory.
#[test]
fn clean_commands_run_in_a_fresh_empty_working_directory() {
    for cmd in [
        clean_command(Path::new("/bin/sh")),
        clean_mcp_command(Path::new("/bin/sh")),
    ] {
        let cwd = cmd
            .get_current_dir()
            .expect("the helper must name a cwd, not inherit the test's")
            .to_path_buf();
        assert!(cwd.is_dir(), "{} must exist", cwd.display());
        assert_ne!(
            Some(cwd.clone()),
            std::env::current_dir().ok(),
            "the child's cwd must not be the test's"
        );
        assert!(
            std::fs::read_dir(&cwd).unwrap().next().is_none(),
            "{} must be empty, so no project file is found there",
            cwd.display()
        );
    }
}

/// A caller that needs a project directory sets one, and that wins.
#[test]
fn a_caller_can_still_set_its_own_working_directory() {
    let mut cmd = clean_command(Path::new("/bin/sh"));
    cmd.current_dir("/");
    assert_eq!(cmd.get_current_dir(), Some(Path::new("/")));
}

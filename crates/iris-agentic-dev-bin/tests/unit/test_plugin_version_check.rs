//! 1.6.4: the Claude Code plugin updates its skills and hooks, but the binary it runs is whatever
//! `iris-agentic-dev` is on PATH, and nothing ever said when that binary was older than the
//! plugin. A Doc team user ran 1.5.0 under newer skills for weeks. A `SessionStart` hook now
//! compares the two and prints one line with the upgrade step; it never downloads anything and
//! never fails the session. The setup skill checks the version too, not only that a binary exists.

use std::path::{Path, PathBuf};
use std::process::Command;

fn root() -> PathBuf {
    let mut p = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    p.pop();
    p.pop();
    p
}

/// A directory holding a stand-in `iris-agentic-dev` that prints `version`, or none at all.
fn fake_path(name: &str, version: Option<&str>) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("iad-version-check-{name}"));
    std::fs::create_dir_all(&dir).unwrap();
    let bin = dir.join("iris-agentic-dev");
    let _ = std::fs::remove_file(&bin);
    if let Some(v) = version {
        std::fs::write(&bin, format!("#!/bin/sh\necho 'iris-agentic-dev {v}'\n")).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
    }
    dir
}

fn run_check(path_dir: &Path) -> (i32, String) {
    run_check_in(path_dir, &root())
}

fn run_check_in(path_dir: &Path, plugin_root: &Path) -> (i32, String) {
    let out = Command::new("bash")
        .arg(root().join("scripts/version-check.sh"))
        .env_clear()
        .env("PATH", format!("{}:/usr/bin:/bin", path_dir.display()))
        .env("CLAUDE_PLUGIN_ROOT", plugin_root)
        .output()
        .expect("bash runs the hook");
    (
        out.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&out.stdout).to_string() + &String::from_utf8_lossy(&out.stderr),
    )
}

#[test]
fn the_plugin_runs_the_version_check_at_session_start() {
    let manifest: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(root().join(".claude-plugin/plugin.json")).unwrap(),
    )
    .unwrap();
    let hooks = manifest["hooks"]["SessionStart"].to_string();
    assert!(
        hooks.contains("${CLAUDE_PLUGIN_ROOT}/scripts/version-check.sh"),
        "{hooks}"
    );
}

#[cfg(unix)]
#[test]
fn an_older_binary_is_named_with_its_upgrade_step() {
    let plugin = env!("CARGO_PKG_VERSION");
    let (code, text) = run_check(&fake_path("older", Some("1.5.0")));
    assert_eq!(code, 0, "the hook never fails the session: {text}");
    assert!(text.contains("1.5.0") && text.contains(plugin), "{text}");
    assert!(
        text.contains("brew upgrade iris-agentic-dev") && text.contains("releases/latest"),
        "the upgrade step is in the line: {text}"
    );
}

#[cfg(unix)]
#[test]
fn a_current_or_newer_binary_prints_nothing() {
    let plugin = env!("CARGO_PKG_VERSION");
    for (name, v) in [("same", plugin), ("newer", "99.0.0")] {
        let (code, text) = run_check(&fake_path(name, Some(v)));
        assert_eq!(code, 0, "{name}: {text}");
        assert!(text.trim().is_empty(), "{name}: {text}");
    }
}

#[cfg(unix)]
#[test]
fn a_missing_binary_says_how_to_install_it() {
    let (code, text) = run_check(&fake_path("missing", None));
    assert_eq!(code, 0, "{text}");
    assert!(text.contains("not on PATH"), "{text}");
    assert!(
        text.contains("iris-agentic-dev-setup"),
        "points at the setup skill: {text}"
    );
}

/// As text, "1.10.0" sorts before "1.9.5". A plugin root of its own pins the plugin's version.
#[cfg(unix)]
#[test]
fn versions_compare_as_numbers_not_text() {
    let plugin_root = std::env::temp_dir().join("iad-version-check-root");
    std::fs::create_dir_all(plugin_root.join(".claude-plugin")).unwrap();
    std::fs::write(
        plugin_root.join(".claude-plugin/plugin.json"),
        r#"{"name":"iris-dev","version":"1.9.5"}"#,
    )
    .unwrap();
    let (_, newer) = run_check_in(&fake_path("numeric-newer", Some("1.10.0")), &plugin_root);
    assert!(
        newer.trim().is_empty(),
        "1.10.0 is newer than 1.9.5: {newer}"
    );
    let (_, older) = run_check_in(&fake_path("numeric-older", Some("1.9.4")), &plugin_root);
    assert!(
        older.contains("1.9.4") && older.contains("1.9.5"),
        "{older}"
    );
}

#[test]
fn the_setup_skill_upgrades_an_older_binary() {
    let skill =
        std::fs::read_to_string(root().join("skills/iris-agentic-dev-setup/SKILL.md")).unwrap();
    let step1 = &skill[skill.find("## 1.").unwrap()..skill.find("## 2.").unwrap()];
    assert!(
        step1.contains("older") && step1.contains("upgrade"),
        "step 1 must check the version, not only that a binary exists: {step1}"
    );
}

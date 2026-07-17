use iris_agentic_dev_core::skill_install::{
    install_skill, mirror_to_iris, AgentKind, InstallOutcome, InstallTarget, InstalledSkill,
};
use std::fs;
use tempfile::TempDir;

// ── T009: AgentKind + InstallTarget path resolution ──────────────────────────

#[test]
fn target_claude_code_unix_path() {
    let tmp = TempDir::new().unwrap();
    let target = InstallTarget::for_claude_code("pyprod", Some(tmp.path())).unwrap();
    assert_eq!(target.agent, AgentKind::ClaudeCode);
    let expected = tmp
        .path()
        .join(".claude")
        .join("skills")
        .join("pyprod")
        .join("SKILL.md");
    assert_eq!(target.target_path, expected);
}

#[test]
fn target_opencode_path() {
    let tmp = TempDir::new().unwrap();
    let target = InstallTarget::for_opencode("pyprod", Some(tmp.path())).unwrap();
    assert_eq!(target.agent, AgentKind::OpenCode);
    let expected = tmp
        .path()
        .join("opencode")
        .join("skills")
        .join("pyprod")
        .join("SKILL.md");
    assert_eq!(target.target_path, expected);
}

#[test]
fn target_copilot_path() {
    let tmp = TempDir::new().unwrap();
    let target = InstallTarget::for_copilot("pyprod", tmp.path());
    assert_eq!(target.agent, AgentKind::Copilot);
    let expected = tmp
        .path()
        .join(".github")
        .join("instructions")
        .join("pyprod.instructions.md");
    assert_eq!(target.target_path, expected);
}

#[test]
fn target_skill_name_stored() {
    let tmp = TempDir::new().unwrap();
    let target = InstallTarget::for_claude_code("objectscript-review", Some(tmp.path())).unwrap();
    assert_eq!(target.skill_name, "objectscript-review");
}

// ── T010: managed_by marker detection ────────────────────────────────────────

use iris_agentic_dev_core::skill_install::is_managed;

#[test]
fn managed_returns_true_when_marker_present() {
    let tmp = TempDir::new().unwrap();
    let path = tmp.path().join("SKILL.md");
    fs::write(
        &path,
        "---\nname: test\nmanaged_by: \"iris-agentic-dev\"\n---\n",
    )
    .unwrap();
    assert!(is_managed(&path));
}

#[test]
fn managed_returns_false_when_marker_absent() {
    let tmp = TempDir::new().unwrap();
    let path = tmp.path().join("SKILL.md");
    fs::write(&path, "---\nname: test\n---\n# My custom skill\n").unwrap();
    assert!(!is_managed(&path));
}

#[test]
fn managed_returns_false_for_missing_file() {
    let tmp = TempDir::new().unwrap();
    let path = tmp.path().join("does-not-exist.md");
    assert!(!is_managed(&path));
}

// ── T015–T018: install_skill behavior ────────────────────────────────────────

fn cc_target(name: &str, home: &std::path::Path) -> InstallTarget {
    InstallTarget::for_claude_code(name, Some(home)).unwrap()
}

const MANAGED_CONTENT: &str =
    "---\nname: pyprod\ndescription: test\nmanaged_by: \"iris-agentic-dev\"\n---\n# pyprod\n";

const USER_CONTENT: &str = "---\nname: pyprod\ndescription: my custom\n---\n# custom\n";

// T015: install with targets returns Written outcome
#[test]
fn install_new_file_returns_written() {
    let tmp = TempDir::new().unwrap();
    let targets = vec![cc_target("pyprod", tmp.path())];
    let results = install_skill("pyprod", MANAGED_CONTENT, &targets, false);
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].outcome, InstallOutcome::Written);
}

// T016: single skill install writes file, marker present
#[test]
fn install_writes_file_with_marker() {
    let tmp = TempDir::new().unwrap();
    let target = cc_target("pyprod", tmp.path());
    let path = target.target_path.clone();
    install_skill("pyprod", MANAGED_CONTENT, &[target], false);
    assert!(path.exists());
    let written = fs::read_to_string(&path).unwrap();
    assert!(written.contains("managed_by: \"iris-agentic-dev\""));
}

// T017: re-install managed file → Updated; user file → Skipped
#[test]
fn reinstall_managed_file_returns_updated() {
    let tmp = TempDir::new().unwrap();
    let target = cc_target("pyprod", tmp.path());
    install_skill("pyprod", MANAGED_CONTENT, &[target.clone()], false);
    let results = install_skill("pyprod", MANAGED_CONTENT, &[target], false);
    assert_eq!(results[0].outcome, InstallOutcome::Updated);
}

#[test]
fn install_user_authored_file_returns_skipped() {
    let tmp = TempDir::new().unwrap();
    let target = cc_target("pyprod", tmp.path());
    let dir = target.target_path.parent().unwrap();
    fs::create_dir_all(dir).unwrap();
    fs::write(&target.target_path, USER_CONTENT).unwrap();
    let results = install_skill("pyprod", MANAGED_CONTENT, &[target], false);
    assert_eq!(results[0].outcome, InstallOutcome::Skipped);
    // file must NOT have been overwritten
    let still_user = fs::read_to_string(results[0].target.target_path.as_path()).unwrap();
    assert!(still_user.contains("my custom"));
}

// T018: --dry-run writes nothing
#[test]
fn dry_run_does_not_write_files() {
    let tmp = TempDir::new().unwrap();
    let target = cc_target("pyprod", tmp.path());
    let path = target.target_path.clone();
    let results = install_skill("pyprod", MANAGED_CONTENT, &[target], true);
    assert!(!path.exists(), "dry-run must not create the file");
    assert_eq!(results[0].outcome, InstallOutcome::Written);
}

#[test]
fn dry_run_existing_managed_reports_updated_without_write() {
    let tmp = TempDir::new().unwrap();
    let target = cc_target("pyprod", tmp.path());
    // First real install
    install_skill("pyprod", MANAGED_CONTENT, &[target.clone()], false);
    // Dry-run re-install
    let results = install_skill("pyprod", MANAGED_CONTENT, &[target], true);
    assert_eq!(results[0].outcome, InstallOutcome::Updated);
}

// T018b: --agent all installs to all three targets
#[test]
fn install_all_targets_returns_three_results() {
    let tmp_home = TempDir::new().unwrap();
    let tmp_config = TempDir::new().unwrap();
    let tmp_repo = TempDir::new().unwrap();

    let targets = vec![
        InstallTarget::for_claude_code("pyprod", Some(tmp_home.path())).unwrap(),
        InstallTarget::for_opencode("pyprod", Some(tmp_config.path())).unwrap(),
        InstallTarget::for_copilot("pyprod", tmp_repo.path()),
    ];

    let results = install_skill("pyprod", MANAGED_CONTENT, &targets, false);
    assert_eq!(results.len(), 3);
    assert_eq!(results[0].target.agent, AgentKind::ClaudeCode);
    assert_eq!(results[1].target.agent, AgentKind::OpenCode);
    assert_eq!(results[2].target.agent, AgentKind::Copilot);
    for r in &results {
        assert_eq!(r.outcome, InstallOutcome::Written);
    }
}

// ── T035: mirror_to_iris without IRIS → IRIS_UNREACHABLE ─────────────────────

#[test]
fn mirror_to_iris_without_connection_returns_unreachable() {
    let skills = vec![InstalledSkill {
        name: "pyprod".to_string(),
        content: MANAGED_CONTENT.to_string(),
    }];
    let result = mirror_to_iris(&skills, None);
    assert!(result.is_err());
    let msg = result.unwrap_err().to_string();
    assert!(
        msg.contains("IRIS_UNREACHABLE"),
        "expected IRIS_UNREACHABLE in error, got: {msg}"
    );
}

// ── T052: skill list output format ────────────────────────────────────────────

#[test]
fn list_installed_shows_correct_state() {
    let tmp_home = TempDir::new().unwrap();
    let tmp_config = TempDir::new().unwrap();

    // Install pyprod for Claude Code only
    let cc = InstallTarget::for_claude_code("pyprod", Some(tmp_home.path())).unwrap();
    install_skill("pyprod", MANAGED_CONTENT, &[cc.clone()], false);

    // Claude Code: installed
    assert!(cc.target_path.exists());

    // OpenCode: not installed
    let oc = InstallTarget::for_opencode("pyprod", Some(tmp_config.path())).unwrap();
    assert!(!oc.target_path.exists());
}

#[test]
fn list_not_installed_when_no_files() {
    let tmp_home = TempDir::new().unwrap();
    let target =
        InstallTarget::for_claude_code("objectscript-review", Some(tmp_home.path())).unwrap();
    assert!(!target.target_path.exists());
}

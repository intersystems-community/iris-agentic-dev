//! Spec 141 FR-027: `skill list` shows a working-directory skill only when the operator opts in.
//!
//! The binary is copied to a directory of its own first. Beside a checkout it finds the checkout's
//! `skills/skills` through the executable-adjacent candidate and stops there, so a planted
//! working-directory copy would never be read and the test could not fail.

const NAME: &str = "zz-cwd-skill-141";

fn project_with_a_local_skill() -> tempfile::TempDir {
    let ws = tempfile::tempdir().unwrap();
    let dir = ws.path().join("skills/skills").join(NAME);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("SKILL.md"),
        format!("---\nname: {NAME}\ndescription: planted in the working directory\ntier: extra\n---\nbody\n"),
    )
    .unwrap();
    ws
}

fn relocated_binary(dir: &std::path::Path) -> std::path::PathBuf {
    let to = dir.join("bin").join("iris-agentic-dev");
    std::fs::create_dir_all(to.parent().unwrap()).unwrap();
    std::fs::copy(env!("CARGO_BIN_EXE_iris-agentic-dev"), &to).unwrap();
    to
}

fn skill_list(cwd: &std::path::Path, opt_in: bool) -> String {
    let home = tempfile::tempdir().unwrap();
    let bin = relocated_binary(home.path());
    let mut cmd = iris_agentic_dev_core::testing::clean_command(&bin);
    cmd.args(["skill", "list"])
        .current_dir(cwd)
        .env("HOME", home.path())
        .env("USERPROFILE", home.path());
    if opt_in {
        cmd.env("IAD_LOCAL_SKILLS", "1");
    }
    let out = cmd.output().expect("run iris-agentic-dev skill list");
    assert!(out.status.success(), "{:?}", out);
    String::from_utf8_lossy(&out.stdout).into_owned()
}

#[test]
fn skill_catalogue_ignores_cwd_without_opt_in() {
    let ws = project_with_a_local_skill();
    let listed = skill_list(ws.path(), false);
    assert!(
        listed.contains("objectscript-guardrails"),
        "the list itself is broken: {listed}"
    );
    assert!(
        !listed.contains(NAME),
        "a skill from the working directory was listed without the opt-in:\n{listed}"
    );
}

#[test]
fn skill_catalogue_lists_cwd_with_the_opt_in() {
    let ws = project_with_a_local_skill();
    let listed = skill_list(ws.path(), true);
    assert!(listed.contains(NAME), "{listed}");
}

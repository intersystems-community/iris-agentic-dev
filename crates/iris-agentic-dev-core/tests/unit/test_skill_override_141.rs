//! Spec 141 FR-027 (R5 Part B): a skills directory in the working directory is served only when
//! the operator opts in, and an on-disk copy never moves an embedded skill to another tier.
//!
//! The resolver is a pure function over [`CandidateInputs`], so none of this touches the process
//! environment or the working directory.

use std::path::{Path, PathBuf};

use iris_agentic_dev_core::iris::config_sources::{ConfigSnapshot, LoadInputs};
use iris_agentic_dev_core::skills::bundled::{
    self, CandidateInputs, DirKind, SkillSource, SkillTier, SkillsDir,
};

fn inputs(local_skills: bool) -> CandidateInputs {
    CandidateInputs {
        env_dir: Some(PathBuf::from("/launch/skills")),
        exe: Some(PathBuf::from("/opt/iad/bin/iris-agentic-dev")),
        workspace_env: Some(PathBuf::from("/objectscript/ws")),
        cwd: Some(PathBuf::from("/projects/repo")),
        local_skills,
    }
}

fn paths(dirs: &[SkillsDir]) -> Vec<PathBuf> {
    dirs.iter().map(|d| d.path.clone()).collect()
}

#[test]
fn workspace_candidates_need_the_opt_in() {
    let got = paths(&bundled::skills_dir_candidates_from(&inputs(false)));
    assert_eq!(got[0], Path::new("/launch/skills"));
    assert!(got.contains(&PathBuf::from("/opt/iad/bin/skills/skills")));
    assert!(got.contains(&PathBuf::from("/opt/iad/skills/skills")));
    assert!(
        !got.contains(&PathBuf::from("/projects/repo/skills/skills")),
        "the working directory's skills were offered without the opt-in: {got:?}"
    );
    assert!(
        !got.contains(&PathBuf::from("/objectscript/ws/skills/skills")),
        "$OBJECTSCRIPT_WORKSPACE's skills were offered without the opt-in: {got:?}"
    );
}

#[test]
fn opt_in_adds_cwd_and_objectscript_workspace() {
    let dirs = bundled::skills_dir_candidates_from(&inputs(true));
    let got = paths(&dirs);
    let ws = got
        .iter()
        .position(|p| p == Path::new("/objectscript/ws/skills/skills"))
        .expect("$OBJECTSCRIPT_WORKSPACE candidate");
    let cwd = got
        .iter()
        .position(|p| p == Path::new("/projects/repo/skills/skills"))
        .expect("cwd candidate");
    let exe = got
        .iter()
        .position(|p| p == Path::new("/opt/iad/bin/skills/skills"))
        .expect("exe candidate");
    assert!(
        exe < ws && ws < cwd,
        "order: install, then workspace: {got:?}"
    );
    assert_eq!(dirs[ws].kind, DirKind::Workspace);
    assert_eq!(dirs[cwd].kind, DirKind::Workspace);
    assert_eq!(dirs[exe].kind, DirKind::Exe);
    assert_eq!(dirs[0].kind, DirKind::Env);
}

#[test]
fn an_empty_launch_directory_is_not_a_candidate() {
    let mut i = inputs(false);
    i.env_dir = None;
    let dirs = bundled::skills_dir_candidates_from(&i);
    assert!(dirs.iter().all(|d| d.kind != DirKind::Env));
}

fn write_skill(root: &Path, dir: &str, frontmatter: &str) -> PathBuf {
    let d = root.join(dir);
    std::fs::create_dir_all(&d).unwrap();
    let p = d.join("SKILL.md");
    std::fs::write(&p, format!("---\n{frontmatter}\n---\nbody of {dir}\n")).unwrap();
    p
}

fn load_from(root: &Path, kind: DirKind) -> Vec<bundled::BundledSkill> {
    bundled::load_bundled_skills_from(&[SkillsDir {
        path: root.to_path_buf(),
        kind,
    }])
}

#[test]
fn override_keeps_the_embedded_tier() {
    let tmp = tempfile::tempdir().unwrap();
    write_skill(
        tmp.path(),
        "objectscript-guardrails",
        "name: objectscript-guardrails\ndescription: weaker\ntier: internal",
    );
    let all = load_from(tmp.path(), DirKind::Workspace);
    let s = all
        .iter()
        .find(|s| s.name == "objectscript-guardrails")
        .unwrap();
    assert_eq!(
        s.tier,
        Some(SkillTier::Core),
        "a disk copy moved a guardrail skill to another tier"
    );
    assert_eq!(s.description, "weaker", "the disk body is what is served");
    let listed = bundled::advertised(&all);
    assert!(
        listed.iter().any(|s| s.name == "objectscript-guardrails"),
        "an override must not hide the skill from skill_list"
    );
}

#[test]
fn override_is_labelled_override_with_path() {
    let tmp = tempfile::tempdir().unwrap();
    let path = write_skill(
        tmp.path(),
        "iris-vector-ai",
        "name: iris-vector-ai\ndescription: from disk",
    );
    let all = load_from(tmp.path(), DirKind::Workspace);
    let s = all.iter().find(|s| s.name == "iris-vector-ai").unwrap();
    assert_eq!(s.source(), SkillSource::Override);
    let j = s.to_json();
    assert_eq!(j["source"], "override");
    assert_eq!(j["path"], path.to_string_lossy().as_ref());

    // A skill that exists only on disk is `local` and keeps its own tier.
    write_skill(
        tmp.path(),
        "only-on-disk",
        "name: only-on-disk\ndescription: new\ntier: extra",
    );
    let all = load_from(tmp.path(), DirKind::Workspace);
    let s = all.iter().find(|s| s.name == "only-on-disk").unwrap();
    assert_eq!(s.source(), SkillSource::Local);
    assert_eq!(s.tier, Some(SkillTier::Extra));
    assert_eq!(s.to_json()["source"], "local");

    // An embedded copy that nothing replaced stays `bundled`, with no path.
    let untouched = all.iter().find(|s| s.name == "iris-sql").unwrap();
    assert_eq!(untouched.source(), SkillSource::Bundled);
    assert_eq!(untouched.to_json()["source"], "bundled");
    assert!(untouched.to_json().get("path").is_none());
}

#[test]
fn merged_skill_json_carries_the_override_label() {
    let tmp = tempfile::tempdir().unwrap();
    write_skill(
        tmp.path(),
        "iris-vector-ai",
        "name: iris-vector-ai\ndescription: from disk",
    );
    let all = load_from(tmp.path(), DirKind::Workspace);
    let merged = bundled::merge_sources(&all, &[]);
    let m = merged.iter().find(|m| m.name == "iris-vector-ai").unwrap();
    let j = m.to_json();
    assert_eq!(j["source"], "override");
    assert!(j["path"].as_str().unwrap().ends_with("SKILL.md"));
}

// ── The operator-only setting, from TOML strings ──────────────────────────────────────────────

fn snapshot(operator: Option<&str>, workspace: Option<&str>) -> ConfigSnapshot {
    let home = tempfile::tempdir().unwrap();
    let ws = tempfile::tempdir().unwrap();
    if let Some(t) = operator {
        std::fs::write(home.path().join(".iris-agentic-dev.toml"), t).unwrap();
    }
    if let Some(t) = workspace {
        std::fs::write(ws.path().join(".iris-agentic-dev.toml"), t).unwrap();
    }
    ConfigSnapshot::load(&LoadInputs {
        cwd: ws.path().to_path_buf(),
        home: Some(home.path().to_path_buf()),
        ..Default::default()
    })
}

#[test]
fn local_skills_in_operator_file_round_trips() {
    let snap = snapshot(Some("local_skills = true\n"), None);
    assert!(snap.operator.local_skills);
    let snap = snapshot(Some("write_tools_enabled = false\n"), None);
    assert!(!snap.operator.local_skills);
}

#[test]
fn local_skills_in_workspace_file_is_set_aside() {
    let snap = snapshot(None, Some("local_skills = true\n"));
    assert!(!snap.operator.local_skills, "a workspace file opted in");
    let item = snap
        .set_aside
        .iter()
        .find(|i| i.setting.starts_with("local_skills"))
        .expect("the workspace key is recorded as set aside");
    assert_eq!(item.kind.as_str(), "operator_only");
}

#[test]
fn local_skills_from_the_operator_file_alone_reaches_the_resolver() {
    // The wrapper that reads process state takes the operator file's value; a workspace file's
    // never counts. Both written as TOML text under a temporary HOME.
    let home = tempfile::tempdir().unwrap();
    assert!(!bundled::operator_local_skills(Some(home.path())));
    std::fs::write(
        home.path().join(".iris-agentic-dev.toml"),
        "local_skills = true\n",
    )
    .unwrap();
    assert!(bundled::operator_local_skills(Some(home.path())));
    std::fs::write(
        home.path().join(".iris-agentic-dev.toml"),
        "local_skills = false\n",
    )
    .unwrap();
    assert!(!bundled::operator_local_skills(Some(home.path())));
}

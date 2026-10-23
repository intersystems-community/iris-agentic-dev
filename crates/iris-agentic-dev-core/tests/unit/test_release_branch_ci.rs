//! A release branch gets the same checks master does, before it is merged. 1.5.0 and 1.6.0 went to
//! master red because the e2e job and Quality Gates ran only there, so each release was preceded
//! by about two hours of local runs standing in for them, and those local runs still missed a
//! shellcheck failure in 1.6.4 that only CI's Quality Gates caught. Pushing `fix/**` or
//! `release/**` now runs CI (including e2e and coverage floors), Quality Gates and the dependency
//! audit, and `docs/release-checklist.md` says to wait for them before merging.

fn workflow(name: &str) -> String {
    std::fs::read_to_string(
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../.github/workflows")
            .join(name),
    )
    .unwrap_or_else(|e| panic!("{name}: {e}"))
}

/// The `branches:` list under `on: push:`.
fn push_branches(yaml: &str) -> String {
    let push = yaml.find("\n  push:").expect("on: push:");
    let rest = &yaml[push..];
    let at = rest.find("branches:").expect("push branches");
    rest[at..rest[at..].find('\n').map(|n| at + n).unwrap()].to_string()
}

#[test]
fn release_branches_trigger_every_pre_merge_workflow() {
    for name in ["ci.yml", "gates.yml", "audit.yml"] {
        let branches = push_branches(&workflow(name));
        for pattern in ["master", "\"fix/**\"", "\"release/**\""] {
            assert!(
                branches.contains(pattern),
                "{name}: {pattern} missing from {branches}"
            );
        }
    }
}

#[test]
fn the_e2e_job_runs_on_release_branches() {
    let ci = workflow("ci.yml");
    let job = &ci[ci.find("\n  e2e-tests:").expect("e2e-tests job")..];
    let cond = &job[job.find("    if:").expect("e2e if")..];
    let cond = &cond[..cond.find('\n').unwrap()];
    for prefix in [
        "refs/heads/master",
        "refs/heads/fix/",
        "refs/heads/release/",
    ] {
        assert!(cond.contains(prefix), "{cond}");
    }
}

#[test]
fn the_checklist_waits_for_ci_on_the_release_branch() {
    let checklist = std::fs::read_to_string(
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../docs/release-checklist.md"),
    )
    .unwrap();
    assert!(
        checklist.contains("release branch"),
        "the checklist names the release-branch push"
    );
    assert!(
        checklist.contains("before merging") || checklist.contains("before you merge"),
        "the checklist says CI is green before the merge"
    );
}

//! T098/T099: every claim in the `iris-storage-mapping` skill names the live test that shows it,
//! and every test it names exists. The claims are measured by those tests, so the skill cannot
//! drift into saying something no test holds.

use std::path::PathBuf;

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn live_sources() -> String {
    let dir = root().join("crates/iris-agentic-dev-core/tests/integration");
    std::fs::read_dir(dir)
        .unwrap()
        .filter_map(|e| {
            let p = e.ok()?.path();
            (p.extension()? == "rs").then(|| std::fs::read_to_string(p).ok())?
        })
        .collect()
}

#[test]
fn test_storage_mapping_skill_172_every_cited_test_exists() {
    let skill = std::fs::read_to_string(root().join("skills/skills/iris-storage-mapping/SKILL.md"))
        .unwrap();
    let re = regex::Regex::new(r"live test:\s*`([a-z0-9_]+)`").unwrap();
    let cited: Vec<String> = re.captures_iter(&skill).map(|c| c[1].to_string()).collect();
    assert!(cited.len() >= 15, "only {} claims cite a test", cited.len());
    let live = live_sources();
    for t in &cited {
        assert!(
            live.contains(&format!("fn {t}(")),
            "the skill cites {t}, which is not a live test"
        );
    }
    // Each bullet under "What goes wrong" makes a claim, so each cites a test.
    let section = skill
        .split("## What goes wrong")
        .nth(1)
        .and_then(|s| s.split("\n## ").next())
        .expect("the section");
    for bullet in section.split("\n- ").skip(1) {
        assert!(
            bullet.contains("live test:"),
            "a claim without a test: {bullet}"
        );
    }
}

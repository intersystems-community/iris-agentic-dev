//! Skill tiers: every bundled skill is `core`, `extra` or `internal`, and every channel that lists
//! skills follows the tiers. Decisions in `docs/adr/0001-skill-tiers.md`.
//!
//! - The Claude Code plugin and a bare `skill install` ship core only.
//! - `skill install --all`, `skills.sh.json` and the install manifest carry core and extra.
//! - Internal skills appear in no list. The MCP `skill_describe` still finds them by name.
//! - The `iris-agentic-dev` skill indexes the extra skills, one line each.
//!
//! The tier lives in each `SKILL.md`'s frontmatter, so a skill moves tier by editing one line,
//! and these tests then name every list that has to follow it.

use std::collections::BTreeSet;
use std::path::PathBuf;

use iris_agentic_dev_core::skills::bundled::{self, SkillTier};

/// The core set, as decided. Changing it is a decision, so it is spelled out here rather than
/// read back from the files it is checking.
const CORE: &[&str] = &[
    "iris-agentic-dev",
    "iris-connectivity",
    "iris-sql",
    "objectscript-debugging",
    "objectscript-guardrails",
    "objectscript-list-patterns",
    "objectscript-review",
    "objectscript-sql-patterns",
    "objectscript-tdd",
    "objectscript-unit-test",
];

const INTERNAL: &[&str] = &["opencode-introspect"];

fn repo() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("repo root must exist")
}

fn read(rel: &str) -> String {
    let path = repo().join(rel);
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("reading {}: {e}", path.display()))
}

/// `tier:` from a SKILL.md's frontmatter, read as raw text so this test does not lean on the
/// parser it is also checking.
fn frontmatter_tier(text: &str) -> Option<String> {
    let rest = text.strip_prefix("---\n")?;
    let fm = &rest[..rest.find("\n---")?];
    fm.lines()
        .find_map(|l| l.strip_prefix("tier:"))
        .map(|v| v.trim().trim_matches('"').to_string())
}

/// `(name, tier)` for every `skills/skills/<name>/SKILL.md`.
fn tiers_on_disk() -> Vec<(String, Option<String>)> {
    let dir = repo().join("skills/skills");
    let mut out: Vec<_> = std::fs::read_dir(&dir)
        .expect("skills/skills must exist")
        .flatten()
        .filter(|e| e.path().join("SKILL.md").is_file())
        .map(|e| {
            let name = e.file_name().to_string_lossy().to_string();
            let text = std::fs::read_to_string(e.path().join("SKILL.md")).unwrap();
            (name, frontmatter_tier(&text))
        })
        .collect();
    out.sort();
    out
}

fn names_with(tier: &str) -> BTreeSet<String> {
    tiers_on_disk()
        .into_iter()
        .filter(|(_, t)| t.as_deref() == Some(tier))
        .map(|(n, _)| n)
        .collect()
}

fn set(names: &[&str]) -> BTreeSet<String> {
    names.iter().map(|s| s.to_string()).collect()
}

fn core_and_extra() -> BTreeSet<String> {
    names_with("core")
        .union(&names_with("extra"))
        .cloned()
        .collect()
}

#[test]
fn every_skill_declares_a_known_tier() {
    let bad: Vec<_> = tiers_on_disk()
        .into_iter()
        .filter(|(_, t)| !matches!(t.as_deref(), Some("core" | "extra" | "internal")))
        .collect();
    assert!(
        bad.is_empty(),
        "these SKILL.md files need `tier: core|extra|internal` in their frontmatter: {bad:?}"
    );
}

#[test]
fn core_is_the_ten_decided_skills() {
    assert_eq!(names_with("core"), set(CORE));
}

#[test]
fn internal_skills_are_the_decided_ones() {
    assert_eq!(names_with("internal"), set(INTERNAL));
}

#[test]
fn parser_reads_the_tier_for_every_embedded_skill() {
    let on_disk: std::collections::BTreeMap<_, _> = tiers_on_disk().into_iter().collect();
    for dir in bundled::embedded_skill_dirs() {
        let want = on_disk[dir].as_deref().map(SkillTier::parse);
        assert_eq!(
            bundled::embedded_tier(dir),
            want.flatten(),
            "{dir}: embedded tier disagrees with its frontmatter"
        );
    }
    assert_eq!(
        bundled::embedded_tier("opencode-introspect"),
        Some(SkillTier::Internal)
    );
    assert_eq!(bundled::embedded_tier("no-such-skill"), None);
}

#[test]
fn tier_parses_only_the_three_names() {
    assert_eq!(SkillTier::parse("core"), Some(SkillTier::Core));
    assert_eq!(SkillTier::parse("extra"), Some(SkillTier::Extra));
    assert_eq!(SkillTier::parse("internal"), Some(SkillTier::Internal));
    assert_eq!(SkillTier::parse("Core"), None);
    assert_eq!(SkillTier::parse(""), None);
    assert_eq!(SkillTier::Extra.as_str(), "extra");
}

#[test]
fn parse_skill_md_reads_tier_and_leaves_it_none_when_absent() {
    let with = "---\nname: a\ntier: internal\ndescription: d\n---\nbody\n";
    let s = bundled::parse_skill_md(with, "a").unwrap();
    assert_eq!(s.tier, Some(SkillTier::Internal));
    assert_eq!(s.description, "d");
    let without = "---\nname: a\ndescription: d\n---\nbody\n";
    assert_eq!(bundled::parse_skill_md(without, "a").unwrap().tier, None);
}

#[test]
fn listed_hides_internal_and_keeps_untiered() {
    let mk = |name: &str, tier| bundled::BundledSkill {
        name: name.into(),
        description: String::new(),
        tags: Vec::new(),
        tier,
        path: None,
    };
    let all = vec![
        mk("c", Some(SkillTier::Core)),
        mk("e", Some(SkillTier::Extra)),
        mk("i", Some(SkillTier::Internal)),
        mk("u", None),
    ];
    let names: Vec<_> = bundled::advertised(&all)
        .into_iter()
        .map(|s| s.name)
        .collect();
    assert_eq!(names, ["c", "e", "u"]);
}

#[test]
fn bundled_skill_json_carries_the_tier() {
    let s = bundled::load_bundled_skills()
        .into_iter()
        .find(|s| s.name == "objectscript-guardrails")
        .unwrap();
    assert_eq!(s.to_json()["tier"], "core");
}

/// The plugin ships core only, each skill by explicit path. `nopws-setup` sits under the core
/// `iris-agentic-dev` skill and ships with it.
#[test]
fn plugin_lists_exactly_the_core_skills() {
    let v: serde_json::Value = serde_json::from_str(&read(".claude-plugin/plugin.json")).unwrap();
    let got: BTreeSet<String> = v["skills"]
        .as_array()
        .expect("plugin.json skills must be an array")
        .iter()
        .map(|s| s.as_str().unwrap().to_string())
        .collect();
    let mut want: BTreeSet<String> = CORE
        .iter()
        .map(|n| format!("./skills/skills/{n}"))
        .collect();
    want.insert("./skills/skills/iris-agentic-dev/nopws-setup".into());
    assert_eq!(got, want);
}

#[test]
fn install_manifest_lists_core_and_extra_only() {
    let v: toml::Value = toml::from_str(&read("iris-agentic-dev.toml")).unwrap();
    let got: BTreeSet<String> = v["provides"]["skills"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s.as_str().unwrap().rsplit('/').next().unwrap().to_string())
        .collect();
    assert_eq!(got, core_and_extra());
}

#[test]
fn registry_manifest_lists_core_and_extra_only() {
    let v: serde_json::Value = serde_json::from_str(&read("skills.sh.json")).unwrap();
    let got: BTreeSet<String> = v["groupings"]
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|g| g["skills"].as_array().unwrap().clone())
        .map(|s| s.as_str().unwrap().to_string())
        .collect();
    assert_eq!(got, core_and_extra());
}

/// `skills/iris-dev.toml` is the older package manifest and carries a subset; it must still
/// name no internal skill.
#[test]
fn package_manifest_names_no_internal_skill() {
    let text = read("skills/iris-dev.toml");
    for n in INTERNAL {
        assert!(!text.contains(n), "skills/iris-dev.toml still names {n}");
    }
}

/// The index in the `iris-agentic-dev` skill: one `- \`name\`: when` line per extra skill, under
/// `## Extra skills`, so an agent with only core installed knows what else exists.
#[test]
fn iris_agentic_dev_indexes_every_extra_skill() {
    let text = read("skills/skills/iris-agentic-dev/SKILL.md");
    let start = text
        .find("\n## Extra skills\n")
        .expect("iris-agentic-dev SKILL.md needs an `## Extra skills` section");
    let section = &text[start + 1..];
    let section = &section[..section[3..].find("\n## ").map_or(section.len(), |i| i + 3)];
    let listed: BTreeSet<String> = section
        .lines()
        .filter_map(|l| l.strip_prefix("- `"))
        .filter_map(|l| l.split_once("`: ").map(|(n, _)| n.to_string()))
        .collect();
    assert_eq!(listed, names_with("extra"));
    for line in section.lines().filter(|l| l.starts_with("- `")) {
        let when = line.split_once("`: ").map(|(_, w)| w.trim()).unwrap_or("");
        assert!(!when.is_empty(), "index line has no description: {line}");
    }
    for n in INTERNAL {
        assert!(!section.contains(n), "the index names internal skill {n}");
    }
}

#[test]
fn docs_describe_the_new_install_default() {
    let t = read("docs/skills.md");
    for want in ["skill install --all", "skill install --prune", "core"] {
        assert!(t.contains(want), "docs/skills.md must mention {want:?}");
    }
}

// ── MCP tools ─────────────────────────────────────────────────────────────────

#[cfg(feature = "testing")]
mod mcp {
    use iris_agentic_dev_core::tools::IrisTools;

    async fn call(tool: &str, args: serde_json::Value) -> serde_json::Value {
        let tools = IrisTools::new(None).expect("IrisTools without IRIS");
        let r = tools.call_for_test(tool, args).await.expect("tool call");
        let text = r.content[0].as_text().expect("text").text.clone();
        serde_json::from_str(&text).unwrap()
    }

    fn names(v: &serde_json::Value, key: &str) -> Vec<String> {
        v[key]
            .as_array()
            .unwrap_or_else(|| panic!("no {key} array: {v}"))
            .iter()
            .map(|s| s["name"].as_str().unwrap_or_default().to_string())
            .collect()
    }

    #[tokio::test]
    async fn skill_list_hides_internal_and_shows_tiers() {
        let v = call("skill_list", serde_json::json!({})).await;
        let n = names(&v, "skills");
        assert!(!n.iter().any(|s| s == "opencode-introspect"), "{n:?}");
        assert!(n.iter().any(|s| s == "iris-query-plans"), "{n:?}");
        assert_eq!(v["count"].as_u64().unwrap() as usize, n.len());
        let g = v["skills"]
            .as_array()
            .unwrap()
            .iter()
            .find(|s| s["name"] == "objectscript-guardrails")
            .unwrap()
            .clone();
        assert_eq!(g["tier"], "core", "{g}");
    }

    #[tokio::test]
    async fn skill_search_hides_internal() {
        let v = call("skill_search", serde_json::json!({"query": "opencode"})).await;
        let n = names(&v, "results");
        assert!(!n.iter().any(|s| s == "opencode-introspect"), "{v}");
    }

    #[tokio::test]
    async fn skill_describe_still_finds_internal_by_name() {
        let v = call(
            "skill_describe",
            serde_json::json!({"name": "opencode-introspect"}),
        )
        .await;
        assert_eq!(v["success"], true, "{v}");
        assert_eq!(v["skill"]["tier"], "internal", "{v}");
    }
}

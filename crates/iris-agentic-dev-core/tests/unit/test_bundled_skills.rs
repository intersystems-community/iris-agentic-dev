//! Unit tests for bundled (on-disk / embedded) skill discovery.
//!
//! Regression cover for the discovery bug where `skill_search` / `skill_list`
//! read only the `^SKILLS` IRIS global and reported `count: 0` while 31 bundled
//! skills sat in `skills/skills/` on disk. A bare zero that means "I only looked
//! in one of two places" reads as "this does not exist".
//!
//! These are pure-logic tests: frontmatter parsing, tag matching, source merge,
//! and path resolution. No IRIS connection, no mocks.

use iris_agentic_dev_core::skills::bundled::{
    self, merge_sources, score_skill, search_bundled, BundledSkill, SkillSource,
};

// ── frontmatter parsing ───────────────────────────────────────────────────────

#[test]
fn parses_name_description_and_block_style_tags() {
    let md = "---\nname: iris-vector-ai\ndescription: Vector search stuff.\ntags:\n  - iris\n  - vector\n  - hnsw\n---\n# Body\n";
    let s = bundled::parse_skill_md(md, "fallback").expect("should parse");
    assert_eq!(s.name, "iris-vector-ai");
    assert_eq!(s.description, "Vector search stuff.");
    assert_eq!(s.tags, vec!["iris", "vector", "hnsw"]);
}

#[test]
fn parses_flow_style_tags() {
    let md = "---\nname: iris-cpf-merge\ndescription: CPF merge.\ntags: [iris, cpf, docker]\n---\n";
    let s = bundled::parse_skill_md(md, "fallback").expect("should parse");
    assert_eq!(s.tags, vec!["iris", "cpf", "docker"]);
}

#[test]
fn parses_dash_at_column_zero_tags() {
    // YAML block sequences are legal unindented under the key.
    let md = "---\nname: iris-sql\ndescription: SQL.\ntags:\n- iris\n- sql\n- quirks\n---\n";
    let s = bundled::parse_skill_md(md, "fallback").expect("should parse");
    assert_eq!(s.tags, vec!["iris", "sql", "quirks"]);
}

#[test]
fn folds_multiline_description_into_one_string() {
    // Real bundled SKILL.md files wrap `description:` across lines.
    let md = "---\nname: iris-vector-ai\ndescription: Use when writing any IRIS vector search, embedding, HNSW index, similarity\n  search, or AI feature code. Hard gate.\ntags:\n  - hnsw\n---\n";
    let s = bundled::parse_skill_md(md, "fallback").expect("should parse");
    assert!(
        s.description.contains("HNSW index, similarity search"),
        "continuation line must be folded in, got: {}",
        s.description
    );
}

#[test]
fn continuation_folding_stops_at_next_key() {
    let md =
        "---\nname: x\ndescription: first line\n  folded line\nstate: reviewed\ntags: [a]\n---\n";
    let s = bundled::parse_skill_md(md, "fallback").expect("should parse");
    assert_eq!(s.description, "first line folded line");
    assert!(!s.description.contains("reviewed"));
}

#[test]
fn nested_map_keys_do_not_leak_into_tags() {
    // `metadata:` blocks appear before `tags:` in real files and contain list items.
    let md = "---\nname: x\ndescription: d\nmetadata:\n  - not-a-tag\ntags:\n  - real-tag\n---\n";
    let s = bundled::parse_skill_md(md, "fallback").expect("should parse");
    assert_eq!(s.tags, vec!["real-tag"]);
}

#[test]
fn falls_back_to_directory_name_when_name_missing() {
    let md = "---\ndescription: no name key here\n---\n";
    let s = bundled::parse_skill_md(md, "dir-name-skill").expect("should parse");
    assert_eq!(s.name, "dir-name-skill");
}

#[test]
fn returns_none_when_no_frontmatter() {
    assert!(bundled::parse_skill_md("# Just a heading\n\ntext", "d").is_none());
}

#[test]
fn tags_are_lowercased_and_trimmed() {
    let md = "---\nname: x\ndescription: d\ntags: [ IRIS , \"HNSW\" ]\n---\n";
    let s = bundled::parse_skill_md(md, "d").expect("should parse");
    assert_eq!(s.tags, vec!["iris", "hnsw"]);
}

// ── scoring / matching ────────────────────────────────────────────────────────

fn vector_skill() -> BundledSkill {
    BundledSkill {
        name: "iris-vector-ai".into(),
        description: "Use when writing any IRIS vector search, embedding, HNSW index, similarity search, or AI feature code.".into(),
        tags: vec!["iris".into(), "vector".into(), "hnsw".into(), "embedding".into()],
        path: None,
    }
}

#[test]
fn tag_only_term_matches() {
    // The bug report's exact case: "hnsw" lives in tags.
    let s = BundledSkill {
        name: "iris-vector-ai".into(),
        description: "Nothing about the acronym here.".into(),
        tags: vec!["hnsw".into()],
        path: None,
    };
    assert!(score_skill(&s, &["hnsw".to_string()]) > 0);
}

#[test]
fn query_vector_hnsw_index_finds_vector_skill() {
    let skills = [vector_skill()];
    let results = search_bundled(&skills, "vector HNSW index", 10);
    assert_eq!(results.len(), 1, "expected iris-vector-ai to match");
    assert_eq!(results[0].0.name, "iris-vector-ai");
}

#[test]
fn name_match_outranks_description_match() {
    let named = BundledSkill {
        name: "iris-vector-ai".into(),
        description: "unrelated".into(),
        tags: vec![],
        path: None,
    };
    let described = BundledSkill {
        name: "something-else".into(),
        description: "mentions vector once".into(),
        tags: vec![],
        path: None,
    };
    let terms = vec!["vector".to_string()];
    assert!(score_skill(&named, &terms) > score_skill(&described, &terms));
}

#[test]
fn results_are_sorted_by_descending_score() {
    let skills = vec![
        BundledSkill {
            name: "weak".into(),
            description: "vector".into(),
            tags: vec![],
            path: None,
        },
        vector_skill(),
    ];
    let results = search_bundled(&skills, "vector hnsw", 10);
    assert_eq!(results[0].0.name, "iris-vector-ai");
}

#[test]
fn search_is_case_insensitive() {
    assert_eq!(search_bundled(&[vector_skill()], "HNSW", 10).len(), 1);
    assert_eq!(search_bundled(&[vector_skill()], "hnsw", 10).len(), 1);
}

#[test]
fn non_matching_query_returns_empty() {
    assert!(search_bundled(&[vector_skill()], "zzzznomatch", 10).is_empty());
}

#[test]
fn empty_query_matches_nothing_rather_than_everything() {
    assert!(search_bundled(&[vector_skill()], "   ", 10).is_empty());
}

#[test]
fn top_k_limits_results() {
    let skills = vec![vector_skill(), vector_skill(), vector_skill()];
    assert_eq!(search_bundled(&skills, "vector", 2).len(), 2);
}

#[test]
fn top_k_zero_returns_nothing() {
    assert!(search_bundled(&[vector_skill()], "vector", 0).is_empty());
}

// ── merge / dedup of the two sources ──────────────────────────────────────────

#[test]
fn merge_tags_each_entry_with_its_source() {
    let bundled_one = vec![vector_skill()];
    let synth = vec![serde_json::json!({"name": "auto-thing", "description": "synthesized"})];
    let merged = merge_sources(&bundled_one, &synth);
    assert_eq!(merged.len(), 2);
    let v = merged
        .iter()
        .find(|e| e.name == "iris-vector-ai")
        .expect("bundled entry present");
    assert_eq!(v.source, SkillSource::Bundled);
    let a = merged
        .iter()
        .find(|e| e.name == "auto-thing")
        .expect("synthesized entry present");
    assert_eq!(a.source, SkillSource::Synthesized);
}

#[test]
fn merge_dedups_by_name_preferring_bundled() {
    let bundled_one = vec![vector_skill()];
    let synth = vec![serde_json::json!({"name": "iris-vector-ai", "description": "stale copy"})];
    let merged = merge_sources(&bundled_one, &synth);
    assert_eq!(merged.len(), 1, "same name must collapse to one entry");
    assert_eq!(merged[0].source, SkillSource::Bundled);
    assert!(
        merged[0].also_synthesized,
        "collapsed entry must record that a synthesized copy also exists"
    );
}

#[test]
fn merge_handles_empty_synthesized_side() {
    let merged = merge_sources(&[vector_skill()], &[]);
    assert_eq!(merged.len(), 1);
    assert_eq!(merged[0].source, SkillSource::Bundled);
    assert!(!merged[0].also_synthesized);
}

#[test]
fn merge_handles_empty_bundled_side() {
    let synth = vec![serde_json::json!({"name": "auto-x", "description": "d"})];
    let merged = merge_sources(&[], &synth);
    assert_eq!(merged.len(), 1);
    assert_eq!(merged[0].source, SkillSource::Synthesized);
}

#[test]
fn merge_skips_synthesized_entries_without_a_name() {
    let synth = vec![serde_json::json!({"description": "nameless"})];
    assert!(merge_sources(&[], &synth).is_empty());
}

#[test]
fn merge_reads_synthesized_string_entries() {
    // ^SKILLS values are sometimes bare strings, not objects.
    let synth = vec![serde_json::json!("legacy-skill-name")];
    let merged = merge_sources(&[], &synth);
    assert_eq!(merged.len(), 1);
    assert_eq!(merged[0].name, "legacy-skill-name");
    assert_eq!(merged[0].source, SkillSource::Synthesized);
}

#[test]
fn source_serializes_to_stable_strings() {
    assert_eq!(SkillSource::Bundled.as_str(), "bundled");
    assert_eq!(SkillSource::Synthesized.as_str(), "synthesized");
}

// ── the real bundled catalog ──────────────────────────────────────────────────

#[test]
fn bundled_catalog_is_not_empty() {
    let all = bundled::load_bundled_skills();
    assert!(
        all.len() >= 25,
        "expected the bundled catalog to carry the shipped skills, got {}",
        all.len()
    );
}

#[test]
fn bundled_catalog_contains_iris_vector_ai_with_hnsw_tag() {
    let all = bundled::load_bundled_skills();
    let v = all
        .iter()
        .find(|s| s.name == "iris-vector-ai")
        .expect("iris-vector-ai must be discoverable");
    assert!(
        v.tags.iter().any(|t| t == "hnsw"),
        "hnsw tag missing, tags = {:?}",
        v.tags
    );
}

#[test]
fn real_catalog_answers_the_query_that_failed_in_production() {
    let all = bundled::load_bundled_skills();
    let results = bundled::search_bundled(&all, "vector HNSW index", 10);
    assert!(
        results.iter().any(|(s, _)| s.name == "iris-vector-ai"),
        "\"vector HNSW index\" must surface iris-vector-ai; got {:?}",
        results.iter().map(|(s, _)| &s.name).collect::<Vec<_>>()
    );
}

#[test]
fn every_bundled_skill_has_a_name_and_description() {
    for s in bundled::load_bundled_skills() {
        assert!(!s.name.is_empty(), "empty name in bundled catalog");
        assert!(
            !s.description.is_empty(),
            "{} has empty description",
            s.name
        );
    }
}

#[test]
fn bundled_catalog_names_are_unique() {
    let all = bundled::load_bundled_skills();
    let mut names: Vec<&str> = all.iter().map(|s| s.name.as_str()).collect();
    names.sort_unstable();
    let before = names.len();
    names.dedup();
    assert_eq!(before, names.len(), "duplicate names in bundled catalog");
}

#[test]
fn embedded_catalog_matches_the_skills_directory_on_disk() {
    // Guard against someone adding skills/skills/<new>/SKILL.md without
    // registering it in the embedded catalog.
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("skills")
        .join("skills");
    let mut on_disk: Vec<String> = std::fs::read_dir(&dir)
        .expect("skills/skills must exist in the repo")
        .flatten()
        .filter(|e| e.path().join("SKILL.md").is_file())
        .filter_map(|e| e.file_name().into_string().ok())
        .collect();
    on_disk.sort();

    let mut embedded: Vec<String> = bundled::embedded_skill_dirs()
        .iter()
        .map(|s| s.to_string())
        .collect();
    embedded.sort();

    assert_eq!(
        embedded, on_disk,
        "embedded bundled-skill catalog is out of sync with skills/skills/ on disk"
    );
}

/// Every tool a bundled skill tells the agent to call must actually exist.
///
/// `ensemble-production` shipped for months instructing agents to call
/// `interop_production_status`, `interop_queues`, `interop_message_search`, and
/// four more names that were consolidated into `iris_production` /
/// `iris_interop_query` by spec 036. A skill that names a tool which does not
/// exist is worse than a skill with no tool guidance: the agent burns turns on
/// calls that can only fail. Nothing caught it because no test read the skill
/// text against the tool registry.
#[test]
fn skills_only_reference_tools_that_exist() {
    use iris_agentic_dev_core::tools::{IrisTools, Toolset};

    // Baseline ∪ Merged is every real tool; Nostub is a subset of Baseline.
    let mut registered = IrisTools::new_with_toolset(None, Toolset::Baseline)
        .expect("IrisTools::new")
        .registered_tool_names();
    registered.extend(
        IrisTools::new_with_toolset(None, Toolset::Merged)
            .expect("IrisTools::new")
            .registered_tool_names(),
    );

    // Only tool-call shapes count: `name(` in prose or a fenced block. Bare
    // mentions and Python identifiers (`iris_package_name = ...`) are not calls.
    let call_shape = regex::Regex::new(r"\b([a-z][a-z0-9]*(?:_[a-z0-9]+)+)\(").unwrap();

    // Prefixes that indicate the author meant an MCP tool. Without this, every
    // ObjectScript or Python helper in a code sample would be flagged.
    let tool_prefixes = [
        "iris_",
        "interop_",
        "skill_",
        "global_",
        "kb_",
        "telemetry_",
    ];

    // Real callables that collide with the tool naming convention. Listed one
    // by one on purpose — widening the regex instead would blind the test to
    // whole families of phantom tool names.
    let not_our_tools = [
        // Helper from the `intersystems-irispython` package (iris-pgwire),
        // used to decode a `%List` in Python. Not an MCP tool.
        "iris_list_to_python",
    ];

    let mut bad: Vec<String> = Vec::new();
    for skill in bundled::load_bundled_skills() {
        let body = match skill.content() {
            Some(c) => c,
            None => continue,
        };
        for caps in call_shape.captures_iter(&body) {
            let name = caps.get(1).unwrap().as_str();
            if !tool_prefixes.iter().any(|p| name.starts_with(p)) {
                continue;
            }
            if registered.contains(name) || not_our_tools.contains(&name) {
                continue;
            }
            let entry = format!("{}: {name}", skill.name);
            if !bad.contains(&entry) {
                bad.push(entry);
            }
        }
    }
    bad.sort();

    assert!(
        bad.is_empty(),
        "bundled skills reference tools that are not registered:\n  {}",
        bad.join("\n  ")
    );
}

// ── directory resolution (must not depend on build-time paths) ────────────────

/// `IRIS_AGENTIC_DEV_SKILLS_DIR` is process-global; the tests that set it must
/// not overlap with each other.
static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

#[test]
fn discovery_does_not_depend_on_any_build_time_path() {
    // Documented past bug class: env!("CARGO_MANIFEST_DIR") and similar bake in
    // the build machine's path and break in a shipped/relocated binary. The
    // bundled catalog must resolve with no skills directory reachable at all —
    // from an unrelated working directory, with no env override.
    let _guard = ENV_LOCK.lock().unwrap();
    let tmp = tempfile::tempdir().unwrap();
    let original = std::env::current_dir().unwrap();
    std::env::remove_var("IRIS_AGENTIC_DEV_SKILLS_DIR");
    std::env::set_current_dir(tmp.path()).unwrap();

    let all = bundled::load_bundled_skills();
    let candidates = bundled::skills_dir_candidates();

    std::env::set_current_dir(&original).unwrap();

    assert!(
        all.len() >= 25,
        "catalog must resolve without any skills directory on disk, got {}",
        all.len()
    );
    for c in candidates {
        assert!(
            !c.starts_with(env!("CARGO_MANIFEST_DIR")),
            "candidate {} derives from the build-time manifest dir",
            c.display()
        );
    }
}

#[test]
fn bundled_module_source_uses_no_build_time_path_constants() {
    // Belt-and-braces on the same bug class: the only compile-time thing allowed
    // in the resolver is include_str! of the skill *contents*.
    let src = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/skills/bundled.rs");
    let text = std::fs::read_to_string(&src).expect("bundled.rs must exist");
    // Comments are allowed to name the anti-pattern; code is not.
    let code: String = text
        .lines()
        .filter(|l| !l.trim_start().starts_with("//"))
        .collect::<Vec<_>>()
        .join("\n");
    for banned in ["CARGO_MANIFEST_DIR", "OUT_DIR", "CARGO_TARGET_DIR"] {
        assert!(
            !code.contains(&format!("env!(\"{banned}\")")),
            "bundled.rs must not use env!(\"{banned}\") — breaks in a relocated binary"
        );
    }
}

#[test]
fn env_override_is_the_first_candidate() {
    let _guard = ENV_LOCK.lock().unwrap();
    let tmp = tempfile::tempdir().unwrap();
    std::env::set_var("IRIS_AGENTIC_DEV_SKILLS_DIR", tmp.path());
    let candidates = bundled::skills_dir_candidates();
    std::env::remove_var("IRIS_AGENTIC_DEV_SKILLS_DIR");
    assert_eq!(candidates.first().map(|p| p.as_path()), Some(tmp.path()));
}

#[test]
fn load_from_dir_reads_skill_md_directories() {
    let tmp = tempfile::tempdir().unwrap();
    let skill_dir = tmp.path().join("my-disk-skill");
    std::fs::create_dir_all(&skill_dir).unwrap();
    std::fs::write(
        skill_dir.join("SKILL.md"),
        "---\nname: my-disk-skill\ndescription: from disk\ntags: [disk, extra]\n---\nbody\n",
    )
    .unwrap();

    let found = bundled::load_from_dir(tmp.path());
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].name, "my-disk-skill");
    assert_eq!(found[0].tags, vec!["disk", "extra"]);
    assert!(found[0].path.is_some(), "disk skills should carry a path");
}

#[test]
fn load_from_dir_ignores_directories_without_skill_md() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(tmp.path().join("not-a-skill")).unwrap();
    assert!(bundled::load_from_dir(tmp.path()).is_empty());
}

#[test]
fn load_from_dir_on_missing_directory_is_empty_not_a_panic() {
    assert!(bundled::load_from_dir(std::path::Path::new("/nonexistent/xyz/123")).is_empty());
}

#[test]
fn disk_override_wins_over_embedded_copy_of_the_same_name() {
    let _guard = ENV_LOCK.lock().unwrap();
    let tmp = tempfile::tempdir().unwrap();
    let skill_dir = tmp.path().join("iris-vector-ai");
    std::fs::create_dir_all(&skill_dir).unwrap();
    std::fs::write(
        skill_dir.join("SKILL.md"),
        "---\nname: iris-vector-ai\ndescription: overridden on disk\ntags: [hnsw]\n---\n",
    )
    .unwrap();

    std::env::set_var("IRIS_AGENTIC_DEV_SKILLS_DIR", tmp.path());
    let all = bundled::load_bundled_skills();
    std::env::remove_var("IRIS_AGENTIC_DEV_SKILLS_DIR");

    let v = all.iter().find(|s| s.name == "iris-vector-ai").unwrap();
    assert_eq!(v.description, "overridden on disk");
    assert_eq!(
        all.iter().filter(|s| s.name == "iris-vector-ai").count(),
        1,
        "override must replace, not duplicate"
    );
}

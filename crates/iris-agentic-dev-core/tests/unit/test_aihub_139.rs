//! Spec 132: AI Hub on EAP build 139 — the parts that need no IRIS.
//!
//! The live half is `tests/integration/test_aihub_139_live.rs`. This file checks the helpers those
//! tests stand on (`AihubEnv`, the probe, the child env) and, from US2 on, the skill text itself.

use iris_agentic_dev_core::testing::{aihub_probe, clean_mcp_command, AihubEnv};
use std::collections::HashMap;
use std::path::Path;

fn vars(pairs: &[(&str, &str)]) -> impl Fn(&str) -> Option<String> {
    let m: HashMap<String, String> = pairs
        .iter()
        .map(|(k, v)| ((*k).to_string(), (*v).to_string()))
        .collect();
    move |k| m.get(k).cloned()
}

#[test]
fn aihub_env_defaults() {
    let env = AihubEnv::from_vars(vars(&[])).expect("no vars set is valid");
    assert_eq!(env.container, "iad-aihub-iris");
    assert_eq!(env.host, "localhost");
    assert_eq!(env.web_port, 52781);
    assert_eq!(env.namespace, "USER");
}

#[test]
fn aihub_env_overrides() {
    let env = AihubEnv::from_vars(vars(&[
        ("IAD_AIHUB_CONTAINER", "other-iris"),
        ("IAD_AIHUB_HOST", "10.0.0.5"),
        ("IAD_AIHUB_WEB_PORT", "8080"),
        ("IAD_AIHUB_NAMESPACE", "AIHUB"),
    ]))
    .expect("all four set is valid");
    assert_eq!(env.container, "other-iris");
    assert_eq!(env.host, "10.0.0.5");
    assert_eq!(env.web_port, 8080);
    assert_eq!(env.namespace, "AIHUB");
}

#[test]
fn aihub_env_bad_port_names_the_variable() {
    let err = AihubEnv::from_vars(vars(&[("IAD_AIHUB_WEB_PORT", "52781x")]))
        .expect_err("a non-numeric port must be refused");
    assert!(err.contains("IAD_AIHUB_WEB_PORT"), "{err}");
    assert!(err.contains("52781x"), "{err}");
}

/// The message a developer sees when 139 is down: it must say which container and how to start it,
/// or the panic is a dead end.
#[test]
fn aihub_unreachable_message_names_container_and_start() {
    let env = AihubEnv::from_vars(vars(&[])).unwrap();
    let msg = env.unreachable_message("connection refused");
    assert!(msg.contains("iad-aihub-iris"), "{msg}");
    assert!(msg.contains("iad-aihub-webgateway"), "{msg}");
    assert!(
        msg.contains("specs/132-aihub-139/quickstart.md"),
        "names where the start command is: {msg}"
    );
    assert!(msg.contains("docker start"), "{msg}");
    assert!(msg.contains("IAD_ALLOW_SKIP=1"), "{msg}");
    assert!(msg.contains("connection refused"), "keeps the cause: {msg}");
}

/// XII hermetic: the real-turn key must never reach the iad child, even when the test process has
/// it. The child only ever sees what the test adds back.
#[test]
fn clean_mcp_command_removes_openai_key() {
    let cmd = clean_mcp_command(Path::new("/nonexistent/iris-agentic-dev"));
    let removed = cmd
        .get_envs()
        .any(|(k, v)| k == "OPENAI_API_KEY" && v.is_none());
    assert!(
        removed,
        "OPENAI_API_KEY must be env_remove'd from the child"
    );
}

/// T006: nothing listens on port 1, so the probe must fail, and fail with the text that says what
/// to do. No IRIS needed.
#[test]
fn aihub_probe_unreachable_is_an_actionable_error() {
    let env = AihubEnv::from_vars(vars(&[("IAD_AIHUB_WEB_PORT", "1")])).unwrap();
    let err = aihub_probe(&env).expect_err("nothing listens on localhost:1");
    assert!(err.contains("iad-aihub-iris"), "{err}");
    assert!(err.contains("localhost:1"), "{err}");
    assert!(err.contains("quickstart.md"), "{err}");
}

// ── US2: one skill that knows where the docs are ─────────────────────────────
//
// `specs/132-aihub-139/contracts/skill-structure.md` is the contract these pin.

const UPSTREAM_COMMIT: &str = "72749d6dbf0b856a60775378fa88d346bb79d4e4";
const REPO_URL: &str = "https://github.com/intersystems-community/ai-hub-eap";
const RAW_BASE: &str =
    "https://raw.githubusercontent.com/intersystems-community/ai-hub-eap/master/";
const UPSTREAM_SKILL_RAW: &str =
    "https://raw.githubusercontent.com/intersystems-community/ai-hub-eap/master/skills/aihub-eap/SKILL.md";

/// The seven section headings, in the order the contract requires.
const SECTIONS: [&str; 7] = [
    "## Where the docs are",
    "## Topic map",
    "## Workflow",
    "## No web gateway",
    "## What holds on 2026.3",
    "## Corrections",
    "## Upstream's own skill",
];

fn repo() -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("repo root must exist")
}

fn read(rel: &str) -> String {
    let path = repo().join(rel);
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("reading {}: {e}", path.display()))
}

fn ai_hub_skill() -> String {
    read("skills/skills/iris-ai-hub/SKILL.md")
}

/// The frontmatter block between the two `---` lines.
fn frontmatter(text: &str) -> &str {
    let rest = text
        .strip_prefix("---\n")
        .expect("SKILL.md starts with ---");
    &rest[..rest.find("\n---").expect("frontmatter closes")]
}

/// The body of one `## ` section, up to the next `## ` heading.
fn section<'a>(text: &'a str, heading: &str) -> &'a str {
    let start = text
        .find(&format!("\n{heading}\n"))
        .unwrap_or_else(|| panic!("no section {heading:?}"))
        + heading.len()
        + 2;
    let end = text[start..]
        .find("\n## ")
        .map_or(text.len(), |i| start + i);
    &text[start..end]
}

/// `(header line, paths)` from the recorded upstream file list.
fn upstream_files() -> (String, Vec<String>) {
    let text = read("crates/iris-agentic-dev-core/tests/fixtures/aihub139/upstream-files.txt");
    let mut lines = text.lines();
    let header = lines.next().expect("header line").to_string();
    (header, lines.map(str::to_string).collect())
}

/// `(topic, file)` rows of the `Topic | File` table in section 2.
fn topic_map(text: &str) -> Vec<(String, String)> {
    section(text, SECTIONS[1])
        .lines()
        .filter(|l| l.starts_with('|') && !l.contains("---") && !l.contains("| Topic"))
        .map(|l| {
            let cells: Vec<&str> = l.trim_matches('|').split('|').map(str::trim).collect();
            (cells[0].to_string(), cells[1].to_string())
        })
        .collect()
}

#[test]
fn skill_frontmatter_names_both_sources() {
    let t = ai_hub_skill();
    let fm = frontmatter(&t);
    for want in [
        "name: iris-ai-hub",
        "version: 0.2.1",
        "managed_by: iris-agentic-dev",
    ] {
        assert!(
            fm.lines().any(|l| l.trim() == want),
            "frontmatter lacks {want:?}"
        );
    }
    let source = &fm[fm.find("source:").expect("frontmatter has source:")..];
    for want in [UPSTREAM_COMMIT, "391c3d5", "Gabriel Ing"] {
        assert!(source.contains(want), "source: must name {want:?}");
    }
}

#[test]
fn skill_sections_in_contract_order() {
    let t = ai_hub_skill();
    let found: Vec<Option<usize>> = SECTIONS
        .iter()
        .map(|h| t.find(&format!("\n{h}\n")))
        .collect();
    for (h, at) in SECTIONS.iter().zip(&found) {
        assert!(at.is_some(), "section {h:?} missing");
    }
    let at: Vec<usize> = found.into_iter().flatten().collect();
    assert!(
        at.windows(2).all(|w| w[0] < w[1]),
        "sections out of contract order: {SECTIONS:?} at {at:?}"
    );
}

#[test]
fn skill_says_where_the_docs_are() {
    let t = ai_hub_skill();
    let s = section(&t, SECTIONS[0]);
    for want in [REPO_URL, "`master`", &format!("{RAW_BASE}<path>")] {
        assert!(s.contains(want), "section 1 lacks {want:?}");
    }
    assert!(
        s.contains("current state"),
        "section 1 tells the agent to check the repo's current state"
    );
}

#[test]
fn skill_topic_map_is_on_the_upstream_list() {
    let t = ai_hub_skill();
    let (header, paths) = upstream_files();
    assert!(
        header.contains(UPSTREAM_COMMIT),
        "upstream-files.txt header {header:?} must be the commit the skill's source: names"
    );
    let rows = topic_map(&t);
    let topics: Vec<&str> = rows.iter().map(|(t, _)| t.as_str()).collect();
    for want in ["ConfigStore", "MCP", "SDK", "LangChain", "Samples"] {
        assert!(topics.contains(&want), "topic map lacks {want}: {topics:?}");
    }
    let mut missing = Vec::new();
    for (topic, cell) in &rows {
        let path = cell
            .strip_prefix('`')
            .and_then(|c| c.strip_suffix('`'))
            .unwrap_or_else(|| panic!("{topic}: file cell {cell:?} is not one backticked path"));
        let on_list = if path.ends_with('/') {
            paths.iter().any(|p| p.starts_with(path))
        } else {
            paths.iter().any(|p| p == path)
        };
        if !on_list {
            missing.push(path.to_string());
        }
    }
    assert!(
        missing.is_empty(),
        "not in ai-hub-eap at {UPSTREAM_COMMIT}: {missing:?}"
    );
}

#[test]
fn skill_workflow_reads_the_build_first() {
    let t = ai_hub_skill();
    let s = section(&t, SECTIONS[2]);
    let step = |n: &str| {
        s.lines()
            .find(|l| l.starts_with(n))
            .unwrap_or_else(|| panic!("workflow has no step {n}"))
    };
    let one = step("1. ");
    assert!(
        one.contains("iris_info") || one.contains("$ZVERSION"),
        "step 1 reads the version: {one}"
    );
    assert!(
        step("2. ").contains("%AI"),
        "step 2 reads the installed %AI classes"
    );
    assert!(
        s.contains("installed classes win"),
        "the build wins over the docs"
    );
    assert!(
        s.contains("tell the user"),
        "the agent reports a disagreement"
    );
    assert!(
        t.contains("GitHub is unreachable"),
        "the skill says what to do without GitHub"
    );
}

#[test]
fn skill_no_gateway_fallback_reads_the_dictionary() {
    let t = ai_hub_skill();
    let s = section(&t, SECTIONS[3]);
    for want in [
        "docker_only",
        "iris_execute",
        "%Dictionary.CompiledMethod",
        "iris_doc",
    ] {
        assert!(s.contains(want), "section 4 lacks {want:?}");
    }
}

#[test]
fn skill_points_at_upstream_skill_by_raw_url() {
    let t = ai_hub_skill();
    let s = section(&t, SECTIONS[6]);
    assert!(
        s.contains(UPSTREAM_SKILL_RAW),
        "section 7 gives the raw URL"
    );
    assert!(
        s.contains("skill_community"),
        "section 7 names skill_community"
    );
    assert!(s.contains("cannot"), "and says it cannot install it");
    assert!(s.contains("iris-agentic-dev.toml"), "and why");
}

#[test]
fn skill_names_only_real_tools() {
    let t = ai_hub_skill();
    let tools = iris_agentic_dev_core::testing::tool_names();
    let re = regex::Regex::new(r"\b((?:iris|docs)_[a-z_]+)\b").unwrap();
    let unknown: Vec<String> = re
        .captures_iter(&t)
        .map(|c| c[1].to_string())
        .filter(|n| !tools.contains(n))
        .collect();
    assert!(unknown.is_empty(), "not tools: {unknown:?}");
}

/// Build 162 is old and gone from the registry. Nothing an agent or user reads names it.
#[test]
fn no_mention_of_build_162() {
    let repo = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    for rel in [
        "skills/skills/iris-ai-hub/SKILL.md",
        "contrib/aihub/README.md",
    ] {
        let t = std::fs::read_to_string(repo.join(rel)).unwrap();
        let hits: Vec<&str> = t.lines().filter(|l| l.contains("162")).collect();
        assert!(hits.is_empty(), "{rel} names build 162: {hits:?}");
    }
}

/// The bundle carries `iris-ai-hub` and not the vendored copy.
#[test]
fn bundle_has_one_ai_hub_skill() {
    let dirs = iris_agentic_dev_core::skills::bundled::embedded_skill_dirs();
    assert!(dirs.contains(&"iris-ai-hub"));
    assert!(
        !dirs.contains(&"aihub-eap"),
        "the vendored aihub-eap is gone"
    );
}

/// No tracked file names `aihub-eap`, except upstream's own path. Recorded run results are history
/// and keep what they recorded.
#[test]
fn no_aihub_eap_left_in_the_repo() {
    let out = std::process::Command::new("git")
        .args(["ls-files", "-z"])
        .current_dir(repo())
        .output()
        .expect("git ls-files");
    let files = String::from_utf8(out.stdout).unwrap();
    let skip = [
        "specs/",
        // Release notes are history too: v1.5.0 names the skill it removed.
        "docs/release-notes/",
        "target/",
        "tests/e2e/results/",
        "tests/e2e/skill_eval/fixtures/skill-eval-",
        "crates/iris-agentic-dev-core/tests/unit/test_aihub_139.rs",
        // Checks the committed pilot record, which installed the vendored skill in its day.
        "tests/e2e/skill_eval/test_pilot.py",
    ];
    let mut hits = Vec::new();
    for f in files.split('\0').filter(|f| !f.is_empty()) {
        if skip.iter().any(|s| f.starts_with(s)) {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(repo().join(f)) else {
            continue;
        };
        for (n, line) in text.lines().enumerate() {
            if !line.contains("aihub-eap") {
                continue;
            }
            let allowed = line.replace(UPSTREAM_SKILL_RAW, "");
            let upstream_list_line =
                f.ends_with("aihub139/upstream-files.txt") && line == "skills/aihub-eap/SKILL.md";
            if allowed.contains("aihub-eap") && !upstream_list_line {
                hits.push(format!("{f}:{}: {line}", n + 1));
            }
        }
    }
    assert!(
        hits.is_empty(),
        "aihub-eap still named:\n{}",
        hits.join("\n")
    );
}

/// XI: every live filter in quickstart.md selects a real `#[ignore]` test, and every `#[ignore]`
/// test is selected by some quickstart line, so the documented commands run what exists.
#[test]
fn quickstart_filters_match_live_tests() {
    let live = read("crates/iris-agentic-dev-core/tests/integration/test_aihub_139_live.rs");
    let re = regex::Regex::new(r"#\[ignore[^\]]*\]\s*(?:async\s+)?fn ([a-z0-9_]+)").unwrap();
    let paths: Vec<String> = re
        .captures_iter(&live)
        .map(|c| format!("test_aihub_139_live::{}", &c[1]))
        .collect();
    assert!(!paths.is_empty(), "no #[ignore] tests parsed");

    let qs = read("specs/132-aihub-139/quickstart.md");
    let filters: Vec<String> = qs
        .lines()
        .filter_map(|l| {
            let after = l.split("--test integration ").nth(1)?;
            Some(after.split_whitespace().next()?.to_string())
        })
        .collect();
    assert!(
        !filters.is_empty(),
        "no integration filters in quickstart.md"
    );
    for f in &filters {
        assert!(
            paths.iter().any(|p| p.contains(f.as_str())),
            "quickstart filter {f:?} selects no #[ignore] test in test_aihub_139_live.rs"
        );
    }
    for p in &paths {
        assert!(
            filters.iter().any(|f| p.contains(f.as_str())),
            "{p} has no quickstart command"
        );
    }
}

// ── US3: every claim holds on 139 ───────────────────────────────────────────

const VERDICTS: [&str; 4] = ["holds", "false", "reworded", "not taken"];

/// One row of the claim table at the end of research.md.
struct Claim {
    id: String,
    claim: String,
    source: String,
    verdict: String,
    test: String,
    credit: String,
}

fn claims() -> Vec<Claim> {
    let text = read("specs/132-aihub-139/research.md");
    let table = &text[text
        .find("\n## Claim table\n")
        .expect("research.md has ## Claim table")..];
    table
        .lines()
        .filter(|l| l.starts_with('|') && !l.starts_with("| ---") && !l.starts_with("| id "))
        .map(|l| {
            let c: Vec<String> = l
                .trim_matches('|')
                .split(" | ")
                .map(|s| s.trim().to_string())
                .collect();
            assert_eq!(c.len(), 6, "claim row has {} cells, want 6: {l}", c.len());
            Claim {
                id: c[0].clone(),
                claim: c[1].clone(),
                source: c[2].clone(),
                verdict: c[3].clone(),
                test: c[4].clone(),
                credit: c[5].clone(),
            }
        })
        .collect()
}

#[test]
fn claim_table_rows_are_well_formed() {
    let rows = claims();
    assert!(!rows.is_empty(), "claim table is empty");
    let live = read("crates/iris-agentic-dev-core/tests/integration/test_aihub_139_live.rs");
    let mut seen = std::collections::HashSet::new();
    for r in &rows {
        assert!(seen.insert(r.id.clone()), "duplicate claim id {}", r.id);
        assert!(
            VERDICTS.contains(&r.verdict.as_str()),
            "{}: verdict {:?} not in {VERDICTS:?}",
            r.id,
            r.verdict
        );
        if r.verdict != "not taken" {
            let name = r
                .test
                .strip_prefix('`')
                .and_then(|t| t.strip_suffix('`'))
                .unwrap_or_else(|| {
                    panic!("{}: test cell {:?} is not one backticked fn", r.id, r.test)
                });
            assert!(
                live.contains(&format!("fn {name}(")),
                "{}: no fn {name} in test_aihub_139_live.rs",
                r.id
            );
        }
        if r.source.starts_with("hackathon:") {
            assert!(
                r.credit.contains("Gabriel Ing"),
                "{}: hackathon claim must credit Gabriel Ing",
                r.id
            );
        }
    }
}

/// Every `%AI.*` / `%ConfigStore.*` name the skill prints is backed by a row that holds.
#[test]
fn skill_names_only_held_classes() {
    let t = ai_hub_skill();
    let rows = claims();
    let held: Vec<&Claim> = rows
        .iter()
        .filter(|r| r.verdict == "holds" || r.verdict == "reworded")
        .collect();
    let re = regex::Regex::new(r"%(?:AI|ConfigStore|Wallet)\.[A-Za-z0-9.]*[A-Za-z0-9]").unwrap();
    let mut names: Vec<&str> = re.find_iter(&t).map(|m| m.as_str()).collect();
    names.sort_unstable();
    names.dedup();
    let unbacked: Vec<&str> = names
        .into_iter()
        .filter(|n| !held.iter().any(|r| r.claim.contains(&format!("`{n}"))))
        .collect();
    assert!(
        unbacked.is_empty(),
        "skill names these with no holds/reworded row: {unbacked:?}"
    );
}

// ── US4: doc mismatches recorded, ready for upstream ─────────────────────────

/// One `## D<n>` / `## B<n>` entry of drafts.md: heading and body.
fn drafts() -> Vec<(String, String)> {
    let text = read("specs/132-aihub-139/drafts.md");
    text.split("\n## ")
        .skip(1)
        .map(|e| {
            let (h, b) = e.split_once('\n').unwrap_or((e, ""));
            (h.trim().to_string(), b.to_string())
        })
        .collect()
}

/// The value after `- <key>: ` in an entry body.
fn field<'a>(heading: &str, body: &'a str, key: &str) -> &'a str {
    body.lines()
        .find_map(|l| l.strip_prefix(&format!("- {key}: ")))
        .unwrap_or_else(|| panic!("{heading}: no `- {key}:` line"))
        .trim()
}

fn backticked<'a>(heading: &str, cell: &'a str) -> &'a str {
    cell.strip_prefix('`')
        .and_then(|c| c.split_once('`'))
        .map(|(inner, _)| inner)
        .unwrap_or_else(|| panic!("{heading}: {cell:?} does not start with a backticked value"))
}

#[test]
fn drafts_entries_are_complete() {
    let entries = drafts();
    let live = read("crates/iris-agentic-dev-core/tests/integration/test_aihub_139_live.rs");
    let mismatches: Vec<_> = entries.iter().filter(|(h, _)| h.starts_with('D')).collect();
    assert!(
        mismatches.len() >= 4,
        "want one D entry per reproduced mismatch, got {}",
        mismatches.len()
    );
    for (h, b) in &mismatches {
        assert!(
            b.lines().any(|l| l.trim() == "- Status: drafted"),
            "{h}: every upstream entry is `Status: drafted`; nothing is filed"
        );
        assert!(
            b.contains("Draft upstream text:") && b.lines().any(|l| l.starts_with("> ")),
            "{h}: needs `Draft upstream text:` and a quoted draft"
        );
    }
    // B entries are iad's own bugs, fixed on this branch, so there is nothing to file upstream.
    let bugs: Vec<_> = entries.iter().filter(|(h, _)| h.starts_with('B')).collect();
    assert!(bugs.len() >= 3, "want B1 to B3, got {}", bugs.len());
    for (h, b) in &bugs {
        assert!(
            field(h, b, "Status").starts_with("fixed in iad, 132."),
            "{h}: a B entry says `Status: fixed in iad, 132.` and how"
        );
        assert!(
            !b.contains("Draft upstream text:"),
            "{h}: an iad bug has no upstream draft"
        );
        let test = backticked(h, field(h, b, "Test"));
        let found = ["iris-agentic-dev-core", "iris-agentic-dev-bin"]
            .iter()
            .flat_map(|c| ["unit", "integration"].map(|d| (c, d)))
            .any(|(c, d)| {
                repo()
                    .join(format!("crates/{c}/tests/{d}/{test}.rs"))
                    .is_file()
            });
        assert!(
            found,
            "{h}: Test names `{test}`, which is no test file in either crate"
        );
    }
    for (h, b) in &mismatches {
        let doc = backticked(h, field(h, b, "Doc"));
        if !h.contains("FR-008") {
            let (file, line) = doc
                .rsplit_once(':')
                .unwrap_or_else(|| panic!("{h}: Doc {doc:?} is not file:line"));
            assert!(file.ends_with(".md"), "{h}: Doc {doc:?} names a .md file");
            assert!(
                line.parse::<u32>().is_ok(),
                "{h}: Doc {doc:?} line is a number"
            );
        }
        assert!(
            !field(h, b, "Doc says").is_empty(),
            "{h}: Doc says is empty"
        );
        assert!(
            !field(h, b, "139 does").is_empty(),
            "{h}: 139 does is empty"
        );
        let test = backticked(h, field(h, b, "Test"));
        assert!(
            live.contains(&format!("fn {test}(")),
            "{h}: no fn {test} in test_aihub_139_live.rs"
        );
    }
}

/// Every mismatch has one correction line in skill section 6, found by doc file name and line.
#[test]
fn skill_corrections_match_drafts() {
    let t = ai_hub_skill();
    let s = section(&t, SECTIONS[5]);
    let lines: Vec<&str> = s.lines().filter(|l| l.starts_with("- ")).collect();
    for (h, b) in drafts()
        .iter()
        .filter(|(h, _)| h.starts_with('D') && !h.contains("FR-008"))
    {
        let doc = backticked(h, field(h, b, "Doc"));
        let (path, line) = doc.rsplit_once(':').unwrap();
        let file = path.rsplit('/').next().unwrap();
        let hit = lines.iter().find(|l| l.contains(file) && l.contains(line));
        let hit = hit.unwrap_or_else(|| panic!("{h}: no section 6 line names {file} and {line}"));
        assert!(
            hit.contains(" says ") && hit.contains("; on 2026.3 it is "),
            "{h}: section 6 line reads `… says X; on 2026.3 it is Y`: {hit}"
        );
    }
    assert_eq!(
        lines.len(),
        drafts()
            .iter()
            .filter(|(h, _)| h.starts_with('D') && !h.contains("FR-008"))
            .count(),
        "one section 6 line per mismatch entry"
    );
}

/// FR-008: upstream has no iris-agentic-dev.toml, so drafts.md asks for one.
#[test]
fn drafts_has_the_fr008_toml_request() {
    let (_, paths) = upstream_files();
    assert!(
        !paths.iter().any(|p| p.ends_with("iris-agentic-dev.toml")),
        "upstream now has an iris-agentic-dev.toml; FR-008's draft is stale"
    );
    let entries = drafts();
    let (h, b) = entries
        .iter()
        .find(|(h, _)| h.contains("FR-008"))
        .expect("drafts.md has an FR-008 entry");
    assert!(
        b.contains("iris-agentic-dev.toml"),
        "{h}: names the file it asks for"
    );
    assert!(
        b.contains("skill_community"),
        "{h}: says what the file enables"
    );
}

/// Build 154 (2026-10-02). `%CanExecute` takes `toolref`, the string the tool was registered under,
/// where 139 passed the tool spec as JSON. `StreamChat` lost `callbackMethod`: its callback extends
/// `%AI.Shell.StreamRenderer`. A fresh instance needs `RebuildRegistry` before any `AI.LLM` Create.
/// The live proof is `aihub_139_policies`, `aihub_139_class_inventory` and
/// `aihub_139_agent_providerconfig`.
#[test]
fn skill_carries_the_154_changes() {
    let t = ai_hub_skill();
    for good in [
        "toolref",
        "%AI.Shell.StreamRenderer",
        "RebuildRegistry",
        "#26414",
        "Build 154",
    ] {
        assert!(t.contains(good), "iris-ai-hub must contain {good:?}");
    }
    for bad in [
        "callbackObj, callbackMethod)",
        "`tool` is the tool spec as JSON",
    ] {
        assert!(!t.contains(bad), "iris-ai-hub still contains {bad:?}");
    }
}

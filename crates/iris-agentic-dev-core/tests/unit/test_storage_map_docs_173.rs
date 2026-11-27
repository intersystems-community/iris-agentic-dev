//! Spec 173, T060 and T061: `docs/storage-maps.md` and the `iris-storage-mapping` skill say what
//! `propose` does, and only what a test holds.
//!
//! - Every response key, parameter and verdict of `data-model.md` is in the doc, and every key of
//!   its entity tables and shapes is one the code emits (the spec cannot drift from the code).
//! - Every `fact_*`, `r*_` and `open_*` test the doc's measured table cites is in
//!   `test_storage_map_measured_173_live.rs`, and every test in that file is cited.
//! - Every claim of the skill's "What `propose` infers and what it does not" names a live test that
//!   exists.

use std::collections::BTreeSet;
use std::path::PathBuf;

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn read(rel: &str) -> String {
    std::fs::read_to_string(root().join(rel)).unwrap_or_else(|e| panic!("{rel}: {e}"))
}

/// Every `.rs` file directly under `rel`, read.
fn rs_files(rel: &str) -> String {
    let mut out = String::new();
    for e in std::fs::read_dir(root().join(rel)).unwrap() {
        let p = e.unwrap().path();
        if p.extension().is_some_and(|x| x == "rs") {
            out.push_str(&std::fs::read_to_string(&p).unwrap());
        }
    }
    out
}

const MODEL: &str = "specs/173-propose-index-linking/data-model.md";
const MEASURED: &str =
    "crates/iris-agentic-dev-core/tests/integration/test_storage_map_measured_173_live.rs";
const INTEGRATION: &str = "crates/iris-agentic-dev-core/tests/integration";
const STORAGE_SRC: &str = "crates/iris-agentic-dev-core/src/tools/storage_map";

/// The text under the heading line that starts with `heading`, up to the next heading.
fn section<'a>(text: &'a str, heading: &str) -> &'a str {
    let start = text
        .lines()
        .scan(0usize, |pos, l| {
            let at = *pos;
            *pos += l.len() + 1;
            Some((at, l))
        })
        .find(|(_, l)| l.starts_with(heading))
        .unwrap_or_else(|| panic!("no heading {heading}"))
        .0;
    let rest = &text[start..];
    let body = rest.find('\n').map_or(rest.len(), |i| i + 1);
    let end = rest[body..]
        .find("\n#")
        .map_or(rest.len(), |i| body + i + 1);
    &rest[..end]
}

/// Backticked words made of lower-case letters, digits and underscores.
fn backticked_words(text: &str) -> Vec<String> {
    let re = regex::Regex::new(r"`([a-z][a-z0-9_]*)`").unwrap();
    re.captures_iter(text).map(|c| c[1].to_string()).collect()
}

/// The backticked words of the first cell of every table row in `text`.
fn first_column(text: &str) -> Vec<String> {
    text.lines()
        .filter(|l| l.starts_with('|') && !l.contains("---"))
        .filter_map(|l| l.split('|').nth(1))
        .flat_map(backticked_words)
        .collect()
}

/// The keys named inside a code span that begins `{` or `[{`: `{form: "x", positions: [int]}` names
/// `form` and `positions`. A key is a word followed by `:`, `?` or `[`, or one in a bare list
/// (`{global, chunk_position}`).
fn brace_keys(text: &str) -> BTreeSet<String> {
    let span = regex::Regex::new(r"`(\[?\{[^`]*\}\]?\??)`").unwrap();
    let quoted = regex::Regex::new(r#""[^"]*""#).unwrap();
    let word = regex::Regex::new(r"[a-z][a-z0-9_]*").unwrap();
    let mut keys = BTreeSet::new();
    for c in span.captures_iter(text) {
        let s = quoted
            .replace_all(&c[1], "")
            .replace("\\|", "")
            .replace('|', "");
        for m in word.find_iter(&s) {
            let before = s[..m.start()].trim_end();
            let after = &s[m.end()..];
            let named = after.starts_with([':', '?', '[']);
            let listed = before.ends_with(['{', ','])
                && after.trim_start().starts_with([',', '}'])
                && !before.ends_with(':');
            if named || listed {
                keys.insert(m.as_str().to_string());
            }
        }
    }
    keys
}

/// Every key, parameter, verdict and enum value `data-model.md` names, and the ones that are
/// response or parameter keys the code must emit.
fn model_words() -> (BTreeSet<String>, BTreeSet<String>) {
    let model = read(MODEL);
    let mut every: BTreeSet<String> = BTreeSet::new();
    let mut emitted: BTreeSet<String> = BTreeSet::new();
    every.extend(first_column(&model));
    every.extend(
        model
            .lines()
            .filter(|l| l.starts_with('#'))
            .flat_map(backticked_words),
    );
    every.extend(
        backticked_words(&model)
            .into_iter()
            .filter(|w| w.contains('_') && w != "validate_global_name"),
    );
    let braces = brace_keys(&model);
    every.extend(braces.iter().cloned());
    emitted.extend(braces);
    for heading in ["### Candidate", "### Link", "## New parameters"] {
        emitted.extend(first_column(section(&model, heading)));
    }
    emitted.extend(
        model
            .lines()
            .filter(|l| l.starts_with("### "))
            .flat_map(backticked_words),
    );
    (every, emitted)
}

#[test]
fn test_storage_map_docs_173_every_key_of_the_data_model_is_in_the_doc() {
    let doc = read("docs/storage-maps.md");
    let (every, _) = model_words();
    assert!(
        every.len() >= 60,
        "the data model named only {} words: {every:?}",
        every.len()
    );
    // The keys the lead's brief and the contract name must be among them, so a parser that read
    // nothing cannot pass.
    for must in [
        "index_candidates",
        "indexes",
        "bitmaps",
        "chunk_map_draft",
        "class_draft",
        "limits",
        "not_inferred_detail",
        "usability",
        "orphan_nodes",
        "unexamined_positions",
        "missing_rows",
        "rows_kept",
        "null_marker",
        "verified",
        "sub_trees_not_looked_for",
        "listing_reason",
        "not_examined",
        "index_globals",
        "field_names",
        "ambiguous",
        "too_few_nodes",
    ] {
        assert!(every.contains(must), "data-model.md no longer names {must}");
    }
    let missing: Vec<&String> = every
        .iter()
        .filter(|w| !doc.contains(&format!("`{w}`")))
        .collect();
    assert!(
        missing.is_empty(),
        "docs/storage-maps.md leaves out what data-model.md names: {missing:?}"
    );
}

#[test]
fn test_storage_map_docs_173_every_key_of_the_data_model_is_one_the_code_emits() {
    let (_, emitted) = model_words();
    assert!(emitted.len() >= 40, "{emitted:?}");
    let src = rs_files(STORAGE_SRC);
    let never: Vec<&String> = emitted
        .iter()
        .filter(|k| !src.contains(&format!("\"{k}\"")) && !src.contains(&format!("pub {k}:")))
        .collect();
    assert!(
        never.is_empty(),
        "data-model.md names keys no code emits: {never:?}"
    );
}

/// The doc's measured table: its `fact_*`, `r*_` and `open_*` cells.
fn cited_measured(doc: &str) -> BTreeSet<String> {
    let re = regex::Regex::new(r"`((?:fact_\d+|r\d+|open)_[a-z0-9_]+)`").unwrap();
    re.captures_iter(doc).map(|c| c[1].to_string()).collect()
}

#[test]
fn test_storage_map_docs_173_the_measured_table_cites_every_measured_test_and_only_them() {
    let doc = read("docs/storage-maps.md");
    let measured = read(MEASURED);
    let re = regex::Regex::new(r"(?m)^async fn ((?:fact_\d+|r\d+|open)_[a-z0-9_]+)\(").unwrap();
    let defined: BTreeSet<String> = re
        .captures_iter(&measured)
        .map(|c| c[1].to_string())
        .collect();
    let cited = cited_measured(&doc);
    assert!(defined.len() >= 17, "the measured file has {defined:?}");
    // Facts 1 to 12 each have a test, and each is cited.
    for n in 1..=12 {
        let prefix = format!("fact_{n}_");
        assert!(
            defined.iter().any(|d| d.starts_with(&prefix)),
            "no test for fact {n}"
        );
    }
    let uncited: Vec<&String> = defined.difference(&cited).collect();
    let unknown: Vec<&String> = cited.difference(&defined).collect();
    assert!(
        uncited.is_empty(),
        "measured tests the doc does not cite: {uncited:?}"
    );
    assert!(
        unknown.is_empty(),
        "cited, and not in the measured file: {unknown:?}"
    );
}

#[test]
fn test_storage_map_docs_173_the_doc_has_the_params_the_verdicts_and_the_rules() {
    let doc = read("docs/storage-maps.md");
    for p in [
        "index_globals",
        "class",
        "field_names",
        "max_nodes",
        "include_values",
    ] {
        assert!(doc.contains(&format!("`{p}`")), "no param {p}");
    }
    for v in [
        "inferred",
        "ambiguous",
        "too_few_nodes",
        "not_linked",
        "not_explained",
        "constant",
    ] {
        assert!(doc.contains(&format!("| `{v}`")), "no verdict row for {v}");
    }
    // The rules that wrong guesses came from.
    for rule in [
        "{F}'=$c(0)",
        "skips NULL only",
        "digits only",
        "`assumed`",
        "`used`",
    ] {
        assert!(doc.contains(rule), "the doc does not say {rule:?}");
    }
}

/// The skill's section of claims.
fn skill_claims() -> Vec<String> {
    let skill = read("skills/skills/iris-storage-mapping/SKILL.md");
    let heading = "## What `propose` infers and what it does not";
    let start = skill.find(heading).expect("the skill has the section");
    let rest = &skill[start + heading.len()..];
    let body = &rest[..rest.find("\n## ").unwrap_or(rest.len())];
    body.split("\n- ").skip(1).map(str::to_string).collect()
}

#[test]
fn test_storage_map_docs_173_every_claim_of_the_skill_cites_a_live_test_that_exists() {
    let claims = skill_claims();
    assert!(claims.len() >= 10, "only {} claims", claims.len());
    let live = rs_files(INTEGRATION);
    let cite = regex::Regex::new(r"live test:\s*`([a-z0-9_]+)`").unwrap();
    for claim in &claims {
        let cited: Vec<String> = cite
            .captures_iter(claim)
            .map(|c| c[1].to_string())
            .collect();
        assert!(!cited.is_empty(), "a claim without a test: {claim}");
        for t in cited {
            assert!(
                live.contains(&format!("fn {t}(")),
                "the skill cites {t}, which is not a live test"
            );
        }
    }
    // The claims the section is for.
    let text = claims.join("\n");
    for word in [
        "inferred",
        "ambiguous",
        "too_few_nodes",
        "not_linked",
        "not_explained",
        "constant",
        "check",
        "class",
        "bitmap",
        "NULL",
    ] {
        assert!(text.contains(word), "the skill's claims never say {word}");
    }
}

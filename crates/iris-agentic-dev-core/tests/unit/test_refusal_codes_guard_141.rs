//! Spec 141 T207: `REFUSAL_CODES` names exactly the refusals the source emits.
//!
//! `classify_refusal` adds `refusal {kind, lift, ...}` only to a code in `REFUSAL_CODES`, and the
//! pipeline's `classify_answer` does the same for a handler's own answer. A refusal that is missing
//! from the list reaches the agent as a bare error with no `lift`, and nothing fails. Two such
//! codes shipped (`SERVICE_ACCOUNT_TERMINAL_DENIED`, `STORED_SOURCE_UNREADABLE`).
//!
//! Definitions, written down so the guard cannot drift into a heuristic:
//!
//! - A code is **emitted** when non-test source under `src/` (text before the first
//!   `#[cfg(test)]`, `policy/refusal.rs` itself left out) holds it as a string literal made of
//!   upper-case words joined by underscores. That takes in `err_json("CODE"`, `"error_code":
//!   "CODE"`, `const ERR_X: &str = "CODE"` and a match arm that returns the code alike. It also
//!   takes in an environment variable name; the refusal-shaped filter below is what keeps those
//!   out of the second check, and the first check only needs a listed code to appear.
//! - A code is **refusal-shaped** when its name ends in `_BLOCKED`, `_DISABLED`, `_REQUIRED`,
//!   `_DENIED`, `_WITHHELD`, `_MISMATCH` or `_CONFLICT`, starts with `SERVICE_ACCOUNT_`, or is one
//!   of the old irregular names in [`IRREGULAR`].
//! - The spec's error code registry (`data-model.md`) gives a kind to the codes 1.7.0 added.
//!   `not a refusal` rows are not refusals; every other kind is.
//!
//! The guard fails when: a listed code is never emitted; an emitted refusal-shaped code is neither
//! listed nor in [`NOT_REFUSALS`] with a reason; a registry row disagrees with the list; or a
//! code is in both [`NOT_REFUSALS`] and the list.

use iris_agentic_dev_core::policy::refusal::REFUSAL_CODES;
use regex::Regex;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

const SUFFIXES: [&str; 7] = [
    "_BLOCKED",
    "_DISABLED",
    "_REQUIRED",
    "_DENIED",
    "_WITHHELD",
    "_MISMATCH",
    "_CONFLICT",
];

/// Refusals whose names carry none of the suffixes.
const IRREGULAR: [&str; 6] = [
    "POLICY_GATE",
    "SYSTEM_BLOCKLIST",
    "READ_ONLY_STRICT",
    "BODY_NOT_REDACTABLE",
    "SCM_NEEDS_UI",
    "SCM_NEEDS_INPUT",
];

/// Emitted codes that look like refusals and are not: an input error, a missing capability, a
/// failure inside IRIS, or a server error. Each carries the reason.
const NOT_REFUSALS: [(&str, &str); 10] = [
    (
        "CONFIRM_MISMATCH",
        "iris_admin: the token names another global, an input error",
    ),
    (
        "COVERAGE_PRIVILEGE_DENIED",
        "IRIS refused the coverage monitor to the account",
    ),
    (
        "DOCKER_REQUIRED",
        "no docker route for a tool that needs one, a capability error",
    ),
    (
        "ELICITATION_SERVER_MISMATCH",
        "data-model.md: not a refusal, a resume for another server",
    ),
    (
        "LEARNING_DISABLED",
        "the skill-learning feature is off, a capability error",
    ),
    (
        "MIRROR_VERSION_MISMATCH",
        "the IRIS build lacks %SYSTEM.Mirror, a capability error",
    ),
    (
        "NOPWS_ATELIER_REQUIRED",
        "a tool that needs Atelier on a server without it",
    ),
    (
        "SCOPE_REQUIRED",
        "iris_search: the query names no scope, an input error",
    ),
    (
        "SERVICE_ACCOUNT_CLIENT_FAILED",
        "the HTTP client for the account could not be built, an internal error",
    ),
    (
        "SERVICE_ACCOUNT_INVALID",
        "data-model.md: not a refusal, the configured name is malformed",
    ),
];

/// The codes one file's text emits (see the module comment).
fn emitted_codes(text: &str) -> BTreeSet<String> {
    let live = text.split("#[cfg(test)]").next().unwrap_or(text);
    let literal = Regex::new(r#""([A-Z][A-Z0-9]*(?:_[A-Z0-9]+)+)""#).unwrap();
    literal
        .captures_iter(live)
        .map(|c| c[1].to_string())
        .collect()
}

fn is_refusal_shaped(code: &str) -> bool {
    SUFFIXES.iter().any(|s| code.ends_with(s))
        || code.starts_with("SERVICE_ACCOUNT_")
        || IRREGULAR.contains(&code)
}

fn source_files(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            source_files(&path, out);
        } else if path.extension().is_some_and(|e| e == "rs") {
            out.push(path);
        }
    }
}

fn emitted_in_source() -> BTreeSet<String> {
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut files = Vec::new();
    source_files(&src, &mut files);
    let mut all = BTreeSet::new();
    for f in files.iter().filter(|f| !f.ends_with("policy/refusal.rs")) {
        all.extend(emitted_codes(&std::fs::read_to_string(f).unwrap()));
    }
    all
}

#[test]
fn the_scan_reads_literals_and_stops_at_the_test_module() {
    let text = r##"
        pub const ERR_ONE_THING: &str = "ONE_THING";
        fn a() { err_json("THREE_THING", "m"); }
        fn b() { json!({ "error_code": "FIVE_THING" }); let x = "lower_case"; let y = "ONEWORD"; }
        fn c() -> &'static str { match k { A => "SIX_THING", } }
        #[cfg(test)]
        mod tests { fn t() { err_json("SEVEN_THING", "m"); } }
    "##;
    let got: Vec<String> = emitted_codes(text).into_iter().collect();
    assert_eq!(
        got,
        ["FIVE_THING", "ONE_THING", "SIX_THING", "THREE_THING"],
        "single words, lower case and the test module are left out"
    );
}

#[test]
fn every_listed_refusal_is_emitted_somewhere() {
    let emitted = emitted_in_source();
    assert!(
        emitted.len() >= 150,
        "the scan found {} codes; it is not reading src/",
        emitted.len()
    );
    let dead: Vec<&&str> = REFUSAL_CODES
        .iter()
        .filter(|c| !emitted.contains(**c))
        .collect();
    assert!(
        dead.is_empty(),
        "listed in REFUSAL_CODES and never emitted by src/: {dead:?}"
    );
}

#[test]
fn every_emitted_refusal_shaped_code_is_listed_or_excused() {
    let emitted = emitted_in_source();
    let missing: Vec<&String> = emitted
        .iter()
        .filter(|c| is_refusal_shaped(c))
        .filter(|c| !REFUSAL_CODES.contains(&c.as_str()))
        .filter(|c| !NOT_REFUSALS.iter().any(|(n, _)| n == c))
        .collect();
    assert!(
        missing.is_empty(),
        "emitted, refusal-shaped, and neither in REFUSAL_CODES nor in NOT_REFUSALS: {missing:?}"
    );
}

#[test]
fn the_excuses_are_current() {
    let emitted = emitted_in_source();
    for (code, reason) in NOT_REFUSALS {
        assert!(!reason.is_empty());
        assert!(
            !REFUSAL_CODES.contains(&code),
            "{code} is excused as a non-refusal and is listed as one"
        );
        assert!(
            emitted.contains(code),
            "{code} is excused and no longer emitted: drop it from NOT_REFUSALS"
        );
        assert!(
            is_refusal_shaped(code),
            "{code} is excused but is not refusal-shaped: drop it from NOT_REFUSALS"
        );
    }
}

#[test]
fn the_registry_in_the_data_model_agrees_with_the_list() {
    let text = std::fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/error-codes.md"),
    )
    .expect("the data model");
    let section = text
        .split("## Error code registry")
        .nth(1)
        .and_then(|s| s.split("\n## ").next())
        .expect("the error code registry section");
    let row = Regex::new(r"(?m)^\|\s*`([A-Z][A-Z0-9_]+)`\s*\|\s*([^|]+?)\s*\|").unwrap();
    let mut rows = 0;
    let mut wrong = Vec::new();
    for r in row.captures_iter(section) {
        rows += 1;
        let (code, kind) = (&r[1], &r[2]);
        let is_refusal = match kind {
            k if k.starts_with("not a refusal") => false,
            "acknowledgement required" | "fixed rule" | "operator limit" | "untrusted setting" => {
                true
            }
            other => panic!("{code}: registry kind {other:?} is not one this guard knows"),
        };
        if REFUSAL_CODES.contains(&code) != is_refusal {
            wrong.push(format!("{code} ({kind})"));
        }
    }
    assert!(rows >= 20, "the scan read {rows} registry rows");
    assert!(
        wrong.is_empty(),
        "the registry and REFUSAL_CODES disagree on: {wrong:?}"
    );
}

#[test]
fn the_two_codes_that_were_missing_now_carry_a_refusal() {
    use iris_agentic_dev_core::policy::refusal::classify_refusal;
    for (code, kind) in [
        ("SERVICE_ACCOUNT_TERMINAL_DENIED", "operator_limit"),
        ("STORED_SOURCE_UNREADABLE", "fixed_rule"),
    ] {
        let body = serde_json::json!({"success": false, "error_code": code, "error": "x"});
        let out = classify_refusal(body, "default", &[]);
        assert_eq!(out["refusal"]["kind"], kind, "{code}: {out}");
    }
}

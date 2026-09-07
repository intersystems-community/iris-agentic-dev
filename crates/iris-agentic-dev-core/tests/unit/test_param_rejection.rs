//! Layer 1 tests for the `UNKNOWN_PARAMETER` message builder (113 US4, FR-015).
//!
//! The message is the whole point of the feature. A closed schema that answers "rejected" and
//! nothing else trades a silent wrong answer for a mute one; SC-008 asks that the error alone be
//! enough to correct the call without opening `docs/tools.md`. So these tests pin the four things
//! the contract asks the text to carry — the offending name as the caller spelled it, the accepted
//! names sorted, *every* unknown key rather than the first, and a near-match suggestion when there
//! is one — and one test pins the assembled string against the example in
//! `specs/113-typed-tool-schemas/contracts/rejection-response.md` character for character, so a
//! reworded message has to be a deliberate edit to the contract rather than a drift.

use iris_agentic_dev_core::tools::param_check::{unknown_keys, unknown_parameter_message};
use std::collections::BTreeSet;

/// `iris_query`'s accepted set, as the contract's worked example spells it. Cross-checked against
/// the router in `the_contract_example_is_iris_query_s_real_accepted_set`, so this literal cannot
/// quietly stop describing the tool.
fn iris_query_accepted() -> BTreeSet<String> {
    [
        "confirm",
        "force",
        "max_rows_affected",
        "mode",
        "namespace",
        "parameters",
        "query",
        "server",
        "table",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect()
}

fn set(names: &[&str]) -> BTreeSet<String> {
    names.iter().map(|s| s.to_string()).collect()
}

#[test]
fn the_message_matches_the_contract_example_exactly() {
    let msg = unknown_parameter_message(
        "iris_query",
        &["namesapce".to_string()],
        &iris_query_accepted(),
    );
    assert_eq!(
        msg,
        "iris_query does not accept \"namesapce\". Accepted parameters: confirm, force, \
         max_rows_affected, mode, namespace, parameters, query, server, table. \
         Did you mean \"namespace\"?",
        "the message must match contracts/rejection-response.md verbatim"
    );
}

#[test]
fn the_contract_example_is_iris_query_s_real_accepted_set() {
    let tools = iris_agentic_dev_core::tools::IrisTools::new(None).expect("IrisTools::new");
    let schema = tools
        .tool_input_schema("iris_query")
        .expect("iris_query must be registered");
    let advertised: BTreeSet<String> = schema
        .get("properties")
        .and_then(|p| p.as_object())
        .expect("iris_query must advertise properties")
        .keys()
        .cloned()
        .collect();
    assert_eq!(
        advertised,
        iris_query_accepted(),
        "the contract's worked example no longer lists iris_query's parameters; update both"
    );
}

#[test]
fn the_offending_key_is_quoted_as_the_caller_spelled_it() {
    // Not lowercased, not normalized. A caller who typed `NameSpace` needs to find that string in
    // the error, otherwise they cannot tell which of their keys the server objected to.
    let msg = unknown_parameter_message(
        "iris_query",
        &["NameSpace".to_string()],
        &iris_query_accepted(),
    );
    assert!(
        msg.contains("does not accept \"NameSpace\""),
        "expected the caller's own spelling, quoted; got: {msg}"
    );
}

#[test]
fn accepted_names_are_listed_sorted() {
    let msg = unknown_parameter_message("iris_query", &["zzz".to_string()], &iris_query_accepted());
    let list = msg
        .split("Accepted parameters: ")
        .nth(1)
        .and_then(|s| s.split('.').next())
        .expect("message must carry an accepted-parameters list");
    let names: Vec<&str> = list.split(", ").collect();
    let mut sorted = names.clone();
    sorted.sort_unstable();
    assert_eq!(names, sorted, "accepted names must be sorted: {list}");
    assert_eq!(names.len(), 9, "all nine names must be listed: {list}");
}

#[test]
fn a_tool_that_accepts_nothing_says_so_rather_than_printing_an_empty_list() {
    // The six no-argument tools advertise `properties: {}`. "Accepted parameters: ." reads like a
    // formatting bug; the caller needs to be told the tool takes no arguments at all.
    let msg = unknown_parameter_message("skill_list", &["namespace".to_string()], &set(&[]));
    assert!(
        msg.contains("accepts no parameters"),
        "expected an explicit no-parameters statement; got: {msg}"
    );
    assert!(
        !msg.contains("Accepted parameters: ."),
        "empty list rendered as punctuation: {msg}"
    );
}

#[test]
fn every_unknown_key_is_reported_not_only_the_first() {
    // One typo fixed per round-trip is the failure mode this feature exists to remove.
    let msg = unknown_parameter_message(
        "iris_query",
        &["namesapce".to_string(), "frobnicate".to_string()],
        &iris_query_accepted(),
    );
    assert!(
        msg.contains("\"namesapce\""),
        "first unknown key missing: {msg}"
    );
    assert!(
        msg.contains("\"frobnicate\""),
        "second unknown key missing: {msg}"
    );
}

#[test]
fn a_near_match_is_suggested_at_edit_distance_one() {
    for typo in [
        "namesapce",
        "namespac",
        "namespacee",
        "Namespace",
        "nomespace",
    ] {
        let msg =
            unknown_parameter_message("iris_query", &[typo.to_string()], &iris_query_accepted());
        assert!(
            msg.contains("Did you mean \"namespace\"?"),
            "{typo} is one edit from `namespace` and should have been suggested; got: {msg}"
        );
    }
}

#[test]
fn nothing_is_suggested_when_nothing_is_close() {
    let msg = unknown_parameter_message(
        "iris_query",
        &["frobnicate".to_string()],
        &iris_query_accepted(),
    );
    assert!(
        !msg.contains("Did you mean"),
        "a guess at two or more edits is noise, not help; got: {msg}"
    );
}

#[test]
fn a_suggestion_per_unknown_key_names_the_key_it_belongs_to() {
    // With one unknown key `Did you mean "namespace"?` is unambiguous. With two it is not, so the
    // sentence has to say which key each suggestion replaces.
    let msg = unknown_parameter_message(
        "iris_query",
        &["namesapce".to_string(), "qeury".to_string()],
        &iris_query_accepted(),
    );
    assert!(
        msg.contains("Did you mean \"namespace\" instead of \"namesapce\"?"),
        "missing the paired suggestion for namesapce: {msg}"
    );
    assert!(
        msg.contains("Did you mean \"query\" instead of \"qeury\"?"),
        "missing the paired suggestion for qeury: {msg}"
    );
}

#[test]
fn the_suggestion_is_deterministic_when_two_names_tie() {
    // `xode` is one edit from both `mode` and `code`, so the builder has to pick. It picks the
    // lexicographically smallest name, every time — an error message that varies between runs is
    // an error message nobody can write a test or a runbook against.
    let accepted = set(&["code", "mode"]);
    for _ in 0..5 {
        let msg = unknown_parameter_message("t", &["xode".to_string()], &accepted);
        assert!(
            msg.contains("Did you mean \"code\"?"),
            "ties must resolve to the lexicographically smallest name: {msg}"
        );
    }
}

#[test]
fn unknown_keys_diffs_the_arguments_against_the_accepted_set() {
    let accepted = iris_query_accepted();
    let args = serde_json::json!({"query": "SELECT 1", "namesapce": "USER", "zz": 1});
    let args = args.as_object().unwrap().clone();
    assert_eq!(
        unknown_keys(Some(&args), &accepted),
        vec!["namesapce".to_string(), "zz".to_string()],
        "unknown keys must come back sorted, with advertised keys filtered out"
    );
}

#[test]
fn no_arguments_and_empty_arguments_are_both_fine() {
    let accepted = iris_query_accepted();
    assert!(unknown_keys(None, &accepted).is_empty());
    let empty = serde_json::Map::new();
    assert!(unknown_keys(Some(&empty), &accepted).is_empty());
}

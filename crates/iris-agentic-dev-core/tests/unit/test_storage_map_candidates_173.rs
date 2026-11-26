//! Spec 173, T021: the candidates `propose` examines for index links: where they come from
//! (`index_globals`, the namespace listing, sub-trees of the master), how duplicates and the cap
//! are handled, the refusal of an `index_globals` entry that is not a plain global name, and the
//! ObjectScript that lists the namespace's globals.

use iris_agentic_dev_core::objectscript::os_str_expr;
use iris_agentic_dev_core::tools::storage_map::candidates::{
    cap, dedupe, from_listing, from_parameter, from_sub_nodes, listing_code, parse_listing,
    validate_index_globals, Candidate, FoundBy, Status, MAX_CANDIDATES,
};
use iris_agentic_dev_core::tools::storage_map::sample::parse;

fn strings(v: &[&str]) -> Vec<String> {
    v.iter().map(|s| s.to_string()).collect()
}

fn cand(global: &str, found_by: FoundBy) -> Candidate {
    Candidate::new(global, found_by)
}

// ---------------------------------------------------------------------------
// from_sub_nodes
// ---------------------------------------------------------------------------

/// Rows 1 and 2, and two literal first subscripts holding sub-trees (`idx`, `byCity`), and one
/// literal first subscript holding only a value (`count`).
const WITH_SUB_TREES: &str = r#"{"d":1,"s":1,"f":"delimited","dl":94,"p":2}
{"d":1,"s":1,"f":"delimited","dl":94,"p":2}
{"d":1,"s":1,"n":"count","f":"plain","p":1}
{"d":1,"s":10,"n":"idx"}
{"d":3,"s":1,"f":"plain","p":1}
{"d":1,"s":10,"n":"byCity"}
{"d":2,"s":1,"f":"plain","p":1}
{"done":1,"c":7,"capped":0}
"#;

#[test]
fn test_candidates_173_sub_nodes_are_the_literal_first_subscripts_with_sub_trees() {
    let mut report = parse(WITH_SUB_TREES).unwrap();
    report.global = "^IadSm173Person".into();
    let got = from_sub_nodes(&report);
    let labels: Vec<(String, Vec<String>)> = got
        .iter()
        .map(|c| (c.global.clone(), c.sub_prefix.clone()))
        .collect();
    assert!(
        labels.contains(&("^IadSm173Person".into(), strings(&["idx"]))),
        "{labels:?}"
    );
    assert!(
        labels.contains(&("^IadSm173Person".into(), strings(&["byCity"]))),
        "{labels:?}"
    );
    assert!(got.iter().all(|c| c.found_by == FoundBy::SubNode));
    assert!(got.iter().all(|c| c.status == Status::Examined));
}

#[test]
fn test_candidates_173_sub_nodes_are_distinct() {
    let mut report = parse(WITH_SUB_TREES).unwrap();
    report.global = "^X".into();
    let got = from_sub_nodes(&report);
    let mut prefixes: Vec<Vec<String>> = got.iter().map(|c| c.sub_prefix.clone()).collect();
    let before = prefixes.len();
    prefixes.sort();
    prefixes.dedup();
    assert_eq!(prefixes.len(), before, "each literal once");
}

#[test]
fn test_candidates_173_a_master_with_no_sub_tree_gives_none() {
    // Numeric rows, and a literal first subscript that holds only a value.
    let lines = r#"{"d":1,"s":1,"f":"delimited","dl":94,"p":2}
{"d":1,"s":1,"f":"delimited","dl":94,"p":2}
{"d":1,"s":1,"n":"count","f":"plain","p":1}
{"done":1,"c":3,"capped":0}
"#;
    let mut report = parse(lines).unwrap();
    report.global = "^X".into();
    assert!(from_sub_nodes(&report).is_empty());
}

#[test]
fn test_candidates_173_a_sub_node_label_quotes_the_subscript() {
    let mut c = cand("^X", FoundBy::SubNode);
    c.sub_prefix = strings(&["a\"b"]);
    assert_eq!(c.label(), "^X(\"a\"\"b\")");
    assert_eq!(cand("^X.x", FoundBy::Parameter).label(), "^X.x");
}

// ---------------------------------------------------------------------------
// from_parameter and from_listing
// ---------------------------------------------------------------------------

#[test]
fn test_candidates_173_parameter_names_become_parameter_candidates() {
    let got = from_parameter(&strings(&["^A.x", "B"]));
    let globals: Vec<&str> = got.iter().map(|c| c.global.as_str()).collect();
    assert_eq!(globals, ["^A.x", "^B"]);
    assert!(got.iter().all(|c| c.found_by == FoundBy::Parameter));
}

#[test]
fn test_candidates_173_listing_keeps_names_that_begin_with_the_master_and_drops_the_master() {
    let names = strings(&[
        "IadSm173A",
        "IadSm173A.x",
        "IadSm173AIdx",
        "IadSm173B",
        "Other",
        "IadSm173A.bad name",
    ]);
    let got = from_listing("IadSm173A", &names);
    let globals: Vec<&str> = got.iter().map(|c| c.global.as_str()).collect();
    assert_eq!(globals, ["^IadSm173A.x", "^IadSm173AIdx"]);
    assert!(got.iter().all(|c| c.found_by == FoundBy::NamePrefix));
    // The master may be passed with its caret.
    assert_eq!(from_listing("^IadSm173A", &names).len(), 2);
}

// ---------------------------------------------------------------------------
// dedupe
// ---------------------------------------------------------------------------

#[test]
fn test_candidates_173_dedupe_lets_the_parameter_win_over_the_name_prefix() {
    let list = vec![
        cand("^A.x", FoundBy::NamePrefix),
        cand("^A.y", FoundBy::NamePrefix),
        cand("^A.x", FoundBy::Parameter),
    ];
    let got = dedupe(list);
    assert_eq!(got.len(), 2);
    let x = got.iter().find(|c| c.global == "^A.x").unwrap();
    assert_eq!(x.found_by, FoundBy::Parameter);
    let y = got.iter().find(|c| c.global == "^A.y").unwrap();
    assert_eq!(y.found_by, FoundBy::NamePrefix);
}

#[test]
fn test_candidates_173_dedupe_keeps_the_order_of_first_appearance() {
    let list = vec![
        cand("^B", FoundBy::Parameter),
        cand("^A", FoundBy::NamePrefix),
        cand("^B", FoundBy::NamePrefix),
        cand("^A", FoundBy::Parameter),
    ];
    let got = dedupe(list);
    let globals: Vec<&str> = got.iter().map(|c| c.global.as_str()).collect();
    assert_eq!(globals, ["^B", "^A"]);
    assert!(got.iter().all(|c| c.found_by == FoundBy::Parameter));
}

#[test]
fn test_candidates_173_dedupe_keeps_a_sub_tree_apart_from_the_whole_global() {
    let mut sub = cand("^X", FoundBy::SubNode);
    sub.sub_prefix = strings(&["idx"]);
    let got = dedupe(vec![cand("^X", FoundBy::Parameter), sub.clone(), sub]);
    assert_eq!(got.len(), 2, "{got:?}");
}

// ---------------------------------------------------------------------------
// cap
// ---------------------------------------------------------------------------

#[test]
fn test_candidates_173_cap_keeps_the_first_eight_and_names_the_rest() {
    assert_eq!(MAX_CANDIDATES, 8);
    let list: Vec<Candidate> = (1..=11)
        .map(|i| cand(&format!("^G{i}"), FoundBy::NamePrefix))
        .collect();
    let capped = cap(list, 8);
    assert_eq!(capped.kept.len(), 8);
    assert_eq!(capped.kept[0].global, "^G1");
    assert_eq!(capped.kept[7].global, "^G8");
    assert!(capped.kept.iter().all(|c| c.status == Status::Examined));
    assert_eq!(
        capped.not_examined_names(),
        strings(&["^G9", "^G10", "^G11"])
    );
    for c in &capped.not_examined {
        assert_eq!(c.status, Status::NotExamined);
        let why = c.reason.as_deref().unwrap();
        assert!(why.contains('8'), "the reason names the cap: {why}");
    }
}

#[test]
fn test_candidates_173_cap_leaves_a_short_list_alone() {
    let list = vec![
        cand("^A", FoundBy::Parameter),
        cand("^B", FoundBy::Parameter),
    ];
    let capped = cap(list, 8);
    assert_eq!(capped.kept.len(), 2);
    assert!(capped.not_examined.is_empty());
    assert!(capped.not_examined_names().is_empty());
}

// ---------------------------------------------------------------------------
// validate_index_globals
// ---------------------------------------------------------------------------

#[test]
fn test_candidates_173_valid_index_globals_come_back_with_a_caret() {
    let got = validate_index_globals(&strings(&["IadSm173A.x", "^IadSm173B", "%SYS.y"])).unwrap();
    assert_eq!(got, strings(&["^IadSm173A.x", "^IadSm173B", "^%SYS.y"]));
    assert!(validate_index_globals(&[]).unwrap().is_empty());
}

#[test]
fn test_candidates_173_index_globals_refuse_anything_that_is_not_a_plain_name() {
    for bad in [
        "X) Kill ^Y",
        "^A(\"x\")",
        "A(1)",
        "A B",
        "^|\"NS\"|X",
        "^[\"NS\"]X",
        "",
        "^",
        "A;B",
        "A\nB",
        "X.",
        "A\" Kill ^Y",
    ] {
        let err = validate_index_globals(&strings(&["Good", bad]))
            .expect_err(&format!("{bad:?} must be refused"));
        assert!(err.contains("index_globals"), "{bad:?}: {err}");
        assert!(err.contains("[1]"), "the position is named: {err}");
    }
}

#[test]
fn test_candidates_173_index_globals_name_the_kind_of_refusal() {
    let err = validate_index_globals(&strings(&["^|\"NS\"|X"])).unwrap_err();
    assert!(err.contains("extended"), "{err}");
    let err = validate_index_globals(&strings(&["^A(\"x\")"])).unwrap_err();
    assert!(err.contains("subscript"), "{err}");
}

#[test]
fn test_candidates_173_index_globals_allow_eight_and_refuse_nine() {
    let eight: Vec<String> = (1..=8).map(|i| format!("G{i}")).collect();
    assert_eq!(validate_index_globals(&eight).unwrap().len(), 8);
    let nine: Vec<String> = (1..=9).map(|i| format!("G{i}")).collect();
    let err = validate_index_globals(&nine).unwrap_err();
    assert!(err.contains("at most 8"), "{err}");
    assert!(err.contains('9'), "{err}");
}

// ---------------------------------------------------------------------------
// listing_code and parse_listing
// ---------------------------------------------------------------------------

#[test]
fn test_candidates_173_listing_code_quotes_the_mask_and_carries_no_bare_caller_text() {
    let code = listing_code("IadSm173A").unwrap();
    let mask = os_str_expr("IadSm173A*");
    assert_eq!(mask, "\"IadSm173A*\"");
    assert!(
        code.contains(&format!("Execute($Namespace,{mask},1,,,0)")),
        "{code}"
    );
    assert!(code.contains("%SYS.GlobalQuery:NameSpaceList"), "{code}");
    assert_eq!(
        code.replace(&mask, "").matches("IadSm173A").count(),
        0,
        "the caller's text appears only inside the quoted mask: {code}"
    );
}

#[test]
fn test_candidates_173_listing_code_accepts_a_caret_and_a_percent_name() {
    assert!(listing_code("^IadSm173A")
        .unwrap()
        .contains("\"IadSm173A*\""));
    assert!(listing_code("%SYS").unwrap().contains("\"%SYS*\""));
}

#[test]
fn test_candidates_173_listing_code_refuses_a_name_that_could_carry_code() {
    for bad in [
        "A\") Kill ^Y",
        "A B",
        "",
        "A*",
        "^A(1)",
        "A\nSet x=1",
        "A\"_$c(1)_\"",
    ] {
        assert!(listing_code(bad).is_err(), "{bad:?} must be refused");
    }
}

#[test]
fn test_candidates_173_listing_output_is_parsed_into_names() {
    let out = "{\"g\":\"IadSm173A\"}\n{\"g\":\"IadSm173A.x\"}\n{\"done\":1}\n";
    assert_eq!(
        parse_listing(out).unwrap(),
        strings(&["IadSm173A", "IadSm173A.x"])
    );
    assert!(parse_listing("{\"done\":1}\n").unwrap().is_empty());
}

#[test]
fn test_candidates_173_a_listing_that_errs_or_stops_early_is_an_error() {
    let err = parse_listing("{\"error\":\"<PROTECT> no privilege\"}\n").unwrap_err();
    assert!(err.contains("PROTECT"), "{err}");
    let err = parse_listing("{\"g\":\"A\"}\n").unwrap_err();
    assert!(err.contains("stopped"), "{err}");
    assert!(parse_listing("garbage\n{\"done\":1}\n").is_err());
}

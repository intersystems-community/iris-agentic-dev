//! Spec 173, T039 and T042 (pure halves): how `propose` finds the index globals that live inside
//! the master (the scan code and its output), how a map drafted for such a sub-tree spells the
//! literal levels, and how a class's own index maps mark a candidate `already_mapped`.

use iris_agentic_dev_core::objectscript::os_str_expr;
use iris_agentic_dev_core::tools::storage_map::candidates::{
    from_sub_trees, listing_code, listing_code_for, mark_already_mapped, parse_listing,
    parse_sub_trees, prefix_levels, sub_tree_code, Candidate, FoundBy, Status, LISTING_QUERY,
};
use iris_agentic_dev_core::tools::storage_map::model::{from_tree, StorageModel};
use iris_agentic_dev_core::tools::storage_map::tree::{parse_body, split_class};
use serde_json::json;

fn strings(v: &[&str]) -> Vec<String> {
    v.iter().map(|s| s.to_string()).collect()
}

// ---------------------------------------------------------------------------
// the scan for sub-trees inside the master
// ---------------------------------------------------------------------------

#[test]
fn test_discovery_173_the_scan_code_quotes_the_master_and_carries_no_bare_caller_text() {
    let code = sub_tree_code("^IadSm173A").unwrap();
    let lit = os_str_expr("^IadSm173A");
    assert!(code.contains(&format!("Set mst={lit}")), "{code}");
    assert_eq!(
        code.replace(&lit, "").matches("IadSm173A").count(),
        0,
        "the master's name appears only inside the quoted literal: {code}"
    );
    // It starts at the first string subscript (numbers sort before strings) and reads names
    // and `$Data` only.
    assert!(code.contains("$Char(0)"), "{code}");
    assert!(code.contains("$Data(@mst@(sk))"), "{code}");
}

#[test]
fn test_discovery_173_the_scan_code_refuses_a_name_that_could_carry_code() {
    for bad in ["A\") Kill ^Y", "A B", "", "A*", "^A(1)", "A\nSet x=1"] {
        assert!(sub_tree_code(bad).is_err(), "{bad:?} must be refused");
    }
}

#[test]
fn test_discovery_173_scan_output_is_parsed_into_names_and_the_cap_flag() {
    let out = "{\"g\":\"IadSm173A.x\"}\n{\"done\":1}\n{\"s\":\"idx\"}\n{\"s\":\"by City\"}\n{\"sdone\":1,\"capped\":0}\n";
    let got = parse_sub_trees(out).unwrap();
    assert_eq!(got.names, strings(&["idx", "by City"]));
    assert!(!got.capped);
    // The listing's lines in the same output are still its own.
    assert_eq!(parse_listing(out).unwrap(), strings(&["IadSm173A.x"]));
    let capped = parse_sub_trees("{\"s\":\"a\"}\n{\"sdone\":1,\"capped\":1}\n").unwrap();
    assert!(capped.capped);
}

#[test]
fn test_discovery_173_scan_output_that_failed_or_stopped_is_an_error_not_a_short_list() {
    let err = parse_sub_trees("{\"s\":\"idx\"}\n{\"serror\":\"<PROTECT> ^A\"}\n").unwrap_err();
    assert!(err.contains("PROTECT"), "{err}");
    assert!(
        parse_sub_trees("{\"s\":\"idx\"}\n").is_err(),
        "no done line"
    );
    assert!(parse_sub_trees("not json\n").is_err());
}

#[test]
fn test_discovery_173_a_failed_listing_does_not_hide_the_scan_nor_the_other_way_round() {
    let out =
        "{\"error\":\"ERROR #5002: no such query\"}\n{\"s\":\"idx\"}\n{\"sdone\":1,\"capped\":0}\n";
    assert!(parse_listing(out).unwrap_err().contains("no such query"));
    assert_eq!(parse_sub_trees(out).unwrap().names, strings(&["idx"]));
    let out = "{\"g\":\"A.x\"}\n{\"done\":1}\n{\"serror\":\"boom\"}\n";
    assert_eq!(parse_listing(out).unwrap(), strings(&["A.x"]));
    assert!(parse_sub_trees(out).is_err());
}

#[test]
fn test_discovery_173_sub_trees_become_sub_node_candidates_of_the_master() {
    let (found, skipped) = from_sub_trees("^M", &strings(&["idx", "NameIdx"]));
    assert_eq!(skipped, 0);
    let got: Vec<(String, Vec<String>)> = found
        .iter()
        .map(|c| (c.global.clone(), c.sub_prefix.clone()))
        .collect();
    assert_eq!(
        got,
        vec![
            ("^M".to_string(), strings(&["idx"])),
            ("^M".to_string(), strings(&["NameIdx"]))
        ]
    );
    assert!(found.iter().all(|c| c.found_by == FoundBy::SubNode));
    assert_eq!(found[0].label(), "^M(\"idx\")");
}

#[test]
fn test_discovery_173_a_sub_tree_name_the_draft_cannot_spell_is_counted_not_listed() {
    let long = "x".repeat(65);
    let (found, skipped) = from_sub_trees(
        "^M",
        &strings(&["ok", "has\"quote", "ctl\u{1}char", "", &long]),
    );
    assert_eq!(found.len(), 1, "{found:?}");
    assert_eq!(found[0].sub_prefix, strings(&["ok"]));
    assert_eq!(skipped, 4);
}

// ---------------------------------------------------------------------------
// the listing's query name
// ---------------------------------------------------------------------------

#[test]
fn test_discovery_173_listing_code_for_names_the_query_it_is_given() {
    assert_eq!(
        listing_code("IadSm173A").unwrap(),
        listing_code_for("IadSm173A", LISTING_QUERY).unwrap()
    );
    let other = listing_code_for("IadSm173A", "Other.Class:Query").unwrap();
    assert!(other.contains("%New(\"Other.Class:Query\")"), "{other}");
    assert!(!other.contains("NameSpaceList"), "{other}");
    // A query IRIS cannot open leaves no object; its reason is in %objlasterror.
    assert!(other.contains("%objlasterror"), "{other}");
}

#[test]
fn test_discovery_173_listing_code_for_refuses_a_query_name_that_could_carry_code() {
    for bad in ["A\")Kill ^Y", "A:B C", "", "NoColon", "A:B\nSet x=1"] {
        assert!(
            listing_code_for("IadSm173A", bad).is_err(),
            "{bad:?} must be refused"
        );
    }
}

// ---------------------------------------------------------------------------
// the literal levels of a drafted map
// ---------------------------------------------------------------------------

#[test]
fn test_discovery_173_prefix_levels_puts_the_literal_first_and_moves_every_level_down() {
    let mut map = json!({
        "name": "Idx",
        "global": "^M",
        "subscripts": [
            {"level": 1, "expression": "$$SQLUPPER({Name})", "null_marker": "$c(0)"},
            {"level": 2, "expression": "{ID}"}
        ]
    });
    let mut guesses = strings(&[
        "/maps/Idx/subscripts/1",
        "/maps/Idx/subscripts/2/null_marker",
        "/maps/Other/subscripts/1",
    ]);
    prefix_levels(&mut map, &mut guesses, &strings(&["idx"]));
    let levels: Vec<(u64, String)> = map["subscripts"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| {
            (
                s["level"].as_u64().unwrap(),
                s["expression"].as_str().unwrap().to_string(),
            )
        })
        .collect();
    assert_eq!(
        levels,
        vec![
            (1, "\"idx\"".to_string()),
            (2, "$$SQLUPPER({Name})".to_string()),
            (3, "{ID}".to_string())
        ]
    );
    assert_eq!(map["subscripts"][1]["null_marker"], "$c(0)");
    // A guess about this map's level follows the level; another map's does not move.
    assert_eq!(
        guesses,
        strings(&[
            "/maps/Idx/subscripts/2",
            "/maps/Idx/subscripts/3/null_marker",
            "/maps/Other/subscripts/1",
        ])
    );
}

#[test]
fn test_discovery_173_prefix_levels_doubles_a_quote_and_takes_several_literals() {
    let mut map = json!({"name": "I", "subscripts": [{"level": 1, "expression": "{ID}"}]});
    let mut guesses = Vec::new();
    prefix_levels(&mut map, &mut guesses, &strings(&["a", "b c"]));
    let exprs: Vec<&str> = map["subscripts"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s["expression"].as_str().unwrap())
        .collect();
    assert_eq!(exprs, vec!["\"a\"", "\"b c\"", "{ID}"]);
}

// ---------------------------------------------------------------------------
// already_mapped
// ---------------------------------------------------------------------------

fn model_of(class: &str, text: &str) -> StorageModel {
    let spans = split_class(text).expect("split failed");
    let span = &spans[0];
    let nodes = parse_body(&text[span.body_start..span.body_end]).expect("parse failed");
    from_tree(class, &span.name, &nodes)
}

fn class_with_maps(maps: &str) -> String {
    format!(
        "Class X.Y Extends %Persistent [ StorageStrategy = SQLStorage ]\n{{\n\nStorage SQLStorage\n{{\n{maps}<Type>%Storage.SQL</Type>\n}}\n\n}}\n"
    )
}

const MASTER: &str = "<SQLMap name=\"Master\">\n<Global>^M</Global>\n<Subscript name=\"1\">\n<Expression>{ID}</Expression>\n</Subscript>\n<Type>data</Type>\n</SQLMap>\n";

fn index_map(name: &str, global: &str, first: &str) -> String {
    format!(
        "<SQLMap name=\"{name}\">\n<Global>{global}</Global>\n<Subscript name=\"1\">\n<Expression>{first}</Expression>\n</Subscript>\n<Subscript name=\"2\">\n<Expression>{{ID}}</Expression>\n</Subscript>\n<Type>index</Type>\n</SQLMap>\n"
    )
}

#[test]
fn test_discovery_173_a_global_an_index_map_uses_is_already_mapped_with_the_maps_name() {
    let text = class_with_maps(&format!(
        "{MASTER}{}{}",
        index_map("NameIdx", "^MX", "{Name}"),
        index_map("Inside", "^M", "\"idx\"")
    ));
    let model = model_of("X.Y", &text);
    let mut cands = vec![
        Candidate::new("^MX", FoundBy::NamePrefix),
        Candidate::new("^MOther", FoundBy::NamePrefix),
        {
            let mut c = Candidate::new("^M", FoundBy::SubNode);
            c.sub_prefix = strings(&["idx"]);
            c
        },
        {
            let mut c = Candidate::new("^M", FoundBy::SubNode);
            c.sub_prefix = strings(&["other"]);
            c
        },
    ];
    mark_already_mapped(&mut cands, &model.storage.maps);
    let got: Vec<(Status, Option<&str>)> = cands
        .iter()
        .map(|c| (c.status, c.reason.as_deref()))
        .collect();
    assert_eq!(
        got,
        vec![
            (Status::AlreadyMapped, Some("NameIdx")),
            (Status::Examined, None),
            (Status::AlreadyMapped, Some("Inside")),
            (Status::Examined, None),
        ]
    );
}

#[test]
fn test_discovery_173_the_data_map_is_not_an_index_map_and_a_caret_does_not_matter() {
    let text = class_with_maps(&format!("{MASTER}{}", index_map("Ix", "^MX", "{Name}")));
    let model = model_of("X.Y", &text);
    // The data map's global is the master's own: a candidate named like it is not mapped as an
    // index. A name without its caret still matches the index map.
    let mut cands = vec![
        Candidate::new("^M", FoundBy::Parameter),
        Candidate::new("MX", FoundBy::Parameter),
    ];
    mark_already_mapped(&mut cands, &model.storage.maps);
    assert_eq!(cands[0].status, Status::Examined);
    assert_eq!(cands[1].status, Status::AlreadyMapped);
    assert_eq!(cands[1].reason.as_deref(), Some("Ix"));
}

//! Spec 173 T024: the usability line of `check`. The planner's own words decide: a warning that an
//! index map "cannot be used" is `unused` and carries that line; "Read index map X, using the given"
//! is `used`; anything else is `unknown`. The plan texts are captured from IRIS 2026.2
//! (`fact_6_planner_words`), not written by hand.

use iris_agentic_dev_core::tools::storage_map::usability::{
    explain_query, parse_plan, sql_table, Usability,
};

fn captured(name: &str) -> String {
    let p = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/storage173/captured")
        .join(name);
    std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("{}: {e}", p.display()))
}

#[test]
fn a_map_the_planner_reads_with_the_given_value_is_used() {
    let plan = captured("plan_used.txt");
    assert_eq!(
        parse_plan(&plan, "M"),
        Usability::Used {
            map: "IadSm173.Q2.M".into()
        }
    );
}

#[test]
fn the_no_collation_warning_is_unused_and_carries_the_planners_words() {
    let plan = captured("plan_unused_no_collation.txt");
    match parse_plan(&plan, "M") {
        Usability::Unused { map, text } => {
            assert_eq!(map, "IadSm173.Q1.M");
            assert!(
                text.contains("there is no index subscript collation"),
                "{text}"
            );
            assert!(
                text.starts_with("Index map IadSm173.Q1.M cannot be used"),
                "{text}"
            );
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn the_length_warning_names_the_stored_collation() {
    let plan = captured("plan_unused_length.txt");
    match parse_plan(&plan, "M") {
        Usability::Unused { text, .. } => {
            assert!(
                text.contains("the index subscript collation is %SQLUPPER(4)"),
                "{text}"
            );
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn the_planner_reading_the_index_without_the_given_value_is_not_used() {
    // The raw plan still says "Read index map ..., looping on Name and ID": a full scan of the
    // index. The warning decides, so a plain "Read index map" is never taken for use.
    let plan = captured("plan_unused_no_collation.txt");
    assert!(plan.contains("Read index map IadSm173.Q1.M, looping on"));
    assert!(!matches!(parse_plan(&plan, "M"), Usability::Used { .. }));
}

#[test]
fn another_map_or_no_plan_is_unknown() {
    let plan = captured("plan_used.txt");
    assert_eq!(parse_plan(&plan, "Other"), Usability::Unknown);
    assert_eq!(parse_plan("", "M"), Usability::Unknown);
    assert_eq!(
        parse_plan("<plans><plan>Read master map X.Master</plan></plans>", "M"),
        Usability::Unknown
    );
}

#[test]
fn a_map_name_that_only_ends_the_same_is_not_the_map() {
    // `...Q2.XM` must not answer for map `M`.
    let plan = "Read index map IadSm173.Q2.XM, using the given %SQLUPPER(Name), and looping on ID.";
    assert_eq!(parse_plan(plan, "M"), Usability::Unknown);
}

#[test]
fn the_sql_table_of_a_class_is_its_schema_and_name() {
    assert_eq!(sql_table("IadSm173.Q1").unwrap(), "IadSm173.Q1");
    assert_eq!(sql_table("A.B.C").unwrap(), "A_B.C");
    assert!(sql_table("Nodot").is_err());
    assert!(sql_table("A.B; DROP").is_err());
}

#[test]
fn the_explain_query_names_the_table_and_the_field() {
    let q = explain_query("IadSm173.Q1", "Name").unwrap();
    assert_eq!(q, "EXPLAIN SELECT %ID FROM IadSm173.Q1 WHERE Name = 'x'");
    assert_eq!(
        explain_query("A.B.C", "Born").unwrap(),
        "EXPLAIN SELECT %ID FROM A_B.C WHERE Born = 'x'"
    );
    for bad in [
        "",
        "Na me",
        "Name; DROP TABLE x",
        "1Name",
        "Name' OR '1'='1",
    ] {
        assert!(explain_query("IadSm173.Q1", bad).is_err(), "{bad:?}");
    }
    assert!(explain_query("bad class", "Name").is_err());
}

// ---- T055/T056: the questions `check` puts to the planner, and the lines it makes of the answers ----

use iris_agentic_dev_core::tools::storage_map::usability::{
    explain_prefix_query, line_for, questions, subscript_kind, MapSubs, SubKind, MAX_QUESTIONS,
};

fn map(name: &str, kind: &str, subs: &[&str]) -> MapSubs {
    MapSubs {
        name: name.into(),
        kind: kind.into(),
        subscripts: subs
            .iter()
            .enumerate()
            .map(|(i, e)| (i as u32 + 1, e.to_string()))
            .collect(),
    }
}

#[test]
fn a_subscript_is_a_field_a_skipped_part_or_something_else() {
    let field = |s: &str| SubKind::Field(s.to_string());
    assert_eq!(subscript_kind("{City}"), field("City"));
    assert_eq!(subscript_kind("$$SQLUPPER({Name})"), field("Name"));
    assert_eq!(subscript_kind("$$SQLUPPER({Name},4)"), field("Name"));
    assert_eq!(
        subscript_kind("$$TRUNCATE({first_name},3)"),
        field("first_name")
    );
    for skip in ["{ID}", "{%ID}", "{L1}", "\"idx\""] {
        assert_eq!(subscript_kind(skip), SubKind::Skip, "{skip}");
    }
    for other in [
        "{Age}+1",
        "{A}_{B}",
        "$$F({A},{B})",
        "$Piece({A},\",\")",
        "",
        "{A",
    ] {
        assert!(
            matches!(subscript_kind(other), SubKind::Other(_)),
            "{other:?}"
        );
    }
}

#[test]
fn one_question_per_field_subscript_with_the_fields_before_it() {
    let maps = [map("M", "index", &["$$SQLUPPER({City})", "{Age}", "{ID}"])];
    let q = questions(&maps);
    assert_eq!(q.len(), 2);
    assert_eq!((q[0].level, q[0].field.as_str()), (1, "City"));
    assert_eq!(q[0].fields, vec!["City"]);
    assert_eq!((q[1].level, q[1].field.as_str()), (2, "Age"));
    assert_eq!(q[1].fields, vec!["City", "Age"]);
    assert!(q.iter().all(|x| x.unanswerable.is_none() && x.map == "M"));
}

#[test]
fn a_field_named_twice_is_asked_once_and_the_data_map_is_not_asked() {
    let maps = [
        map("Master", "data", &["{ID}"]),
        map("M", "index", &["{City}", "{City}", "{ID}"]),
    ];
    let q = questions(&maps);
    assert_eq!(q.len(), 2, "{q:?}");
    assert_eq!(q[1].fields, vec!["City"], "no repeated field in the query");
}

#[test]
fn a_computed_subscript_and_what_follows_it_are_unanswerable_with_a_reason() {
    let maps = [map("M", "index", &["{Age}+1", "{City}", "{ID}"])];
    let q = questions(&maps);
    assert_eq!(q.len(), 2, "{q:?}");
    assert!(
        q[0].unanswerable
            .as_deref()
            .unwrap()
            .contains("not a field"),
        "{q:?}"
    );
    let after = q[1].unanswerable.as_deref().unwrap();
    assert!(
        after.contains("subscript 1") && after.contains("not a field"),
        "{after}"
    );
}

#[test]
fn a_bitmap_map_is_one_unknown_line_and_an_extent_map_none() {
    let maps = [
        map("Bits", "bitmap", &["{Status}", "{ID}"]),
        map("Ext", "bitmapextent", &["{ID}"]),
    ];
    let q = questions(&maps);
    assert_eq!(q.len(), 1, "{q:?}");
    assert_eq!(q[0].map, "Bits");
    assert!(
        q[0].unanswerable.as_deref().unwrap().contains("bitmap"),
        "{q:?}"
    );
}

#[test]
fn the_prefix_query_is_an_equality_on_each_field() {
    let q = explain_prefix_query("IadSm173.Q1", &["City".into(), "Age".into()]).unwrap();
    assert_eq!(
        q,
        "EXPLAIN SELECT %ID FROM IadSm173.Q1 WHERE City = 'x' AND Age = 'x'"
    );
    assert!(explain_prefix_query("IadSm173.Q1", &[]).is_err());
    assert!(explain_prefix_query("IadSm173.Q1", &["a b".into()]).is_err());
    assert_eq!(
        explain_query("IadSm173.Q1", "first_name").unwrap(),
        "EXPLAIN SELECT %ID FROM IadSm173.Q1 WHERE first_name = 'x'"
    );
}

#[test]
fn a_line_is_the_planners_words_with_the_real_class_in_place_of_the_scratch_copy() {
    let maps = [map("M", "index", &["{Name}"])];
    let q = &questions(&maps)[0];
    // The captured plan names IadSm173.Q1; pretend that class is the scratch copy.
    let plan = captured("plan_unused_no_collation.txt");
    let line = line_for(q, Ok(&plan), "IadSm173.Q1", "Real.Class");
    assert_eq!(line.status, "unused");
    assert_eq!(line.map, "M");
    assert_eq!(line.field, "Name");
    assert_eq!(line.subscript_level, 1);
    assert!(
        line.text
            .starts_with("Index map Real.Class.M cannot be used"),
        "{}",
        line.text
    );
    assert!(!line.text.contains("IadSm173"), "{}", line.text);

    let plan = captured("plan_used.txt");
    let line = line_for(q, Ok(&plan), "IadSm173.Q2", "Real.Class");
    assert_eq!(line.status, "used");
    assert_eq!(
        line.text,
        "Read index map Real.Class.M, using the given %SQLUPPER(Name), and looping on ID."
    );
}

#[test]
fn a_plan_that_does_not_name_the_map_or_a_failed_question_is_unknown_with_the_reason() {
    let maps = [map("Other", "index", &["{Name}"])];
    let q = &questions(&maps)[0];
    let plan = captured("plan_used.txt");
    let line = line_for(q, Ok(&plan), "IadSm173.Q2", "C.D");
    assert_eq!(line.status, "unknown");
    assert!(
        line.text.contains("does not name index map Other"),
        "{}",
        line.text
    );

    let line = line_for(q, Err("SQLCODE -400"), "IadSm173.Q2", "C.D");
    assert_eq!(line.status, "unknown");
    assert!(
        line.text.contains("could not be asked") && line.text.contains("SQLCODE -400"),
        "{}",
        line.text
    );

    let maps = [map("M", "index", &["{Age}+1"])];
    let q = &questions(&maps)[0];
    let line = line_for(q, Err("never asked"), "X.Y", "C.D");
    assert!(
        line.text.contains("not a field"),
        "the unanswerable reason wins: {}",
        line.text
    );
}

#[test]
fn a_warning_names_the_question_and_carries_the_planners_words() {
    let maps = [map("M", "index", &["{Name}"])];
    let q = &questions(&maps)[0];
    let plan = captured("plan_unused_no_collation.txt");
    let line = line_for(q, Ok(&plan), "IadSm173.Q1", "Real.Class");
    let w = line.warning();
    assert!(
        w.starts_with(
            "index map M, subscript 1 (Name), unused: Index map Real.Class.M cannot be used"
        ),
        "{w}"
    );
    assert_eq!(line.to_json()["status"], "unused");
    assert_eq!(line.to_json()["subscript_level"], 1);
}

#[test]
fn the_questions_are_listed_in_full_and_the_runner_caps_them() {
    let subs: Vec<String> = (0..40).map(|i| format!("{{F{i}}}")).collect();
    let refs: Vec<&str> = subs.iter().map(String::as_str).collect();
    let q = questions(&[map("M", "index", &refs)]);
    assert_eq!(q.len(), 40);
    let capped = iris_agentic_dev_core::tools::storage_map::usability_run::askable(q);
    let asked = capped.iter().filter(|q| q.unanswerable.is_none()).count();
    assert_eq!(asked, MAX_QUESTIONS);
    let last = capped.last().unwrap().unanswerable.as_deref().unwrap();
    assert!(last.contains("not asked") && last.contains("20"), "{last}");
}

#[test]
fn a_field_that_is_no_sql_name_is_not_asked() {
    let maps = [map("M", "index", &["{a-b}"])];
    let q = iris_agentic_dev_core::tools::storage_map::usability_run::askable(questions(&maps));
    assert_eq!(q.len(), 1);
    assert!(q[0].unanswerable.is_some(), "{q:?}");
}

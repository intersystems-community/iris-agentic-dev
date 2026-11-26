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

//! Spec 141 T200: what `iris_coverage` reports and what it refuses, without a monitor.
//!
//! The live half is `tests/integration/test_coverage_diag_141_live.rs`, which runs the monitor.

use iris_agentic_dev_core::tools::coverage::{
    parse_check_output, parse_coverage_output, restrict_to_classes,
};
use serde_json::json;

/// One class that compiled to two routines: the entry is their sum and lists both.
#[test]
fn a_class_with_several_routines_is_one_entry_with_their_sum() {
    let out = "COVERAGE_DATA_START\nDemo.A|Demo.A.0|0|0\nDemo.A|Demo.A.1|3|11\nDemo.B|Demo.B.1|1|4\nTOTAL|4|15\n";
    let v = parse_coverage_output(out);
    assert_eq!(v["success"], true, "{v}");
    let classes = v["classes"].as_array().unwrap();
    assert_eq!(classes.len(), 2, "{v}");
    assert_eq!(classes[0]["class"], "Demo.A");
    assert_eq!(classes[0]["routines"], json!(["Demo.A.0", "Demo.A.1"]));
    assert_eq!(classes[0]["hit"], 3);
    assert_eq!(classes[0]["total"], 11);
    assert_eq!(classes[0]["pct"], 27.3);
    assert_eq!(v["hits"], 4);
    assert_eq!(v["total"], 15);
    assert_eq!(v["total_pct"], 26.7);
}

#[test]
fn zero_of_zero_is_null_not_zero_percent() {
    let v = parse_coverage_output("COVERAGE_DATA_START\nDemo.E|Demo.E.0|0|0\nTOTAL|0|0\n");
    assert_eq!(v["success"], true, "{v}");
    assert!(v["classes"][0]["pct"].is_null(), "{v}");
    assert!(v["total_pct"].is_null(), "{v}");
    assert!(
        v["classes"][0]["note"]
            .as_str()
            .unwrap()
            .contains("not 0% coverage"),
        "the entry says what the null means: {v}"
    );
    // The other way: a class with lines and none run is a real 0%.
    let none = parse_coverage_output("COVERAGE_DATA_START\nDemo.N|Demo.N.1|0|9\nTOTAL|0|9\n");
    assert_eq!(none["classes"][0]["pct"], 0.0, "{none}");
    assert_eq!(none["total_pct"], 0.0, "{none}");
}

/// A line that threw is kept with what was collected, and says so.
#[test]
fn a_run_that_threw_is_partial_and_names_the_error() {
    let out = "COVERAGE_DATA_START\nDemo.A|Demo.A.1|3|11\nTOTAL|3|11\nRUN_ERROR|<UNDEFINED>x+1^Demo.1 | with a bar\n";
    let v = parse_coverage_output(out);
    assert_eq!(v["success"], true, "{v}");
    assert_eq!(v["partial"], true, "{v}");
    assert_eq!(v["run_error"], "<UNDEFINED>x+1^Demo.1 | with a bar");
    assert_eq!(v["hits"], 3);
}

#[test]
fn a_run_that_did_not_throw_is_not_partial() {
    let v = parse_coverage_output("COVERAGE_DATA_START\nDemo.A|Demo.A.1|3|11\nTOTAL|3|11\n");
    assert!(
        v.get("partial").is_none() && v.get("run_error").is_none(),
        "{v}"
    );
}

#[test]
fn refusals_carry_their_code_and_message() {
    let check = parse_check_output("ERROR|MONITOR_IN_USE|a line monitor is already running");
    assert_eq!(check["success"], false);
    assert_eq!(check["error_code"], "MONITOR_IN_USE");
    assert_eq!(check["message"], "a line monitor is already running");
    for code in [
        "MONITOR_IN_USE",
        "COVERAGE_ROUTINE_UNKNOWN",
        "MONITOR_NOT_RUNNING",
    ] {
        let v = parse_coverage_output(&format!("ERROR|{code}|why it is so"));
        assert_eq!(v["error_code"], code);
        assert_eq!(v["message"], "why it is so");
    }
}

/// `mode=report` is asked about some classes of a monitor someone started on others too.
#[test]
fn a_report_covers_the_classes_asked_for_and_recomputes_the_totals() {
    let all = parse_coverage_output(
        "Demo.A|Demo.A.1|3|10\nDemo.B|Demo.B.1|5|10\nDemo.C|Demo.C.1|0|10\nTOTAL|8|30\n",
    );
    let ab = restrict_to_classes(all.clone(), &["Demo.A".to_string(), "Demo.B".to_string()]);
    assert_eq!(ab["classes"].as_array().unwrap().len(), 2, "{ab}");
    assert_eq!(ab["hits"], 8);
    assert_eq!(ab["total"], 20);
    assert_eq!(ab["total_pct"], 40.0);

    let typo = restrict_to_classes(all, &["Demo.A".to_string(), "Demo.Z".to_string()]);
    assert_eq!(typo["success"], false, "{typo}");
    assert_eq!(typo["error_code"], "COVERAGE_ROUTINE_UNKNOWN");
    assert!(
        typo["message"].as_str().unwrap().contains("Demo.Z"),
        "{typo}"
    );
    assert!(typo.get("classes").is_none());
}

#[test]
fn restricting_an_error_leaves_it_alone() {
    let err = parse_coverage_output("ERROR|MONITOR_NOT_RUNNING|none");
    assert_eq!(
        restrict_to_classes(err.clone(), &["Demo.A".to_string()]),
        err
    );
}

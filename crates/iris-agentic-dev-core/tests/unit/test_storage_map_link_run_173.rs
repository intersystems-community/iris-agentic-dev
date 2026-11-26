// Spec 173, T027: what stands between the linker's lines and the answer, with no IRIS. A master
// field becomes the linker's field table entry, a report becomes a link with a verdict, and an
// index global the caller names meets the same global gates the master does.

use iris_agentic_dev_core::policy::effective::EffectivePolicy;
use iris_agentic_dev_core::tools::storage_map::globals::check_index_global;
use iris_agentic_dev_core::tools::storage_map::link::{
    assemble, link_json, overall_verdict, parse_link_output, IdForm, MasterView, Verdict,
};
use iris_agentic_dev_core::tools::storage_map::link_code::{field_access, FieldAccess};
use iris_agentic_dev_core::tools::storage_map::master::MasterField;
use serde_json::json;

fn field(delimiter: Option<&str>, piece: Option<&str>, node: Option<&str>) -> MasterField {
    MasterField {
        name: "Field1".into(),
        placeholder: "Field1".into(),
        node: node.map(String::from),
        delimiter: delimiter.map(String::from),
        piece: piece.map(String::from),
        format: "delimited".into(),
        guessed: true,
    }
}

// ── field_access ───────────────────────────────────────────────────────────

#[test]
fn a_quoted_one_character_delimiter_is_its_character_code() {
    let got = field_access(&field(Some("\"^\""), Some("3"), None)).unwrap();
    assert_eq!(
        got,
        FieldAccess {
            node: None,
            delimiter: Some(94),
            piece: 3
        }
    );
    // A quote inside the quotes is doubled.
    let q = field_access(&field(Some("\"\"\"\""), Some("1"), None)).unwrap();
    assert_eq!(q.delimiter, Some(34));
}

#[test]
fn a_char_call_delimiter_is_its_code() {
    for (text, code) in [("$c(1)", 1), ("$CHAR(124)", 124), ("$C( 9 )", 9)] {
        let got = field_access(&field(Some(text), Some("2"), None)).unwrap();
        assert_eq!(got.delimiter, Some(code), "{text}");
    }
}

#[test]
fn no_delimiter_is_a_list_and_the_node_is_carried() {
    let got = field_access(&field(None, Some("2"), Some("contact"))).unwrap();
    assert_eq!(got.delimiter, None);
    assert_eq!(got.piece, 2);
    assert_eq!(got.node.as_deref(), Some("contact"));
}

#[test]
fn a_field_the_linker_cannot_read_is_refused_with_the_reason() {
    for (d, p) in [
        (Some("\"ab\""), Some("1")),   // two characters
        (Some("\"^\""), Some("0")),    // piece 0
        (Some("\"^\""), Some("x")),    // not a number
        (Some("\"^\""), None),         // no piece
        (Some("$c(0)"), Some("1")),    // code 0 would read as a $LIST
        (Some("$c(1)+1"), Some("1")),  // an expression
        (Some("\"^\" _x"), Some("1")), // text after the quote
        (None, None),                  // a list with no element
    ] {
        let err = field_access(&field(d, p, None)).unwrap_err();
        assert!(!err.is_empty(), "{d:?} {p:?}");
    }
}

// ── overall_verdict ────────────────────────────────────────────────────────

#[test]
fn the_overall_verdict_is_the_weakest_reason_in_priority_order() {
    use Verdict::*;
    assert_eq!(overall_verdict(1.0, 40, &[Inferred, Inferred]), Inferred);
    assert_eq!(overall_verdict(1.0, 40, &[Inferred, Ambiguous]), Ambiguous);
    assert_eq!(overall_verdict(1.0, 40, &[Ambiguous, Constant]), Constant);
    assert_eq!(
        overall_verdict(1.0, 40, &[Constant, NotExplained, Ambiguous]),
        NotExplained
    );
    // Too few nodes and no ID link outrank what the positions say.
    assert_eq!(overall_verdict(1.0, 19, &[Inferred]), TooFewNodes);
    assert_eq!(overall_verdict(0.89, 40, &[Inferred]), NotLinked);
    assert_eq!(overall_verdict(0.89, 5, &[Inferred]), NotLinked);
    // Nothing to explain is not an inferred index.
    assert_eq!(overall_verdict(1.0, 40, &[]), NotExplained);
}

// ── assemble and link_json ─────────────────────────────────────────────────

fn f02_report() -> iris_agentic_dev_core::tools::storage_map::link::LinkReport {
    let text = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/storage173/captured/link_F02.jsonl"
    ))
    .unwrap();
    parse_link_output(&text).unwrap()
}

fn master() -> MasterView {
    MasterView {
        global: "^IadSm173F02".into(),
        field_names: ["Field1", "Field2", "Field3", "Field4", "Field5"]
            .map(String::from)
            .to_vec(),
        key_names: vec!["ID".into()],
    }
}

#[test]
fn the_first_id_line_is_the_best_and_its_run_is_the_id() {
    let a = assemble("^IadSm173F02X", "X", &f02_report());
    assert_eq!(a.link.id.form, IdForm::Subscript);
    assert_eq!(a.link.id.positions, vec![2]);
    assert!((a.id_agreement - 1.0).abs() < 1e-9);
    assert_eq!(a.link.verdict, Verdict::Inferred);
    assert_eq!(a.link.subscripts.len(), 1);
    let sub = &a.link.subscripts[0];
    assert_eq!(sub.position, 1);
    assert_eq!(sub.verdict, Verdict::Inferred);
    let pick = sub.pick.as_ref().unwrap();
    assert_eq!(pick.field, 1);
    assert_eq!(pick.matches, 40);
}

#[test]
fn the_link_json_carries_the_data_model_keys() {
    let report = f02_report();
    let a = assemble("^IadSm173F02X", "X", &report);
    let j = link_json(&a, &master(), &report);
    assert_eq!(j["global"], "^IadSm173F02X");
    assert_eq!(j["nodes_sampled"], 40);
    assert_eq!(j["distinct_first_values"], 38);
    assert_eq!(j["usable_nodes"], 40);
    assert_eq!(j["verdict"], "inferred");
    assert_eq!(j["id"]["form"], "subscript");
    assert_eq!(j["id"]["positions"], json!([2]));
    assert_eq!(j["id"]["agreement"], json!(1.0));
    let s = &j["subscripts"][0];
    assert_eq!(s["position"], 1);
    assert_eq!(s["verdict"], "inferred");
    assert_eq!(s["field"], "Field1");
    assert_eq!(s["transform"], "sqlupper");
    assert_eq!(s["agreement"], json!(1.0));
    // No length or piece key when there is none.
    assert!(s.get("length").is_none() && s.get("piece").is_none(), "{s}");
    assert_eq!(s["alternatives"], json!([]));
    assert_eq!(j["not_explained"], json!([]));
    assert_eq!(j["data_in_value"], json!([]));
}

#[test]
fn a_position_nothing_explains_is_listed_with_its_best_agreement() {
    let text = r#"{"t":"sample","nodes":40,"distinct_first":38,"usable":40,"stopped":"none"}
{"t":"id","form":"subscript","positions":[2],"resolved":40,"total":40}
{"t":"pos","position":1,"total":40,"distinct":38,"scores":[{"field":1,"transform":"exact","length":null,"piece":null,"matches":30}]}
{"t":"done"}
"#;
    let report = parse_link_output(text).unwrap();
    let a = assemble("^G", "X", &report);
    assert_eq!(a.link.verdict, Verdict::NotExplained);
    let j = link_json(&a, &master(), &report);
    assert_eq!(
        j["not_explained"],
        json!([{"position": 1, "best_agreement": 0.75}])
    );
    let s = &j["subscripts"][0];
    assert_eq!(s["verdict"], "not_explained");
    assert!(
        s.get("field").is_none(),
        "an unexplained position names no field: {s}"
    );
}

#[test]
fn an_index_that_does_not_find_master_rows_is_not_linked() {
    let text = r#"{"t":"sample","nodes":40,"distinct_first":40,"usable":3,"stopped":"none"}
{"t":"id","form":"subscript","positions":[2],"resolved":3,"total":40}
{"t":"done"}
"#;
    let report = parse_link_output(text).unwrap();
    let a = assemble("^G", "X", &report);
    assert_eq!(a.link.verdict, Verdict::NotLinked);
    let j = link_json(&a, &master(), &report);
    assert_eq!(j["verdict"], "not_linked");
    assert_eq!(j["id"]["agreement"], json!(0.075));
}

#[test]
fn a_run_with_no_id_line_is_not_linked_not_a_panic() {
    let text = r#"{"t":"sample","nodes":0,"distinct_first":0,"usable":0,"stopped":"none"}
{"t":"done"}
"#;
    let report = parse_link_output(text).unwrap();
    let a = assemble("^G", "X", &report);
    assert_eq!(a.link.verdict, Verdict::NotLinked);
    assert!(a.link.id.positions.is_empty());
}

// ── check_index_global ─────────────────────────────────────────────────────

#[test]
fn an_index_global_meets_the_gates_the_master_does() {
    let policy = EffectivePolicy::default();
    assert!(check_index_global("^IadSm173F02X", &policy, "default", false).is_none());
    for refused in ["^oddDEF", "^ROUTINE", "^%SYS"] {
        let body = check_index_global(refused, &policy, "default", false)
            .unwrap_or_else(|| panic!("{refused} was let through"));
        assert_eq!(body["success"], false, "{refused}");
        assert!(body["error_code"].is_string(), "{refused}: {body}");
    }
    // A PHI-named global needs the acknowledgement, as it does for the master.
    let phi = check_index_global("^PAPMIIdx", &policy, "default", false)
        .expect("a PHI-named global is gated");
    assert_eq!(phi["error_code"], "PHI_GATE_BLOCKED");
    assert!(check_index_global("^PAPMIIdx", &policy, "default", true).is_none());
}

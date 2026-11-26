// Spec 173, T051 and T052: the pure half of sparse and conditional indexes. The linker's `missing`
// line is read by `parse_link_output`, reported by `missing_rows_json`, and turned into a
// `Condition` with `PopulationType conditional` by `condition_for` and `apply_condition`; the scratch
// verification asks the master the same question through `present_count_query`. Nothing here
// touches IRIS.

use iris_agentic_dev_core::tools::storage_map::link::{
    apply_condition, condition_for, draft_index_map, missing_rows_json, parse_link_output, IdForm,
    IdLink, Link, MasterView, MissingRule, PositionScore, SubscriptLink, Transform, Verdict,
};
use iris_agentic_dev_core::tools::storage_map::link_code::{
    build_code, Caps, FieldAccess, LinkRequest,
};
use iris_agentic_dev_core::tools::storage_map::link_verify::present_count_query;
use iris_agentic_dev_core::tools::storage_map::model::map_node;
use iris_agentic_dev_core::tools::storage_map::tree::Node;
use iris_agentic_dev_core::tools::storage_map::validate::validate;
use serde_json::json;

fn view() -> MasterView {
    MasterView {
        global: "^IadSm173F22".into(),
        field_names: ["Field1", "Field2", "Field3", "Field4", "Field5"]
            .map(String::from)
            .to_vec(),
        key_names: vec!["ID".into()],
    }
}

fn missing(rule: &str, field: Option<u32>, literal: Option<&str>) -> String {
    let field = field.map_or("null".to_string(), |f| f.to_string());
    // JSON, not Rust's Debug spelling: `\u{1}` is not a JSON escape.
    let literal = literal.map_or("null".to_string(), |l| serde_json::to_string(l).unwrap());
    format!(
        "{{\"t\":\"sample\",\"nodes\":48,\"distinct_first\":9,\"usable\":48,\"stopped\":\"none\"}}\n\
         {{\"t\":\"missing\",\"share\":0.2,\"rule\":\"{rule}\",\"field\":{field},\"literal\":{literal},\"rows\":60,\"count\":12}}\n\
         {{\"t\":\"done\"}}\n"
    )
}

fn passes_the_validator(map: &serde_json::Value) {
    let nodes = vec![
        Node {
            tag: "Type".into(),
            name: None,
            value: Some("%Storage.SQL".into()),
            children: None,
        },
        map_node(map).unwrap_or_else(|e| panic!("model JSON: {e}\n{map}")),
    ];
    if let Err(found) = validate(&nodes) {
        panic!("the validator refuses the draft: {found:?}\n{map}");
    }
}

fn city_link() -> Link {
    Link {
        global: "^IadSm173F22X".into(),
        map_name: "X".into(),
        verdict: Verdict::Inferred,
        id: IdLink {
            form: IdForm::Subscript,
            positions: vec![2],
        },
        subscripts: vec![SubscriptLink {
            position: 1,
            verdict: Verdict::Inferred,
            pick: Some(PositionScore {
                field: 2,
                transform: Transform::Exact,
                length: None,
                piece: None,
                matches: 48,
                total: 48,
            }),
            alternatives: vec![],
            null_marker: None,
        }],
        data_in_value: vec![],
    }
}

// ── the missing line ───────────────────────────────────────────────────────

#[test]
fn the_missing_line_names_the_share_the_rule_and_the_field() {
    let r = parse_link_output(&missing("null", Some(2), None)).unwrap();
    let m = r.missing.expect("a missing line");
    assert_eq!(m.rule, MissingRule::Null);
    assert_eq!(m.field, Some(2));
    assert!((m.share - 0.2).abs() < 1e-9);
    assert_eq!((m.rows, m.count), (60, 12));
    assert_eq!(m.literal, None);
}

#[test]
fn a_missing_line_without_the_counts_still_reads() {
    let out =
        "{\"t\":\"sample\",\"nodes\":4,\"distinct_first\":1,\"usable\":4,\"stopped\":\"none\"}\n\
               {\"t\":\"missing\",\"share\":0.14,\"rule\":\"none\"}\n{\"t\":\"done\"}\n";
    let m = parse_link_output(out).unwrap().missing.unwrap();
    assert_eq!(m.rule, MissingRule::None);
    assert_eq!((m.rows, m.count), (0, 0));
}

#[test]
fn more_missing_rows_than_rows_is_a_linker_bug() {
    let bad = missing("null", Some(2), None).replace("\"count\":12", "\"count\":61");
    assert!(parse_link_output(&bad).is_err());
}

// ── what the answer says ───────────────────────────────────────────────────

#[test]
fn missing_rows_reports_the_share_the_counts_and_the_rule_in_the_masters_names() {
    let m = parse_link_output(&missing("null", Some(2), None))
        .unwrap()
        .missing
        .unwrap();
    let v = missing_rows_json(&m, &view());
    assert_eq!(v["share"], json!(0.2));
    assert_eq!(v["rows"], 60);
    assert_eq!(v["missing"], 12);
    assert_eq!(v["rule"], json!({"kind": "null", "field": "Field2"}));
}

#[test]
fn no_rule_is_a_null_rule_and_the_share_stays() {
    let m = parse_link_output(&missing("none", None, None))
        .unwrap()
        .missing
        .unwrap();
    let v = missing_rows_json(&m, &view());
    assert_eq!(v["rule"], json!(null));
    assert_eq!(v["share"], json!(0.2));
}

#[test]
fn a_fixed_rules_literal_is_in_the_answer_only_when_the_linker_wrote_it() {
    let held = parse_link_output(&missing("fixed", Some(5), None))
        .unwrap()
        .missing
        .unwrap();
    let v = missing_rows_json(&held, &view());
    assert_eq!(v["rule"], json!({"kind": "fixed", "field": "Field5"}));
    let given = parse_link_output(&missing("fixed", Some(5), Some("A")))
        .unwrap()
        .missing
        .unwrap();
    let v = missing_rows_json(&given, &view());
    assert_eq!(
        v["rule"],
        json!({"kind": "fixed", "field": "Field5", "literal": "A"})
    );
}

// ── the condition ──────────────────────────────────────────────────────────

fn cond(rule: &str, field: u32, literal: Option<&str>) -> Result<String, String> {
    let m = parse_link_output(&missing(rule, Some(field), literal))
        .unwrap()
        .missing
        .unwrap();
    condition_for(&m, &view()).map(|c| c.expression)
}

#[test]
fn a_null_rule_is_the_condition_that_skips_null_only() {
    assert_eq!(cond("null", 2, None).unwrap(), "{Field2}'=\"\"");
}

#[test]
fn an_empty_string_rule_is_not_the_null_condition() {
    // Measured (open_c0_condition): an empty string is stored as $c(0), so `{F}'=""` does not skip it.
    assert_eq!(cond("empty_string", 2, None).unwrap(), "{Field2}'=$c(0)");
}

#[test]
fn a_fixed_rule_needs_its_literal_and_quotes_it() {
    assert_eq!(cond("fixed", 5, Some("A")).unwrap(), "{Field5}=\"A\"");
    assert_eq!(
        cond("fixed", 5, Some("say \"hi\"")).unwrap(),
        "{Field5}=\"say \"\"hi\"\"\""
    );
    let held = cond("fixed", 5, None).unwrap_err();
    assert!(held.contains("include_values"), "{held}");
}

#[test]
fn a_literal_with_a_control_character_is_not_drafted() {
    assert!(cond("fixed", 5, Some("a\u{1}b")).is_err());
    assert!(cond("fixed", 5, Some("a\nb")).is_err());
}

#[test]
fn no_rule_and_an_unknown_field_draft_no_condition() {
    let none = parse_link_output(&missing("none", None, None))
        .unwrap()
        .missing
        .unwrap();
    assert!(condition_for(&none, &view()).is_err());
    assert!(cond("null", 9, None).is_err());
}

#[test]
fn the_condition_goes_on_the_map_with_the_population_type_and_validates() {
    let (mut map, _) = draft_index_map(&city_link(), &view()).unwrap();
    let m = parse_link_output(&missing("null", Some(2), None))
        .unwrap()
        .missing
        .unwrap();
    let c = condition_for(&m, &view()).unwrap();
    apply_condition(&mut map, &c);
    assert_eq!(map["condition"], "{Field2}'=\"\"");
    assert_eq!(map["condition_fields"], "Field2");
    assert_eq!(map["population_type"], "conditional");
    passes_the_validator(&map);
}

#[test]
fn a_fixed_condition_names_its_field_in_condition_fields() {
    let (mut map, _) = draft_index_map(&city_link(), &view()).unwrap();
    let m = parse_link_output(&missing("fixed", Some(5), Some("A")))
        .unwrap()
        .missing
        .unwrap();
    apply_condition(&mut map, &condition_for(&m, &view()).unwrap());
    assert_eq!(map["condition"], "{Field5}=\"A\"");
    assert_eq!(map["condition_fields"], "Field5");
    passes_the_validator(&map);
}

// ── the scratch check asks the master the same question ────────────────────

#[test]
fn the_master_side_query_counts_the_rows_the_condition_keeps_among_the_first_rows() {
    let rule = |rule: &str, field: u32, lit: Option<&str>| {
        let m = parse_link_output(&missing(rule, Some(field), lit))
            .unwrap()
            .missing
            .unwrap();
        condition_for(&m, &view()).unwrap()
    };
    let q = present_count_query("IrisDevTmp.SmDraft1", "Field2", &rule("null", 2, None), 60);
    assert!(q.contains("TOP 60"), "{q}");
    assert!(q.contains("ORDER BY %ID"), "{q}");
    assert!(q.contains("IS NOT NULL"), "{q}");
    let q = present_count_query(
        "IrisDevTmp.SmDraft1",
        "Field2",
        &rule("empty_string", 2, None),
        60,
    );
    assert!(q.contains("IS NULL OR") && q.contains("<> ''"), "{q}");
    let q = present_count_query(
        "IrisDevTmp.SmDraft1",
        "Field5",
        &rule("fixed", 5, Some("it's")),
        60,
    );
    assert!(q.contains("= 'it''s'"), "{q}");
}

// ── the linker's code ──────────────────────────────────────────────────────

fn fields() -> Vec<FieldAccess> {
    (1..=5)
        .map(|piece| FieldAccess {
            node: None,
            delimiter: Some(94),
            piece,
        })
        .collect()
}

#[test]
fn the_linker_classifies_the_master_rows_the_index_lacks() {
    let f = fields();
    let mut req = LinkRequest {
        master: "^IadSm173F22",
        candidate: "^IadSm173F22X",
        sub_prefix: &[],
        key_parts: 1,
        fields: &f,
        caps: Caps::for_node_cap(200),
        with_values: false,
    };
    let code = build_code(&req).unwrap();
    assert!(code.contains("o.t=\"missing\""), "{code}");
    for rule in ["null", "empty_string", "fixed", "none"] {
        assert!(code.contains(&format!("\"{rule}\"")), "{rule}: {code}");
    }
    // The literal of a fixed rule is a sampled value: it is written only under `vals`.
    assert!(code.contains("Set:vals o.literal"), "{code}");
    assert!(code.contains("vals=0"), "{code}");
    req.with_values = true;
    assert!(build_code(&req).unwrap().contains("vals=1"));
    // Still only JSON lines.
    for w in code.match_indices("Write ").map(|(i, _)| &code[i..]) {
        let stmt = w.lines().next().unwrap();
        assert!(stmt.starts_with("Write o.%ToJSON(),!"), "{stmt}");
    }
}

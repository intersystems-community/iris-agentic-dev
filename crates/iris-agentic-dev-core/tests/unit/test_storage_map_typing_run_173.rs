// Spec 173, T053/T054 (US6): the typing call that feeds the class draft, with no IRIS. The generated
// code, the lines it writes and how they read back, the collations the linking evidence gives each
// property, and the Storage block spliced into the class source. The live half is
// `test_storage_map_typing_173_live`.

use iris_agentic_dev_core::tools::storage_map::link::{
    IdForm, IdLink, Link, PositionScore, SubscriptLink, Transform, Verdict,
};
use iris_agentic_dev_core::tools::storage_map::link_code::FieldAccess;
use iris_agentic_dev_core::tools::storage_map::typing::{class_source, property_type, Evidence};
use iris_agentic_dev_core::tools::storage_map::typing_run::{
    build_code, class_name_for, collation_picks, parse_output, properties, splice_storage,
    FieldCollation, TypingRequest,
};
use serde_json::json;

fn fields(n: usize) -> Vec<FieldAccess> {
    (1..=n as u32)
        .map(|piece| FieldAccess {
            node: None,
            delimiter: Some(94),
            piece,
        })
        .collect()
}

fn request<'a>(master: &'a str, f: &'a [FieldAccess]) -> TypingRequest<'a> {
    TypingRequest {
        master,
        key_parts: 1,
        fields: f,
        node_cap: 200,
        time_cap_secs: 5,
    }
}

// ── the generated code ─────────────────────────────────────────────────────

#[test]
fn the_code_names_the_master_through_a_quoted_literal_and_calls_no_internal_function() {
    let f = fields(3);
    let code = build_code(&request("^IadSm173F37", &f)).unwrap();
    assert!(code.contains("Set mst=\"^IadSm173F37\""), "{code}");
    for banned in ["$ZU(", "$ZUTIL(", "$ZCVT", "$ZCONVERT", "$zu(", "$zcvt"] {
        assert!(!code.contains(banned), "{banned} in the code:\n{code}");
    }
    // The date window comes from the server's own documented date function.
    assert!(code.contains("$ZDATEH(\"1900-01-01\",3)"), "{code}");
    assert!(code.contains("$ZDATEH(\"2100-12-31\",3)"), "{code}");
    assert!(code.contains("$IsValidNum("), "{code}");
}

#[test]
fn the_code_writes_json_objects_and_never_a_value() {
    let f = fields(2);
    let code = build_code(&request("^G", &f)).unwrap();
    let writes: Vec<&str> = code.lines().filter(|l| l.contains("Write ")).collect();
    assert!(!writes.is_empty());
    for line in writes {
        assert!(
            line.contains("Write o.%ToJSON(),!"),
            "a Write that is not a JSON object: {line}"
        );
    }
    // A property name with an underscore is set through %Set, never as `o.max_scale` (the `_` is
    // the concatenation operator).
    assert!(code.contains("%Set(\"max_scale\""), "{code}");
    assert!(code.contains("%Set(\"date_fit\""), "{code}");
    assert!(code.contains("%Set(\"max_length\""), "{code}");
    assert!(!code.contains("o.max_scale"), "{code}");
}

#[test]
fn a_hostile_global_name_builds_nothing() {
    let f = fields(1);
    for bad in ["^A) Kill ^B", "^A(\"x\")", "A B", "", "^"] {
        assert!(
            build_code(&request(bad, &f)).is_err(),
            "{bad:?} was accepted"
        );
    }
}

#[test]
fn no_fields_or_no_key_parts_build_nothing() {
    let none: Vec<FieldAccess> = Vec::new();
    assert!(build_code(&request("^G", &none)).is_err());
    let f = fields(1);
    let mut r = request("^G", &f);
    r.key_parts = 0;
    assert!(build_code(&r).is_err());
}

#[test]
fn the_field_table_carries_node_delimiter_and_piece() {
    let f = vec![
        FieldAccess {
            node: Some("contact".into()),
            delimiter: None,
            piece: 2,
        },
        FieldAccess {
            node: None,
            delimiter: Some(124),
            piece: 3,
        },
    ];
    let code = build_code(&request("^G", &f)).unwrap();
    assert!(code.contains("Set nf=2"), "{code}");
    assert!(
        code.contains("Set fa(1)=$ListBuild(\"contact\",0,2)"),
        "{code}"
    );
    assert!(code.contains("Set fa(2)=$ListBuild(\"\",124,3)"), "{code}");
}

// ── the lines it writes ────────────────────────────────────────────────────

const GOOD: &str = r#"{"t":"rows","n":40,"stopped":"none"}
{"t":"field","field":1,"of":40,"integer":40,"numeric":40,"max_scale":0,"date_fit":0,"max_length":2}
{"t":"field","field":2,"of":35,"integer":0,"numeric":0,"max_scale":0,"date_fit":0,"max_length":37}
{"t":"done"}
"#;

#[test]
fn parse_reads_counts_per_field() {
    let r = parse_output(GOOD, 2).unwrap();
    assert_eq!(r.rows, 40);
    assert_eq!(r.stopped, "none");
    assert_eq!(
        r.evidence[0],
        Evidence {
            of: 40,
            integer: 40,
            numeric: 40,
            max_scale: 0,
            date_fit: 0,
            max_length: 2
        }
    );
    assert_eq!(r.evidence[1].of, 35);
    assert_eq!(r.evidence[1].max_length, 37);
}

#[test]
fn parse_refuses_a_run_that_did_not_finish_or_wrote_nonsense() {
    // No done line: a failed run.
    let cut = GOOD.replace("{\"t\":\"done\"}\n", "");
    assert!(parse_output(&cut, 2).is_err());
    // An error line carries its text.
    let err = "{\"t\":\"error\",\"text\":\"<UNDEFINED> mr\"}\n";
    let why = parse_output(err, 2).unwrap_err();
    assert!(why.contains("<UNDEFINED> mr"), "{why}");
    // A field the table does not have, a field twice, a missing field.
    let outside = GOOD.replace("\"field\":2", "\"field\":3");
    assert!(parse_output(&outside, 2).is_err());
    let twice = GOOD.replace("\"field\":2", "\"field\":1");
    assert!(parse_output(&twice, 2).is_err());
    assert!(parse_output(GOOD, 3).is_err());
    // A count above `of` cannot be true.
    let over = GOOD.replace("\"integer\":40", "\"integer\":41");
    assert!(parse_output(&over, 2).is_err());
    // Something that is not JSON.
    assert!(parse_output("garbage\n", 2).is_err());
}

// ── the collation each property takes from the linking evidence ────────────

fn pick(field: u32, transform: Transform, length: Option<u32>, piece: Option<u8>) -> PositionScore {
    PositionScore {
        field,
        transform,
        length,
        piece,
        matches: 40,
        total: 40,
    }
}

fn link(verdict: Verdict, subs: Vec<SubscriptLink>) -> Link {
    Link {
        global: "^IadSm173F37X".into(),
        map_name: "X".into(),
        verdict,
        id: IdLink {
            form: IdForm::Subscript,
            positions: vec![2],
        },
        subscripts: subs,
        data_in_value: Vec::new(),
        unexamined: Vec::new(),
    }
}

fn sub(position: u32, verdict: Verdict, p: Option<PositionScore>) -> SubscriptLink {
    SubscriptLink {
        position,
        verdict,
        pick: p,
        alternatives: Vec::new(),
        null_marker: None,
    }
}

#[test]
fn only_an_inferred_pair_is_collation_evidence() {
    let l = link(
        Verdict::Inferred,
        vec![sub(
            1,
            Verdict::Inferred,
            Some(pick(3, Transform::Exact, None, None)),
        )],
    );
    assert_eq!(
        collation_picks(&l),
        vec![FieldCollation {
            field: 3,
            transform: Transform::Exact,
            length: None
        }]
    );
    // A tie is a guess, not evidence.
    let tie = link(
        Verdict::Ambiguous,
        vec![sub(
            1,
            Verdict::Ambiguous,
            Some(pick(3, Transform::Exact, None, None)),
        )],
    );
    assert!(collation_picks(&tie).is_empty());
    // A link nothing explains.
    let none = link(
        Verdict::NotExplained,
        vec![sub(1, Verdict::NotExplained, None)],
    );
    assert!(collation_picks(&none).is_empty());
    // A piece of a collated value is no property collation.
    let piece = link(
        Verdict::Inferred,
        vec![sub(
            1,
            Verdict::Inferred,
            Some(pick(1, Transform::SqlUpper, None, Some(1))),
        )],
    );
    assert!(collation_picks(&piece).is_empty());
    // A length variant keeps its length.
    let long = link(
        Verdict::Inferred,
        vec![sub(
            1,
            Verdict::Inferred,
            Some(pick(1, Transform::SqlString, Some(4), None)),
        )],
    );
    assert_eq!(collation_picks(&long)[0].length, Some(4));
}

fn ev_text(max_length: u32) -> Evidence {
    Evidence {
        of: 10,
        max_length,
        ..Evidence::default()
    }
}

fn ev_int() -> Evidence {
    Evidence {
        of: 10,
        integer: 10,
        numeric: 10,
        max_length: 2,
        ..Evidence::default()
    }
}

#[test]
fn a_string_property_takes_its_collation_and_other_types_take_none() {
    let names = vec![
        "Name".to_string(),
        "Alias".to_string(),
        "City".to_string(),
        "Qty".to_string(),
    ];
    let evidence = vec![ev_text(18), ev_text(18), ev_text(6), ev_int()];
    let picks = vec![
        FieldCollation {
            field: 1,
            transform: Transform::SqlUpper,
            length: None,
        },
        FieldCollation {
            field: 2,
            transform: Transform::SqlString,
            length: None,
        },
        FieldCollation {
            field: 3,
            transform: Transform::Exact,
            length: None,
        },
        // A %Integer takes no COLLATION: IRIS refuses the parameter (ERROR #5480).
        FieldCollation {
            field: 4,
            transform: Transform::Exact,
            length: None,
        },
    ];
    let props = properties(&names, &evidence, &picks).unwrap();
    assert_eq!(props[0].collation, None, "sqlupper is the default");
    assert_eq!(
        props[1].collation.as_deref(),
        Some("COLLATION = \"SQLSTRING\"")
    );
    assert_eq!(props[2].collation.as_deref(), Some("COLLATION = \"EXACT\""));
    assert_eq!(props[3].ty.name, "%Integer");
    assert_eq!(props[3].collation, None);
    let draft = class_source("Draft.T", &props).unwrap();
    assert!(
        !draft.source.contains("%Integer(COLLATION"),
        "{}",
        draft.source
    );
}

#[test]
fn a_field_with_no_values_is_a_string() {
    let names = vec!["Empty".to_string()];
    let evidence = vec![Evidence::default()];
    let props = properties(&names, &evidence, &[]).unwrap();
    assert_eq!(props[0].ty.name, "%String");
}

#[test]
fn the_names_and_the_evidence_must_agree_in_number() {
    let names = vec!["A".to_string(), "B".to_string()];
    assert!(properties(&names, &[ev_int()], &[]).is_err());
    // A pick for a field that does not exist.
    let bad = vec![FieldCollation {
        field: 9,
        transform: Transform::Exact,
        length: None,
    }];
    assert!(properties(&names, &[ev_int(), ev_int()], &bad).is_err());
}

#[test]
fn class_source_refuses_a_collation_on_a_type_that_has_none() {
    use iris_agentic_dev_core::tools::storage_map::typing::PropertySpec;
    let p = PropertySpec {
        name: "Age".into(),
        ty: property_type(&ev_int()),
        collation: Some("COLLATION = \"EXACT\"".into()),
    };
    let why = class_source("Draft.T", &[p]).unwrap_err();
    assert!(why.contains("COLLATION") && why.contains("Age"), "{why}");
}

// ── the class name and the Storage block ───────────────────────────────────

#[test]
fn the_class_is_named_from_the_global() {
    assert_eq!(class_name_for("^IadSm173F37"), "Draft.IadSm173F37");
    assert_eq!(class_name_for("^%Foo"), "Draft.Foo");
    assert_eq!(class_name_for("^9lives"), "Draft.G9lives");
    assert_eq!(class_name_for("^"), "Draft.Master");
}

#[test]
fn the_storage_block_goes_before_the_final_brace() {
    let names = vec!["Name".to_string()];
    let props = properties(&names, &[ev_text(5)], &[]).unwrap();
    let draft = class_source("Draft.T", &props).unwrap();
    let map = json!({
        "name": "X",
        "type": "index",
        "global": "^IadSm173F37X",
        "subscripts": [
            {"level": 1, "expression": "$$SQLUPPER({Name})"},
            {"level": 2, "expression": "{ID}"},
        ],
    });
    let text = splice_storage(&draft.source, &[&map]).unwrap();
    assert!(text.contains("Storage SQLStorage"), "{text}");
    assert!(text.contains("<SQLMap name=\"X\">"), "{text}");
    assert!(text.contains("$$SQLUPPER({Name})"), "{text}");
    assert!(text.trim_end().ends_with('}'), "{text}");
    let storage = text.find("Storage SQLStorage").unwrap();
    let last_prop = text.rfind("Property ").unwrap();
    assert!(
        storage > last_prop,
        "the Storage block comes after the properties"
    );
    // Exactly one more block than the source had: the class's own closing brace is still last.
    assert_eq!(text.matches("Class Draft.T").count(), 1);
    // A source that does not end in a brace is refused.
    assert!(splice_storage("no brace here", &[&map]).is_err());
}

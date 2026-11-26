// Spec 173, T048 to T050: the pure half of linking a bitmap index and recognising a bit-string
// master. The linker's `bitmap` line is read by `parse_link_output`, judged by `bitmap_verdict`,
// assembled by `assemble_bitmap` and drafted by `draft_bitmap_map`; `looks_like_bit_master` decides
// whether the master probe runs at all. Nothing here touches IRIS.

use iris_agentic_dev_core::tools::storage_map::bitmap::{
    assemble_bitmap, bitmap_verdict, constant_literal, draft_bitmap_map, looks_like_bit_master,
    parse_master_bits,
};
use iris_agentic_dev_core::tools::storage_map::link::{parse_link_output, MasterView, Verdict};
use iris_agentic_dev_core::tools::storage_map::model::map_node;
use iris_agentic_dev_core::tools::storage_map::sample::parse;
use iris_agentic_dev_core::tools::storage_map::tree::Node;
use iris_agentic_dev_core::tools::storage_map::validate::validate;
use serde_json::json;

fn view() -> MasterView {
    MasterView {
        global: "^IadSm173F19".into(),
        field_names: vec![
            "Name".into(),
            "City".into(),
            "Age".into(),
            "Born".into(),
            "Status".into(),
        ],
        key_names: vec!["ID".into()],
    }
}

fn view_with_leading_blank() -> MasterView {
    MasterView {
        global: "^IadSm173.F20D".into(),
        field_names: vec![
            "Class".into(),
            "Name".into(),
            "City".into(),
            "Age".into(),
            "Born".into(),
            "Status".into(),
        ],
        key_names: vec!["ID".into()],
    }
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

// ── the bitmap line ────────────────────────────────────────────────────────

const F19_OUT: &str = r#"{"t":"sample","nodes":40,"distinct_first":3,"usable":40,"stopped":"none"}
{"t":"pos","position":1,"total":40,"distinct":3,"scores":[{"field":5,"transform":"exact","length":null,"piece":null,"matches":40}]}
{"t":"bitmap","chunk_position":2,"ids_checked":40,"ids_found":40}
{"t":"done"}
"#;

const F20_OUT: &str = r#"{"t":"sample","nodes":40,"distinct_first":1,"usable":40,"stopped":"none"}
{"t":"pos","position":1,"total":40,"distinct":1,"scores":[]}
{"t":"pos","position":2,"total":40,"distinct":3,"scores":[{"field":6,"transform":"sqlupper","length":null,"piece":null,"matches":40}]}
{"t":"bitmap","chunk_position":3,"ids_checked":40,"ids_found":40,"lead":"StatusIdx"}
{"t":"done"}
"#;

#[test]
fn the_bitmap_line_carries_the_chunk_position_and_the_counts() {
    let r = parse_link_output(F19_OUT).unwrap();
    assert_eq!(r.bitmaps.len(), 1);
    let b = &r.bitmaps[0];
    assert_eq!((b.chunk_position, b.ids_checked, b.ids_found), (2, 40, 40));
    assert_eq!(b.lead, None);
    assert_eq!((b.later_checked, b.later_found), (0, 0));
}

#[test]
fn the_bitmap_line_may_name_the_index_and_count_later_chunks() {
    let out = r#"{"t":"sample","nodes":43,"distinct_first":1,"usable":40,"stopped":"none"}
{"t":"bitmap","chunk_position":3,"ids_checked":43,"ids_found":40,"later_checked":3,"later_found":0,"lead":"StatusIdx"}
{"t":"done"}
"#;
    let b = parse_link_output(out).unwrap().bitmaps.remove(0);
    assert_eq!(b.lead.as_deref(), Some("StatusIdx"));
    assert_eq!((b.later_checked, b.later_found), (3, 0));
    // More found than checked is a linker bug, not a report.
    let bad = out.replace("\"later_found\":0", "\"later_found\":4");
    assert!(parse_link_output(&bad).is_err());
}

// ── the verdict ────────────────────────────────────────────────────────────

fn line(checked: u32, found: u32, later_checked: u32, later_found: u32) -> String {
    format!(
        "{{\"t\":\"sample\",\"nodes\":{checked},\"distinct_first\":3,\"usable\":{found},\"stopped\":\"none\"}}\n\
         {{\"t\":\"bitmap\",\"chunk_position\":2,\"ids_checked\":{checked},\"ids_found\":{found},\"later_checked\":{later_checked},\"later_found\":{later_found}}}\n\
         {{\"t\":\"done\"}}\n"
    )
}

fn bitmap_line(
    checked: u32,
    found: u32,
    lc: u32,
    lf: u32,
) -> iris_agentic_dev_core::tools::storage_map::link::BitmapLine {
    parse_link_output(&line(checked, found, lc, lf))
        .unwrap()
        .bitmaps
        .remove(0)
}

#[test]
fn ids_that_find_master_rows_keep_the_position_verdict() {
    let (v, assumed) = bitmap_verdict(&bitmap_line(40, 40, 0, 0), Verdict::Inferred);
    assert_eq!(v, Verdict::Inferred);
    assert!(assumed.is_empty(), "{assumed:?}");
}

#[test]
fn ids_that_are_not_in_the_master_are_not_linked() {
    assert_eq!(
        bitmap_verdict(&bitmap_line(40, 0, 0, 0), Verdict::Inferred).0,
        Verdict::NotLinked
    );
    // 0.75 is under the 0.9 floor.
    assert_eq!(
        bitmap_verdict(&bitmap_line(40, 30, 0, 0), Verdict::Inferred).0,
        Verdict::NotLinked
    );
    // Nothing decoded: nothing linked.
    assert_eq!(
        bitmap_verdict(&bitmap_line(0, 0, 0, 0), Verdict::Inferred).0,
        Verdict::NotLinked
    );
}

#[test]
fn ids_from_later_chunks_that_find_nothing_make_the_chunk_size_an_assumption() {
    // 40 of 43 is 0.93, over the floor, but the 3 IDs decoded from chunk 2 on found no row.
    let (v, assumed) = bitmap_verdict(&bitmap_line(43, 40, 3, 0), Verdict::Inferred);
    assert_eq!(v, Verdict::Ambiguous);
    assert_eq!(assumed.len(), 1, "{assumed:?}");
    assert!(assumed[0].contains("64000"), "{assumed:?}");
    // Later chunks that do resolve confirm the size.
    let (v, assumed) = bitmap_verdict(&bitmap_line(43, 43, 3, 3), Verdict::Inferred);
    assert_eq!(v, Verdict::Inferred);
    assert!(assumed.is_empty());
}

#[test]
fn a_weaker_position_verdict_is_not_hidden_by_good_ids() {
    let (v, _) = bitmap_verdict(&bitmap_line(40, 40, 0, 0), Verdict::NotExplained);
    assert_eq!(v, Verdict::NotExplained);
}

// ── the assembled link ─────────────────────────────────────────────────────

#[test]
fn a_map_compiled_bitmap_is_explained_by_its_one_position() {
    let r = parse_link_output(F19_OUT).unwrap();
    let a = assemble_bitmap("^IadSm173F19X", "X", &r).expect("a bitmap line");
    assert_eq!(a.link.verdict, Verdict::Inferred);
    assert!((a.id_agreement - 1.0).abs() < 1e-9);
    let (map, guesses) = draft_bitmap_map(&a, &view(), None).unwrap();
    assert_eq!(map["type"], "bitmap");
    assert_eq!(map["global"], "^IadSm173F19X");
    assert_eq!(map["name"], "X");
    let subs = map["subscripts"].as_array().unwrap();
    assert_eq!(subs.len(), 1, "the field alone: {map}");
    assert_eq!(subs[0]["expression"], "{Status}");
    assert_eq!(subs[0]["level"], 1);
    assert!(guesses.is_empty(), "{guesses:?}");
    passes_the_validator(&map);
}

#[test]
fn a_class_compiled_bitmap_keeps_its_index_name_as_the_first_subscript() {
    let r = parse_link_output(F20_OUT).unwrap();
    let a = assemble_bitmap("^IadSm173.F20I", "I", &r).expect("a bitmap line");
    // The index name is a constant position; it does not make the link `constant`.
    assert_eq!(a.link.verdict, Verdict::Inferred, "{:?}", a.link);
    let lead = r.bitmaps[0].lead.as_deref();
    assert_eq!(lead, Some("StatusIdx"));
    let (map, _) = draft_bitmap_map(&a, &view_with_leading_blank(), lead).unwrap();
    let exprs: Vec<&str> = map["subscripts"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s["expression"].as_str().unwrap())
        .collect();
    assert_eq!(exprs, ["\"StatusIdx\"", "$$SQLUPPER({Status})"], "{map}");
    passes_the_validator(&map);
}

#[test]
fn an_index_name_that_is_not_a_name_is_not_spliced() {
    let r = parse_link_output(F20_OUT).unwrap();
    let a = assemble_bitmap("^IadSm173.F20I", "I", &r).unwrap();
    for bad in ["A\"); Kill ^X ;(\"", "has space", "", "1abc"] {
        assert!(
            draft_bitmap_map(&a, &view_with_leading_blank(), Some(bad)).is_err(),
            "{bad:?}"
        );
    }
}

#[test]
fn no_bitmap_line_no_bitmap() {
    let out = r#"{"t":"sample","nodes":40,"distinct_first":3,"usable":40,"stopped":"none"}
{"t":"done"}
"#;
    let r = parse_link_output(out).unwrap();
    assert!(assemble_bitmap("^G", "X", &r).is_none());
}

#[test]
fn a_bitmap_whose_position_nothing_explains_drafts_no_map() {
    let out = r#"{"t":"sample","nodes":40,"distinct_first":3,"usable":40,"stopped":"none"}
{"t":"pos","position":1,"total":40,"distinct":3,"scores":[{"field":2,"transform":"exact","length":null,"piece":null,"matches":13}]}
{"t":"bitmap","chunk_position":2,"ids_checked":40,"ids_found":40}
{"t":"done"}
"#;
    let r = parse_link_output(out).unwrap();
    let a = assemble_bitmap("^G", "X", &r).unwrap();
    assert_eq!(a.link.verdict, Verdict::NotExplained);
    assert!(draft_bitmap_map(&a, &view(), None).is_err());
}

// ── the master probe ───────────────────────────────────────────────────────

fn shape(
    lines: &[serde_json::Value],
) -> iris_agentic_dev_core::tools::storage_map::shape::ShapeReport {
    let mut out = String::new();
    for l in lines {
        out.push_str(&l.to_string());
        out.push('\n');
    }
    out.push_str("{\"done\":1,\"c\":9,\"capped\":0}\n");
    parse(&out).unwrap()
}

#[test]
fn a_global_of_const_chunk_bit_strings_looks_like_a_bit_master() {
    // ^G(0, chunk): depth 1 holds no value, depth 2 holds a plain string at a numeric subscript.
    let r = shape(&[
        json!({"d":1,"s":10}),
        json!({"d":2,"s":1,"f":"plain","p":1}),
        json!({"d":2,"s":1,"f":"plain","p":1}),
    ]);
    assert!(looks_like_bit_master(&r));
    // A dense bit string holds the delimiter bytes the sampler looks for.
    let r = shape(&[
        json!({"d":1,"s":10}),
        json!({"d":2,"s":1,"f":"delimited","dl":94,"p":3}),
    ]);
    assert!(looks_like_bit_master(&r));
}

#[test]
fn an_ordinary_master_does_not_look_like_a_bit_master() {
    // ^G(id) = a^b^c
    let flat = shape(&[
        json!({"d":1,"s":1,"f":"delimited","dl":94,"p":5}),
        json!({"d":1,"s":1,"f":"delimited","dl":94,"p":5}),
    ]);
    assert!(!looks_like_bit_master(&flat));
    // $LIST values at depth 2
    let list = shape(&[
        json!({"d":1,"s":10}),
        json!({"d":2,"s":1,"f":"$LIST","p":3}),
    ]);
    assert!(!looks_like_bit_master(&list));
    // a literal sub-node name at depth 2: ^G(id,"EV")
    let named = shape(&[
        json!({"d":1,"s":11,"f":"delimited","dl":94,"p":3}),
        json!({"d":2,"s":1,"f":"plain","p":1,"n":"EV"}),
    ]);
    assert!(!looks_like_bit_master(&named));
    // nothing sampled
    assert!(!looks_like_bit_master(&shape(&[])));
}

#[test]
fn the_probe_answer_is_read() {
    let out = r#"{"t":"masterbits","found":true,"constant":"0","chunks":2,"ids":6,"idsum":192013,"first":2,"last":64001}
{"t":"done"}
"#;
    let m = parse_master_bits(out).unwrap().expect("found");
    assert_eq!(m.constant, "0");
    assert_eq!(
        (m.chunks, m.ids, m.id_sum, m.first, m.last),
        (2, 6, 192013, 2, 64001)
    );
    let none = "{\"t\":\"masterbits\",\"found\":false}\n{\"t\":\"done\"}\n";
    assert_eq!(parse_master_bits(none).unwrap(), None);
}

#[test]
fn a_probe_that_did_not_finish_or_failed_is_an_error() {
    assert!(parse_master_bits("{\"t\":\"masterbits\",\"found\":false}\n").is_err());
    assert!(parse_master_bits("{\"t\":\"error\",\"text\":\"<UNDEFINED>\"}\n").is_err());
    assert!(parse_master_bits("garbage\n").is_err());
    // Counts that cannot be: an ID sum with no IDs.
    let bad = r#"{"t":"masterbits","found":true,"constant":"0","chunks":1,"ids":0,"idsum":5,"first":0,"last":0}
{"t":"done"}
"#;
    assert!(parse_master_bits(bad).is_err());
}

#[test]
fn the_constant_is_a_number_or_a_name_and_nothing_else() {
    assert_eq!(constant_literal("0").unwrap(), "0");
    assert_eq!(constant_literal("12").unwrap(), "12");
    assert_eq!(constant_literal("Bits").unwrap(), "\"Bits\"");
    for bad in ["", "a b", "x\"y", "1e3", "0; Kill ^X", "a(\"b\")"] {
        assert!(constant_literal(bad).is_err(), "{bad:?}");
    }
}

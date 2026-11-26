// Spec 173, T017 to T019: the pure half of linking an index global to its master. `spelling` and
// `verdict` are the closed set and the thresholds of data-model.md, `parse_link_output` reads the
// linker's lines (the contract in `link_code.rs`), `draft_index_map` turns a link into a map the
// model validator accepts. Nothing here touches IRIS.

use iris_agentic_dev_core::tools::storage_map::link::{
    draft_index_map, map_name, parse_link_output, spelling, verdict, DataInValue, EmptyKind,
    IdForm, IdLink, Link, MasterView, NullKind, NullMarker, PositionScore, Stopped, SubscriptLink,
    Transform, Verdict, VerdictInput,
};
use iris_agentic_dev_core::tools::storage_map::model::map_node;
use iris_agentic_dev_core::tools::storage_map::tree::Node;
use iris_agentic_dev_core::tools::storage_map::validate::validate;
use serde_json::json;

fn score(field: u32, transform: Transform, matches: u32, total: u32) -> PositionScore {
    PositionScore {
        field,
        transform,
        length: None,
        piece: None,
        matches,
        total,
    }
}

// ── spelling ───────────────────────────────────────────────────────────────

#[test]
fn spelling_gives_the_draft_strings_of_the_table() {
    let cases: &[(Transform, Option<u32>, Option<u8>, &str)] = &[
        (Transform::Exact, None, None, "{Name}"),
        (Transform::SqlUpper, None, None, "$$SQLUPPER({Name})"),
        (Transform::SqlUpper, Some(4), None, "$$SQLUPPER({Name},4)"),
        (Transform::SqlString, None, None, "$$SQLSTRING({Name})"),
        (
            Transform::SqlString,
            Some(10),
            None,
            "$$SQLSTRING({Name},10)",
        ),
        (Transform::Str, None, None, "$$STRING({Name})"),
        (Transform::Str, Some(3), None, "$$STRING({Name},3)"),
        (Transform::AlphaUp, None, None, "$$ALPHAUP({Name})"),
        (Transform::Upper, None, None, "$$UPPER({Name})"),
        (Transform::Mvr, None, None, "$$MVR({Name})"),
        (Transform::Truncate, Some(3), None, "$$TRUNCATE({Name},3)"),
        (
            Transform::SqlUpper,
            None,
            Some(1),
            "$p($$SQLUPPER({Name}),\",\",1)",
        ),
        (
            Transform::AlphaUp,
            None,
            Some(2),
            "$p($$ALPHAUP({Name}),\",\",2)",
        ),
        (
            Transform::Truncate,
            Some(5),
            Some(2),
            "$p($$TRUNCATE({Name},5),\",\",2)",
        ),
    ];
    for (t, len, piece, want) in cases {
        assert_eq!(
            spelling(*t, "Name", *len, *piece).as_deref(),
            Ok(*want),
            "{t:?} {len:?} {piece:?}"
        );
    }
    assert_eq!(
        spelling(Transform::Exact, "City", None, None).as_deref(),
        Ok("{City}")
    );
}

#[test]
fn spelling_refuses_what_is_outside_the_closed_set() {
    // Names the server's collation family does not offer as map functions (spec fact 1).
    for outside in [
        "exact_",
        "collate",
        "plus",
        "minus",
        "space",
        "SQLUPPER ",
        "",
    ] {
        assert!(
            Transform::parse(outside).is_err(),
            "{outside:?} must not parse as a transform"
        );
    }
    // The eight of the closed set parse by the contract's names.
    let names = [
        "exact",
        "sqlupper",
        "sqlstring",
        "string",
        "alphaup",
        "upper",
        "mvr",
        "truncate",
    ];
    for n in names {
        let t = Transform::parse(n).unwrap_or_else(|e| panic!("{n}: {e}"));
        assert_eq!(t.name(), n);
    }
    // Combinations the table does not list.
    let bad: &[(Transform, Option<u32>, Option<u8>)] = &[
        (Transform::Exact, Some(3), None),
        (Transform::Exact, None, Some(1)),
        (Transform::AlphaUp, Some(3), None),
        (Transform::Upper, Some(3), None),
        (Transform::Mvr, Some(3), None),
        (Transform::Truncate, None, None),
        (Transform::SqlUpper, Some(0), None),
        (Transform::SqlUpper, None, Some(0)),
        (Transform::SqlUpper, None, Some(3)),
        (Transform::SqlUpper, Some(4), Some(1)),
    ];
    for (t, len, piece) in bad {
        assert!(
            spelling(*t, "Name", *len, *piece).is_err(),
            "{t:?} {len:?} {piece:?} is not in the table"
        );
    }
}

#[test]
fn spelling_refuses_a_field_name_that_could_break_the_expression() {
    for field in [
        "", "Na}me", "Name,1", "Na\"me", "1Name", "a b", "N{ame}", "Name)",
    ] {
        assert!(
            spelling(Transform::Exact, field, None, None).is_err(),
            "{field:?}"
        );
    }
    assert!(spelling(Transform::Exact, "%Id", None, None).is_ok());
}

// ── verdict ────────────────────────────────────────────────────────────────

fn judge(
    usable: u32,
    id_agreement: f64,
    distinct: u32,
    scores: &[PositionScore],
) -> iris_agentic_dev_core::tools::storage_map::link::Judgement {
    verdict(&VerdictInput {
        usable_nodes: usable,
        id_agreement,
        distinct,
        scores,
    })
}

#[test]
fn verdict_inferred_at_twenty_nodes_and_ninety_five_percent() {
    let s = [
        score(1, Transform::SqlUpper, 19, 20),
        score(1, Transform::Exact, 12, 20),
    ];
    let j = judge(20, 1.0, 17, &s);
    assert_eq!(j.verdict, Verdict::Inferred);
    assert_eq!(
        j.best.as_ref().map(|b| (b.field, b.transform)),
        Some((1, Transform::SqlUpper))
    );
    assert!((j.agreement - 0.95).abs() < 1e-12, "{}", j.agreement);
    assert!(j.alternatives.is_empty());
}

#[test]
fn verdict_too_few_nodes_at_nineteen() {
    let s = [score(1, Transform::SqlUpper, 19, 19)];
    let j = judge(19, 1.0, 19, &s);
    assert_eq!(j.verdict, Verdict::TooFewNodes);
}

#[test]
fn verdict_ambiguous_lists_every_tied_pair() {
    // 40/40 and 199/200-style gaps: two pairs 0.005 apart tie, a third 0.02 below does not.
    let s = [
        score(1, Transform::SqlUpper, 200, 200),
        score(2, Transform::SqlUpper, 199, 200),
        score(3, Transform::SqlUpper, 196, 200),
    ];
    let j = judge(200, 1.0, 150, &s);
    assert_eq!(j.verdict, Verdict::Ambiguous);
    let tied: Vec<u32> = j.alternatives.iter().map(|a| a.field).collect();
    assert_eq!(tied, vec![1, 2], "{:?}", j.alternatives);
    assert_eq!(j.best.as_ref().map(|b| b.field), Some(1));
}

#[test]
fn verdict_a_tie_is_within_one_hundredth_inclusive() {
    let at_edge = [
        score(1, Transform::Exact, 100, 100),
        score(2, Transform::Exact, 99, 100),
    ];
    assert_eq!(judge(100, 1.0, 50, &at_edge).verdict, Verdict::Ambiguous);
    let past_edge = [
        score(1, Transform::Exact, 100, 100),
        score(2, Transform::Exact, 98, 100),
    ];
    let j = judge(100, 1.0, 50, &past_edge);
    assert_eq!(j.verdict, Verdict::Inferred);
    assert_eq!(j.best.as_ref().map(|b| b.field), Some(1));
}

#[test]
fn verdict_not_linked_below_ninety_percent_ids() {
    let s = [score(1, Transform::SqlUpper, 40, 40)];
    let j = judge(40, 0.89, 38, &s);
    assert_eq!(j.verdict, Verdict::NotLinked);
    assert!((j.agreement - 0.89).abs() < 1e-12);
    // Exactly 0.9 is linked.
    assert_eq!(judge(40, 0.9, 38, &s).verdict, Verdict::Inferred);
}

#[test]
fn verdict_not_linked_wins_over_too_few_nodes() {
    // An unrelated global resolves few IDs, so its usable count is small too; it is not linked.
    let s = [score(1, Transform::SqlUpper, 2, 5)];
    let j = judge(5, 0.12, 38, &s);
    assert_eq!(j.verdict, Verdict::NotLinked);
}

#[test]
fn verdict_not_explained_carries_the_best_agreement() {
    let s = [
        score(1, Transform::SqlUpper, 94, 100),
        score(2, Transform::Exact, 31, 100),
    ];
    let j = judge(100, 1.0, 90, &s);
    assert_eq!(j.verdict, Verdict::NotExplained);
    assert!((j.agreement - 0.94).abs() < 1e-12, "{}", j.agreement);
    // No score at all is also not explained, with agreement 0.
    let none = judge(100, 1.0, 90, &[]);
    assert_eq!(none.verdict, Verdict::NotExplained);
    assert_eq!(none.agreement, 0.0);
}

#[test]
fn verdict_constant_for_one_distinct_value() {
    // Every field of a constant column ties, and constant still wins.
    let s = [
        score(1, Transform::Exact, 100, 100),
        score(2, Transform::Exact, 100, 100),
    ];
    let j = judge(100, 1.0, 1, &s);
    assert_eq!(j.verdict, Verdict::Constant);
}

#[test]
fn verdict_names_serialise_as_the_contract_spells_them() {
    let all = [
        (Verdict::Inferred, "inferred"),
        (Verdict::Ambiguous, "ambiguous"),
        (Verdict::TooFewNodes, "too_few_nodes"),
        (Verdict::NotLinked, "not_linked"),
        (Verdict::NotExplained, "not_explained"),
        (Verdict::Constant, "constant"),
    ];
    for (v, s) in all {
        assert_eq!(serde_json::to_value(v).unwrap(), json!(s));
        assert_eq!(v.as_str(), s);
    }
}

// ── map_name ───────────────────────────────────────────────────────────────

#[test]
fn map_name_is_the_last_segment_made_unique() {
    assert_eq!(map_name("^IadSm173A.x", &[]), "X");
    let taken = vec!["X".to_string()];
    assert_eq!(map_name("^IadSm173B.x", &taken), "X2");
    let taken = vec!["X".to_string(), "X2".to_string()];
    assert_eq!(map_name("^IadSm173C.x", &taken), "X3");
    // No dot: the whole name; a leading % is not a map name character.
    assert_eq!(map_name("^IadSm173F02X", &[]), "IadSm173F02X");
    assert_eq!(map_name("IadSm173F02X", &[]), "IadSm173F02X");
    // Taken names compare without case, and "Master" is not special here.
    assert_eq!(map_name("^G.x", &["x".to_string()]), "X2");
    // Nothing usable in the segment still gives a name.
    assert_eq!(map_name("^G.1", &[]), "Index1");
}

// ── parse_link_output ──────────────────────────────────────────────────────

/// F02's known answer, read from its seed file: the row count, how many distinct first-subscript
/// values the rows give under SQLUPPER (upper case, trailing blanks dropped), the ID position, and
/// the field number and transform that explain position 1.
struct F02Seed {
    rows: u64,
    distinct: u64,
    id_position: u64,
    field: u64,
    transform: String,
}

fn f02_seed() -> F02Seed {
    let text = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/storage173/seed/F02.json"
    ))
    .unwrap();
    let seed: serde_json::Value = serde_json::from_str(&text).unwrap();
    let columns: Vec<&str> = seed["columns"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|c| c.as_str())
        .collect();
    let answer = &seed["answer"];
    let at = columns
        .iter()
        .position(|c| Some(*c) == answer["field"].as_str())
        .unwrap();
    let rows = seed["rows"].as_array().unwrap();
    let mut keys: Vec<String> = rows
        .iter()
        .map(|r| r[at].as_str().unwrap().trim_end().to_uppercase())
        .collect();
    keys.sort();
    keys.dedup();
    F02Seed {
        rows: rows.len() as u64,
        distinct: keys.len() as u64,
        id_position: answer["id_positions"][0].as_u64().unwrap(),
        field: at as u64 + 1,
        transform: answer["transform"].as_str().unwrap().to_string(),
    }
}

const F02: &str = include_str!("../fixtures/storage173/captured/link_F02.jsonl");
const F02_NO_DONE: &str = include_str!("../fixtures/storage173/captured/link_F02_no_done.jsonl");
const IRIS_ERROR: &str = include_str!("../fixtures/storage173/captured/link_iris_error.txt");

#[test]
fn parse_reads_a_normal_run() {
    // F02's known answer comes from its seed file, not from the capture.
    let want = f02_seed();
    let r = parse_link_output(F02).expect("a complete run parses");
    assert_eq!(u64::from(r.sample.nodes), want.rows);
    assert_eq!(u64::from(r.sample.usable), want.rows);
    assert_eq!(u64::from(r.sample.distinct_first), want.distinct);
    assert_eq!(r.sample.stopped, Stopped::None);
    assert_eq!(r.ids[0].form, IdForm::Subscript);
    assert_eq!(
        r.ids[0].positions,
        vec![u32::try_from(want.id_position).unwrap()]
    );
    assert_eq!(
        (u64::from(r.ids[0].resolved), u64::from(r.ids[0].total)),
        (want.rows, want.rows)
    );
    assert_eq!(r.positions.len(), 1);
    let p = &r.positions[0];
    assert_eq!(p.position, 1);
    assert_eq!(
        (u64::from(p.total), u64::from(p.distinct)),
        (want.rows, want.distinct)
    );
    let best = &p.scores[0];
    assert_eq!(u64::from(best.field), want.field);
    assert_eq!(best.transform.name(), want.transform);
    assert_eq!(u64::from(best.matches), want.rows);
    assert_eq!(u64::from(best.total), want.rows);
    assert_eq!(best.length, None);
    assert_eq!(best.piece, None);
    assert!(
        p.scores.windows(2).all(|w| w[0].matches >= w[1].matches),
        "scores are listed best first"
    );
    assert!(r.nulls.is_empty() && r.data.is_empty() && r.bitmaps.is_empty());
    assert!(r.missing.is_none());
}

#[test]
fn parse_reads_every_line_kind() {
    let out = r#"{"t":"sample","nodes":120,"distinct_first":50,"usable":100,"stopped":"nodes"}
{"t":"id","form":"subscript","positions":[2,3],"resolved":100,"total":120}
{"t":"pos","position":1,"total":100,"distinct":7,"scores":[{"field":3,"transform":"truncate","length":3,"piece":null,"matches":100},{"field":3,"transform":"sqlupper","length":null,"piece":1,"matches":60}]}
{"t":"null","position":1,"field":2,"kind":"c0","empty":"empty_string","count":3}
{"t":"null","position":1,"field":4,"kind":"discovered","empty":"null","count":2,"length":5}
{"t":"data","field":2,"element":1,"matches":98,"total":100}
{"t":"missing","share":0.14,"rule":"fixed","field":5,"literal":null}
{"t":"bitmap","chunk_position":2,"ids_checked":1000,"ids_found":990}
{"t":"done"}
"#;
    let r = parse_link_output(out).expect("parses");
    assert_eq!(r.sample.stopped, Stopped::Nodes);
    assert_eq!(r.ids[0].positions, vec![2, 3]);
    assert_eq!((r.ids[0].resolved, r.ids[0].total), (100, 120));
    let s = &r.positions[0].scores;
    assert_eq!(s[0].length, Some(3));
    assert_eq!(s[0].transform, Transform::Truncate);
    assert_eq!(s[1].piece, Some(1));
    assert_eq!(r.nulls.len(), 2);
    assert_eq!(r.nulls[0].kind.as_str(), "c0");
    assert_eq!(r.nulls[0].empty.as_str(), "empty_string");
    assert_eq!((r.nulls[0].field, r.nulls[0].count), (2, 3));
    assert_eq!(r.nulls[1].kind.as_str(), "discovered");
    assert_eq!(r.nulls[1].empty.as_str(), "null");
    assert_eq!(r.nulls[1].length, Some(5));
    assert_eq!(r.data[0].field, 2);
    assert_eq!(
        (r.data[0].element, r.data[0].matches, r.data[0].total),
        (1, 98, 100)
    );
    let m = r.missing.as_ref().expect("a missing line");
    assert_eq!(m.rule.as_str(), "fixed");
    assert_eq!(m.field, Some(5));
    assert_eq!(m.literal, None);
    assert!((m.share - 0.14).abs() < 1e-12);
    assert_eq!(r.bitmaps[0].chunk_position, 2);
    assert_eq!(
        (r.bitmaps[0].ids_checked, r.bitmaps[0].ids_found),
        (1000, 990)
    );
}

#[test]
fn parse_a_missing_done_is_an_error_not_a_short_report() {
    let e = parse_link_output(F02_NO_DONE).expect_err("no done line");
    assert!(e.contains("done") || e.contains("finish"), "{e}");
}

#[test]
fn parse_an_iris_error_text_is_an_error() {
    let e = parse_link_output(IRIS_ERROR).expect_err("not JSON lines");
    assert!(e.contains("UNDEFINED"), "the text is quoted: {e}");
}

#[test]
fn parse_an_error_line_is_an_error_even_with_done() {
    let out = format!(
        "{}{}\n{}\n",
        F02.trim_end_matches("{\"t\":\"done\"}\n"),
        r#"{"t":"error","text":"<PROTECT> ^IadSm173F02X"}"#,
        r#"{"t":"done"}"#
    );
    let e = parse_link_output(&out).expect_err("an error line");
    assert!(e.contains("<PROTECT>"), "{e}");
}

#[test]
fn parse_refuses_what_the_contract_does_not_say() {
    let ok_head = r#"{"t":"sample","nodes":40,"distinct_first":38,"usable":40,"stopped":"none"}"#;
    let done = r#"{"t":"done"}"#;
    // An unknown line kind, an unknown transform, a score above its total, a bad piece, a position 0.
    let cases = [
        format!("{ok_head}\n{{\"t\":\"surprise\"}}\n{done}\n"),
        format!("{ok_head}\n{{\"t\":\"pos\",\"position\":1,\"total\":40,\"distinct\":3,\"scores\":[{{\"field\":1,\"transform\":\"collate\",\"length\":null,\"piece\":null,\"matches\":4}}]}}\n{done}\n"),
        format!("{ok_head}\n{{\"t\":\"pos\",\"position\":1,\"total\":40,\"distinct\":3,\"scores\":[{{\"field\":1,\"transform\":\"exact\",\"length\":null,\"piece\":null,\"matches\":41}}]}}\n{done}\n"),
        format!("{ok_head}\n{{\"t\":\"pos\",\"position\":1,\"total\":40,\"distinct\":3,\"scores\":[{{\"field\":1,\"transform\":\"upper\",\"length\":null,\"piece\":3,\"matches\":4}}]}}\n{done}\n"),
        format!("{ok_head}\n{{\"t\":\"pos\",\"position\":0,\"total\":40,\"distinct\":3,\"scores\":[]}}\n{done}\n"),
        format!("{ok_head}\n{{\"t\":\"id\",\"form\":\"subscript\",\"positions\":[2],\"resolved\":41,\"total\":40}}\n{done}\n"),
        format!("{ok_head}\n{{\"t\":\"missing\",\"share\":1.5,\"rule\":\"none\",\"field\":null,\"literal\":null}}\n{done}\n"),
        format!("{ok_head}\n{done}\nstray text\n"),
        // Two missing lines: the contract has one per run.
        format!("{ok_head}\n{{\"t\":\"missing\",\"share\":0.1,\"rule\":\"none\",\"field\":null,\"literal\":null}}\n{{\"t\":\"missing\",\"share\":0.1,\"rule\":\"null\",\"field\":2,\"literal\":null}}\n{done}\n"),
        // No sample line at all.
        format!("{done}\n"),
        // Two sample lines.
        format!("{ok_head}\n{ok_head}\n{done}\n"),
        String::new(),
    ];
    for (i, c) in cases.iter().enumerate() {
        assert!(
            parse_link_output(c).is_err(),
            "case {i} must be refused:\n{c}"
        );
    }
}

// ── draft_index_map ────────────────────────────────────────────────────────

fn master() -> MasterView {
    MasterView {
        global: "^IadSm173Person".into(),
        field_names: ["Name", "City", "Age", "Born", "Status"]
            .map(String::from)
            .to_vec(),
        key_names: vec![],
    }
}

fn f02_link() -> Link {
    Link {
        global: "^IadSm173F02X".into(),
        map_name: "X".into(),
        verdict: Verdict::Inferred,
        id: IdLink {
            form: IdForm::Subscript,
            positions: vec![2],
        },
        subscripts: vec![SubscriptLink {
            position: 1,
            verdict: Verdict::Inferred,
            pick: Some(score(1, Transform::SqlUpper, 40, 40)),
            alternatives: vec![],
            null_marker: None,
        }],
        data_in_value: vec![],
    }
}

/// The draft as the write path sees it: a Storage tree holding the map.
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

#[test]
fn draft_f02_is_the_declared_index_map() {
    let (map, guesses) = draft_index_map(&f02_link(), &master()).expect("drafts");
    assert_eq!(
        map,
        json!({
            "name": "X",
            "type": "index",
            "global": "^IadSm173F02X",
            "subscripts": [
                {"level": 1, "expression": "$$SQLUPPER({Name})"},
                {"level": 2, "expression": "{ID}"},
            ],
        })
    );
    assert!(
        guesses.is_empty(),
        "nothing in an inferred link is a guess: {guesses:?}"
    );
    passes_the_validator(&map);
}

#[test]
fn draft_f01_plain_city_is_the_field_in_braces() {
    let mut l = f02_link();
    l.subscripts[0].pick = Some(score(2, Transform::Exact, 40, 40));
    let (map, _) = draft_index_map(&l, &master()).unwrap();
    assert_eq!(map["subscripts"][0]["expression"], "{City}");
    passes_the_validator(&map);
}

#[test]
fn draft_length_and_piece_variants() {
    let mut l = f02_link();
    let mut p = score(1, Transform::SqlUpper, 40, 40);
    p.length = Some(4);
    l.subscripts[0].pick = Some(p);
    let (map, _) = draft_index_map(&l, &master()).unwrap();
    assert_eq!(map["subscripts"][0]["expression"], "$$SQLUPPER({Name},4)");
    let mut p = score(1, Transform::SqlUpper, 40, 40);
    p.piece = Some(1);
    l.subscripts[0].pick = Some(p);
    let (map, _) = draft_index_map(&l, &master()).unwrap();
    assert_eq!(
        map["subscripts"][0]["expression"],
        "$p($$SQLUPPER({Name}),\",\",1)"
    );
    passes_the_validator(&map);
}

#[test]
fn draft_composite_key_ends_with_the_key_part_names() {
    // F12: index (Name, a, b); the ID is the run at positions 2 and 3.
    let mut l = f02_link();
    l.id.positions = vec![2, 3];
    let mut m = master();
    m.key_names = vec!["A".into(), "B".into()];
    let (map, _) = draft_index_map(&l, &m).unwrap();
    assert_eq!(
        map["subscripts"],
        json!([
            {"level": 1, "expression": "$$SQLUPPER({Name})"},
            {"level": 2, "expression": "{A}"},
            {"level": 3, "expression": "{B}"},
        ])
    );
    passes_the_validator(&map);
    // Fewer key names than positions cannot be drafted.
    m.key_names = vec!["A".into()];
    assert!(draft_index_map(&l, &m).is_err());
}

#[test]
fn draft_id_in_the_node_value_reads_the_whole_node() {
    // F13: one subscript, the ID in the value.
    let mut l = f02_link();
    l.id = IdLink {
        form: IdForm::Value,
        positions: vec![],
    };
    let (map, _) = draft_index_map(&l, &master()).unwrap();
    assert_eq!(
        map,
        json!({
            "name": "X",
            "type": "index",
            "global": "^IadSm173F02X",
            "subscripts": [{"level": 1, "expression": "$$SQLUPPER({Name})"}],
            "data": {"Id": {"name": "Id"}},
            "row_id": [{"position": 1, "expression": "{Id}", "field": "Id"}],
        })
    );
    passes_the_validator(&map);
}

#[test]
fn draft_covering_index_has_a_data_entry_per_covered_field() {
    // F14: the node value is a $LIST of City (element 1) and Age (element 2).
    let mut l = f02_link();
    l.data_in_value = vec![
        DataInValue {
            field: 2,
            element: 1,
        },
        DataInValue {
            field: 3,
            element: 2,
        },
    ];
    let (map, _) = draft_index_map(&l, &master()).unwrap();
    assert_eq!(
        map["data"],
        json!({
            "City": {"name": "City", "piece": "1"},
            "Age": {"name": "Age", "piece": "2"},
        })
    );
    assert!(
        map.get("row_id").is_none(),
        "the ID is a subscript here: {map}"
    );
    passes_the_validator(&map);
    // A covered field named twice is refused, not merged.
    l.data_in_value.push(DataInValue {
        field: 2,
        element: 3,
    });
    assert!(draft_index_map(&l, &master()).is_err());
}

#[test]
fn draft_ambiguous_pick_is_a_guess() {
    // F26: two identical columns; the draft takes the best and says so.
    let mut l = f02_link();
    l.verdict = Verdict::Ambiguous;
    l.subscripts[0].verdict = Verdict::Ambiguous;
    l.subscripts[0].pick = Some(score(2, Transform::Exact, 40, 40));
    l.subscripts[0].alternatives = vec![
        score(2, Transform::Exact, 40, 40),
        score(3, Transform::Exact, 40, 40),
    ];
    let (map, guesses) = draft_index_map(&l, &master()).unwrap();
    assert_eq!(map["subscripts"][0]["expression"], "{City}");
    assert_eq!(guesses, vec!["/maps/X/subscripts/1".to_string()]);
    passes_the_validator(&map);
}

#[test]
fn draft_refuses_what_it_cannot_explain() {
    let m = master();
    // A position no form explained: no code is drafted (F33).
    let mut l = f02_link();
    l.subscripts[0].verdict = Verdict::NotExplained;
    l.subscripts[0].pick = None;
    assert!(draft_index_map(&l, &m).is_err());
    // A constant position has nothing to spell.
    let mut l = f02_link();
    l.subscripts[0].verdict = Verdict::Constant;
    assert!(draft_index_map(&l, &m).is_err());
    // A link that is not inferred or ambiguous is not drafted.
    for v in [
        Verdict::NotLinked,
        Verdict::TooFewNodes,
        Verdict::NotExplained,
    ] {
        let mut l = f02_link();
        l.verdict = v;
        assert!(draft_index_map(&l, &m).is_err(), "{v:?}");
    }
    // A gap: position 3 with nothing at 2.
    let mut l = f02_link();
    l.id.positions = vec![3];
    assert!(draft_index_map(&l, &m).is_err());
    // A field number the master does not have.
    let mut l = f02_link();
    l.subscripts[0].pick = Some(score(9, Transform::Exact, 40, 40));
    assert!(draft_index_map(&l, &m).is_err());
    // A position named twice.
    let mut l = f02_link();
    l.subscripts.push(l.subscripts[0].clone());
    assert!(draft_index_map(&l, &m).is_err());
    // A subscript form with no ID position; a value form with one.
    let mut l = f02_link();
    l.id.positions = vec![];
    assert!(draft_index_map(&l, &m).is_err());
    let mut l = f02_link();
    l.id.form = IdForm::Value;
    assert!(draft_index_map(&l, &m).is_err());
}

#[test]
fn draft_refuses_a_global_that_is_not_a_plain_name() {
    for bad in ["^X\") Kill ^Y", "^A(\"x\")", "^|\"OTHER\"|G", "^a b", ""] {
        let mut l = f02_link();
        l.global = bad.into();
        assert!(draft_index_map(&l, &master()).is_err(), "{bad:?}");
    }
}

#[test]
fn draft_refuses_a_field_name_the_master_cannot_spell() {
    let mut m = master();
    m.field_names[0] = "Na}me".into();
    assert!(draft_index_map(&f02_link(), &m).is_err());
}

#[test]
fn draft_holds_no_sampled_value() {
    // The link carries counts, positions and names, so the map can hold nothing else; the one
    // string the caller controls is the field name, and it is the master's, not a value.
    let (map, guesses) = draft_index_map(&f02_link(), &master()).unwrap();
    let text = format!("{map}{guesses:?}");
    for word in ["Smith", "Lyon", "Boston"] {
        assert!(!text.contains(word), "{word}");
    }
}

// ── null markers in the draft (T030) ───────────────────────────────────────

fn with_marker(kind: NullKind, literal: Option<&str>) -> Link {
    let mut link = f02_link();
    link.subscripts[0].null_marker = Some(NullMarker {
        kind,
        empty: EmptyKind::Null,
        count: 3,
        length: literal.map(|l| u32::try_from(l.len()).unwrap()),
        literal: literal.map(String::from),
    });
    link
}

#[test]
fn the_engines_own_markers_draft_no_null_marker() {
    for kind in [NullKind::C0, NullKind::Minus1e14] {
        let (map, guesses) = draft_index_map(&with_marker(kind, None), &master()).unwrap();
        assert!(map["subscripts"][0].get("null_marker").is_none(), "{map}");
        assert!(
            guesses.iter().all(|g| !g.contains("null_marker")),
            "{guesses:?}"
        );
    }
}

#[test]
fn a_space_marker_is_drafted_as_the_quoted_space() {
    let (map, _) = draft_index_map(&with_marker(NullKind::Space, None), &master()).unwrap();
    assert_eq!(map["subscripts"][0]["null_marker"], "\" \"");
    passes_the_validator(&map);
    let (map, _) = draft_index_map(&with_marker(NullKind::SpaceC0, None), &master()).unwrap();
    assert_eq!(map["subscripts"][0]["null_marker"], "$c(32,0)");
}

#[test]
fn a_discovered_marker_needs_its_literal_and_without_it_the_guess_names_it() {
    let view = master();
    let (map, guesses) = draft_index_map(&with_marker(NullKind::Discovered, None), &view).unwrap();
    assert!(map["subscripts"][0].get("null_marker").is_none(), "{map}");
    assert!(
        guesses
            .iter()
            .any(|g| g == "/maps/X/subscripts/1/null_marker"),
        "{guesses:?}"
    );
    let (map, guesses) =
        draft_index_map(&with_marker(NullKind::Discovered, Some("~N\"L~")), &view).unwrap();
    assert_eq!(map["subscripts"][0]["null_marker"], "\"~N\"\"L~\"");
    assert!(
        guesses.iter().all(|g| !g.contains("null_marker")),
        "{guesses:?}"
    );
}

#[test]
fn a_discovered_marker_with_a_control_character_is_not_drafted() {
    let (map, guesses) = draft_index_map(
        &with_marker(NullKind::Discovered, Some("a\u{1}b")),
        &master(),
    )
    .unwrap();
    assert!(map["subscripts"][0].get("null_marker").is_none(), "{map}");
    assert!(
        guesses.iter().any(|g| g.ends_with("/null_marker")),
        "{guesses:?}"
    );
}

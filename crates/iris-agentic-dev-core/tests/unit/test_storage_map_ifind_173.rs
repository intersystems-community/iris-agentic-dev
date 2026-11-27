//! Spec 173 T076 to T083: what `propose`, `compare` and `diagram` learned from a real `%iFind` index
//! and from Benjamin's `^bdb` shape. The live answers are in `test_storage_map_ifind_173_live.rs`;
//! these are the pure halves: the names a compiler files beside a master, the node a field reads,
//! what the sampler's `bits` format does to a draft, and the label of a computed level.

use iris_agentic_dev_core::tools::storage_map::candidates::{
    from_stem_siblings, listing_code, none_found_text, sibling_stem, FoundBy,
};
use iris_agentic_dev_core::tools::storage_map::diagram::graph;
use iris_agentic_dev_core::tools::storage_map::link_code::{build_code, field_access, Caps};
use iris_agentic_dev_core::tools::storage_map::link_code::{FieldAccess, LinkRequest};
use iris_agentic_dev_core::tools::storage_map::master::MasterField;
use iris_agentic_dev_core::tools::storage_map::model::from_tree;
use iris_agentic_dev_core::tools::storage_map::sample::parse;
use iris_agentic_dev_core::tools::storage_map::shape::{bit_string_note, draft};
use iris_agentic_dev_core::tools::storage_map::tree::{parse_body, split_class};

fn names(v: &[&str]) -> Vec<String> {
    v.iter().map(|s| s.to_string()).collect()
}

#[test]
fn a_default_storage_master_and_a_ddl_master_have_a_stem() {
    assert_eq!(
        sibling_stem("^IadSm173.Ifind.DocD"),
        Some("IadSm173.Ifind.Doc")
    );
    assert_eq!(
        sibling_stem("IadSm173.Ifind.DocD"),
        Some("IadSm173.Ifind.Doc")
    );
    assert_eq!(sibling_stem("^Cd34.E0li.1"), Some("Cd34.E0li"));
    // Neither shape: no stem, so nothing beside it is offered.
    for plain in [
        "^IadSm173A",
        "^IadSm173Person.x",
        "^Foo.D",
        "^AD",
        "^ID",
        "^A.1",
    ] {
        assert_eq!(sibling_stem(plain), None, "{plain}");
    }
}

#[test]
fn the_class_index_global_is_offered_beside_its_d_master_and_nothing_else_is() {
    let listed = names(&[
        "IadSm173.Ifind.DocD",
        "IadSm173.Ifind.DocI",
        "IadSm173.Ifind.DocS",
        "IadSm173.Ifind.Doc",
        "IadSm173.Ifind.DocIx",
        "IadSm173.Ifind.Other",
    ]);
    let got = from_stem_siblings("^IadSm173.Ifind.DocD", &listed);
    let globals: Vec<&str> = got.iter().map(|c| c.global.as_str()).collect();
    assert_eq!(globals, ["^IadSm173.Ifind.DocI"]);
    assert!(got.iter().all(|c| c.found_by == FoundBy::StemSibling));
}

#[test]
fn a_ddl_masters_other_globals_are_offered_beside_its_dot_one() {
    let listed = names(&[
        "Cd34.E0li.1",
        "Cd34.E0li.2",
        "Cd34.E0li.3",
        "Cd34.E0li.I",
        "Cd34.E0li.S",
        "Cd34.E0li.x",
        "Cd34.E0liX.3",
    ]);
    let got = from_stem_siblings("^Cd34.E0li.1", &listed);
    let globals: Vec<&str> = got.iter().map(|c| c.global.as_str()).collect();
    assert_eq!(globals, ["^Cd34.E0li.2", "^Cd34.E0li.3", "^Cd34.E0li.I"]);
}

#[test]
fn a_master_with_no_stem_offers_no_sibling() {
    let listed = names(&["IadSm173A", "IadSm173AI", "IadSm173A.x"]);
    assert!(from_stem_siblings("^IadSm173A", &listed).is_empty());
}

#[test]
fn the_listing_asks_for_the_stem_so_the_sibling_is_in_it() {
    let code = listing_code("^IadSm173.Ifind.DocD").unwrap();
    assert!(code.contains("\"IadSm173.Ifind.Doc*\""), "{code}");
    // A master with no stem keeps the mask it had.
    assert!(listing_code("^IadSm173A")
        .unwrap()
        .contains("\"IadSm173A*\""));
}

#[test]
fn the_none_found_text_says_where_it_looked() {
    let t = none_found_text("^IadSm173.Ifind.DocD", "USER", true);
    for want in [
        "^IadSm173.Ifind.DocD",
        "USER",
        "^IadSm173.Ifind.DocI",
        "sub-trees inside the master",
        "index_globals",
    ] {
        assert!(t.contains(want), "{want}: {t}");
    }
    let t = none_found_text("^IadSm173A", "USER", false);
    assert!(!t.contains("sub-trees"), "{t}");
    assert!(!t.contains("compiler files beside"), "{t}");
}

fn field(node: Option<&str>) -> MasterField {
    MasterField {
        name: "Sub1".into(),
        placeholder: "Sub1".into(),
        node: node.map(String::from),
        delimiter: Some("\"^\"".into()),
        piece: Some("1".into()),
        format: "delimited".into(),
        guessed: true,
    }
}

#[test]
fn a_field_reads_the_node_its_literal_names_not_the_literal_with_its_quotes() {
    let got = |n: &str| field_access(&field(Some(n))).unwrap().node;
    assert_eq!(got("\"sub\"").as_deref(), Some("sub"));
    assert_eq!(got("\"a\"\"b\"").as_deref(), Some("a\"b"));
    assert_eq!(got("2").as_deref(), Some("2"));
    assert_eq!(field_access(&field(None)).unwrap().node, None);
}

#[test]
fn the_linkers_field_table_subscripts_by_the_unquoted_node() {
    let fields: Vec<FieldAccess> = vec![field_access(&field(Some("\"sub\""))).unwrap()];
    let code = build_code(&LinkRequest {
        master: "^IadSm173Bdb",
        candidate: "^IadSm173Bdb.x",
        sub_prefix: &[],
        key_parts: 1,
        fields: &fields,
        caps: Caps::for_node_cap(200),
        with_values: false,
    })
    .unwrap();
    assert!(
        code.contains("Set fa(1)=$ListBuild(\"sub\",94,1)"),
        "{code}"
    );
    assert!(!code.contains("\"\"\"sub\"\"\""), "{code}");
}

#[test]
fn a_constant_position_is_never_ranked_as_the_row_id_by_the_linker_code() {
    let fields: Vec<FieldAccess> = vec![field_access(&field(None)).unwrap()];
    let code = build_code(&LinkRequest {
        master: "^M",
        candidate: "^M.x",
        sub_prefix: &[],
        key_parts: 1,
        fields: &fields,
        caps: Caps::for_node_cap(200),
        with_values: false,
    })
    .unwrap();
    // The ways are scored by rows found and then distinct IDs, and a way that reads one value
    // from three or more nodes is not written.
    assert!(code.contains("ws(st)=res*10000+dn"), "{code}");
    assert!(code.contains("(tot>=3)&&(dn=1)"), "{code}");
    assert!(code.contains("Set o.resolved=way(bs)"), "{code}");
}

/// A sampler's lines for a global whose only values are bit strings, at depth 2.
const BITS_ONLY: &str = r#"{"d":1,"s":10,"n":"Idx"}
{"d":2,"s":1,"f":"bits","p":1}
{"d":2,"s":1,"f":"bits","p":1}
{"done":1,"c":3,"capped":0}
"#;

#[test]
fn a_bit_string_is_named_and_no_field_is_drafted_from_it() {
    let mut report = parse(BITS_ONLY).unwrap();
    report.global = "^IadSm173.Ifind.DocI".into();
    assert!(report.formats.iter().any(|f| f.format == "bits"));
    let (map, guesses) = draft(&report);
    assert_eq!(map["data"], serde_json::json!({}), "{map}");
    assert!(guesses.iter().all(|g| !g.contains("/data/")), "{guesses:?}");
    let note = bit_string_note(&report).expect("a note");
    assert!(
        note.contains("depth 2") && note.contains("bit strings"),
        "{note}"
    );
}

#[test]
fn text_with_a_delimiter_still_drafts_its_fields_and_gets_no_bit_note() {
    let lines = "{\"d\":1,\"s\":1,\"f\":\"delimited\",\"dl\":94,\"p\":3}\n{\"done\":1,\"c\":1,\"capped\":0}\n";
    let mut report = parse(lines).unwrap();
    report.global = "^G".into();
    let (map, _) = draft(&report);
    assert_eq!(map["data"].as_object().map(|d| d.len()), Some(3), "{map}");
    assert_eq!(bit_string_note(&report), None);
}

fn bits_class_graph() -> iris_agentic_dev_core::tools::storage_map::diagram::Graph {
    let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/storage172/classes/Bits.cls");
    let text = std::fs::read_to_string(path).unwrap();
    let span = split_class(&text).unwrap().remove(0);
    let nodes = parse_body(&text[span.body_start..span.body_end]).unwrap();
    graph(&from_tree("IadSm172.Bits", &span.name, &nodes), None)
}

#[test]
fn the_diagram_node_of_a_computed_level_says_it_is_computed() {
    let g = bits_class_graph();
    let label = |prefix: &str| {
        g.nodes
            .iter()
            .map(|n| n.label.as_str())
            .find(|l| l.starts_with(prefix))
            .unwrap_or_else(|| panic!("no {prefix} in {:?}", g.nodes))
            .to_string()
    };
    assert!(
        label("sub 3").contains("computed by code"),
        "{}",
        label("sub 3")
    );
    // A plain level is drawn as it was.
    assert_eq!(label("sub 1"), "sub 1: 0 (Sub)");
}

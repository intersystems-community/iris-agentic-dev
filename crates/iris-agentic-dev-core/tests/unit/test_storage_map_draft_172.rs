//! T110: `shape::draft` turns a shape report into a map in model JSON, with every element it had
//! to guess listed, so it can go to `check` or `replace` once the names are fixed.

use iris_agentic_dev_core::tools::storage_map::model::map_node;
use iris_agentic_dev_core::tools::storage_map::sample::parse;
use iris_agentic_dev_core::tools::storage_map::shape::draft;

/// The sampler's lines for `Member`'s seed: the root holds the counter 3; rows 1-3 hold
/// name^city^age.
const MEMBER: &str = r#"{"d":0,"s":11,"f":"plain","p":1}
{"d":1,"s":1,"f":"delimited","dl":94,"p":3}
{"d":1,"s":1,"f":"delimited","dl":94,"p":3}
{"d":1,"s":1,"f":"delimited","dl":94,"p":3}
{"done":1,"c":4,"capped":0}
"#;

#[test]
fn test_storage_map_draft_172_three_pieces_on_the_id_node() {
    let mut report = parse(MEMBER).unwrap();
    report.global = "^IadSm172Member".into();
    let (map, guesses) = draft(&report);
    assert_eq!(map["global"], "^IadSm172Member");
    assert_eq!(map["type"], "data");
    assert_eq!(map["subscripts"][0]["expression"], "{ID}");
    for (i, name) in ["Field1", "Field2", "Field3"].iter().enumerate() {
        let f = &map["data"][name];
        assert_eq!(f["piece"], (i + 1).to_string(), "{map}");
        assert_eq!(f["delimiter"], "\"^\"", "{map}");
        assert!(f["node"].is_null(), "{map}");
    }
    assert!(
        guesses.iter().any(|g| g == "/maps/Master/data/Field1"),
        "{guesses:?}"
    );
    // The draft is a model iad accepts as it is.
    map_node(&map).expect("the draft is valid model JSON");
}

#[test]
fn test_storage_map_draft_172_lists_and_sub_nodes() {
    let lines = r#"{"d":1,"s":11,"f":"$LIST","p":2}
{"d":2,"s":1,"n":"contact","f":"delimited","dl":94,"p":2}
{"d":1,"s":11,"f":"$LIST","p":3}
{"d":2,"s":1,"n":"contact","f":"delimited","dl":94,"p":2}
{"done":1,"c":4,"capped":0}
"#;
    let mut report = parse(lines).unwrap();
    report.global = "^X".into();
    let (map, _) = draft(&report);
    let f = &map["data"]["Field3"];
    assert!(
        f["delimiter"].is_null(),
        "a $LIST field has no delimiter: {map}"
    );
    assert_eq!(f["piece"], "3", "{map}");
    let c = &map["data"]["Contact2"];
    assert_eq!(c["node"], "\"contact\"", "{map}");
    assert_eq!(c["piece"], "2", "{map}");
    map_node(&map).expect("valid model JSON");
}

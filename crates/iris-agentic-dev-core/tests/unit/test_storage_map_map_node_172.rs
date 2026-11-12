//! T065 prerequisite: `model::map_node` turns one SqlMap in model JSON (what `read` returns and
//! what `check`/`preview`/`replace` take as `model`) back into its Storage element, so a
//! caller's map can be compiled and written.

use iris_agentic_dev_core::tools::storage_map::model::{from_tree, map_node};
use iris_agentic_dev_core::tools::storage_map::tree::{
    emit_body, normalize, parse_body, split_class, Node,
};
use serde_json::json;

fn fixture_texts() -> Vec<(String, String)> {
    let dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/storage172/classes");
    let mut out: Vec<(String, String)> = std::fs::read_dir(&dir)
        .unwrap()
        .map(|e| {
            let p = e.unwrap().path();
            (
                p.file_stem().unwrap().to_string_lossy().to_string(),
                std::fs::read_to_string(&p).unwrap(),
            )
        })
        .collect();
    out.sort();
    out
}

fn storage_nodes(text: &str) -> (String, Vec<Node>) {
    let span = split_class(text).unwrap().remove(0);
    (
        span.name.clone(),
        parse_body(&text[span.body_start..span.body_end]).unwrap(),
    )
}

fn canonical(n: &Node) -> String {
    emit_body(&normalize(vec![n.clone()]))
}

#[test]
fn test_storage_map_map_node_172_round_trips_every_fixture_map() {
    let mut checked = 0;
    for (name, text) in fixture_texts() {
        let (storage, nodes) = storage_nodes(&text);
        let model = from_tree(&name, &storage, &nodes);
        let originals: Vec<&Node> = nodes.iter().filter(|n| n.tag == "SQLMap").collect();
        assert_eq!(originals.len(), model.storage.maps.len(), "{name}");
        for (orig, map) in originals.iter().zip(&model.storage.maps) {
            let as_json = serde_json::to_value(map).unwrap();
            let built = map_node(&as_json).unwrap_or_else(|e| panic!("{name}/{}: {e}", map.name));
            assert_eq!(canonical(&built), canonical(orig), "{name}/{}", map.name);
            checked += 1;
        }
    }
    assert!(checked >= 16, "only {checked} maps checked");
}

/// `Accessvar` and `Invalidcondition` are child elements in IRIS (`Code`/`Variable`,
/// `Expression`; `%Dictionary.StorageSQLMapSub*Definition`), and survive the round trip.
#[test]
fn test_storage_map_map_node_172_access_vars_and_invalid_conditions() {
    let body = "<SQLMap name=\"M\">\n<Global>^G</Global>\n<Subscript name=\"1\">\n\
                <Accessvar name=\"1\">\n<Code> s {1D1}=$g(^G({L1}))</Code>\n<Variable>{1D1}</Variable>\n</Accessvar>\n\
                <Expression>{ID}</Expression>\n\
                <Invalidcondition name=\"1\">\n<Expression>{1D1}=\"\"</Expression>\n</Invalidcondition>\n\
                </Subscript>\n<Type>data</Type>\n</SQLMap>";
    let nodes = parse_body(body).unwrap();
    let model = from_tree("X.Y", "S", &nodes);
    let sub = &model.storage.maps[0].subscripts[0];
    assert_eq!(
        sub.invalid_conditions[0].expression, "{1D1}=\"\"",
        "read lost the expression"
    );
    assert_eq!(sub.access_vars[0].variable, "{1D1}");
    let built = map_node(&serde_json::to_value(&model.storage.maps[0]).unwrap()).unwrap();
    assert_eq!(canonical(&built), canonical(&nodes[0]));
}

#[test]
fn test_storage_map_map_node_172_refuses_what_it_cannot_write() {
    // An unknown key would be dropped without a word (the #110 pattern), so it is refused.
    let err = map_node(&json!({"name": "M", "type": "data", "global": "^G", "wibble": 1}))
        .expect_err("unknown key");
    assert!(err.contains("wibble"), "{err}");
    let err = map_node(&json!({"name": "M", "subscripts": [{"level": 1, "expresion": "{ID}"}]}))
        .expect_err("misspelt subscript key");
    assert!(err.contains("expresion"), "{err}");
    assert!(map_node(&json!({"type": "data"})).is_err(), "no name");
    assert!(map_node(&json!("Master")).is_err(), "not an object");
    assert!(
        map_node(&json!({"name": "M", "global": 5})).is_err(),
        "a number where text goes"
    );
}

#[test]
fn test_storage_map_map_node_172_writes_access_type_only_when_not_sub() {
    let base = json!({"name": "M", "type": "data", "global": "^G",
                      "subscripts": [{"level": 1, "expression": "{ID}", "access_type": "Sub"}]});
    let text = emit_body(&[map_node(&base).unwrap()]);
    assert!(!text.contains("AccessType"), "{text}");
    let mut piece = base.clone();
    piece["subscripts"][0]["access_type"] = json!("Piece");
    let text = emit_body(&[map_node(&piece).unwrap()]);
    assert!(text.contains("<AccessType>Piece</AccessType>"), "{text}");
}

//! T067–T069: paths name one element of a Storage tree (data-model.md "Paths"), and `edit`'s
//! operations apply to a copy of the tree, all or nothing (data-model.md "Path operations").

use iris_agentic_dev_core::tools::storage_map::path::{
    apply, get, leaf_paths, parse_path, PathSeg,
};
use iris_agentic_dev_core::tools::storage_map::tree::{emit_body, parse_body, split_class, Node};
use serde_json::{json, Value};

fn fixture_text(name: &str) -> String {
    let p = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/storage172/classes")
        .join(format!("{name}.cls"));
    std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("{}: {e}", p.display()))
}

fn nodes_of(text: &str) -> Vec<Node> {
    let span = split_class(text).unwrap().remove(0);
    parse_body(&text[span.body_start..span.body_end]).unwrap()
}

fn member() -> Vec<Node> {
    nodes_of(&fixture_text("Member"))
}

fn find<'a>(nodes: &'a [Node], tag: &str, name: Option<&str>) -> Option<&'a Node> {
    nodes
        .iter()
        .find(|n| n.tag == tag && n.name.as_deref() == name)
}

fn master(nodes: &[Node]) -> &Node {
    find(nodes, "SQLMap", Some("Master")).expect("Master")
}

fn kids(n: &Node) -> &[Node] {
    n.children.as_deref().unwrap_or(&[])
}

fn leaf<'a>(n: &'a Node, tag: &str) -> Option<&'a str> {
    find(kids(n), tag, None).and_then(|l| l.value.as_deref())
}

fn elem(tag: &str, name: &str) -> PathSeg {
    PathSeg::Elem {
        tag: tag.into(),
        name: name.into(),
    }
}

// ── T067: parse_path ──────────────────────────────────────────────────────────

#[test]
fn test_storage_map_path_172_documented_paths_parse() {
    use PathSeg::{Extra, Leaf};
    let cases: Vec<(&str, Vec<PathSeg>)> = vec![
        (
            "/keywords/SqlIdExpression",
            vec![Leaf("SqlIdExpression".into())],
        ),
        ("/maps/Master", vec![elem("SQLMap", "Master")]),
        (
            "/maps/Master/global",
            vec![elem("SQLMap", "Master"), Leaf("Global".into())],
        ),
        (
            "/maps/Master/type",
            vec![elem("SQLMap", "Master"), Leaf("Type".into())],
        ),
        (
            "/maps/Master/subscripts/2/expression",
            vec![
                elem("SQLMap", "Master"),
                elem("Subscript", "2"),
                Leaf("Expression".into()),
            ],
        ),
        (
            "/maps/Master/subscripts/2/access_vars/1/code",
            vec![
                elem("SQLMap", "Master"),
                elem("Subscript", "2"),
                elem("Accessvar", "1"),
                Leaf("Code".into()),
            ],
        ),
        (
            "/maps/Master/subscripts/1/invalid_conditions/1/expression",
            vec![
                elem("SQLMap", "Master"),
                elem("Subscript", "1"),
                elem("Invalidcondition", "1"),
                Leaf("Expression".into()),
            ],
        ),
        (
            "/maps/Master/data/City/piece",
            vec![
                elem("SQLMap", "Master"),
                elem("Data", "City"),
                Leaf("Piece".into()),
            ],
        ),
        (
            "/maps/Master/row_id/1/field",
            vec![
                elem("SQLMap", "Master"),
                elem("RowIdSpec", "1"),
                Leaf("Field".into()),
            ],
        ),
        (
            "/maps/Master/extra/Wibble",
            vec![elem("SQLMap", "Master"), Extra("Wibble".into())],
        ),
        (
            "/maps/Master/subscripts/1/access_type",
            vec![
                elem("SQLMap", "Master"),
                elem("Subscript", "1"),
                Leaf("AccessType".into()),
            ],
        ),
    ];
    for (p, want) in cases {
        assert_eq!(parse_path(p), Ok(want), "{p}");
    }
}

#[test]
fn test_storage_map_path_172_bad_paths_are_refused() {
    for p in [
        "",
        "/",
        "maps/Master",
        "/maps",
        "/maps/",
        "/maps//x",
        "/maps/Master/subscripts/a/x",
        "/maps/Master/subscripts/0/expression",
        "/maps/Master/subscripts/01/expression",
        "/maps/Master/bogus",
        "/maps/Master/data/City/bogus",
        "/maps/Master/data/City/piece/more",
        "/maps/Master/extra",
        "/keywords",
        "/keywords/SQLMap",
        "/keywords/Wibble",
        "/storage/maps/Master",
    ] {
        assert!(parse_path(p).is_err(), "{p:?} parsed");
    }
}

// ── T068: apply ───────────────────────────────────────────────────────────────

fn ops(v: Value) -> Vec<Value> {
    v.as_array().unwrap().clone()
}

#[test]
fn test_storage_map_path_172_set_changes_a_leaf_and_null_removes_it() {
    let mut t = member();
    let changed = apply(
        &mut t,
        &ops(json!([{"op": "set", "path": "/maps/Master/data/Age/piece", "value": "4"}])),
    )
    .unwrap();
    assert_eq!(changed, vec!["/maps/Master/data/Age/piece"]);
    let age = find(kids(master(&t)), "Data", Some("Age")).unwrap();
    assert_eq!(leaf(age, "Piece"), Some("4"));

    apply(
        &mut t,
        &ops(json!([{"op": "set", "path": "/maps/Master/data/Age/delimiter", "value": null}])),
    )
    .unwrap();
    let age = find(kids(master(&t)), "Data", Some("Age")).unwrap();
    assert_eq!(leaf(age, "Delimiter"), None);

    // A leaf that is not there yet is created.
    apply(
        &mut t,
        &ops(json!([{"op": "set", "path": "/keywords/ExtentSize", "value": "3"}])),
    )
    .unwrap();
    assert_eq!(
        find(&t, "ExtentSize", None).and_then(|n| n.value.as_deref()),
        Some("3")
    );
    // Only text goes in a leaf.
    assert!(apply(
        &mut t,
        &ops(json!([{"op": "set", "path": "/maps/Master/global", "value": 5}]))
    )
    .is_err());
}

#[test]
fn test_storage_map_path_172_add_creates_and_refuses_an_existing_element() {
    let mut t = member();
    let add = json!([{"op": "add", "path": "/maps/Master/data/Email",
                      "value": {"node": "\"contact\"", "delimiter": "\"^\"", "piece": "1"}}]);
    let changed = apply(&mut t, &ops(add.clone())).unwrap();
    assert_eq!(changed, vec!["/maps/Master/data/Email"]);
    let email = find(kids(master(&t)), "Data", Some("Email")).expect("Email");
    assert_eq!(leaf(email, "Piece"), Some("1"));
    assert_eq!(leaf(email, "Node"), Some("\"contact\""));
    assert!(apply(&mut t, &ops(add)).is_err(), "added twice");

    let sub =
        json!([{"op": "add", "path": "/maps/Master/subscripts/2", "value": {"expression": "1"}}]);
    apply(&mut t, &ops(sub)).unwrap();
    let s2 = find(kids(master(&t)), "Subscript", Some("2")).expect("subscript 2");
    assert_eq!(leaf(s2, "Expression"), Some("1"));

    let map = json!([{"op": "add", "path": "/maps/CityIdx",
                      "value": {"type": "index", "global": "^IadSm172MemberI",
                                "subscripts": [{"level": 1, "expression": "{City}"}]}}]);
    apply(&mut t, &ops(map)).unwrap();
    assert!(find(&t, "SQLMap", Some("CityIdx")).is_some());
    // A fragment that names itself differently from its path is refused.
    let wrong = json!([{"op": "add", "path": "/maps/Other", "value": {"name": "Else"}}]);
    assert!(apply(&mut t, &ops(wrong)).is_err());
}

#[test]
fn test_storage_map_path_172_remove_and_move() {
    let mut t = member();
    apply(
        &mut t,
        &ops(json!([{"op": "remove", "path": "/maps/Master/data/City"}])),
    )
    .unwrap();
    assert!(find(kids(master(&t)), "Data", Some("City")).is_none());
    assert!(
        apply(
            &mut t,
            &ops(json!([{"op": "remove", "path": "/maps/Master/data/City"}]))
        )
        .is_err(),
        "removing what is not there"
    );

    let changed = apply(
        &mut t,
        &ops(json!([{"op": "move", "path": "/maps/Master/subscripts/1",
                     "to": "/maps/Master/subscripts/2"}])),
    )
    .unwrap();
    assert_eq!(
        changed,
        vec!["/maps/Master/subscripts/1", "/maps/Master/subscripts/2"]
    );
    assert!(find(kids(master(&t)), "Subscript", Some("1")).is_none());
    assert!(find(kids(master(&t)), "Subscript", Some("2")).is_some());

    for bad in [
        json!({"op": "move", "path": "/maps/Master/subscripts/2", "to": "/maps/Other/subscripts/3"}),
        json!({"op": "move", "path": "/maps/Master/data/Age", "to": "/maps/Master/data/Years"}),
        json!({"op": "move", "path": "/maps/Master/subscripts/2", "to": "/maps/Master/row_id/3"}),
        json!({"op": "move", "path": "/maps/Master/subscripts/9", "to": "/maps/Master/subscripts/3"}),
        json!({"op": "move", "path": "/maps/Master/subscripts/2"}),
        json!({"op": "copy", "path": "/maps/Master/subscripts/2"}),
    ] {
        assert!(apply(&mut t, std::slice::from_ref(&bad)).is_err(), "{bad}");
    }
}

#[test]
fn test_storage_map_path_172_a_failing_op_leaves_the_tree_unchanged() {
    let mut t = member();
    let before = t.clone();
    let err = apply(
        &mut t,
        &ops(json!([
            {"op": "set", "path": "/maps/Master/data/Age/piece", "value": "4"},
            {"op": "remove", "path": "/maps/Master/data/City"},
            {"op": "remove", "path": "/maps/Master/data/Nobody"}
        ])),
    );
    assert!(err.is_err());
    assert_eq!(t, before, "a failed edit changed the tree");
}

#[test]
fn test_storage_map_path_172_extra_can_only_be_removed() {
    let text = fixture_text("Member").replacen(
        "<Global>^IadSm172Member</Global>",
        "<Global>^IadSm172Member</Global>\n<Wibble>x</Wibble>",
        1,
    );
    let mut t = nodes_of(&text);
    assert!(apply(
        &mut t,
        &ops(json!([{"op": "set", "path": "/maps/Master/extra/Wibble", "value": "y"}]))
    )
    .is_err());
    apply(
        &mut t,
        &ops(json!([{"op": "remove", "path": "/maps/Master/extra/Wibble"}])),
    )
    .unwrap();
    assert!(find(kids(master(&t)), "Wibble", None).is_none());
    // A known element is not `extra`.
    assert!(apply(
        &mut t,
        &ops(json!([{"op": "remove", "path": "/maps/Master/extra/Global"}]))
    )
    .is_err());
}

// ── T069: a one-field edit changes one element ───────────────────────────────

#[test]
fn test_storage_map_path_172_single_field_edit_changes_one_line() {
    let dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/storage172/classes");
    let mut checked = 0;
    for entry in std::fs::read_dir(&dir).unwrap() {
        let path = entry.unwrap().path();
        let text = std::fs::read_to_string(&path).unwrap();
        let original = nodes_of(&text);
        let before = emit_body(&original);
        let before_lines: Vec<&str> = before.lines().collect();
        for p in leaf_paths(&original) {
            // The new value is the old one with a prefix: it keeps the old value's lines and its need
            // for CDATA, so the edit changes only the first line of the value.
            let old = get(&original, &p)
                .and_then(|n| n.value.as_deref())
                .unwrap_or_else(|| panic!("{}: {p}: get found nothing", path.display()));
            let new = format!("Z9edit{old}");
            let mut t = original.clone();
            apply(&mut t, &[json!({"op": "set", "path": p, "value": new})])
                .unwrap_or_else(|e| panic!("{}: {p}: {e}", path.display()));
            let after = emit_body(&t);
            let after_lines: Vec<&str> = after.lines().collect();
            assert_eq!(
                before_lines.len(),
                after_lines.len(),
                "{}: {p}: the edit added or removed lines",
                path.display()
            );
            let differing: Vec<usize> = (0..before_lines.len())
                .filter(|&i| before_lines[i] != after_lines[i])
                .collect();
            assert_eq!(
                differing.len(),
                1,
                "{}: {p}: lines {differing:?}",
                path.display()
            );
            assert!(
                after_lines[differing[0]].contains("Z9edit"),
                "{}: {p}",
                path.display()
            );
            checked += 1;
        }
    }
    assert!(checked > 150, "only {checked} leaf edits checked");

    // A value that needs CDATA wraps only that element.
    let mut t = member();
    apply(
        &mut t,
        &[
            json!({"op": "set", "path": "/maps/Master/data/Age/retrieval_code",
                 "value": "Set {*}=$S({L1}>3:1,1:0)"}),
        ],
    )
    .unwrap();
    let text = emit_body(&t);
    assert!(
        text.contains("<RetrievalCode>\n<![CDATA[Set {*}=$S({L1}>3:1,1:0)]]></RetrievalCode>"),
        "{text}"
    );
}

//! Spec 173 T044: the shape report in an answer carries structure, not data. A literal subscript
//! that names a sub-node (`"title"` under every row) is structure; the keys of an index held
//! inside the master (`^M("idx","SMITH, ANNA",7)`) are values the sampler read, and without
//! `include_values` they stay in IRIS. The sampler's own report keeps them: `write` and
//! `candidates` read them inside iad, so only the copy that goes into an answer is cut.

use iris_agentic_dev_core::tools::storage_map::sample::parse;
use serde_json::{json, Value};

/// The sampler's lines for a master with `rows` rows of `title`/`pub`/`loc` sub-nodes under each.
fn structure_lines(rows: u32) -> String {
    let mut out = String::new();
    for _ in 1..=rows {
        // The row node: a numeric ID with children only.
        out.push_str("{\"d\":1,\"s\":10}\n");
        for name in ["loc", "pub", "title"] {
            out.push_str(&format!(
                "{{\"d\":2,\"s\":1,\"n\":\"{name}\",\"f\":\"plain\",\"p\":1}}\n"
            ));
        }
    }
    out
}

/// The sampler's lines for a master whose first subscript `idx` holds an index keyed by `keys`.
fn index_lines(keys: &[&str]) -> String {
    let mut out = String::from("{\"d\":1,\"s\":10,\"n\":\"idx\"}\n");
    for (i, key) in keys.iter().enumerate() {
        out.push_str(&format!("{{\"d\":2,\"s\":10,\"n\":\"{key}\"}}\n"));
        out.push_str(&format!(
            "{{\"d\":3,\"s\":1,\"f\":\"plain\",\"p\":0,\"i\":{i}}}\n"
        ));
    }
    out
}

fn done() -> &'static str {
    "{\"done\":1,\"c\":9,\"capped\":0}\n"
}

fn answer_shape(lines: &str, include_values: bool) -> Value {
    let report = parse(&format!("{lines}{}", done())).expect("a readable report");
    serde_json::to_value(report.for_answer(include_values)).unwrap()
}

#[test]
fn index_keys_below_the_first_subscript_stay_out_of_the_answer() {
    let keys = [
        "ZQ-SEED-0001",
        "ZQ-SEED-0002",
        "ZQ-SEED-0003",
        "ZQ-SEED-0004",
    ];
    let lines = index_lines(&keys);
    // The sampler's own report reads them: the answer's copy does not.
    let report = parse(&format!("{lines}{}", done())).unwrap();
    assert_eq!(report.sub_nodes["2"].len(), 4, "the report keeps the keys");
    let shape = answer_shape(&lines, false);
    let text = shape.to_string();
    for key in keys {
        assert!(!text.contains(key), "{key} reached the answer: {text}");
    }
    // The depth is still said, with how many names were held back, so the shape stays honest.
    assert_eq!(shape["withheld_sub_nodes"], json!({"2": 4}), "{shape}");
    // The first subscript is structure (an index's own name), and it stays.
    assert_eq!(shape["sub_nodes"]["1"], json!(["idx"]), "{shape}");
}

#[test]
fn with_include_values_the_keys_are_the_answers_to_give() {
    let keys = ["ZQ-SEED-0001", "ZQ-SEED-0002", "ZQ-SEED-0003"];
    let shape = answer_shape(&index_lines(&keys), true);
    assert_eq!(
        shape["sub_nodes"]["2"].as_array().unwrap().len(),
        3,
        "{shape}"
    );
    assert!(shape.get("withheld_sub_nodes").is_none(), "{shape}");
}

#[test]
fn names_that_repeat_under_every_row_are_structure_and_stay() {
    let lines = structure_lines(3);
    let shape = answer_shape(&lines, false);
    assert_eq!(
        shape["sub_nodes"]["2"],
        json!(["loc", "pub", "title"]),
        "{shape}"
    );
    assert!(shape.get("withheld_sub_nodes").is_none(), "{shape}");
    let formats = shape["formats"].as_array().unwrap();
    let nodes: Vec<&str> = formats.iter().filter_map(|f| f["node"].as_str()).collect();
    assert_eq!(nodes, vec!["\"loc\"", "\"pub\"", "\"title\""], "{shape}");
}

#[test]
fn a_depth_with_fewer_than_three_keys_is_not_judged() {
    // Two once-only names cannot be told from two structure names in one row: left as reported.
    let shape = answer_shape(&index_lines(&["ZQ-SEED-0001", "ZQ-SEED-0002"]), false);
    assert_eq!(
        shape["sub_nodes"]["2"].as_array().unwrap().len(),
        2,
        "{shape}"
    );
}

#[test]
fn value_nodes_named_by_data_keys_lose_their_names_too() {
    // `^M("idx","KEY")="x"`: a value node at depth 2 whose name is the key.
    let mut lines = String::from("{\"d\":1,\"s\":10,\"n\":\"idx\"}\n");
    for key in ["ZQ-SEED-0001", "ZQ-SEED-0002", "ZQ-SEED-0003"] {
        lines.push_str(&format!(
            "{{\"d\":2,\"s\":1,\"n\":\"{key}\",\"f\":\"plain\",\"p\":1}}\n"
        ));
    }
    let shape = answer_shape(&lines, false);
    let text = shape.to_string();
    assert!(!text.contains("ZQ-SEED"), "{text}");
    // One format at depth 2 now, with no node name, counting all three values.
    let depth2: Vec<&Value> = shape["formats"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|f| f["depth"] == 2)
        .collect();
    assert_eq!(depth2.len(), 1, "{shape}");
    assert!(depth2[0]["node"].is_null(), "{shape}");
    assert_eq!(shape["depths"]["2"], 3, "{shape}");
}

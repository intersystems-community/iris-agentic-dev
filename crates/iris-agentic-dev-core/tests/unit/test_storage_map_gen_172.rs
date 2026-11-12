//! T102–T104: the generator's dimensions, its covering array and its class writer.

use crate::storage172_gen::class::{class_for, NodeValue};
use crate::storage172_gen::pairwise::pairwise;
use crate::storage172_gen::{dims, Dim, Pair};
use std::collections::BTreeSet;

#[test]
fn test_storage_map_gen_172_every_dimension_has_two_values() {
    let d = dims();
    assert!(d.len() >= 6, "{d:?}");
    for dim in &d {
        assert!(dim.values.len() >= 2, "{} has one value", dim.name);
    }
}

fn three_by_three() -> Vec<Dim> {
    ["a", "b", "c"]
        .iter()
        .map(|n| Dim {
            name: n.to_string(),
            values: vec!["0".into(), "1".into(), "2".into()],
        })
        .collect()
}

/// Every allowed pair, checked by brute force over the output.
fn covered(dims: &[Dim], rows: &[Vec<usize>]) -> BTreeSet<(usize, usize, usize, usize)> {
    let mut out = BTreeSet::new();
    for r in rows {
        for i in 0..dims.len() {
            for j in i + 1..dims.len() {
                out.insert((i, r[i], j, r[j]));
            }
        }
    }
    out
}

#[test]
fn test_storage_map_gen_172_pairwise_covers_every_allowed_pair() {
    let d = dims();
    let forbidden = vec![Pair {
        a: ("delimiter".into(), "list".into()),
        b: ("computed".into(), "level1".into()),
    }];
    let rows = pairwise(&d, &forbidden, 7);
    let have = covered(&d, &rows);
    let di = |n: &str| d.iter().position(|x| x.name == n).unwrap();
    let (dl, cp) = (di("delimiter"), di("computed"));
    let list = d[dl].values.iter().position(|v| v == "list").unwrap();
    let level1 = d[cp].values.iter().position(|v| v == "level1").unwrap();
    for i in 0..d.len() {
        for j in i + 1..d.len() {
            for a in 0..d[i].values.len() {
                for b in 0..d[j].values.len() {
                    let banned = (i, a, j, b)
                        == (
                            dl.min(cp),
                            if dl < cp { list } else { level1 },
                            dl.max(cp),
                            if dl < cp { level1 } else { list },
                        );
                    if banned {
                        assert!(
                            !have.contains(&(i, a, j, b)),
                            "a forbidden pair was emitted"
                        );
                    } else {
                        assert!(have.contains(&(i, a, j, b)), "uncovered: {i}={a} {j}={b}");
                    }
                }
            }
        }
    }
    assert_eq!(
        rows,
        pairwise(&d, &forbidden, 7),
        "the same seed gave other rows"
    );
}

#[test]
fn test_storage_map_gen_172_three_by_three_needs_at_most_twelve_rows() {
    let d = three_by_three();
    let rows = pairwise(&d, &[], 1);
    assert!(rows.len() <= 12, "{} rows", rows.len());
    assert_eq!(covered(&d, &rows).len(), 27);
}

fn row_with(dims: &[Dim], set: &[(&str, &str)]) -> Vec<usize> {
    dims.iter()
        .map(|d| {
            set.iter()
                .find(|(n, _)| *n == d.name)
                .map(|(_, v)| d.values.iter().position(|x| x == v).unwrap())
                .unwrap_or(0)
        })
        .collect()
}

#[test]
fn test_storage_map_gen_172_the_encoder_writes_pieces_and_lists() {
    let d = dims();
    let caret = class_for(
        &row_with(&d, &[("delimiter", "caret"), ("node_layout", "row")]),
        &d,
        1,
    );
    assert_eq!(caret.nodes[0].1, NodeValue::Text("ada^x1".into()));
    let list = class_for(
        &row_with(&d, &[("delimiter", "list"), ("node_layout", "row")]),
        &d,
        2,
    );
    assert_eq!(
        list.nodes[0].1,
        NodeValue::List(vec!["ada".into(), "x1".into()])
    );
    // The answer is the row model, in ID order.
    let answer: Vec<String> = caret
        .answer
        .iter()
        .map(|r| r["F1"].as_str().unwrap().to_string())
        .collect();
    assert_eq!(answer, vec!["ada", "bob", "cy"]);
    assert_eq!(caret.rows.len(), 3);
    // With the index, the question asks for one row.
    let idx = class_for(&row_with(&d, &[("index", "on_f1")]), &d, 3);
    assert_eq!(idx.answer.len(), 1);
    assert!(
        idx.question.contains("WHERE F1 = 'bob'"),
        "{}",
        idx.question
    );
    assert!(
        idx.cls_text.contains("<SQLMap name=\"IdxF1\">"),
        "{}",
        idx.cls_text
    );
}

/// T106: every pair the compile oracle found IRIS refusing (`forbidden.toml`) is either refused by
/// iad's validator before any write, or marked `iris_only` with the reason only IRIS can tell; and
/// every IRIS error number it records has an explanation. The oracle found none over the current
/// dimensions, so this holds for every pair added later.
#[test]
fn test_storage_map_gen_172_forbidden_pairs_are_refused_or_explained() {
    use iris_agentic_dev_core::tools::storage_map::explain::EXPLAINED;
    use iris_agentic_dev_core::tools::storage_map::tree::{parse_body, split_class};
    use iris_agentic_dev_core::tools::storage_map::validate::validate;
    let path = crate::storage172_gen::fixture_dir().join("forbidden.toml");
    let Ok(text) = std::fs::read_to_string(&path) else {
        return; // no pair found yet
    };
    let t: toml::Table = text.parse().expect("forbidden.toml parses");
    let d = dims();
    for p in t
        .get("pair")
        .and_then(|p| p.as_array())
        .into_iter()
        .flatten()
    {
        let s = |k: &str| p[k].as_str().unwrap_or_default().to_string();
        for n in p
            .get("errors")
            .and_then(|e| e.as_array())
            .into_iter()
            .flatten()
        {
            let n = n.as_integer().unwrap_or(0) as u32;
            assert!(
                EXPLAINED.iter().any(|(k, _)| *k == n),
                "#{n} has no explanation"
            );
        }
        if p.get("iris_only").and_then(|v| v.as_str()).is_some() {
            continue;
        }
        let row = row_with(
            &d,
            &[(&s("dim_a"), &s("value_a")), (&s("dim_b"), &s("value_b"))],
        );
        let g = class_for(&row, &d, 999);
        let span = split_class(&g.cls_text).unwrap().remove(0);
        let nodes = parse_body(&g.cls_text[span.body_start..span.body_end]).unwrap();
        assert!(
            validate(&nodes).is_err(),
            "{} = {} with {} = {}: IRIS refuses it and iad's validator does not; add a rule or mark \
             it iris_only with the reason",
            s("dim_a"),
            s("value_a"),
            s("dim_b"),
            s("value_b")
        );
    }
}

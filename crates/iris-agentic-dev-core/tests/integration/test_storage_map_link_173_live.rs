//! Spec 173 T026: `propose` with `index_globals` links an index global to its master and says which
//! field, transform, length and piece explain each subscript, and where the row ID sits. Each
//! fixture's known answer comes from `tests/fixtures/storage173/seed/<F>.json`, never from the
//! answer under test.
//!
//! Run: `IRIS_HOST=localhost IRIS_WEB_PORT=52780 nice -n 15 cargo test --features testing -j 2 \
//!   --test integration test_storage_map_link_173 -- --test-threads=1 --include-ignored`

use crate::live_rig_171::{rig, Rig, Where};
use crate::storage173_rig::*;
use iris_agentic_dev_core::tools::Toolset;
use serde_json::{json, Value};

async fn live() -> Rig {
    let r = rig(&Where::live(), Toolset::Merged).await;
    drop_all(&r).await;
    r
}

/// The master draft's name for the seed's column: the fixtures' data maps put the columns in
/// pieces 1 to 5 in the order of `columns`, and the draft calls piece `n` `Field<n>`.
fn draft_name(f: &Fixture, column: &str) -> String {
    let at = f
        .columns()
        .iter()
        .position(|c| c == column)
        .unwrap_or_else(|| panic!("{}: no column {column}", f.name));
    format!("Field{}", at + 1)
}

/// The expression the fixture's class declares for the first subscript of its index map, with each
/// column name `{Name}` spelt as the master draft names it (`{Field1}`).
fn declared_expression(f: &Fixture) -> String {
    let index = f.seed["index_globals"][0].as_str().unwrap();
    let after = f
        .cls_text
        .split(&format!("<Global>{index}</Global>"))
        .nth(1)
        .unwrap_or_else(|| panic!("{}: no map over {index}", f.name));
    let expr = after
        .split("<Expression>")
        .nth(1)
        .and_then(|s| s.split("</Expression>").next())
        .unwrap_or_else(|| panic!("{}: no expression", f.name));
    let mut out = expr.to_string();
    for c in f.columns() {
        out = out.replace(&format!("{{{c}}}"), &format!("{{{}}}", draft_name(f, &c)));
    }
    out
}

fn opt(v: &Value, key: &str) -> Value {
    v.get(key).cloned().unwrap_or(Value::Null)
}

async fn link_of(r: &Rig, f: &Fixture) -> Value {
    let master = f.seed["master_global"].as_str().unwrap();
    let index = f.seed["index_globals"][0].as_str().unwrap();
    let out = ask_propose(r, json!({"global": master, "index_globals": [index]})).await;
    assert_eq!(out["success"], true, "{}: {out}", f.name);
    out
}

#[tokio::test]
#[ignore]
async fn f01_to_f10_link_to_the_known_answer() {
    let r = live().await;
    let mut failures = Vec::new();
    for n in 1..=10 {
        let name = format!("F{n:02}");
        let f = Fixture::read(&name);
        load_fixture_173(&r, &f).await;
        let out = link_of(&r, &f).await;
        let answer = &f.seed["answer"];
        let link = &out["indexes"][0];
        let sub = &link["subscripts"][0];
        let mut wrong = Vec::new();
        let mut expect = |what: &str, got: Value, want: Value| {
            if got != want {
                wrong.push(format!("{what}: got {got}, want {want}"));
            }
        };
        expect(
            "global",
            opt(link, "global"),
            answer["index_global"].clone(),
        );
        expect("verdict", opt(link, "verdict"), json!("inferred"));
        expect(
            "id.positions",
            opt(&link["id"], "positions"),
            answer["id_positions"].clone(),
        );
        expect("subscript verdict", opt(sub, "verdict"), json!("inferred"));
        expect(
            "field",
            opt(sub, "field"),
            json!(draft_name(&f, answer["field"].as_str().unwrap())),
        );
        expect(
            "transform",
            opt(sub, "transform"),
            answer["transform"].clone(),
        );
        expect("length", opt(sub, "length"), opt(answer, "length"));
        expect("piece", opt(sub, "piece"), opt(answer, "piece"));
        let agreement = sub["agreement"].as_f64().unwrap_or(0.0);
        if agreement < 0.95 {
            wrong.push(format!("agreement {agreement}"));
        }
        if !wrong.is_empty() {
            failures.push(format!(
                "{name}: {} in {link}; candidates {}; detail {}",
                wrong.join("; "),
                out["index_candidates"],
                out["not_inferred_detail"]
            ));
        }
        drop_fixture_173(&r, &f).await;
    }
    drop_all(&r).await;
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// T028: the drafted map is the one the fixture's class declares, and it compiled in a scratch copy
/// and found a known row through an indexed query.
#[tokio::test]
#[ignore]
async fn f01_to_f10_draft_the_declared_map_and_verify_it() {
    let r = live().await;
    let mut failures = Vec::new();
    for n in 1..=10 {
        let name = format!("F{n:02}");
        let f = Fixture::read(&name);
        load_fixture_173(&r, &f).await;
        let out = link_of(&r, &f).await;
        let link = &out["indexes"][0];
        let want = declared_expression(&f);
        let got = link["map"]["subscripts"][0]["expression"].clone();
        if got != json!(want) {
            failures.push(format!("{name}: expression got {got}, want {want:?}"));
        }
        let second = link["map"]["subscripts"][1]["expression"].clone();
        if second != json!("{ID}") {
            failures.push(format!("{name}: second subscript {second}"));
        }
        let verified = &link["verified"];
        if verified["compiled"] != json!(true) || verified["row_found"] != json!(true) {
            failures.push(format!("{name}: verified {verified}"));
        }
        drop_fixture_173(&r, &f).await;
    }
    drop_all(&r).await;
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

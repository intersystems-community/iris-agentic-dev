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

/// A map that is not the one the index holds does not verify, and the same setup with the right map
/// does: the verification can fail, so its pass means something (T028).
#[tokio::test]
#[ignore]
async fn a_wrong_map_does_not_verify_and_the_right_one_does() {
    use iris_agentic_dev_core::iris::connection::{DiscoverySource, IrisConnection};
    use iris_agentic_dev_core::tools::storage_map::link::{
        draft_index_map, IdForm, IdLink, Link, PositionScore, SubscriptLink, Transform, Verdict,
    };
    use iris_agentic_dev_core::tools::storage_map::link_run::master_view;
    use iris_agentic_dev_core::tools::storage_map::link_verify::verify;
    use iris_agentic_dev_core::tools::storage_map::master::{MasterField, MasterSpec};

    let r = live().await;
    let f = Fixture::read("F02");
    load_fixture_173(&r, &f).await;
    let t = iris_agentic_dev_core::testing::LiveTarget::from_env();
    let mut conn = IrisConnection::new(
        t.base_url(),
        &t.namespace,
        &t.username,
        &t.password,
        DiscoverySource::ExplicitFlag,
    );
    conn.probe().await;
    let client = conn
        .client(iris_agentic_dev_core::iris::http::ClientKind::Call)
        .expect("client");
    let fields: Vec<MasterField> = (1..=5)
        .map(|i| MasterField {
            name: format!("Field{i}"),
            placeholder: format!("Field{i}"),
            node: None,
            delimiter: Some("\"^\"".into()),
            piece: Some(i.to_string()),
            format: "delimited".into(),
            guessed: true,
        })
        .collect();
    let spec = MasterSpec {
        global: "^IadSm173F02".into(),
        class: None,
        map_name: "Master".into(),
        subscripts: vec!["{ID}".into()],
        key_names: vec!["ID".into()],
        row_id: vec![],
        fields,
    };
    let data: serde_json::Map<String, Value> = (1..=5)
        .map(|i| {
            (
                format!("Field{i}"),
                json!({"name": format!("Field{i}"), "delimiter": "\"^\"", "piece": i.to_string()}),
            )
        })
        .collect();
    let master_map = json!({"name": "Master", "type": "data", "global": "^IadSm173F02",
        "subscripts": [{"level": 1, "expression": "{ID}"}], "data": data});
    let view = master_view(&spec);
    let link_with = |transform| Link {
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
            pick: Some(PositionScore {
                field: 1,
                transform,
                length: None,
                piece: None,
                matches: 40,
                total: 40,
            }),
            alternatives: vec![],
        }],
        data_in_value: vec![],
    };
    let outcome = |transform| {
        let link = link_with(transform);
        let (map, _) = draft_index_map(&link, &view).unwrap();
        (link, map)
    };
    let (right_link, right_map) = outcome(Transform::SqlUpper);
    let (wrong_link, wrong_map) = outcome(Transform::Exact);
    let right = verify(
        &conn,
        &client,
        "USER",
        &spec,
        &master_map,
        &right_map,
        &right_link,
        &view,
    )
    .await;
    let wrong = verify(
        &conn,
        &client,
        "USER",
        &spec,
        &master_map,
        &wrong_map,
        &wrong_link,
        &view,
    )
    .await;
    // A map over a field the master lacks does not compile.
    let (mut lost_link, _) = outcome(Transform::SqlUpper);
    lost_link.subscripts[0].pick.as_mut().unwrap().field = 9;
    let lost_map = json!({"name": "X", "type": "index", "global": "^IadSm173F02X",
        "subscripts": [{"level": 1, "expression": "$$SQLUPPER({Nope})"}, {"level": 2, "expression": "{ID}"}]});
    let mut lost_view = view.clone();
    lost_view.field_names.push("Nope".into());
    lost_link.subscripts[0].pick.as_mut().unwrap().field = 6;
    let lost = verify(
        &conn,
        &client,
        "USER",
        &spec,
        &master_map,
        &lost_map,
        &lost_link,
        &lost_view,
    )
    .await;
    drop_fixture_173(&r, &f).await;
    drop_all(&r).await;
    assert!(
        right.compiled && right.row_found,
        "the right map must verify: {right:?}"
    );
    assert!(
        wrong.compiled && !wrong.row_found,
        "an exact map over SQLUPPER keys must compile and find nothing: {wrong:?}"
    );
    assert!(
        !lost.compiled,
        "a map over a missing property must not compile: {lost:?}"
    );
}

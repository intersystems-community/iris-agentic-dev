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

pub(crate) async fn live() -> Rig {
    let r = rig(&Where::live(), Toolset::Merged).await;
    drop_all(&r).await;
    r
}

/// The master draft's name for the seed's column: the fixtures' data maps put the columns in
/// pieces 1 to 5 in the order of `columns`, and the draft calls piece `n` `Field<n>`.
pub(crate) fn draft_name(f: &Fixture, column: &str) -> String {
    let at = f
        .master_columns()
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
    for c in f.master_columns() {
        out = out.replace(&format!("{{{c}}}"), &format!("{{{}}}", draft_name(f, &c)));
    }
    out
}

/// The fixture's declared first-subscript expression with the real column names in it.
fn declared_expression_named(f: &Fixture) -> String {
    let index = f.seed["index_globals"][0].as_str().unwrap();
    let after = f
        .cls_text
        .split(&format!("<Global>{index}</Global>"))
        .nth(1)
        .unwrap();
    after
        .split("<Expression>")
        .nth(1)
        .and_then(|s| s.split("</Expression>").next())
        .unwrap()
        .replace("&quot;", "\"")
}

pub(crate) fn opt(v: &Value, key: &str) -> Value {
    v.get(key).cloned().unwrap_or(Value::Null)
}

pub(crate) async fn link_of(r: &Rig, f: &Fixture) -> Value {
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
            null_marker: None,
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

// ---- T029: F11 to F18 ------------------------------------------------------------------------

/// Every `<Expression>` of the fixture's index map, in level order, with column names spelt as the
/// master draft names them and key parts as the seed's `key_map` says (the draft calls the parts of
/// a composite key `Key1`, `ID`).
fn declared_expressions(f: &Fixture) -> Vec<String> {
    let index = f.seed["index_globals"][0].as_str().unwrap();
    let after = f
        .cls_text
        .split(&format!("<Global>{index}</Global>"))
        .nth(1)
        .unwrap_or_else(|| panic!("{}: no map over {index}", f.name));
    let map = after.split("</SQLMap>").next().unwrap();
    let mut out = Vec::new();
    for part in map.split("<Subscript name=").skip(1) {
        let mut e = part
            .split("<Expression>")
            .nth(1)
            .and_then(|s| s.split("</Expression>").next())
            .unwrap()
            .to_string();
        for c in f.master_columns() {
            e = e.replace(&format!("{{{c}}}"), &format!("{{{}}}", draft_name(f, &c)));
        }
        if let Some(keys) = f.seed.get("key_map").and_then(Value::as_object) {
            for (k, v) in keys {
                e = e.replace(&format!("{{{k}}}"), &format!("{{{}}}", v.as_str().unwrap()));
            }
        }
        out.push(e);
    }
    out
}

/// The seed's expected subscripts: `answer.subscripts`, or the one subscript of F01 to F10.
fn answer_subscripts(answer: &Value) -> Vec<Value> {
    match answer["subscripts"].as_array() {
        Some(a) => a.clone(),
        None => vec![
            json!({"position": 1, "field": answer["field"], "transform": answer["transform"],
            "length": opt(answer, "length"), "piece": opt(answer, "piece")}),
        ],
    }
}

fn check(wrong: &mut Vec<String>, what: &str, got: Value, want: Value) {
    if got != want {
        wrong.push(format!("{what}: got {got}, want {want}"));
    }
}

/// What differs between the link and the fixture's known answer (empty when they agree).
fn differences(f: &Fixture, link: &Value) -> Vec<String> {
    let answer = &f.seed["answer"];
    let mut wrong = Vec::new();
    check(
        &mut wrong,
        "global",
        opt(link, "global"),
        answer["index_global"].clone(),
    );
    check(
        &mut wrong,
        "id.positions",
        opt(&link["id"], "positions"),
        answer["id_positions"].clone(),
    );
    check(
        &mut wrong,
        "id.form",
        opt(&link["id"], "form"),
        answer.get("id_form").cloned().unwrap_or(json!("subscript")),
    );
    for want in answer_subscripts(answer) {
        let position = want["position"].as_u64().unwrap();
        let sub = link["subscripts"]
            .as_array()
            .and_then(|a| a.iter().find(|s| s["position"] == json!(position)))
            .cloned()
            .unwrap_or(Value::Null);
        let at = format!("position {position}");
        check(
            &mut wrong,
            &format!("{at} verdict"),
            opt(&sub, "verdict"),
            want.get("verdict").cloned().unwrap_or(json!("inferred")),
        );
        // A tie lists exactly the transforms the seed names (the pick is the seed's own transform:
        // digits are the same under EXACT, ALPHAUP and UPPER, so the fixture cannot split them).
        if let Some(tied) = want.get("alternatives").and_then(Value::as_array) {
            let mut got_alt: Vec<String> = sub["alternatives"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|a| a["transform"].as_str().map(String::from))
                .collect();
            got_alt.retain(|t| Some(t.as_str()) != want["transform"].as_str());
            got_alt.sort();
            let mut want_alt: Vec<String> = tied
                .iter()
                .filter_map(|t| t.as_str().map(String::from))
                .collect();
            want_alt.sort();
            check(
                &mut wrong,
                &format!("{at} tied transforms"),
                json!(got_alt),
                json!(want_alt),
            );
        }
        check(
            &mut wrong,
            &format!("{at} field"),
            opt(&sub, "field"),
            json!(draft_name(f, want["field"].as_str().unwrap())),
        );
        check(
            &mut wrong,
            &format!("{at} transform"),
            opt(&sub, "transform"),
            want["transform"].clone(),
        );
        check(
            &mut wrong,
            &format!("{at} length"),
            opt(&sub, "length"),
            opt(&want, "length"),
        );
        check(
            &mut wrong,
            &format!("{at} piece"),
            opt(&sub, "piece"),
            opt(&want, "piece"),
        );
        if sub["agreement"].as_f64().unwrap_or(0.0) < 0.95 {
            wrong.push(format!("{at} agreement {}", sub["agreement"]));
        }
    }
    if let Some(covered) = answer["data_in_value"].as_array() {
        let want: Vec<Value> = covered
            .iter()
            .map(|c| json!({"field": draft_name(f, c["field"].as_str().unwrap()), "element": c["element"]}))
            .collect();
        check(
            &mut wrong,
            "data_in_value",
            opt(link, "data_in_value"),
            json!(want),
        );
    }
    if let Some(n) = answer.get("null") {
        let position = n["position"].as_u64().unwrap();
        let sub = link["subscripts"]
            .as_array()
            .and_then(|a| a.iter().find(|s| s["position"] == json!(position)))
            .cloned()
            .unwrap_or(Value::Null);
        let got = &sub["null_marker"];
        check(&mut wrong, "null kind", opt(got, "kind"), n["kind"].clone());
        check(
            &mut wrong,
            "null empty",
            opt(got, "empty"),
            n["empty"].clone(),
        );
        // `sampled` is how many of the `count` empty rows the sample reaches: it reads at most 3
        // nodes under one first-subscript value (every empty row shares one), except when the
        // first subscript has under 7 values (F17's index name), where it reads in order.
        check(
            &mut wrong,
            "null count",
            opt(got, "count"),
            n["sampled"].clone(),
        );
        check(
            &mut wrong,
            "null length",
            opt(got, "length"),
            opt(n, "length"),
        );
        // No literal unless the caller asked for values under the policy.
        check(&mut wrong, "null literal", opt(got, "literal"), Value::Null);
    }
    wrong
}

#[tokio::test]
#[ignore]
async fn f11_to_f18_link_to_the_known_answer() {
    let r = live().await;
    let mut failures = Vec::new();
    for n in 11..=18 {
        let name = format!("F{n:02}");
        let f = Fixture::read(&name);
        load_fixture_173(&r, &f).await;
        let out = link_of(&r, &f).await;
        let link = &out["indexes"][0];
        let mut wrong = differences(&f, link);
        // F17 is class-compiled: its first subscript is the index name, so the link as a whole is
        // `constant`; the others are inferred.
        let tied = answer_subscripts(&f.seed["answer"])
            .iter()
            .any(|s| s["verdict"] == json!("ambiguous"));
        let want = if name == "F17" {
            "constant"
        } else if tied {
            "ambiguous"
        } else {
            "inferred"
        };
        if link["verdict"] != json!(want) {
            wrong.push(format!("verdict {} want {want}", link["verdict"]));
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

/// The drafted map of each fixture that has a declared index map is that map, in the master draft's
/// names, and it verified in a scratch copy.
#[tokio::test]
#[ignore]
async fn f11_to_f16_draft_the_declared_map_and_verify_it() {
    let r = live().await;
    let mut failures = Vec::new();
    for n in 11..=16 {
        let name = format!("F{n:02}");
        let f = Fixture::read(&name);
        load_fixture_173(&r, &f).await;
        let out = link_of(&r, &f).await;
        let link = &out["indexes"][0];
        let got: Vec<Value> = link["map"]["subscripts"]
            .as_array()
            .map(|a| a.iter().map(|s| s["expression"].clone()).collect())
            .unwrap_or_default();
        let mut want: Vec<Value> = declared_expressions(&f)
            .into_iter()
            .map(Value::from)
            .collect();
        // The ID in the node value is not a subscript.
        if f.seed["answer"]["id_form"] == json!("value") {
            want.retain(|e| e != "{ID}");
        }
        if got != want {
            failures.push(format!(
                "{name}: subscripts got {got:?}, want {want:?}; {link}"
            ));
        }
        if link["verified"]["compiled"] != json!(true)
            || link["verified"]["row_found"] != json!(true)
        {
            failures.push(format!("{name}: verified {}", link["verified"]));
        }
        drop_fixture_173(&r, &f).await;
    }
    drop_all(&r).await;
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// F13: the ID is the node value, so the draft carries `Data name="Id"` and a RowIdSpec `{Id}`.
#[tokio::test]
#[ignore]
async fn f13_drafts_the_id_in_the_node_value() {
    let r = live().await;
    let f = Fixture::read("F13");
    load_fixture_173(&r, &f).await;
    let out = link_of(&r, &f).await;
    let map = &out["indexes"][0]["map"];
    assert_eq!(map["data"]["Id"]["name"], "Id", "{map}");
    assert_eq!(map["row_id"][0]["expression"], "{Id}", "{map}");
    drop_fixture_173(&r, &f).await;
    drop_all(&r).await;
}

/// F14: the drafted map covers City and Age by element.
#[tokio::test]
#[ignore]
async fn f14_drafts_the_covered_fields() {
    let r = live().await;
    let f = Fixture::read("F14");
    load_fixture_173(&r, &f).await;
    let out = link_of(&r, &f).await;
    let map = &out["indexes"][0]["map"];
    assert_eq!(map["data"]["Field2"]["piece"], "1", "{map}");
    assert_eq!(map["data"]["Field3"]["piece"], "2", "{map}");
    assert!(map["data"]["Field2"].get("delimiter").is_none(), "{map}");
    drop_fixture_173(&r, &f).await;
    drop_all(&r).await;
}

/// F15 and F16 keep the engine's own markers (`$c(0)`, `-1E14`): nothing to draft. F18's marker is
/// not one of the four, so its draft needs a `NullMarker` the sample alone cannot give: the guess
/// names it, and the literal stays out of the answer (FR-009, FR-012).
#[tokio::test]
#[ignore]
async fn f18_names_the_missing_marker_and_returns_no_literal() {
    let r = live().await;
    for name in ["F15", "F16"] {
        let f = Fixture::read(name);
        load_fixture_173(&r, &f).await;
        let out = link_of(&r, &f).await;
        let sub = &out["indexes"][0]["map"]["subscripts"][0];
        assert!(sub.get("null_marker").is_none(), "{name}: {sub}");
        drop_fixture_173(&r, &f).await;
    }
    let f = Fixture::read("F18");
    load_fixture_173(&r, &f).await;
    let out = link_of(&r, &f).await;
    let link = &out["indexes"][0];
    assert!(
        !link.to_string().contains("~NIL~"),
        "the marker's literal leaked: {link}"
    );
    let guesses = link["guesses"].to_string();
    assert!(guesses.contains("null_marker"), "{guesses}");
    assert!(
        link["map"]["subscripts"][0].get("null_marker").is_none(),
        "{link}"
    );
    assert!(
        out["not_inferred_detail"]
            .to_string()
            .contains("NullMarker"),
        "{}",
        out["not_inferred_detail"]
    );
    drop_fixture_173(&r, &f).await;
    drop_all(&r).await;
}

/// Under `dataPolicy = allow` with `include_values` and `acknowledgePhi`, the literal appears and
/// the draft carries it as `NullMarker`; without the acknowledgement the call is refused.
#[tokio::test]
#[ignore]
async fn f18_literal_under_include_values_and_the_allow_policy() {
    use crate::live_rig_171::rig_with;
    let r = rig_with(
        &Where::live(),
        Toolset::Merged,
        "write_tools_enabled = true\ndestructive_tools_enabled = true\n\n[policy.default]\ndataPolicy = \"allow\"\n",
    )
    .await;
    drop_all(&r).await;
    let f = Fixture::read("F18");
    load_fixture_173(&r, &f).await;
    let master = f.seed["master_global"].as_str().unwrap();
    let index = f.seed["index_globals"][0].as_str().unwrap();
    let args = json!({"global": master, "index_globals": [index], "include_values": true,
        "acknowledgePhi": true});
    let out = ask_propose(&r, args).await;
    assert_eq!(out["success"], true, "{out}");
    let link = &out["indexes"][0];
    assert_eq!(
        link["subscripts"][0]["null_marker"]["literal"], "~NIL~",
        "{link}"
    );
    assert_eq!(
        link["map"]["subscripts"][0]["null_marker"], "\"~NIL~\"",
        "{link}"
    );
    assert_eq!(link["verified"]["row_found"], true, "{link}");
    let unacked = ask_propose(
        &r,
        json!({"global": master, "index_globals": [index], "include_values": true}),
    )
    .await;
    assert_eq!(unacked["error_code"], "PHI_ACK_REQUIRED", "{unacked}");
    drop_fixture_173(&r, &f).await;
    drop_all(&r).await;
}

/// Under `dataPolicy = block` a call that asks for values is refused.
#[tokio::test]
#[ignore]
async fn include_values_is_refused_under_the_block_policy() {
    use crate::live_rig_171::rig_with;
    let r = rig_with(
        &Where::live(),
        Toolset::Merged,
        "write_tools_enabled = true\ndestructive_tools_enabled = true\n\n[policy.default]\ndataPolicy = \"block\"\n",
    )
    .await;
    let out = ask_propose(
        &r,
        json!({"global": "^IadSm173F18", "index_globals": ["^IadSm173F18X"],
            "include_values": true, "acknowledgePhi": true}),
    )
    .await;
    assert_eq!(out["error_code"], "DATA_POLICY_BLOCKED", "{out}");
}

/// T031: `field_names` replaces `Field1..5` in the master draft, its guesses, the index draft and
/// the link's own field names; F10 also returns the name-piece form with the real name in it.
#[tokio::test]
#[ignore]
async fn field_names_rename_the_master_draft_the_index_draft_and_the_link() {
    let r = live().await;
    let f = Fixture::read("F10");
    load_fixture_173(&r, &f).await;
    let master = f.seed["master_global"].as_str().unwrap();
    let index = f.seed["index_globals"][0].as_str().unwrap();
    let names = f.master_columns();
    let out = ask_propose(
        &r,
        json!({"global": master, "index_globals": [index], "field_names": names}),
    )
    .await;
    assert_eq!(out["success"], true, "{out}");
    let draft_keys: Vec<String> = out["draft"]["data"]
        .as_object()
        .map(|o| o.keys().cloned().collect())
        .unwrap_or_default();
    let mut want_keys = names.clone();
    want_keys.sort();
    let mut got_keys = draft_keys.clone();
    got_keys.sort();
    assert_eq!(
        got_keys, want_keys,
        "master draft data keys: {}",
        out["draft"]
    );
    for name in &names {
        assert_eq!(
            out["draft"]["data"][name]["name"],
            json!(name),
            "{}",
            out["draft"]
        );
    }
    let guesses = out["guesses"].to_string();
    assert!(guesses.contains("/maps/Master/data/Name"), "{guesses}");
    assert!(!guesses.contains("Field1"), "{guesses}");
    let link = &out["indexes"][0];
    let want_expr = declared_expression_named(&f);
    assert_eq!(
        link["map"]["subscripts"][0]["expression"],
        json!(want_expr),
        "{link}"
    );
    let want_field = f.seed["answer"]["field"].as_str().unwrap();
    assert_eq!(link["subscripts"][0]["field"], json!(want_field), "{link}");
    assert!(!link.to_string().contains("Field1"), "{link}");
    drop_fixture_173(&r, &f).await;
    drop_all(&r).await;
}

/// A list shorter than the fields the sample found is refused with the count it found.
#[tokio::test]
#[ignore]
async fn a_short_field_names_list_is_invalid_params_naming_the_sampled_count() {
    let r = live().await;
    let f = Fixture::read("F10");
    load_fixture_173(&r, &f).await;
    let master = f.seed["master_global"].as_str().unwrap();
    let index = f.seed["index_globals"][0].as_str().unwrap();
    let out = ask_propose(
        &r,
        json!({"global": master, "index_globals": [index], "field_names": ["Name", "City"]}),
    )
    .await;
    assert_eq!(out["error_code"], "INVALID_PARAMS", "{out}");
    let msg = out["error"].to_string();
    assert!(msg.contains("5 fields"), "{msg}");
    assert!(msg.contains("2 name"), "{msg}");
    drop_fixture_173(&r, &f).await;
    drop_all(&r).await;
}

/// T032: the linker's raw lines for F02, F11 and F13, as the unit parser tests read them. With
/// `IAD_REGEN_CAPTURED=1` the files are rewritten; without it the run must match them line for
/// line, so a change to the linker's output shows up here and not in a unit test that kept the
/// old format.
#[tokio::test]
#[ignore]
async fn capture_the_linker_lines_for_f02_f11_and_f13() {
    use iris_agentic_dev_core::tools::storage_map::link::parse_link_output;
    use iris_agentic_dev_core::tools::storage_map::link_code::{
        build_code, Caps, FieldAccess, LinkRequest,
    };
    let regen = std::env::var("IAD_REGEN_CAPTURED").is_ok_and(|v| v == "1");
    let dir =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/storage173/captured");
    let r = live().await;
    for name in ["F02", "F11", "F13"] {
        let f = Fixture::read(name);
        load_fixture_173(&r, &f).await;
        let fields: Vec<FieldAccess> = (1..=f.master_columns().len())
            .map(|piece| FieldAccess {
                node: None,
                delimiter: Some(94),
                piece: u32::try_from(piece).unwrap(),
            })
            .collect();
        let master = f.seed["master_global"].as_str().unwrap();
        let index = f.seed["index_globals"][0].as_str().unwrap();
        let code = build_code(&LinkRequest {
            master,
            candidate: index,
            sub_prefix: &[],
            key_parts: 1,
            fields: &fields,
            caps: Caps::for_node_cap(200),
            with_values: false,
        })
        .unwrap();
        let out = exec(&r, &code).await;
        let lines: Vec<String> = out
            .lines()
            .map(str::trim)
            .filter(|l| l.starts_with('{') && serde_json::from_str::<Value>(l).is_ok())
            .map(String::from)
            .collect();
        let text = format!("{}\n", lines.join("\n"));
        parse_link_output(&text).unwrap_or_else(|e| panic!("{name}: {e}\n{out}"));
        let path = dir.join(format!("link_{name}.jsonl"));
        if regen {
            std::fs::write(&path, &text).unwrap();
        } else {
            let kept = std::fs::read_to_string(&path)
                .unwrap_or_else(|e| panic!("{}: {e}", path.display()));
            assert_eq!(
                kept, text,
                "{name}: the linker's lines changed (rerun with IAD_REGEN_CAPTURED=1)"
            );
        }
        drop_fixture_173(&r, &f).await;
    }
    drop_all(&r).await;
}

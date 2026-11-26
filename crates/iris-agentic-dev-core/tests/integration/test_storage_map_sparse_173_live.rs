//! Spec 173 T051 and T052: an index with fewer nodes than the master has rows is sparse. `propose`
//! reports the share the index lacks and, when one rule explains every missing row and no present
//! row, drafts it as a `Condition` with `PopulationType conditional`, checked in a scratch copy.
//! Every known answer (which rows are missing, the rule that skipped them) comes from
//! `tests/fixtures/storage173/seed/<F>.json`, never from the answer under test.
//!
//! Run: `IRIS_HOST=localhost IRIS_WEB_PORT=52780 nice -n 15 cargo test --features testing -j 2 \
//!   --test integration test_storage_map_sparse_173 -- --test-threads=1 --include-ignored`

use crate::live_rig_171::{rig_with, Rig, Where};
use crate::storage173_rig::*;
use crate::test_storage_map_link_173_live::{draft_name, live};
use iris_agentic_dev_core::tools::Toolset;
use serde_json::{json, Value};

async fn sparse_of(r: &Rig, f: &Fixture, extra: Value) -> Value {
    load_fixture_173(r, f).await;
    let master = f.seed["master_global"].as_str().unwrap();
    let index = f.seed["index_globals"][0].as_str().unwrap();
    let mut args = json!({"global": master, "index_globals": [index]});
    for (k, v) in extra.as_object().into_iter().flatten() {
        args[k] = v.clone();
    }
    let out = ask_propose(r, args).await;
    assert_eq!(out["success"], true, "{}: {out}", f.name);
    out
}

async fn done(r: &Rig, f: &Fixture) {
    drop_fixture_173(r, f).await;
    drop_all(r).await;
}

/// The seed's condition with each column name spelt as the master draft names it.
fn named(f: &Fixture, text: &str) -> String {
    let mut out = text.to_string();
    for c in f.master_columns() {
        out = out.replace(&format!("{{{c}}}"), &format!("{{{}}}", draft_name(f, &c)));
    }
    out
}

/// What every sparse fixture shares: the share, the counts, the rule, the verdict.
fn assert_report(f: &Fixture, link: &Value) {
    let a = &f.seed["answer"];
    let m = &link["missing_rows"];
    let share = m["share"].as_f64().unwrap_or(-1.0);
    let want = a["missing_share"].as_f64().unwrap();
    assert!(
        (share - want).abs() < 0.02,
        "{}: share {share}, want {want}: {m}",
        f.name
    );
    assert_eq!(m["rows"], a["rows"], "{}: {m}", f.name);
    assert_eq!(
        m["missing"],
        json!(a["missing_ids"].as_array().unwrap().len()),
        "{}: {m}",
        f.name
    );
    assert_eq!(link["verdict"], a["link_verdict"], "{}: {link}", f.name);
}

fn assert_condition_drafted_and_verified(f: &Fixture, link: &Value) {
    let a = &f.seed["answer"];
    let map = &link["map"];
    assert_eq!(
        map["condition"],
        json!(named(f, a["condition"].as_str().unwrap())),
        "{}: {map}",
        f.name
    );
    assert_eq!(map["population_type"], "conditional", "{}: {map}", f.name);
    assert_eq!(
        map["condition_fields"],
        json!(draft_name(f, a["field"].as_str().unwrap())),
        "{}: {map}",
        f.name
    );
    // The same rows through the map as through the master: the master keeps exactly the rows the
    // index holds, which the seed knows.
    let kept = a["rows"].as_u64().unwrap() - a["missing_ids"].as_array().unwrap().len() as u64;
    assert_eq!(link["verified"]["compiled"], true, "{}: {link}", f.name);
    assert_eq!(link["verified"]["row_found"], true, "{}: {link}", f.name);
    assert_eq!(
        link["verified"]["rows_kept"],
        json!(kept),
        "{}: {link}",
        f.name
    );
}

/// F22: the index skips NULL City, so the rule is NULL and the draft is `{City}'=""`.
#[tokio::test]
#[ignore]
async fn f22_a_null_rule_drafts_the_null_condition() {
    let r = live().await;
    let f = Fixture::read("F22");
    let out = sparse_of(&r, &f, json!({})).await;
    let link = &out["indexes"][0];
    assert_report(&f, link);
    assert_eq!(
        link["missing_rows"]["rule"],
        json!({"kind": "null", "field": draft_name(&f, "City")}),
        "{link}"
    );
    assert_condition_drafted_and_verified(&f, link);
    done(&r, &f).await;
}

/// F23: the index skips empty-string City. That is not the NULL rule: the condition is
/// `{City}'=$c(0)`, the form measured to skip exactly the empty-string rows.
#[tokio::test]
#[ignore]
async fn f23_an_empty_string_rule_drafts_the_c0_condition_not_the_null_one() {
    let r = live().await;
    let f = Fixture::read("F23");
    let out = sparse_of(&r, &f, json!({})).await;
    let link = &out["indexes"][0];
    assert_report(&f, link);
    assert_eq!(
        link["missing_rows"]["rule"],
        json!({"kind": "empty_string", "field": draft_name(&f, "City")}),
        "{link}"
    );
    assert_ne!(
        link["map"]["condition"],
        json!(named(&f, "{City}'=\"\"")),
        "{link}"
    );
    assert_condition_drafted_and_verified(&f, link);
    done(&r, &f).await;
}

/// F24: the index skips Status other than A. The literal is a sampled value: with `include_values`
/// under `dataPolicy = allow` and `acknowledgePhi` the draft carries `{Status}="A"`; without them
/// the answer says the rule exists and holds the literal back.
#[tokio::test]
#[ignore]
async fn f24_a_fixed_value_rule_needs_include_values_for_its_literal() {
    let r = rig_with(
        &Where::live(),
        Toolset::Merged,
        "write_tools_enabled = true\ndestructive_tools_enabled = true\n\n[policy.default]\ndataPolicy = \"allow\"\n",
    )
    .await;
    drop_all(&r).await;
    let f = Fixture::read("F24");
    let field = draft_name(&f, "Status");
    // Without the values: the rule and its field, no literal, no condition.
    let out = sparse_of(&r, &f, json!({})).await;
    let link = &out["indexes"][0];
    assert_report(&f, link);
    assert_eq!(
        link["missing_rows"]["rule"],
        json!({"kind": "fixed", "field": field}),
        "{link}"
    );
    assert!(link["map"].get("condition").is_none(), "{link}");
    let why = out["not_inferred_detail"].to_string();
    assert!(why.contains("include_values"), "{why}");
    assert!(why.contains(&field), "{why}");

    // With them: the literal is returned and the condition drafted and verified.
    let out = ask_propose(
        &r,
        json!({"global": f.seed["master_global"], "index_globals": [f.seed["index_globals"][0]],
            "include_values": true, "acknowledgePhi": true}),
    )
    .await;
    assert_eq!(out["success"], true, "{out}");
    let link = &out["indexes"][0];
    assert_report(&f, link);
    assert_eq!(
        link["missing_rows"]["rule"],
        json!({"kind": "fixed", "field": field, "literal": f.seed["answer"]["literal"]}),
        "{link}"
    );
    assert_condition_drafted_and_verified(&f, link);
    done(&r, &f).await;
}

/// F25: every 7th row is missing and no rule explains that: the share is reported, no condition is
/// drafted, and the answer says the map covers fewer rows than the master.
#[tokio::test]
#[ignore]
async fn f25_no_rule_fits_every_seventh_row() {
    let r = live().await;
    let f = Fixture::read("F25");
    let out = sparse_of(&r, &f, json!({})).await;
    let link = &out["indexes"][0];
    assert_report(&f, link);
    let share = link["missing_rows"]["share"].as_f64().unwrap();
    assert!((share - 1.0 / 7.0).abs() < 0.02, "{link}");
    assert_eq!(link["missing_rows"]["rule"], Value::Null, "{link}");
    let map = &link["map"];
    assert!(map.get("condition").is_none(), "{map}");
    assert!(map.get("population_type").is_none(), "{map}");
    let why = out["not_inferred_detail"].to_string();
    assert!(why.contains("no rule fit"), "{why}");
    assert!(why.contains("fewer rows"), "{why}");
    done(&r, &f).await;
}

/// A dense index lacks nothing: no `missing_rows` key, no condition.
#[tokio::test]
#[ignore]
async fn a_dense_index_reports_no_missing_rows() {
    let r = live().await;
    let f = Fixture::read("F01");
    load_fixture_173(&r, &f).await;
    let master = f.seed["master_global"].as_str().unwrap();
    let index = f.seed["index_globals"][0].as_str().unwrap();
    let out = ask_propose(&r, json!({"global": master, "index_globals": [index]})).await;
    let link = &out["indexes"][0];
    assert_eq!(link["verdict"], "inferred", "{link}");
    assert!(link.get("missing_rows").is_none(), "{link}");
    assert!(link["map"].get("condition").is_none(), "{link}");
    done(&r, &f).await;
}

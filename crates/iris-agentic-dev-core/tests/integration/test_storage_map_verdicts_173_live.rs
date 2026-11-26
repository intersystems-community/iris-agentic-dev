//! Spec 173 T033 to T037: a link says how sure it is. Each fixture's known answer comes from
//! `tests/fixtures/storage173/seed/<F>.json`, never from the answer under test. The false-link set
//! (F29, F30, F31) must never read as `inferred`.
//!
//! Run: `IRIS_HOST=localhost IRIS_WEB_PORT=52780 nice -n 15 cargo test --features testing -j 2 \
//!   --test integration test_storage_map_verdicts_173 -- --test-threads=1 --include-ignored`

use crate::live_rig_171::Rig;
use crate::storage173_rig::*;
use crate::test_storage_map_link_173_live::{draft_name, link_of, live, opt};
use serde_json::{json, Value};

/// The fixture's link (item 0 of `indexes`) after loading it, and the whole answer.
async fn linked(r: &Rig, f: &Fixture) -> Value {
    load_fixture_173(r, f).await;
    link_of(r, f).await
}

async fn done(r: &Rig, f: &Fixture) {
    drop_fixture_173(r, f).await;
    drop_all(r).await;
}

fn subscript(link: &Value, position: u64) -> &Value {
    link["subscripts"]
        .as_array()
        .and_then(|a| a.iter().find(|s| s["position"] == json!(position)))
        .unwrap_or_else(|| panic!("no subscript at position {position} in {link}"))
}

fn seed_subscript(f: &Fixture, position: u64) -> Value {
    f.seed["answer"]["subscripts"]
        .as_array()
        .and_then(|a| a.iter().find(|s| s["position"] == json!(position)))
        .cloned()
        .unwrap_or_else(|| panic!("{}: no seed subscript at {position}", f.name))
}

fn verdicts_agree(f: &Fixture, link: &Value) {
    assert_eq!(
        link["verdict"], f.seed["answer"]["link_verdict"],
        "{}: link verdict in {link}",
        f.name
    );
    if f.seed["answer"]["no_map"] == json!(true) {
        assert!(
            link.get("map").is_none(),
            "{}: a map was drafted: {link}",
            f.name
        );
    }
    if let Some(positions) = f.seed["answer"]["id_positions"].as_array() {
        assert_eq!(
            link["id"]["positions"],
            json!(positions),
            "{}: id positions in {link}",
            f.name
        );
    }
    for want in f.seed["answer"]["subscripts"]
        .as_array()
        .into_iter()
        .flatten()
    {
        let got = subscript(link, want["position"].as_u64().unwrap());
        assert_eq!(
            got["verdict"], want["verdict"],
            "{}: position {} in {link}",
            f.name, want["position"]
        );
    }
}

/// F26: two identical columns tie; both fields are listed and the pick is a guess.
#[tokio::test]
#[ignore]
async fn f26_identical_columns_are_ambiguous_with_both_fields_listed() {
    let r = live().await;
    let f = Fixture::read("F26");
    let out = linked(&r, &f).await;
    let link = &out["indexes"][0];
    verdicts_agree(&f, link);
    let want = seed_subscript(&f, 1);
    let sub = subscript(link, 1);
    let mut got: Vec<String> = sub["alternatives"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|a| a["field"].as_str().map(String::from))
        .collect();
    got.sort();
    got.dedup();
    let mut fields: Vec<String> = want["fields"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| draft_name(&f, c.as_str().unwrap()))
        .collect();
    fields.sort();
    assert_eq!(got, fields, "alternatives in {sub}");
    assert_eq!(sub["transform"], want["transform"], "{sub}");
    let name = link["map"]["name"].as_str().unwrap();
    assert!(
        link["guesses"]
            .to_string()
            .contains(&format!("/maps/{name}/subscripts/1")),
        "the pick is not marked a guess: {}",
        link["guesses"]
    );
    done(&r, &f).await;
}

/// F27: five rows are under the 20-node floor: listed, not drafted.
#[tokio::test]
#[ignore]
async fn f27_five_rows_are_too_few_nodes_and_draft_no_map() {
    let r = live().await;
    let f = Fixture::read("F27");
    let out = linked(&r, &f).await;
    let link = &out["indexes"][0];
    verdicts_agree(&f, link);
    assert_eq!(link["usable_nodes"], json!(f.rows().len()), "{link}");
    done(&r, &f).await;
}

/// F28: one value across the sample is `constant`, which explains no field.
#[tokio::test]
#[ignore]
async fn f28_a_constant_column_is_constant() {
    let r = live().await;
    let f = Fixture::read("F28");
    let out = linked(&r, &f).await;
    let link = &out["indexes"][0];
    verdicts_agree(&f, link);
    let sub = subscript(link, 1);
    assert!(
        sub.get("field").is_none() && sub.get("transform").is_none(),
        "{sub}"
    );
    done(&r, &f).await;
}

/// F29: Age equals the ID on every row. Both subscripts resolve to master rows; the ID is the last
/// position, and Age is never reported as the ID.
#[tokio::test]
#[ignore]
async fn f29_age_equal_to_id_is_never_the_id() {
    let r = live().await;
    let f = Fixture::read("F29");
    let out = linked(&r, &f).await;
    let link = &out["indexes"][0];
    verdicts_agree(&f, link);
    let want = seed_subscript(&f, 1);
    let sub = subscript(link, 1);
    assert_eq!(
        sub["field"],
        json!(draft_name(&f, want["field"].as_str().unwrap())),
        "{sub}"
    );
    assert_eq!(sub["transform"], want["transform"], "{sub}");
    done(&r, &f).await;
}

/// F30: an index whose first subscript matches no field is never `inferred`.
#[tokio::test]
#[ignore]
async fn f30_an_unrelated_index_is_not_inferred() {
    let r = live().await;
    let f = Fixture::read("F30");
    let out = linked(&r, &f).await;
    let link = &out["indexes"][0];
    verdicts_agree(&f, link);
    assert_ne!(link["verdict"], "inferred", "{link}");
    assert_eq!(link["not_explained"][0]["position"], 1, "{link}");
    done(&r, &f).await;
}

/// F31: a global that shares the master's name prefix and holds nothing the master has is
/// `not_linked`, with the best agreement seen.
#[tokio::test]
#[ignore]
async fn f31_an_unrelated_global_is_not_linked_with_its_best_agreement() {
    let r = live().await;
    let f = Fixture::read("F31");
    let out = linked(&r, &f).await;
    let link = &out["indexes"][0];
    verdicts_agree(&f, link);
    assert_eq!(
        link["id"]["agreement"].as_f64(),
        f.seed["answer"]["id_agreement"].as_f64(),
        "{link}"
    );
    done(&r, &f).await;
}

/// F33: a subscript made by code (`$zd`) is `not_explained` with the best agreement seen, and no
/// expression is guessed for it.
#[tokio::test]
#[ignore]
async fn f33_a_computed_subscript_is_not_explained_and_names_no_code() {
    let r = live().await;
    let f = Fixture::read("F33");
    let out = linked(&r, &f).await;
    let link = &out["indexes"][0];
    verdicts_agree(&f, link);
    let sub = subscript(link, 1);
    assert!(sub["agreement"].as_f64().unwrap() < 0.95, "{sub}");
    assert!(
        sub.get("field").is_none() && sub.get("transform").is_none(),
        "{sub}"
    );
    assert_eq!(link["not_explained"][0]["position"], 1, "{link}");
    assert!(
        link["not_explained"][0]["best_agreement"]
            .as_f64()
            .is_some(),
        "{link}"
    );
    done(&r, &f).await;
}

/// UPPER keeps punctuation and blanks, ALPHAUP drops them.
fn alphaup(s: &str) -> String {
    s.chars()
        .filter(|c| c.is_alphanumeric())
        .flat_map(char::to_uppercase)
        .collect()
}

/// F34: UPPER and ALPHAUP give the same key for the first 700 names, so a walk of the first nodes
/// cannot tell them apart; the spread sample reaches the names with a blank and separates them.
#[tokio::test]
#[ignore]
async fn f34_the_spread_sample_separates_what_a_prefix_walk_cannot() {
    let r = live().await;
    let f = Fixture::read("F34");
    // The fixture's own claim, from its seed rows: sorted by UPPER, the first `plain_prefix_keys`
    // names are the same under ALPHAUP, and some later name is not.
    let mut keys: Vec<String> = f
        .rows()
        .iter()
        .map(|row| row[0].as_str().unwrap().to_uppercase())
        .collect();
    keys.sort();
    let plain = f.seed["answer"]["plain_prefix_keys"].as_u64().unwrap() as usize;
    assert!(
        keys[..plain].iter().all(|k| *k == alphaup(k)),
        "the prefix is not plain"
    );
    assert!(
        keys[plain..].iter().any(|k| *k != alphaup(k)),
        "the tail is not different"
    );
    let out = linked(&r, &f).await;
    let link = &out["indexes"][0];
    verdicts_agree(&f, link);
    let want = seed_subscript(&f, 1);
    let sub = subscript(link, 1);
    assert_eq!(
        sub["field"],
        json!(draft_name(&f, want["field"].as_str().unwrap())),
        "{sub}"
    );
    assert_eq!(sub["transform"], want["transform"], "{sub}");
    assert_eq!(sub["alternatives"], json!([]), "{sub}");
    assert!(
        opt(link, "nodes_sampled").as_u64().unwrap() <= 200,
        "{link}"
    );
    done(&r, &f).await;
}

/// How many of the fixture's index nodes have no master row, from the seed: the rows it killed.
fn orphans_of(f: &Fixture) -> (u64, u64) {
    (
        f.seed["answer"]["killed_rows"].as_array().unwrap().len() as u64,
        f.rows().len() as u64,
    )
}

/// F44 and F45: deleted rows leave index nodes with no master row. Under 5% they change nothing;
/// above it the link is still `inferred` (the ID agreement is over 0.9) and the share is reported.
#[tokio::test]
#[ignore]
async fn f44_f45_orphan_nodes_are_reported_only_above_five_percent() {
    let r = live().await;
    for name in ["F44", "F45"] {
        let f = Fixture::read(name);
        let out = linked(&r, &f).await;
        let link = &out["indexes"][0];
        verdicts_agree(&f, link);
        let (gone, of) = orphans_of(&f);
        assert_eq!(link["usable_nodes"], json!(of - gone), "{name}: {link}");
        if f.seed["answer"]["orphan_reported"] == json!(true) {
            assert_eq!(link["orphan_nodes"]["count"], json!(gone), "{name}: {link}");
            let share = link["orphan_nodes"]["share"].as_f64().unwrap();
            assert!(
                (share - gone as f64 / of as f64).abs() < 1e-3,
                "{name}: {link}"
            );
            assert!(
                out["not_inferred_detail"]
                    .to_string()
                    .contains("no master row"),
                "{name}: {}",
                out["not_inferred_detail"]
            );
        } else {
            assert!(link.get("orphan_nodes").is_none(), "{name}: {link}");
        }
        drop_fixture_173(&r, &f).await;
    }
    drop_all(&r).await;
}

/// F46: 89 of 100 nodes find their row, under the 0.9 floor, so the link is `not_linked` and the
/// agreement is the one seen.
#[tokio::test]
#[ignore]
async fn f46_an_id_agreement_under_the_floor_is_not_linked() {
    let r = live().await;
    let f = Fixture::read("F46");
    let out = linked(&r, &f).await;
    let link = &out["indexes"][0];
    assert_eq!(link["verdict"], f.seed["answer"]["link_verdict"], "{link}");
    let (gone, of) = orphans_of(&f);
    let want = (of - gone) as f64 / of as f64;
    assert!(
        (link["id"]["agreement"].as_f64().unwrap() - want).abs() < 1e-3,
        "{link}"
    );
    assert!(link.get("map").is_none(), "{link}");
    done(&r, &f).await;
}

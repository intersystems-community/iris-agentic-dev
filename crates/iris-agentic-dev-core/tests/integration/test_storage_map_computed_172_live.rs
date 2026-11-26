//! Benjamin De Boe's iFind case on live IRIS (spec 172, 2026-11-26): a table over a bitmap-chunk
//! global whose third subscript is found by `NextCode`. `IadSm172.Bits` is an invented look-alike
//! (its own global `^IadSm172Bits`, its own class), seeded with a $BIT string at depth 2.
//!
//! Run: `IRIS_HOST=localhost IRIS_WEB_PORT=52780 cargo test --features testing --test integration \
//!   test_storage_map_computed_172 -- --test-threads=1 --include-ignored`

use crate::live_rig_171::{rig, Rig, Where};
use crate::storage172_rig::{ask, drop_fixture, load_fixture, Fixture};
use iris_agentic_dev_core::tools::Toolset;
use serde_json::{json, Value};

async fn call(r: &Rig, action: &str, class: &str) -> Value {
    r.call(
        "iris_storage_map",
        json!({"action": action, "class": class, "namespace": "USER"}),
    )
    .await
}

#[tokio::test]
#[ignore]
async fn computed_172_the_look_alike_really_returns_the_ids_its_bits_name() {
    let r = rig(&Where::live(), Toolset::Merged).await;
    let f = Fixture::read("Bits");
    load_fixture(&r, &f).await;
    let rows = ask(&r, &f).await;
    let problems = drop_fixture(&r, &f).await;
    assert!(problems.is_empty(), "{problems:?}");
    // Bits 3, 5 and 8 are IDs 2, 4 and 7 (bit 1 is ID 0): the level is real, though no node holds it.
    let words: Vec<i64> = rows.iter().filter_map(|x| x["Word"].as_i64()).collect();
    assert_eq!(words, vec![2, 4, 7], "{rows:?}");
}

#[tokio::test]
#[ignore]
async fn computed_172_read_flags_level_three_and_the_streamlocation_is_compiler_added() {
    let r = rig(&Where::live(), Toolset::Merged).await;
    let f = Fixture::read("Bits");
    load_fixture(&r, &f).await;
    let got = call(&r, "read", "IadSm172.Bits").await;
    let md = call(&r, "diagram", "IadSm172.Bits").await;
    let problems = drop_fixture(&r, &f).await;
    assert!(problems.is_empty(), "{problems:?}");
    let facts = got["derived"].to_string();
    assert!(
        facts.contains("/maps/Chunks/subscripts/3") && facts.contains("computed by code"),
        "{got}"
    );
    assert!(facts.contains("access variable 2D2"), "{got}");
    assert_eq!(got["compiler_added"], json!(["StreamLocation"]), "{got}");
    assert!(
        got["storage"]["keywords"]["StreamLocation"].is_string(),
        "read keeps the element: {got}"
    );
    let document = md["markdown"].as_str().unwrap_or_default();
    assert!(!document.contains("StreamLocation"), "{md}");
    assert!(document.contains("computed by code"), "{md}");
}

#[tokio::test]
#[ignore]
async fn computed_172_compare_over_data_names_the_depth_the_bits_sit_at() {
    let r = rig(&Where::live(), Toolset::Merged).await;
    let f = Fixture::read("Bits");
    load_fixture(&r, &f).await;
    let got = call(&r, "compare", "IadSm172.Bits").await;
    let problems = drop_fixture(&r, &f).await;
    assert!(problems.is_empty(), "{problems:?}");
    assert_eq!(got["success"], true, "{got}");
    assert_eq!(got["verified"], true, "{got}");
    let notes = got["notes"].to_string();
    assert!(
        notes.contains("declares 3 levels") && notes.contains("data sits at depth 2"),
        "{got}"
    );
}

#[tokio::test]
#[ignore]
async fn computed_172_compare_over_an_empty_global_says_it_checked_nothing() {
    let r = rig(&Where::live(), Toolset::Merged).await;
    let f = Fixture::read("Bits");
    load_fixture(&r, &f).await;
    let kill = r
        .call(
            "iris_global",
            json!({"action": "kill", "global_name": "^IadSm172Bits", "namespace": "USER"}),
        )
        .await;
    assert_eq!(kill["success"], true, "{kill}");
    let got = call(&r, "compare", "IadSm172.Bits").await;
    let problems = drop_fixture(&r, &f).await;
    assert!(problems.is_empty(), "{problems:?}");
    assert_eq!(got["verified"], false, "{got}");
    assert!(
        got["notes"]
            .to_string()
            .contains("no data found at ^IadSm172Bits, the map was not checked"),
        "{got}"
    );
}

#[tokio::test]
#[ignore]
async fn computed_172_propose_lists_what_it_did_not_infer() {
    let r = rig(&Where::live(), Toolset::Merged).await;
    let f = Fixture::read("Member");
    load_fixture(&r, &f).await;
    let got = r
        .call(
            "iris_storage_map",
            json!({"action": "propose", "global": "^IadSm172Member", "namespace": "USER"}),
        )
        .await;
    let problems = drop_fixture(&r, &f).await;
    assert!(problems.is_empty(), "{problems:?}");
    assert_eq!(got["success"], true, "{got}");
    let not = got["not_inferred"].to_string();
    for want in [
        "datatypes",
        "collations",
        "index maps",
        "computed subscripts",
    ] {
        assert!(not.contains(want), "{want}: {got}");
    }
    assert!(
        got["draft_note"]
            .as_str()
            .unwrap_or_default()
            .contains("check against real data"),
        "{got}"
    );
    // The known answer under the draft: three pieces on a one-level row node.
    assert_eq!(
        got["draft"]["data"].as_object().map(|d| d.len()),
        Some(3),
        "{got}"
    );
}

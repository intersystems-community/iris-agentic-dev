//! T050: `uses` action for finding classes using a global (spec 172 US2).
//!
//! Run: `IRIS_HOST=localhost IRIS_WEB_PORT=52780 cargo test --features testing --test integration \
//!   test_storage_map_uses_172 -- --test-threads=1 --include-ignored`

use crate::live_rig_171::{rig, Where};
use crate::storage172_rig::{drop_fixture, load_fixture, Fixture};
use iris_agentic_dev_core::tools::Toolset;
use serde_json::json;

#[tokio::test]
#[ignore]
async fn uses_finds_data_map() {
    let r = rig(&Where::live(), Toolset::Merged).await;
    let member = Fixture::read("Member");

    load_fixture(&r, &member).await;

    let got = r
        .call(
            "iris_storage_map",
            json!({"action": "uses", "global": "^IadSm172Member", "namespace": "USER"}),
        )
        .await;

    let p = drop_fixture(&r, &member).await;
    assert!(p.is_empty(), "{p:?}");

    assert_eq!(got["success"], true, "{got}");
    assert_eq!(got["action"], "uses", "{got}");
    assert_eq!(got["global"], "^IadSm172Member", "{got}");
    assert!(got["namespaces_searched"].is_array(), "{got}");

    let uses = got["uses"].as_array().expect("uses should be array");
    assert!(!uses.is_empty(), "Expected at least one use: {got}");

    // Should find the Master data map of Member
    let member_use = uses
        .iter()
        .find(|u| u["class"].as_str() == Some("IadSm172.Member"));
    assert!(member_use.is_some(), "Member not found: {got}");
    let m = member_use.unwrap();
    assert_eq!(m["map"], "Master", "{got}");
    assert_eq!(m["type"], "data", "{got}");

    assert_eq!(got["count"].as_u64().unwrap(), 1, "{got}");
}

#[tokio::test]
#[ignore]
async fn uses_finds_index_global() {
    let r = rig(&Where::live(), Toolset::Merged).await;
    let member_by_city = Fixture::read("MemberByCity");
    load_fixture(&r, &member_by_city).await;

    let got = r
        .call(
            "iris_storage_map",
            json!({"action": "uses", "global": "^IadSm172MemberByCityI", "namespace": "USER"}),
        )
        .await;

    let problems = drop_fixture(&r, &member_by_city).await;
    assert!(problems.is_empty(), "{problems:?}");

    assert_eq!(got["success"], true, "{got}");
    let uses = got["uses"].as_array().expect("uses should be array");
    assert!(!uses.is_empty(), "Expected at least one use: {got}");

    let use_item = &uses[0];
    assert_eq!(use_item["class"], "IadSm172.MemberByCity", "{got}");
    assert_eq!(use_item["type"], "index", "{got}");
}

#[tokio::test]
#[ignore]
async fn uses_returns_empty_for_unused_global() {
    let r = rig(&Where::live(), Toolset::Merged).await;

    let got = r
        .call(
            "iris_storage_map",
            json!({"action": "uses", "global": "^NoSuchGlobal", "namespace": "USER"}),
        )
        .await;

    assert_eq!(got["success"], true, "{got}");
    let uses = got["uses"].as_array().expect("uses should be array");
    assert!(uses.is_empty(), "Expected no uses: {got}");
    assert_eq!(got["count"], 0, "{got}");

    let ns_searched = got["namespaces_searched"].as_array().unwrap();
    assert!(ns_searched.contains(&json!("USER")), "{got}");
}

/// Default storage counts: a class whose `DataLocation` and `IdLocation` are the global is two
/// uses, `default_data` and `default_id`; its `IndexLocation` global is a `default_index` use.
#[tokio::test]
#[ignore]
async fn uses_finds_default_storage_locations() {
    let r = rig(&Where::live(), Toolset::Merged).await;
    let book = Fixture::read("DefaultBook");
    load_fixture(&r, &book).await;
    let ask = |g: &str| json!({"action": "uses", "global": g, "namespaces": ["USER"], "namespace": "USER"});
    let data = r
        .call("iris_storage_map", ask("^IadSm172.DefaultBookD"))
        .await;
    let index = r
        .call("iris_storage_map", ask("^IadSm172.DefaultBookI"))
        .await;
    let p = drop_fixture(&r, &book).await;
    assert!(p.is_empty(), "{p:?}");

    let maps = |v: &serde_json::Value| -> Vec<(String, String, String)> {
        let mut out: Vec<_> = v["uses"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|u| u["class"] == "IadSm172.DefaultBook")
            .map(|u| {
                (
                    u["storage"].as_str().unwrap_or("").to_string(),
                    u["map"].as_str().unwrap_or("").to_string(),
                    u["type"].as_str().unwrap_or("").to_string(),
                )
            })
            .collect();
        out.sort();
        out
    };
    let d = |m: &str, t: &str| ("default".to_string(), m.to_string(), t.to_string());
    assert_eq!(
        maps(&data),
        vec![d("default_data", "data"), d("default_id", "id")],
        "{data}"
    );
    assert_eq!(maps(&index), vec![d("default_index", "index")], "{index}");
}

/// A namespace that does not exist fails the search and says so; it is not an empty answer.
#[tokio::test]
#[ignore]
async fn uses_in_a_namespace_that_does_not_exist_fails() {
    let r = rig(&Where::live(), Toolset::Merged).await;
    let got = r
        .call(
            "iris_storage_map",
            json!({"action": "uses", "global": "^IadSm172Member",
                   "namespaces": ["IADNOSUCHNS172"], "namespace": "USER"}),
        )
        .await;
    assert_ne!(got["success"], true, "{got}");
    assert!(got["uses"].is_null(), "{got}");
    assert!(
        got.to_string().contains("Query failed"),
        "the answer does not say the search failed: {got}"
    );
}

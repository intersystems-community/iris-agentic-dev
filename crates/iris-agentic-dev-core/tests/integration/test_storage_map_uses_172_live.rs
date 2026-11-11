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

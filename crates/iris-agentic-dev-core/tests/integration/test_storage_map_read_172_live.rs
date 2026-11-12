//! T043: `iris_storage_map action=read` on live IRIS, checked against the compiled dictionary and
//! the fixtures (spec 172 US1).
//!
//! Run: `IRIS_HOST=localhost IRIS_WEB_PORT=52780 cargo test --features testing --test integration \
//!   test_storage_map_read_172 -- --test-threads=1 --include-ignored`

use crate::live_rig_171::{rig, Rig, Where};
use crate::storage172_rig::{drop_fixture, load_fixture, Fixture};
use iris_agentic_dev_core::tools::Toolset;
use serde_json::{json, Value};

async fn read(r: &Rig, class: &str) -> Value {
    r.call(
        "iris_storage_map",
        json!({"action": "read", "class": class, "namespace": "USER"}),
    )
    .await
}

#[tokio::test]
#[ignore]
async fn read_member_matches_the_compiled_dictionary() {
    let r = rig(&Where::live(), Toolset::Merged).await;
    let f = Fixture::read("Member");
    load_fixture(&r, &f).await;
    let got = read(&r, "IadSm172.Member").await;
    let dict = r
        .call(
            "iris_query",
            json!({"query": "SELECT Name, Delimiter, Piece FROM %Dictionary.CompiledStorageSQLMapData WHERE parent = 'IadSm172.Member||SQLStorage||Master' ORDER BY Name",
                "namespace": "USER"}),
        )
        .await;
    let problems = drop_fixture(&r, &f).await;
    assert!(problems.is_empty(), "{problems:?}");
    assert_eq!(got["success"], true, "{got}");
    assert_eq!(got["model_version"], 1, "{got}");
    let map = &got["storage"]["maps"][0];
    assert_eq!(map["name"], "Master", "{got}");
    assert_eq!(map["global"], "^IadSm172Member", "{got}");
    assert_eq!(map["subscripts"][0]["expression"], "{ID}", "{got}");
    let rows = dict["rows"].as_array().cloned().unwrap_or_default();
    assert_eq!(rows.len(), 3, "dictionary rows: {dict}");
    for row in rows {
        let name = row["Name"].as_str().unwrap_or_default();
        let field = &map["data"][name];
        assert_eq!(field["delimiter"], row["Delimiter"], "{name}: {got}");
        assert_eq!(field["piece"], row["Piece"], "{name}: {got}");
    }
    assert!(
        !got["storage"]["version"].as_str().unwrap_or("").is_empty(),
        "{got}"
    );
}

#[tokio::test]
#[ignore]
async fn read_a_missing_class_is_storage_map_not_found() {
    let r = rig(&Where::live(), Toolset::Merged).await;
    let got = read(&r, "IadSm172.NoSuchClass").await;
    assert_eq!(got["success"], false, "{got}");
    assert_eq!(got["error_code"], "STORAGE_MAP_NOT_FOUND", "{got}");
}

#[tokio::test]
#[ignore]
async fn read_loan_warns_that_a_delete_also_deletes_loan_events() {
    let r = rig(&Where::live(), Toolset::Merged).await;
    let loan = Fixture::read("Loan");
    let ev = Fixture::read("LoanEvent");
    load_fixture(&r, &loan).await;
    load_fixture(&r, &ev).await;
    let got = read(&r, "IadSm172.Loan").await;
    let child = read(&r, "IadSm172.LoanEvent").await;
    let p1 = drop_fixture(&r, &ev).await;
    let p2 = drop_fixture(&r, &loan).await;
    assert!(p1.is_empty() && p2.is_empty(), "{p1:?} {p2:?}");
    let facts = got["derived"].to_string();
    assert!(
        facts.contains("SQL DELETE") && facts.contains("IadSm172.LoanEvent"),
        "{got}"
    );
    let child_facts = child["derived"].to_string();
    assert!(child_facts.contains("IadSm172.Loan."), "{child}");
}

/// T044: `resolve_storage` returns a `%Storage.SQL` class's maps and keeps every key it had.
#[tokio::test]
#[ignore]
async fn resolve_storage_returns_sql_maps_and_keeps_its_keys() {
    let r = rig(&Where::live(), Toolset::Merged).await;
    let f = Fixture::read("Member");
    load_fixture(&r, &f).await;
    let got = r
        .call(
            "resolve_storage",
            json!({"class": "IadSm172.Member", "namespace": "USER"}),
        )
        .await;
    let problems = drop_fixture(&r, &f).await;
    assert!(problems.is_empty(), "{problems:?}");
    assert_eq!(got["success"], true, "{got}");
    let s = &got["storages"][0];
    for key in [
        "name",
        "type",
        "data_location",
        "id_location",
        "index_location",
    ] {
        assert!(s.get(key).is_some(), "{key} missing: {got}");
    }
    assert_eq!(s["type"], "%Storage.SQL", "{got}");
    assert_eq!(s["sql_maps"][0]["name"], "Master", "{got}");
    assert_eq!(s["sql_maps"][0]["global"], "^IadSm172Member", "{got}");
}

/// T048: `iris_storage_map action=diagram` returns Mermaid and JSON graph.
#[tokio::test]
#[ignore]
async fn diagram_returns_mermaid_and_graph() {
    let r = rig(&Where::live(), Toolset::Merged).await;
    let f = Fixture::read("Member");
    load_fixture(&r, &f).await;
    let got = r
        .call(
            "iris_storage_map",
            json!({"action": "diagram", "class": "IadSm172.Member", "namespace": "USER"}),
        )
        .await;
    let problems = drop_fixture(&r, &f).await;
    assert!(problems.is_empty(), "{problems:?}");
    assert_eq!(got["success"], true, "{got}");
    assert_eq!(got["action"], "diagram", "{got}");
    assert_eq!(got["class"], "IadSm172.Member", "{got}");

    // Check Mermaid output
    let mermaid = got["mermaid"].as_str().unwrap_or("");
    assert!(mermaid.contains("flowchart LR"), "{got}");
    assert!(mermaid.contains("^IadSm172Member"), "{got}");
    assert!(mermaid.contains("{ID}"), "{got}");

    // The Markdown document: the layout and a field row a person can read, from the live class.
    let md = got["markdown"].as_str().unwrap_or("");
    assert!(md.starts_with("# Storage of IadSm172.Member\n"), "{got}");
    assert!(md.contains("`^IadSm172Member({ID})`"), "{md}");
    assert!(
        md.contains("| `Age` | row node | `\"^\"` | `3` | delimited | yes |"),
        "{md}"
    );
    assert!(md.contains("```mermaid\n"), "{md}");

    // Check graph structure
    let graph = &got["graph"];
    assert!(graph["nodes"].is_array(), "{got}");
    assert!(graph["edges"].is_array(), "{got}");
    let nodes = graph["nodes"].as_array().unwrap();
    assert!(!nodes.is_empty(), "{got}");

    // Should have global, subscript, and data nodes at minimum
    let kinds: Vec<&str> = nodes.iter().filter_map(|n| n["kind"].as_str()).collect();
    assert!(kinds.contains(&"global"), "Missing global node: {got}");
    assert!(
        kinds.contains(&"subscript"),
        "Missing subscript node: {got}"
    );
    assert!(kinds.contains(&"data"), "Missing data node: {got}");
    // row_id node may or may not be present depending on the map
}

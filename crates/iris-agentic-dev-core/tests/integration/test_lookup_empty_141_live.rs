//! Spec 141 T196, live half: a lookup key whose value is empty exists, and deleting a key that
//! does not exist is `KEY_NOT_FOUND`.
//!
//! IRIS keeps a lookup value in `^Ens.LookupTable(table, key)`. `$GET` answers `""` both for a
//! key set to `""` and for a key never set, so only `$DATA` tells them apart. `get` used to test
//! the value and called a key set to `""` missing; `delete` of an absent key passed the raw
//! `%RemoveValue` status text through as `INTEROP_ERROR`.
//!
//! Against iris-dev-iris (USER is an Interoperability namespace). The table the tests make is
//! killed before and after each test.
//!
//! Run: `IRIS_HOST=localhost IRIS_WEB_PORT=52780 cargo test --features testing --test integration
//! test_lookup_empty_141 -- --include-ignored --test-threads=1`

use iris_agentic_dev_core::iris::config_sources::{ConfigSnapshot, LoadInputs, CONFIG_FILE_NAME};
use iris_agentic_dev_core::iris::connection::{DiscoverySource, IrisConnection};
use iris_agentic_dev_core::skills::SkillRegistry;
use iris_agentic_dev_core::tools::{IrisTools, Toolset};
use serde_json::{json, Value};

const TABLE: &str = "IadLookupEmpty141";

fn conn() -> IrisConnection {
    let host = std::env::var("IRIS_HOST").unwrap_or_else(|_| "localhost".into());
    let port = std::env::var("IRIS_WEB_PORT").unwrap_or_else(|_| "52780".into());
    let user = std::env::var("IRIS_USERNAME").unwrap_or_else(|_| "_SYSTEM".into());
    let pass = std::env::var("IRIS_PASSWORD").unwrap_or_else(|_| "SYS".into());
    IrisConnection::new(
        format!("http://{host}:{port}"),
        "USER",
        user,
        pass,
        DiscoverySource::EnvVar,
    )
}

/// Tools on the live connection with the write and destructive tiers declared in an operator file,
/// as `iris_lookup_manage set` and `delete` are destructive-tier.
fn tools() -> (IrisTools, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path().join("home");
    let cwd = dir.path().join("proj");
    std::fs::create_dir_all(&home).unwrap();
    std::fs::create_dir_all(&cwd).unwrap();
    std::fs::write(
        home.join(CONFIG_FILE_NAME),
        "write_tools_enabled = true\ndestructive_tools_enabled = true\n",
    )
    .unwrap();
    let inputs = LoadInputs {
        cwd: std::fs::canonicalize(&cwd).unwrap(),
        home: Some(std::fs::canonicalize(&home).unwrap()),
        ..Default::default()
    };
    let snapshot = ConfigSnapshot::load(&inputs);
    let declared = snapshot.declared_gates();
    let tools = IrisTools::with_registry_and_toolset(
        Some(conn()),
        SkillRegistry::new(),
        Toolset::Merged,
        None,
        None,
        true,
        declared,
    )
    .unwrap();
    tools.set_config(inputs, snapshot);
    (tools, dir)
}

async fn drop_table() {
    let client = IrisConnection::http_client().unwrap();
    conn()
        .execute_via_generator(
            &format!(r#"Kill ^Ens.LookupTable("{TABLE}") Write "OK""#),
            "USER",
            &client,
        )
        .await
        .expect("IRIS drops the test table");
}

async fn lookup(tools: &IrisTools, args: Value) -> Value {
    let r = tools
        .call_for_test("iris_lookup_manage", args)
        .await
        .unwrap_or_else(|e| panic!("iris_lookup_manage: {e}"));
    r.structured_content.unwrap_or(Value::Null)
}

fn act(action: &str, key: &str) -> Value {
    json!({"action": action, "table": TABLE, "key": key})
}

fn set(key: &str, value: &str) -> Value {
    json!({"action": "set", "table": TABLE, "key": key, "value": value})
}

fn code(b: &Value) -> &str {
    b["error_code"].as_str().unwrap_or("")
}

#[tokio::test]
#[ignore = "live IRIS on iris-dev-iris"]
async fn a_key_set_to_empty_is_found_and_listed() {
    drop_table().await;
    let (t, _d) = tools();
    let stored = lookup(&t, set("blank", "")).await;
    let other = lookup(&t, set("full", "v")).await;
    let got = lookup(&t, act("get", "blank")).await;
    let missing = lookup(&t, act("get", "never-set")).await;
    let keys = lookup(&t, json!({"action": "list_keys", "table": TABLE})).await;
    drop_table().await;

    assert_eq!(stored["success"], true, "{stored}");
    assert_eq!(other["success"], true, "{other}");
    assert_eq!(got["success"], true, "an empty value is a value: {got}");
    assert_eq!(got["value"], "", "{got}");
    assert_eq!(code(&missing), "KEY_NOT_FOUND", "{missing}");
    let listed: Vec<&str> = keys["keys"]
        .as_array()
        .unwrap_or_else(|| panic!("keys: {keys}"))
        .iter()
        .filter_map(Value::as_str)
        .collect();
    assert!(listed.contains(&"blank"), "list_keys shows it: {keys}");
}

#[tokio::test]
#[ignore = "live IRIS on iris-dev-iris"]
async fn delete_of_an_absent_key_is_key_not_found_and_of_a_present_key_works() {
    drop_table().await;
    let (t, _d) = tools();
    lookup(&t, set("blank", "")).await;
    lookup(&t, set("full", "v")).await;
    // IRIS drops a table when its last key goes, so one key stays to keep the table in place.
    lookup(&t, set("keep", "v")).await;
    let absent = lookup(&t, act("delete", "never-set")).await;
    let blank = lookup(&t, act("delete", "blank")).await;
    let full = lookup(&t, act("delete", "full")).await;
    let again = lookup(&t, act("delete", "full")).await;
    let after = lookup(&t, act("get", "blank")).await;
    let no_table = lookup(
        &t,
        json!({"action": "delete", "table": "IadLookupNoSuchTable141", "key": "k"}),
    )
    .await;
    drop_table().await;

    assert_eq!(code(&absent), "KEY_NOT_FOUND", "{absent}");
    assert_eq!(
        blank["success"], true,
        "an empty-valued key can be deleted: {blank}"
    );
    assert_eq!(full["success"], true, "{full}");
    assert_eq!(
        code(&again),
        "KEY_NOT_FOUND",
        "a deleted key is absent: {again}"
    );
    assert_eq!(code(&after), "KEY_NOT_FOUND", "{after}");
    assert_eq!(code(&no_table), "TABLE_NOT_FOUND", "{no_table}");
}

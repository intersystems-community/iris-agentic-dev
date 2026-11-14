//! Spec 172: each action's parameter refusals, against a live class. Every call is refused with
//! the error the caller can act on, and nothing changes: the class's storage version after the
//! calls is the one before.
//!
//! Run: `IRIS_HOST=localhost IRIS_WEB_PORT=52780 cargo test --features testing --test integration \
//!   test_storage_map_refusals_172 -- --test-threads=1 --include-ignored`

use crate::live_rig_171::{rig, Rig, Where};
use crate::storage172_rig::{drop_fixture, load_fixture, Fixture};
use iris_agentic_dev_core::tools::Toolset;
use serde_json::{json, Value};

async fn sm(r: &Rig, args: Value) -> Value {
    r.call("iris_storage_map", args).await
}

fn says(got: &Value, code: &str, text: &str) {
    assert_eq!(got["error_code"], code, "{got}");
    assert!(
        got["error"].as_str().unwrap_or("").contains(text),
        "expected {text:?} in {got}"
    );
}

#[tokio::test]
#[ignore = "live IRIS"]
async fn test_storage_map_refusals_172_bad_parameters_change_nothing() {
    let r = rig(&Where::live(), Toolset::Merged).await;
    let f = Fixture::read("Member");
    load_fixture(&r, &f).await;
    let read = sm(
        &r,
        json!({"action": "read", "class": f.class, "namespace": "USER"}),
    )
    .await;
    let version = read["storage"]["version"].clone();
    let master_version = read["storage"]["maps"][0]["version"].clone();

    let bad_name = sm(
        &r,
        json!({"action": "read", "class": "IadSm172.Not A Class", "namespace": "USER"}),
    )
    .await;
    let no_version = sm(
        &r,
        json!({"action": "edit", "class": f.class, "namespace": "USER",
               "ops": [{"op": "replace", "path": "/maps/Master/data/Name/piece", "value": "1"}]}),
    )
    .await;
    let no_ops = sm(
        &r,
        json!({"action": "edit", "class": f.class, "namespace": "USER", "version": version}),
    )
    .await;
    let no_such_map = sm(
        &r,
        json!({"action": "compare", "class": f.class, "namespace": "USER", "map": "Nope"}),
    )
    .await;
    let replace_missing = sm(
        &r,
        json!({"action": "replace", "class": f.class, "namespace": "USER", "map": "Nope",
               "version": master_version, "model": {"name": "Nope"}}),
    )
    .await;
    let propose_bare = sm(&r, json!({"action": "propose", "namespace": "USER"})).await;
    let propose_subscripted = sm(
        &r,
        json!({"action": "propose", "namespace": "USER", "global": "^IadSm172Member(1)"}),
    )
    .await;
    let propose_garbage = sm(
        &r,
        json!({"action": "propose", "namespace": "USER", "global": "^1bad"}),
    )
    .await;
    let uses_garbage = sm(
        &r,
        json!({"action": "uses", "namespace": "USER", "global": "^1bad"}),
    )
    .await;
    let after = sm(
        &r,
        json!({"action": "read", "class": f.class, "namespace": "USER"}),
    )
    .await["storage"]["version"]
        .clone();
    let problems = drop_fixture(&r, &f).await;

    assert_eq!(bad_name["success"], false, "{bad_name}");
    says(&no_version, "INVALID_PARAMS", "a write needs `version`");
    says(&no_ops, "INVALID_PARAMS", "edit needs `ops`");
    says(&no_such_map, "STORAGE_MAP_NOT_FOUND", "has no map Nope");
    says(
        &replace_missing,
        "STORAGE_MAP_NOT_FOUND",
        "add one with edit",
    );
    says(&propose_bare, "INVALID_PARAMS", "propose needs `global`");
    // A global name the dispatcher's name gate cannot read never reaches the action.
    for refused in [&propose_subscripted, &propose_garbage, &uses_garbage] {
        assert_eq!(refused["error_code"], "INVALID_GLOBAL_NAME", "{refused}");
    }
    assert_eq!(after, version, "a refused call changed the class");
    assert!(problems.is_empty(), "cleanup: {problems:?}");
}

/// A class with no Storage block (a registered object) has no map to read.
#[tokio::test]
#[ignore = "live IRIS"]
async fn test_storage_map_refusals_172_a_class_without_storage_has_no_map() {
    let r = rig(&Where::live(), Toolset::Merged).await;
    let class = "IadSm172.NoStorage";
    let doc = format!("{class}.cls");
    let put = r
        .call(
            "iris_doc",
            json!({"mode": "put", "name": doc, "namespace": "USER", "compile": true,
                   "content": format!("Class {class} Extends %RegisteredObject\n{{\n\nProperty Name As %String;\n\n}}\n")}),
        )
        .await;
    let got = sm(
        &r,
        json!({"action": "read", "class": class, "namespace": "USER"}),
    )
    .await;
    r.call(
        "iris_doc",
        json!({"mode": "delete", "name": doc, "namespace": "USER"}),
    )
    .await;

    assert_eq!(put["success"], true, "{put}");
    says(&got, "STORAGE_MAP_NOT_FOUND", "has no Storage block");
}

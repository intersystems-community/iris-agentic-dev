//! T059: every `iris_storage_map` action that writes, checks or samples calls the gates before
//! IRIS is touched (spec 172 US8, FR-026).
//!
//! For `check`, `preview`, `replace` (a `model` naming the global), `edit` (an op setting it) and
//! `propose` (the `global` param), a call that names `^ROUTINE` or `ROUTINE` is a fixed-rule
//! refusal, and the class source is the same before and after. `compare` takes no caller tree: it
//! reads the stored class, so its gate is covered by `gate_tree` in the unit tests.
//!
//! Red until the write actions exist (T086); the lead keeps it on the red list until then.
//!
//! Run: `IRIS_HOST=localhost IRIS_WEB_PORT=52780 cargo test --features testing --test integration \
//!   test_storage_map_gates_172 -- --test-threads=1 --include-ignored`

use crate::live_rig_171::{rig, Rig, Where};
use crate::storage172_rig::{drop_fixture, load_fixture, Fixture};
use iris_agentic_dev_core::tools::Toolset;
use serde_json::{json, Value};

const CLASS: &str = "IadSm172.Member";

async fn source(r: &Rig) -> Value {
    let got = r
        .call(
            "iris_doc",
            json!({"mode": "get", "name": format!("{CLASS}.cls"), "namespace": "USER"}),
        )
        .await;
    assert_eq!(got["success"], true, "{got}");
    got["content"].clone()
}

/// The calls under test, each naming `global` the way its action takes input.
fn calls(read: &Value, global: &str) -> Vec<(&'static str, Value)> {
    let mut map = read["storage"]["maps"][0].clone();
    map["global"] = json!(global);
    let version = read["storage"]["maps"][0]["version"].clone();
    vec![
        (
            "check",
            json!({"action": "check", "class": CLASS, "map": "Master", "model": map, "namespace": "USER"}),
        ),
        (
            "preview",
            json!({"action": "preview", "class": CLASS, "map": "Master", "model": map, "namespace": "USER"}),
        ),
        (
            "replace",
            json!({"action": "replace", "class": CLASS, "map": "Master", "version": version, "model": map, "namespace": "USER"}),
        ),
        (
            "edit",
            json!({"action": "edit", "class": CLASS, "version": version, "namespace": "USER",
                   "ops": [{"op": "set", "path": "/maps/Master/global", "value": global}]}),
        ),
        (
            "propose",
            json!({"action": "propose", "global": global, "namespace": "USER"}),
        ),
    ]
}

async fn assert_refused_and_untouched(global: &str) {
    let r = rig(&Where::live(), Toolset::Merged).await;
    let f = Fixture::read("Member");
    load_fixture(&r, &f).await;
    let read = r
        .call(
            "iris_storage_map",
            json!({"action": "read", "class": CLASS, "namespace": "USER"}),
        )
        .await;
    let before = source(&r).await;

    let mut failures = Vec::new();
    for (action, args) in calls(&read, global) {
        let got = r.call("iris_storage_map", args).await;
        if got["success"] != false || got["refusal"]["kind"] != "fixed_rule" {
            failures.push(format!(
                "{action} {global}: not a fixed-rule refusal: {got}"
            ));
        }
    }
    let after = source(&r).await;
    let problems = drop_fixture(&r, &f).await;

    assert_eq!(read["success"], true, "{read}");
    assert!(failures.is_empty(), "{failures:#?}");
    assert_eq!(
        before, after,
        "the class changed although every call was refused"
    );
    assert!(problems.is_empty(), "{problems:?}");
}

#[tokio::test]
#[ignore]
async fn routine_with_caret_is_refused_by_every_gated_action() {
    assert_refused_and_untouched("^ROUTINE").await;
}

#[tokio::test]
#[ignore]
async fn routine_without_caret_is_refused_by_every_gated_action() {
    assert_refused_and_untouched("ROUTINE").await;
}

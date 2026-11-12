//! T065: `iris_storage_map check` compiles a proposed map in a scratch copy and never touches
//! the real class (spec 172 US4: "a map that fails to compile leaves the real class and table
//! working").
//!
//! Run: `IRIS_HOST=localhost IRIS_WEB_PORT=52780 cargo test --features testing --test integration \
//!   test_storage_map_edit_172 -- --test-threads=1 --include-ignored`

use crate::live_rig_171::{rig, Rig, Where};
use crate::storage172_rig::{ask, drop_fixture, load_fixture, Fixture};
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

async fn exists(r: &Rig, class: &str) -> bool {
    r.call(
        "iris_doc",
        json!({"mode": "get", "name": format!("{class}.cls"), "namespace": "USER"}),
    )
    .await["success"]
        == true
}

async fn master(r: &Rig) -> Value {
    let read = r
        .call(
            "iris_storage_map",
            json!({"action": "read", "class": CLASS, "namespace": "USER"}),
        )
        .await;
    assert_eq!(read["success"], true, "{read}");
    read["storage"]["maps"][0].clone()
}

#[tokio::test]
#[ignore = "live IRIS"]
async fn test_storage_map_edit_172_check_failure_is_explained_and_leaves_the_class() {
    let r = rig(&Where::live(), Toolset::Merged).await;
    let f = Fixture::read("Member");
    load_fixture(&r, &f).await;
    let before = source(&r).await;
    let mut bad = master(&r).await;
    bad["type"] = json!("DATA");

    let got = r
        .call(
            "iris_storage_map",
            json!({"action": "check", "class": CLASS, "map": "Master", "model": bad,
                   "namespace": "USER"}),
        )
        .await;
    let after = source(&r).await;
    let rows = ask(&r, &f).await;
    let left = match got["scratch_class"].as_str() {
        Some(c) => exists(&r, c).await,
        None => false,
    };
    let problems = drop_fixture(&r, &f).await;

    assert_eq!(got["success"], false, "{got}");
    assert_eq!(got["error_code"], "STORAGE_MAP_COMPILE_FAILED", "{got}");
    let errors = got["iris_errors"].to_string();
    assert!(errors.contains("5511"), "no #5511 in {got}");
    let ex = &got["explanations"][0];
    assert_eq!(ex["code"], 5511, "{got}");
    assert!(
        ex["text"]
            .as_str()
            .unwrap_or_default()
            .contains("case-sensitive"),
        "{got}"
    );
    assert!(!left, "the scratch class stayed: {got}");
    assert_eq!(after, before, "check changed the real class");
    let expected = f.seed["answer"].as_array().cloned().unwrap_or_default();
    assert_eq!(
        rows, expected,
        "the real class no longer answers its question"
    );
    assert!(problems.is_empty(), "cleanup: {problems:?}");
}

#[tokio::test]
#[ignore = "live IRIS"]
async fn test_storage_map_edit_172_check_success_leaves_no_scratch_class() {
    let r = rig(&Where::live(), Toolset::Merged).await;
    let f = Fixture::read("Member");
    load_fixture(&r, &f).await;
    let before = source(&r).await;
    let mut good = master(&r).await;
    // A real change: Age moves to piece 4, which compiles.
    good["data"]["Age"]["piece"] = json!("4");

    let with_model = r
        .call(
            "iris_storage_map",
            json!({"action": "check", "class": CLASS, "map": "Master", "model": good,
                   "namespace": "USER"}),
        )
        .await;
    let stored = r
        .call(
            "iris_storage_map",
            json!({"action": "check", "class": CLASS, "namespace": "USER"}),
        )
        .await;
    let mut left = Vec::new();
    for got in [&with_model, &stored] {
        if let Some(c) = got["scratch_class"].as_str() {
            if exists(&r, c).await {
                left.push(c.to_string());
            }
        }
    }
    let after = source(&r).await;
    let rows = ask(&r, &f).await;
    let problems = drop_fixture(&r, &f).await;

    for got in [&with_model, &stored] {
        assert_eq!(got["success"], true, "{got}");
        assert_eq!(got["action"], "check", "{got}");
        assert_eq!(got["compiled"], true, "{got}");
        assert!(
            got["scratch_class"]
                .as_str()
                .is_some_and(|c| c.starts_with("IrisDevTmp.Sm")),
            "{got}"
        );
    }
    assert!(left.is_empty(), "scratch classes stayed: {left:?}");
    assert_eq!(after, before, "check changed the real class");
    let expected = f.seed["answer"].as_array().cloned().unwrap_or_default();
    assert_eq!(rows, expected);
    assert!(problems.is_empty(), "cleanup: {problems:?}");
}

#[tokio::test]
#[ignore = "live IRIS"]
async fn test_storage_map_edit_172_check_refuses_bad_params() {
    let r = rig(&Where::live(), Toolset::Merged).await;
    let f = Fixture::read("Member");
    load_fixture(&r, &f).await;
    let good = master(&r).await;
    let mut unknown = good.clone();
    unknown["wibble"] = json!("x");

    let mismatch = r
        .call(
            "iris_storage_map",
            json!({"action": "check", "class": CLASS, "map": "Other", "model": good,
                   "namespace": "USER"}),
        )
        .await;
    let bad_key = r
        .call(
            "iris_storage_map",
            json!({"action": "check", "class": CLASS, "model": unknown, "namespace": "USER"}),
        )
        .await;
    let default_storage = {
        let book = Fixture::read("DefaultBook");
        load_fixture(&r, &book).await;
        let got = r
            .call(
                "iris_storage_map",
                json!({"action": "check", "class": book.class, "namespace": "USER"}),
            )
            .await;
        drop_fixture(&r, &book).await;
        got
    };
    let problems = drop_fixture(&r, &f).await;

    assert_eq!(mismatch["error_code"], "INVALID_PARAMS", "{mismatch}");
    assert_eq!(bad_key["error_code"], "INVALID_PARAMS", "{bad_key}");
    assert!(bad_key["error"].to_string().contains("wibble"), "{bad_key}");
    assert_eq!(
        default_storage["error_code"], "STORAGE_MAP_READ_ONLY",
        "{default_storage}"
    );
    assert!(problems.is_empty(), "cleanup: {problems:?}");
}

/// T066: under `read_only = "strict"` every action that compiles, writes or samples is refused
/// with `READ_ONLY_STRICT`, and `read`, `diagram` and `uses` still answer.
#[tokio::test]
#[ignore = "live IRIS"]
async fn test_storage_map_edit_172_strict_refuses_writes_and_keeps_reads() {
    use crate::live_rig_171::rig_with;
    let open = rig(&Where::live(), Toolset::Merged).await;
    let f = Fixture::read("Member");
    load_fixture(&open, &f).await;
    let map = master(&open).await;
    let version = map["version"].clone();
    let strict = rig_with(
        &Where::live(),
        Toolset::Merged,
        "write_tools_enabled = true\ndestructive_tools_enabled = true\nread_only = \"strict\"\n",
    )
    .await;

    let refused = [
        json!({"action": "check", "class": CLASS, "namespace": "USER"}),
        json!({"action": "edit", "class": CLASS, "version": version, "namespace": "USER",
               "ops": [{"op": "set", "path": "/maps/Master/data/Age/piece", "value": "4"}]}),
        json!({"action": "replace", "class": CLASS, "map": "Master", "version": version,
               "model": map, "namespace": "USER"}),
        json!({"action": "preview", "class": CLASS, "map": "Master", "model": map,
               "namespace": "USER"}),
        json!({"action": "compare", "class": CLASS, "map": "Master", "namespace": "USER"}),
        json!({"action": "propose", "global": "^IadSm172Member", "namespace": "USER"}),
    ];
    let answered = [
        json!({"action": "read", "class": CLASS, "namespace": "USER"}),
        json!({"action": "diagram", "class": CLASS, "namespace": "USER"}),
        json!({"action": "uses", "global": "^IadSm172Member", "namespace": "USER"}),
    ];
    let mut failures = Vec::new();
    for args in refused {
        let got = strict.call("iris_storage_map", args.clone()).await;
        if got["error_code"] != "READ_ONLY_STRICT" {
            failures.push(format!("{}: not refused: {got}", args["action"]));
        }
    }
    for args in answered {
        let got = strict.call("iris_storage_map", args.clone()).await;
        if got["success"] != true {
            failures.push(format!("{}: did not answer: {got}", args["action"]));
        }
    }
    let problems = drop_fixture(&open, &f).await;
    assert!(failures.is_empty(), "{}", failures.join("\n"));
    assert!(problems.is_empty(), "cleanup: {problems:?}");
}

//! Spec 141 T218, live: `iris_test` runs on an instance that never set `^UnitTestRoot`.
//!
//! A fresh container has no root, and `%UnitTest.Manager` then answers `ERROR #5007` and runs
//! nothing. 1.6.4 hid that by pointing the global at a shared `/tmp/httest/`; T218 first dropped
//! that and so broke every fresh instance. Now a root the instance has is used as it is, so tests
//! staged under it run, and only an instance with none gets a root of its own for the run, under
//! `<mgr>/Temp`, removed afterwards with the old value (none) put back.
//!
//! What `RunTest` runs is decided by the files in `<root>/<suite>/`, not by the compiled classes:
//! measured on 2025.3 and 2026.2, a class that exists only in the database is not run by any
//! spec form under `/noload`. That gap predates 141 and is not tested here.
//!
//! The server and the container come from the environment, so the same test runs on a fresh
//! CI-like container and on iris-dev-iris:
//!
//! ```text
//! IRIS_HOST=localhost IRIS_WEB_PORT=52780 IRIS_CONTAINER=iris-dev-iris \
//! lockf -k /tmp/iad-141-iris.lock cargo test --features testing --test integration \
//!     unit_test_root_141 -- --ignored --test-threads=1
//! ```

use iris_agentic_dev_core::iris::connection::{DiscoverySource, IrisConnection};
use iris_agentic_dev_core::testing::LiveTarget;
use iris_agentic_dev_core::tools::{IrisTools, Toolset};
use serde_json::{json, Value};

const CLASS: &str = "IadRoot141.T";
const SOURCE: &str = "Class IadRoot141.T Extends %UnitTest.TestCase\n{\n\nMethod TestRuns()\n{\n    Set ^IadRoot141Ran = $Get(^IadRoot141Ran) + 1\n    Do $$$AssertEquals(1, 1)\n}\n\n}\n";

fn target() -> LiveTarget {
    LiveTarget::from_env()
}

fn session(code: &str) -> String {
    target().session("USER", code)
}

/// `^UnitTestRoot` as `D:<$Data>|V:<value>`, and the count of `iad-*` run roots under `<mgr>/Temp`.
fn root_state() -> (String, String) {
    let out = session(
        "Write \"ROOT[\",$Data(^UnitTestRoot)#2,\"|\",$Get(^UnitTestRoot),\"]\",! Set n=0,d=##class(%File).NormalizeDirectory($System.Util.ManagerDirectory()_\"Temp\"),rs=##class(%ResultSet).%New(\"%File:FileSet\") Do rs.Execute(d,\"iad-*\",,1) While rs.Next() { Set n=n+1 } Write \"RUNS[\",n,\"]\",!",
    );
    let pick = |tag: &str| {
        out.split(&format!("{tag}["))
            .nth(1)
            .and_then(|s| s.split(']').next())
            .unwrap_or_else(|| panic!("no {tag} in: {out}"))
            .to_string()
    };
    (pick("ROOT"), pick("RUNS"))
}

fn body(r: Result<rmcp::model::CallToolResult, String>) -> Value {
    let r = r.expect("the tool answers");
    if let Some(v) = r.structured_content {
        return v;
    }
    let text = r.content[0].as_text().unwrap().text.clone();
    serde_json::from_str(&text).unwrap_or_else(|e| panic!("not JSON ({e}): {text}"))
}

async fn put_and_compile(t: &LiveTarget) -> Value {
    let client = reqwest::Client::new();
    let base = t.base_url();
    let put = client
        .put(format!("{base}/api/atelier/v1/USER/doc/{CLASS}.cls"))
        .basic_auth(&t.username, Some(&t.password))
        .json(&json!({"enc": false, "content": SOURCE.lines().collect::<Vec<_>>()}))
        .send()
        .await
        .expect("PUT");
    assert!(put.status().is_success(), "PUT: {}", put.status());
    client
        .post(format!(
            "{base}/api/atelier/v1/USER/action/compile?flags=cuk"
        ))
        .basic_auth(&t.username, Some(&t.password))
        .json(&json!([format!("{CLASS}.cls")]))
        .send()
        .await
        .expect("compile")
        .json()
        .await
        .unwrap()
}

fn tools(t: &LiveTarget) -> IrisTools {
    let conn = IrisConnection::new(
        t.base_url(),
        "USER",
        &t.username,
        &t.password,
        DiscoverySource::EnvVar,
    );
    IrisTools::for_test_gates_open(Some(conn), Toolset::Merged).unwrap()
}

/// Put the instance's root back: `saved` is `root_state`'s `D|V`.
fn restore_root(saved: &str) {
    session(&format!(
        "Kill ^UnitTestRoot,^IadRoot141Ran Do $SYSTEM.OBJ.Delete(\"{CLASS}\",\"/displaylog=0\") Do ##class(%File).RemoveDirectoryTree(\"/tmp/iadroot141/\")"
    ));
    if let Some(value) = saved.strip_prefix("1|") {
        session(&format!(
            "Set ^UnitTestRoot=\"{}\"",
            value.replace('"', "\"\"")
        ));
    }
}

#[tokio::test]
#[ignore = "needs a live IRIS (IRIS_HOST, IRIS_WEB_PORT, IRIS_CONTAINER)"]
async fn a_fresh_instance_gets_a_root_for_the_run_and_none_afterwards() {
    let t = target();
    t.assert_running();
    let (saved, _) = root_state();
    session("Kill ^UnitTestRoot");
    let (before, runs_before) = root_state();
    let compiled = put_and_compile(&t).await;
    let answer = body(
        tools(&t)
            .call_gated(
                "iris_test",
                json!({"pattern": "IadRoot141", "namespace": "USER"}),
            )
            .await,
    );
    let (after, runs_after) = root_state();
    restore_root(&saved);

    assert!(
        before.starts_with("0|"),
        "the root must start unset: {before}"
    );
    assert!(
        compiled["status"]["errors"]
            .as_array()
            .is_some_and(|e| e.is_empty()),
        "{compiled}"
    );
    // RunTest found a root: no `Finding directories: ERROR #5007` diagnosis in the answer.
    assert!(
        !answer.to_string().contains("which is not a directory"),
        "the run had no usable root: {answer}"
    );
    assert!(
        after.starts_with("0|"),
        "the run must put back no root: {after}"
    );
    assert_eq!(
        runs_after, runs_before,
        "a run root was left under <mgr>/Temp"
    );
}

#[tokio::test]
#[ignore = "needs a live IRIS (IRIS_HOST, IRIS_WEB_PORT, IRIS_CONTAINER)"]
async fn a_root_the_instance_has_is_used_and_its_staged_tests_run() {
    let t = target();
    t.assert_running();
    let (saved, _) = root_state();
    // Stage the test under a root of the operator's choosing, as CI stages its suite.
    let staged = session(&format!(
        "Set d=\"/tmp/iadroot141/IadRoot141/\" Do ##class(%File).CreateDirectoryChain(d) Set f=##class(%Stream.FileCharacter).%New(),f.Filename=d_\"T.cls\" Do f.Write(\"{}\") Write \"STAGED:\",$System.Status.IsOK(f.%Save()),! Set ^UnitTestRoot=\"/tmp/iadroot141/\" Kill ^IadRoot141Ran",
        SOURCE.replace('"', "\"\"").replace('\n', "\"_$C(10)_\"")
    ));
    let compiled = put_and_compile(&t).await;
    let answer = body(
        tools(&t)
            .call_gated(
                "iris_test",
                json!({"pattern": "IadRoot141", "namespace": "USER"}),
            )
            .await,
    );
    let ran = session("Write \"RAN[\",$Get(^IadRoot141Ran),\"]\",!");
    let (after, _) = root_state();
    restore_root(&saved);

    assert!(staged.contains("STAGED:1"), "could not stage: {staged}");
    assert!(
        compiled["status"]["errors"]
            .as_array()
            .is_some_and(|e| e.is_empty()),
        "{compiled}"
    );
    assert_eq!(answer["success"], true, "{answer}");
    assert!(
        ran.contains("RAN[1]"),
        "the staged test did not run: {ran} {answer}"
    );
    assert_eq!(
        after, "1|/tmp/iadroot141/",
        "the instance's root must be left as it was"
    );
}

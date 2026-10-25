//! Spec 141 C8, live half: `journal_search` withholds the value of a blocklisted global.
//!
//! Against iris-dev-iris: `Set ^IadV6Secret141` is journaled, and the search returns its record.
//! With the server blocklist holding `^IadV6Secret*` the value is dropped and `redacted` is true.
//! The control search, with no blocklist entry, returns the value, so the redaction is the
//! blocklist's doing and not a journal that never held the value.
//!
//! Run: `IRIS_HOST=localhost IRIS_WEB_PORT=52780 IRIS_USERNAME=_SYSTEM IRIS_PASSWORD=SYS
//! cargo test --features testing --test integration test_journal_redact_141 -- --include-ignored
//! --test-threads=1`

use iris_agentic_dev_core::iris::connection::{DiscoverySource, IrisConnection};
use iris_agentic_dev_core::tools::global::{handle_iris_global, IrisGlobalParams};
use iris_agentic_dev_core::tools::observability::journal_search_impl;

const GLOBAL: &str = "IadV6Secret141";
const VALUE: &str = "secret-141";

fn make_conn() -> Option<(IrisConnection, reqwest::Client)> {
    let iris_host = std::env::var("IRIS_HOST").unwrap_or_default();
    if iris_host.is_empty() {
        return None;
    }
    let web_port = std::env::var("IRIS_WEB_PORT").unwrap_or_else(|_| "52780".to_string());
    let username = std::env::var("IRIS_USERNAME").unwrap_or_else(|_| "_SYSTEM".to_string());
    let password = std::env::var("IRIS_PASSWORD").unwrap_or_else(|_| "SYS".to_string());
    let conn = IrisConnection::new(
        format!("http://{}:{}", iris_host, web_port),
        "USER",
        username,
        password,
        DiscoverySource::EnvVar,
    );
    Some((conn, reqwest::Client::new()))
}

fn global_params(action: &str, value: Option<&str>) -> IrisGlobalParams {
    IrisGlobalParams {
        confirm: false,
        action: action.to_string(),
        global_name: GLOBAL.to_string(),
        subscripts: None,
        value: value.map(str::to_string),
        namespace: None,
        subtree: None,
        max_nodes: None,
        max_subscripts: None,
        acknowledge_phi: None,
        server: None,
    }
}

/// The journal record for `GLOBAL` in a search, or `None` when the search did not return one.
fn record_for_global(v: &serde_json::Value) -> Option<serde_json::Value> {
    v["records"]
        .as_array()?
        .iter()
        // The SET this test made, not a KILL left in the journal by an earlier run.
        .filter(|r| r["global_ref"].as_str().unwrap_or("").contains(GLOBAL))
        .rfind(|r| {
            r["operation"]
                .as_str()
                .unwrap_or("")
                .eq_ignore_ascii_case("SET")
        })
        .cloned()
}

fn body(result: rmcp::model::CallToolResult) -> serde_json::Value {
    let text = result
        .content
        .first()
        .map(|c| c.as_text().unwrap().text.clone())
        .expect("text content");
    serde_json::from_str(&text).unwrap_or_else(|e| panic!("not JSON ({e}): {text}"))
}

#[tokio::test]
#[ignore]
async fn journal_search_withholds_a_blocklisted_value() {
    let Some((conn, client)) = make_conn() else {
        eprintln!("IRIS_HOST not set; skipping");
        return;
    };
    let set = handle_iris_global(&conn, &client, &global_params("set", Some(VALUE)), Ok(())).await;
    assert_eq!(set["success"], true, "set failed: {set}");

    // Control: no server blocklist entry, so the journaled value comes back.
    let control = body(
        journal_search_impl(
            Some(&conn),
            "allow",
            Some(GLOBAL),
            None,
            Some(50),
            &[],
            false,
        )
        .await
        .expect("Ok"),
    );
    let control_rec = record_for_global(&control);

    let blocklist = vec![format!("^{GLOBAL}*")];
    let guarded = body(
        journal_search_impl(
            Some(&conn),
            "allow",
            Some(GLOBAL),
            None,
            Some(50),
            &blocklist,
            false,
        )
        .await
        .expect("Ok"),
    );
    let guarded_rec = record_for_global(&guarded);

    handle_iris_global(&conn, &client, &global_params("kill", None), Ok(())).await;

    let control_rec = control_rec.unwrap_or_else(|| panic!("no journal record: {control}"));
    assert_eq!(
        control_rec["value"], VALUE,
        "control lost the value: {control_rec}"
    );
    let guarded_rec = guarded_rec.unwrap_or_else(|| panic!("no journal record: {guarded}"));
    assert_eq!(
        guarded_rec["value"], "",
        "the value was returned: {guarded_rec}"
    );
    assert_eq!(guarded_rec["redacted"], true, "{guarded_rec}");
}

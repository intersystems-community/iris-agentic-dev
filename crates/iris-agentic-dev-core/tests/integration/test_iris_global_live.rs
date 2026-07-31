//! Live integration tests for iris_global tool.
//!
//! All tests are `#[ignore]` — run with:
//!   IRIS_HOST=localhost IRIS_WEB_PORT=52780 \
//!   cargo test --test test_iris_global_live -- --ignored --nocapture
//!
//! Uses globals under `IrisDevTest.*` namespace to avoid polluting production globals.

use iris_agentic_dev_core::iris::connection::{DiscoverySource, IrisConnection};
use iris_agentic_dev_core::tools::global::{handle_iris_global, IrisGlobalParams};

// ── Helpers ───────────────────────────────────────────────────────────────────

fn make_conn() -> Option<(IrisConnection, reqwest::Client)> {
    let iris_host = std::env::var("IRIS_HOST").unwrap_or_default();
    if iris_host.is_empty() {
        return None;
    }
    let web_port = std::env::var("IRIS_WEB_PORT").unwrap_or_else(|_| "52780".to_string());
    let username = std::env::var("IRIS_USERNAME").unwrap_or_else(|_| "_SYSTEM".to_string());
    let password = std::env::var("IRIS_PASSWORD").unwrap_or_else(|_| "SYS".to_string());
    let base_url = format!("http://{}:{}", iris_host, web_port);
    let conn = IrisConnection::new(
        base_url,
        "USER",
        username,
        password,
        DiscoverySource::EnvVar,
    );
    let client = reqwest::Client::new();
    Some((conn, client))
}

fn make_params(action: &str, global_name: &str) -> IrisGlobalParams {
    IrisGlobalParams {
        action: action.to_string(),
        global_name: global_name.to_string(),
        subscripts: None,
        value: None,
        namespace: None,
        subtree: None,
        max_nodes: None,
        max_subscripts: None,
        acknowledge_phi: None,
        server: None,
    }
}

// ── T019: get round-trip ─────────────────────────────────────────────────────

#[tokio::test]
#[ignore]
async fn test_get_defined_and_absent() {
    let Some((conn, client)) = make_conn() else {
        eprintln!("IRIS_HOST not set — skipping");
        return;
    };

    // Set a known value via execute_via_generator directly
    let set_code = r#" Set ^IrisDevTest.GlobalT019 = "hello-052"
 Write "{""success"":true}",$C(10)"#;
    let _ = conn.execute_via_generator(set_code, "USER", &client).await;

    // get — defined
    let p = IrisGlobalParams {
        action: "get".to_string(),
        global_name: "IrisDevTest.GlobalT019".to_string(),
        subscripts: None,
        value: None,
        namespace: None,
        subtree: None,
        max_nodes: None,
        max_subscripts: None,
        acknowledge_phi: None,
        server: None,
    };
    let result = handle_iris_global(&conn, &client, &p, Ok(())).await;
    assert_eq!(result["success"], true, "get failed: {result}");
    assert_eq!(result["defined"], true, "expected defined=true: {result}");
    assert_eq!(result["value"], "hello-052", "value mismatch: {result}");

    // get — absent node
    let p2 = IrisGlobalParams {
        action: "get".to_string(),
        global_name: "IrisDevTest.GlobalT019Absent12345".to_string(),
        ..p.clone()
    };
    let result2 = handle_iris_global(&conn, &client, &p2, Ok(())).await;
    assert_eq!(result2["success"], true, "absent get failed: {result2}");
    assert_eq!(
        result2["defined"], false,
        "expected defined=false: {result2}"
    );

    // Cleanup
    let kill_code = r#" Kill ^IrisDevTest.GlobalT019
 Write "{""success"":true}",$C(10)"#;
    let _ = conn.execute_via_generator(kill_code, "USER", &client).await;
}

// ── T026: set → get round-trip ───────────────────────────────────────────────

#[tokio::test]
#[ignore]
async fn test_set_and_get_roundtrip() {
    let Some((conn, client)) = make_conn() else {
        eprintln!("IRIS_HOST not set — skipping");
        return;
    };

    // set
    let set_params = IrisGlobalParams {
        action: "set".to_string(),
        global_name: "IrisDevTest.GlobSet".to_string(),
        subscripts: Some(vec!["u1".to_string()]),
        value: Some("val42".to_string()),
        namespace: None,
        subtree: None,
        max_nodes: None,
        max_subscripts: None,
        acknowledge_phi: None,
        server: None,
    };
    let set_result = handle_iris_global(&conn, &client, &set_params, Ok(())).await;
    assert_eq!(set_result["success"], true, "set failed: {set_result}");

    // get to verify
    let get_params = IrisGlobalParams {
        action: "get".to_string(),
        global_name: "IrisDevTest.GlobSet".to_string(),
        subscripts: Some(vec!["u1".to_string()]),
        value: None,
        namespace: None,
        subtree: None,
        max_nodes: None,
        max_subscripts: None,
        acknowledge_phi: None,
        server: None,
    };
    let get_result = handle_iris_global(&conn, &client, &get_params, Ok(())).await;
    assert_eq!(
        get_result["success"], true,
        "get after set failed: {get_result}"
    );
    assert_eq!(get_result["value"], "val42", "value mismatch: {get_result}");

    // cleanup
    let mut kill = make_params("kill", "IrisDevTest.GlobSet");
    kill.subscripts = Some(vec!["u1".to_string()]);
    let _ = handle_iris_global(&conn, &client, &kill, Ok(())).await;
}

// ── T032: kill removes subtree ────────────────────────────────────────────────

#[tokio::test]
#[ignore]
async fn test_kill_removes_subtree() {
    let Some((conn, client)) = make_conn() else {
        eprintln!("IRIS_HOST not set — skipping");
        return;
    };

    // Set three nodes
    for k in &["a", "b", "c"] {
        let p = IrisGlobalParams {
            action: "set".to_string(),
            global_name: "IrisDevTest.GlobKill".to_string(),
            subscripts: Some(vec![k.to_string()]),
            value: Some("x".to_string()),
            namespace: None,
            subtree: None,
            max_nodes: None,
            max_subscripts: None,
            acknowledge_phi: None,
            server: None,
        };
        let r = handle_iris_global(&conn, &client, &p, Ok(())).await;
        assert_eq!(r["success"], true, "set {k} failed: {r}");
    }

    // Kill the root
    let kill = make_params("kill", "IrisDevTest.GlobKill");
    let kr = handle_iris_global(&conn, &client, &kill, Ok(())).await;
    assert_eq!(kr["success"], true, "kill failed: {kr}");

    // Verify all three are gone
    for k in &["a", "b", "c"] {
        let get = IrisGlobalParams {
            action: "get".to_string(),
            global_name: "IrisDevTest.GlobKill".to_string(),
            subscripts: Some(vec![k.to_string()]),
            value: None,
            namespace: None,
            subtree: None,
            max_nodes: None,
            max_subscripts: None,
            acknowledge_phi: None,
            server: None,
        };
        let gr = handle_iris_global(&conn, &client, &get, Ok(())).await;
        assert_eq!(gr["defined"], false, "expected {k} gone after kill: {gr}");
    }
}

// ── T038: list returns subscripts ─────────────────────────────────────────────

#[tokio::test]
#[ignore]
async fn test_list_subscripts() {
    let Some((conn, client)) = make_conn() else {
        eprintln!("IRIS_HOST not set — skipping");
        return;
    };

    // Set 5 nodes
    for i in 1..=5 {
        let p = IrisGlobalParams {
            action: "set".to_string(),
            global_name: "IrisDevTest.GlobList".to_string(),
            subscripts: Some(vec![format!("k{i}")]),
            value: Some(format!("v{i}")),
            namespace: None,
            subtree: None,
            max_nodes: None,
            max_subscripts: None,
            acknowledge_phi: None,
            server: None,
        };
        let r = handle_iris_global(&conn, &client, &p, Ok(())).await;
        assert_eq!(r["success"], true, "set k{i} failed: {r}");
    }

    // list — all 5
    let list = make_params("list", "IrisDevTest.GlobList");
    let lr = handle_iris_global(&conn, &client, &list, Ok(())).await;
    assert_eq!(lr["success"], true, "list failed: {lr}");
    let subs = lr["subscripts"].as_array().expect("subscripts array");
    assert_eq!(subs.len(), 5, "expected 5 subscripts: {lr}");
    assert_eq!(lr["truncated"], false, "should not be truncated: {lr}");

    // list with max_subscripts=2 → truncated
    let list2 = IrisGlobalParams {
        action: "list".to_string(),
        global_name: "IrisDevTest.GlobList".to_string(),
        max_subscripts: Some(2),
        subscripts: None,
        value: None,
        namespace: None,
        subtree: None,
        max_nodes: None,
        acknowledge_phi: None,
        server: None,
    };
    let lr2 = handle_iris_global(&conn, &client, &list2, Ok(())).await;
    assert_eq!(lr2["success"], true, "list(2) failed: {lr2}");
    let subs2 = lr2["subscripts"].as_array().expect("subscripts2");
    assert_eq!(subs2.len(), 2, "expected 2: {lr2}");
    assert_eq!(lr2["truncated"], true, "expected truncated: {lr2}");

    // cleanup
    let kill = make_params("kill", "IrisDevTest.GlobList");
    let _ = handle_iris_global(&conn, &client, &kill, Ok(())).await;
}

// ── T041: PHI gate blocks PAPMI without acknowledgePhi ───────────────────────

#[tokio::test]
#[ignore]
async fn test_phi_gate_blocks_papmi() {
    use iris_agentic_dev_core::iris::workspace_config::{ConnectionPolicy, DataPolicy};
    use iris_agentic_dev_core::policy::gate::dispatch_gate;

    let policy = ConnectionPolicy {
        server_name: "iris-dev-iris".to_string(),
        allow: None,
        mcp_template: None,
        data_policy: Some(DataPolicy::Allow),
        global_blocklist: vec![],
        data_policy_kill_allowlist: vec![],
    };

    let params_no_ack = serde_json::json!({"action": "get", "global_name": "PAPMI"});
    let blocked = dispatch_gate(
        "iris_global",
        "iris-dev-iris",
        Some(&policy),
        &params_no_ack,
    );
    assert!(blocked.is_err(), "PAPMI without ack must block");
    assert_eq!(blocked.unwrap_err()["error_code"], "PHI_GATE_BLOCKED");

    let params_ack =
        serde_json::json!({"action": "get", "global_name": "PAPMI", "acknowledgePhi": true});
    let allowed = dispatch_gate("iris_global", "iris-dev-iris", Some(&policy), &params_ack);
    assert!(
        allowed.is_ok(),
        "PAPMI with ack must pass gate: {:?}",
        allowed
    );
}

// ── T042: system blocklist blocks ^%SYS even with dataPolicy=allow ───────────

#[tokio::test]
#[ignore]
async fn test_system_blocklist_blocks_pct_sys_live() {
    use iris_agentic_dev_core::iris::workspace_config::{ConnectionPolicy, DataPolicy};
    use iris_agentic_dev_core::policy::gate::dispatch_gate;

    let policy = ConnectionPolicy {
        server_name: "iris-dev-iris".to_string(),
        allow: None,
        mcp_template: None,
        data_policy: Some(DataPolicy::Allow),
        global_blocklist: vec![],
        data_policy_kill_allowlist: vec![],
    };

    let params = serde_json::json!({"action": "get", "global_name": "%SYS"});
    let result = dispatch_gate("iris_global", "iris-dev-iris", Some(&policy), &params);
    assert!(result.is_err(), "^%SYS must be blocked");
    assert_eq!(result.unwrap_err()["error_code"], "SYSTEM_BLOCKLIST");
}

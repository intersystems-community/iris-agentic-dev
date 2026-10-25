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
        confirm: false,
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
        confirm: false,
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
        confirm: false,
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
        confirm: false,
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
            confirm: false,
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
            confirm: false,
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
            confirm: false,
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
        confirm: false,
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
    use iris_agentic_dev_core::testing::gated;

    let policy = ConnectionPolicy {
        server_name: "iris-dev-iris".to_string(),
        allow: None,
        mcp_template: None,
        data_policy: Some(DataPolicy::Allow),
        global_blocklist: vec![],
        data_policy_kill_allowlist: vec![],
        iris_audit: false,
    };

    let params_no_ack = serde_json::json!({"action": "get", "global_name": "PAPMI"});
    let blocked = gated::dispatch(
        "iris_global",
        "iris-dev-iris",
        Some(&policy),
        &params_no_ack,
    );
    assert!(blocked.is_err(), "PAPMI without ack must block");
    assert_eq!(blocked.unwrap_err()["error_code"], "PHI_GATE_BLOCKED");

    let params_ack =
        serde_json::json!({"action": "get", "global_name": "PAPMI", "acknowledgePhi": true});
    let allowed = gated::dispatch("iris_global", "iris-dev-iris", Some(&policy), &params_ack);
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
    use iris_agentic_dev_core::testing::gated;

    let policy = ConnectionPolicy {
        server_name: "iris-dev-iris".to_string(),
        allow: None,
        mcp_template: None,
        data_policy: Some(DataPolicy::Allow),
        global_blocklist: vec![],
        data_policy_kill_allowlist: vec![],
        iris_audit: false,
    };

    let params = serde_json::json!({"action": "get", "global_name": "%SYS"});
    let result = gated::dispatch("iris_global", "iris-dev-iris", Some(&policy), &params);
    assert!(result.is_err(), "^%SYS must be blocked");
    assert_eq!(result.unwrap_err()["error_code"], "SYSTEM_BLOCKLIST");
}

// ── #146: get with subtree under a subscripted reference ─────────────────────

async fn run_in_user(conn: &IrisConnection, client: &reqwest::Client, code: &str) {
    let _ = conn.execute_via_generator(code, "USER", client).await;
}

#[tokio::test]
#[ignore]
async fn test_get_subtree_below_a_subscripted_node_returns_its_descendants() {
    let Some((conn, client)) = make_conn() else {
        eprintln!("IRIS_HOST not set — skipping");
        return;
    };
    // The data the reporter had: a settings node with children and a grandchild, plus a sibling
    // whose last subscript starts with the same letters and must not come back.
    run_in_user(
        &conn,
        &client,
        r#" Kill ^IrisDevTest.G146
 Set ^IrisDevTest.G146("SourceControl","Git","settings","user")="tdyar"
 Set ^IrisDevTest.G146("SourceControl","Git","settings","remote","url")="git@example"
 Set ^IrisDevTest.G146("SourceControl","Git","settingsOther","user")="not mine"
 Write "ok",$C(10)"#,
    )
    .await;

    let p = IrisGlobalParams {
        subscripts: Some(vec![
            "SourceControl".to_string(),
            "Git".to_string(),
            "settings".to_string(),
        ]),
        subtree: Some(true),
        max_nodes: Some(200),
        ..make_params("get", "IrisDevTest.G146")
    };
    let result = handle_iris_global(&conn, &client, &p, Ok(())).await;
    run_in_user(
        &conn,
        &client,
        r#" Kill ^IrisDevTest.G146
 Write "ok",$C(10)"#,
    )
    .await;

    assert_eq!(result["success"], true, "get failed: {result}");
    let paths: Vec<String> = result["nodes"]
        .as_array()
        .expect("nodes array")
        .iter()
        .map(|n| n["path"].as_str().unwrap_or_default().to_string())
        .collect();
    assert_eq!(
        result["node_count"], 2,
        "expected two descendants: {result}"
    );
    assert!(
        paths.iter().any(|p| p.ends_with(r#""settings","user")"#)),
        "missing settings/user in {paths:?}"
    );
    assert!(
        paths
            .iter()
            .any(|p| p.ends_with(r#""settings","remote","url")"#)),
        "missing the grandchild in {paths:?}"
    );
    assert!(
        !paths.iter().any(|p| p.contains("settingsOther")),
        "a sibling with the same prefix leaked in: {paths:?}"
    );
}

#[tokio::test]
#[ignore]
async fn test_get_subtree_of_an_unsubscripted_global_still_works() {
    let Some((conn, client)) = make_conn() else {
        eprintln!("IRIS_HOST not set — skipping");
        return;
    };
    run_in_user(
        &conn,
        &client,
        r#" Kill ^IrisDevTest.G146b
 Set ^IrisDevTest.G146b("a")=1
 Set ^IrisDevTest.G146b("b","c")=2
 Write "ok",$C(10)"#,
    )
    .await;
    let p = IrisGlobalParams {
        subtree: Some(true),
        ..make_params("get", "IrisDevTest.G146b")
    };
    let result = handle_iris_global(&conn, &client, &p, Ok(())).await;
    run_in_user(
        &conn,
        &client,
        r#" Kill ^IrisDevTest.G146b
 Write "ok",$C(10)"#,
    )
    .await;
    assert_eq!(result["node_count"], 2, "expected two nodes: {result}");
}

// ── #146 follow-up: a node with a value and children ($DATA = 11) ────────────

#[tokio::test]
#[ignore]
async fn test_get_subtree_includes_the_base_nodes_own_value() {
    let Some((conn, client)) = make_conn() else {
        eprintln!("IRIS_HOST not set — skipping");
        return;
    };
    run_in_user(
        &conn,
        &client,
        r#" Kill ^IrisDevTest.G146c
 Set ^IrisDevTest.G146c("cfg")="top"
 Set ^IrisDevTest.G146c("cfg","a")=1
 Set ^IrisDevTest.G146c("cfg","b","c")=2
 Set ^IrisDevTest.G146c("none","a")=3
 Write "ok",$C(10)"#,
    )
    .await;
    let with_value = IrisGlobalParams {
        subscripts: Some(vec!["cfg".to_string()]),
        subtree: Some(true),
        ..make_params("get", "IrisDevTest.G146c")
    };
    let r11 = handle_iris_global(&conn, &client, &with_value, Ok(())).await;
    // A node with children and no value of its own (D=10) emits no row for itself.
    let valueless = IrisGlobalParams {
        subscripts: Some(vec!["none".to_string()]),
        subtree: Some(true),
        ..make_params("get", "IrisDevTest.G146c")
    };
    let r10 = handle_iris_global(&conn, &client, &valueless, Ok(())).await;
    run_in_user(
        &conn,
        &client,
        r#" Kill ^IrisDevTest.G146c
 Write "ok",$C(10)"#,
    )
    .await;

    assert_eq!(
        r11["node_count"], 3,
        "own value plus two descendants: {r11}"
    );
    let first = &r11["nodes"][0];
    assert!(
        first["path"]
            .as_str()
            .unwrap_or_default()
            .ends_with(r#"("cfg")"#),
        "the base node comes first: {r11}"
    );
    assert_eq!(first["value"], "top", "{r11}");
    assert_eq!(
        r10["node_count"], 1,
        "a valueless node emits only its child: {r10}"
    );
}

// ── global_name is code, not data: a read must not run what the name smuggles in ──

#[tokio::test]
#[ignore]
async fn test_get_with_a_crafted_global_name_runs_nothing() {
    let Some((conn, client)) = make_conn() else {
        eprintln!("IRIS_HOST not set — skipping");
        return;
    };
    run_in_user(
        &conn,
        &client,
        r#" Set ^IrisDevTest.G136Victim = "still here"
 Write "ok",$C(10)"#,
    )
    .await;
    // Closes the $Get, kills the sentinel, reopens a $Get: valid ObjectScript once interpolated.
    let crafted = "IrisDevTest.G136X) Kill ^IrisDevTest.G136Victim Set q=$Get(^IrisDevTest.G136X";
    for subtree in [false, true] {
        let p = IrisGlobalParams {
            subtree: Some(subtree),
            ..make_params("get", crafted)
        };
        let r = handle_iris_global(&conn, &client, &p, Ok(())).await;
        assert_eq!(
            r["error_code"], "INVALID_GLOBAL_NAME",
            "subtree={subtree}: {r}"
        );
    }
    let list = make_params("list", crafted);
    let r = handle_iris_global(&conn, &client, &list, Ok(())).await;
    assert_eq!(r["error_code"], "INVALID_GLOBAL_NAME", "list: {r}");

    let check = IrisGlobalParams {
        ..make_params("get", "IrisDevTest.G136Victim")
    };
    let still = handle_iris_global(&conn, &client, &check, Ok(())).await;
    run_in_user(
        &conn,
        &client,
        r#" Kill ^IrisDevTest.G136Victim
 Write "ok",$C(10)"#,
    )
    .await;
    assert_eq!(
        still["value"], "still here",
        "a read killed a global: {still}"
    );
}

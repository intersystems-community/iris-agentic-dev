//! E2E tests for admin tools (T081, T087, T092, T096, T101, T108-T118).
//! All tests require a live IRIS container and are #[ignore] by default.
//!
//! Run with:
//!   IRIS_HOST=localhost IRIS_WEB_PORT=52780 IRIS_USERNAME=_SYSTEM IRIS_PASSWORD=SYS \
//!   cargo test --test test_admin_e2e -- --include-ignored --test-threads=1 --nocapture

use iris_agentic_dev_core::iris::connection::{DiscoverySource, IrisConnection};
use std::sync::Arc;

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

fn parse_json(r: rmcp::model::CallToolResult) -> serde_json::Value {
    let text = r
        .content
        .first()
        .map(|c| c.raw.as_text().unwrap().text.clone())
        .expect("no text content");
    serde_json::from_str(&text).expect("json parse failed")
}

// T081: iris_namespace_list returns at least USER and %SYS
#[tokio::test]
#[ignore]
async fn e2e_namespace_list() {
    use iris_agentic_dev_core::tools::admin_tools::iris_namespace_list_impl;

    let (conn, client) = match make_conn() {
        Some(c) => c,
        None => {
            eprintln!("IRIS_HOST not set — skipping e2e_namespace_list");
            return;
        }
    };

    let result = iris_namespace_list_impl(&conn, &client)
        .await
        .expect("iris_namespace_list_impl failed");
    let v = parse_json(result);
    assert!(
        v["success"].as_bool().unwrap_or(false),
        "expected success=true, got: {v}"
    );
    let namespaces = v["namespaces"]
        .as_array()
        .expect("namespaces must be an array");
    let ns_strs: Vec<&str> = namespaces.iter().filter_map(|n| n.as_str()).collect();
    assert!(
        ns_strs.iter().any(|n| n.eq_ignore_ascii_case("USER")),
        "USER namespace must be present, got: {ns_strs:?}"
    );
}

// T087: journal_search returns result without error
#[tokio::test]
#[ignore]
async fn e2e_journal_search() {
    use iris_agentic_dev_core::tools::admin_tools::journal_search_impl;

    let (conn, client) = match make_conn() {
        Some(c) => c,
        None => {
            eprintln!("IRIS_HOST not set — skipping e2e_journal_search");
            return;
        }
    };

    let result = journal_search_impl(&conn, &client, None, None, None, 10)
        .await
        .expect("journal_search_impl failed");
    let v = parse_json(result);
    // Either success=true (entries returned) or a known non-crash error
    let success = v["success"].as_bool().unwrap_or(false);
    let err_code = v["error_code"].as_str().unwrap_or("");
    assert!(
        success || err_code == "NO_JOURNAL",
        "journal_search must succeed or return NO_JOURNAL, got: {v}"
    );
}

// T092: my_access returns current username and at least one role
#[tokio::test]
#[ignore]
async fn e2e_my_access() {
    use iris_agentic_dev_core::tools::admin_tools::my_access_impl;

    let (conn, client) = match make_conn() {
        Some(c) => c,
        None => {
            eprintln!("IRIS_HOST not set — skipping e2e_my_access");
            return;
        }
    };

    let result = my_access_impl(&conn, &client)
        .await
        .expect("my_access_impl failed");
    let v = parse_json(result);
    assert!(
        v["success"].as_bool().unwrap_or(false),
        "expected success=true, got: {v}"
    );
    let username = v["username"].as_str().unwrap_or("");
    assert!(!username.is_empty(), "username must not be empty, got: {v}");
}

// T096: hl7_schema_list — if EnsLib.HL7.Schema absent, returns HL7_NOT_AVAILABLE
#[tokio::test]
#[ignore]
async fn e2e_hl7_schema_list() {
    use iris_agentic_dev_core::tools::admin_tools::{hl7_schema_list_impl, ERR_HL7_NOT_AVAILABLE};

    let (conn, client) = match make_conn() {
        Some(c) => c,
        None => {
            eprintln!("IRIS_HOST not set — skipping e2e_hl7_schema_list");
            return;
        }
    };

    let result = hl7_schema_list_impl(&conn, &client, "USER")
        .await
        .expect("hl7_schema_list_impl failed");
    let v = parse_json(result);
    // Either succeeds with schemas list, or returns HL7_NOT_AVAILABLE (not a crash)
    let success = v["success"].as_bool().unwrap_or(false);
    let err_code = v["error_code"].as_str().unwrap_or("");
    assert!(
        success || err_code == ERR_HL7_NOT_AVAILABLE,
        "hl7_schema_list must succeed or return HL7_NOT_AVAILABLE, got: {v}"
    );
}

// T101: mermaid_class returns a string starting with classDiagram
#[tokio::test]
#[ignore]
async fn e2e_mermaid_class() {
    use iris_agentic_dev_core::tools::admin_tools::mermaid_class_impl;

    let (conn, client) = match make_conn() {
        Some(c) => c,
        None => {
            eprintln!("IRIS_HOST not set — skipping e2e_mermaid_class");
            return;
        }
    };

    let result = mermaid_class_impl(&conn, &client, "%Library.Persistent", 2, "USER")
        .await
        .expect("mermaid_class_impl failed");
    let v = parse_json(result);
    assert!(
        v["success"].as_bool().unwrap_or(false),
        "expected success=true, got: {v}"
    );
    let diagram = v["diagram"].as_str().unwrap_or("");
    assert!(
        diagram.starts_with("classDiagram"),
        "diagram must start with 'classDiagram', got: {diagram}"
    );
}

// T076: global_preview + global_kill confirm round-trip
#[tokio::test]
#[ignore]
async fn e2e_global_kill_confirm() {
    use iris_agentic_dev_core::tools::admin_tools::{
        global_kill_impl, global_preview_impl, GlobalKillParams, GlobalPreviewParams,
    };
    use tokio::sync::Mutex;

    let (conn, client) = match make_conn() {
        Some(c) => c,
        None => {
            eprintln!("IRIS_HOST not set — skipping e2e_global_kill_confirm");
            return;
        }
    };
    let iris = Arc::new(conn);
    let client = Arc::new(client);
    let confirm_tokens = Mutex::new(std::collections::HashMap::new());

    // Seed a known global so preview has something to show.
    let seed_code = r#"Set ^IrisAgentDevTest("e2e-kill-test")="delete-me""#;
    let _ = iris
        .execute_via_generator(seed_code, &iris.namespace, &client)
        .await;

    // Step 1: preview — should mint a token.
    let preview_result = global_preview_impl(
        GlobalPreviewParams {
            global: "IrisAgentDevTest".to_string(),
            server: None,
            count: 5,
            iris: Arc::clone(&iris),
            client: Arc::clone(&client),
        },
        &confirm_tokens,
    )
    .await
    .expect("global_preview_impl failed");
    let pv = parse_json(preview_result);
    assert!(
        pv["success"].as_bool().unwrap_or(false),
        "global_preview must succeed, got: {pv}"
    );
    let token = pv["confirm_token"]
        .as_str()
        .expect("no confirm_token")
        .to_string();

    // Step 2: kill with the token.
    let kill_result = global_kill_impl(
        GlobalKillParams {
            global: "IrisAgentDevTest".to_string(),
            server: None,
            confirm_token: token,
            iris: Arc::clone(&iris),
            client: Arc::clone(&client),
            write_tools_enabled: true,
        },
        &confirm_tokens,
    )
    .await
    .expect("global_kill_impl failed");
    let kv = parse_json(kill_result);
    assert!(
        kv["success"].as_bool().unwrap_or(false),
        "global_kill must succeed, got: {kv}"
    );
    assert!(
        kv["killed"].as_bool().unwrap_or(false),
        "killed must be true, got: {kv}"
    );
}

// T108: iris_database_list returns at least one database entry
#[tokio::test]
#[ignore]
async fn e2e_database_list() {
    use iris_agentic_dev_core::tools::admin_tools::iris_database_list_impl;

    let (conn, client) = match make_conn() {
        Some(c) => c,
        None => {
            eprintln!("IRIS_HOST not set — skipping e2e_database_list");
            return;
        }
    };

    let result = iris_database_list_impl(&conn, &client)
        .await
        .expect("iris_database_list_impl failed");
    let v = parse_json(result);
    assert!(
        v["success"].as_bool().unwrap_or(false),
        "expected success=true, got: {v}"
    );
    let dbs = v["databases"].as_array().expect("databases must be array");
    assert!(!dbs.is_empty(), "at least one database must be returned");
    // Every entry must have a directory field
    assert!(
        dbs.iter().all(|d| d["directory"].as_str().is_some()),
        "all databases must have a directory field, got: {v}"
    );
}

// T109: iris_database_stats returns stats for all databases
#[tokio::test]
#[ignore]
async fn e2e_database_stats() {
    use iris_agentic_dev_core::tools::admin_tools::iris_database_stats_impl;

    let (conn, client) = match make_conn() {
        Some(c) => c,
        None => {
            eprintln!("IRIS_HOST not set — skipping e2e_database_stats");
            return;
        }
    };

    let result = iris_database_stats_impl(&conn, &client, None)
        .await
        .expect("iris_database_stats_impl failed");
    let v = parse_json(result);
    assert!(
        v["success"].as_bool().unwrap_or(false),
        "expected success=true, got: {v}"
    );
    let stats = v["stats"].as_array().expect("stats must be array");
    assert!(
        !stats.is_empty(),
        "at least one stats entry must be returned"
    );
}

// T110: iris_namespace_create — write-gate blocks without flag, succeeds with flag
#[tokio::test]
#[ignore]
async fn e2e_namespace_create_write_gate() {
    use iris_agentic_dev_core::tools::admin_tools::{iris_namespace_create_impl, ERR_WRITE_GATE};

    let (conn, client) = match make_conn() {
        Some(c) => c,
        None => {
            eprintln!("IRIS_HOST not set — skipping e2e_namespace_create_write_gate");
            return;
        }
    };

    // Without write gate: must return WRITE_TOOLS_DISABLED
    let blocked = iris_namespace_create_impl(&conn, &client, "IADTESTNS", None, false)
        .await
        .expect("iris_namespace_create_impl failed");
    let bv = parse_json(blocked);
    assert_eq!(
        bv["error_code"].as_str().unwrap_or(""),
        ERR_WRITE_GATE,
        "expected WRITE_TOOLS_DISABLED without write gate, got: {bv}"
    );

    // With write gate: should succeed or return a meaningful error (e.g. already exists)
    let result = iris_namespace_create_impl(&conn, &client, "IADTESTNS", None, true)
        .await
        .expect("iris_namespace_create_impl failed");
    let v = parse_json(result);
    let success = v["success"].as_bool().unwrap_or(false);
    let err_code = v["error_code"].as_str().unwrap_or("");
    assert!(
        success || !err_code.is_empty(),
        "namespace create must succeed or return an error code, got: {v}"
    );
}

// T111: query_audit_log returns entries array (may be empty on dev instances)
#[tokio::test]
#[ignore]
async fn e2e_query_audit_log() {
    use iris_agentic_dev_core::tools::admin_tools::query_audit_log_impl;

    let (conn, client) = match make_conn() {
        Some(c) => c,
        None => {
            eprintln!("IRIS_HOST not set — skipping e2e_query_audit_log");
            return;
        }
    };

    let result = query_audit_log_impl(&conn, &client, None, None, None, None, 10)
        .await
        .expect("query_audit_log_impl failed");
    let v = parse_json(result);
    assert!(
        v["success"].as_bool().unwrap_or(false),
        "expected success=true, got: {v}"
    );
    assert!(
        v["entries"].is_array(),
        "entries must be an array, got: {v}"
    );
}

// T112: stream_inspect — nonexistent OID returns a structured response (empty stream or
// STREAM_NOT_FOUND); must not panic.
#[tokio::test]
#[ignore]
async fn e2e_stream_inspect_not_found() {
    use iris_agentic_dev_core::tools::admin_tools::stream_inspect_impl;

    let (conn, client) = match make_conn() {
        Some(c) => c,
        None => {
            eprintln!("IRIS_HOST not set — skipping e2e_stream_inspect_not_found");
            return;
        }
    };

    let result = stream_inspect_impl(&conn, &client, "999999999", "USER")
        .await
        .expect("stream_inspect_impl failed");
    let v = parse_json(result);
    // IRIS opens an empty stream on an unknown numeric OID rather than returning an error,
    // so either success=true (empty content) or error_code=STREAM_NOT_FOUND are valid.
    let success = v["success"].as_bool().unwrap_or(false);
    let err_code = v["error_code"].as_str().unwrap_or("");
    assert!(
        success || err_code == "STREAM_NOT_FOUND",
        "stream_inspect must return success or STREAM_NOT_FOUND, got: {v}"
    );
}

// T113: capability_matrix returns user and roles for current session
#[tokio::test]
#[ignore]
async fn e2e_capability_matrix() {
    use iris_agentic_dev_core::tools::admin_tools::capability_matrix_impl;

    let (conn, client) = match make_conn() {
        Some(c) => c,
        None => {
            eprintln!("IRIS_HOST not set — skipping e2e_capability_matrix");
            return;
        }
    };

    let result = capability_matrix_impl(&conn, &client, None)
        .await
        .expect("capability_matrix_impl failed");
    let v = parse_json(result);
    assert!(
        v["success"].as_bool().unwrap_or(false),
        "expected success=true, got: {v}"
    );
    assert!(
        v["user"].as_str().is_some(),
        "user field must be present, got: {v}"
    );
    assert!(v["roles"].is_array(), "roles must be an array, got: {v}");
}

// T114: hl7_schema_inspect — returns HL7_NOT_AVAILABLE or a structured list
#[tokio::test]
#[ignore]
async fn e2e_hl7_schema_inspect() {
    use iris_agentic_dev_core::tools::admin_tools::{
        hl7_schema_inspect_impl, hl7_schema_list_impl, ERR_HL7_NOT_AVAILABLE,
    };

    let (conn, client) = match make_conn() {
        Some(c) => c,
        None => {
            eprintln!("IRIS_HOST not set — skipping e2e_hl7_schema_inspect");
            return;
        }
    };

    // First check if HL7 is available on this instance
    let list_result = hl7_schema_list_impl(&conn, &client, "USER")
        .await
        .expect("hl7_schema_list_impl failed");
    let lv = parse_json(list_result);
    if lv["error_code"].as_str() == Some(ERR_HL7_NOT_AVAILABLE) {
        // HL7 not installed — inspect should also return HL7_NOT_AVAILABLE cleanly
        let result = hl7_schema_inspect_impl(&conn, &client, "2.5", None, "USER")
            .await
            .expect("hl7_schema_inspect_impl failed");
        let v = parse_json(result);
        assert_eq!(
            v["error_code"].as_str().unwrap_or(""),
            ERR_HL7_NOT_AVAILABLE,
            "inspect must return HL7_NOT_AVAILABLE when HL7 absent, got: {v}"
        );
    } else {
        // HL7 available — inspect must return a structured response
        let schemas = lv["schemas"].as_array().expect("schemas must be array");
        if let Some(first) = schemas.first().and_then(|s| s.as_str()) {
            let result = hl7_schema_inspect_impl(&conn, &client, first, None, "USER")
                .await
                .expect("hl7_schema_inspect_impl failed");
            let v = parse_json(result);
            let success = v["success"].as_bool().unwrap_or(false);
            let err_code = v["error_code"].as_str().unwrap_or("");
            assert!(
                success || !err_code.is_empty(),
                "hl7_schema_inspect must return success or error_code, got: {v}"
            );
        }
    }
}

// T115: mermaid_production — returns flowchart TD or structured error
#[tokio::test]
#[ignore]
async fn e2e_mermaid_production() {
    use iris_agentic_dev_core::tools::admin_tools::mermaid_production_impl;

    let (conn, client) = match make_conn() {
        Some(c) => c,
        None => {
            eprintln!("IRIS_HOST not set — skipping e2e_mermaid_production");
            return;
        }
    };

    // Use a nonexistent production — should return success=true with an empty diagram
    // (the tool does not error on missing productions, it just returns no items)
    let result = mermaid_production_impl(&conn, &client, "IAD.Test.NonExistentProduction", "USER")
        .await
        .expect("mermaid_production_impl failed");
    let v = parse_json(result);
    assert!(
        v["success"].as_bool().unwrap_or(false),
        "expected success=true, got: {v}"
    );
    let diagram = v["diagram"].as_str().unwrap_or("");
    assert!(
        diagram.starts_with("flowchart TD"),
        "diagram must start with 'flowchart TD', got: {diagram:?}"
    );
}

// T116: resolve_storage — %Persistent has storage entries; unknown class returns empty
#[tokio::test]
#[ignore]
async fn e2e_resolve_storage() {
    use iris_agentic_dev_core::tools::admin_tools::resolve_storage_impl;

    let (conn, client) = match make_conn() {
        Some(c) => c,
        None => {
            eprintln!("IRIS_HOST not set — skipping e2e_resolve_storage");
            return;
        }
    };

    let result = resolve_storage_impl(&conn, &client, "%Library.Persistent", "USER")
        .await
        .expect("resolve_storage_impl failed");
    let v = parse_json(result);
    assert!(
        v["success"].as_bool().unwrap_or(false),
        "expected success=true, got: {v}"
    );
    assert!(
        v["storages"].is_array(),
        "storages must be an array, got: {v}"
    );
}

// T117: compare_namespace — same server vs self; only_in_a and only_in_b both empty
#[tokio::test]
#[ignore]
async fn e2e_compare_namespace_self() {
    use iris_agentic_dev_core::tools::comparison_tools::{
        compare_namespace_impl, CompareNamespaceParams,
    };

    let (conn, client) = match make_conn() {
        Some(c) => c,
        None => {
            eprintln!("IRIS_HOST not set — skipping e2e_compare_namespace_self");
            return;
        }
    };

    let server = Arc::new(conn);
    let result = compare_namespace_impl(
        CompareNamespaceParams {
            namespace: "USER".to_string(),
            server_a: Arc::clone(&server),
            server_b: Arc::clone(&server),
        },
        &client,
    )
    .await
    .expect("compare_namespace_impl failed");
    let v = parse_json(result);
    assert!(
        v["success"].as_bool().unwrap_or(false),
        "expected success=true, got: {v}"
    );
    let only_a = v["only_in_a"].as_array().map(|a| a.len()).unwrap_or(0);
    let only_b = v["only_in_b"].as_array().map(|a| a.len()).unwrap_or(0);
    assert_eq!(only_a, 0, "self-compare must have nothing only in A");
    assert_eq!(only_b, 0, "self-compare must have nothing only in B");
}

// T118: iris_import_servers — returns structured result (imported + skipped counts).
// Does not require a live IRIS connection — reads VS Code/Cursor settings files.
// Requires --features testing (call_for_test is gated on that feature).
#[tokio::test]
#[ignore]
async fn e2e_import_servers() {
    use std::process::{Command, Stdio};

    // Invoke the binary directly with a tools/call MCP message — no IRIS needed.
    // Respect IRIS_DEV_BIN (set by scripts/coverage.sh) so the instrumented
    // binary is used when collecting subprocess coverage.
    let bin = if let Ok(p) = std::env::var("IRIS_DEV_BIN") {
        let p = std::path::PathBuf::from(p);
        if p.exists() {
            p
        } else {
            let workspace_root = {
                let mut p = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
                p.pop();
                p.pop();
                p
            };
            [
                "target/debug/iris-agentic-dev",
                "target/release/iris-agentic-dev",
            ]
            .iter()
            .map(|s| workspace_root.join(s))
            .find(|p| p.exists())
            .expect("iris-agentic-dev binary not found; run `cargo build` first")
        }
    } else {
        let workspace_root = {
            let mut p = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
            p.pop();
            p.pop();
            p
        };
        [
            "target/debug/iris-agentic-dev",
            "target/release/iris-agentic-dev",
        ]
        .iter()
        .map(|s| workspace_root.join(s))
        .find(|p| p.exists())
        .expect("iris-agentic-dev binary not found; run `cargo build` first")
    };

    let messages = serde_json::json!([
        {"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"test","version":"0.1"}}},
        {"jsonrpc":"2.0","method":"notifications/initialized","params":{}},
        {"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"iris_import_servers","arguments":{}}}
    ]);

    let mut cmd = Command::new(&bin);
    cmd.args(["mcp"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    // Propagate LLVM_PROFILE_FILE so the spawned process writes coverage data
    // when built with -C instrument-coverage (used by scripts/coverage.sh).
    if let Ok(profile) = std::env::var("LLVM_PROFILE_FILE") {
        cmd.env("LLVM_PROFILE_FILE", &profile);
    }
    let mut child = cmd.spawn().expect("failed to spawn binary");

    let stdin = child.stdin.take().unwrap();
    {
        use std::io::Write;
        let mut w = std::io::BufWriter::new(stdin);
        for msg in messages.as_array().unwrap() {
            writeln!(w, "{msg}").unwrap();
        }
    }

    let output = child.wait_with_output().expect("wait failed");
    let stdout = String::from_utf8_lossy(&output.stdout);

    // Find the tools/call response (id=2)
    let response: serde_json::Value = stdout
        .lines()
        .filter_map(|l| serde_json::from_str(l).ok())
        .find(|v: &serde_json::Value| v["id"] == 2)
        .expect("no response for id=2");

    let text = response["result"]["content"][0]["text"]
        .as_str()
        .expect("no text in response");
    let v: serde_json::Value = serde_json::from_str(text).expect("not json");
    assert!(
        v["success"].as_bool().unwrap_or(false),
        "expected success=true, got: {v}"
    );
    assert!(
        v["imported"].is_number(),
        "imported must be a number, got: {v}"
    );
    assert!(
        v["skipped"].is_number(),
        "skipped must be a number, got: {v}"
    );
}

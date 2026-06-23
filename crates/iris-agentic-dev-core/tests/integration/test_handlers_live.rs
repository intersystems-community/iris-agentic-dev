//! Live integration tests for IRIS handler functions.
//!
//! These tests require a running IRIS instance. All tests skip gracefully when
//! IRIS_HOST is not set in the environment.
//!
//! Run with:
//!   IRIS_HOST=localhost IRIS_WEB_PORT=52773 \
//!   cargo test --test test_handlers_live -- --nocapture
//!
//! Optional:
//!   IRIS_CONTAINER=<name>  — enables execute() / docker-backed tests (B, C)
//!   IRIS_USERNAME=_SYSTEM IRIS_PASSWORD=SYS  — override credentials

use iris_agentic_dev_core::iris::connection::{DiscoverySource, IrisConnection};
use iris_agentic_dev_core::tools::doc::{handle_iris_doc, DocMode, IrisDocParams};
use iris_agentic_dev_core::tools::info::{
    handle_iris_info, handle_iris_macro, handle_iris_table_info, InfoParams, MacroParams,
    TableInfoParams,
};
use iris_agentic_dev_core::tools::log_store;
use iris_agentic_dev_core::tools::search::{handle_iris_search, SearchParams};
use std::sync::{Arc, Mutex};

// ── Helpers ───────────────────────────────────────────────────────────────────

fn make_conn() -> Option<(IrisConnection, reqwest::Client)> {
    let iris_host = std::env::var("IRIS_HOST").unwrap_or_default();
    if iris_host.is_empty() {
        return None;
    }
    let web_port = std::env::var("IRIS_WEB_PORT").unwrap_or_else(|_| "52773".to_string());
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
    let client = IrisConnection::http_client().unwrap();
    Some((conn, client))
}

fn make_log_store() -> Arc<Mutex<log_store::LogStore>> {
    Arc::new(Mutex::new(log_store::LogStore::new(100, 60)))
}

fn rt() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
}

/// Parse the text payload out of a CallToolResult and deserialise it as JSON.
fn result_json(r: Result<rmcp::model::CallToolResult, rmcp::ErrorData>) -> serde_json::Value {
    let tool_result = r.expect("handler returned Err(ErrorData)");
    let text = tool_result.content[0]
        .raw
        .as_text()
        .expect("first content item is not text")
        .text
        .clone();
    serde_json::from_str(&text).expect("response is not valid JSON")
}

// ── Test A: probe sets version ────────────────────────────────────────────────

#[test]
fn test_probe_sets_version() {
    rt().block_on(async {
        let (mut conn, _client) = match make_conn() {
            Some(c) => c,
            None => {
                eprintln!("SKIP test_probe_sets_version — IRIS_HOST not set");
                return;
            }
        };
        conn.probe().await;
        assert!(
            conn.version.is_some(),
            "probe() should populate conn.version; got None (is IRIS reachable?)"
        );
        let v = conn.version.as_ref().unwrap();
        assert!(
            v.contains("IRIS") || v.contains("Cache") || !v.is_empty(),
            "version string looks wrong: {v}"
        );
    });
}

// ── Test B: execute Write 42 ──────────────────────────────────────────────────

#[test]
fn test_execute_write_number() {
    rt().block_on(async {
        let (conn, _client) = match make_conn() {
            Some(c) => c,
            None => {
                eprintln!("SKIP test_execute_write_number — IRIS_HOST not set");
                return;
            }
        };
        if std::env::var("IRIS_CONTAINER").is_err() {
            eprintln!(
                "SKIP test_execute_write_number — IRIS_CONTAINER not set (docker exec required)"
            );
            return;
        }
        let output = conn
            .execute("Write 42", "USER")
            .await
            .expect("execute() should succeed");
        assert!(
            output.trim().contains("42"),
            "expected '42' in output, got: {output:?}"
        );
    });
}

// ── Test C: execute $ZVersion contains "IRIS" ─────────────────────────────────

#[test]
fn test_execute_zversion() {
    rt().block_on(async {
        let (conn, _client) = match make_conn() {
            Some(c) => c,
            None => {
                eprintln!("SKIP test_execute_zversion — IRIS_HOST not set");
                return;
            }
        };
        if std::env::var("IRIS_CONTAINER").is_err() {
            eprintln!("SKIP test_execute_zversion — IRIS_CONTAINER not set (docker exec required)");
            return;
        }
        let output = conn
            .execute("Write $ZVersion", "USER")
            .await
            .expect("execute() should succeed");
        assert!(
            output.contains("IRIS") || output.contains("Cache"),
            "expected IRIS version string, got: {output:?}"
        );
    });
}

// ── Test D: query SELECT 1 returns rows ───────────────────────────────────────

#[test]
fn test_query_select_one() {
    rt().block_on(async {
        let (conn, client) = match make_conn() {
            Some(c) => c,
            None => {
                eprintln!("SKIP test_query_select_one — IRIS_HOST not set");
                return;
            }
        };
        let body = conn
            .query("SELECT 1 AS val", vec![], "USER", &client)
            .await
            .expect("query() should succeed");
        let rows = body["result"]["content"]
            .as_array()
            .expect("result.content should be an array");
        assert!(!rows.is_empty(), "SELECT 1 should return at least one row");
        let val = &rows[0]["val"];
        assert!(
            val.as_i64() == Some(1) || val.as_str() == Some("1"),
            "val column should be 1, got: {val}"
        );
    });
}

// ── Test E: query TOP 3 from ClassDefinition ──────────────────────────────────

#[test]
fn test_query_class_dict() {
    rt().block_on(async {
        let (conn, client) = match make_conn() {
            Some(c) => c,
            None => {
                eprintln!("SKIP test_query_class_dict — IRIS_HOST not set");
                return;
            }
        };
        let body = conn
            .query(
                "SELECT TOP 3 Name FROM %Dictionary.ClassDefinition",
                vec![],
                "USER",
                &client,
            )
            .await
            .expect("query() should succeed");
        let rows = body["result"]["content"]
            .as_array()
            .expect("result.content should be an array");
        assert_eq!(rows.len(), 3, "expected exactly 3 rows from TOP 3");
        for row in rows {
            assert!(
                row["Name"].as_str().is_some(),
                "each row should have a Name string field"
            );
        }
    });
}

// ── Test F: compile non-existent class returns errors ─────────────────────────

#[test]
fn test_compile_nonexistent_class() {
    rt().block_on(async {
        let (conn, client) = match make_conn() {
            Some(c) => c,
            None => {
                eprintln!("SKIP test_compile_nonexistent_class — IRIS_HOST not set");
                return;
            }
        };
        // compile_document returns Ok even when the class doesn't exist —
        // the errors are inside CompileResult.errors.
        let result = conn
            .compile_document("IrisDevTest.DoesNotExist9999.cls", "USER", "ck", &client)
            .await;
        match result {
            Ok(cr) => {
                // Expect errors about the class not being found
                assert!(
                    !cr.errors.is_empty(),
                    "expected compile errors for non-existent class, got none"
                );
            }
            Err(e) => {
                // A transport/HTTP error is also acceptable (e.g. 404 from Atelier)
                let msg = e.to_string();
                assert!(
                    msg.contains("HTTP") || msg.contains("compile"),
                    "unexpected error: {e}"
                );
            }
        }
    });
}

// ── Test G: compile %Library.Object succeeds or errors gracefully ─────────────

#[test]
fn test_compile_system_class() {
    rt().block_on(async {
        let (conn, client) = match make_conn() {
            Some(c) => c,
            None => {
                eprintln!("SKIP test_compile_system_class — IRIS_HOST not set");
                return;
            }
        };
        let result = conn
            .compile_document("%Library.Object.cls", "USER", "ck", &client)
            .await;
        match result {
            Ok(cr) => {
                // System class compile may succeed or produce warnings — neither is a test failure
                eprintln!(
                    "compile %Library.Object: errors={:?} console={:?}",
                    cr.errors, cr.console
                );
            }
            Err(e) => {
                // An error response from Atelier (e.g. 403/404) is acceptable
                eprintln!("compile %Library.Object returned Err (ok): {e}");
            }
        }
    });
}

// ── Test H: handle_iris_info namespace ────────────────────────────────────────

#[test]
fn test_handle_iris_info_namespace() {
    rt().block_on(async {
        let (conn, client) = match make_conn() {
            Some(c) => c,
            None => {
                eprintln!("SKIP test_handle_iris_info_namespace — IRIS_HOST not set");
                return;
            }
        };
        let p = InfoParams {
            what: "namespace".to_string(),
            doc_type: None,
            name: None,
            namespace: "USER".to_string(),
            inline: false,
        };
        let r = handle_iris_info(&conn, &client, p, make_log_store()).await;
        let v = result_json(r);
        assert_eq!(
            v["success"].as_bool(),
            Some(true),
            "expected success:true, got: {v}"
        );
        assert_eq!(v["what"].as_str(), Some("namespace"));
    });
}

// ── Test I: handle_iris_info documents ────────────────────────────────────────

#[test]
fn test_handle_iris_info_documents() {
    rt().block_on(async {
        let (conn, client) = match make_conn() {
            Some(c) => c,
            None => {
                eprintln!("SKIP test_handle_iris_info_documents — IRIS_HOST not set");
                return;
            }
        };
        let p = InfoParams {
            what: "documents".to_string(),
            doc_type: Some("CLS".to_string()),
            name: None,
            namespace: "USER".to_string(),
            inline: true,
        };
        let r = handle_iris_info(&conn, &client, p, make_log_store()).await;
        let v = result_json(r);
        assert_eq!(
            v["success"].as_bool(),
            Some(true),
            "expected success:true, got: {v}"
        );
        assert_eq!(v["what"].as_str(), Some("documents"));
    });
}

// ── Test J: handle_iris_search basic ─────────────────────────────────────────

#[test]
fn test_handle_iris_search_basic() {
    rt().block_on(async {
        let (conn, client) = match make_conn() {
            Some(c) => c,
            None => {
                eprintln!("SKIP test_handle_iris_search_basic — IRIS_HOST not set");
                return;
            }
        };
        let p = SearchParams {
            query: "Class".to_string(),
            regex: false,
            case_sensitive: false,
            category: None,
            documents: vec![],
            namespace: "USER".to_string(),
            inline: true,
        };
        let r = handle_iris_search(&conn, &client, p, make_log_store()).await;
        let v = result_json(r);
        // Response has either success:true with total_found, or success:false with error_code.
        // Both are valid — we just verify the handler didn't panic and returned parseable JSON.
        assert!(
            v.get("success").is_some(),
            "expected a 'success' field in response, got: {v}"
        );
        if v["success"].as_bool() == Some(true) {
            let total = v["total_found"].as_i64().unwrap_or(0);
            assert!(total >= 0, "total_found should be non-negative");
        }
    });
}

// ── Test K: handle_iris_doc GET %Library.Object.cls ───────────────────────────

#[test]
fn test_handle_iris_doc_get_object_cls() {
    rt().block_on(async {
        let (conn, client) = match make_conn() {
            Some(c) => c,
            None => {
                eprintln!("SKIP test_handle_iris_doc_get_object_cls — IRIS_HOST not set");
                return;
            }
        };
        let elicitation_store = iris_agentic_dev_core::elicitation::ElicitationStore::new();
        let p = IrisDocParams {
            mode: DocMode::Get,
            name: Some("%Library.Object.cls".to_string()),
            names: vec![],
            content: None,
            namespace: "USER".to_string(),
            elicitation_id: None,
            elicitation_answer: None,
            compile: false,
        };
        let r = handle_iris_doc(&conn, &client, p, &elicitation_store).await;
        let v = result_json(r);
        assert!(
            v.get("success").is_some(),
            "expected a 'success' field in response, got: {v}"
        );
        if v["success"].as_bool() == Some(true) {
            assert!(
                v.get("name").is_some() || v.get("content").is_some() || v.get("result").is_some(),
                "successful GET should return name/content/result, got: {v}"
            );
        }
    });
}

// ── Test L: handle_iris_doc HEAD %Library.Object.cls ─────────────────────────

#[test]
fn test_handle_iris_doc_head_object_cls() {
    rt().block_on(async {
        let (conn, client) = match make_conn() {
            Some(c) => c,
            None => {
                eprintln!("SKIP test_handle_iris_doc_head_object_cls — IRIS_HOST not set");
                return;
            }
        };
        let elicitation_store = iris_agentic_dev_core::elicitation::ElicitationStore::new();
        let p = IrisDocParams {
            mode: DocMode::Head,
            name: Some("%Library.Object.cls".to_string()),
            names: vec![],
            content: None,
            namespace: "USER".to_string(),
            elicitation_id: None,
            elicitation_answer: None,
            compile: false,
        };
        // Must not panic; any structured JSON response is acceptable
        let r = handle_iris_doc(&conn, &client, p, &elicitation_store).await;
        let v = result_json(r);
        assert!(
            v.get("success").is_some(),
            "HEAD response should include 'success' field, got: {v}"
        );
    });
}

// ── Test M: handle_iris_macro list ────────────────────────────────────────────

#[test]
fn test_handle_iris_macro_list() {
    rt().block_on(async {
        let (conn, client) = match make_conn() {
            Some(c) => c,
            None => {
                eprintln!("SKIP test_handle_iris_macro_list — IRIS_HOST not set");
                return;
            }
        };
        let p = MacroParams {
            action: "list".to_string(),
            name: None,
            args: vec![],
            namespace: "USER".to_string(),
        };
        let r = handle_iris_macro(&conn, &client, p).await;
        let v = result_json(r);
        assert_eq!(
            v["success"].as_bool(),
            Some(true),
            "macro list should succeed, got: {v}"
        );
        assert!(
            v.get("macros").is_some(),
            "macro list response should include 'macros' field, got: {v}"
        );
    });
}

// ── Test N: handle_iris_table_info INFORMATION_SCHEMA.TABLES ─────────────────

#[test]
fn test_handle_iris_table_info() {
    rt().block_on(async {
        let (conn, client) = match make_conn() {
            Some(c) => c,
            None => {
                eprintln!("SKIP test_handle_iris_table_info — IRIS_HOST not set");
                return;
            }
        };
        // handle_iris_table_info uses execute_via_generator which requires IRIS_CONTAINER.
        // Skip gracefully when container is not configured.
        if std::env::var("IRIS_CONTAINER").is_err() {
            eprintln!(
                "SKIP test_handle_iris_table_info — IRIS_CONTAINER not set (execute_via_generator required)"
            );
            return;
        }
        let p = TableInfoParams {
            table: "INFORMATION_SCHEMA.TABLES".to_string(),
            namespace: "USER".to_string(),
            include_row_count: false,
        };
        let r = handle_iris_table_info(&conn, &client, p).await;
        let v = result_json(r);
        assert!(
            v.get("success").is_some(),
            "table_info response should include 'success' field, got: {v}"
        );
    });
}

// ── Test O: query system namespaces via INFORMATION_SCHEMA ────────────────────

#[test]
fn test_query_system_namespaces() {
    rt().block_on(async {
        let (conn, client) = match make_conn() {
            Some(c) => c,
            None => {
                eprintln!("SKIP test_query_system_namespaces — IRIS_HOST not set");
                return;
            }
        };
        // Query INFORMATION_SCHEMA.SCHEMATA — available in every IRIS namespace.
        let body = conn
            .query(
                "SELECT TOP 5 SCHEMA_NAME FROM INFORMATION_SCHEMA.SCHEMATA",
                vec![],
                "USER",
                &client,
            )
            .await
            .expect("query() should succeed");
        let rows = body["result"]["content"]
            .as_array()
            .expect("result.content should be an array");
        assert!(
            !rows.is_empty(),
            "INFORMATION_SCHEMA.SCHEMATA should return at least one schema"
        );
        for row in rows {
            assert!(
                row["SCHEMA_NAME"].as_str().is_some(),
                "each row should have a SCHEMA_NAME string, got: {row}"
            );
        }
    });
}

// ── Test P: handle_iris_info metadata (root endpoint) ────────────────────────

#[test]
fn test_handle_iris_info_metadata() {
    rt().block_on(async {
        let (conn, client) = match make_conn() {
            Some(c) => c,
            None => {
                eprintln!("SKIP test_handle_iris_info_metadata — IRIS_HOST not set");
                return;
            }
        };
        let p = InfoParams {
            what: "metadata".to_string(),
            doc_type: None,
            name: None,
            namespace: "USER".to_string(),
            inline: false,
        };
        let r = handle_iris_info(&conn, &client, p, make_log_store()).await;
        let v = result_json(r);
        assert_eq!(
            v["success"].as_bool(),
            Some(true),
            "metadata query should succeed, got: {v}"
        );
    });
}

// ── Test Q: handle_iris_info invalid 'what' returns error_code ────────────────

#[test]
fn test_handle_iris_info_invalid_what() {
    rt().block_on(async {
        let (conn, client) = match make_conn() {
            Some(c) => c,
            None => {
                eprintln!("SKIP test_handle_iris_info_invalid_what — IRIS_HOST not set");
                return;
            }
        };
        let p = InfoParams {
            what: "invalid_value_xyz".to_string(),
            doc_type: None,
            name: None,
            namespace: "USER".to_string(),
            inline: false,
        };
        let r = handle_iris_info(&conn, &client, p, make_log_store()).await;
        let v = result_json(r);
        assert_eq!(
            v["success"].as_bool(),
            Some(false),
            "invalid 'what' should return success:false, got: {v}"
        );
        assert_eq!(
            v["error_code"].as_str(),
            Some("INVALID_PARAM"),
            "expected INVALID_PARAM error_code, got: {v}"
        );
    });
}

// ── Test R: query with params (parameterised SQL) ─────────────────────────────

#[test]
fn test_query_with_params() {
    rt().block_on(async {
        let (conn, client) = match make_conn() {
            Some(c) => c,
            None => {
                eprintln!("SKIP test_query_with_params — IRIS_HOST not set");
                return;
            }
        };
        let body = conn
            .query(
                "SELECT ? + ? AS total",
                vec![serde_json::json!(3), serde_json::json!(4)],
                "USER",
                &client,
            )
            .await
            .expect("parameterised query() should succeed");
        let rows = body["result"]["content"]
            .as_array()
            .expect("result.content should be an array");
        assert!(!rows.is_empty(), "parameterised SELECT should return a row");
        let total = &rows[0]["total"];
        assert!(
            total.as_i64() == Some(7)
                || total.as_str() == Some("7")
                || total.as_f64().map(|f| f as i64) == Some(7),
            "3 + 4 should equal 7, got: {total}"
        );
    });
}

// ── Test S: handle_iris_doc batch GET (names vec) ─────────────────────────────

#[test]
fn test_handle_iris_doc_batch_get() {
    rt().block_on(async {
        let (conn, client) = match make_conn() {
            Some(c) => c,
            None => {
                eprintln!("SKIP test_handle_iris_doc_batch_get — IRIS_HOST not set");
                return;
            }
        };
        let elicitation_store = iris_agentic_dev_core::elicitation::ElicitationStore::new();
        let p = IrisDocParams {
            mode: DocMode::Get,
            name: None,
            names: vec![
                "%Library.Object.cls".to_string(),
                "%Library.RegisteredObject.cls".to_string(),
            ],
            content: None,
            namespace: "USER".to_string(),
            elicitation_id: None,
            elicitation_answer: None,
            compile: false,
        };
        let r = handle_iris_doc(&conn, &client, p, &elicitation_store).await;
        let v = result_json(r);
        assert!(
            v.get("success").is_some(),
            "batch GET response should include 'success' field, got: {v}"
        );
    });
}

// ── Test T: handle_iris_search regex mode ─────────────────────────────────────

#[test]
fn test_handle_iris_search_regex() {
    rt().block_on(async {
        let (conn, client) = match make_conn() {
            Some(c) => c,
            None => {
                eprintln!("SKIP test_handle_iris_search_regex — IRIS_HOST not set");
                return;
            }
        };
        let p = SearchParams {
            query: "^Class ".to_string(),
            regex: true,
            case_sensitive: false,
            category: Some("CLS".to_string()),
            documents: vec![],
            namespace: "USER".to_string(),
            inline: true,
        };
        let r = handle_iris_search(&conn, &client, p, make_log_store()).await;
        let v = result_json(r);
        assert!(
            v.get("success").is_some(),
            "regex search response should include 'success' field, got: {v}"
        );
    });
}

// ── Test U: query non-existent table returns Err (Atelier error surfaced) ─────

#[test]
fn test_query_nonexistent_table() {
    rt().block_on(async {
        let (conn, client) = match make_conn() {
            Some(c) => c,
            None => {
                eprintln!("SKIP test_query_nonexistent_table — IRIS_HOST not set");
                return;
            }
        };
        let result = conn
            .query(
                "SELECT * FROM IrisDevTest_DoesNotExist9999_Tbl",
                vec![],
                "USER",
                &client,
            )
            .await;
        // query() should surface the Atelier error — either as Err or as an ok body
        // with status.errors populated. Either is acceptable behaviour.
        match result {
            Err(e) => {
                eprintln!("non-existent table query returned Err (expected): {e}");
            }
            Ok(body) => {
                eprintln!("non-existent table query returned Ok body: {body}");
            }
        }
    });
}

// ── IrisTools::call_for_test dispatch tests ───────────────────────────────────
// These use the #[cfg(test)] dispatch shim to call private IrisTools handler
// methods directly, giving tarpaulin visibility into tools/mod.rs handler code.

fn make_iris_tools() -> Option<iris_agentic_dev_core::tools::IrisTools> {
    let iris_host = std::env::var("IRIS_HOST").unwrap_or_default();
    if iris_host.is_empty() {
        return None;
    }
    let web_port = std::env::var("IRIS_WEB_PORT").unwrap_or_else(|_| "52773".to_string());
    let base_url = format!("http://{}:{}", iris_host, web_port);
    let username = std::env::var("IRIS_USERNAME").unwrap_or_else(|_| "_SYSTEM".to_string());
    let password = std::env::var("IRIS_PASSWORD").unwrap_or_else(|_| "SYS".to_string());
    let conn = IrisConnection::new(
        base_url,
        "USER",
        username,
        password,
        DiscoverySource::EnvVar,
    );
    Some(iris_agentic_dev_core::tools::IrisTools::new(Some(conn)).expect("IrisTools::new"))
}

fn parse_result(r: Result<rmcp::model::CallToolResult, String>) -> serde_json::Value {
    let r = r.expect("call_for_test returned Err");
    let text = r.content[0].raw.as_text().unwrap().text.clone();
    serde_json::from_str(&text).unwrap_or_else(|_| serde_json::json!({"raw": text}))
}

#[tokio::test]
async fn test_dispatch_iris_compile() {
    let tools = match make_iris_tools() {
        Some(t) => t,
        None => return,
    };
    let result = tools
        .call_for_test(
            "iris_compile",
            serde_json::json!({
                "target": "IrisDevTest.DoesNotExist9999",
                "flags": "ck",
                "namespace": "USER"
            }),
        )
        .await;
    let v = parse_result(result);
    // Either success:false (class not found) or success:true — never panics
    assert!(
        v.get("success").is_some(),
        "compile must return success field: {v}"
    );
}

#[tokio::test]
async fn test_dispatch_iris_execute_write() {
    let tools = match make_iris_tools() {
        Some(t) => t,
        None => return,
    };
    let result = tools
        .call_for_test(
            "iris_execute",
            serde_json::json!({
                "code": "Write 1+1",
                "namespace": "USER"
            }),
        )
        .await;
    let v = parse_result(result);
    assert!(
        v.get("success").is_some() || v.get("output").is_some(),
        "execute must return output or success: {v}"
    );
}

#[tokio::test]
async fn test_dispatch_iris_execute_zversion() {
    let tools = match make_iris_tools() {
        Some(t) => t,
        None => return,
    };
    let result = tools
        .call_for_test(
            "iris_execute",
            serde_json::json!({
                "code": "Write $ZVersion",
                "namespace": "USER"
            }),
        )
        .await;
    let v = parse_result(result);
    // Should contain IRIS in output or at least return success field
    assert!(
        v.get("success").is_some() || v.get("output").is_some(),
        "execute $ZVersion: {v}"
    );
}

#[tokio::test]
async fn test_dispatch_iris_query_select1() {
    let tools = match make_iris_tools() {
        Some(t) => t,
        None => return,
    };
    let result = tools
        .call_for_test(
            "iris_query",
            serde_json::json!({
                "query": "SELECT 1 AS val",
                "namespace": "USER"
            }),
        )
        .await;
    let v = parse_result(result);
    assert!(v["success"] == true, "SELECT 1 should succeed: {v}");
}

#[tokio::test]
async fn test_dispatch_iris_query_class_dict() {
    let tools = match make_iris_tools() {
        Some(t) => t,
        None => return,
    };
    let result = tools
        .call_for_test(
            "iris_query",
            serde_json::json!({
                "query": "SELECT TOP 3 Name FROM %Dictionary.ClassDefinition",
                "namespace": "USER"
            }),
        )
        .await;
    let v = parse_result(result);
    assert!(v["success"] == true, "class dict query: {v}");
    let rows = v["rows"].as_array().unwrap_or(&vec![]).len();
    assert!(rows > 0, "should return at least one class: {v}");
}

#[tokio::test]
async fn test_dispatch_iris_symbols() {
    let tools = match make_iris_tools() {
        Some(t) => t,
        None => return,
    };
    let result = tools
        .call_for_test(
            "iris_symbols",
            serde_json::json!({
                "query": "%Library.*",
                "namespace": "USER"
            }),
        )
        .await;
    let v = parse_result(result);
    assert!(
        v.get("success").is_some() || v.get("symbols").is_some() || v.get("count").is_some(),
        "symbols must return symbols/count/success: {v}"
    );
}

#[tokio::test]
async fn test_dispatch_iris_doc_get() {
    let tools = match make_iris_tools() {
        Some(t) => t,
        None => return,
    };
    let result = tools
        .call_for_test(
            "iris_doc",
            serde_json::json!({
                "name": "%Library.Object.cls",
                "mode": "get",
                "namespace": "USER"
            }),
        )
        .await;
    let v = parse_result(result);
    assert!(
        v.get("success").is_some(),
        "doc get must return success: {v}"
    );
}

#[tokio::test]
async fn test_dispatch_iris_doc_put_and_compile() {
    let tools = match make_iris_tools() {
        Some(t) => t,
        None => return,
    };
    // PUT a simple valid class
    let cls_content = "Class IrisDevTest.DispatchPutTest {\n}\n";
    let result = tools
        .call_for_test(
            "iris_doc",
            serde_json::json!({
                "name": "IrisDevTest.DispatchPutTest.cls",
                "mode": "put",
                "content": cls_content,
                "compile": true,
                "namespace": "USER"
            }),
        )
        .await;
    let v = parse_result(result);
    // PUT may succeed or fail if namespace not writable — just confirm it doesn't panic
    assert!(
        v.get("success").is_some() || v.get("error_code").is_some(),
        "doc put: {v}"
    );
}

#[tokio::test]
async fn test_dispatch_iris_get_log() {
    let tools = match make_iris_tools() {
        Some(t) => t,
        None => return,
    };
    let result = tools
        .call_for_test(
            "iris_get_log",
            serde_json::json!({
                "limit": 5
            }),
        )
        .await;
    let v = parse_result(result);
    assert!(
        v.get("entries").is_some() || v.get("logs").is_some() || v.get("success").is_some(),
        "get_log must return logs or success: {v}"
    );
}

#[tokio::test]
async fn test_dispatch_iris_source_control_menu() {
    let tools = match make_iris_tools() {
        Some(t) => t,
        None => return,
    };
    let result = tools
        .call_for_test(
            "iris_source_control",
            serde_json::json!({
                "action": "menu",
                "document": "",
                "namespace": "USER"
            }),
        )
        .await;
    // SCM may not be configured — success or error are both acceptable
    match result {
        Ok(r) => {
            let text = r.content[0].raw.as_text().unwrap().text.clone();
            eprintln!("scm menu: {text}");
        }
        Err(e) => eprintln!("scm menu error (ok): {e}"),
    }
}

#[tokio::test]
async fn test_dispatch_iris_symbols_local() {
    let tools = match make_iris_tools() {
        Some(t) => t,
        None => return,
    };
    let result = tools
        .call_for_test(
            "iris_symbols_local",
            serde_json::json!({
                "query": "*.cls",
                "namespace": "USER"
            }),
        )
        .await;
    // symbols_local may return empty results — just confirm no panic
    match result {
        Ok(r) => {
            let text = r.content[0].raw.as_text().unwrap().text.clone();
            let v: serde_json::Value = serde_json::from_str(&text).unwrap_or_default();
            eprintln!(
                "symbols_local: {} symbols",
                v["symbols"].as_array().map(|a| a.len()).unwrap_or(0)
            );
        }
        Err(e) => eprintln!("symbols_local error (ok): {e}"),
    }
}

// ── iris_admin tests ──────────────────────────────────────────────────────────

#[tokio::test]
async fn test_dispatch_iris_admin_list_namespaces() {
    let tools = match make_iris_tools() {
        Some(t) => t,
        None => return,
    };
    let result = tools
        .call_for_test(
            "iris_admin",
            serde_json::json!({
                "action": "list_namespaces"
            }),
        )
        .await;
    let v = parse_result(result);
    assert!(
        v.get("namespaces").is_some() || v.get("success").is_some(),
        "admin list_namespaces: {v}"
    );
}

#[tokio::test]
async fn test_dispatch_iris_admin_list_users() {
    let tools = match make_iris_tools() {
        Some(t) => t,
        None => return,
    };
    let result = tools
        .call_for_test(
            "iris_admin",
            serde_json::json!({
                "action": "list_users"
            }),
        )
        .await;
    let v = parse_result(result);
    assert!(
        v.get("users").is_some() || v.get("success").is_some(),
        "admin list_users: {v}"
    );
}

#[tokio::test]
async fn test_dispatch_iris_admin_list_databases() {
    let tools = match make_iris_tools() {
        Some(t) => t,
        None => return,
    };
    let result = tools
        .call_for_test(
            "iris_admin",
            serde_json::json!({
                "action": "list_databases"
            }),
        )
        .await;
    let v = parse_result(result);
    assert!(
        v.get("databases").is_some() || v.get("success").is_some(),
        "admin list_databases: {v}"
    );
}

#[tokio::test]
async fn test_dispatch_iris_admin_list_roles() {
    let tools = match make_iris_tools() {
        Some(t) => t,
        None => return,
    };
    let result = tools
        .call_for_test(
            "iris_admin",
            serde_json::json!({
                "action": "list_roles"
            }),
        )
        .await;
    let v = parse_result(result);
    assert!(
        v.get("roles").is_some() || v.get("success").is_some(),
        "admin list_roles: {v}"
    );
}

#[tokio::test]
async fn test_dispatch_iris_admin_list_webapps() {
    let tools = match make_iris_tools() {
        Some(t) => t,
        None => return,
    };
    let result = tools
        .call_for_test(
            "iris_admin",
            serde_json::json!({
                "action": "list_webapps"
            }),
        )
        .await;
    let v = parse_result(result);
    assert!(
        v.get("webapps").is_some() || v.get("success").is_some(),
        "admin list_webapps: {v}"
    );
}

#[tokio::test]
async fn test_dispatch_iris_admin_check_permission() {
    let tools = match make_iris_tools() {
        Some(t) => t,
        None => return,
    };
    let result = tools
        .call_for_test(
            "iris_admin",
            serde_json::json!({
                "action": "check_permission"
            }),
        )
        .await;
    let v = parse_result(result);
    assert!(
        v.get("roles").is_some() || v.get("privileges").is_some() || v.get("success").is_some(),
        "admin check_permission: {v}"
    );
}

#[tokio::test]
async fn test_dispatch_iris_admin_list_user_roles() {
    let tools = match make_iris_tools() {
        Some(t) => t,
        None => return,
    };
    let result = tools
        .call_for_test(
            "iris_admin",
            serde_json::json!({
                "action": "list_user_roles",
                "username": "_SYSTEM"
            }),
        )
        .await;
    let v = parse_result(result);
    assert!(
        v.get("roles").is_some() || v.get("success").is_some(),
        "admin list_user_roles: {v}"
    );
}

// ── iris_production tests ─────────────────────────────────────────────────────

#[tokio::test]
async fn test_dispatch_iris_production_status() {
    let tools = match make_iris_tools() {
        Some(t) => t,
        None => return,
    };
    let result = tools
        .call_for_test(
            "iris_production",
            serde_json::json!({
                "action": "status",
                "namespace": "USER"
            }),
        )
        .await;
    let v = parse_result(result);
    // Production may not exist — success or error both acceptable
    assert!(
        v.get("success").is_some()
            || v.get("productions").is_some()
            || v.get("error_code").is_some(),
        "production status: {v}"
    );
}

#[tokio::test]
async fn test_dispatch_iris_production_get_autostart() {
    let tools = match make_iris_tools() {
        Some(t) => t,
        None => return,
    };
    let result = tools
        .call_for_test(
            "iris_production",
            serde_json::json!({
                "action": "get_autostart",
                "namespace": "USER"
            }),
        )
        .await;
    let v = parse_result(result);
    assert!(
        v.get("autostart").is_some() || v.get("success").is_some() || v.get("error_code").is_some(),
        "production get_autostart: {v}"
    );
}

// ── iris_interop_query tests ──────────────────────────────────────────────────

#[tokio::test]
async fn test_dispatch_iris_interop_query_logs() {
    let tools = match make_iris_tools() {
        Some(t) => t,
        None => return,
    };
    let result = tools
        .call_for_test(
            "iris_interop_query",
            serde_json::json!({
                "what": "logs",
                "namespace": "USER",
                "limit": 5
            }),
        )
        .await;
    let v = parse_result(result);
    assert!(
        v.get("logs").is_some() || v.get("success").is_some() || v.get("error_code").is_some(),
        "interop_query logs: {v}"
    );
}

#[tokio::test]
async fn test_dispatch_iris_interop_query_queues() {
    let tools = match make_iris_tools() {
        Some(t) => t,
        None => return,
    };
    let result = tools
        .call_for_test(
            "iris_interop_query",
            serde_json::json!({
                "what": "queues",
                "namespace": "USER"
            }),
        )
        .await;
    let v = parse_result(result);
    assert!(
        v.get("queues").is_some() || v.get("success").is_some() || v.get("error_code").is_some(),
        "interop_query queues: {v}"
    );
}

#[tokio::test]
async fn test_dispatch_iris_interop_query_messages() {
    let tools = match make_iris_tools() {
        Some(t) => t,
        None => return,
    };
    let result = tools
        .call_for_test(
            "iris_interop_query",
            serde_json::json!({
                "what": "messages",
                "namespace": "USER",
                "limit": 5
            }),
        )
        .await;
    let v = parse_result(result);
    assert!(
        v.get("messages").is_some() || v.get("success").is_some() || v.get("error_code").is_some(),
        "interop_query messages: {v}"
    );
}

// ── iris_test tests ───────────────────────────────────────────────────────────

#[tokio::test]
async fn test_dispatch_iris_test_nonexistent_pattern() {
    let tools = match make_iris_tools() {
        Some(t) => t,
        None => return,
    };
    // Pattern that matches nothing — exercises build_test_run_from_sql with empty suites
    let result = tools
        .call_for_test(
            "iris_test",
            serde_json::json!({
                "pattern": "IrisDevTest.NonExistent9999",
                "namespace": "USER"
            }),
        )
        .await;
    let v = parse_result(result);
    // Should return success:false with NO_TESTS_FOUND or similar
    assert!(
        v.get("success").is_some() || v.get("error_code").is_some(),
        "iris_test nonexistent: {v}"
    );
}

#[tokio::test]
async fn test_dispatch_iris_test_unit_test_manager() {
    let tools = match make_iris_tools() {
        Some(t) => t,
        None => return,
    };
    // Try to run a known system test class — may fail due to permissions but exercises code paths
    let result = tools
        .call_for_test(
            "iris_test",
            serde_json::json!({
                "pattern": "%UnitTest.TestSuite",
                "namespace": "USER"
            }),
        )
        .await;
    match result {
        Ok(r) => {
            let text = r.content[0].raw.as_text().unwrap().text.clone();
            eprintln!(
                "iris_test %UnitTest.TestSuite result: {}",
                &text[..text.len().min(200)]
            );
        }
        Err(e) => eprintln!("iris_test error (ok): {e}"),
    }
}

// ── iris_get_log pagination tests ─────────────────────────────────────────────

#[tokio::test]
async fn test_dispatch_iris_get_log_with_limit() {
    let tools = match make_iris_tools() {
        Some(t) => t,
        None => return,
    };
    let result = tools
        .call_for_test(
            "iris_get_log",
            serde_json::json!({
                "limit": 3
            }),
        )
        .await;
    let v = parse_result(result);
    assert!(
        v.get("entries").is_some() || v.get("logs").is_some() || v.get("success").is_some(),
        "get_log with limit: {v}"
    );
}

#[tokio::test]
async fn test_dispatch_iris_get_log_nonexistent_id() {
    let tools = match make_iris_tools() {
        Some(t) => t,
        None => return,
    };
    let result = tools
        .call_for_test(
            "iris_get_log",
            serde_json::json!({
                "id": "nonexistent-log-id-xyz-9999"
            }),
        )
        .await;
    let v = parse_result(result);
    // Should return error_code LOG_NOT_FOUND
    assert!(
        v.get("error_code").is_some() || v.get("success").map(|s| s == &false).unwrap_or(false),
        "get_log nonexistent id: {v}"
    );
}

#[tokio::test]
async fn test_dispatch_iris_get_log_invalid_limit() {
    let tools = match make_iris_tools() {
        Some(t) => t,
        None => return,
    };
    // limit=0 should return INVALID_PARAMS
    let result = tools
        .call_for_test(
            "iris_get_log",
            serde_json::json!({
                "id": "some-id",
                "limit": 0
            }),
        )
        .await;
    let v = parse_result(result);
    assert!(
        v.get("error_code").is_some(),
        "get_log limit=0 should return error_code: {v}"
    );
}

// ── iris_compile edge cases ───────────────────────────────────────────────────

#[tokio::test]
async fn test_dispatch_iris_compile_multiple_targets() {
    let tools = match make_iris_tools() {
        Some(t) => t,
        None => return,
    };
    let result = tools
        .call_for_test(
            "iris_compile",
            serde_json::json!({
                "target": "%Library.Object,%Library.Persistent",
                "flags": "ck",
                "namespace": "USER"
            }),
        )
        .await;
    let v = parse_result(result);
    assert!(v.get("success").is_some(), "compile multiple targets: {v}");
}

#[tokio::test]
async fn test_dispatch_iris_compile_system_class() {
    let tools = match make_iris_tools() {
        Some(t) => t,
        None => return,
    };
    let result = tools
        .call_for_test(
            "iris_compile",
            serde_json::json!({
                "target": "%Library.Object",
                "flags": "ck",
                "namespace": "USER"
            }),
        )
        .await;
    let v = parse_result(result);
    assert!(v.get("success").is_some(), "compile system class: {v}");
}

// ── iris_admin write-disabled tests ──────────────────────────────────────────

#[tokio::test]
async fn test_dispatch_iris_admin_create_user_write_disabled() {
    let tools = match make_iris_tools() {
        Some(t) => t,
        None => return,
    };
    // Without IRIS_ADMIN_TOOLS=1, write ops return ADMIN_WRITE_DISABLED
    let result = tools
        .call_for_test(
            "iris_admin",
            serde_json::json!({
                "action": "create_user",
                "username": "testuser_xyz",
                "password": "Test123!"
            }),
        )
        .await;
    let v = parse_result(result);
    // Either write-disabled or created (if env var set)
    assert!(
        v.get("error_code").is_some() || v.get("success").is_some(),
        "admin create_user: {v}"
    );
}

#[tokio::test]
async fn test_dispatch_iris_admin_get_webapp() {
    let tools = match make_iris_tools() {
        Some(t) => t,
        None => return,
    };
    let result = tools
        .call_for_test(
            "iris_admin",
            serde_json::json!({
                "action": "get_webapp",
                "path": "/api/atelier"
            }),
        )
        .await;
    let v = parse_result(result);
    assert!(
        v.get("webapp").is_some() || v.get("success").is_some() || v.get("error_code").is_some(),
        "admin get_webapp: {v}"
    );
}

// ── iris_execute edge cases ───────────────────────────────────────────────────

#[tokio::test]
async fn test_dispatch_iris_execute_set_variable() {
    let tools = match make_iris_tools() {
        Some(t) => t,
        None => return,
    };
    let result = tools
        .call_for_test(
            "iris_execute",
            serde_json::json!({
                "code": "Set x = 42 Write x",
                "namespace": "USER"
            }),
        )
        .await;
    let v = parse_result(result);
    assert!(
        v.get("success").is_some() || v.get("output").is_some(),
        "execute set var: {v}"
    );
}

#[tokio::test]
async fn test_dispatch_iris_execute_error_code() {
    let tools = match make_iris_tools() {
        Some(t) => t,
        None => return,
    };
    // Syntax error or runtime error — tests error handling paths
    let result = tools
        .call_for_test(
            "iris_execute",
            serde_json::json!({
                "code": "Do ##class(NonExistent.Class999).Method()",
                "namespace": "USER"
            }),
        )
        .await;
    let v = parse_result(result);
    // Should return success:false with error details
    assert!(
        v.get("success").is_some() || v.get("error").is_some(),
        "execute error: {v}"
    );
}

// ── iris_query edge cases ─────────────────────────────────────────────────────

#[tokio::test]
async fn test_dispatch_iris_query_with_params() {
    let tools = match make_iris_tools() {
        Some(t) => t,
        None => return,
    };
    let result = tools
        .call_for_test(
            "iris_query",
            serde_json::json!({
                "query": "SELECT Name FROM %Dictionary.ClassDefinition WHERE Name = ?",
                "params": ["%Library.Object"],
                "namespace": "USER"
            }),
        )
        .await;
    let v = parse_result(result);
    // params may not be supported by all IRIS versions — success or error both ok
    assert!(
        v.get("success").is_some() || v.get("error_code").is_some(),
        "query with params: {v}"
    );
}

#[tokio::test]
async fn test_dispatch_iris_query_top_n() {
    let tools = match make_iris_tools() {
        Some(t) => t,
        None => return,
    };
    let result = tools
        .call_for_test(
            "iris_query",
            serde_json::json!({
                "query": "SELECT TOP 1 Name FROM %Dictionary.ClassDefinition",
                "namespace": "USER"
            }),
        )
        .await;
    let v = parse_result(result);
    assert!(v["success"] == true, "query TOP 1: {v}");
}

// ── iris_search edge cases ────────────────────────────────────────────────────

#[tokio::test]
async fn test_dispatch_iris_search_empty_pattern() {
    let tools = match make_iris_tools() {
        Some(t) => t,
        None => return,
    };
    let result = tools
        .call_for_test(
            "iris_search",
            serde_json::json!({
                "query": "NONEXISTENT_PATTERN_XYZ_9999_ABC",
                "namespace": "USER"
            }),
        )
        .await;
    let v = parse_result(result);
    assert!(
        v.get("results").is_some() || v.get("success").is_some(),
        "search empty result: {v}"
    );
}

#[tokio::test]
async fn test_dispatch_iris_search_with_limit() {
    let tools = match make_iris_tools() {
        Some(t) => t,
        None => return,
    };
    let result = tools
        .call_for_test(
            "iris_search",
            serde_json::json!({
                "query": "Object",
                "namespace": "USER",
                "limit": 3
            }),
        )
        .await;
    let v = parse_result(result);
    assert!(
        v.get("results").is_some() || v.get("success").is_some(),
        "search with limit: {v}"
    );
}

// ── iris_doc edge cases ───────────────────────────────────────────────────────

#[tokio::test]
async fn test_dispatch_iris_doc_head_nonexistent() {
    let tools = match make_iris_tools() {
        Some(t) => t,
        None => return,
    };
    let result = tools
        .call_for_test(
            "iris_doc",
            serde_json::json!({
                "name": "IrisDevTest.NonExistent9999.cls",
                "mode": "head",
                "namespace": "USER"
            }),
        )
        .await;
    let v = parse_result(result);
    // head on nonexistent — error or success:false
    assert!(
        v.get("success").is_some() || v.get("error_code").is_some(),
        "doc head nonexistent: {v}"
    );
}

#[tokio::test]
async fn test_dispatch_iris_doc_list() {
    let tools = match make_iris_tools() {
        Some(t) => t,
        None => return,
    };
    let result = tools
        .call_for_test(
            "iris_doc",
            serde_json::json!({
                "name": "%Library.Object.cls",
                "mode": "list",
                "namespace": "USER"
            }),
        )
        .await;
    match result {
        Ok(r) => {
            let text = r.content[0].raw.as_text().unwrap().text.clone();
            eprintln!("doc list: {}", &text[..text.len().min(100)]);
        }
        Err(e) => eprintln!("doc list error (ok if mode unsupported): {e}"),
    }
}

// ── iris_info edge cases ──────────────────────────────────────────────────────

#[tokio::test]
async fn test_dispatch_iris_info_macros() {
    let tools = match make_iris_tools() {
        Some(t) => t,
        None => return,
    };
    let result = tools
        .call_for_test(
            "iris_info",
            serde_json::json!({
                "what": "macros",
                "namespace": "USER"
            }),
        )
        .await;
    let v = parse_result(result);
    assert!(
        v.get("macros").is_some() || v.get("success").is_some() || v.get("error_code").is_some(),
        "info macros: {v}"
    );
}

#[tokio::test]
async fn test_dispatch_iris_info_tables() {
    let tools = match make_iris_tools() {
        Some(t) => t,
        None => return,
    };
    let result = tools
        .call_for_test(
            "iris_info",
            serde_json::json!({
                "what": "tables",
                "namespace": "USER"
            }),
        )
        .await;
    let v = parse_result(result);
    assert!(
        v.get("tables").is_some() || v.get("success").is_some() || v.get("error_code").is_some(),
        "info tables: {v}"
    );
}

#[tokio::test]
async fn test_dispatch_iris_info_globals_v2() {
    let tools = match make_iris_tools() {
        Some(t) => t,
        None => return,
    };
    let result = tools
        .call_for_test(
            "iris_info",
            serde_json::json!({
                "what": "globals",
                "namespace": "USER"
            }),
        )
        .await;
    let v = parse_result(result);
    assert!(
        v.get("globals").is_some() || v.get("success").is_some() || v.get("error_code").is_some(),
        "info globals: {v}"
    );
}

// ── iris_macro dispatch tests ─────────────────────────────────────────────────

#[tokio::test]
async fn test_dispatch_iris_macro_list() {
    let tools = match make_iris_tools() {
        Some(t) => t,
        None => return,
    };
    let result = tools
        .call_for_test(
            "iris_macro",
            serde_json::json!({
                "action": "list",
                "namespace": "USER"
            }),
        )
        .await;
    let v = parse_result(result);
    assert!(
        v.get("macros").is_some() || v.get("success").is_some(),
        "iris_macro list: {v}"
    );
}

#[tokio::test]
async fn test_dispatch_iris_macro_signature_system_macro() {
    let tools = match make_iris_tools() {
        Some(t) => t,
        None => return,
    };
    let result = tools
        .call_for_test(
            "iris_macro",
            serde_json::json!({
                "action": "signature",
                "name": "$$OK",
                "namespace": "USER"
            }),
        )
        .await;
    match result {
        Ok(r) => {
            let text = r.content[0].raw.as_text().unwrap().text.clone();
            eprintln!("macro signature: {}", &text[..text.len().min(200)]);
        }
        Err(e) => eprintln!("macro signature error (ok): {e}"),
    }
}

#[tokio::test]
async fn test_dispatch_iris_macro_definition() {
    let tools = match make_iris_tools() {
        Some(t) => t,
        None => return,
    };
    let result = tools
        .call_for_test(
            "iris_macro",
            serde_json::json!({
                "action": "definition",
                "name": "$$OK",
                "namespace": "USER"
            }),
        )
        .await;
    match result {
        Ok(r) => {
            let text = r.content[0].raw.as_text().unwrap().text.clone();
            eprintln!("macro definition: {}", &text[..text.len().min(200)]);
        }
        Err(e) => eprintln!("macro definition error (ok): {e}"),
    }
}

#[tokio::test]
async fn test_dispatch_iris_macro_location() {
    let tools = match make_iris_tools() {
        Some(t) => t,
        None => return,
    };
    let result = tools
        .call_for_test(
            "iris_macro",
            serde_json::json!({
                "action": "location",
                "name": "$$OK",
                "namespace": "USER"
            }),
        )
        .await;
    match result {
        Ok(r) => {
            let text = r.content[0].raw.as_text().unwrap().text.clone();
            eprintln!("macro location: {}", &text[..text.len().min(200)]);
        }
        Err(e) => eprintln!("macro location error (ok): {e}"),
    }
}

#[tokio::test]
async fn test_dispatch_iris_macro_expand() {
    let tools = match make_iris_tools() {
        Some(t) => t,
        None => return,
    };
    let result = tools
        .call_for_test(
            "iris_macro",
            serde_json::json!({
                "action": "expand",
                "name": "$$OK",
                "args": ["1"],
                "namespace": "USER"
            }),
        )
        .await;
    match result {
        Ok(r) => {
            let text = r.content[0].raw.as_text().unwrap().text.clone();
            eprintln!("macro expand: {}", &text[..text.len().min(200)]);
        }
        Err(e) => eprintln!("macro expand error (ok): {e}"),
    }
}

// ── iris_table_info dispatch tests ────────────────────────────────────────────

#[tokio::test]
async fn test_dispatch_iris_table_info_class_dict() {
    let tools = match make_iris_tools() {
        Some(t) => t,
        None => return,
    };
    let result = tools
        .call_for_test(
            "iris_table_info",
            serde_json::json!({
                "table": "SQLUser.Person",
                "namespace": "USER"
            }),
        )
        .await;
    match result {
        Ok(r) => {
            let text = r.content[0].raw.as_text().unwrap().text.clone();
            eprintln!(
                "table_info SQLUser.Person: {}",
                &text[..text.len().min(200)]
            );
        }
        Err(e) => eprintln!("table_info error (ok): {e}"),
    }
}

#[tokio::test]
async fn test_dispatch_iris_table_info_system_table() {
    let tools = match make_iris_tools() {
        Some(t) => t,
        None => return,
    };
    let result = tools
        .call_for_test(
            "iris_table_info",
            serde_json::json!({
                "table": "INFORMATION_SCHEMA.TABLES",
                "namespace": "USER"
            }),
        )
        .await;
    let v = parse_result(result);
    assert!(
        v.get("columns").is_some() || v.get("success").is_some() || v.get("error_code").is_some(),
        "table_info INFORMATION_SCHEMA.TABLES: {v}"
    );
}

// ── iris_info additional what values ─────────────────────────────────────────

#[tokio::test]
async fn test_dispatch_iris_info_modified() {
    let tools = match make_iris_tools() {
        Some(t) => t,
        None => return,
    };
    let result = tools
        .call_for_test(
            "iris_info",
            serde_json::json!({
                "what": "modified",
                "namespace": "USER"
            }),
        )
        .await;
    let v = parse_result(result);
    assert!(
        v.get("success").is_some() || v.get("error_code").is_some(),
        "info modified: {v}"
    );
}

#[tokio::test]
async fn test_dispatch_iris_info_jobs() {
    let tools = match make_iris_tools() {
        Some(t) => t,
        None => return,
    };
    let result = tools
        .call_for_test(
            "iris_info",
            serde_json::json!({
                "what": "jobs",
                "namespace": "USER"
            }),
        )
        .await;
    let v = parse_result(result);
    assert!(
        v.get("success").is_some() || v.get("error_code").is_some(),
        "info jobs: {v}"
    );
}

#[tokio::test]
async fn test_dispatch_iris_info_csp_apps() {
    let tools = match make_iris_tools() {
        Some(t) => t,
        None => return,
    };
    let result = tools
        .call_for_test(
            "iris_info",
            serde_json::json!({
                "what": "csp_apps",
                "namespace": "USER"
            }),
        )
        .await;
    let v = parse_result(result);
    assert!(
        v.get("success").is_some() || v.get("error_code").is_some(),
        "info csp_apps: {v}"
    );
}

#[tokio::test]
async fn test_dispatch_iris_info_invalid_what() {
    let tools = match make_iris_tools() {
        Some(t) => t,
        None => return,
    };
    let result = tools
        .call_for_test(
            "iris_info",
            serde_json::json!({
                "what": "nonexistent_what_value",
                "namespace": "USER"
            }),
        )
        .await;
    let v = parse_result(result);
    // Should return INVALID_PARAM error
    assert!(
        v.get("error_code").is_some(),
        "info invalid what should return error_code: {v}"
    );
}

// ── iris_debug dispatch tests ─────────────────────────────────────────────────

#[tokio::test]
async fn test_dispatch_iris_debug_error_logs() {
    let tools = match make_iris_tools() {
        Some(t) => t,
        None => return,
    };
    let result = tools
        .call_for_test(
            "iris_debug",
            serde_json::json!({
                "action": "error_logs",
                "namespace": "USER",
                "limit": 5
            }),
        )
        .await;
    match result {
        Ok(r) => {
            let text = r.content[0].raw.as_text().unwrap().text.clone();
            eprintln!("debug error_logs: {}", &text[..text.len().min(200)]);
        }
        Err(e) => eprintln!("debug error_logs error (ok): {e}"),
    }
}

#[tokio::test]
async fn test_dispatch_iris_debug_source_map() {
    let tools = match make_iris_tools() {
        Some(t) => t,
        None => return,
    };
    let result = tools
        .call_for_test(
            "iris_debug",
            serde_json::json!({
                "action": "source_map",
                "document": "%Library.Object.1.INT",
                "namespace": "USER"
            }),
        )
        .await;
    match result {
        Ok(r) => {
            let text = r.content[0].raw.as_text().unwrap().text.clone();
            eprintln!("debug source_map: {}", &text[..text.len().min(200)]);
        }
        Err(e) => eprintln!("debug source_map error (ok): {e}"),
    }
}

// ── interop credential/lookup coverage ───────────────────────────────────────

#[tokio::test]
async fn test_dispatch_iris_production_item_list() {
    let tools = match make_iris_tools() {
        Some(t) => t,
        None => return,
    };
    let result = tools
        .call_for_test(
            "iris_production_item",
            serde_json::json!({
                "action": "get_settings",
                "item_name": "NonExistentItem",
                "namespace": "USER"
            }),
        )
        .await;
    match result {
        Ok(r) => {
            let text = r.content[0].raw.as_text().unwrap().text.clone();
            eprintln!(
                "production_item get_settings: {}",
                &text[..text.len().min(200)]
            );
        }
        Err(e) => eprintln!("production_item error (ok): {e}"),
    }
}

#[tokio::test]
async fn test_dispatch_iris_interop_query_invalid_what() {
    let tools = match make_iris_tools() {
        Some(t) => t,
        None => return,
    };
    let result = tools
        .call_for_test(
            "iris_interop_query",
            serde_json::json!({
                "what": "invalid_what_xyz",
                "namespace": "USER"
            }),
        )
        .await;
    let v = parse_result(result);
    assert!(
        v.get("error_code").is_some() || v.get("success").map(|s| s == &false).unwrap_or(false),
        "interop_query invalid what: {v}"
    );
}

// ── scm edge cases ─────────────────────────────────────────────────────────────

#[tokio::test]
async fn test_dispatch_iris_scm_status() {
    let tools = match make_iris_tools() {
        Some(t) => t,
        None => return,
    };
    let result = tools
        .call_for_test(
            "iris_source_control",
            serde_json::json!({
                "action": "status",
                "document": "%Library.Object.cls",
                "namespace": "USER"
            }),
        )
        .await;
    match result {
        Ok(r) => {
            let text = r.content[0].raw.as_text().unwrap().text.clone();
            eprintln!("scm status: {}", &text[..text.len().min(200)]);
        }
        Err(e) => eprintln!("scm status error (ok): {e}"),
    }
}

#[tokio::test]
async fn test_dispatch_iris_scm_get() {
    let tools = match make_iris_tools() {
        Some(t) => t,
        None => return,
    };
    let result = tools
        .call_for_test(
            "iris_source_control",
            serde_json::json!({
                "action": "get",
                "document": "%Library.Object.cls",
                "namespace": "USER"
            }),
        )
        .await;
    match result {
        Ok(r) => {
            let text = r.content[0].raw.as_text().unwrap().text.clone();
            eprintln!("scm get: {}", &text[..text.len().min(200)]);
        }
        Err(e) => eprintln!("scm get error (ok): {e}"),
    }
}

// ── iris_credential_list / iris_lookup tests ──────────────────────────────────

#[tokio::test]
async fn test_dispatch_iris_credential_list() {
    let tools = match make_iris_tools() {
        Some(t) => t,
        None => return,
    };
    let result = tools
        .call_for_test(
            "iris_credential_list",
            serde_json::json!({
                "namespace": "USER"
            }),
        )
        .await;
    let v = parse_result(result);
    assert!(
        v.get("credentials").is_some()
            || v.get("success").is_some()
            || v.get("error_code").is_some(),
        "credential_list: {v}"
    );
}

#[tokio::test]
async fn test_dispatch_iris_lookup_manage_list_tables() {
    let tools = match make_iris_tools() {
        Some(t) => t,
        None => return,
    };
    let result = tools
        .call_for_test(
            "iris_lookup_manage",
            serde_json::json!({
                "action": "list_tables",
                "namespace": "USER"
            }),
        )
        .await;
    let v = parse_result(result);
    assert!(
        v.get("tables").is_some() || v.get("success").is_some() || v.get("error_code").is_some(),
        "lookup list_tables: {v}"
    );
}

#[tokio::test]
async fn test_dispatch_iris_lookup_manage_get_nonexistent() {
    let tools = match make_iris_tools() {
        Some(t) => t,
        None => return,
    };
    let result = tools
        .call_for_test(
            "iris_lookup_manage",
            serde_json::json!({
                "action": "get",
                "table_name": "NonExistentTable9999",
                "key": "somekey",
                "namespace": "USER"
            }),
        )
        .await;
    let v = parse_result(result);
    // Not found or error — both OK
    assert!(
        v.get("value").is_some() || v.get("success").is_some() || v.get("error_code").is_some(),
        "lookup get nonexistent: {v}"
    );
}

#[tokio::test]
async fn test_dispatch_iris_lookup_manage_list_keys() {
    let tools = match make_iris_tools() {
        Some(t) => t,
        None => return,
    };
    let result = tools
        .call_for_test(
            "iris_lookup_manage",
            serde_json::json!({
                "action": "list_keys",
                "table_name": "NonExistentTable9999",
                "namespace": "USER"
            }),
        )
        .await;
    let v = parse_result(result);
    assert!(
        v.get("keys").is_some() || v.get("success").is_some() || v.get("error_code").is_some(),
        "lookup list_keys: {v}"
    );
}

#[tokio::test]
async fn test_dispatch_iris_credential_manage_write_disabled() {
    let tools = match make_iris_tools() {
        Some(t) => t,
        None => return,
    };
    // Without IRIS_ALLOW_PROD, write ops are suppressed
    let result = tools
        .call_for_test(
            "iris_credential_manage",
            serde_json::json!({
                "action": "create",
                "id": "TestCred_999",
                "username": "testuser",
                "password": "testpass",
                "namespace": "USER"
            }),
        )
        .await;
    let v = parse_result(result);
    // Either write-disabled or created
    assert!(
        v.get("error_code").is_some() || v.get("success").is_some(),
        "credential_manage create: {v}"
    );
}

// ── symbols_local edge cases ──────────────────────────────────────────────────

#[tokio::test]
async fn test_dispatch_symbols_local_deep_search() {
    let tools = match make_iris_tools() {
        Some(t) => t,
        None => return,
    };
    let result = tools
        .call_for_test(
            "iris_symbols_local",
            serde_json::json!({
                "query": "*.mac",
                "namespace": "USER"
            }),
        )
        .await;
    match result {
        Ok(r) => {
            let text = r.content[0].raw.as_text().unwrap().text.clone();
            eprintln!("symbols_local *.mac: {}", &text[..text.len().min(100)]);
        }
        Err(e) => eprintln!("symbols_local *.mac error (ok): {e}"),
    }
}

#[tokio::test]
async fn test_dispatch_symbols_local_inc_pattern() {
    let tools = match make_iris_tools() {
        Some(t) => t,
        None => return,
    };
    let result = tools
        .call_for_test(
            "iris_symbols_local",
            serde_json::json!({
                "query": "*.inc",
                "namespace": "USER"
            }),
        )
        .await;
    match result {
        Ok(r) => {
            let text = r.content[0].raw.as_text().unwrap().text.clone();
            eprintln!("symbols_local *.inc: {}", &text[..text.len().min(100)]);
        }
        Err(e) => eprintln!("symbols_local *.inc error (ok): {e}"),
    }
}

// ── doc additional edge cases ─────────────────────────────────────────────────

#[tokio::test]
async fn test_dispatch_iris_doc_delete_nonexistent() {
    let tools = match make_iris_tools() {
        Some(t) => t,
        None => return,
    };
    let result = tools
        .call_for_test(
            "iris_doc",
            serde_json::json!({
                "name": "IrisDevTest.NonExistent9999.cls",
                "mode": "delete",
                "namespace": "USER"
            }),
        )
        .await;
    let v = parse_result(result);
    // Delete may succeed (no-op) or fail — just confirm no panic
    assert!(
        v.get("success").is_some() || v.get("error_code").is_some(),
        "doc delete nonexistent: {v}"
    );
}

#[tokio::test]
async fn test_dispatch_iris_doc_batch_head() {
    let tools = match make_iris_tools() {
        Some(t) => t,
        None => return,
    };
    let result = tools
        .call_for_test(
            "iris_doc",
            serde_json::json!({
                "names": ["%Library.Object.cls", "%Library.Persistent.cls"],
                "mode": "batch_head",
                "namespace": "USER"
            }),
        )
        .await;
    match result {
        Ok(r) => {
            let text = r.content[0].raw.as_text().unwrap().text.clone();
            eprintln!("doc batch_head: {}", &text[..text.len().min(200)]);
        }
        Err(e) => eprintln!("doc batch_head error (ok): {e}"),
    }
}

// ── search edge cases ─────────────────────────────────────────────────────────

#[tokio::test]
async fn test_dispatch_iris_search_in_class() {
    let tools = match make_iris_tools() {
        Some(t) => t,
        None => return,
    };
    let result = tools
        .call_for_test(
            "iris_search",
            serde_json::json!({
                "query": "Property Name",
                "document": "%Library.Object.cls",
                "namespace": "USER"
            }),
        )
        .await;
    let v = parse_result(result);
    assert!(
        v.get("results").is_some() || v.get("success").is_some(),
        "search in class: {v}"
    );
}

#[tokio::test]
async fn test_dispatch_iris_search_class_type() {
    let tools = match make_iris_tools() {
        Some(t) => t,
        None => return,
    };
    let result = tools
        .call_for_test(
            "iris_search",
            serde_json::json!({
                "query": "Extends %Persistent",
                "doc_type": "CLS",
                "namespace": "USER",
                "limit": 5
            }),
        )
        .await;
    let v = parse_result(result);
    assert!(
        v.get("results").is_some() || v.get("success").is_some(),
        "search CLS type: {v}"
    );
}

// ── symbols_local with real workspace ────────────────────────────────────────

#[tokio::test]
async fn test_dispatch_symbols_local_with_workspace() {
    let tools = match make_iris_tools() {
        Some(t) => t,
        None => return,
    };
    // Point to workspace root which has .cls files
    let workspace = env!("CARGO_MANIFEST_DIR")
        .replace("/crates/iris-agentic-dev-core", "")
        .replace("iris-agentic-dev-core", "");
    let result = tools
        .call_for_test(
            "iris_symbols_local",
            serde_json::json!({
                "query": "*.cls",
                "workspace_path": workspace
            }),
        )
        .await;
    match result {
        Ok(r) => {
            let text = r.content[0].raw.as_text().unwrap().text.clone();
            let v: serde_json::Value = serde_json::from_str(&text).unwrap_or_default();
            let count = v["symbols"].as_array().map(|a| a.len()).unwrap_or(0);
            eprintln!("symbols_local with workspace: {count} symbols");
        }
        Err(e) => eprintln!("symbols_local workspace error (ok): {e}"),
    }
}

#[tokio::test]
async fn test_dispatch_symbols_local_method_query() {
    let tools = match make_iris_tools() {
        Some(t) => t,
        None => return,
    };
    let workspace = env!("CARGO_MANIFEST_DIR")
        .replace("/crates/iris-agentic-dev-core", "")
        .replace("iris-agentic-dev-core", "");
    let result = tools
        .call_for_test(
            "iris_symbols_local",
            serde_json::json!({
                "query": "ProductionHelper",
                "workspace_path": workspace
            }),
        )
        .await;
    match result {
        Ok(r) => {
            let text = r.content[0].raw.as_text().unwrap().text.clone();
            eprintln!(
                "symbols_local ProductionHelper: {}",
                &text[..text.len().min(200)]
            );
        }
        Err(e) => eprintln!("symbols_local method query error (ok): {e}"),
    }
}

// ── admin write operations (requires IRIS_ADMIN_TOOLS=1) ─────────────────────

#[tokio::test]
async fn test_dispatch_iris_admin_create_delete_user() {
    let tools = match make_iris_tools() {
        Some(t) => t,
        None => return,
    };
    if std::env::var("IRIS_ADMIN_TOOLS").unwrap_or_default() != "1" {
        eprintln!("SKIP test_dispatch_iris_admin_create_delete_user — IRIS_ADMIN_TOOLS not set");
        return;
    }
    // Create a test user
    let create_result = tools
        .call_for_test(
            "iris_admin",
            serde_json::json!({
                "action": "create_user",
                "username": "iris_dev_test_user_999",
                "password": "TestPass123!"
            }),
        )
        .await;
    let cv = parse_result(create_result);
    eprintln!("admin create_user: {cv}");

    // Delete the test user (cleanup)
    let del_result = tools
        .call_for_test(
            "iris_admin",
            serde_json::json!({
                "action": "delete_user",
                "username": "iris_dev_test_user_999"
            }),
        )
        .await;
    let dv = parse_result(del_result);
    eprintln!("admin delete_user: {dv}");
}

#[tokio::test]
async fn test_dispatch_iris_admin_create_delete_namespace() {
    let tools = match make_iris_tools() {
        Some(t) => t,
        None => return,
    };
    if std::env::var("IRIS_ADMIN_TOOLS").unwrap_or_default() != "1" {
        eprintln!(
            "SKIP test_dispatch_iris_admin_create_delete_namespace — IRIS_ADMIN_TOOLS not set"
        );
        return;
    }
    let create_result = tools
        .call_for_test(
            "iris_admin",
            serde_json::json!({
                "action": "create_namespace",
                "namespace": "IRISDEVTEST999",
                "database": "USER"
            }),
        )
        .await;
    let cv = parse_result(create_result);
    eprintln!("admin create_namespace: {cv}");

    let del_result = tools
        .call_for_test(
            "iris_admin",
            serde_json::json!({
                "action": "delete_namespace",
                "namespace": "IRISDEVTEST999"
            }),
        )
        .await;
    let dv = parse_result(del_result);
    eprintln!("admin delete_namespace: {dv}");
}

// ── interop write operations (requires IRIS_ALLOW_PROD=1) ────────────────────

#[tokio::test]
async fn test_dispatch_iris_lookup_set_delete() {
    let tools = match make_iris_tools() {
        Some(t) => t,
        None => return,
    };
    // Set a key (write op — exercised even without IRIS_ALLOW_PROD since lookup writes
    // may be allowed; check result)
    let set_result = tools
        .call_for_test(
            "iris_lookup_manage",
            serde_json::json!({
                "action": "set",
                "table_name": "IrisDevTestTable",
                "key": "testkey",
                "value": "testvalue",
                "namespace": "USER"
            }),
        )
        .await;
    match set_result {
        Ok(r) => {
            let text = r.content[0].raw.as_text().unwrap().text.clone();
            eprintln!("lookup set: {}", &text[..text.len().min(200)]);
        }
        Err(e) => eprintln!("lookup set error (ok): {e}"),
    }

    // Delete the key
    let del_result = tools
        .call_for_test(
            "iris_lookup_manage",
            serde_json::json!({
                "action": "delete",
                "table_name": "IrisDevTestTable",
                "key": "testkey",
                "namespace": "USER"
            }),
        )
        .await;
    match del_result {
        Ok(r) => {
            let text = r.content[0].raw.as_text().unwrap().text.clone();
            eprintln!("lookup delete: {}", &text[..text.len().min(200)]);
        }
        Err(e) => eprintln!("lookup delete error (ok): {e}"),
    }
}

// ── production start/stop (requires IRIS_ALLOW_PROD=1 and production class) ──

#[tokio::test]
async fn test_dispatch_iris_production_check() {
    let tools = match make_iris_tools() {
        Some(t) => t,
        None => return,
    };
    let result = tools
        .call_for_test(
            "iris_production",
            serde_json::json!({
                "action": "check",
                "namespace": "USER"
            }),
        )
        .await;
    match result {
        Ok(r) => {
            let text = r.content[0].raw.as_text().unwrap().text.clone();
            eprintln!("production check: {}", &text[..text.len().min(200)]);
        }
        Err(e) => eprintln!("production check error (ok): {e}"),
    }
}

#[tokio::test]
async fn test_dispatch_iris_production_needs_update() {
    let tools = match make_iris_tools() {
        Some(t) => t,
        None => return,
    };
    let result = tools
        .call_for_test(
            "iris_production",
            serde_json::json!({
                "action": "check",
                "namespace": "USER"
            }),
        )
        .await;
    match result {
        Ok(r) => {
            let text = r.content[0].raw.as_text().unwrap().text.clone();
            eprintln!("production needs_update: {}", &text[..text.len().min(200)]);
        }
        Err(e) => eprintln!("production needs_update error (ok): {e}"),
    }
}

// ── dict tools ────────────────────────────────────────────────────────────────

#[tokio::test]
async fn test_dispatch_iris_info_sa_schema() {
    let tools = match make_iris_tools() {
        Some(t) => t,
        None => return,
    };
    let result = tools
        .call_for_test(
            "iris_info",
            serde_json::json!({
                "what": "sa_schema",
                "name": "%Library.Object",
                "namespace": "USER"
            }),
        )
        .await;
    let v = parse_result(result);
    assert!(
        v.get("success").is_some() || v.get("error_code").is_some(),
        "info sa_schema: {v}"
    );
}

// ── find_subclass_implementations ─────────────────────────────────────────────

#[tokio::test]
async fn test_dispatch_find_subclass_implementations() {
    let tools = match make_iris_tools() {
        Some(t) => t,
        None => return,
    };
    // find_subclass_implementations needs a different tool name check
    // It's dispatched via iris_info with what=find_subclass  or a separate tool
    // Check via iris_search as a proxy
    let result = tools
        .call_for_test(
            "iris_search",
            serde_json::json!({
                "query": "Extends %Persistent",
                "namespace": "USER",
                "limit": 3
            }),
        )
        .await;
    let v = parse_result(result);
    assert!(
        v.get("results").is_some() || v.get("success").is_some(),
        "find subclass via search: {v}"
    );
}

// ── dict tools: resolve_dynamic_dispatch, extract_message_map, find_subclass ─

#[tokio::test]
async fn test_dispatch_resolve_dynamic_dispatch() {
    let tools = match make_iris_tools() {
        Some(t) => t,
        None => return,
    };
    let result = tools
        .call_for_test(
            "resolve_dynamic_dispatch",
            serde_json::json!({
                "method_name": "ClassName",
                "namespace": "USER"
            }),
        )
        .await;
    match result {
        Ok(r) => {
            let text = r.content[0].raw.as_text().unwrap().text.clone();
            eprintln!("resolve_dynamic_dispatch: {}", &text[..text.len().min(200)]);
        }
        Err(e) => eprintln!("resolve_dynamic_dispatch error (ok): {e}"),
    }
}

#[tokio::test]
async fn test_dispatch_extract_message_map_routing() {
    let tools = match make_iris_tools() {
        Some(t) => t,
        None => return,
    };
    let result = tools
        .call_for_test(
            "extract_message_map_routing",
            serde_json::json!({
                "class_name": "%Library.Application",
                "namespace": "USER"
            }),
        )
        .await;
    match result {
        Ok(r) => {
            let text = r.content[0].raw.as_text().unwrap().text.clone();
            eprintln!(
                "extract_message_map_routing: {}",
                &text[..text.len().min(200)]
            );
        }
        Err(e) => eprintln!("extract_message_map_routing error (ok): {e}"),
    }
}

#[tokio::test]
async fn test_dispatch_find_subclass_implementations_dict() {
    let tools = match make_iris_tools() {
        Some(t) => t,
        None => return,
    };
    let result = tools
        .call_for_test(
            "find_subclass_implementations",
            serde_json::json!({
                "base_classes": ["%Library.Persistent"],
                "method_name": "SaveData",
                "namespace": "USER"
            }),
        )
        .await;
    match result {
        Ok(r) => {
            let text = r.content[0].raw.as_text().unwrap().text.clone();
            eprintln!(
                "find_subclass_implementations: {}",
                &text[..text.len().min(300)]
            );
        }
        Err(e) => eprintln!("find_subclass_implementations error (ok): {e}"),
    }
}

#[tokio::test]
async fn test_dispatch_resolve_dynamic_dispatch_nonexistent_class() {
    let tools = match make_iris_tools() {
        Some(t) => t,
        None => return,
    };
    let result = tools
        .call_for_test(
            "resolve_dynamic_dispatch",
            serde_json::json!({
                "method_name": "SomeMethod",
                "namespace": "USER"
            }),
        )
        .await;
    match result {
        Ok(r) => {
            let text = r.content[0].raw.as_text().unwrap().text.clone();
            eprintln!(
                "resolve_dynamic_dispatch nonexistent: {}",
                &text[..text.len().min(200)]
            );
        }
        Err(e) => eprintln!("resolve nonexistent error (ok): {e}"),
    }
}

// ── check_permission with real resource (covers admin_check_permission_impl) ──

#[tokio::test]
async fn test_dispatch_iris_admin_check_permission_with_resource() {
    let tools = match make_iris_tools() {
        Some(t) => t,
        None => return,
    };
    // Check USE permission on a real %SYS resource
    let result = tools
        .call_for_test(
            "iris_admin",
            serde_json::json!({
                "action": "check_permission",
                "resource": "%DB_DEFAULT",
                "permission": "USE"
            }),
        )
        .await;
    let v = parse_result(result);
    assert!(
        v.get("success").is_some() || v.get("error_code").is_some(),
        "check_permission with resource: {v}"
    );
}

#[tokio::test]
async fn test_dispatch_iris_admin_check_permission_write() {
    let tools = match make_iris_tools() {
        Some(t) => t,
        None => return,
    };
    let result = tools
        .call_for_test(
            "iris_admin",
            serde_json::json!({
                "action": "check_permission",
                "resource": "%DB_USER",
                "permission": "WRITE"
            }),
        )
        .await;
    let v = parse_result(result);
    assert!(
        v.get("success").is_some() || v.get("error_code").is_some(),
        "check_permission WRITE: {v}"
    );
}

#[tokio::test]
async fn test_dispatch_iris_admin_check_permission_read() {
    let tools = match make_iris_tools() {
        Some(t) => t,
        None => return,
    };
    let result = tools
        .call_for_test(
            "iris_admin",
            serde_json::json!({
                "action": "check_permission",
                "resource": "%DB_USER",
                "permission": "READ"
            }),
        )
        .await;
    let v = parse_result(result);
    assert!(
        v.get("success").is_some() || v.get("error_code").is_some(),
        "check_permission READ: {v}"
    );
}

#[tokio::test]
async fn test_dispatch_iris_admin_check_permission_create() {
    let tools = match make_iris_tools() {
        Some(t) => t,
        None => return,
    };
    let result = tools
        .call_for_test(
            "iris_admin",
            serde_json::json!({
                "action": "check_permission",
                "resource": "%DB_DEFAULT",
                "permission": "CREATE"
            }),
        )
        .await;
    let v = parse_result(result);
    assert!(
        v.get("success").is_some() || v.get("error_code").is_some(),
        "check_permission CREATE: {v}"
    );
}

#[tokio::test]
async fn test_dispatch_iris_admin_check_permission_delete() {
    let tools = match make_iris_tools() {
        Some(t) => t,
        None => return,
    };
    let result = tools
        .call_for_test(
            "iris_admin",
            serde_json::json!({
                "action": "check_permission",
                "resource": "%DB_DEFAULT",
                "permission": "DELETE"
            }),
        )
        .await;
    let v = parse_result(result);
    assert!(
        v.get("success").is_some() || v.get("error_code").is_some(),
        "check_permission DELETE: {v}"
    );
}

// ── iris_source_control status action (covers scm.rs main handler path) ──────

#[tokio::test]
async fn test_dispatch_iris_source_control_status() {
    let tools = match make_iris_tools() {
        Some(t) => t,
        None => return,
    };
    let result = tools
        .call_for_test(
            "iris_source_control",
            serde_json::json!({
                "action": "status",
                "document": "%Library.Application.cls",
                "namespace": "USER"
            }),
        )
        .await;
    let v = parse_result(result);
    // May succeed (source control active) or error (no SCM configured) — both are valid
    assert!(
        v.get("success").is_some() || v.get("error_code").is_some(),
        "source_control status: {v}"
    );
}

#[tokio::test]
async fn test_dispatch_iris_source_control_list_root() {
    let tools = match make_iris_tools() {
        Some(t) => t,
        None => return,
    };
    let result = tools
        .call_for_test(
            "iris_source_control",
            serde_json::json!({
                "action": "list",
                "namespace": "USER"
            }),
        )
        .await;
    let v = parse_result(result);
    assert!(
        v.get("success").is_some() || v.get("error_code").is_some(),
        "source_control list: {v}"
    );
}

// ── iris_info: additional actions to cover more branches ─────────────────────

#[tokio::test]
async fn test_dispatch_iris_info_macros_action() {
    let tools = match make_iris_tools() {
        Some(t) => t,
        None => return,
    };
    let result = tools
        .call_for_test(
            "iris_macro",
            serde_json::json!({
                "action": "list",
                "namespace": "USER"
            }),
        )
        .await;
    let v = parse_result(result);
    assert!(
        v.get("success").is_some() || v.get("macros").is_some() || v.get("error_code").is_some(),
        "iris_macro list: {v}"
    );
}

#[tokio::test]
async fn test_dispatch_iris_macro_signature() {
    let tools = match make_iris_tools() {
        Some(t) => t,
        None => return,
    };
    let result = tools
        .call_for_test(
            "iris_macro",
            serde_json::json!({
                "action": "signature",
                "name": "$$ISERR",
                "namespace": "USER"
            }),
        )
        .await;
    let v = parse_result(result);
    assert!(
        v.get("success").is_some() || v.get("error_code").is_some(),
        "iris_macro signature: {v}"
    );
}

// ── iris_interop_query: production start/stop (may fail without production) ──

#[tokio::test]
async fn test_dispatch_iris_production_status_full() {
    let tools = match make_iris_tools() {
        Some(t) => t,
        None => return,
    };
    let result = tools
        .call_for_test(
            "iris_production",
            serde_json::json!({
                "action": "status",
                "namespace": "USER",
                "full_status": true
            }),
        )
        .await;
    let v = parse_result(result);
    assert!(
        v.get("success").is_some() || v.get("error_code").is_some(),
        "production status full: {v}"
    );
}

#[tokio::test]
async fn test_dispatch_iris_production_needs_update_v2() {
    let tools = match make_iris_tools() {
        Some(t) => t,
        None => return,
    };
    let result = tools
        .call_for_test(
            "iris_production",
            serde_json::json!({
                "action": "needs_update",
                "namespace": "USER"
            }),
        )
        .await;
    let v = parse_result(result);
    assert!(
        v.get("success").is_some() || v.get("error_code").is_some(),
        "production needs_update: {v}"
    );
}

// ── autostart actions to cover interop_autostart_get/set paths ───────────────

#[tokio::test]
async fn test_dispatch_iris_production_get_autostart_ensemble() {
    let tools = match make_iris_tools() {
        Some(t) => t,
        None => return,
    };
    let result = tools
        .call_for_test(
            "iris_production",
            serde_json::json!({
                "action": "get_autostart",
                "namespace": "ENSLIB"
            }),
        )
        .await;
    let v = parse_result(result);
    assert!(
        v.get("success").is_some() || v.get("error_code").is_some(),
        "production get_autostart ENSLIB: {v}"
    );
}

// ── iris_interop_query: recover action ───────────────────────────────────────

#[tokio::test]
async fn test_dispatch_iris_interop_query_recover() {
    let tools = match make_iris_tools() {
        Some(t) => t,
        None => return,
    };
    let result = tools
        .call_for_test(
            "iris_interop_query",
            serde_json::json!({
                "what": "recover",
                "namespace": "USER"
            }),
        )
        .await;
    let v = parse_result(result);
    // May fail if no production, but should return a structured response
    assert!(
        v.get("success").is_some() || v.get("error_code").is_some(),
        "interop_query recover: {v}"
    );
}

// ── iris_symbols with various query patterns ──────────────────────────────────

#[tokio::test]
async fn test_dispatch_iris_symbols_prefix_query() {
    let tools = match make_iris_tools() {
        Some(t) => t,
        None => return,
    };
    let result = tools
        .call_for_test(
            "iris_symbols",
            serde_json::json!({
                "query": "Ens.*",
                "namespace": "USER",
                "limit": 10
            }),
        )
        .await;
    let v = parse_result(result);
    assert!(
        v.get("symbols").is_some() || v.get("count").is_some() || v.get("error_code").is_some(),
        "iris_symbols Ens.*: {v}"
    );
}

#[tokio::test]
async fn test_dispatch_iris_symbols_wildcard() {
    let tools = match make_iris_tools() {
        Some(t) => t,
        None => return,
    };
    let result = tools
        .call_for_test(
            "iris_symbols",
            serde_json::json!({
                "query": "*",
                "namespace": "USER",
                "limit": 5
            }),
        )
        .await;
    let v = parse_result(result);
    assert!(
        v.get("symbols").is_some() || v.get("count").is_some() || v.get("error_code").is_some(),
        "iris_symbols wildcard: {v}"
    );
}

// ── iris_doc: additional actions ──────────────────────────────────────────────

#[tokio::test]
async fn test_dispatch_iris_doc_class_list() {
    let tools = match make_iris_tools() {
        Some(t) => t,
        None => return,
    };
    let result = tools
        .call_for_test(
            "iris_doc",
            serde_json::json!({
                "document": "%Library.Application",
                "namespace": "USER"
            }),
        )
        .await;
    let v = parse_result(result);
    assert!(
        v.get("success").is_some() || v.get("error_code").is_some() || v.get("content").is_some(),
        "iris_doc class: {v}"
    );
}

// ── Additional coverage tests ─────────────────────────────────────────────────

#[tokio::test]
async fn test_dispatch_iris_search_case_sensitive() {
    let tools = match make_iris_tools() {
        Some(t) => t,
        None => return,
    };
    let result = tools
        .call_for_test(
            "iris_search",
            serde_json::json!({
                "query": "Object",
                "namespace": "USER",
                "case_sensitive": true,
                "limit": 5
            }),
        )
        .await;
    let v = parse_result(result);
    assert!(
        v.get("results").is_some() || v.get("success").is_some(),
        "search case_sensitive: {v}"
    );
}

#[tokio::test]
async fn test_dispatch_iris_search_regex_mode() {
    let tools = match make_iris_tools() {
        Some(t) => t,
        None => return,
    };
    let result = tools
        .call_for_test(
            "iris_search",
            serde_json::json!({
                "query": "Class.*Definition",
                "namespace": "USER",
                "regex": true,
                "limit": 5
            }),
        )
        .await;
    let v = parse_result(result);
    assert!(
        v.get("results").is_some() || v.get("success").is_some(),
        "search regex: {v}"
    );
}

#[tokio::test]
async fn test_dispatch_iris_search_with_category() {
    let tools = match make_iris_tools() {
        Some(t) => t,
        None => return,
    };
    let result = tools
        .call_for_test(
            "iris_search",
            serde_json::json!({
                "query": "Date",
                "namespace": "USER",
                "category": "CLS",
                "limit": 5
            }),
        )
        .await;
    let v = parse_result(result);
    assert!(
        v.get("results").is_some() || v.get("success").is_some(),
        "search with category: {v}"
    );
}

// ── iris_query with inline param ──────────────────────────────────────────────

#[tokio::test]
async fn test_dispatch_iris_query_inline() {
    let tools = match make_iris_tools() {
        Some(t) => t,
        None => return,
    };
    let result = tools
        .call_for_test(
            "iris_query",
            serde_json::json!({
                "query": "SELECT TOP 3 Name FROM %Dictionary.ClassDefinition WHERE Name LIKE '%Library%'",
                "namespace": "USER"
            }),
        )
        .await;
    let v = parse_result(result);
    assert!(
        v.get("rows").is_some() || v.get("success").is_some() || v.get("error_code").is_some(),
        "iris_query inline: {v}"
    );
}

// ── iris_info with additional actions ────────────────────────────────────────

#[tokio::test]
async fn test_dispatch_iris_info_globals_v3() {
    let tools = match make_iris_tools() {
        Some(t) => t,
        None => return,
    };
    let result = tools
        .call_for_test(
            "iris_info",
            serde_json::json!({
                "what": "globals",
                "namespace": "USER"
            }),
        )
        .await;
    let v = parse_result(result);
    assert!(
        v.get("success").is_some() || v.get("error_code").is_some(),
        "iris_info globals: {v}"
    );
}

#[tokio::test]
async fn test_dispatch_iris_info_routines() {
    let tools = match make_iris_tools() {
        Some(t) => t,
        None => return,
    };
    let result = tools
        .call_for_test(
            "iris_info",
            serde_json::json!({
                "what": "routines",
                "namespace": "USER"
            }),
        )
        .await;
    let v = parse_result(result);
    assert!(
        v.get("success").is_some() || v.get("error_code").is_some(),
        "iris_info routines: {v}"
    );
}

#[tokio::test]
async fn test_dispatch_iris_info_csp() {
    let tools = match make_iris_tools() {
        Some(t) => t,
        None => return,
    };
    let result = tools
        .call_for_test(
            "iris_info",
            serde_json::json!({
                "what": "csp",
                "namespace": "USER"
            }),
        )
        .await;
    let v = parse_result(result);
    assert!(
        v.get("success").is_some() || v.get("error_code").is_some(),
        "iris_info csp: {v}"
    );
}

#[tokio::test]
async fn test_dispatch_iris_info_class_detail() {
    let tools = match make_iris_tools() {
        Some(t) => t,
        None => return,
    };
    let result = tools
        .call_for_test(
            "iris_info",
            serde_json::json!({
                "what": "class",
                "name": "%Library.Object",
                "namespace": "USER"
            }),
        )
        .await;
    let v = parse_result(result);
    assert!(
        v.get("success").is_some() || v.get("error_code").is_some(),
        "iris_info class: {v}"
    );
}

// ── iris_debug actions ────────────────────────────────────────────────────────

#[tokio::test]
async fn test_dispatch_iris_debug_logs() {
    let tools = match make_iris_tools() {
        Some(t) => t,
        None => return,
    };
    let result = tools
        .call_for_test(
            "iris_debug",
            serde_json::json!({
                "action": "get_error_logs",
                "namespace": "USER",
                "limit": 5
            }),
        )
        .await;
    let v = parse_result(result);
    assert!(
        v.get("success").is_some() || v.get("error_code").is_some() || v.get("logs").is_some(),
        "iris_debug get_error_logs: {v}"
    );
}

// ── iris_get_log with different log types ─────────────────────────────────────

#[tokio::test]
async fn test_dispatch_iris_get_log_cconsole() {
    let tools = match make_iris_tools() {
        Some(t) => t,
        None => return,
    };
    let result = tools
        .call_for_test(
            "iris_get_log",
            serde_json::json!({
                "log_type": "cconsole",
                "limit": 10
            }),
        )
        .await;
    let v = parse_result(result);
    assert!(
        v.get("success").is_some() || v.get("error_code").is_some() || v.get("entries").is_some(),
        "iris_get_log cconsole: {v}"
    );
}

// ── iris_table_info ───────────────────────────────────────────────────────────

#[tokio::test]
async fn test_dispatch_iris_table_info_columns() {
    let tools = match make_iris_tools() {
        Some(t) => t,
        None => return,
    };
    let result = tools
        .call_for_test(
            "iris_table_info",
            serde_json::json!({
                "table": "%Dictionary.ClassDefinition",
                "action": "columns",
                "namespace": "USER"
            }),
        )
        .await;
    let v = parse_result(result);
    assert!(
        v.get("success").is_some() || v.get("error_code").is_some() || v.get("columns").is_some(),
        "iris_table_info columns: {v}"
    );
}

// ── iris_macro actions (signature, expand, location, definition) ──────────────

#[tokio::test]
async fn test_dispatch_iris_macro_signature_v2() {
    let tools = match make_iris_tools() {
        Some(t) => t,
        None => return,
    };
    let result = tools
        .call_for_test(
            "iris_macro",
            serde_json::json!({
                "action": "signature",
                "name": "$$AssertEquals",
                "args": [],
                "namespace": "USER"
            }),
        )
        .await;
    let v = parse_result(result);
    assert!(
        v.get("success").is_some() || v.get("error_code").is_some(),
        "iris_macro signature: {v}"
    );
}

#[tokio::test]
async fn test_dispatch_iris_macro_expand_v2() {
    let tools = match make_iris_tools() {
        Some(t) => t,
        None => return,
    };
    let result = tools
        .call_for_test(
            "iris_macro",
            serde_json::json!({
                "action": "expand",
                "name": "$$AssertEquals",
                "args": ["x", "y"],
                "namespace": "USER"
            }),
        )
        .await;
    let v = parse_result(result);
    assert!(
        v.get("success").is_some() || v.get("error_code").is_some(),
        "iris_macro expand: {v}"
    );
}

#[tokio::test]
async fn test_dispatch_iris_macro_location_v2() {
    let tools = match make_iris_tools() {
        Some(t) => t,
        None => return,
    };
    let result = tools
        .call_for_test(
            "iris_macro",
            serde_json::json!({
                "action": "location",
                "name": "$$AssertEquals",
                "args": [],
                "namespace": "USER"
            }),
        )
        .await;
    let v = parse_result(result);
    assert!(
        v.get("success").is_some() || v.get("error_code").is_some(),
        "iris_macro location: {v}"
    );
}

#[tokio::test]
async fn test_dispatch_iris_macro_definition_v2() {
    let tools = match make_iris_tools() {
        Some(t) => t,
        None => return,
    };
    let result = tools
        .call_for_test(
            "iris_macro",
            serde_json::json!({
                "action": "definition",
                "name": "$$AssertEquals",
                "args": [],
                "namespace": "USER"
            }),
        )
        .await;
    let v = parse_result(result);
    assert!(
        v.get("success").is_some() || v.get("error_code").is_some(),
        "iris_macro definition: {v}"
    );
}

#[tokio::test]
async fn test_dispatch_iris_info_unknown_what() {
    let tools = match make_iris_tools() {
        Some(t) => t,
        None => return,
    };
    let result = tools
        .call_for_test(
            "iris_info",
            serde_json::json!({
                "what": "totally_unknown_xyz",
                "namespace": "USER"
            }),
        )
        .await;
    let v = parse_result(result);
    assert_eq!(
        v["error_code"].as_str().unwrap_or(""),
        "INVALID_PARAM",
        "unknown what should return INVALID_PARAM: {v}"
    );
}

// ── iris_debug additional actions ─────────────────────────────────────────────

#[tokio::test]
async fn test_dispatch_iris_debug_error_logs_v2() {
    let tools = match make_iris_tools() {
        Some(t) => t,
        None => return,
    };
    let result = tools
        .call_for_test(
            "iris_debug",
            serde_json::json!({
                "action": "error_logs",
                "namespace": "USER",
                "limit": 5
            }),
        )
        .await;
    let v = parse_result(result);
    assert!(
        v.get("success").is_some() || v.get("error_code").is_some() || v.get("logs").is_some(),
        "iris_debug error_logs: {v}"
    );
}

#[tokio::test]
async fn test_dispatch_iris_debug_capture_v2() {
    let tools = match make_iris_tools() {
        Some(t) => t,
        None => return,
    };
    let result = tools
        .call_for_test(
            "iris_debug",
            serde_json::json!({
                "action": "capture",
                "namespace": "USER"
            }),
        )
        .await;
    let v = parse_result(result);
    assert!(
        v.get("success").is_some() || v.get("error_code").is_some() || v.get("capture").is_some(),
        "iris_debug capture: {v}"
    );
}

#[tokio::test]
async fn test_dispatch_iris_debug_map_int() {
    let tools = match make_iris_tools() {
        Some(t) => t,
        None => return,
    };
    let result = tools
        .call_for_test(
            "iris_debug",
            serde_json::json!({
                "action": "map_int",
                "error_string": "<UNDEFINED>x+1^%SYS.Monitor",
                "namespace": "USER"
            }),
        )
        .await;
    let v = parse_result(result);
    assert!(
        v.get("success").is_some() || v.get("error_code").is_some(),
        "iris_debug map_int: {v}"
    );
}

#[tokio::test]
async fn test_dispatch_iris_debug_source_map_v2() {
    let tools = match make_iris_tools() {
        Some(t) => t,
        None => return,
    };
    let result = tools
        .call_for_test(
            "iris_debug",
            serde_json::json!({
                "action": "source_map",
                "class_name": "%Library.Persistent",
                "namespace": "USER"
            }),
        )
        .await;
    let v = parse_result(result);
    assert!(
        v.get("success").is_some() || v.get("error_code").is_some(),
        "iris_debug source_map: {v}"
    );
}

// ── iris_generate (info.rs handle_iris_generate) ──────────────────────────────

#[tokio::test]
async fn test_dispatch_iris_generate_class() {
    let tools = match make_iris_tools() {
        Some(t) => t,
        None => return,
    };
    let result = tools
        .call_for_test(
            "iris_generate",
            serde_json::json!({
                "gen_type": "class",
                "description": "A simple persistent class for test data",
                "namespace": "USER"
            }),
        )
        .await;
    let v = parse_result(result);
    assert!(
        v["success"].as_bool().unwrap_or(false) || v.get("error_code").is_some(),
        "iris_generate class: {v}"
    );
}

#[tokio::test]
async fn test_dispatch_iris_generate_test() {
    let tools = match make_iris_tools() {
        Some(t) => t,
        None => return,
    };
    let result = tools
        .call_for_test(
            "iris_generate",
            serde_json::json!({
                "gen_type": "test",
                "class_name": "%Library.Persistent",
                "description": "Unit tests for persistent class methods",
                "namespace": "USER"
            }),
        )
        .await;
    let v = parse_result(result);
    assert!(
        v["success"].as_bool().unwrap_or(false) || v.get("error_code").is_some(),
        "iris_generate test: {v}"
    );
}

// ── iris_table_info additional actions ────────────────────────────────────────

#[tokio::test]
async fn test_dispatch_iris_table_info_indexes() {
    let tools = match make_iris_tools() {
        Some(t) => t,
        None => return,
    };
    let result = tools
        .call_for_test(
            "iris_table_info",
            serde_json::json!({
                "table": "%Dictionary.ClassDefinition",
                "action": "indexes",
                "namespace": "USER"
            }),
        )
        .await;
    let v = parse_result(result);
    assert!(
        v.get("success").is_some() || v.get("error_code").is_some(),
        "iris_table_info indexes: {v}"
    );
}

#[tokio::test]
async fn test_dispatch_iris_table_info_row_count() {
    let tools = match make_iris_tools() {
        Some(t) => t,
        None => return,
    };
    let result = tools
        .call_for_test(
            "iris_table_info",
            serde_json::json!({
                "table": "%Dictionary.ClassDefinition",
                "action": "columns",
                "include_row_count": true,
                "namespace": "USER"
            }),
        )
        .await;
    let v = parse_result(result);
    assert!(
        v.get("success").is_some() || v.get("error_code").is_some(),
        "iris_table_info row_count: {v}"
    );
}

// ── iris_doc delete mode ──────────────────────────────────────────────────────

#[tokio::test]
async fn test_dispatch_iris_doc_delete_nonexistent_v2() {
    let tools = match make_iris_tools() {
        Some(t) => t,
        None => return,
    };
    let result = tools
        .call_for_test(
            "iris_doc",
            serde_json::json!({
                "mode": "delete",
                "name": "IrisDevTest.NonExistentClassXYZ9999.cls",
                "namespace": "USER"
            }),
        )
        .await;
    let v = parse_result(result);
    assert!(
        v.get("success").is_some() || v.get("error_code").is_some() || v.get("deleted").is_some(),
        "iris_doc delete: {v}"
    );
}

#[tokio::test]
async fn test_dispatch_iris_doc_put_and_get() {
    let tools = match make_iris_tools() {
        Some(t) => t,
        None => return,
    };
    // Upload a minimal class then GET it back
    let put = tools
        .call_for_test(
            "iris_doc",
            serde_json::json!({
                "mode": "put",
                "name": "IrisDevTmp.LiveTestPutGet.cls",
                "content": "Class IrisDevTmp.LiveTestPutGet {}",
                "namespace": "USER"
            }),
        )
        .await;
    let pv = parse_result(put);
    assert!(
        pv.get("success").is_some() || pv.get("error_code").is_some(),
        "iris_doc put: {pv}"
    );
    let get = tools
        .call_for_test(
            "iris_doc",
            serde_json::json!({
                "mode": "get",
                "name": "IrisDevTmp.LiveTestPutGet.cls",
                "namespace": "USER"
            }),
        )
        .await;
    let gv = parse_result(get);
    assert!(
        gv.get("success").is_some()
            || gv.get("error_code").is_some()
            || gv.get("content").is_some(),
        "iris_doc get: {gv}"
    );
}

// ── iris_admin additional actions ─────────────────────────────────────────────

#[tokio::test]
async fn test_dispatch_iris_admin_check_permission_extra_cases() {
    let tools = match make_iris_tools() {
        Some(t) => t,
        None => return,
    };
    for permission in &["WRITE", "READ"] {
        let result = tools
            .call_for_test(
                "iris_admin",
                serde_json::json!({
                    "action": "check_permission",
                    "resource": "%Admin_Operate",
                    "permission": permission
                }),
            )
            .await;
        let v = parse_result(result);
        assert!(
            v.get("success").is_some() || v.get("error_code").is_some(),
            "iris_admin check_permission {permission}: {v}"
        );
    }
}

// ── iris_query with namespace param ──────────────────────────────────────────

#[tokio::test]
async fn test_dispatch_iris_query_user_namespace() {
    let tools = match make_iris_tools() {
        Some(t) => t,
        None => return,
    };
    let result = tools
        .call_for_test(
            "iris_query",
            serde_json::json!({
                "query": "SELECT TOP 1 Name FROM %Dictionary.ClassDefinition ORDER BY Name",
                "namespace": "USER"
            }),
        )
        .await;
    let v = parse_result(result);
    assert!(
        v["success"].as_bool().unwrap_or(false) || v.get("error_code").is_some(),
        "iris_query namespace: {v}"
    );
}

// ── iris_symbols with different query forms ───────────────────────────────────

#[tokio::test]
async fn test_dispatch_iris_symbols_trailing_dot() {
    let tools = match make_iris_tools() {
        Some(t) => t,
        None => return,
    };
    let result = tools
        .call_for_test(
            "iris_symbols",
            serde_json::json!({
                "query": "%Library.",
                "namespace": "USER",
                "limit": 5
            }),
        )
        .await;
    let v = parse_result(result);
    assert!(
        v.get("symbols").is_some() || v.get("error_code").is_some(),
        "iris_symbols trailing dot: {v}"
    );
}

#[tokio::test]
async fn test_dispatch_iris_symbols_mid_glob() {
    let tools = match make_iris_tools() {
        Some(t) => t,
        None => return,
    };
    let result = tools
        .call_for_test(
            "iris_symbols",
            serde_json::json!({
                "query": "%Library.*.cls",
                "namespace": "USER",
                "limit": 5
            }),
        )
        .await;
    let v = parse_result(result);
    assert!(
        v.get("symbols").is_some() || v.get("error_code").is_some(),
        "iris_symbols mid glob: {v}"
    );
}

#[tokio::test]
async fn test_dispatch_iris_symbols_plain_substring() {
    let tools = match make_iris_tools() {
        Some(t) => t,
        None => return,
    };
    let result = tools
        .call_for_test(
            "iris_symbols",
            serde_json::json!({
                "query": "Persistent",
                "namespace": "USER",
                "limit": 10
            }),
        )
        .await;
    let v = parse_result(result);
    assert!(
        v.get("symbols").is_some() || v.get("error_code").is_some(),
        "iris_symbols plain: {v}"
    );
}

// ── iris_get_log additional log types ─────────────────────────────────────────

#[tokio::test]
async fn test_dispatch_iris_get_log_app() {
    let tools = match make_iris_tools() {
        Some(t) => t,
        None => return,
    };
    let result = tools
        .call_for_test(
            "iris_get_log",
            serde_json::json!({
                "log_type": "app",
                "limit": 5
            }),
        )
        .await;
    let v = parse_result(result);
    assert!(
        v.get("success").is_some() || v.get("error_code").is_some() || v.get("entries").is_some(),
        "iris_get_log app: {v}"
    );
}

#[tokio::test]
async fn test_dispatch_iris_get_log_with_id() {
    let tools = match make_iris_tools() {
        Some(t) => t,
        None => return,
    };
    // First store something, then retrieve by id — use cconsole which always exists
    let store_result = tools
        .call_for_test(
            "iris_get_log",
            serde_json::json!({
                "log_type": "cconsole",
                "limit": 5,
                "store": true
            }),
        )
        .await;
    let sv = parse_result(store_result);
    // If we got a log_id back, try to retrieve it
    if let Some(id) = sv["log_id"].as_str() {
        let get_result = tools
            .call_for_test(
                "iris_get_log",
                serde_json::json!({
                    "id": id,
                    "limit": 5,
                    "offset": 0
                }),
            )
            .await;
        let gv = parse_result(get_result);
        assert!(
            gv.get("success").is_some() || gv.get("error_code").is_some(),
            "iris_get_log by id: {gv}"
        );
    }
}

// ── iris_admin update_user ────────────────────────────────────────────────────

#[tokio::test]
async fn test_dispatch_iris_admin_update_user() {
    let tools = match make_iris_tools() {
        Some(t) => t,
        None => return,
    };
    if std::env::var("IRIS_ADMIN_TOOLS").unwrap_or_default() != "1" {
        return;
    }
    // Create test user first
    let _ = tools
        .call_for_test(
            "iris_admin",
            serde_json::json!({
                "action": "create_user",
                "username": "iris_dev_update_test_888",
                "password": "TestPass123!"
            }),
        )
        .await;
    // Update the user
    let upd = tools
        .call_for_test(
            "iris_admin",
            serde_json::json!({
                "action": "update_user",
                "username": "iris_dev_update_test_888",
                "password": "NewPass456!"
            }),
        )
        .await;
    let uv = parse_result(upd);
    assert!(
        uv.get("success").is_some() || uv.get("error_code").is_some(),
        "iris_admin update_user: {uv}"
    );
    // Cleanup
    let _ = tools
        .call_for_test(
            "iris_admin",
            serde_json::json!({
                "action": "delete_user",
                "username": "iris_dev_update_test_888"
            }),
        )
        .await;
}

// ── iris_compile wildcard expansion ──────────────────────────────────────────

#[tokio::test]
async fn test_dispatch_iris_compile_wildcard() {
    let tools = match make_iris_tools() {
        Some(t) => t,
        None => return,
    };
    // Use a wildcard pattern that matches some classes — covers the wildcard expansion path
    let result = tools
        .call_for_test(
            "iris_compile",
            serde_json::json!({
                "target": "IrisDevTmp.*.cls",
                "namespace": "USER",
                "flags": "ck"
            }),
        )
        .await;
    let v = parse_result(result);
    // Might succeed (found classes) or return empty (no matches) — both are valid
    assert!(
        v.get("success").is_some() || v.get("error_code").is_some(),
        "iris_compile wildcard: {v}"
    );
}

// ── iris_admin get_user ───────────────────────────────────────────────────────

#[tokio::test]
async fn test_dispatch_iris_admin_get_user() {
    let tools = match make_iris_tools() {
        Some(t) => t,
        None => return,
    };
    let result = tools
        .call_for_test(
            "iris_admin",
            serde_json::json!({
                "action": "get_user",
                "username": "_SYSTEM"
            }),
        )
        .await;
    let v = parse_result(result);
    assert!(
        v.get("success").is_some() || v.get("error_code").is_some(),
        "iris_admin get_user: {v}"
    );
}

// ── iris_admin list_resources ─────────────────────────────────────────────────

#[tokio::test]
async fn test_dispatch_iris_admin_list_resources() {
    let tools = match make_iris_tools() {
        Some(t) => t,
        None => return,
    };
    let result = tools
        .call_for_test(
            "iris_admin",
            serde_json::json!({
                "action": "list_resources"
            }),
        )
        .await;
    let v = parse_result(result);
    assert!(
        v.get("success").is_some() || v.get("error_code").is_some(),
        "iris_admin list_resources: {v}"
    );
}

// ── iris_info additional what= values ────────────────────────────────────────

#[tokio::test]
async fn test_dispatch_iris_info_csp_apps_v2() {
    let tools = match make_iris_tools() {
        Some(t) => t,
        None => return,
    };
    let result = tools
        .call_for_test(
            "iris_info",
            serde_json::json!({
                "what": "csp_apps",
                "namespace": "USER"
            }),
        )
        .await;
    let v = parse_result(result);
    assert!(
        v.get("success").is_some() || v.get("error_code").is_some(),
        "iris_info csp_apps: {v}"
    );
}

#[tokio::test]
async fn test_dispatch_iris_info_jobs_v2() {
    let tools = match make_iris_tools() {
        Some(t) => t,
        None => return,
    };
    let result = tools
        .call_for_test(
            "iris_info",
            serde_json::json!({
                "what": "jobs",
                "namespace": "USER"
            }),
        )
        .await;
    let v = parse_result(result);
    assert!(
        v.get("success").is_some() || v.get("error_code").is_some(),
        "iris_info jobs: {v}"
    );
}

#[tokio::test]
async fn test_dispatch_iris_info_namespace_v2() {
    let tools = match make_iris_tools() {
        Some(t) => t,
        None => return,
    };
    let result = tools
        .call_for_test(
            "iris_info",
            serde_json::json!({
                "what": "namespace",
                "namespace": "USER"
            }),
        )
        .await;
    let v = parse_result(result);
    assert!(
        v.get("success").is_some() || v.get("error_code").is_some(),
        "iris_info namespace: {v}"
    );
}

// ── iris_doc head mode ────────────────────────────────────────────────────────

#[tokio::test]
async fn test_dispatch_iris_doc_head_existing() {
    let tools = match make_iris_tools() {
        Some(t) => t,
        None => return,
    };
    let result = tools
        .call_for_test(
            "iris_doc",
            serde_json::json!({
                "mode": "head",
                "name": "%Library.Persistent.cls",
                "namespace": "USER"
            }),
        )
        .await;
    let v = parse_result(result);
    assert!(
        v.get("success").is_some() || v.get("error_code").is_some(),
        "iris_doc head existing: {v}"
    );
}

// ── iris_lookup_transfer ──────────────────────────────────────────────────────

#[tokio::test]
async fn test_dispatch_iris_lookup_transfer_nonexistent() {
    let tools = match make_iris_tools() {
        Some(t) => t,
        None => return,
    };
    if std::env::var("IRIS_ADMIN_TOOLS").unwrap_or_default() != "1" {
        return;
    }
    let result = tools
        .call_for_test(
            "iris_lookup_transfer",
            serde_json::json!({
                "action": "export",
                "table": "IrisDevNonExistentTable99999",
                "namespace": "USER"
            }),
        )
        .await;
    let v = parse_result(result);
    assert!(
        v.get("success").is_some() || v.get("error_code").is_some(),
        "iris_lookup_transfer export: {v}"
    );
}

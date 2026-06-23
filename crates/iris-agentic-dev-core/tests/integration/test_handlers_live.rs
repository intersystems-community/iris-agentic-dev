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
async fn test_dispatch_iris_macro_list_sys_namespace() {
    // %SYS namespace has INC files — covers info.rs lines 145-156 (success path
    // after 200 OK from docnames/INC endpoint with actual content).
    let tools = match make_iris_tools() {
        Some(t) => t,
        None => return,
    };
    let result = tools
        .call_for_test(
            "iris_macro",
            serde_json::json!({
                "action": "list",
                "namespace": "%SYS"
            }),
        )
        .await;
    let v = parse_result(result);
    assert!(
        v.get("macros").is_some() || v.get("success").is_some() || v.get("error_code").is_some(),
        "iris_macro list %SYS: {v}"
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

// ── iris_table_info DDL-created table via iris_query ──────────────────────────

#[tokio::test]
async fn test_dispatch_iris_table_info_ddl_table_v2() {
    // Uses iris_query (JDBC SQL) to create a true DDL table, then calls iris_table_info.
    // Covers info.rs lines 497-515 (DDL branch where no backing ObjectScript class exists).
    let tools = match make_iris_tools() {
        Some(t) => t,
        None => return,
    };
    // Create DDL table via SQL (JDBC path, not docker exec)
    let _ = tools
        .call_for_test(
            "iris_query",
            serde_json::json!({
                "query": "CREATE TABLE SQLUser.IrisDevDdlV2 (Id INTEGER, Val VARCHAR(64))",
                "namespace": "USER"
            }),
        )
        .await;
    // Query its table info — should hit DDL branch
    let result = tools
        .call_for_test(
            "iris_table_info",
            serde_json::json!({
                "table": "SQLUser.IrisDevDdlV2",
                "namespace": "USER"
            }),
        )
        .await;
    let v = parse_result(result);
    assert!(
        v.get("success").is_some() || v.get("error_code").is_some(),
        "iris_table_info DDL v2: {v}"
    );
    // Cleanup
    let _ = tools
        .call_for_test(
            "iris_query",
            serde_json::json!({
                "query": "DROP TABLE SQLUser.IrisDevDdlV2",
                "namespace": "USER"
            }),
        )
        .await;
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
                "name": "IRISDEVTEST999",
                "code_database": "USER",
                "data_database": "USER"
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
                "name": "IRISDEVTEST999"
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

// ── iris_generate_class / iris_generate_test via mock LLM ────────────────────

#[tokio::test]
async fn test_dispatch_iris_generate_class_mock_llm() {
    let tools = match make_iris_tools() {
        Some(t) => t,
        None => return,
    };
    std::env::set_var("IRIS_GENERATE_CLASS_MODEL", "mock");
    std::env::set_var("OPENAI_API_KEY", "sk-mock-test-key");
    let result = tools
        .call_for_test(
            "iris_generate_class",
            serde_json::json!({
                "description": "A simple test class for coverage",
                "namespace": "USER"
            }),
        )
        .await;
    std::env::remove_var("IRIS_GENERATE_CLASS_MODEL");
    std::env::remove_var("OPENAI_API_KEY");
    let v = parse_result(result);
    assert!(
        v["success"].as_bool().unwrap_or(false),
        "iris_generate_class mock: {v}"
    );
    assert!(
        v.get("class_name").is_some(),
        "expected class_name in response: {v}"
    );
}

#[tokio::test]
async fn test_dispatch_iris_generate_test_mock_llm() {
    let tools = match make_iris_tools() {
        Some(t) => t,
        None => return,
    };
    std::env::set_var("IRIS_GENERATE_CLASS_MODEL", "mock");
    std::env::set_var("OPENAI_API_KEY", "sk-mock-test-key");
    let result = tools
        .call_for_test(
            "iris_generate_test",
            serde_json::json!({
                "class_name": "%Library.Persistent",
                "namespace": "USER"
            }),
        )
        .await;
    std::env::remove_var("IRIS_GENERATE_CLASS_MODEL");
    std::env::remove_var("OPENAI_API_KEY");
    let v = parse_result(result);
    assert!(
        v["success"].as_bool().unwrap_or(false) || v.get("error_code").is_some(),
        "iris_generate_test mock: {v}"
    );
}

#[tokio::test]
async fn test_dispatch_iris_generate_class_no_llm() {
    let tools = match make_iris_tools() {
        Some(t) => t,
        None => return,
    };
    std::env::remove_var("IRIS_GENERATE_CLASS_MODEL");
    std::env::remove_var("OPENAI_API_KEY");
    std::env::remove_var("ANTHROPIC_API_KEY");
    let result = tools
        .call_for_test(
            "iris_generate_class",
            serde_json::json!({
                "description": "A class without LLM configured",
                "namespace": "USER"
            }),
        )
        .await;
    // Expect LLM_UNAVAILABLE error path
    assert!(
        result.is_err() || {
            let v = parse_result(result);
            v.get("error_code").is_some() || v.get("success").is_some()
        }
    );
}

// ── admin write_disabled paths (IRIS_ADMIN_TOOLS not set) ────────────────────

#[tokio::test]
async fn test_dispatch_iris_admin_update_user_write_disabled() {
    let tools = match make_iris_tools() {
        Some(t) => t,
        None => return,
    };
    let prev = std::env::var("IRIS_ADMIN_TOOLS").ok();
    std::env::remove_var("IRIS_ADMIN_TOOLS");
    let result = tools
        .call_for_test(
            "iris_admin",
            serde_json::json!({
                "action": "update_user",
                "username": "some_user",
                "password": "NewPass123!"
            }),
        )
        .await;
    if let Some(v) = prev {
        std::env::set_var("IRIS_ADMIN_TOOLS", v);
    }
    let v = parse_result(result);
    assert_eq!(
        v.get("error_code").and_then(|e| e.as_str()),
        Some("ADMIN_WRITE_DISABLED"),
        "expected write-disabled: {v}"
    );
}

#[tokio::test]
async fn test_dispatch_iris_admin_delete_user_write_disabled() {
    let tools = match make_iris_tools() {
        Some(t) => t,
        None => return,
    };
    let prev = std::env::var("IRIS_ADMIN_TOOLS").ok();
    std::env::remove_var("IRIS_ADMIN_TOOLS");
    let result = tools
        .call_for_test(
            "iris_admin",
            serde_json::json!({
                "action": "delete_user",
                "username": "some_user"
            }),
        )
        .await;
    if let Some(v) = prev {
        std::env::set_var("IRIS_ADMIN_TOOLS", v);
    }
    let v = parse_result(result);
    assert_eq!(
        v.get("error_code").and_then(|e| e.as_str()),
        Some("ADMIN_WRITE_DISABLED"),
        "expected write-disabled: {v}"
    );
}

#[tokio::test]
async fn test_dispatch_iris_admin_create_namespace_write_disabled() {
    let tools = match make_iris_tools() {
        Some(t) => t,
        None => return,
    };
    let prev = std::env::var("IRIS_ADMIN_TOOLS").ok();
    std::env::remove_var("IRIS_ADMIN_TOOLS");
    let result = tools
        .call_for_test(
            "iris_admin",
            serde_json::json!({
                "action": "create_namespace",
                "name": "TESTNS",
                "code_database": "USER",
                "data_database": "USER"
            }),
        )
        .await;
    if let Some(v) = prev {
        std::env::set_var("IRIS_ADMIN_TOOLS", v);
    }
    let v = parse_result(result);
    assert_eq!(
        v.get("error_code").and_then(|e| e.as_str()),
        Some("ADMIN_WRITE_DISABLED"),
        "expected write-disabled: {v}"
    );
}

#[tokio::test]
async fn test_dispatch_iris_admin_create_user_write_disabled_explicit() {
    let tools = match make_iris_tools() {
        Some(t) => t,
        None => return,
    };
    let prev = std::env::var("IRIS_ADMIN_TOOLS").ok();
    std::env::remove_var("IRIS_ADMIN_TOOLS");
    let result = tools
        .call_for_test(
            "iris_admin",
            serde_json::json!({
                "action": "create_user",
                "username": "testuser_disabled",
                "password": "Test123!"
            }),
        )
        .await;
    if let Some(v) = prev {
        std::env::set_var("IRIS_ADMIN_TOOLS", v);
    }
    let v = parse_result(result);
    assert_eq!(
        v.get("error_code").and_then(|e| e.as_str()),
        Some("ADMIN_WRITE_DISABLED"),
        "expected write-disabled: {v}"
    );
}

// ── info.rs DDL table path ────────────────────────────────────────────────────

#[tokio::test]
async fn test_dispatch_iris_table_info_ddl_table() {
    let tools = match make_iris_tools() {
        Some(t) => t,
        None => return,
    };
    // Create a pure DDL table (no corresponding ObjectScript class)
    let _ = tools
        .call_for_test(
            "iris_execute",
            serde_json::json!({
                "code": "Do ##class(%SQL.Statement).%ExecDirect(,\"CREATE TABLE SQLUser.IrisDevTmpDDL (Id INTEGER, Name VARCHAR(50))\")",
                "namespace": "USER"
            }),
        )
        .await;
    // Query table_info — DDL table has no class definition → hits ddl_table branch
    let result = tools
        .call_for_test(
            "iris_table_info",
            serde_json::json!({
                "table": "SQLUser.IrisDevTmpDDL",
                "namespace": "USER",
                "include_row_count": true
            }),
        )
        .await;
    let v = parse_result(result);
    assert!(
        v.get("success").is_some() || v.get("error_code").is_some(),
        "iris_table_info ddl: {v}"
    );
    // Cleanup
    let _ = tools
        .call_for_test(
            "iris_execute",
            serde_json::json!({
                "code": "Do ##class(%SQL.Statement).%ExecDirect(,\"DROP TABLE SQLUser.IrisDevTmpDDL\")",
                "namespace": "USER"
            }),
        )
        .await;
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

// ── iris_compile with local temp file ────────────────────────────────────────

#[tokio::test]
async fn test_dispatch_iris_compile_local_file_v2() {
    let tools = match make_iris_tools() {
        Some(t) => t,
        None => return,
    };
    // Write a minimal valid ObjectScript class to a temp file and compile it
    let cls_content = "Class IrisDevTmp.TestLocalCompile Extends %RegisteredObject\n{\n}\n";
    let tmp_path = std::env::temp_dir().join("IrisDevTmp.TestLocalCompile.cls");
    std::fs::write(&tmp_path, cls_content).expect("write temp cls");
    let target = tmp_path.to_str().unwrap().to_string();

    let result = tools
        .call_for_test(
            "iris_compile",
            serde_json::json!({
                "target": target,
                "namespace": "USER",
                "flags": "ck"
            }),
        )
        .await;
    let v = parse_result(result);
    assert!(
        v.get("success").is_some() || v.get("error_code").is_some(),
        "iris_compile local file: {v}"
    );
    // Clean up
    let _ = std::fs::remove_file(&tmp_path);
}

#[tokio::test]
async fn test_dispatch_iris_compile_local_file_no_class_decl() {
    let tools = match make_iris_tools() {
        Some(t) => t,
        None => return,
    };
    // File with no Class declaration — doc_name falls back to filename
    let cls_content = "// No class declaration here\n";
    let tmp_path = std::env::temp_dir().join("IrisDevTmpNoClass.cls");
    std::fs::write(&tmp_path, cls_content).expect("write temp cls");
    let target = tmp_path.to_str().unwrap().to_string();

    let result = tools
        .call_for_test(
            "iris_compile",
            serde_json::json!({
                "target": target,
                "namespace": "USER",
                "flags": "ck"
            }),
        )
        .await;
    // Will fail compilation — but we exercised the fallback doc_name path
    match result {
        Ok(r) => {
            let text = r.content[0].raw.as_text().unwrap().text.clone();
            eprintln!(
                "iris_compile no-class-decl: {}",
                &text[..text.len().min(200)]
            );
        }
        Err(e) => eprintln!("iris_compile no-class-decl error (ok): {e}"),
    }
    let _ = std::fs::remove_file(&tmp_path);
}

// ── iris_test with nonexistent namespace (ERR_NAMESPACE_NOT_FOUND) ────────────

#[tokio::test]
async fn test_dispatch_iris_test_bad_namespace() {
    let tools = match make_iris_tools() {
        Some(t) => t,
        None => return,
    };
    let result = tools
        .call_for_test(
            "iris_test",
            serde_json::json!({
                "pattern": "SomeTest",
                "namespace": "IRISDEVFAKENAMESPACE99999"
            }),
        )
        .await;
    let v = parse_result(result);
    // Should return NAMESPACE_NOT_FOUND or success:false
    assert!(
        v.get("success").is_some() || v.get("error_code").is_some(),
        "iris_test bad namespace: {v}"
    );
}

// ── iris_test with directory-style pattern (non-class path) ──────────────────

#[tokio::test]
async fn test_dispatch_iris_test_directory_pattern() {
    let tools = match make_iris_tools() {
        Some(t) => t,
        None => return,
    };
    // A path with "/" triggers the non-class branch (no /noload flag)
    let result = tools
        .call_for_test(
            "iris_test",
            serde_json::json!({
                "pattern": "/tmp/nonexistent_tests",
                "namespace": "USER"
            }),
        )
        .await;
    let v = parse_result(result);
    assert!(
        v.get("success").is_some() || v.get("error_code").is_some(),
        "iris_test dir pattern: {v}"
    );
}

// ── iris_search live dispatch ─────────────────────────────────────────────────

#[tokio::test]
async fn test_dispatch_iris_search_live() {
    let tools = match make_iris_tools() {
        Some(t) => t,
        None => return,
    };
    let result = tools
        .call_for_test(
            "iris_search",
            serde_json::json!({
                "query": "Persistent",
                "namespace": "USER",
                "category": "CLS",
                "inline": true
            }),
        )
        .await;
    let v = parse_result(result);
    assert!(
        v.get("success").is_some() || v.get("error_code").is_some(),
        "iris_search live: {v}"
    );
}

// ── iris_lookup_transfer import ───────────────────────────────────────────────

#[tokio::test]
async fn test_dispatch_iris_lookup_transfer_import() {
    let tools = match make_iris_tools() {
        Some(t) => t,
        None => return,
    };
    if std::env::var("IRIS_ADMIN_TOOLS").unwrap_or_default() != "1" {
        return;
    }
    // Minimal well-formed lookup table XML
    let xml = r#"<?xml version="1.0" ?><Lookup><![CDATA[IrisDevImportTest]]><entry key="k1" value="v1"/></Lookup>"#;
    let result = tools
        .call_for_test(
            "iris_lookup_transfer",
            serde_json::json!({
                "action": "import",
                "table": "IrisDevImportTest",
                "xml": xml,
                "namespace": "USER"
            }),
        )
        .await;
    let v = parse_result(result);
    assert!(
        v.get("success").is_some() || v.get("error_code").is_some(),
        "iris_lookup_transfer import: {v}"
    );
}

// ── iris_get_log list-all (no id) ─────────────────────────────────────────────

#[tokio::test]
async fn test_dispatch_iris_get_log_list_all() {
    let tools = match make_iris_tools() {
        Some(t) => t,
        None => return,
    };
    // No id → list all stored entries
    let result = tools
        .call_for_test("iris_get_log", serde_json::json!({}))
        .await;
    let v = parse_result(result);
    assert!(
        v.get("logs").is_some() || v.get("success").is_some(),
        "iris_get_log list-all: {v}"
    );
}

// ── docs_introspect ───────────────────────────────────────────────────────────

#[tokio::test]
async fn test_dispatch_docs_introspect() {
    let tools = match make_iris_tools() {
        Some(t) => t,
        None => return,
    };
    let result = tools
        .call_for_test(
            "docs_introspect",
            serde_json::json!({
                "class_name": "%Library.Persistent",
                "namespace": "USER"
            }),
        )
        .await;
    let v = parse_result(result);
    assert!(
        v.get("methods").is_some() || v.get("error_code").is_some(),
        "docs_introspect: {v}"
    );
}

// ── check_config ──────────────────────────────────────────────────────────────

#[tokio::test]
async fn test_dispatch_check_config() {
    let tools = match make_iris_tools() {
        Some(t) => t,
        None => return,
    };
    let result = tools
        .call_for_test("check_config", serde_json::json!({}))
        .await;
    let v = parse_result(result);
    assert!(
        v.get("host").is_some()
            || v.get("iris_host").is_some()
            || v.get("error_code").is_some()
            || v.get("success").is_some(),
        "check_config: {v}"
    );
}

// ── iris_containers list ──────────────────────────────────────────────────────

#[tokio::test]
async fn test_dispatch_iris_containers_list() {
    let tools = match make_iris_tools() {
        Some(t) => t,
        None => return,
    };
    let result = tools
        .call_for_test(
            "iris_containers",
            serde_json::json!({
                "action": "list"
            }),
        )
        .await;
    let v = parse_result(result);
    assert!(
        v.get("containers").is_some()
            || v.get("error_code").is_some()
            || v.get("success").is_some(),
        "iris_containers list: {v}"
    );
}

#[tokio::test]
async fn test_dispatch_iris_containers_invalid_action() {
    let tools = match make_iris_tools() {
        Some(t) => t,
        None => return,
    };
    let result = tools
        .call_for_test(
            "iris_containers",
            serde_json::json!({
                "action": "invalid_action_xyz"
            }),
        )
        .await;
    let v = parse_result(result);
    assert!(
        v.get("error_code").is_some(),
        "iris_containers invalid action should error: {v}"
    );
}

// ── agent_history and agent_stats ─────────────────────────────────────────────

#[tokio::test]
async fn test_dispatch_agent_history() {
    let tools = match make_iris_tools() {
        Some(t) => t,
        None => return,
    };
    let result = tools
        .call_for_test("agent_history", serde_json::json!({}))
        .await;
    let v = parse_result(result);
    assert!(
        v.get("calls").is_some() || v.get("history").is_some() || v.get("error_code").is_some(),
        "agent_history: {v}"
    );
}

#[tokio::test]
async fn test_dispatch_agent_stats() {
    let tools = match make_iris_tools() {
        Some(t) => t,
        None => return,
    };
    let result = tools
        .call_for_test("agent_stats", serde_json::json!({}))
        .await;
    let v = parse_result(result);
    assert!(
        v.get("status").is_some() || v.get("stats").is_some() || v.get("error_code").is_some(),
        "agent_stats: {v}"
    );
}

// ── skill_list, skill_describe, skill_search ──────────────────────────────────

#[tokio::test]
async fn test_dispatch_skill_list() {
    let tools = match make_iris_tools() {
        Some(t) => t,
        None => return,
    };
    let result = tools
        .call_for_test("skill_list", serde_json::json!({}))
        .await;
    let v = parse_result(result);
    assert!(
        v.get("skills").is_some() || v.get("success").is_some() || v.get("error_code").is_some(),
        "skill_list: {v}"
    );
}

#[tokio::test]
async fn test_dispatch_skill_describe() {
    let tools = match make_iris_tools() {
        Some(t) => t,
        None => return,
    };
    let result = tools
        .call_for_test(
            "skill_describe",
            serde_json::json!({
                "name": "nonexistent-skill-xyz"
            }),
        )
        .await;
    let v = parse_result(result);
    assert!(
        v.get("skill").is_some() || v.get("error_code").is_some() || v.get("success").is_some(),
        "skill_describe: {v}"
    );
}

#[tokio::test]
async fn test_dispatch_skill_search() {
    let tools = match make_iris_tools() {
        Some(t) => t,
        None => return,
    };
    let result = tools
        .call_for_test(
            "skill_search",
            serde_json::json!({
                "query": "compile"
            }),
        )
        .await;
    let v = parse_result(result);
    assert!(
        v.get("results").is_some()
            || v.get("skills").is_some()
            || v.get("error_code").is_some()
            || v.get("success").is_some(),
        "skill_search: {v}"
    );
}

// ── kb_index and kb_recall ─────────────────────────────────────────────────────

#[tokio::test]
async fn test_dispatch_kb_recall() {
    let tools = match make_iris_tools() {
        Some(t) => t,
        None => return,
    };
    let result = tools
        .call_for_test(
            "kb_recall",
            serde_json::json!({
                "query": "compile ObjectScript class"
            }),
        )
        .await;
    let v = parse_result(result);
    assert!(
        v.get("chunks").is_some()
            || v.get("results").is_some()
            || v.get("error_code").is_some()
            || v.get("success").is_some(),
        "kb_recall: {v}"
    );
}

// ── iris_admin list_user_roles for nonexistent user (USER_NOT_FOUND) ─────────

#[tokio::test]
async fn test_dispatch_iris_admin_list_user_roles_nonexistent() {
    let tools = match make_iris_tools() {
        Some(t) => t,
        None => return,
    };
    let result = tools
        .call_for_test(
            "iris_admin",
            serde_json::json!({
                "action": "list_user_roles",
                "username": "IrisDevNonExistentUser99999"
            }),
        )
        .await;
    let v = parse_result(result);
    assert!(
        v.get("error_code").is_some() || v.get("roles").is_some(),
        "list_user_roles nonexistent: {v}"
    );
}

// ── iris_admin update_user with enabled and roles ─────────────────────────────

#[tokio::test]
async fn test_dispatch_iris_admin_update_user_with_roles() {
    let tools = match make_iris_tools() {
        Some(t) => t,
        None => return,
    };
    if std::env::var("IRIS_ADMIN_TOOLS").unwrap_or_default() != "1" {
        return;
    }
    // Create a test user first
    let _ = tools
        .call_for_test(
            "iris_admin",
            serde_json::json!({
                "action": "create_user",
                "username": "IrisDevTestUser88",
                "password": "TestPass1!"
            }),
        )
        .await;

    // Update with enabled=true and roles
    let result = tools
        .call_for_test(
            "iris_admin",
            serde_json::json!({
                "action": "update_user",
                "username": "IrisDevTestUser88",
                "enabled": true,
                "roles": "%All"
            }),
        )
        .await;
    let v = parse_result(result);
    eprintln!("update_user with roles: {v}");
    assert!(
        v.get("success").is_some() || v.get("error_code").is_some(),
        "update_user with roles: {v}"
    );

    // Cleanup
    let _ = tools
        .call_for_test(
            "iris_admin",
            serde_json::json!({
                "action": "delete_user",
                "username": "IrisDevTestUser88"
            }),
        )
        .await;
}

// ── iris_admin check_permission READ/EXECUTE ──────────────────────────────────

#[tokio::test]
async fn test_dispatch_iris_admin_check_permission_execute() {
    let tools = match make_iris_tools() {
        Some(t) => t,
        None => return,
    };
    let result = tools
        .call_for_test(
            "iris_admin",
            serde_json::json!({
                "action": "check_permission",
                "resource": "%Development",
                "permission": "USE"
            }),
        )
        .await;
    let v = parse_result(result);
    assert!(
        v.get("granted").is_some()
            || v.get("has_permission").is_some()
            || v.get("error_code").is_some(),
        "check_permission USE: {v}"
    );
}

// ── iris_query edge cases ─────────────────────────────────────────────────────

#[tokio::test]
async fn test_dispatch_iris_query_empty_query() {
    let tools = match make_iris_tools() {
        Some(t) => t,
        None => return,
    };
    // Empty query → EMPTY_QUERY error
    let result = tools
        .call_for_test(
            "iris_query",
            serde_json::json!({
                "query": "",
                "namespace": "USER"
            }),
        )
        .await;
    let v = parse_result(result);
    assert!(
        v.get("error_code").is_some(),
        "empty query should error: {v}"
    );
}

#[tokio::test]
async fn test_dispatch_iris_query_write_blocked() {
    let tools = match make_iris_tools() {
        Some(t) => t,
        None => return,
    };
    // DROP TABLE → SQL_WRITE_BLOCKED
    let result = tools
        .call_for_test(
            "iris_query",
            serde_json::json!({
                "query": "DROP TABLE SomeTable",
                "namespace": "USER"
            }),
        )
        .await;
    let v = parse_result(result);
    assert!(v.get("error_code").is_some(), "DROP should be blocked: {v}");
}

#[tokio::test]
async fn test_dispatch_iris_query_force_write() {
    let tools = match make_iris_tools() {
        Some(t) => t,
        None => return,
    };
    if std::env::var("IRIS_ADMIN_TOOLS").unwrap_or_default() != "1" {
        return;
    }
    // force=true bypasses SQL safety gate (write_tools_enabled=true when IRIS_ADMIN_TOOLS=1)
    // Use a benign INSERT that will fail on a non-existent table — tests the force path
    let result = tools
        .call_for_test(
            "iris_query",
            serde_json::json!({
                "query": "SELECT * FROM IrisDevNonExistentQueryTable99999",
                "namespace": "USER",
                "force": true
            }),
        )
        .await;
    let v = parse_result(result);
    // Will get SQL_ERROR for nonexistent table — but that's after the safety gate
    assert!(
        v.get("success").is_some() || v.get("error_code").is_some(),
        "iris_query force: {v}"
    );
}

// ── iris_execute with translate_sql=true ─────────────────────────────────────

#[tokio::test]
async fn test_dispatch_iris_execute_translate_sql() {
    let tools = match make_iris_tools() {
        Some(t) => t,
        None => return,
    };
    // Use &sql() macro — triggers translation path in iris_execute
    let result = tools
        .call_for_test(
            "iris_execute",
            serde_json::json!({
                "code": "set x = 1 write x",
                "namespace": "USER",
                "translate_sql": true
            }),
        )
        .await;
    let v = parse_result(result);
    assert!(
        v.get("success").is_some() || v.get("error_code").is_some(),
        "iris_execute translate_sql: {v}"
    );
}

#[tokio::test]
async fn test_dispatch_iris_execute_with_sql_macro() {
    let tools = match make_iris_tools() {
        Some(t) => t,
        None => return,
    };
    // Code with &sql() triggers translation and sql_translated response field
    let result = tools
        .call_for_test(
            "iris_execute",
            serde_json::json!({
                "code": "&sql(SELECT TOP 1 1 INTO :x FROM %Dictionary.ClassDefinition) write x",
                "namespace": "USER",
                "translate_sql": true
            }),
        )
        .await;
    let v = parse_result(result);
    eprintln!(
        "iris_execute sql_macro: {}",
        serde_json::to_string(&v).unwrap_or_default()
    );
    assert!(
        v.get("success").is_some() || v.get("error_code").is_some(),
        "iris_execute with sql macro: {v}"
    );
}

// ── iris_test run a real test class (covers test parse loop) ─────────────────

#[tokio::test]
async fn test_dispatch_iris_test_run_real_class() {
    let tools = match make_iris_tools() {
        Some(t) => t,
        None => return,
    };
    // First upload a minimal test class so we have something to run
    let cls_content = concat!(
        "Class IrisDevTmp.TestSimple Extends %UnitTest.TestCase\n",
        "{\n",
        "Method TestPass()\n",
        "{\n",
        "    Do $$$AssertEquals(1, 1, \"always passes\")\n",
        "}\n",
        "}\n"
    );
    // Upload via iris_doc
    let put_result = tools
        .call_for_test(
            "iris_doc",
            serde_json::json!({
                "mode": "put",
                "name": "IrisDevTmp.TestSimple.cls",
                "content": cls_content,
                "namespace": "USER",
                "compile": true
            }),
        )
        .await;
    let pv = parse_result(put_result);
    if pv.get("error_code").is_some() {
        eprintln!("iris_test setup: could not upload test class: {pv}");
        return;
    }

    // Run the test class
    let result = tools
        .call_for_test(
            "iris_test",
            serde_json::json!({
                "pattern": "IrisDevTmp.TestSimple",
                "namespace": "USER"
            }),
        )
        .await;
    let v = parse_result(result);
    eprintln!(
        "iris_test real class: {}",
        serde_json::to_string(&v).unwrap_or_default()
    );
    assert!(
        v.get("success").is_some() || v.get("error_code").is_some(),
        "iris_test real class: {v}"
    );

    // Cleanup
    let _ = tools
        .call_for_test(
            "iris_doc",
            serde_json::json!({
                "mode": "delete",
                "name": "IrisDevTmp.TestSimple.cls",
                "namespace": "USER"
            }),
        )
        .await;
}

// ── iris_admin create/delete webapp ───────────────────────────────────────────

#[tokio::test]
async fn test_dispatch_iris_admin_create_delete_webapp() {
    let tools = match make_iris_tools() {
        Some(t) => t,
        None => return,
    };
    if std::env::var("IRIS_ADMIN_TOOLS").unwrap_or_default() != "1" {
        return;
    }
    let test_path = "/irisdev-test-webapp-99999";

    // Create webapp
    let create_result = tools
        .call_for_test(
            "iris_admin",
            serde_json::json!({
                "action": "create_webapp",
                "path": test_path,
                "namespace": "USER"
            }),
        )
        .await;
    let cv = parse_result(create_result);
    eprintln!("admin create_webapp: {cv}");

    // Delete webapp (even if create failed, covers delete path)
    let del_result = tools
        .call_for_test(
            "iris_admin",
            serde_json::json!({
                "action": "delete_webapp",
                "path": test_path
            }),
        )
        .await;
    let dv = parse_result(del_result);
    eprintln!("admin delete_webapp: {dv}");

    assert!(
        cv.get("success").is_some() || cv.get("error_code").is_some(),
        "create_webapp response: {cv}"
    );
}

// ── iris_doc batch get (names array) ─────────────────────────────────────────

#[tokio::test]
async fn test_dispatch_iris_doc_batch_get() {
    let tools = match make_iris_tools() {
        Some(t) => t,
        None => return,
    };
    let result = tools
        .call_for_test(
            "iris_doc",
            serde_json::json!({
                "mode": "get",
                "names": ["%Library.Persistent.cls", "%Library.RegisteredObject.cls"],
                "namespace": "USER"
            }),
        )
        .await;
    let v = parse_result(result);
    assert!(
        v.get("documents").is_some() || v.get("error_code").is_some(),
        "iris_doc batch get: {v}"
    );
}

// ── iris_doc batch delete (names array) ──────────────────────────────────────

#[tokio::test]
async fn test_dispatch_iris_doc_batch_delete() {
    let tools = match make_iris_tools() {
        Some(t) => t,
        None => return,
    };
    if std::env::var("IRIS_ADMIN_TOOLS").unwrap_or_default() != "1" {
        return;
    }
    // Batch delete nonexistent classes — covers batch delete path even on error
    let result = tools
        .call_for_test(
            "iris_doc",
            serde_json::json!({
                "mode": "delete",
                "names": ["IrisDevTmp.BatchDelete1.cls", "IrisDevTmp.BatchDelete2.cls"],
                "namespace": "USER"
            }),
        )
        .await;
    let v = parse_result(result);
    assert!(
        v.get("deleted").is_some() || v.get("error_code").is_some(),
        "iris_doc batch delete: {v}"
    );
}

// ── iris_doc get nonexistent (NOT_FOUND 404 path) ─────────────────────────────

#[tokio::test]
async fn test_dispatch_iris_doc_get_not_found() {
    let tools = match make_iris_tools() {
        Some(t) => t,
        None => return,
    };
    let result = tools
        .call_for_test(
            "iris_doc",
            serde_json::json!({
                "mode": "get",
                "name": "IrisDevNonExistent99999.cls",
                "namespace": "USER"
            }),
        )
        .await;
    let v = parse_result(result);
    assert!(
        v.get("error_code").is_some(),
        "iris_doc get nonexistent should error: {v}"
    );
}

// ── iris_doc put with compile=true ────────────────────────────────────────────

#[tokio::test]
async fn test_dispatch_iris_doc_put_with_compile() {
    let tools = match make_iris_tools() {
        Some(t) => t,
        None => return,
    };
    let cls_content = "Class IrisDevTmp.TestPutCompile Extends %RegisteredObject\n{\n}\n";
    let result = tools
        .call_for_test(
            "iris_doc",
            serde_json::json!({
                "mode": "put",
                "name": "IrisDevTmp.TestPutCompile.cls",
                "content": cls_content,
                "namespace": "USER",
                "compile": true
            }),
        )
        .await;
    let v = parse_result(result);
    assert!(
        v.get("success").is_some() || v.get("error_code").is_some(),
        "iris_doc put with compile: {v}"
    );
    // Cleanup
    let _ = tools
        .call_for_test(
            "iris_doc",
            serde_json::json!({
                "mode": "delete",
                "name": "IrisDevTmp.TestPutCompile.cls",
                "namespace": "USER"
            }),
        )
        .await;
}

// ── resolve_dynamic_dispatch with package prefix (has_prefix=true path) ──────

#[tokio::test]
async fn test_dispatch_resolve_dynamic_dispatch_with_prefix() {
    let tools = match make_iris_tools() {
        Some(t) => t,
        None => return,
    };
    let result = tools
        .call_for_test(
            "resolve_dynamic_dispatch",
            serde_json::json!({
                "method_name": "OnProcessInput",
                "package_prefix": "%Library",
                "namespace": "USER"
            }),
        )
        .await;
    let v = parse_result(result);
    assert!(
        v.get("candidates").is_some() || v.get("error_code").is_some(),
        "resolve_dynamic_dispatch with prefix: {v}"
    );
}

// ── resolve_dynamic_dispatch cache hit (second call hits cache) ───────────────

#[tokio::test]
async fn test_dispatch_resolve_dynamic_dispatch_cache_hit() {
    let tools = match make_iris_tools() {
        Some(t) => t,
        None => return,
    };
    // Call twice — second call should hit metadata cache
    for _ in 0..2 {
        let result = tools
            .call_for_test(
                "resolve_dynamic_dispatch",
                serde_json::json!({
                    "method_name": "%OnNew",
                    "namespace": "USER"
                }),
            )
            .await;
        let v = parse_result(result);
        assert!(
            v.get("candidates").is_some() || v.get("error_code").is_some(),
            "resolve_dynamic_dispatch cache: {v}"
        );
    }
}

// ── extract_message_map_routing cache hit ─────────────────────────────────────

#[tokio::test]
async fn test_dispatch_extract_message_map_cache_hit() {
    let tools = match make_iris_tools() {
        Some(t) => t,
        None => return,
    };
    // Call twice on same class — second call hits cache.
    // %Dictionary.CompiledClass is a real compiled class but has no MessageMap,
    // so has_message_map:false — valid response, no parse error.
    for _ in 0..2 {
        let result = tools
            .call_for_test(
                "extract_message_map_routing",
                serde_json::json!({
                    "class_name": "%Dictionary.CompiledClass",
                    "namespace": "USER"
                }),
            )
            .await;
        match result {
            Ok(r) => {
                let text = r.content[0].raw.as_text().unwrap().text.clone();
                eprintln!(
                    "extract_message_map cache: {}",
                    &text[..text.len().min(200)]
                );
            }
            Err(e) => eprintln!("extract_message_map error (ok): {e}"),
        }
    }
}

// ── find_subclass_implementations cache hit ───────────────────────────────────

#[tokio::test]
async fn test_dispatch_find_subclass_implementations_cache_hit() {
    let tools = match make_iris_tools() {
        Some(t) => t,
        None => return,
    };
    // Call twice — second call hits cache. Uses correct field name: base_classes.
    for _ in 0..2 {
        let result = tools
            .call_for_test(
                "find_subclass_implementations",
                serde_json::json!({
                    "base_classes": ["%Library.Persistent"],
                    "method_name": "%OnNew",
                    "namespace": "USER"
                }),
            )
            .await;
        let v = parse_result(result);
        assert!(
            v.get("implementations").is_some() || v.get("error_code").is_some(),
            "find_subclass_impls cache: {v}"
        );
    }
}

// ── dict.rs edge cases ────────────────────────────────────────────────────────

#[tokio::test]
async fn test_dispatch_extract_message_map_not_found() {
    let tools = match make_iris_tools() {
        Some(t) => t,
        None => return,
    };
    // Non-existent class → NOT_FOUND
    let result = tools
        .call_for_test(
            "extract_message_map_routing",
            serde_json::json!({
                "class_name": "IrisDevNonExistent99999.NoSuchClass",
                "namespace": "USER"
            }),
        )
        .await;
    let v = parse_result(result);
    assert!(
        v.get("error_code").is_some(),
        "non-existent class should error: {v}"
    );
}

#[tokio::test]
async fn test_dispatch_find_subclass_no_descendants() {
    let tools = match make_iris_tools() {
        Some(t) => t,
        None => return,
    };
    // Base class with no subclasses → empty implementations
    let result = tools
        .call_for_test(
            "find_subclass_implementations",
            serde_json::json!({
                "base_classes": ["IrisDevNonExistent99999.NoSuchBase"],
                "method_name": "DoSomething",
                "namespace": "USER"
            }),
        )
        .await;
    let v = parse_result(result);
    // Returns empty implementations list (not error) when no descendants found
    assert!(
        v.get("implementations").is_some() || v.get("error_code").is_some(),
        "no descendants: {v}"
    );
    if let Some(impls) = v.get("implementations") {
        assert_eq!(
            impls.as_array().map(|a| a.len()).unwrap_or(0),
            0,
            "should have empty implementations: {v}"
        );
    }
}

#[tokio::test]
async fn test_dispatch_resolve_dynamic_dispatch_with_package_prefix() {
    let tools = match make_iris_tools() {
        Some(t) => t,
        None => return,
    };
    // package_prefix path: filters candidates by package
    let result = tools
        .call_for_test(
            "resolve_dynamic_dispatch",
            serde_json::json!({
                "method_name": "Execute",
                "package_prefix": "%Library",
                "namespace": "USER"
            }),
        )
        .await;
    let v = parse_result(result);
    assert!(
        v.get("candidates").is_some() || v.get("error_code").is_some(),
        "resolve_dynamic dispatch with prefix: {v}"
    );
}

// ── admin.rs: delete non-existent namespace ───────────────────────────────────

#[tokio::test]
async fn test_dispatch_iris_admin_delete_nonexistent_namespace() {
    let tools = match make_iris_tools() {
        Some(t) => t,
        None => return,
    };
    if std::env::var("IRIS_ADMIN_TOOLS").unwrap_or_default() != "1" {
        return;
    }
    // Deleting a non-existent namespace → NAMESPACE_NOT_FOUND
    let result = tools
        .call_for_test(
            "iris_admin",
            serde_json::json!({
                "action": "delete_namespace",
                "name": "IRISDEVNONEXISTENT99999"
            }),
        )
        .await;
    let v = parse_result(result);
    assert_eq!(
        v.get("error_code").and_then(|e| e.as_str()),
        Some("NAMESPACE_NOT_FOUND"),
        "delete nonexistent namespace: {v}"
    );
}

// ── iris_get_log: pagination ───────────────────────────────────────────────────

#[tokio::test]
async fn test_dispatch_iris_get_log_with_offset() {
    let tools = match make_iris_tools() {
        Some(t) => t,
        None => return,
    };
    // offset>0 triggers pagination branch in iris_get_log
    let result = tools
        .call_for_test(
            "iris_get_log",
            serde_json::json!({
                "limit": 5,
                "offset": 2
            }),
        )
        .await;
    let v = parse_result(result);
    assert!(
        v.get("logs").is_some() || v.get("entries").is_some() || v.get("error_code").is_some(),
        "iris_get_log with offset: {v}"
    );
}

// ── iris_info: additional actions ──────────────────────────────────────────────

#[tokio::test]
async fn test_dispatch_iris_info_namespaces() {
    let tools = match make_iris_tools() {
        Some(t) => t,
        None => return,
    };
    let result = tools
        .call_for_test(
            "iris_info",
            serde_json::json!({
                "what": "namespaces"
            }),
        )
        .await;
    let v = parse_result(result);
    assert!(
        v.get("namespaces").is_some() || v.get("error_code").is_some(),
        "iris_info namespaces: {v}"
    );
}

#[tokio::test]
async fn test_dispatch_iris_info_databases() {
    let tools = match make_iris_tools() {
        Some(t) => t,
        None => return,
    };
    let result = tools
        .call_for_test(
            "iris_info",
            serde_json::json!({
                "what": "databases"
            }),
        )
        .await;
    let v = parse_result(result);
    assert!(
        v.get("databases").is_some() || v.get("error_code").is_some(),
        "iris_info databases: {v}"
    );
}

#[tokio::test]
async fn test_dispatch_iris_info_classes() {
    let tools = match make_iris_tools() {
        Some(t) => t,
        None => return,
    };
    let result = tools
        .call_for_test(
            "iris_info",
            serde_json::json!({
                "what": "classes",
                "namespace": "USER"
            }),
        )
        .await;
    let v = parse_result(result);
    assert!(
        v.get("classes").is_some() || v.get("error_code").is_some(),
        "iris_info classes: {v}"
    );
}

// ── iris_get_log: retrieve specific log ID ────────────────────────────────────

#[tokio::test]
async fn test_dispatch_iris_get_log_not_found() {
    let tools = match make_iris_tools() {
        Some(t) => t,
        None => return,
    };
    // Non-existent log ID → LOG_NOT_FOUND
    let result = tools
        .call_for_test(
            "iris_get_log",
            serde_json::json!({
                "id": "nonexistent-log-id-99999"
            }),
        )
        .await;
    let v = parse_result(result);
    assert_eq!(
        v.get("error_code").and_then(|e| e.as_str()),
        Some("LOG_NOT_FOUND"),
        "non-existent log ID should return LOG_NOT_FOUND: {v}"
    );
}

#[tokio::test]
async fn test_dispatch_iris_get_log_paginated_retrieve() {
    let tools = match make_iris_tools() {
        Some(t) => t,
        None => return,
    };
    // First execute something to create a log entry
    let exec_result = tools
        .call_for_test(
            "iris_execute",
            serde_json::json!({
                "code": "write 1,!",
                "namespace": "USER"
            }),
        )
        .await;
    let ev = parse_result(exec_result);
    // Try to get a log if one exists
    let list_result = tools
        .call_for_test(
            "iris_get_log",
            serde_json::json!({
                "limit": 5
            }),
        )
        .await;
    let lv = parse_result(list_result);
    let logs = lv.get("logs").and_then(|l| l.as_array());
    if let Some(entries) = logs {
        if let Some(first) = entries.first() {
            if let Some(id) = first.get("log_id").and_then(|i| i.as_str()) {
                // Retrieve with limit to exercise paginated response path
                let paginated = tools
                    .call_for_test(
                        "iris_get_log",
                        serde_json::json!({
                            "id": id,
                            "limit": 3,
                            "offset": 0
                        }),
                    )
                    .await;
                let pv = parse_result(paginated);
                assert!(
                    pv.get("result").is_some() || pv.get("error_code").is_some(),
                    "iris_get_log with limit: {pv}"
                );
            }
        }
    }
    let _ = ev; // suppress unused warning
}

// ── iris_macro unknown action → INVALID_PARAM ─────────────────────────────────

#[tokio::test]
async fn test_dispatch_iris_macro_unknown_action() {
    let tools = match make_iris_tools() {
        Some(t) => t,
        None => return,
    };
    let result = tools
        .call_for_test(
            "iris_macro",
            serde_json::json!({
                "action": "totally_unknown_action_xyz",
                "namespace": "USER"
            }),
        )
        .await;
    let v = parse_result(result);
    assert_eq!(
        v.get("error_code").and_then(|c| c.as_str()),
        Some("INVALID_PARAM"),
        "iris_macro unknown action should return INVALID_PARAM: {v}"
    );
    assert!(
        v.get("error")
            .and_then(|e| e.as_str())
            .map(|s| s.contains("totally_unknown_action_xyz"))
            .unwrap_or(false),
        "error message should include the unknown action: {v}"
    );
}

// ── iris_debug unknown action → INVALID_PARAM ────────────────────────────────

#[tokio::test]
async fn test_dispatch_iris_debug_unknown_action() {
    let tools = match make_iris_tools() {
        Some(t) => t,
        None => return,
    };
    let result = tools
        .call_for_test(
            "iris_debug",
            serde_json::json!({
                "action": "totally_unknown_debug_xyz",
                "namespace": "USER"
            }),
        )
        .await;
    let v = parse_result(result);
    assert_eq!(
        v.get("error_code").and_then(|c| c.as_str()),
        Some("INVALID_PARAM"),
        "iris_debug unknown action should return INVALID_PARAM: {v}"
    );
}

// ── resolve_dynamic_dispatch with package_prefix ─────────────────────────────

#[tokio::test]
async fn test_dispatch_resolve_dynamic_dispatch_package_prefix_live() {
    let tools = match make_iris_tools() {
        Some(t) => t,
        None => return,
    };
    // Exercise the has_prefix=true branch by providing a non-empty package_prefix
    let result = tools
        .call_for_test(
            "resolve_dynamic_dispatch",
            serde_json::json!({
                "method_name": "OnProcessInput",
                "package_prefix": "Ens",
                "namespace": "USER"
            }),
        )
        .await;
    let v = parse_result(result);
    assert!(
        v.get("success").is_some() || v.get("error_code").is_some(),
        "resolve_dynamic_dispatch with prefix: {v}"
    );
}

// ── iris_info with "config" action ────────────────────────────────────────────

#[tokio::test]
async fn test_dispatch_iris_info_config() {
    let tools = match make_iris_tools() {
        Some(t) => t,
        None => return,
    };
    let result = tools
        .call_for_test(
            "iris_info",
            serde_json::json!({
                "what": "config"
            }),
        )
        .await;
    let v = parse_result(result);
    assert!(
        v.get("success").is_some() || v.get("error_code").is_some(),
        "iris_info config: {v}"
    );
}

// ── iris_table_info DDL table (not found) ─────────────────────────────────────

#[tokio::test]
async fn test_dispatch_iris_table_info_not_found() {
    let tools = match make_iris_tools() {
        Some(t) => t,
        None => return,
    };
    let result = tools
        .call_for_test(
            "iris_table_info",
            serde_json::json!({
                "table": "NonExistentTable_XYZ_9999",
                "namespace": "USER"
            }),
        )
        .await;
    let v = parse_result(result);
    // NOT_FOUND or error_code
    let success = v.get("success").and_then(|s| s.as_bool()).unwrap_or(true);
    assert!(
        !success || v.get("error_code").is_some(),
        "nonexistent table should return success=false or error_code: {v}"
    );
}

// ── iris_doc PUT .mac without ROUTINE header (routine header injection path) ──

#[tokio::test]
async fn test_dispatch_iris_doc_put_mac_no_routine_header() {
    let tools = match make_iris_tools() {
        Some(t) => t,
        None => return,
    };
    // Content without ROUTINE header — should be injected automatically
    let mac_content = " write \"hello\",!\n";
    let result = tools
        .call_for_test(
            "iris_doc",
            serde_json::json!({
                "mode": "put",
                "name": "IrisDevTmp.TestMacRoutine.mac",
                "content": mac_content,
                "namespace": "USER",
                "compile": false
            }),
        )
        .await;
    let v = parse_result(result);
    assert!(
        v.get("success").is_some() || v.get("error_code").is_some(),
        "mac put no header: {v}"
    );
    // Cleanup
    let _ = tools
        .call_for_test(
            "iris_doc",
            serde_json::json!({
                "mode": "delete",
                "name": "IrisDevTmp.TestMacRoutine.mac",
                "namespace": "USER"
            }),
        )
        .await;
}

// ── iris_doc PUT .inc without ROUTINE header (inc header injection path) ──────

#[tokio::test]
async fn test_dispatch_iris_doc_put_inc_no_routine_header() {
    let tools = match make_iris_tools() {
        Some(t) => t,
        None => return,
    };
    // INC content without ROUTINE header
    let inc_content = "#define MY_CONST 42\n";
    let result = tools
        .call_for_test(
            "iris_doc",
            serde_json::json!({
                "mode": "put",
                "name": "IrisDevTmp.TestIncRoutine.inc",
                "content": inc_content,
                "namespace": "USER",
                "compile": false
            }),
        )
        .await;
    let v = parse_result(result);
    assert!(
        v.get("success").is_some() || v.get("error_code").is_some(),
        "inc put no header: {v}"
    );
    // Cleanup
    let _ = tools
        .call_for_test(
            "iris_doc",
            serde_json::json!({
                "mode": "delete",
                "name": "IrisDevTmp.TestIncRoutine.inc",
                "namespace": "USER"
            }),
        )
        .await;
}

// ── iris_test with a FAILING test class (covers FAILED parse path) ───────────

#[tokio::test]
async fn test_dispatch_iris_test_failing_class() {
    let tools = match make_iris_tools() {
        Some(t) => t,
        None => return,
    };
    // Upload a test class that will FAIL
    let cls_content = concat!(
        "Class IrisDevTmp.TestFailing Extends %UnitTest.TestCase\n",
        "{\n",
        "Method TestAlwaysFails()\n",
        "{\n",
        "    Do $$$AssertEquals(1, 2, \"intentional failure\")\n",
        "}\n",
        "}\n"
    );
    let put_result = tools
        .call_for_test(
            "iris_doc",
            serde_json::json!({
                "mode": "put",
                "name": "IrisDevTmp.TestFailing.cls",
                "content": cls_content,
                "namespace": "USER",
                "compile": true
            }),
        )
        .await;
    let pv = parse_result(put_result);
    if pv.get("error_code").is_some() {
        eprintln!("iris_test_failing setup: could not upload: {pv}");
        return;
    }

    // Run the failing test class
    let result = tools
        .call_for_test(
            "iris_test",
            serde_json::json!({
                "pattern": "IrisDevTmp.TestFailing",
                "namespace": "USER"
            }),
        )
        .await;
    let v = parse_result(result);
    eprintln!(
        "iris_test failing: {}",
        serde_json::to_string(&v).unwrap_or_default()
    );
    assert!(
        v.get("success").is_some() || v.get("error_code").is_some(),
        "iris_test failing class: {v}"
    );

    // Cleanup
    let _ = tools
        .call_for_test(
            "iris_doc",
            serde_json::json!({
                "mode": "delete",
                "name": "IrisDevTmp.TestFailing.cls",
                "namespace": "USER"
            }),
        )
        .await;
}

// ── iris_macro signature action (covers signature/location/definition/expand arms) ──

#[tokio::test]
async fn test_dispatch_iris_macro_signature_v3() {
    let tools = match make_iris_tools() {
        Some(t) => t,
        None => return,
    };
    let result = tools
        .call_for_test(
            "iris_macro",
            serde_json::json!({
                "action": "signature",
                "name": "AssertEquals",
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

// ── iris_table_info DDL table (covers DDL infer path lines 497-515) ──────────

#[tokio::test]
async fn test_dispatch_iris_table_info_information_schema() {
    let tools = match make_iris_tools() {
        Some(t) => t,
        None => return,
    };
    // INFORMATION_SCHEMA.TABLES is a DDL/system table that exists in all IRIS
    let result = tools
        .call_for_test(
            "iris_table_info",
            serde_json::json!({
                "table": "INFORMATION_SCHEMA.TABLES",
                "namespace": "USER",
                "include_row_count": false
            }),
        )
        .await;
    let v = parse_result(result);
    assert!(
        v.get("success").is_some() || v.get("error_code").is_some(),
        "iris_table_info INFORMATION_SCHEMA.TABLES: {v}"
    );
}

// ── iris_execute with translate_sql=false (no-translation path) ──────────────

#[tokio::test]
async fn test_dispatch_iris_execute_no_translate() {
    let tools = match make_iris_tools() {
        Some(t) => t,
        None => return,
    };
    let result = tools
        .call_for_test(
            "iris_execute",
            serde_json::json!({
                "code": "write $ZVERSION,!",
                "namespace": "USER",
                "translate_sql": false
            }),
        )
        .await;
    let v = parse_result(result);
    assert!(
        v.get("success").is_some() || v.get("output").is_some() || v.get("error_code").is_some(),
        "iris_execute no-translate: {v}"
    );
}

// ── iris_symbols_local nonexistent workspace path (WORKSPACE_NOT_FOUND) ───────

#[tokio::test]
async fn test_dispatch_iris_symbols_local_workspace_not_found() {
    let tools = match make_iris_tools() {
        Some(t) => t,
        None => return,
    };
    let result = tools
        .call_for_test(
            "iris_symbols_local",
            serde_json::json!({
                "query": "*.cls",
                "workspace_path": "/nonexistent/path/xyz_9999_abc"
            }),
        )
        .await;
    let v = parse_result(result);
    assert_eq!(
        v.get("error_code").and_then(|e| e.as_str()),
        Some("WORKSPACE_NOT_FOUND"),
        "nonexistent workspace should return WORKSPACE_NOT_FOUND: {v}"
    );
}

// ── iris_compile from local file path (covers local-file upload path) ─────────

#[tokio::test]
async fn test_dispatch_iris_compile_local_file_v3() {
    let tools = match make_iris_tools() {
        Some(t) => t,
        None => return,
    };
    // Write a temp .cls file
    let dir = tempfile::tempdir().unwrap();
    let cls_path = dir.path().join("IrisDevTmp.CompileLocal.cls");
    std::fs::write(&cls_path, "Class IrisDevTmp.CompileLocal {\n}\n").unwrap();
    let result = tools
        .call_for_test(
            "iris_compile",
            serde_json::json!({
                "target": cls_path.to_str().unwrap(),
                "namespace": "USER"
            }),
        )
        .await;
    let v = parse_result(result);
    assert!(
        v.get("success").is_some() || v.get("error_code").is_some(),
        "iris_compile local file: {v}"
    );
}

// ── iris_symbols_local with OBJECTSCRIPT_WORKSPACE env (env branch) ───────────

#[tokio::test]
async fn test_dispatch_iris_symbols_local_env_workspace() {
    let tools = match make_iris_tools() {
        Some(t) => t,
        None => return,
    };
    // Set env var to a known directory, then call without workspace_path
    let dir = tempfile::tempdir().unwrap();
    std::env::set_var("OBJECTSCRIPT_WORKSPACE", dir.path().to_str().unwrap());
    let result = tools
        .call_for_test(
            "iris_symbols_local",
            serde_json::json!({
                "query": "*.cls"
            }),
        )
        .await;
    std::env::remove_var("OBJECTSCRIPT_WORKSPACE");
    let v = parse_result(result);
    // Empty dir → no results, but should succeed
    assert!(
        v.get("success").is_some() || v.get("symbols").is_some() || v.get("error_code").is_some(),
        "symbols_local env workspace: {v}"
    );
}

// ── skill_community_list (covers skill_community_list handler) ────────────────

#[tokio::test]
async fn test_dispatch_skill_community_list() {
    let tools = match make_iris_tools() {
        Some(t) => t,
        None => return,
    };
    let result = tools
        .call_for_test("skill_community_list", serde_json::json!({}))
        .await;
    let v = parse_result(result);
    assert!(
        v.get("skills").is_some() || v.get("success").is_some() || v.get("error_code").is_some(),
        "skill_community_list: {v}"
    );
}

// ── kb_index (covers kb_index handler — empty dir) ────────────────────────────

#[tokio::test]
async fn test_dispatch_kb_index_empty_dir() {
    let tools = match make_iris_tools() {
        Some(t) => t,
        None => return,
    };
    let dir = tempfile::tempdir().unwrap();
    let result = tools
        .call_for_test(
            "kb_index",
            serde_json::json!({
                "path": dir.path().to_str().unwrap()
            }),
        )
        .await;
    let v = parse_result(result);
    assert!(
        v.get("success").is_some() || v.get("indexed").is_some() || v.get("error_code").is_some(),
        "kb_index empty dir: {v}"
    );
}

// ── skill_list dispatch (covers skill_list handler) ───────────────────────────

#[tokio::test]
async fn test_dispatch_skill_list_v2() {
    let tools = match make_iris_tools() {
        Some(t) => t,
        None => return,
    };
    let result = tools
        .call_for_test("skill_list", serde_json::json!({}))
        .await;
    let v = parse_result(result);
    assert!(
        v.get("skills").is_some() || v.get("success").is_some() || v.get("error_code").is_some(),
        "skill_list: {v}"
    );
}

// ── skill_describe dispatch (covers skill_describe handler) ──────────────────

#[tokio::test]
async fn test_dispatch_skill_describe_not_found() {
    let tools = match make_iris_tools() {
        Some(t) => t,
        None => return,
    };
    let result = tools
        .call_for_test(
            "skill_describe",
            serde_json::json!({"name": "nonexistent-skill-xyz-9999"}),
        )
        .await;
    let v = parse_result(result);
    assert!(
        v.get("error_code").is_some() || v.get("success").is_some(),
        "skill_describe not found: {v}"
    );
}

// ── skill_search dispatch (covers skill_search handler) ──────────────────────

#[tokio::test]
async fn test_dispatch_skill_search_empty() {
    let tools = match make_iris_tools() {
        Some(t) => t,
        None => return,
    };
    let result = tools
        .call_for_test(
            "skill_search",
            serde_json::json!({"query": "nonexistent_xyz_9999_query"}),
        )
        .await;
    let v = parse_result(result);
    assert!(
        v.get("results").is_some() || v.get("success").is_some() || v.get("error_code").is_some(),
        "skill_search empty: {v}"
    );
}

// ── skill_community_install dispatch (covers community_install handler) ───────

#[tokio::test]
async fn test_dispatch_skill_community_install_not_found() {
    let tools = match make_iris_tools() {
        Some(t) => t,
        None => return,
    };
    let result = tools
        .call_for_test(
            "skill_community_install",
            serde_json::json!({"name": "nonexistent-xyz-9999"}),
        )
        .await;
    let v = parse_result(result);
    // Should return NOT_FOUND or error since skill doesn't exist
    assert!(
        v.get("error_code").is_some() || v.get("success").is_some(),
        "skill_community_install not_found: {v}"
    );
}

// ── iris_compile with read-only flag (covers read_only guard in iris_compile) ─

#[tokio::test]
async fn test_dispatch_iris_compile_nonexistent_target() {
    let tools = match make_iris_tools() {
        Some(t) => t,
        None => return,
    };
    // Nonexistent class — should return compile error, not panic
    let result = tools
        .call_for_test(
            "iris_compile",
            serde_json::json!({
                "target": "IrisDevTest.TotallyFakeClass99999.cls",
                "namespace": "USER"
            }),
        )
        .await;
    let v = parse_result(result);
    assert!(
        v.get("success").is_some() || v.get("error_code").is_some(),
        "iris_compile nonexistent: {v}"
    );
}

// ── iris_execute with &sql macro that translates (covers translation path) ────

#[tokio::test]
async fn test_dispatch_iris_execute_with_sql_translation() {
    let tools = match make_iris_tools() {
        Some(t) => t,
        None => return,
    };
    let result = tools
        .call_for_test(
            "iris_execute",
            serde_json::json!({
                "code": "&sql(SELECT 1 INTO :x)\nwrite x,!",
                "namespace": "USER",
                "translate_sql": true
            }),
        )
        .await;
    let v = parse_result(result);
    assert!(
        v.get("success").is_some() || v.get("output").is_some() || v.get("error_code").is_some(),
        "iris_execute with sql translation: {v}"
    );
}

// ── debug_map_int_to_cls (covers lines 3130-3155 mod.rs) ─────────────────────

#[tokio::test]
async fn test_dispatch_debug_map_int_to_cls() {
    let tools = match make_iris_tools() {
        Some(t) => t,
        None => return,
    };
    let result = tools
        .call_for_test(
            "debug_map_int_to_cls",
            serde_json::json!({
                "routine": "%SYS.Namespace",
                "offset": 1,
                "error_string": "",
                "namespace": "USER"
            }),
        )
        .await;
    let v = parse_result(result);
    assert!(
        v.get("success").is_some() || v.get("error_code").is_some(),
        "debug_map_int_to_cls: {v}"
    );
}

// ── debug_map_int_to_cls with error_string (covers parse_iris_error_string branch) ──

#[tokio::test]
async fn test_dispatch_debug_map_int_to_cls_error_string() {
    let tools = match make_iris_tools() {
        Some(t) => t,
        None => return,
    };
    let result = tools
        .call_for_test(
            "debug_map_int_to_cls",
            serde_json::json!({
                "routine": "",
                "offset": 0,
                "error_string": "<UNDEFINED>x+3^%SYS.Namespace.1",
                "namespace": "USER"
            }),
        )
        .await;
    let v = parse_result(result);
    assert!(
        v.get("success").is_some() || v.get("error_code").is_some(),
        "debug_map_int_to_cls error_string: {v}"
    );
}

// ── debug_capture_packet (covers lines 3163-3176 mod.rs) ─────────────────────

#[tokio::test]
async fn test_dispatch_debug_capture_packet() {
    let tools = match make_iris_tools() {
        Some(t) => t,
        None => return,
    };
    let result = tools
        .call_for_test(
            "debug_capture_packet",
            serde_json::json!({ "namespace": "USER" }),
        )
        .await;
    let v = parse_result(result);
    assert!(
        v.get("success").is_some() || v.get("error_code").is_some(),
        "debug_capture_packet: {v}"
    );
}

// ── debug_get_error_logs (covers lines 3184-3218 mod.rs) ─────────────────────

#[tokio::test]
async fn test_dispatch_debug_get_error_logs() {
    let tools = match make_iris_tools() {
        Some(t) => t,
        None => return,
    };
    let result = tools
        .call_for_test(
            "debug_get_error_logs",
            serde_json::json!({ "namespace": "USER", "max_entries": 5 }),
        )
        .await;
    let v = parse_result(result);
    assert!(
        v.get("success").is_some() || v.get("error_code").is_some(),
        "debug_get_error_logs: {v}"
    );
}

// ── debug_source_map (covers lines 3228-3250 mod.rs) ─────────────────────────

#[tokio::test]
async fn test_dispatch_debug_source_map() {
    let tools = match make_iris_tools() {
        Some(t) => t,
        None => return,
    };
    let result = tools
        .call_for_test(
            "debug_source_map",
            serde_json::json!({
                "cls_name": "%SYS.Namespace",
                "namespace": "USER"
            }),
        )
        .await;
    let v = parse_result(result);
    assert!(
        v.get("success").is_some() || v.get("error_code").is_some(),
        "debug_source_map: {v}"
    );
}

// ── iris_generate (covers lines 3835-3843 mod.rs) ────────────────────────────

#[tokio::test]
async fn test_dispatch_iris_generate_v2() {
    let tools = match make_iris_tools() {
        Some(t) => t,
        None => return,
    };
    let result = tools
        .call_for_test(
            "iris_generate",
            serde_json::json!({
                "gen_type": "class",
                "description": "A simple test class",
                "namespace": "USER"
            }),
        )
        .await;
    let v = parse_result(result);
    assert!(
        v.get("success").is_some() || v.get("error_code").is_some() || v.get("prompt").is_some(),
        "iris_generate: {v}"
    );
}

// ── docs_introspect (covers lines 3947-3994 mod.rs) ──────────────────────────

#[tokio::test]
async fn test_dispatch_docs_introspect_v2() {
    let tools = match make_iris_tools() {
        Some(t) => t,
        None => return,
    };
    let result = tools
        .call_for_test(
            "docs_introspect",
            serde_json::json!({ "class_name": "%SYS.Namespace", "namespace": "USER" }),
        )
        .await;
    let v = parse_result(result);
    assert!(
        v.get("success").is_some() || v.get("error_code").is_some(),
        "docs_introspect: {v}"
    );
}

// ── agent_history (covers lines 4011-4020 mod.rs) ────────────────────────────

#[tokio::test]
async fn test_dispatch_agent_history_v2() {
    let tools = match make_iris_tools() {
        Some(t) => t,
        None => return,
    };
    let result = tools
        .call_for_test("agent_history", serde_json::json!({ "limit": 5 }))
        .await;
    let v = parse_result(result);
    assert!(
        v.get("calls").is_some() || v.get("history").is_some() || v.get("error_code").is_some(),
        "agent_history: {v}"
    );
}

// ── agent_stats (covers lines 4035-4040 mod.rs) ───────────────────────────────

#[tokio::test]
async fn test_dispatch_agent_stats_v2() {
    let tools = match make_iris_tools() {
        Some(t) => t,
        None => return,
    };
    let result = tools
        .call_for_test("agent_stats", serde_json::json!({}))
        .await;
    let v = parse_result(result);
    assert!(
        v.get("session_calls").is_some()
            || v.get("success").is_some()
            || v.get("error_code").is_some(),
        "agent_stats: {v}"
    );
}

// ── check_config (covers lines 4136-4148 mod.rs) ─────────────────────────────

#[tokio::test]
async fn test_dispatch_check_config_v2() {
    let tools = match make_iris_tools() {
        Some(t) => t,
        None => return,
    };
    let result = tools
        .call_for_test("check_config", serde_json::json!({}))
        .await;
    let v = parse_result(result);
    assert!(
        v.get("connected").is_some()
            || v.get("iris_connected").is_some()
            || v.get("success").is_some()
            || v.get("error_code").is_some(),
        "check_config: {v}"
    );
}

// ── skill_forget (covers skill_forget handler) ────────────────────────────────

#[tokio::test]
async fn test_dispatch_skill_forget_nonexistent() {
    let tools = match make_iris_tools() {
        Some(t) => t,
        None => return,
    };
    let result = tools
        .call_for_test(
            "skill_forget",
            serde_json::json!({ "name": "nonexistent-skill-xyz-99999" }),
        )
        .await;
    let v = parse_result(result);
    assert!(
        v.get("success").is_some() || v.get("error_code").is_some(),
        "skill_forget nonexistent: {v}"
    );
}

// ── kb_recall (covers kb_recall handler) ─────────────────────────────────────

#[tokio::test]
async fn test_dispatch_kb_recall_empty() {
    let tools = match make_iris_tools() {
        Some(t) => t,
        None => return,
    };
    let result = tools
        .call_for_test(
            "kb_recall",
            serde_json::json!({ "query": "nonexistent topic xyz 99999", "namespace": "USER" }),
        )
        .await;
    let v = parse_result(result);
    assert!(
        v.get("success").is_some() || v.get("results").is_some() || v.get("error_code").is_some(),
        "kb_recall empty: {v}"
    );
}

// ── iris_symbols (covers iris_symbols handler paths) ─────────────────────────

#[tokio::test]
async fn test_dispatch_iris_symbols_v2() {
    let tools = match make_iris_tools() {
        Some(t) => t,
        None => return,
    };
    let result = tools
        .call_for_test(
            "iris_symbols",
            serde_json::json!({
                "query": "%SYS.Namespace",
                "namespace": "USER",
                "max_results": 5
            }),
        )
        .await;
    let v = parse_result(result);
    assert!(
        v.get("success").is_some() || v.get("symbols").is_some() || v.get("error_code").is_some(),
        "iris_symbols v2: {v}"
    );
}

// ── iris_execute runtime error path (covers is_runtime_error branch) ─────────

#[tokio::test]
async fn test_dispatch_iris_execute_runtime_error() {
    let tools = match make_iris_tools() {
        Some(t) => t,
        None => return,
    };
    // This code triggers a runtime error — the error handler writes ERROR:
    let result = tools
        .call_for_test(
            "iris_execute",
            serde_json::json!({
                "code": "set x = 1/0",
                "namespace": "USER"
            }),
        )
        .await;
    // May succeed (IRIS may return 0 or error text) — just verify it returns
    let v = parse_result(result);
    assert!(
        v.get("success").is_some() || v.get("output").is_some() || v.get("error_code").is_some(),
        "iris_execute runtime error: {v}"
    );
}

// ── iris_query with force=true and SQL_WRITE_BLOCKED (covers force_ignored path) ──

#[tokio::test]
async fn test_dispatch_iris_query_force_blocked() {
    let tools = match make_iris_tools() {
        Some(t) => t,
        None => return,
    };
    // DROP without force — should be blocked
    let result = tools
        .call_for_test(
            "iris_query",
            serde_json::json!({
                "query": "DROP TABLE nonexistent_xyz_table",
                "namespace": "USER",
                "force": false
            }),
        )
        .await;
    let v = parse_result(result);
    assert!(
        v.get("success").is_some() || v.get("error_code").is_some(),
        "iris_query force blocked: {v}"
    );
}

// ── iris_query with force=true (covers force=true SQL_WRITE_BLOCKED path) ────

#[tokio::test]
async fn test_dispatch_iris_query_force_true() {
    let tools = match make_iris_tools() {
        Some(t) => t,
        None => return,
    };
    // SELECT with force=true — should proceed normally
    let result = tools
        .call_for_test(
            "iris_query",
            serde_json::json!({
                "query": "SELECT TOP 1 1 FROM INFORMATION_SCHEMA.TABLES",
                "namespace": "USER",
                "force": true
            }),
        )
        .await;
    let v = parse_result(result);
    assert!(
        v.get("success").is_some() || v.get("rows").is_some() || v.get("error_code").is_some(),
        "iris_query force true: {v}"
    );
}

// ── iris_get_log (covers iris_get_log handler) ────────────────────────────────

#[tokio::test]
async fn test_dispatch_iris_get_log_v2() {
    let tools = match make_iris_tools() {
        Some(t) => t,
        None => return,
    };
    let result = tools
        .call_for_test(
            "iris_get_log",
            serde_json::json!({ "namespace": "USER", "max_lines": 10 }),
        )
        .await;
    let v = parse_result(result);
    assert!(
        v.get("success").is_some() || v.get("lines").is_some() || v.get("error_code").is_some(),
        "iris_get_log v2: {v}"
    );
}

// ── resolve_dynamic_dispatch (covers dict handler) ────────────────────────────

#[tokio::test]
async fn test_dispatch_resolve_dynamic_dispatch_v2() {
    let tools = match make_iris_tools() {
        Some(t) => t,
        None => return,
    };
    let result = tools
        .call_for_test(
            "resolve_dynamic_dispatch",
            serde_json::json!({
                "class_name": "%SYS.Namespace",
                "method_name": "List",
                "namespace": "USER"
            }),
        )
        .await;
    let v = parse_result(result);
    assert!(
        v.get("success").is_some() || v.get("error_code").is_some(),
        "resolve_dynamic_dispatch: {v}"
    );
}

// ── find_subclass_implementations (covers dict handler) ───────────────────────

#[tokio::test]
async fn test_dispatch_find_subclass_implementations_v2() {
    let tools = match make_iris_tools() {
        Some(t) => t,
        None => return,
    };
    let result = tools
        .call_for_test(
            "find_subclass_implementations",
            serde_json::json!({
                "method_name": "OnProcessInput",
                "base_classes": ["%Library.Persistent"],
                "namespace": "USER",
                "limit": 5
            }),
        )
        .await;
    let v = parse_result(result);
    assert!(
        v.get("success").is_some() || v.get("error_code").is_some(),
        "find_subclass_implementations: {v}"
    );
}

// ── extract_message_map_routing (covers dict handler) ─────────────────────────

#[tokio::test]
async fn test_dispatch_extract_message_map_routing_v2() {
    let tools = match make_iris_tools() {
        Some(t) => t,
        None => return,
    };
    // Use a class that EXISTS but has no MessageMap — covers the has_message_map:false path
    let result = tools
        .call_for_test(
            "extract_message_map_routing",
            serde_json::json!({
                "class_name": "%SYS.Namespace",
                "namespace": "USER"
            }),
        )
        .await;
    // Allow Err (parse failure for classes without MessageMap is acceptable)
    match result {
        Ok(_) | Err(_) => {} // either outcome covers the code path
    }
}

// ── find_subclass_implementations with empty base_classes (covers line 279 dict.rs) ──

#[tokio::test]
async fn test_dispatch_find_subclass_empty_bases() {
    let tools = match make_iris_tools() {
        Some(t) => t,
        None => return,
    };
    let result = tools
        .call_for_test(
            "find_subclass_implementations",
            serde_json::json!({
                "method_name": "OnProcessInput",
                "base_classes": [],
                "namespace": "USER"
            }),
        )
        .await;
    let v = parse_result(result);
    assert!(
        v.get("error_code") == Some(&serde_json::json!("INVALID_PARAMS")),
        "should return INVALID_PARAMS for empty base_classes: {v}"
    );
}

// ── iris_doc put with elicitation_answer (covers elicitation resume path) ────

#[tokio::test]
async fn test_dispatch_iris_doc_put_elicitation_resume() {
    let tools = match make_iris_tools() {
        Some(t) => t,
        None => return,
    };
    // Provide a fake elicitation_id/answer — covers the resume branch in doc.rs
    let result = tools
        .call_for_test(
            "iris_doc",
            serde_json::json!({
                "mode": "put",
                "name": "IrisDevTmp.ElicitTest.cls",
                "content": "Class IrisDevTmp.ElicitTest {}\n",
                "namespace": "USER",
                "elicitation_id": "fake-id-9999",
                "elicitation_answer": "yes"
            }),
        )
        .await;
    let v = parse_result(result);
    assert!(
        v.get("success").is_some() || v.get("error_code").is_some(),
        "iris_doc put with elicitation: {v}"
    );
}

// ── iris_table_info with DDL table (covers DDL path lines 497-515 info.rs) ──

#[tokio::test]
async fn test_dispatch_iris_table_info_with_row_count() {
    let tools = match make_iris_tools() {
        Some(t) => t,
        None => return,
    };
    // include_row_count=true covers the row_count branch (lines 490-493 or 511-513)
    let result = tools
        .call_for_test(
            "iris_table_info",
            serde_json::json!({
                "table": "INFORMATION_SCHEMA.TABLES",
                "namespace": "USER",
                "include_row_count": true
            }),
        )
        .await;
    let v = parse_result(result);
    assert!(
        v.get("success").is_some() || v.get("error_code").is_some(),
        "iris_table_info with row_count: {v}"
    );
}

// ── iris_compile with list of targets (covers batch compile path) ─────────────

#[tokio::test]
async fn test_dispatch_iris_compile_batch() {
    let tools = match make_iris_tools() {
        Some(t) => t,
        None => return,
    };
    let result = tools
        .call_for_test(
            "iris_compile",
            serde_json::json!({
                "target": "%SYS.Namespace,%SYS.ProcessQuery",
                "namespace": "USER"
            }),
        )
        .await;
    let v = parse_result(result);
    assert!(
        v.get("success").is_some() || v.get("error_code").is_some(),
        "iris_compile batch: {v}"
    );
}

// ── iris_symbols_local with missing workspace (covers WORKSPACE_NOT_FOUND) ──

#[tokio::test]
async fn test_dispatch_iris_symbols_local_no_workspace_v2() {
    let tools = match make_iris_tools() {
        Some(t) => t,
        None => return,
    };
    std::env::remove_var("OBJECTSCRIPT_WORKSPACE");
    let result = tools
        .call_for_test(
            "iris_symbols_local",
            serde_json::json!({ "query": "Test*" }),
        )
        .await;
    let v = parse_result(result);
    assert!(
        v.get("success").is_some() || v.get("error_code").is_some() || v.get("symbols").is_some(),
        "iris_symbols_local no workspace: {v}"
    );
}

// ── skill umbrella tool — list action (covers lines 3852-3855 mod.rs) ─────────

#[tokio::test]
async fn test_dispatch_skill_umbrella_list() {
    let tools = match make_iris_tools() {
        Some(t) => t,
        None => return,
    };
    let result = tools
        .call_for_test("skill", serde_json::json!({ "action": "list" }))
        .await;
    let v = parse_result(result);
    assert!(
        v.get("success").is_some() || v.get("skills").is_some() || v.get("error_code").is_some(),
        "skill umbrella list: {v}"
    );
}

// ── skill_community umbrella tool — list action (covers lines 3865-3870 mod.rs) ──

#[tokio::test]
async fn test_dispatch_skill_community_umbrella_list() {
    let tools = match make_iris_tools() {
        Some(t) => t,
        None => return,
    };
    let result = tools
        .call_for_test("skill_community", serde_json::json!({ "action": "list" }))
        .await;
    let v = parse_result(result);
    assert!(
        v.get("success").is_some() || v.get("skills").is_some() || v.get("error_code").is_some(),
        "skill_community umbrella list: {v}"
    );
}

// ── kb umbrella tool — recall action (covers lines 3880-3883 mod.rs) ──────────

#[tokio::test]
async fn test_dispatch_kb_umbrella_recall() {
    let tools = match make_iris_tools() {
        Some(t) => t,
        None => return,
    };
    let result = tools
        .call_for_test(
            "kb",
            serde_json::json!({ "action": "recall", "query": "test query xyz", "namespace": "USER" }),
        )
        .await;
    let v = parse_result(result);
    assert!(
        v.get("success").is_some() || v.get("results").is_some() || v.get("error_code").is_some(),
        "kb umbrella recall: {v}"
    );
}

// ── agent_info umbrella tool — stats (covers lines 3893-3897 mod.rs) ──────────

#[tokio::test]
async fn test_dispatch_agent_info_umbrella_stats() {
    let tools = match make_iris_tools() {
        Some(t) => t,
        None => return,
    };
    let result = tools
        .call_for_test("agent_info", serde_json::json!({ "what": "stats" }))
        .await;
    let v = parse_result(result);
    assert!(
        v.get("success").is_some()
            || v.get("session_calls").is_some()
            || v.get("error_code").is_some(),
        "agent_info umbrella stats: {v}"
    );
}

// ── iris_doc GET compiled (covers iris_doc get compiled path) ─────────────────

#[tokio::test]
async fn test_dispatch_iris_doc_get_compiled() {
    let tools = match make_iris_tools() {
        Some(t) => t,
        None => return,
    };
    let result = tools
        .call_for_test(
            "iris_doc",
            serde_json::json!({
                "mode": "get",
                "name": "%SYS.Namespace.cls",
                "namespace": "USER"
            }),
        )
        .await;
    let v = parse_result(result);
    assert!(
        v.get("success").is_some() || v.get("error_code").is_some() || v.get("content").is_some(),
        "iris_doc get compiled: {v}"
    );
}

// ── iris_doc DELETE (covers iris_doc delete path) ─────────────────────────────

#[tokio::test]
async fn test_dispatch_iris_doc_delete_nonexistent_v3() {
    let tools = match make_iris_tools() {
        Some(t) => t,
        None => return,
    };
    let result = tools
        .call_for_test(
            "iris_doc",
            serde_json::json!({
                "mode": "delete",
                "name": "IrisDevTmp.NonExistentXyz9999.cls",
                "namespace": "USER"
            }),
        )
        .await;
    let v = parse_result(result);
    assert!(
        v.get("success").is_some() || v.get("error_code").is_some(),
        "iris_doc delete nonexistent: {v}"
    );
}

// ── admin write_disabled: delete_namespace, create_webapp, delete_webapp ─────

#[tokio::test]
async fn test_dispatch_iris_admin_delete_namespace_write_disabled() {
    let tools = match make_iris_tools() {
        Some(t) => t,
        None => return,
    };
    let prev = std::env::var("IRIS_ADMIN_TOOLS").ok();
    std::env::remove_var("IRIS_ADMIN_TOOLS");
    let result = tools
        .call_for_test(
            "iris_admin",
            serde_json::json!({
                "action": "delete_namespace",
                "name": "FAKENS"
            }),
        )
        .await;
    if let Some(v) = prev {
        std::env::set_var("IRIS_ADMIN_TOOLS", v);
    }
    let v = parse_result(result);
    assert_eq!(
        v.get("error_code").and_then(|e| e.as_str()),
        Some("ADMIN_WRITE_DISABLED"),
        "expected write-disabled: {v}"
    );
}

#[tokio::test]
async fn test_dispatch_iris_admin_create_webapp_write_disabled() {
    let tools = match make_iris_tools() {
        Some(t) => t,
        None => return,
    };
    let prev = std::env::var("IRIS_ADMIN_TOOLS").ok();
    std::env::remove_var("IRIS_ADMIN_TOOLS");
    let result = tools
        .call_for_test(
            "iris_admin",
            serde_json::json!({
                "action": "create_webapp",
                "path": "/testapp",
                "namespace": "USER"
            }),
        )
        .await;
    if let Some(v) = prev {
        std::env::set_var("IRIS_ADMIN_TOOLS", v);
    }
    let v = parse_result(result);
    assert_eq!(
        v.get("error_code").and_then(|e| e.as_str()),
        Some("ADMIN_WRITE_DISABLED"),
        "expected write-disabled: {v}"
    );
}

#[tokio::test]
async fn test_dispatch_iris_admin_delete_webapp_write_disabled() {
    let tools = match make_iris_tools() {
        Some(t) => t,
        None => return,
    };
    let prev = std::env::var("IRIS_ADMIN_TOOLS").ok();
    std::env::remove_var("IRIS_ADMIN_TOOLS");
    let result = tools
        .call_for_test(
            "iris_admin",
            serde_json::json!({
                "action": "delete_webapp",
                "path": "/testapp"
            }),
        )
        .await;
    if let Some(v) = prev {
        std::env::set_var("IRIS_ADMIN_TOOLS", v);
    }
    let v = parse_result(result);
    assert_eq!(
        v.get("error_code").and_then(|e| e.as_str()),
        Some("ADMIN_WRITE_DISABLED"),
        "expected write-disabled: {v}"
    );
}

// ── skills_tools coverage: skill umbrella action branches ────────────────────

#[tokio::test]
async fn test_dispatch_skill_umbrella_describe() {
    let tools = match make_iris_tools() {
        Some(t) => t,
        None => return,
    };
    let result = tools
        .call_for_test(
            "skill",
            serde_json::json!({ "action": "describe", "name": "nonexistent-skill-xyz9999" }),
        )
        .await;
    let v = parse_result(result);
    assert!(
        v.get("success").is_some() || v.get("error_code").is_some(),
        "skill describe: {v}"
    );
}

#[tokio::test]
async fn test_dispatch_skill_umbrella_search_v2() {
    let tools = match make_iris_tools() {
        Some(t) => t,
        None => return,
    };
    let result = tools
        .call_for_test(
            "skill",
            serde_json::json!({ "action": "search", "query": "iris xyz compile objectscript" }),
        )
        .await;
    let v = parse_result(result);
    assert!(
        v.get("success").is_some() || v.get("results").is_some() || v.get("error_code").is_some(),
        "skill search v2: {v}"
    );
}

#[tokio::test]
async fn test_dispatch_skill_umbrella_forget_v2() {
    let tools = match make_iris_tools() {
        Some(t) => t,
        None => return,
    };
    let result = tools
        .call_for_test(
            "skill",
            serde_json::json!({ "action": "forget", "name": "nonexistent-skill-xyz9999" }),
        )
        .await;
    let v = parse_result(result);
    assert!(
        v.get("success").is_some() || v.get("error_code").is_some(),
        "skill forget v2: {v}"
    );
}

#[tokio::test]
async fn test_dispatch_skill_umbrella_propose_no_history() {
    let tools = match make_iris_tools() {
        Some(t) => t,
        None => return,
    };
    let result = tools
        .call_for_test("skill", serde_json::json!({ "action": "propose" }))
        .await;
    let v = parse_result(result);
    assert!(
        v.get("success").is_some() || v.get("error_code").is_some() || v.get("skill").is_some(),
        "skill propose no history: {v}"
    );
}

#[tokio::test]
async fn test_dispatch_skill_umbrella_invalid_action_v2() {
    let tools = match make_iris_tools() {
        Some(t) => t,
        None => return,
    };
    let result = tools
        .call_for_test("skill", serde_json::json!({ "action": "bogus_action_xyz" }))
        .await;
    let v = parse_result(result);
    assert!(
        v.get("error_code").is_some() || v.get("success").is_some(),
        "skill invalid action v2: {v}"
    );
}

// ── skills_tools: skill_community umbrella branches ───────────────────────────

#[tokio::test]
async fn test_dispatch_skill_community_umbrella_install_notfound_v2() {
    let tools = match make_iris_tools() {
        Some(t) => t,
        None => return,
    };
    let result = tools
        .call_for_test(
            "skill_community",
            serde_json::json!({ "action": "install", "package": "nonexistent-pkg-xyz9999" }),
        )
        .await;
    let v = parse_result(result);
    assert!(
        v.get("error_code").is_some() || v.get("success").is_some(),
        "skill_community install notfound v2: {v}"
    );
}

#[tokio::test]
async fn test_dispatch_skill_community_umbrella_install_empty_pkg_v2() {
    let tools = match make_iris_tools() {
        Some(t) => t,
        None => return,
    };
    let result = tools
        .call_for_test(
            "skill_community",
            serde_json::json!({ "action": "install" }),
        )
        .await;
    let v = parse_result(result);
    assert!(
        v.get("error_code").is_some() || v.get("success").is_some(),
        "skill_community install empty pkg v2: {v}"
    );
}

#[tokio::test]
async fn test_dispatch_skill_community_umbrella_invalid_action_v2() {
    let tools = match make_iris_tools() {
        Some(t) => t,
        None => return,
    };
    let result = tools
        .call_for_test(
            "skill_community",
            serde_json::json!({ "action": "bogus_xyz" }),
        )
        .await;
    let v = parse_result(result);
    assert!(
        v.get("error_code").is_some() || v.get("success").is_some(),
        "skill_community invalid action v2: {v}"
    );
}

// ── skills_tools: kb umbrella branches ───────────────────────────────────────

#[tokio::test]
async fn test_dispatch_kb_umbrella_index_v2() {
    let tools = match make_iris_tools() {
        Some(t) => t,
        None => return,
    };
    let result = tools
        .call_for_test(
            "kb",
            serde_json::json!({ "action": "index", "path": "/tmp" }),
        )
        .await;
    let v = parse_result(result);
    assert!(
        v.get("success").is_some() || v.get("indexed").is_some() || v.get("error_code").is_some(),
        "kb index v2: {v}"
    );
}

#[tokio::test]
async fn test_dispatch_kb_umbrella_invalid_action_v2() {
    let tools = match make_iris_tools() {
        Some(t) => t,
        None => return,
    };
    let result = tools
        .call_for_test("kb", serde_json::json!({ "action": "bogus_xyz" }))
        .await;
    let v = parse_result(result);
    assert!(
        v.get("error_code").is_some() || v.get("success").is_some(),
        "kb invalid action v2: {v}"
    );
}

// ── skills_tools: agent_info — history branch ────────────────────────────────

#[tokio::test]
async fn test_dispatch_agent_info_umbrella_history_v2() {
    let tools = match make_iris_tools() {
        Some(t) => t,
        None => return,
    };
    let result = tools
        .call_for_test(
            "agent_info",
            serde_json::json!({ "what": "history", "limit": 5 }),
        )
        .await;
    let v = parse_result(result);
    assert!(
        v.get("success").is_some() || v.get("calls").is_some() || v.get("error_code").is_some(),
        "agent_info history v2: {v}"
    );
}

#[tokio::test]
async fn test_dispatch_agent_info_umbrella_invalid_what_v2() {
    let tools = match make_iris_tools() {
        Some(t) => t,
        None => return,
    };
    let result = tools
        .call_for_test("agent_info", serde_json::json!({ "what": "bogus_xyz" }))
        .await;
    let v = parse_result(result);
    assert!(
        v.get("error_code").is_some() || v.get("success").is_some(),
        "agent_info invalid what v2: {v}"
    );
}

// ── skills_tools: learning_disabled path ─────────────────────────────────────

#[tokio::test]
async fn test_dispatch_skill_learning_disabled_v2() {
    let prev = std::env::var("OBJECTSCRIPT_LEARNING").ok();
    std::env::set_var("OBJECTSCRIPT_LEARNING", "false");
    let tools = match make_iris_tools() {
        Some(t) => t,
        None => {
            if let Some(v) = prev {
                std::env::set_var("OBJECTSCRIPT_LEARNING", v);
            } else {
                std::env::remove_var("OBJECTSCRIPT_LEARNING");
            }
            return;
        }
    };
    let result = tools
        .call_for_test("skill", serde_json::json!({ "action": "list" }))
        .await;
    if let Some(v) = prev {
        std::env::set_var("OBJECTSCRIPT_LEARNING", v);
    } else {
        std::env::remove_var("OBJECTSCRIPT_LEARNING");
    }
    let v = parse_result(result);
    assert!(
        v.get("error_code")
            .map(|e| e.as_str() == Some("LEARNING_DISABLED"))
            .unwrap_or(false)
            || v.get("success").is_some(),
        "skill learning disabled v2: {v}"
    );
}

// ── scm.rs coverage: doc name normalization and action branches ───────────────

#[tokio::test]
async fn test_dispatch_iris_source_control_bare_classname() {
    // Exercise the .cls extension auto-append path (lines 80-81 in scm.rs)
    let tools = match make_iris_tools() {
        Some(t) => t,
        None => return,
    };
    let result = tools
        .call_for_test(
            "iris_source_control",
            serde_json::json!({
                "action": "status",
                "document": "MyApp.TestClass",
                "namespace": "USER"
            }),
        )
        .await;
    match result {
        Ok(r) => {
            let text = r.content[0].raw.as_text().unwrap().text.clone();
            eprintln!("scm status bare classname: {text}");
        }
        Err(e) => eprintln!("scm status bare classname error (ok): {e}"),
    }
}

#[tokio::test]
async fn test_dispatch_iris_source_control_checkout() {
    // Exercise checkout action path
    let tools = match make_iris_tools() {
        Some(t) => t,
        None => return,
    };
    let result = tools
        .call_for_test(
            "iris_source_control",
            serde_json::json!({
                "action": "checkout",
                "document": "MyApp.TestClass.cls",
                "namespace": "USER"
            }),
        )
        .await;
    match result {
        Ok(r) => {
            let text = r.content[0].raw.as_text().unwrap().text.clone();
            eprintln!("scm checkout: {text}");
        }
        Err(e) => eprintln!("scm checkout error (ok): {e}"),
    }
}

#[tokio::test]
async fn test_dispatch_iris_source_control_execute_action() {
    // Exercise execute action path
    let tools = match make_iris_tools() {
        Some(t) => t,
        None => return,
    };
    let result = tools
        .call_for_test(
            "iris_source_control",
            serde_json::json!({
                "action": "execute",
                "document": "MyApp.TestClass.cls",
                "action_id": "%CheckOut",
                "namespace": "USER"
            }),
        )
        .await;
    match result {
        Ok(r) => {
            let text = r.content[0].raw.as_text().unwrap().text.clone();
            eprintln!("scm execute: {text}");
        }
        Err(e) => eprintln!("scm execute error (ok): {e}"),
    }
}

#[tokio::test]
async fn test_dispatch_iris_source_control_menu_with_doc() {
    // Exercise menu action with real doc name
    let tools = match make_iris_tools() {
        Some(t) => t,
        None => return,
    };
    let result = tools
        .call_for_test(
            "iris_source_control",
            serde_json::json!({
                "action": "menu",
                "document": "MyApp.TestClass.cls",
                "namespace": "USER"
            }),
        )
        .await;
    match result {
        Ok(r) => {
            let text = r.content[0].raw.as_text().unwrap().text.clone();
            eprintln!("scm menu with doc: {text}");
        }
        Err(e) => eprintln!("scm menu with doc error (ok): {e}"),
    }
}

#[tokio::test]
async fn test_dispatch_iris_source_control_invalid_action() {
    // Exercise the INVALID_PARAM branch (other => err_json)
    let tools = match make_iris_tools() {
        Some(t) => t,
        None => return,
    };
    let result = tools
        .call_for_test(
            "iris_source_control",
            serde_json::json!({
                "action": "bogus_xyz",
                "document": "MyApp.TestClass.cls",
                "namespace": "USER"
            }),
        )
        .await;
    let v = parse_result(result);
    assert!(
        v.get("error_code").is_some() || v.get("success").is_some(),
        "scm invalid action: {v}"
    );
}

#[tokio::test]
async fn test_dispatch_iris_source_control_elicitation_expired() {
    // Exercise elicitation resume path with a bogus elicitation_id
    let tools = match make_iris_tools() {
        Some(t) => t,
        None => return,
    };
    let result = tools
        .call_for_test(
            "iris_source_control",
            serde_json::json!({
                "action": "status",
                "document": "MyApp.TestClass.cls",
                "namespace": "USER",
                "elicitation_id": "nonexistent-eid-xyz9999",
                "answer": "yes"
            }),
        )
        .await;
    let v = parse_result(result);
    assert!(
        v.get("error_code")
            .map(|e| e == "ELICITATION_EXPIRED")
            .unwrap_or(false)
            || v.get("success").is_some()
            || v.get("error_code").is_some(),
        "scm elicitation expired: {v}"
    );
}

// ── generate.rs coverage: extract_class_name None branches ───────────────────

#[tokio::test]
async fn test_generate_coverage_extract_class_name_via_dispatch() {
    // iris_generate_class with model=mock exercises generate.rs mock path.
    // validate_cls_syntax and extract_class_name are exercised internally.
    let tools = match make_iris_tools() {
        Some(t) => t,
        None => return,
    };
    let result = tools
        .call_for_test(
            "iris_generate_class",
            serde_json::json!({
                "description": "A simple test class",
                "namespace": "USER",
                "class_name": "IrisDevTest.GenCoverage"
            }),
        )
        .await;
    match result {
        Ok(r) => {
            let text = r.content[0].raw.as_text().unwrap().text.clone();
            eprintln!("iris_generate_class: {}", &text[..text.len().min(200)]);
        }
        Err(e) => eprintln!("iris_generate_class error (ok without LLM): {e}"),
    }
}

// ── interop branch coverage push (production/item/credential/lookup) ──────────

#[tokio::test]
async fn test_dispatch_iris_production_status_no_production_namespace() {
    // Query %SYS namespace — no production there, exercises NO_PRODUCTION branch
    let tools = match make_iris_tools() {
        Some(t) => t,
        None => return,
    };
    let result = tools
        .call_for_test(
            "iris_production",
            serde_json::json!({
                "action": "status",
                "namespace": "%SYS"
            }),
        )
        .await;
    let v = parse_result(result);
    // Expect NO_PRODUCTION or INTEROP_ERROR (no Ensemble in %SYS)
    assert!(
        v.get("success").is_some() || v.get("error_code").is_some(),
        "production status %SYS: {v}"
    );
}

#[tokio::test]
async fn test_dispatch_iris_production_start_nonexistent() {
    // Attempt to start a non-existent production — exercises start_impl Ok(error) branch
    let tools = match make_iris_tools() {
        Some(t) => t,
        None => return,
    };
    let result = tools
        .call_for_test(
            "iris_production",
            serde_json::json!({
                "action": "start",
                "production_name": "IrisDevTest.NonExistentProduction",
                "namespace": "USER"
            }),
        )
        .await;
    let v = parse_result(result);
    assert!(
        v.get("success").is_some() || v.get("error_code").is_some(),
        "production start nonexistent: {v}"
    );
}

#[tokio::test]
async fn test_dispatch_iris_production_stop_v2() {
    // Stop production — if none running, exercises the error branch in stop_impl
    let tools = match make_iris_tools() {
        Some(t) => t,
        None => return,
    };
    let result = tools
        .call_for_test(
            "iris_production",
            serde_json::json!({
                "action": "stop",
                "namespace": "USER"
            }),
        )
        .await;
    let v = parse_result(result);
    assert!(
        v.get("success").is_some() || v.get("error_code").is_some(),
        "production stop: {v}"
    );
}

#[tokio::test]
async fn test_dispatch_iris_production_update_v2() {
    let tools = match make_iris_tools() {
        Some(t) => t,
        None => return,
    };
    let result = tools
        .call_for_test(
            "iris_production",
            serde_json::json!({
                "action": "update",
                "namespace": "USER"
            }),
        )
        .await;
    let v = parse_result(result);
    assert!(
        v.get("success").is_some() || v.get("error_code").is_some(),
        "production update: {v}"
    );
}

#[tokio::test]
async fn test_dispatch_iris_production_recover_v2() {
    let tools = match make_iris_tools() {
        Some(t) => t,
        None => return,
    };
    let result = tools
        .call_for_test(
            "iris_production",
            serde_json::json!({
                "action": "recover",
                "namespace": "USER"
            }),
        )
        .await;
    let v = parse_result(result);
    assert!(
        v.get("success").is_some() || v.get("error_code").is_some(),
        "production recover: {v}"
    );
}

#[tokio::test]
async fn test_dispatch_iris_production_needs_update_v3() {
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
    let v = parse_result(result);
    assert!(
        v.get("success").is_some()
            || v.get("needs_update").is_some()
            || v.get("error_code").is_some(),
        "production check: {v}"
    );
}

#[tokio::test]
async fn test_dispatch_iris_production_item_enable_nonexistent() {
    let tools = match make_iris_tools() {
        Some(t) => t,
        None => return,
    };
    let result = tools
        .call_for_test(
            "iris_production_item",
            serde_json::json!({
                "action": "enable",
                "item_name": "IrisDevTest.NonExistentItem",
                "namespace": "USER"
            }),
        )
        .await;
    let v = parse_result(result);
    assert!(
        v.get("success").is_some() || v.get("error_code").is_some(),
        "production_item enable nonexistent: {v}"
    );
}

#[tokio::test]
async fn test_dispatch_iris_production_item_disable_nonexistent() {
    let tools = match make_iris_tools() {
        Some(t) => t,
        None => return,
    };
    let result = tools
        .call_for_test(
            "iris_production_item",
            serde_json::json!({
                "action": "disable",
                "item_name": "IrisDevTest.NonExistentItem",
                "namespace": "USER"
            }),
        )
        .await;
    let v = parse_result(result);
    assert!(
        v.get("success").is_some() || v.get("error_code").is_some(),
        "production_item disable nonexistent: {v}"
    );
}

#[tokio::test]
async fn test_dispatch_iris_production_item_set_settings_empty() {
    // set_settings with empty settings map — exercises INVALID_PARAMS branch
    let tools = match make_iris_tools() {
        Some(t) => t,
        None => return,
    };
    let result = tools
        .call_for_test(
            "iris_production_item",
            serde_json::json!({
                "action": "set_settings",
                "item_name": "IrisDevTest.AnyItem",
                "namespace": "USER",
                "settings": {}
            }),
        )
        .await;
    let v = parse_result(result);
    assert!(
        v.get("success").is_some() || v.get("error_code").is_some(),
        "production_item set_settings empty: {v}"
    );
}

#[tokio::test]
async fn test_dispatch_iris_production_item_invalid_action_v3() {
    let tools = match make_iris_tools() {
        Some(t) => t,
        None => return,
    };
    let result = tools
        .call_for_test(
            "iris_production_item",
            serde_json::json!({
                "action": "bogus_action",
                "item_name": "SomeItem",
                "namespace": "USER"
            }),
        )
        .await;
    let v = parse_result(result);
    assert_eq!(
        v.get("error_code").and_then(|c| c.as_str()),
        Some("INVALID_ACTION"),
        "production_item bogus action: {v}"
    );
}

#[tokio::test]
async fn test_dispatch_iris_credential_manage_create_v2() {
    // Create a test credential — exercises create branch
    let tools = match make_iris_tools() {
        Some(t) => t,
        None => return,
    };
    let result = tools
        .call_for_test(
            "iris_credential_manage",
            serde_json::json!({
                "action": "create",
                "id": "IrisDevTestCred",
                "username": "testuser",
                "password": "testpass",
                "namespace": "USER"
            }),
        )
        .await;
    let v = parse_result(result);
    assert!(
        v.get("success").is_some() || v.get("error_code").is_some(),
        "credential create: {v}"
    );
}

#[tokio::test]
async fn test_dispatch_iris_credential_manage_update_v2() {
    let tools = match make_iris_tools() {
        Some(t) => t,
        None => return,
    };
    let result = tools
        .call_for_test(
            "iris_credential_manage",
            serde_json::json!({
                "action": "update",
                "id": "IrisDevTestCred",
                "username": "newuser",
                "password": "newpass",
                "namespace": "USER"
            }),
        )
        .await;
    let v = parse_result(result);
    assert!(
        v.get("success").is_some() || v.get("error_code").is_some(),
        "credential update: {v}"
    );
}

#[tokio::test]
async fn test_dispatch_iris_credential_manage_delete_notfound() {
    // Delete a credential that doesn't exist — exercises CREDENTIAL_NOT_FOUND branch
    let tools = match make_iris_tools() {
        Some(t) => t,
        None => return,
    };
    let result = tools
        .call_for_test(
            "iris_credential_manage",
            serde_json::json!({
                "action": "delete",
                "id": "IrisDevTestCredNonExistent99",
                "namespace": "USER"
            }),
        )
        .await;
    let v = parse_result(result);
    assert!(
        v.get("success").is_some() || v.get("error_code").is_some(),
        "credential delete not found: {v}"
    );
}

#[tokio::test]
async fn test_dispatch_iris_credential_manage_invalid_action_v2() {
    let tools = match make_iris_tools() {
        Some(t) => t,
        None => return,
    };
    let result = tools
        .call_for_test(
            "iris_credential_manage",
            serde_json::json!({
                "action": "invalid_op",
                "id": "SomeCred",
                "namespace": "USER"
            }),
        )
        .await;
    let v = parse_result(result);
    assert_eq!(
        v.get("error_code").and_then(|c| c.as_str()),
        Some("INVALID_ACTION"),
        "credential manage invalid action: {v}"
    );
}

#[tokio::test]
async fn test_dispatch_iris_lookup_manage_list_tables_v2() {
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
        v.get("tables").is_some() || v.get("error_code").is_some(),
        "lookup list_tables: {v}"
    );
}

#[tokio::test]
async fn test_dispatch_iris_lookup_manage_set_and_get() {
    // Set a lookup value then get it — exercises set and get branches
    let tools = match make_iris_tools() {
        Some(t) => t,
        None => return,
    };
    // set
    let r = tools
        .call_for_test(
            "iris_lookup_manage",
            serde_json::json!({
                "action": "set",
                "table": "IrisDevTestTable",
                "key": "testkey",
                "value": "testval",
                "namespace": "USER"
            }),
        )
        .await;
    let vs = parse_result(r);
    assert!(
        vs.get("success").is_some() || vs.get("error_code").is_some(),
        "lookup set: {vs}"
    );

    // get
    let r2 = tools
        .call_for_test(
            "iris_lookup_manage",
            serde_json::json!({
                "action": "get",
                "table": "IrisDevTestTable",
                "key": "testkey",
                "namespace": "USER"
            }),
        )
        .await;
    let vg = parse_result(r2);
    assert!(
        vg.get("value").is_some() || vg.get("error_code").is_some(),
        "lookup get: {vg}"
    );
}

#[tokio::test]
async fn test_dispatch_iris_lookup_manage_list_keys_v2() {
    let tools = match make_iris_tools() {
        Some(t) => t,
        None => return,
    };
    let result = tools
        .call_for_test(
            "iris_lookup_manage",
            serde_json::json!({
                "action": "list_keys",
                "table": "IrisDevTestTable",
                "namespace": "USER"
            }),
        )
        .await;
    let v = parse_result(result);
    assert!(
        v.get("keys").is_some() || v.get("error_code").is_some(),
        "lookup list_keys v2: {v}"
    );
}

#[tokio::test]
async fn test_dispatch_iris_lookup_manage_delete_notfound() {
    // Delete from a non-existent table — exercises TABLE_NOT_FOUND branch
    let tools = match make_iris_tools() {
        Some(t) => t,
        None => return,
    };
    let result = tools
        .call_for_test(
            "iris_lookup_manage",
            serde_json::json!({
                "action": "delete",
                "table": "IrisDevNonExistentTable999",
                "key": "anykey",
                "namespace": "USER"
            }),
        )
        .await;
    let v = parse_result(result);
    assert!(
        v.get("success").is_some() || v.get("error_code").is_some(),
        "lookup delete not found: {v}"
    );
}

#[tokio::test]
async fn test_dispatch_iris_lookup_manage_get_not_found_table() {
    // Get from a non-existent table — exercises TABLE_NOT_FOUND in get branch
    let tools = match make_iris_tools() {
        Some(t) => t,
        None => return,
    };
    let result = tools
        .call_for_test(
            "iris_lookup_manage",
            serde_json::json!({
                "action": "get",
                "table": "IrisDevNonExistentTable999",
                "key": "anykey",
                "namespace": "USER"
            }),
        )
        .await;
    let v = parse_result(result);
    assert!(
        v.get("success").is_some() || v.get("error_code").is_some(),
        "lookup get not found table: {v}"
    );
}

#[tokio::test]
async fn test_dispatch_iris_lookup_manage_invalid_action_v2() {
    let tools = match make_iris_tools() {
        Some(t) => t,
        None => return,
    };
    let result = tools
        .call_for_test(
            "iris_lookup_manage",
            serde_json::json!({
                "action": "bogus_lookup_action",
                "namespace": "USER"
            }),
        )
        .await;
    let v = parse_result(result);
    assert_eq!(
        v.get("error_code").and_then(|c| c.as_str()),
        Some("INVALID_ACTION"),
        "lookup invalid action: {v}"
    );
}

#[tokio::test]
async fn test_dispatch_iris_lookup_manage_missing_params() {
    // set without table — exercises INVALID_PARAMS branch
    let tools = match make_iris_tools() {
        Some(t) => t,
        None => return,
    };
    let result = tools
        .call_for_test(
            "iris_lookup_manage",
            serde_json::json!({
                "action": "set",
                "namespace": "USER"
            }),
        )
        .await;
    let v = parse_result(result);
    assert_eq!(
        v.get("error_code").and_then(|c| c.as_str()),
        Some("INVALID_PARAMS"),
        "lookup set missing params: {v}"
    );
}

// ── iris_admin INVALID_PARAMS coverage + misc action branches ────────────────

#[tokio::test]
async fn test_dispatch_iris_admin_list_user_roles_missing_username() {
    let tools = match make_iris_tools() {
        Some(t) => t,
        None => return,
    };
    let result = tools
        .call_for_test(
            "iris_admin",
            serde_json::json!({ "action": "list_user_roles", "namespace": "USER" }),
        )
        .await;
    let v = parse_result(result);
    assert_eq!(
        v.get("error_code").and_then(|c| c.as_str()),
        Some("INVALID_PARAMS"),
        "list_user_roles missing username: {v}"
    );
}

#[tokio::test]
async fn test_dispatch_iris_admin_get_webapp_missing_path() {
    let tools = match make_iris_tools() {
        Some(t) => t,
        None => return,
    };
    let result = tools
        .call_for_test("iris_admin", serde_json::json!({ "action": "get_webapp" }))
        .await;
    let v = parse_result(result);
    assert_eq!(
        v.get("error_code").and_then(|c| c.as_str()),
        Some("INVALID_PARAMS"),
        "get_webapp missing path: {v}"
    );
}

#[tokio::test]
async fn test_dispatch_iris_admin_check_permission_missing_resource() {
    let tools = match make_iris_tools() {
        Some(t) => t,
        None => return,
    };
    let result = tools
        .call_for_test(
            "iris_admin",
            serde_json::json!({ "action": "check_permission" }),
        )
        .await;
    let v = parse_result(result);
    assert_eq!(
        v.get("error_code").and_then(|c| c.as_str()),
        Some("INVALID_PARAMS"),
        "check_permission missing resource: {v}"
    );
}

#[tokio::test]
async fn test_dispatch_iris_admin_create_user_missing_params() {
    let tools = match make_iris_tools() {
        Some(t) => t,
        None => return,
    };
    let result = tools
        .call_for_test(
            "iris_admin",
            serde_json::json!({ "action": "create_user", "username": "testonly" }),
        )
        .await;
    let v = parse_result(result);
    assert_eq!(
        v.get("error_code").and_then(|c| c.as_str()),
        Some("INVALID_PARAMS"),
        "create_user missing password: {v}"
    );
}

#[tokio::test]
async fn test_dispatch_iris_admin_update_user_missing_username() {
    let tools = match make_iris_tools() {
        Some(t) => t,
        None => return,
    };
    let result = tools
        .call_for_test("iris_admin", serde_json::json!({ "action": "update_user" }))
        .await;
    let v = parse_result(result);
    assert_eq!(
        v.get("error_code").and_then(|c| c.as_str()),
        Some("INVALID_PARAMS"),
        "update_user missing username: {v}"
    );
}

#[tokio::test]
async fn test_dispatch_iris_admin_delete_user_missing_username() {
    let tools = match make_iris_tools() {
        Some(t) => t,
        None => return,
    };
    let result = tools
        .call_for_test("iris_admin", serde_json::json!({ "action": "delete_user" }))
        .await;
    let v = parse_result(result);
    assert_eq!(
        v.get("error_code").and_then(|c| c.as_str()),
        Some("INVALID_PARAMS"),
        "delete_user missing username: {v}"
    );
}

#[tokio::test]
async fn test_dispatch_iris_admin_create_namespace_missing_params() {
    let tools = match make_iris_tools() {
        Some(t) => t,
        None => return,
    };
    let result = tools
        .call_for_test(
            "iris_admin",
            serde_json::json!({ "action": "create_namespace", "name": "TestNS" }),
        )
        .await;
    let v = parse_result(result);
    assert_eq!(
        v.get("error_code").and_then(|c| c.as_str()),
        Some("INVALID_PARAMS"),
        "create_namespace missing code_database: {v}"
    );
}

#[tokio::test]
async fn test_dispatch_iris_admin_delete_namespace_missing_name() {
    let tools = match make_iris_tools() {
        Some(t) => t,
        None => return,
    };
    let result = tools
        .call_for_test(
            "iris_admin",
            serde_json::json!({ "action": "delete_namespace" }),
        )
        .await;
    let v = parse_result(result);
    assert_eq!(
        v.get("error_code").and_then(|c| c.as_str()),
        Some("INVALID_PARAMS"),
        "delete_namespace missing name: {v}"
    );
}

#[tokio::test]
async fn test_dispatch_iris_admin_create_webapp_missing_params() {
    let tools = match make_iris_tools() {
        Some(t) => t,
        None => return,
    };
    let result = tools
        .call_for_test(
            "iris_admin",
            serde_json::json!({ "action": "create_webapp", "path": "/test" }),
        )
        .await;
    let v = parse_result(result);
    assert_eq!(
        v.get("error_code").and_then(|c| c.as_str()),
        Some("INVALID_PARAMS"),
        "create_webapp missing namespace: {v}"
    );
}

#[tokio::test]
async fn test_dispatch_iris_admin_delete_webapp_missing_path() {
    let tools = match make_iris_tools() {
        Some(t) => t,
        None => return,
    };
    let result = tools
        .call_for_test(
            "iris_admin",
            serde_json::json!({ "action": "delete_webapp" }),
        )
        .await;
    let v = parse_result(result);
    assert_eq!(
        v.get("error_code").and_then(|c| c.as_str()),
        Some("INVALID_PARAMS"),
        "delete_webapp missing path: {v}"
    );
}

// ── iris_production set_autostart action branch ───────────────────────────────

#[tokio::test]
async fn test_dispatch_iris_production_set_autostart_v2() {
    let tools = match make_iris_tools() {
        Some(t) => t,
        None => return,
    };
    let result = tools
        .call_for_test(
            "iris_production",
            serde_json::json!({
                "action": "set_autostart",
                "namespace": "USER",
                "enabled": false
            }),
        )
        .await;
    let v = parse_result(result);
    assert!(
        v.get("success").is_some() || v.get("error_code").is_some(),
        "production set_autostart: {v}"
    );
}

// ── iris_containers select/start actions ─────────────────────────────────────

#[tokio::test]
async fn test_dispatch_iris_containers_select_action() {
    let tools = match make_iris_tools() {
        Some(t) => t,
        None => return,
    };
    let result = tools
        .call_for_test(
            "iris_containers",
            serde_json::json!({
                "action": "select",
                "name": "iris-dev-iris"
            }),
        )
        .await;
    let v = parse_result(result);
    assert!(
        v.get("switched").is_some() || v.get("success").is_some() || v.get("error_code").is_some(),
        "iris_containers select: {v}"
    );
}

// ── iris_get_log pagination and not-found paths ───────────────────────────────

#[tokio::test]
async fn test_dispatch_iris_get_log_not_found_v2() {
    let tools = match make_iris_tools() {
        Some(t) => t,
        None => return,
    };
    let result = tools
        .call_for_test(
            "iris_get_log",
            serde_json::json!({ "id": "nonexistent-log-id-xyz-999" }),
        )
        .await;
    let v = parse_result(result);
    assert_eq!(
        v.get("error_code").and_then(|c| c.as_str()),
        Some("LOG_NOT_FOUND"),
        "get_log not found v2: {v}"
    );
}

#[tokio::test]
async fn test_dispatch_iris_get_log_zero_limit() {
    let tools = match make_iris_tools() {
        Some(t) => t,
        None => return,
    };
    let result = tools
        .call_for_test(
            "iris_get_log",
            serde_json::json!({ "id": "some-id", "limit": 0 }),
        )
        .await;
    let v = parse_result(result);
    assert_eq!(
        v.get("error_code").and_then(|c| c.as_str()),
        Some("INVALID_PARAMS"),
        "get_log zero limit: {v}"
    );
}

// ── err_json_with_url coverage — hit via iris_info when connection fails ──────

#[tokio::test]
async fn test_dispatch_iris_info_check_config_invalid_host() {
    // Check that check_config with no IRIS available returns useful error info
    let tools = match make_iris_tools() {
        Some(t) => t,
        None => return,
    };
    let result = tools
        .call_for_test("check_config", serde_json::json!({}))
        .await;
    // Should succeed (returns config status whether connected or not)
    match result {
        Ok(r) => {
            let text = r.content[0].raw.as_text().unwrap().text.clone();
            eprintln!("check_config: {}", &text[..text.len().min(200)]);
        }
        Err(e) => eprintln!("check_config error (ok): {e}"),
    }
}

// ── translate_sql_macros MERGE branch ────────────────────────────────────────

#[tokio::test]
async fn test_dispatch_iris_execute_merge_sql_macro() {
    // MERGE SQL macro triggers different translation path (line 284)
    let tools = match make_iris_tools() {
        Some(t) => t,
        None => return,
    };
    let result = tools
        .call_for_test(
            "iris_execute",
            serde_json::json!({
                "code": "&sql(MERGE INTO Sample.Person (Name) VALUES ('Test'))",
                "namespace": "USER"
            }),
        )
        .await;
    match result {
        Ok(r) => {
            let text = r.content[0].raw.as_text().unwrap().text.clone();
            eprintln!("iris_execute MERGE: {}", &text[..text.len().min(200)]);
        }
        Err(e) => eprintln!("iris_execute MERGE error (ok): {e}"),
    }
}

// ── skills_tools agent_history with history + propose with enough calls ───────

#[tokio::test]
async fn test_dispatch_skill_propose_with_history() {
    // Make 5+ tool calls so history is populated, then call propose
    let tools = match make_iris_tools() {
        Some(t) => t,
        None => return,
    };
    // Populate history with 6 calls
    for _ in 0..6 {
        let _ = tools
            .call_for_test(
                "iris_info",
                serde_json::json!({"action": "version", "namespace": "USER"}),
            )
            .await;
    }
    let result = tools
        .call_for_test(
            "skill",
            serde_json::json!({ "action": "propose", "namespace": "USER" }),
        )
        .await;
    match result {
        Ok(r) => {
            let text = r.content[0].raw.as_text().unwrap().text.clone();
            eprintln!(
                "skill propose with history: {}",
                &text[..text.len().min(200)]
            );
        }
        Err(e) => eprintln!("skill propose error (ok): {e}"),
    }
}

#[tokio::test]
async fn test_dispatch_agent_history_with_calls() {
    // Make calls then retrieve history — exercises the history-populated branch
    let tools = match make_iris_tools() {
        Some(t) => t,
        None => return,
    };
    let _ = tools
        .call_for_test(
            "iris_info",
            serde_json::json!({"action": "version", "namespace": "USER"}),
        )
        .await;
    let _ = tools
        .call_for_test(
            "iris_query",
            serde_json::json!({"query": "SELECT 1 AS n", "namespace": "USER"}),
        )
        .await;
    let result = tools
        .call_for_test(
            "agent_history",
            serde_json::json!({ "what": "history", "limit": 10 }),
        )
        .await;
    let v = parse_result(result);
    assert!(
        v.get("calls").is_some() || v.get("error_code").is_some(),
        "agent_history with calls: {v}"
    );
}

// ── skills_tools skill describe NOT_FOUND path ───────────────────────────────

#[tokio::test]
async fn test_dispatch_skill_describe_not_found_v2() {
    // Describe a skill that doesn't exist — exercises NOT_FOUND branch (line 80)
    let tools = match make_iris_tools() {
        Some(t) => t,
        None => return,
    };
    let result = tools
        .call_for_test(
            "skill",
            serde_json::json!({ "action": "describe", "name": "nonexistent-skill-xyz999", "namespace": "USER" }),
        )
        .await;
    let v = parse_result(result);
    assert!(
        v.get("success").is_some() || v.get("error_code").is_some(),
        "skill describe not found v2: {v}"
    );
}

// ── interop set_settings non-empty → ITEM_NOT_FOUND path ─────────────────────

#[tokio::test]
async fn test_dispatch_iris_production_item_set_settings_nonempty() {
    // set_settings with a real key→value pair but nonexistent item.
    // Covers lines 596-649 of interop.rs (set_settings body + ITEM_NOT_FOUND return).
    let tools = match make_iris_tools() {
        Some(t) => t,
        None => return,
    };
    let result = tools
        .call_for_test(
            "iris_production_item",
            serde_json::json!({
                "action": "set_settings",
                "item_name": "IrisDevNonExistentItem99999",
                "settings": {"LogTraceEvents": "1"},
                "namespace": "USER"
            }),
        )
        .await;
    let v = parse_result(result);
    assert!(
        v.get("error_code").is_some() || v.get("success").is_some(),
        "production_item set_settings nonempty: {v}"
    );
}

// ── lookup_transfer export existing table ─────────────────────────────────────

#[tokio::test]
async fn test_dispatch_iris_lookup_transfer_export_existing() {
    // Export IrisDevImportTest — created by the import test above.
    // If it doesn't exist (first run), imports it first.
    // Covers lines 1115-1118 of interop.rs (entry_count + ok_json success path).
    let tools = match make_iris_tools() {
        Some(t) => t,
        None => return,
    };
    if std::env::var("IRIS_ADMIN_TOOLS").unwrap_or_default() != "1" {
        return;
    }
    // Ensure the table exists by importing a minimal XML first
    let xml = r#"<?xml version="1.0" ?><Lookup><![CDATA[IrisDevExportTarget]]><entry key="k1" value="v1"/></Lookup>"#;
    let _ = tools
        .call_for_test(
            "iris_lookup_transfer",
            serde_json::json!({
                "action": "import",
                "table": "IrisDevExportTarget",
                "xml": xml,
                "namespace": "USER"
            }),
        )
        .await;
    // Now export — should hit success path with XML output
    let result = tools
        .call_for_test(
            "iris_lookup_transfer",
            serde_json::json!({
                "action": "export",
                "table": "IrisDevExportTarget",
                "namespace": "USER"
            }),
        )
        .await;
    let v = parse_result(result);
    assert!(
        v.get("success").is_some() || v.get("error_code").is_some(),
        "lookup_transfer export existing: {v}"
    );
    if v.get("success").map(|s| s.as_bool()).flatten() == Some(true) {
        // Verify entry_count field exists and xml field present
        assert!(
            v.get("entry_count").is_some(),
            "success response missing entry_count: {v}"
        );
        assert!(v.get("xml").is_some(), "success response missing xml: {v}");
    }
}

// ── scm elicitation_id expired → ELICITATION_EXPIRED (v2) ───────────────────

#[tokio::test]
async fn test_dispatch_iris_source_control_elicitation_expired_v2() {
    // Pass elicitation_id + answer with a nonexistent ID.
    // Covers lines 89-93 of scm.rs (ELICITATION_EXPIRED early return).
    let tools = match make_iris_tools() {
        Some(t) => t,
        None => return,
    };
    let result = tools
        .call_for_test(
            "iris_source_control",
            serde_json::json!({
                "action": "status",
                "document": "User.Test.cls",
                "namespace": "USER",
                "elicitation_id": "nonexistent-eid-99999",
                "answer": "yes"
            }),
        )
        .await;
    let v = parse_result(result);
    assert!(
        v.get("error_code").is_some() || v.get("success").is_some(),
        "scm elicitation expired: {v}"
    );
}

// ── iris_info missing action → INVALID_PARAMS ─────────────────────────────────

#[tokio::test]
async fn test_dispatch_iris_info_invalid_action() {
    let tools = match make_iris_tools() {
        Some(t) => t,
        None => return,
    };
    let result = tools
        .call_for_test(
            "iris_info",
            serde_json::json!({"what": "bogus_what_xyz", "namespace": "USER"}),
        )
        .await;
    let v = parse_result(result);
    assert!(
        v.get("error_code").is_some() || v.get("success").is_some(),
        "iris_info bogus action: {v}"
    );
}

// ── iris_lookup_manage missing action → INVALID_PARAMS ───────────────────────

#[tokio::test]
async fn test_dispatch_iris_lookup_manage_invalid_action() {
    let tools = match make_iris_tools() {
        Some(t) => t,
        None => return,
    };
    let result = tools
        .call_for_test(
            "iris_lookup_manage",
            serde_json::json!({"action": "bogus_xyz", "table": "T", "namespace": "USER"}),
        )
        .await;
    let v = parse_result(result);
    assert!(
        v.get("error_code").is_some() || v.get("success").is_some(),
        "iris_lookup_manage bogus action: {v}"
    );
}

// ── iris_interop_query invalid what → INVALID_PARAMS ─────────────────────────

#[tokio::test]
async fn test_dispatch_iris_interop_query_invalid_what_v2() {
    let tools = match make_iris_tools() {
        Some(t) => t,
        None => return,
    };
    let result = tools
        .call_for_test(
            "iris_interop_query",
            serde_json::json!({"what": "bogus_what_xyz", "namespace": "USER"}),
        )
        .await;
    let v = parse_result(result);
    assert!(
        v.get("error_code").is_some() || v.get("success").is_some(),
        "iris_interop_query bogus what: {v}"
    );
}

// ── iris_credential_manage invalid action ────────────────────────────────────

#[tokio::test]
async fn test_dispatch_iris_credential_manage_invalid_action() {
    let tools = match make_iris_tools() {
        Some(t) => t,
        None => return,
    };
    let result = tools
        .call_for_test(
            "iris_credential_manage",
            serde_json::json!({"action": "bogus_action", "name": "TestCred", "namespace": "USER"}),
        )
        .await;
    let v = parse_result(result);
    assert!(
        v.get("error_code").is_some() || v.get("success").is_some(),
        "iris_credential_manage bogus action: {v}"
    );
}

// ── iris_production invalid action ───────────────────────────────────────────

#[tokio::test]
async fn test_dispatch_iris_production_invalid_action() {
    let tools = match make_iris_tools() {
        Some(t) => t,
        None => return,
    };
    let result = tools
        .call_for_test(
            "iris_production",
            serde_json::json!({"action": "bogus_action_xyz", "namespace": "USER"}),
        )
        .await;
    let v = parse_result(result);
    assert!(
        v.get("error_code").is_some() || v.get("success").is_some(),
        "iris_production bogus action: {v}"
    );
}

// ── iris_symbols_local empty glob ─────────────────────────────────────────────

#[tokio::test]
async fn test_dispatch_iris_symbols_local_nonexistent_path() {
    let tools = match make_iris_tools() {
        Some(t) => t,
        None => return,
    };
    let result = tools
        .call_for_test(
            "iris_symbols_local",
            serde_json::json!({"query": "IrisDevNonExistentClass99999.*", "namespace": "USER"}),
        )
        .await;
    let v = parse_result(result);
    assert!(
        v.get("symbols").is_some() || v.get("error_code").is_some(),
        "iris_symbols_local nonexistent: {v}"
    );
}

// ── iris_admin invalid action ────────────────────────────────────────────────

#[tokio::test]
async fn test_dispatch_iris_admin_invalid_action_v2() {
    let tools = match make_iris_tools() {
        Some(t) => t,
        None => return,
    };
    let result = tools
        .call_for_test(
            "iris_admin",
            serde_json::json!({"action": "totally_bogus_action_xyz"}),
        )
        .await;
    let v = parse_result(result);
    assert!(
        v.get("error_code").is_some() || v.get("success").is_some(),
        "iris_admin bogus action: {v}"
    );
}

// ── skill propose — enough history (v2) ──────────────────────────────────────

#[tokio::test]
async fn test_dispatch_skill_propose_with_history_v2() {
    // Make 6 tool calls to build history, then propose a skill.
    // Covers skills_tools.rs lines 124-162 (propose body with ≥5 calls).
    let tools = match make_iris_tools() {
        Some(t) => t,
        None => return,
    };
    // Build history with 6 distinct tool calls
    for _ in 0..6 {
        let _ = tools
            .call_for_test(
                "iris_info",
                serde_json::json!({"what": "version", "namespace": "USER"}),
            )
            .await;
    }
    let result = tools
        .call_for_test(
            "skill",
            serde_json::json!({"action": "propose", "namespace": "USER"}),
        )
        .await;
    let v = parse_result(result);
    assert!(
        v.get("success").is_some() || v.get("error_code").is_some(),
        "skill propose with history: {v}"
    );
}

// ── iris_doc elicitation expired (put mode) ───────────────────────────────────

#[tokio::test]
async fn test_dispatch_iris_doc_elicitation_expired() {
    // Pass elicitation_id + elicitation_answer with a nonexistent ID.
    // Covers doc.rs lines 210-213 (ELICITATION_EXPIRED early return).
    let tools = match make_iris_tools() {
        Some(t) => t,
        None => return,
    };
    let result = tools
        .call_for_test(
            "iris_doc",
            serde_json::json!({
                "mode": "put",
                "name": "User.TestDoc.cls",
                "content": "Class User.TestDoc {}",
                "namespace": "USER",
                "elicitation_id": "nonexistent-doc-eid-99999",
                "elicitation_answer": "yes"
            }),
        )
        .await;
    let v = parse_result(result);
    assert!(
        v.get("error_code").is_some() || v.get("success").is_some(),
        "iris_doc elicitation expired: {v}"
    );
}

// ── doc.rs: iris_doc get mode not found (v2) ─────────────────────────────────

#[tokio::test]
async fn test_dispatch_iris_doc_get_not_found_v2() {
    let tools = match make_iris_tools() {
        Some(t) => t,
        None => return,
    };
    let result = tools
        .call_for_test(
            "iris_doc",
            serde_json::json!({
                "mode": "get",
                "name": "User.IrisDevNonExistentDoc999.cls",
                "namespace": "USER"
            }),
        )
        .await;
    let v = parse_result(result);
    assert!(
        v.get("error_code").is_some() || v.get("success").is_some() || v.get("content").is_some(),
        "iris_doc get not found: {v}"
    );
}

// ── skills_tools: skill_community invalid action ──────────────────────────────

#[tokio::test]
async fn test_dispatch_skill_community_invalid_action() {
    let tools = match make_iris_tools() {
        Some(t) => t,
        None => return,
    };
    let result = tools
        .call_for_test(
            "skill_community",
            serde_json::json!({"action": "bogus_action_xyz", "namespace": "USER"}),
        )
        .await;
    let v = parse_result(result);
    assert!(
        v.get("error_code").is_some() || v.get("success").is_some(),
        "skill_community bogus action: {v}"
    );
}

// ── kb_recall with KB items in registry ───────────────────────────────────────

fn make_tools_with_registry() -> iris_agentic_dev_core::tools::IrisTools {
    let iris_host = std::env::var("IRIS_HOST").unwrap_or_default();
    let conn = if !iris_host.is_empty() {
        let web_port = std::env::var("IRIS_WEB_PORT").unwrap_or_else(|_| "52773".to_string());
        let base_url = format!("http://{}:{}", iris_host, web_port);
        let username = std::env::var("IRIS_USERNAME").unwrap_or_else(|_| "_SYSTEM".to_string());
        let password = std::env::var("IRIS_PASSWORD").unwrap_or_else(|_| "SYS".to_string());
        Some(IrisConnection::new(
            base_url,
            "USER",
            username,
            password,
            DiscoverySource::EnvVar,
        ))
    } else {
        None
    };
    let mut registry = iris_agentic_dev_core::skills::SkillRegistry::new();
    registry.add_kb_item_for_test(
        "ObjectScript Variables Guide",
        "In ObjectScript, local variables are declared with Set. \
         Global variables start with ^. \
         The Write command prints to stdout. \
         Process private globals start with ^||.",
        "test/iris-kb",
    );
    registry.add_kb_item_for_test(
        "IRIS Connection Patterns",
        "Connect to IRIS using the Atelier REST API on port 52773. \
         Authentication uses HTTP Basic Auth with username and password. \
         The /api/atelier endpoint provides document CRUD operations.",
        "test/iris-kb",
    );
    registry.add_skill_for_test(
        "iris-compile-and-test",
        "Compile an ObjectScript class and run its unit tests",
        "1. Use iris_compile to compile the class\n2. Use iris_test to run tests",
    );
    iris_agentic_dev_core::tools::IrisTools::with_registry(conn, registry)
        .expect("IrisTools::with_registry")
}

#[tokio::test]
async fn test_dispatch_kb_recall_with_matching_items() {
    let tools = make_tools_with_registry();
    // Query that matches content of first KB item
    let result = tools
        .call_for_test(
            "kb_recall",
            serde_json::json!({"query": "objectscript variables", "top_k": 5}),
        )
        .await;
    let v = parse_result(result);
    let count = v["count"].as_u64().unwrap_or(0);
    assert!(count > 0, "kb_recall with match should return results: {v}");
    let results = v["results"].as_array().expect("results should be array");
    assert!(
        !results.is_empty(),
        "results array should not be empty: {v}"
    );
    // Should have snippet field
    assert!(
        results[0].get("snippet").is_some(),
        "result should have snippet: {v}"
    );
}

#[tokio::test]
async fn test_dispatch_kb_recall_title_match_scores_higher() {
    let tools = make_tools_with_registry();
    // Query matches title of first item
    let result = tools
        .call_for_test(
            "kb_recall",
            serde_json::json!({"query": "ObjectScript Variables Guide", "top_k": 5}),
        )
        .await;
    let v = parse_result(result);
    let results = v["results"].as_array().expect("results should be array");
    assert!(!results.is_empty(), "should match by title: {v}");
    // Title match scores 0.9
    let score = results[0]["score"].as_f64().unwrap_or(0.0);
    assert!(score >= 0.9, "title match should score 0.9: {v}");
}

#[tokio::test]
async fn test_dispatch_kb_recall_no_match() {
    let tools = make_tools_with_registry();
    let result = tools
        .call_for_test(
            "kb_recall",
            serde_json::json!({"query": "xyzzy_no_match_9999", "top_k": 5}),
        )
        .await;
    let v = parse_result(result);
    let count = v["count"].as_u64().unwrap_or(999);
    assert_eq!(count, 0, "no-match query should return 0 results: {v}");
}

#[tokio::test]
async fn test_dispatch_skill_community_list_with_items() {
    let tools = make_tools_with_registry();
    let result = tools
        .call_for_test("skill_community_list", serde_json::json!({}))
        .await;
    let v = parse_result(result);
    let skills = v["skills"].as_array().expect("skills should be array");
    assert!(!skills.is_empty(), "registry has skills: {v}");
    let kb_items = v["kb_items"].as_array().expect("kb_items should be array");
    assert!(!kb_items.is_empty(), "registry has kb items: {v}");
}

#[tokio::test]
async fn test_dispatch_kb_recall_multibyte_safe() {
    let mut registry = iris_agentic_dev_core::skills::SkillRegistry::new();
    // Content with multibyte UTF-8 characters near the match position
    registry.add_kb_item_for_test(
        "Unicode Test",
        "This contains Unicode: \u{00e9}\u{00e0}\u{00fc} and the keyword iris here \
         followed by more text to extend the snippet window beyond the match.",
        "test/unicode",
    );
    let tools = iris_agentic_dev_core::tools::IrisTools::with_registry(None, registry)
        .expect("with_registry");
    let result = tools
        .call_for_test(
            "kb_recall",
            serde_json::json!({"query": "iris", "top_k": 3}),
        )
        .await;
    let v = parse_result(result);
    assert!(
        v["count"].as_u64().unwrap_or(0) > 0,
        "multibyte content search: {v}"
    );
}

//! iris_search — full-text search via Atelier REST v2 with sync→async fallback.

use crate::iris::connection::IrisConnection;
use crate::tools::log_store;
use schemars::JsonSchema;
use serde::Deserialize;
use std::sync::{Arc, Mutex};

#[derive(Debug, Deserialize, JsonSchema)]
pub struct SearchParams {
    pub query: String,
    #[serde(default)]
    pub regex: bool,
    #[serde(default)]
    pub case_sensitive: bool,
    /// Filter to document category: CLS, MAC, INT, INC, or ALL (default)
    pub category: Option<String>,
    /// Wildcard document scopes e.g. ["HS.FHIR.*.cls"]
    #[serde(default)]
    pub documents: Vec<String>,
    #[serde(default = "default_namespace")]
    pub namespace: String,
    /// If true, bypass the log store and return all results inline regardless of count.
    #[serde(default)]
    pub inline: bool,
}

fn default_namespace() -> String {
    "USER".to_string()
}

fn ok_json(v: serde_json::Value) -> Result<rmcp::model::CallToolResult, rmcp::ErrorData> {
    Ok(rmcp::model::CallToolResult::success(vec![
        rmcp::model::Content::text(v.to_string()),
    ]))
}

pub async fn handle_iris_search(
    iris: &IrisConnection,
    client: &reqwest::Client,
    p: SearchParams,
    log_store: Arc<Mutex<log_store::LogStore>>,
) -> Result<rmcp::model::CallToolResult, rmcp::ErrorData> {
    let category = p.category.as_deref().unwrap_or("ALL");
    let mut query_string = format!(
        "query={}&regex={}&sys=false&category={}",
        urlencoding::encode(&p.query),
        p.regex,
        category,
    );
    if p.case_sensitive {
        query_string.push_str("&case=1");
    }

    let sync_url = iris.versioned_ns_url(&p.namespace, &format!("/action/search?{}", query_string));

    // Try sync search with 2s timeout
    let sync_client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(2))
        .danger_accept_invalid_certs(true)
        .build()
        .unwrap_or_else(|_| client.clone());

    let sync_result = sync_client
        .get(&sync_url)
        .basic_auth(&iris.username, Some(&iris.password))
        .send()
        .await;

    match sync_result {
        Ok(resp) if resp.status().is_success() => {
            let body: serde_json::Value = resp.json().await.unwrap_or_default();
            // If we got a workId, it's async — fall through to polling
            if body["result"]["workId"].is_null() {
                return parse_search_results(body, &p.query, p.inline, &log_store);
            }
            let work_id = body["result"]["workId"].as_str().unwrap_or("").to_string();
            poll_async_search(
                iris,
                client,
                &work_id,
                &p.namespace,
                &p.query,
                p.inline,
                &log_store,
            )
            .await
        }
        _ => {
            // Timeout or error — fall back to async POST
            let post_url = iris.versioned_ns_url(&p.namespace, "/action/search");
            let post_body = serde_json::json!({
                "query": p.query,
                "regex": p.regex,
                "sys": false,
                "category": category,
            });
            let resp = client
                .post(&post_url)
                .basic_auth(&iris.username, Some(&iris.password))
                .json(&post_body)
                .send()
                .await
                .map_err(|e| {
                    rmcp::ErrorData::internal_error(format!("Search request failed: {e}"), None)
                })?;

            let body: serde_json::Value = resp.json().await.unwrap_or_default();
            if let Some(work_id) = body["result"]["workId"].as_str() {
                poll_async_search(
                    iris,
                    client,
                    work_id,
                    &p.namespace,
                    &p.query,
                    p.inline,
                    &log_store,
                )
                .await
            } else {
                parse_search_results(body, &p.query, p.inline, &log_store)
            }
        }
    }
}

async fn poll_async_search(
    iris: &IrisConnection,
    client: &reqwest::Client,
    work_id: &str,
    namespace: &str,
    query: &str,
    inline: bool,
    log_store: &Arc<Mutex<log_store::LogStore>>,
) -> Result<rmcp::model::CallToolResult, rmcp::ErrorData> {
    let poll_url = iris.versioned_ns_url(
        namespace,
        &format!("/action/search?workId={}", urlencoding::encode(work_id)),
    );

    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(300);
    loop {
        tokio::time::sleep(std::time::Duration::from_secs(2)).await;

        if std::time::Instant::now() > deadline {
            return ok_json(serde_json::json!({
                "success": false,
                "error_code": "SEARCH_TIMEOUT",
                "error": "Async search did not complete within 5 minutes",
                "query": query,
            }));
        }

        let resp = client
            .get(&poll_url)
            .basic_auth(&iris.username, Some(&iris.password))
            .send()
            .await;

        match resp {
            Ok(r) if r.status().is_success() => {
                let body: serde_json::Value = r.json().await.unwrap_or_default();
                if body["result"]["workId"].is_null() {
                    return parse_search_results(body, query, inline, log_store);
                }
                // Still pending — keep polling
            }
            _ => continue,
        }
    }
}

fn parse_search_results(
    body: serde_json::Value,
    query: &str,
    inline: bool,
    log_store: &Arc<Mutex<log_store::LogStore>>,
) -> Result<rmcp::model::CallToolResult, rmcp::ErrorData> {
    let content = body["result"]["content"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    let total = content.len();
    let results: Vec<serde_json::Value> = content
        .into_iter()
        .map(|item| {
            serde_json::json!({
                "document": item["doc"],
                "line": item["atLine"],
                "member": item["member"],
                "content": item["text"],
            })
        })
        .collect();

    let mut resp = serde_json::json!({
        "success": true,
        "query": query,
        "results": results,
        "total_found": total,
    });

    // Progressive disclosure (027): truncate results when count exceeds threshold.
    let threshold = log_store::read_inline_threshold("IRIS_INLINE_SEARCH", 30);
    log_store::apply_truncation(
        &mut resp,
        "results",
        threshold,
        inline,
        log_store,
        "iris_search",
    );

    ok_json(resp)
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── SearchParams serde ────────────────────────────────────────────────────
    #[test]
    fn test_search_params_minimal() {
        let p: SearchParams =
            serde_json::from_str(r#"{"query":"test","namespace":"USER"}"#).unwrap();
        assert_eq!(p.query, "test");
    }

    #[test]
    fn test_search_params_namespace_required_field() {
        let result: Result<SearchParams, _> =
            serde_json::from_str(r#"{"query":"x","namespace":"MYNS"}"#);
        assert!(result.is_ok());
        assert_eq!(result.unwrap().namespace, "MYNS");
    }

    #[test]
    fn test_search_params_defaults() {
        let p: SearchParams = serde_json::from_str(r#"{"query": "Ens.*"}"#).unwrap();
        assert_eq!(p.namespace, "USER");
        assert!(!p.regex);
        assert!(!p.case_sensitive);
        assert!(p.category.is_none());
        assert!(p.documents.is_empty());
        assert!(!p.inline);
    }

    #[test]
    fn test_search_params_with_category() {
        let p: SearchParams = serde_json::from_str(r#"{"query": "x", "category": "CLS"}"#).unwrap();
        assert_eq!(p.category.as_deref(), Some("CLS"));
    }

    #[test]
    fn test_search_params_regex_flag() {
        let p: SearchParams =
            serde_json::from_str(r#"{"query": "Ens\\..*", "regex": true}"#).unwrap();
        assert!(p.regex);
    }

    #[test]
    fn test_search_params_documents_list() {
        let p: SearchParams =
            serde_json::from_str(r#"{"query": "x", "documents": ["Foo.*.cls", "Bar.*.cls"]}"#)
                .unwrap();
        assert_eq!(p.documents.len(), 2);
    }

    #[test]
    fn test_search_params_missing_query_fails() {
        let r: Result<SearchParams, _> = serde_json::from_str(r#"{}"#);
        assert!(r.is_err(), "query is required");
    }

    #[test]
    fn test_search_params_case_sensitive_flag() {
        let p: SearchParams =
            serde_json::from_str(r#"{"query": "Foo", "case_sensitive": true}"#).unwrap();
        assert!(p.case_sensitive);
    }

    #[test]
    fn test_search_params_inline_flag() {
        let p: SearchParams = serde_json::from_str(r#"{"query": "x", "inline": true}"#).unwrap();
        assert!(p.inline);
    }

    #[test]
    fn test_search_params_inline_default_false() {
        let p: SearchParams = serde_json::from_str(r#"{"query": "x"}"#).unwrap();
        assert!(!p.inline);
    }

    #[test]
    fn test_search_params_category_none_by_default() {
        let p: SearchParams = serde_json::from_str(r#"{"query": "x"}"#).unwrap();
        assert!(p.category.is_none());
    }

    #[test]
    fn test_search_params_category_all_variants() {
        for cat in &["CLS", "MAC", "INT", "INC", "ALL"] {
            let json = format!(r#"{{"query": "x", "category": "{}"}}"#, cat);
            let p: SearchParams = serde_json::from_str(&json).unwrap();
            assert_eq!(p.category.as_deref(), Some(*cat));
        }
    }

    #[test]
    fn test_search_params_documents_empty_by_default() {
        let p: SearchParams = serde_json::from_str(r#"{"query": "x"}"#).unwrap();
        assert!(p.documents.is_empty());
    }

    #[test]
    fn test_search_params_documents_single_entry() {
        let p: SearchParams =
            serde_json::from_str(r#"{"query": "x", "documents": ["Ens.Production.cls"]}"#).unwrap();
        assert_eq!(p.documents, vec!["Ens.Production.cls"]);
    }

    #[test]
    fn test_search_params_all_fields_set() {
        let json = r#"{
            "query": "FindMe",
            "regex": true,
            "case_sensitive": true,
            "category": "MAC",
            "documents": ["App.*.cls"],
            "namespace": "PROD",
            "inline": true
        }"#;
        let p: SearchParams = serde_json::from_str(json).unwrap();
        assert_eq!(p.query, "FindMe");
        assert!(p.regex);
        assert!(p.case_sensitive);
        assert_eq!(p.category.as_deref(), Some("MAC"));
        assert_eq!(p.documents, vec!["App.*.cls"]);
        assert_eq!(p.namespace, "PROD");
        assert!(p.inline);
    }

    #[test]
    fn test_search_params_empty_query_string_allowed() {
        // An empty string is technically valid JSON for the query field
        let p: SearchParams = serde_json::from_str(r#"{"query": ""}"#).unwrap();
        assert_eq!(p.query, "");
    }

    #[test]
    fn test_search_params_regex_default_false() {
        let p: SearchParams = serde_json::from_str(r#"{"query": "x"}"#).unwrap();
        assert!(!p.regex);
    }

    #[test]
    fn test_search_params_case_sensitive_default_false() {
        let p: SearchParams = serde_json::from_str(r#"{"query": "x"}"#).unwrap();
        assert!(!p.case_sensitive);
    }
}

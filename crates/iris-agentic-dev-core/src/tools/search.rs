//! iris_search — full-text search via Atelier REST v2 with sync→async fallback.

use crate::iris::connection::IrisConnection;
use crate::tools::log_store;
use schemars::JsonSchema;
use serde::Deserialize;
use std::sync::{Arc, Mutex};

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SearchParams {
    pub query: String,
    #[serde(default)]
    pub regex: bool,
    #[serde(default)]
    pub case_sensitive: bool,
    /// Filter to document category: CLS, MAC, INT, INC, or ALL (default)
    pub category: Option<String>,
    /// REQUIRED wildcard document scope, e.g. ["Region.**.*.cls"] or ["HS.FHIR.*.cls"].
    /// Atelier search is a sequential grep — an empty scope greps the whole namespace
    /// and times out server-side, so at least one scope must be provided.
    #[serde(default)]
    pub documents: Vec<String>,
    /// IRIS namespace. Defaults to the connection namespace (IRIS_NAMESPACE).
    #[serde(default)]
    pub namespace: Option<String>,
    /// Most matches to return, 1 to 10000. Defaults to 200, Atelier's own default. A response
    /// that reached it has `limit_reached: true`.
    #[serde(default)]
    pub limit: Option<u32>,
    /// If true, bypass the log store and return all results inline regardless of count.
    #[serde(default)]
    pub inline: bool,
    /// Route this call to a named registered IRIS instance. If omitted, uses the default connection.
    #[serde(default)]
    pub server: Option<String>,
}

/// Atelier's `max` when the parameter is absent.
const DEFAULT_LIMIT: u32 = 200;
const MAX_LIMIT: u32 = 10_000;
/// What Atelier writes to `console` when it stopped at `max`.
const LIMIT_REACHED: &str = "Maximum search limit reached";

/// The query Atelier is sent. Its regex must match a whole line, so a pattern is wrapped to match
/// anywhere in one; a pattern's own `^` and `$` still anchor it.
fn atelier_query(query: &str, regex: bool) -> String {
    if regex {
        format!(".*(?:{query}).*")
    } else {
        query.to_string()
    }
}

/// What shapes the response, the same whichever path the search took.
#[derive(Clone, Copy)]
struct Shape<'a> {
    query: &'a str,
    inline: bool,
    limit: u32,
}

fn ok_json(v: serde_json::Value) -> Result<rmcp::model::CallToolResult, rmcp::ErrorData> {
    Ok(rmcp::model::CallToolResult::structured(v))
}

/// Resolve the Atelier `files` scope for a search.
///
/// Atelier `/action/search` is a sequential grep, not an index: it searches only
/// the documents matched by the `files` wildcard list. A caller who supplies
/// `documents` gets exactly that scope (comma-joined). A caller who supplies none
/// is asking to grep the *entire* namespace — measured at 38s for a bare `*.cls`
/// and a hard 504 for `*.cls,*.mac,*.int,*.inc` on a HealthShare-sized namespace.
/// That can't be salvaged client-side (the server itself times out), so we return
/// `None` here and surface an explicit error rather than a misleading empty result.
fn resolve_files(documents: &[String]) -> Option<String> {
    if documents.is_empty() {
        return None;
    }
    Some(documents.join(","))
}

pub async fn handle_iris_search(
    iris: &IrisConnection,
    client: &reqwest::Client,
    p: SearchParams,
    log_store: Arc<Mutex<log_store::LogStore>>,
) -> Result<rmcp::model::CallToolResult, rmcp::ErrorData> {
    let namespace = crate::tools::resolve_namespace(p.namespace.as_deref(), &iris.namespace);
    let category = p.category.as_deref().unwrap_or("ALL");
    // The category goes into the query string; `CLS&sys=true` would add a parameter (137).
    if !["CLS", "MAC", "INT", "INC", "ALL"].contains(&category.to_ascii_uppercase().as_str()) {
        return crate::tools::err_result(serde_json::json!({
            "success": false,
            "error_code": "INVALID_PARAMS",
            "error": format!("category must be CLS, MAC, INT, INC or ALL, not '{category}'"),
        }));
    }
    let limit = p.limit.unwrap_or(DEFAULT_LIMIT);
    if !(1..=MAX_LIMIT).contains(&limit) {
        return crate::tools::err_result(serde_json::json!({
            "success": false,
            "error_code": "INVALID_PARAMS",
            "error": format!("limit must be between 1 and {MAX_LIMIT}, not {limit}"),
        }));
    }
    // A scope is mandatory. Without one Atelier would grep the whole namespace and
    // time out server-side, returning nothing — an empty result that reads as
    // "term not found" when the term is simply out of a (nonexistent) scope.
    let files = match resolve_files(&p.documents) {
        Some(f) => f,
        None => {
            return crate::tools::err_result(serde_json::json!({
                "success": false,
                "error_code": "SCOPE_REQUIRED",
                "error": "iris_search requires a document scope. Namespace-wide search \
                          greps every document sequentially and times out server-side. \
                          Pass `documents` with a wildcard scope, e.g. [\"Region.**.*.cls\"] \
                          or [\"MyPkg.*.cls\"].",
                "query": p.query,
            }));
        }
    };
    // Atelier `/action/search` treats a *missing* `case` param as case-SENSITIVE,
    // so omitting it (the old behaviour when `case_sensitive=false`) silently made
    // every default search exact-case — `hiddenset` or even `HiddenSet` would miss
    // unless the casing matched byte-for-byte. Always send `case` explicitly:
    // `case=0` = insensitive (the tool default), `case=1` = sensitive.
    let case_flag = if p.case_sensitive { 1 } else { 0 };
    // Atelier reads `regex=1`; `regex=true` reads as off, which made every regex a literal search.
    let regex_flag = if p.regex { 1 } else { 0 };
    let atelier_q = atelier_query(&p.query, p.regex);
    let shape = Shape {
        query: &p.query,
        inline: p.inline,
        limit,
    };
    let query_string = format!(
        "query={}&regex={}&sys=false&category={}&files={}&case={}&max={}",
        urlencoding::encode(&atelier_q),
        regex_flag,
        category,
        urlencoding::encode(&files),
        case_flag,
        limit,
    );

    let sync_url = iris.versioned_ns_url(namespace, &format!("/action/search?{}", query_string));

    // Try the synchronous search first. Many IRIS servers answer `/action/search`
    // synchronously — even for broad wildcard scopes that take several seconds —
    // and never hand back a `workId` to poll. The old 2s timeout tripped on those:
    // a broad `Region.**.*.cls` search (~5s here) timed out, fell through to the
    // async POST, which on those servers returns an empty `{}` (no workId) and
    // parsed as zero hits. Give the sync path a generous, env-overridable budget so
    // slow-but-synchronous results actually land. Async polling remains the fallback
    // for servers that genuinely defer via workId.
    let sync_timeout_secs = std::env::var("IRIS_SEARCH_SYNC_TIMEOUT")
        .ok()
        .and_then(|v| v.parse::<u64>().ok())
        .filter(|&v| v > 0)
        .unwrap_or(30);
    let sync_client = iris
        .client(crate::iris::http::ClientKind::Search)
        .unwrap_or_else(|_| client.clone());

    let sync_result = sync_client
        .get(&sync_url)
        .timeout(std::time::Duration::from_secs(sync_timeout_secs))
        .basic_auth(&iris.username, Some(&iris.password))
        .send()
        .await;

    match sync_result {
        Ok(resp) if resp.status().is_success() => {
            let body: serde_json::Value = crate::iris::http::json_or_default(resp).await;
            // If we got a workId, it's async — fall through to polling
            if body["result"]["workId"].is_null() {
                return parse_search_results(body, shape, &log_store);
            }
            let work_id = body["result"]["workId"].as_str().unwrap_or("").to_string();
            poll_async_search(iris, client, &work_id, namespace, shape, &log_store).await
        }
        _ => {
            // Timeout or error — fall back to async POST
            let post_url = iris.versioned_ns_url(namespace, "/action/search");
            let post_body = serde_json::json!({
                "query": atelier_q,
                "regex": regex_flag,
                "sys": false,
                "category": category,
                "files": files,
                // Always explicit — a missing `case` defaults to case-sensitive server-side.
                "case": case_flag,
                "max": limit,
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

            let body: serde_json::Value = crate::iris::http::json_or_default(resp).await;
            if let Some(work_id) = body["result"]["workId"].as_str() {
                poll_async_search(iris, client, work_id, namespace, shape, &log_store).await
            } else {
                parse_search_results(body, shape, &log_store)
            }
        }
    }
}

async fn poll_async_search(
    iris: &IrisConnection,
    client: &reqwest::Client,
    work_id: &str,
    namespace: &str,
    shape: Shape<'_>,
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
            return crate::tools::err_result(serde_json::json!({
                "success": false,
                "error_code": "SEARCH_TIMEOUT",
                "error": "Async search did not complete within 5 minutes",
                "query": shape.query,
            }));
        }

        let resp = client
            .get(&poll_url)
            .basic_auth(&iris.username, Some(&iris.password))
            .send()
            .await;

        match resp {
            Ok(r) if r.status().is_success() => {
                let body: serde_json::Value = crate::iris::http::json_or_default(r).await;
                if body["result"]["workId"].is_null() {
                    return parse_search_results(body, shape, log_store);
                }
                // Still pending — keep polling
            }
            _ => continue,
        }
    }
}

fn parse_search_results(
    body: serde_json::Value,
    shape: Shape<'_>,
    log_store: &Arc<Mutex<log_store::LogStore>>,
) -> Result<rmcp::model::CallToolResult, rmcp::ErrorData> {
    // Atelier's response shape for `/action/search` varies by API version:
    //   • v8 (IRIS 2024+): `result` is itself the array of document entries.
    //   • older/async:      `result.content` holds the array.
    // Accept whichever is present so results aren't silently dropped.
    let content = body["result"]
        .as_array()
        .or_else(|| body["result"]["content"].as_array())
        .cloned()
        .unwrap_or_default();

    // Atelier `/action/search` returns one entry per document. In v2 the matches
    // are nested under a `matches[]` array (`{doc, matches:[{text, line, member}]}`);
    // some responses/fixtures put a single match flat on the entry
    // (`{doc, atLine, text, member}`). Flatten both into one result per match so
    // `total_found` counts matches, not documents.
    let mut results: Vec<serde_json::Value> = Vec::new();
    for item in content {
        let doc = item["doc"].clone();
        match item["matches"].as_array() {
            Some(matches) if !matches.is_empty() => {
                for m in matches {
                    results.push(serde_json::json!({
                        "document": doc,
                        // v2 nested matches use `line`; tolerate `atLine` too.
                        "line": if m["line"].is_null() { m["atLine"].clone() } else { m["line"].clone() },
                        "member": m["member"],
                        "content": m["text"],
                    }));
                }
            }
            _ => {
                // Flat shape (or a document entry with no explicit matches array).
                results.push(serde_json::json!({
                    "document": doc,
                    "line": if item["line"].is_null() { item["atLine"].clone() } else { item["line"].clone() },
                    "member": item["member"],
                    "content": item["text"],
                }));
            }
        }
    }
    let total = results.len();
    // Atelier reports the cut-off only in `console`; a count at the limit is one too.
    let said_so = body["console"].as_array().is_some_and(|c| {
        c.iter()
            .filter_map(|l| l.as_str())
            .any(|l| l.contains(LIMIT_REACHED))
    });
    let reached = said_so || total >= shape.limit as usize;

    let mut resp = serde_json::json!({
        "success": true,
        "query": shape.query,
        "results": results,
        "total_found": total,
        "limit": shape.limit,
        "limit_reached": reached,
    });
    if reached {
        resp["warning"] = serde_json::json!(format!(
            "Atelier stopped at {total} matches, the limit, so there may be more. Narrow \
             `documents`, or raise `limit` (up to {MAX_LIMIT})."
        ));
    }

    // Progressive disclosure (027): truncate results when count exceeds threshold.
    let threshold = log_store::read_inline_threshold("IRIS_INLINE_SEARCH", 30);
    log_store::apply_truncation(
        &mut resp,
        "results",
        threshold,
        shape.inline,
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
        assert_eq!(result.unwrap().namespace.as_deref(), Some("MYNS"));
    }

    #[test]
    fn test_search_params_defaults() {
        let p: SearchParams = serde_json::from_str(r#"{"query": "Ens.*"}"#).unwrap();
        assert_eq!(p.namespace, None);
        assert_eq!(
            crate::tools::resolve_namespace(p.namespace.as_deref(), "APP"),
            "APP"
        );
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
        assert_eq!(p.namespace.as_deref(), Some("PROD"));
        assert_eq!(
            crate::tools::resolve_namespace(p.namespace.as_deref(), "APP"),
            "PROD"
        );
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

    // ── parse_search_results shape handling ───────────────────────────────────
    fn parse(body: serde_json::Value) -> serde_json::Value {
        parse_with_limit(body, DEFAULT_LIMIT)
    }

    fn parse_with_limit(body: serde_json::Value, limit: u32) -> serde_json::Value {
        let log = Arc::new(Mutex::new(log_store::LogStore::new(200, 60)));
        let shape = Shape {
            query: "q",
            inline: true,
            limit,
        };
        let r = parse_search_results(body, shape, &log).unwrap();
        let text = r.content[0].as_text().unwrap().text.clone();
        serde_json::from_str(&text).unwrap()
    }

    #[test]
    fn test_parse_nested_matches_shape() {
        // Real Atelier v2 shape: matches nested per document.
        let body = serde_json::json!({"result": {"workId": null, "content": [
            {"doc": "Foo.cls", "matches": [
                {"text": "ClassMethod A()", "line": 10, "member": "A"},
                {"text": "ClassMethod B()", "line": 20, "member": "B"}
            ]}
        ]}});
        let v = parse(body);
        // Two matches from one document → total counts matches, not docs.
        assert_eq!(v["total_found"], 2);
        assert_eq!(v["results"][0]["document"], "Foo.cls");
        assert_eq!(v["results"][0]["line"], 10);
        assert_eq!(v["results"][0]["content"], "ClassMethod A()");
        assert_eq!(v["results"][1]["line"], 20);
    }

    #[test]
    fn test_parse_v8_result_is_array_directly() {
        // IRIS 2024+ (Atelier API v8): `result` IS the document array, no `content` wrapper.
        let body = serde_json::json!({"result": [
            {"doc": "Region.FRXX.Foo.cls", "matches": [
                {"member": "Bar", "line": 122, "text": "set x = DataSource"},
                {"member": "Bar", "line": 135, "text": "quit DataSource"}
            ]}
        ]});
        let v = parse(body);
        assert_eq!(v["total_found"], 2);
        assert_eq!(v["results"][0]["document"], "Region.FRXX.Foo.cls");
        assert_eq!(v["results"][0]["line"], 122);
        assert_eq!(v["results"][0]["content"], "set x = DataSource");
        assert_eq!(v["results"][1]["line"], 135);
    }

    #[test]
    fn test_parse_flat_shape() {
        // Legacy/flat shape: one match per content entry.
        let body = serde_json::json!({"result": {"workId": null, "content": [
            {"doc": "Bar.cls", "atLine": 1, "text": "bar", "member": ""}
        ]}});
        let v = parse(body);
        assert_eq!(v["total_found"], 1);
        assert_eq!(v["results"][0]["document"], "Bar.cls");
        assert_eq!(v["results"][0]["line"], 1);
        assert_eq!(v["results"][0]["content"], "bar");
    }

    // ── resolve_files: a scope is mandatory (namespace-wide grep times out) ────
    #[test]
    fn test_resolve_files_honours_explicit_documents() {
        let docs = vec!["Region.**.*.cls".to_string(), "Foo.*.mac".to_string()];
        assert_eq!(
            resolve_files(&docs).as_deref(),
            Some("Region.**.*.cls,Foo.*.mac")
        );
    }

    #[test]
    fn test_resolve_files_single_document() {
        let docs = vec!["MyPkg.*.cls".to_string()];
        assert_eq!(resolve_files(&docs).as_deref(), Some("MyPkg.*.cls"));
    }

    #[test]
    fn test_resolve_files_empty_scope_is_rejected() {
        // No scope → None, so the handler returns SCOPE_REQUIRED instead of
        // greping the whole namespace and timing out.
        assert_eq!(resolve_files(&[]), None);
    }

    #[test]
    fn test_parse_empty_content() {
        let body = serde_json::json!({"result": {"workId": null, "content": []}});
        let v = parse(body);
        assert_eq!(v["total_found"], 0);
        assert_eq!(v["results"].as_array().unwrap().len(), 0);
    }

    // ── SCOPE_REQUIRED guard condition ────────────────────────────────────────
    #[test]
    fn test_scope_required_guard_empty_documents_no_category() {
        // Verify the guard condition that triggers SCOPE_REQUIRED:
        // SearchParams with no documents and no category should fail the scope check.
        let p: SearchParams =
            serde_json::from_str(r#"{"query": "FindMe", "documents": [], "category": null}"#)
                .unwrap();
        // Trigger the SCOPE_REQUIRED path by checking the guard condition directly
        assert!(
            p.documents.is_empty(),
            "documents must be empty to trigger SCOPE_REQUIRED"
        );
        assert!(
            p.category.is_none(),
            "category must be None to proceed to handler check"
        );
        // resolve_files should return None for empty documents
        assert_eq!(
            resolve_files(&p.documents),
            None,
            "empty documents should return None and trigger SCOPE_REQUIRED"
        );
    }

    #[test]
    fn test_scope_required_guard_with_category_still_requires_scope() {
        // Even with a category specified, empty documents should still fail
        let p: SearchParams =
            serde_json::from_str(r#"{"query": "x", "documents": [], "category": "CLS"}"#).unwrap();
        assert!(p.documents.is_empty());
        assert_eq!(resolve_files(&p.documents), None);
    }

    // ── IRIS_SEARCH_SYNC_TIMEOUT environment variable parsing ─────────────────
    #[test]
    fn test_iris_search_sync_timeout_unset_defaults_to_30() {
        // Simulate the parsing logic when env var is unset
        let parsed = std::env::var("IRIS_SEARCH_SYNC_TIMEOUT")
            .ok()
            .and_then(|v| v.parse::<u64>().ok())
            .filter(|&v| v > 0)
            .unwrap_or(30);
        // When unset (or missing), should default to 30
        // This test runs with the var unset, so we expect 30
        assert_eq!(parsed, 30);
    }

    #[test]
    fn test_iris_search_sync_timeout_valid_number() {
        // Simulate parsing when var is set to a valid number
        let valid_value = "5".parse::<u64>().ok().filter(|&v| v > 0).unwrap_or(30);
        assert_eq!(valid_value, 5);
    }

    #[test]
    fn test_iris_search_sync_timeout_invalid_string_falls_back() {
        // Simulate parsing when var is set to invalid string
        let invalid_value = "abc".parse::<u64>().ok().filter(|&v| v > 0).unwrap_or(30);
        assert_eq!(invalid_value, 30, "invalid string should fall back to 30");
    }

    #[test]
    fn test_iris_search_sync_timeout_zero_falls_back() {
        // Simulate parsing when var is set to zero (invalid)
        let zero_value = "0".parse::<u64>().ok().filter(|&v| v > 0).unwrap_or(30);
        assert_eq!(zero_value, 30, "zero should fall back to 30");
    }

    #[test]
    fn test_iris_search_sync_timeout_negative_falls_back() {
        // Negative numbers can't be parsed into u64, so should fall back to 30
        let negative_value = "-5".parse::<u64>().ok().filter(|&v| v > 0).unwrap_or(30);
        assert_eq!(negative_value, 30, "negative should fall back to 30");
    }

    // ── parse_search_results: count is match count, not document count ────────
    #[test]
    fn test_parse_single_match_with_correct_fields() {
        // Single document with one match should have total_found = 1
        let body = serde_json::json!({"result": {"workId": null, "content": [
            {"doc": "MyApp.Util.cls", "matches": [
                {"text": "MyMethod()", "line": 42, "member": "MyMethod"}
            ]}
        ]}});
        let v = parse(body);
        assert_eq!(v["total_found"], 1);
        assert_eq!(v["results"][0]["document"], "MyApp.Util.cls");
        assert_eq!(v["results"][0]["line"], 42);
        assert_eq!(v["results"][0]["member"], "MyMethod");
        assert_eq!(v["results"][0]["content"], "MyMethod()");
    }

    #[test]
    fn test_parse_multiple_matches_one_document_counts_matches() {
        // One document with 5 matches should count as 5 results, not 1
        let body = serde_json::json!({"result": [
            {"doc": "App.Handler.cls", "matches": [
                {"text": "line 1", "line": 10, "member": "M1"},
                {"text": "line 2", "line": 20, "member": "M2"},
                {"text": "line 3", "line": 30, "member": "M3"},
                {"text": "line 4", "line": 40, "member": "M4"},
                {"text": "line 5", "line": 50, "member": "M5"}
            ]}
        ]});
        let v = parse(body);
        // Key assertion: total_found counts matches, not documents
        assert_eq!(
            v["total_found"], 5,
            "should count 5 matches as 5 results, not 1 document"
        );
        assert_eq!(v["results"].as_array().unwrap().len(), 5);
    }

    #[test]
    fn test_parse_multiple_documents_multiple_matches_counts_total_matches() {
        // 2 docs with 2 matches each should be 4 total
        let body = serde_json::json!({"result": [
            {"doc": "Doc1.cls", "matches": [
                {"text": "a", "line": 1, "member": ""},
                {"text": "b", "line": 2, "member": ""}
            ]},
            {"doc": "Doc2.cls", "matches": [
                {"text": "c", "line": 3, "member": ""},
                {"text": "d", "line": 4, "member": ""}
            ]}
        ]});
        let v = parse(body);
        assert_eq!(
            v["total_found"], 4,
            "should count total matches across all documents"
        );
        assert_eq!(v["results"].as_array().unwrap().len(), 4);
    }

    #[test]
    fn test_parse_respects_atline_fallback_when_line_missing() {
        // Some responses use `atLine` instead of `line`
        let body = serde_json::json!({"result": {"workId": null, "content": [
            {"doc": "Legacy.cls", "atLine": 99, "text": "oldstyle", "member": "OldMethod"}
        ]}});
        let v = parse(body);
        assert_eq!(v["total_found"], 1);
        assert_eq!(v["results"][0]["line"], 99);
        assert_eq!(v["results"][0]["content"], "oldstyle");
    }

    // ── the match limit: Atelier stops at `max`, 200 when unsent ──────────────
    fn matches(n: usize) -> serde_json::Value {
        let m: Vec<_> = (1..=n)
            .map(|i| serde_json::json!({"text": "x", "line": i, "member": "M"}))
            .collect();
        serde_json::json!([{"doc": "A.cls", "matches": m}])
    }

    /// Atelier said it stopped, so the result is not the whole answer, and the response says so.
    #[test]
    fn atelier_reporting_its_limit_marks_the_limit_reached() {
        let body = serde_json::json!({
            "console": ["", "Found 3 occurrence/s in 1 file/s. Maximum search limit reached."],
            "result": matches(3),
        });
        let v = parse_with_limit(body, 500);
        assert_eq!(v["limit_reached"], true, "{v}");
        assert_eq!(v["limit"], 500);
        let w = v["warning"].as_str().unwrap_or("");
        assert!(w.contains("limit") && w.contains("documents"), "{w}");
    }

    /// A count that reaches the limit is a cut-off even when the console says nothing.
    #[test]
    fn a_count_at_the_limit_has_reached_it() {
        let v = parse_with_limit(serde_json::json!({"result": matches(5)}), 5);
        assert_eq!(v["limit_reached"], true, "{v}");
        assert_eq!(v["total_found"], 5);
    }

    #[test]
    fn a_count_under_the_limit_is_complete() {
        let body = serde_json::json!({
            "console": ["", "Found 4 occurrence/s in 1 file/s."],
            "result": matches(4),
        });
        let v = parse_with_limit(body, 200);
        assert_eq!(v["limit_reached"], false, "{v}");
        assert!(v.get("warning").is_none(), "{v}");
    }

    #[test]
    fn limit_is_a_parameter_and_defaults_to_atelier_s_own() {
        let p: SearchParams =
            serde_json::from_str(r#"{"query":"x","documents":["A.*.cls"],"limit":1000}"#).unwrap();
        assert_eq!(p.limit, Some(1000));
        let p: SearchParams = serde_json::from_str(r#"{"query":"x"}"#).unwrap();
        assert_eq!(p.limit, None);
        assert_eq!(DEFAULT_LIMIT, 200);
    }

    /// Atelier matches a regex against the whole line, so the pattern is wrapped to find it
    /// anywhere in one, the way grep does. A literal search is sent as it is.
    #[test]
    fn a_regex_is_wrapped_to_match_anywhere_in_the_line() {
        assert_eq!(atelier_query(r"##class\(%", true), r".*(?:##class\(%).*");
        assert_eq!(atelier_query("Q.it", false), "Q.it");
    }
}

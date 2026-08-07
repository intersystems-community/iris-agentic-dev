//! Cross-instance comparison tools — compare documents and namespaces across IRIS servers.

use crate::iris::connection::IrisConnection;
use rmcp::{model::*, ErrorData as McpError};
use std::sync::Arc;

// ── Error codes ──────────────────────────────────────────────────────────────
pub const ERR_COMPARE_FETCH_FAILED: &str = "FETCH_FAILED";

// ── Pure logic ───────────────────────────────────────────────────────────────

/// Produce a unified diff of two text strings using `similar`.
/// Lines that are unchanged have space prefix; removed lines start with `-`,
/// added lines start with `+`.
pub fn unified_diff(a: &str, b: &str) -> String {
    use similar::{ChangeTag, TextDiff};
    let diff = TextDiff::from_lines(a, b);
    let mut out = String::new();
    for change in diff.iter_all_changes() {
        let sign = match change.tag() {
            ChangeTag::Delete => "-",
            ChangeTag::Insert => "+",
            ChangeTag::Equal => " ",
        };
        out.push_str(sign);
        out.push_str(change.as_str().unwrap_or(""));
    }
    out
}

// ── Atelier helpers ──────────────────────────────────────────────────────────

/// Fetch the raw source text of a document from an IRIS server.
pub async fn fetch_document_source(
    iris: &IrisConnection,
    client: &reqwest::Client,
    document: &str,
    namespace: &str,
) -> Result<String, String> {
    let encoded = urlencoding::encode(document);
    let url = format!(
        "{}/api/atelier/v1/{}/doc/{}",
        iris.base_url, namespace, encoded
    );
    let resp = client
        .get(&url)
        .basic_auth(&iris.username, Some(&iris.password))
        .send()
        .await
        .map_err(|e| format!("HTTP error fetching {document}: {e}"))?;
    if !resp.status().is_success() {
        return Err(format!(
            "Server returned {} for document {document}",
            resp.status()
        ));
    }
    let body: serde_json::Value = resp
        .json()
        .await
        .map_err(|e| format!("JSON parse error: {e}"))?;
    // Atelier v1 doc response: { "result": { "content": ["line1", "line2", ...] } }
    let content = body["result"]["content"]
        .as_array()
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str())
                .collect::<Vec<_>>()
                .join("\n")
        })
        .unwrap_or_default();
    Ok(content)
}

/// Fetch the list of class names in a namespace from the Atelier docnames endpoint.
pub async fn fetch_class_list(
    iris: &IrisConnection,
    client: &reqwest::Client,
    namespace: &str,
) -> Result<Vec<String>, String> {
    let url = format!(
        "{}/api/atelier/v1/{}/docnames/CLS",
        iris.base_url, namespace
    );
    let resp = client
        .get(&url)
        .basic_auth(&iris.username, Some(&iris.password))
        .send()
        .await
        .map_err(|e| format!("HTTP error fetching class list: {e}"))?;
    if !resp.status().is_success() {
        return Err(format!("Server returned {} for class list", resp.status()));
    }
    let body: serde_json::Value = resp
        .json()
        .await
        .map_err(|e| format!("JSON parse error: {e}"))?;
    // Atelier v1 docnames: { "result": [{ "name": "Foo.Bar.cls" }, ...] }
    let names = body["result"]
        .as_array()
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v["name"].as_str().map(|s| s.to_string()))
                .collect()
        })
        .unwrap_or_default();
    Ok(names)
}

// ── compare_document ─────────────────────────────────────────────────────────

pub struct CompareDocumentParams {
    pub document: String,
    pub server_a: Arc<IrisConnection>,
    pub server_b: Arc<IrisConnection>,
    pub namespace: String,
}

pub async fn compare_document_impl(
    params: CompareDocumentParams,
    client: &reqwest::Client,
) -> Result<CallToolResult, McpError> {
    let source_a = fetch_document_source(
        &params.server_a,
        client,
        &params.document,
        &params.namespace,
    )
    .await;
    let source_b = fetch_document_source(
        &params.server_b,
        client,
        &params.document,
        &params.namespace,
    )
    .await;

    match (source_a, source_b) {
        (Err(e), _) => crate::tools::err_result(serde_json::json!({
            "success": false,
            "error_code": ERR_COMPARE_FETCH_FAILED,
            "error": format!("Failed to fetch from server_a: {e}"),
        })),
        (_, Err(e)) => crate::tools::err_result(serde_json::json!({
            "success": false,
            "error_code": ERR_COMPARE_FETCH_FAILED,
            "error": format!("Failed to fetch from server_b: {e}"),
        })),
        (Ok(a), Ok(b)) => {
            let same = a == b;
            let diff = if same {
                String::new()
            } else {
                unified_diff(&a, &b)
            };
            ok_json(serde_json::json!({
                "success": true,
                "document": params.document,
                "server_a": params.server_a.base_url,
                "server_b": params.server_b.base_url,
                "namespace": params.namespace,
                "same": same,
                "diff": diff,
            }))
        }
    }
}

// ── compare_namespace ────────────────────────────────────────────────────────

pub struct CompareNamespaceParams {
    pub namespace: String,
    pub server_a: Arc<IrisConnection>,
    pub server_b: Arc<IrisConnection>,
}

pub async fn compare_namespace_impl(
    params: CompareNamespaceParams,
    client: &reqwest::Client,
) -> Result<CallToolResult, McpError> {
    let list_a = fetch_class_list(&params.server_a, client, &params.namespace).await;
    let list_b = fetch_class_list(&params.server_b, client, &params.namespace).await;

    let (list_a, list_b) = match (list_a, list_b) {
        (Err(e), _) => {
            return crate::tools::err_result(serde_json::json!({
                "success": false,
                "error_code": ERR_COMPARE_FETCH_FAILED,
                "error": format!("Failed to list from server_a: {e}"),
            }))
        }
        (_, Err(e)) => {
            return crate::tools::err_result(serde_json::json!({
                "success": false,
                "error_code": ERR_COMPARE_FETCH_FAILED,
                "error": format!("Failed to list from server_b: {e}"),
            }))
        }
        (Ok(a), Ok(b)) => (a, b),
    };

    let set_a: std::collections::HashSet<String> = list_a.into_iter().collect();
    let set_b: std::collections::HashSet<String> = list_b.into_iter().collect();

    let mut only_in_a: Vec<String> = set_a.difference(&set_b).cloned().collect();
    let mut only_in_b: Vec<String> = set_b.difference(&set_a).cloned().collect();
    only_in_a.sort();
    only_in_b.sort();

    // For classes present in both, compare source text — cap at 200 to avoid overload.
    let mut common: Vec<String> = set_a.intersection(&set_b).cloned().collect();
    common.sort();
    let cap = common.len().min(200);

    let mut different: Vec<String> = Vec::new();
    let mut same_count: usize = 0;

    // Fetch and compare common classes sequentially in chunks to avoid overload.
    for doc in &common[..cap] {
        let a = fetch_document_source(&params.server_a, client, doc, &params.namespace).await;
        let b = fetch_document_source(&params.server_b, client, doc, &params.namespace).await;
        match (a, b) {
            (Ok(sa), Ok(sb)) => {
                if sa == sb {
                    same_count += 1;
                } else {
                    different.push(doc.clone());
                }
            }
            _ => {
                different.push(doc.clone());
            }
        }
    }

    let unchecked = if common.len() > cap {
        common.len() - cap
    } else {
        0
    };
    different.sort();

    ok_json(serde_json::json!({
        "success": true,
        "namespace": params.namespace,
        "server_a": params.server_a.base_url,
        "server_b": params.server_b.base_url,
        "only_in_a": only_in_a,
        "only_in_b": only_in_b,
        "different": different,
        "same_count": same_count,
        "unchecked_count": unchecked,
    }))
}

fn ok_json(v: serde_json::Value) -> Result<CallToolResult, McpError> {
    Ok(CallToolResult::success(vec![Content::text(v.to_string())]))
}

// ── Unit tests ───────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // T069: unified_diff produces expected markers
    #[test]
    fn diff_produces_minus_plus() {
        let d = unified_diff("a\nb\nc\n", "a\nX\nc\n");
        assert!(d.contains("-b"), "diff should contain -b, got: {d}");
        assert!(d.contains("+X"), "diff should contain +X, got: {d}");
    }

    #[test]
    fn diff_identical_has_no_change_markers() {
        let d = unified_diff("hello\nworld\n", "hello\nworld\n");
        assert!(
            !d.contains('-'),
            "identical diff should have no '-', got: {d}"
        );
        assert!(
            !d.contains('+'),
            "identical diff should have no '+', got: {d}"
        );
    }

    #[test]
    fn diff_empty_inputs() {
        let d = unified_diff("", "");
        assert!(d.is_empty() || !d.contains('-'));
    }
}

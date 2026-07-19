use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::{json, Value};

// Public search-only credentials embedded in every docs.intersystems.com page.
// Re-scrape if key rotates:
//   curl -sS -A "Mozilla/5.0" "https://docs.intersystems.com/irislatest/csp/docbook/DocBook.UI.Page.cls?KEY=GCM_monitoring" \
//     | grep -oiE 'ALG-[A-Za-z]+.*content="[^"]*"'
const ALGOLIA_APP_ID: &str = "EP91R43SFK";
const ALGOLIA_SEARCH_KEY: &str = "709759d92d99a5cf927e90c965741389";
const ALGOLIA_ENDPOINT: &str = "https://EP91R43SFK-dsn.algolia.net/1/indexes/docs/query";

const DEFAULT_HITS: u8 = 5;
const MAX_HITS: u8 = 10;
const EXCERPT_LEN: usize = 600;

#[derive(Debug, Deserialize, JsonSchema)]
pub struct IrisDocSearchParams {
    pub query: String,
    pub version: Option<String>,
    pub product: Option<String>,
    pub hits: Option<u8>,
}

pub fn build_request_body(p: &IrisDocSearchParams) -> Value {
    let hits = p.hits.map(|h| h.min(MAX_HITS)).unwrap_or(DEFAULT_HITS);

    let mut body = json!({
        "query": p.query,
        "hitsPerPage": hits,
        "attributesToRetrieve": ["title", "URL", "text", "breadcrumbs", "version", "product"]
    });

    let mut filters: Vec<String> = Vec::new();
    if let Some(v) = &p.version {
        filters.push(format!("version:{v}"));
    }
    if let Some(prod) = &p.product {
        filters.push(format!("product:{prod}"));
    }
    if !filters.is_empty() {
        body["facetFilters"] = json!(filters);
    }

    body
}

pub fn parse_hits(response: &Value) -> Vec<Value> {
    let hits = match response.get("hits").and_then(|h| h.as_array()) {
        Some(h) => h,
        None => return vec![],
    };

    hits.iter()
        .map(|hit| {
            let title = hit
                .get("title")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let url = hit
                .get("URL")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let text = hit.get("text").and_then(|v| v.as_str()).unwrap_or("");
            let excerpt = if text.len() > EXCERPT_LEN {
                text[..EXCERPT_LEN].to_string()
            } else {
                text.to_string()
            };
            let breadcrumbs = hit
                .get("breadcrumbs")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let version = hit
                .get("version")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let product = hit
                .get("product")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();

            json!({
                "title": title,
                "url": url,
                "excerpt": excerpt,
                "breadcrumbs": breadcrumbs,
                "version": version,
                "product": product
            })
        })
        .collect()
}

pub async fn handle_iris_doc_search(client: &reqwest::Client, p: &IrisDocSearchParams) -> Value {
    let body = build_request_body(p);

    let result = client
        .post(ALGOLIA_ENDPOINT)
        .header("X-Algolia-Application-Id", ALGOLIA_APP_ID)
        .header("X-Algolia-API-Key", ALGOLIA_SEARCH_KEY)
        .header("Content-Type", "application/json")
        .json(&body)
        .send()
        .await;

    match result {
        Err(e) => json!({
            "error": format!("Network error: {e}"),
            "hits": []
        }),
        Ok(resp) => {
            let status = resp.status();
            if !status.is_success() {
                return json!({
                    "error": format!("Algolia returned HTTP {status}"),
                    "hits": []
                });
            }
            match resp.json::<Value>().await {
                Err(e) => json!({
                    "error": format!("Failed to parse response: {e}"),
                    "hits": []
                }),
                Ok(data) => {
                    let hits = parse_hits(&data);
                    let nb_hits = data
                        .get("nbHits")
                        .and_then(|v| v.as_u64())
                        .unwrap_or(hits.len() as u64);
                    json!({
                        "query": p.query,
                        "total_hits": nb_hits,
                        "hits": hits
                    })
                }
            }
        }
    }
}

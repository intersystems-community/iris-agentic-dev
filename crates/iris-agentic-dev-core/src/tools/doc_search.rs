use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::{json, Value};

// docs.intersystems.com publishes its Algolia app id, index and search key in every page's
// `<meta class="algolia" name="ALG-...">` tags. They are read from a page at run time and kept for
// the life of the process, not copied into the source: the key the site served in October 2026
// carried write rights as well as search (TPRM-6602), so it will be rotated, and a copy here would
// stop the tool working the day it is.
const DOCS_PAGE: &str =
    "https://docs.intersystems.com/irislatest/csp/docbook/DocBook.UI.Page.cls?KEY=GCM_monitoring";
// The site answers a non-browser User-Agent with 403.
const BROWSER_UA: &str = "Mozilla/5.0";

static CREDS: std::sync::OnceLock<AlgoliaCreds> = std::sync::OnceLock::new();

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AlgoliaCreds {
    pub app_id: String,
    pub index: String,
    pub search_key: String,
}

fn meta_content<'a>(html: &'a str, name: &str) -> Option<&'a str> {
    let at = html.find(&format!("name=\"{name}\""))?;
    let (tag, _) = html[at..].split_once('>')?;
    let (_, value) = tag.split_once("content=\"")?;
    let (value, _) = value.split_once('"')?;
    Some(value)
}

/// The app id goes into a hostname and the key into a header, so each must be plainly an id.
fn plain_id(v: &str, min: usize, max: usize, extra: &[char]) -> bool {
    (min..=max).contains(&v.len())
        && v.chars()
            .all(|c| c.is_ascii_alphanumeric() || extra.contains(&c))
}

pub fn parse_algolia_meta(html: &str) -> Option<AlgoliaCreds> {
    let app_id = meta_content(html, "ALG-AppID")?;
    let index = meta_content(html, "ALG-Index")?;
    let search_key = meta_content(html, "ALG-SearchKey")?;
    (plain_id(app_id, 8, 32, &[])
        && plain_id(index, 1, 64, &['_', '-'])
        && plain_id(search_key, 16, 128, &[]))
    .then(|| AlgoliaCreds {
        app_id: app_id.to_string(),
        index: index.to_string(),
        search_key: search_key.to_string(),
    })
}

pub async fn fetch_algolia_creds(client: &reqwest::Client) -> Result<AlgoliaCreds, String> {
    if let Some(c) = CREDS.get() {
        return Ok(c.clone());
    }
    let creds = read_algolia_creds(client, DOCS_PAGE).await?;
    Ok(CREDS.get_or_init(|| creds).clone())
}

/// [`fetch_algolia_creds`] against `page`, without the process-wide cache.
#[doc(hidden)]
pub async fn fetch_algolia_creds_at(
    client: &reqwest::Client,
    page: &str,
) -> Result<AlgoliaCreds, String> {
    read_algolia_creds(client, page).await
}

async fn read_algolia_creds(client: &reqwest::Client, page: &str) -> Result<AlgoliaCreds, String> {
    let resp = client
        .get(page)
        .header("User-Agent", BROWSER_UA)
        .send()
        .await
        .map_err(|e| format!("could not fetch {page}: {e}"))?;
    if !resp.status().is_success() {
        return Err(format!("{page} returned HTTP {}", resp.status()));
    }
    // The page is read up to the external cap: it is HTML from a public host.
    let html = crate::iris::http::external_text(resp)
        .await
        .map_err(|e| format!("could not read {page}: {e}"))?;
    parse_algolia_meta(&html).ok_or_else(|| {
        format!("{page} carries no usable ALG-AppID, ALG-Index and ALG-SearchKey meta tags")
    })
}

const DEFAULT_HITS: u8 = 5;
const MAX_HITS: u8 = 10;
const EXCERPT_LEN: usize = 600;

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
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
    let creds = match fetch_algolia_creds(client).await {
        Ok(c) => c,
        Err(e) => {
            return json!({
                "error": format!("The docs search key is not available: {e}"),
                "error_code": "DOC_SEARCH_KEY_UNAVAILABLE",
                "hits": []
            })
        }
    };
    let body = build_request_body(p);
    let endpoint = format!(
        "https://{}-dsn.algolia.net/1/indexes/{}/query",
        creds.app_id, creds.index
    );

    let result = client
        .post(&endpoint)
        .header("X-Algolia-Application-Id", &creds.app_id)
        .header("X-Algolia-API-Key", &creds.search_key)
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
            match crate::iris::http::external_json::<Value>(resp).await {
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

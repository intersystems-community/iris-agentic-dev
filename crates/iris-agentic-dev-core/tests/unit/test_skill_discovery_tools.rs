//! Tool-level tests for `skill_list` / `skill_search` / `skill_describe` with no
//! IRIS connection available.
//!
//! Regression cover: these tools used to return `{"count":0,"results":[]}` when
//! IRIS was unreachable, hiding the 31 bundled skills shipped on disk. Bundled
//! skills are files — they need no IRIS. No IRIS is mocked here; the no-IRIS
//! path is exercised for real.

#![cfg(feature = "testing")]

use iris_agentic_dev_core::tools::IrisTools;

fn parse(result: Result<rmcp::model::CallToolResult, String>) -> serde_json::Value {
    let r = result.expect("tool call should not error");
    let text = r.content[0].as_text().expect("text content").text.clone();
    serde_json::from_str(&text).unwrap_or_else(|e| panic!("not JSON: {e}\n{text}"))
}

/// `IrisTools::new(None)` with no reachable IRIS — the connection is absent, so
/// only the bundled source can answer.
fn tools_without_iris() -> IrisTools {
    IrisTools::new(None).expect("IrisTools::new should succeed without IRIS")
}

#[tokio::test]
async fn skill_list_returns_bundled_skills_without_iris() {
    let tools = tools_without_iris();
    let v = parse(
        tools
            .call_for_test("skill_list", serde_json::json!({}))
            .await,
    );
    let skills = v["skills"].as_array().expect("skills array");
    assert!(
        !skills.is_empty(),
        "skill_list must surface bundled skills with no IRIS: {v}"
    );
    assert!(
        v["count"].as_u64().unwrap_or(0) >= 25,
        "count should reflect the bundled catalog: {v}"
    );
}

#[tokio::test]
async fn skill_list_labels_the_source_of_every_entry() {
    let tools = tools_without_iris();
    let v = parse(
        tools
            .call_for_test("skill_list", serde_json::json!({}))
            .await,
    );
    for s in v["skills"].as_array().unwrap() {
        let src = s["source"].as_str().unwrap_or("");
        assert!(
            src == "bundled" || src == "synthesized",
            "every entry needs a source field: {s}"
        );
    }
}

#[tokio::test]
async fn skill_list_reports_per_source_counts_and_iris_availability() {
    let tools = tools_without_iris();
    let v = parse(
        tools
            .call_for_test("skill_list", serde_json::json!({}))
            .await,
    );
    assert!(v["sources"]["bundled"]["available"].is_number(), "{v}");
    assert!(v["sources"]["synthesized"]["available"].is_number(), "{v}");
    assert_eq!(
        v["sources"]["synthesized"]["searched"], false,
        "with no IRIS the synthesized source must say it was not searched: {v}"
    );
    assert_eq!(v["sources"]["bundled"]["searched"], true, "{v}");
}

#[tokio::test]
async fn skill_search_finds_vector_skill_for_hnsw_query_without_iris() {
    let tools = tools_without_iris();
    let v = parse(
        tools
            .call_for_test(
                "skill_search",
                serde_json::json!({"query": "vector HNSW index"}),
            )
            .await,
    );
    let names: Vec<&str> = v["results"]
        .as_array()
        .expect("results array")
        .iter()
        .filter_map(|r| r["name"].as_str())
        .collect();
    assert!(
        names.contains(&"iris-vector-ai"),
        "\"vector HNSW index\" must find iris-vector-ai; got {names:?} from {v}"
    );
}

#[tokio::test]
async fn skill_search_matches_a_tag_that_appears_nowhere_else() {
    // "similarity-search" is only in iris-vector-ai's tags.
    let tools = tools_without_iris();
    let v = parse(
        tools
            .call_for_test(
                "skill_search",
                serde_json::json!({"query": "similarity-search"}),
            )
            .await,
    );
    assert!(
        v["count"].as_u64().unwrap_or(0) > 0,
        "tag-only match must work: {v}"
    );
}

#[tokio::test]
async fn skill_search_zero_hits_is_never_a_bare_zero() {
    let tools = tools_without_iris();
    let v = parse(
        tools
            .call_for_test(
                "skill_search",
                serde_json::json!({"query": "zzz-no-such-thing-qqq"}),
            )
            .await,
    );
    assert_eq!(v["count"], 0);
    // The caller must be able to tell that both sources were considered and how
    // many candidates each held.
    assert!(
        v["sources"]["bundled"]["available"].as_u64().unwrap_or(0) >= 25,
        "{v}"
    );
    assert_eq!(v["sources"]["bundled"]["searched"], true, "{v}");
    assert!(v["sources"]["synthesized"]["searched"].is_boolean(), "{v}");
    assert!(
        v["note"].as_str().unwrap_or("").contains("bundled"),
        "zero-result response must explain what was searched: {v}"
    );
}

#[tokio::test]
async fn skill_search_results_carry_source_labels() {
    let tools = tools_without_iris();
    let v = parse(
        tools
            .call_for_test("skill_search", serde_json::json!({"query": "sql"}))
            .await,
    );
    let results = v["results"].as_array().unwrap();
    assert!(!results.is_empty(), "expected sql hits: {v}");
    for r in results {
        assert_eq!(r["source"], "bundled", "{r}");
    }
}

#[tokio::test]
async fn skill_search_honours_top_k() {
    let tools = tools_without_iris();
    let v = parse(
        tools
            .call_for_test(
                "skill_search",
                serde_json::json!({"query": "iris", "top_k": 2}),
            )
            .await,
    );
    assert!(v["results"].as_array().unwrap().len() <= 2, "{v}");
}

#[tokio::test]
async fn skill_describe_finds_a_bundled_skill_without_iris() {
    let tools = tools_without_iris();
    let v = parse(
        tools
            .call_for_test(
                "skill_describe",
                serde_json::json!({"name": "iris-vector-ai"}),
            )
            .await,
    );
    assert_eq!(v["success"], true, "{v}");
    assert_eq!(v["skill"]["source"], "bundled", "{v}");
    assert!(
        v["skill"]["body"]
            .as_str()
            .unwrap_or("")
            .to_lowercase()
            .contains("hnsw"),
        "describe should return the skill body: {v}"
    );
}

#[tokio::test]
async fn skill_describe_unknown_name_lists_where_it_looked() {
    let tools = tools_without_iris();
    let v = parse(
        tools
            .call_for_test(
                "skill_describe",
                serde_json::json!({"name": "definitely-not-a-skill-xyz"}),
            )
            .await,
    );
    assert_eq!(v["success"], false, "{v}");
    assert_eq!(v["error_code"], "NOT_FOUND", "{v}");
    assert!(
        v["sources"]["bundled"]["available"].as_u64().unwrap_or(0) >= 25,
        "NOT_FOUND must still report what was searched: {v}"
    );
}

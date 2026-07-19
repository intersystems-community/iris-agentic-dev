//! Live integration tests for iris_doc_search — requires network access to Algolia.
//! Run with: cargo test --test test_doc_search_live -- --include-ignored

use iris_agentic_dev_core::tools::doc_search::{handle_iris_doc_search, IrisDocSearchParams};

fn make_client() -> reqwest::Client {
    reqwest::Client::new()
}

#[tokio::test]
#[ignore]
async fn doc_search_returns_hits_for_sql_query() {
    let client = make_client();
    let p = IrisDocSearchParams {
        query: "SQL execution methods ObjectScript".to_string(),
        version: None,
        product: Some("InterSystems IRIS".to_string()),
        hits: Some(3),
    };
    let result = handle_iris_doc_search(&client, &p).await;
    assert!(result.get("error").is_none(), "unexpected error: {result}");
    let hits = result["hits"].as_array().unwrap();
    assert!(!hits.is_empty(), "expected hits, got none");
    // Each hit should have required fields
    let first = &hits[0];
    assert!(first["title"].as_str().is_some_and(|s| !s.is_empty()));
    assert!(first["url"]
        .as_str()
        .is_some_and(|s| s.starts_with("https://")));
    assert!(first["excerpt"].as_str().is_some_and(|s| !s.is_empty()));
}

#[tokio::test]
#[ignore]
async fn doc_search_returns_empty_hits_for_garbage_query() {
    let client = make_client();
    let p = IrisDocSearchParams {
        query: "xyzzy frobnicator zzzzzz99999".to_string(),
        version: None,
        product: None,
        hits: None,
    };
    let result = handle_iris_doc_search(&client, &p).await;
    assert!(result.get("error").is_none(), "unexpected error: {result}");
    let hits = result["hits"].as_array().unwrap();
    assert!(hits.is_empty(), "expected no hits for garbage query");
    assert_eq!(result["total_hits"], 0);
}

#[tokio::test]
#[ignore]
async fn doc_search_version_filter_respected() {
    let client = make_client();
    let p = IrisDocSearchParams {
        query: "coverage line by line monitor".to_string(),
        version: Some("2025.1".to_string()),
        product: Some("InterSystems IRIS".to_string()),
        hits: Some(5),
    };
    let result = handle_iris_doc_search(&client, &p).await;
    assert!(result.get("error").is_none(), "unexpected error: {result}");
    let hits = result["hits"].as_array().unwrap();
    // All hits should be version 2025.1 when filter applied
    for hit in hits {
        let v = hit["version"].as_str().unwrap_or("");
        assert_eq!(v, "2025.1", "version filter not respected: got {v}");
    }
}

#[tokio::test]
#[ignore]
async fn doc_search_security_check_method_findable() {
    let client = make_client();
    let p = IrisDocSearchParams {
        query: "$SYSTEM.Security.Check permission resource".to_string(),
        version: None,
        product: Some("InterSystems IRIS".to_string()),
        hits: Some(5),
    };
    let result = handle_iris_doc_search(&client, &p).await;
    assert!(result.get("error").is_none(), "unexpected error: {result}");
    let hits = result["hits"].as_array().unwrap();
    assert!(!hits.is_empty(), "expected security docs to be findable");
    // At least one hit should mention Security or permission
    let any_relevant = hits.iter().any(|h| {
        let text = format!(
            "{} {}",
            h["title"].as_str().unwrap_or(""),
            h["excerpt"].as_str().unwrap_or("")
        )
        .to_lowercase();
        text.contains("security") || text.contains("permission") || text.contains("check")
    });
    assert!(any_relevant, "no security-related hits found");
}

#[tokio::test]
#[ignore]
async fn doc_search_hit_count_respects_hits_param() {
    let client = make_client();
    let p = IrisDocSearchParams {
        query: "interoperability business operation".to_string(),
        version: None,
        product: None,
        hits: Some(2),
    };
    let result = handle_iris_doc_search(&client, &p).await;
    assert!(result.get("error").is_none(), "unexpected error: {result}");
    let hits = result["hits"].as_array().unwrap();
    assert!(hits.len() <= 2, "expected ≤2 hits, got {}", hits.len());
}

#[tokio::test]
#[ignore]
async fn doc_search_excerpt_within_600_chars() {
    let client = make_client();
    let p = IrisDocSearchParams {
        query: "ObjectScript special variables".to_string(),
        version: None,
        product: Some("InterSystems IRIS".to_string()),
        hits: Some(3),
    };
    let result = handle_iris_doc_search(&client, &p).await;
    assert!(result.get("error").is_none(), "unexpected error: {result}");
    let hits = result["hits"].as_array().unwrap();
    for hit in hits {
        let excerpt = hit["excerpt"].as_str().unwrap_or("");
        assert!(
            excerpt.len() <= 600,
            "excerpt too long: {} chars",
            excerpt.len()
        );
    }
}

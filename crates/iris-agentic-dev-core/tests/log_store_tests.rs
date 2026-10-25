// Unit tests for the LogStore, LogEntry, and truncation helpers.
// These tests run without a live IRIS connection.
//
// Phase 2 tests (T004–T007): LogStore core logic — RED before implementation.
// Phase 3 tests (T018–T020): iris_compile truncation helper — RED before wiring.
// Phase 4 tests (T026–T027): iris_search truncation — RED before wiring.
// Phase 5 tests (T033–T035): iris_info + debug_get_error_logs — RED before wiring.
// Phase 6 tests (T044–T046): iris_get_log dispatch — RED before wiring.

use iris_agentic_dev_core::testing::EnvGuard;
use iris_agentic_dev_core::tools::log_store::{
    apply_truncation, new_log_id, read_inline_threshold, GetResult, LogEntry, LogStore,
};
use serde_json::{json, Value};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

// ── Helpers ──────────────────────────────────────────────────────────────────

fn make_entry(tool: &str, count: usize) -> LogEntry {
    let items: Vec<Value> = (0..count).map(|i| json!({"item": i})).collect();
    LogEntry {
        id: new_log_id(),
        tool: tool.to_string(),
        created_at: Instant::now(),
        preview: items[..count.min(5)].to_vec(),
        full_result: Value::Array(items),
        total_count: count,
    }
}

#[allow(dead_code)]
fn make_entry_aged(tool: &str, count: usize, age: Duration) -> LogEntry {
    LogEntry {
        id: new_log_id(),
        tool: tool.to_string(),
        created_at: Instant::now() - age,
        preview: vec![],
        full_result: json!([]),
        total_count: count,
    }
}

fn make_store(max: usize, ttl: u64) -> Arc<Mutex<LogStore>> {
    Arc::new(Mutex::new(LogStore::new(max, ttl)))
}

// ── T004: LogStore::store + eviction ─────────────────────────────────────────

#[test]
fn test_store_basic() {
    let mut s = LogStore::new(10, 60);
    let entry = make_entry("iris_compile", 5);
    let id = s.store(entry);
    assert!(!id.is_empty(), "returned id must be non-empty");
    assert_eq!(s.entries.len(), 1);
}

#[test]
fn test_store_id_format() {
    let id = new_log_id();
    // Must match iris-{13_digit_ms}-{8_hex_chars}
    let re = regex::Regex::new(r"^iris-\d{13}-[a-f0-9]{8}$").unwrap();
    assert!(
        re.is_match(&id),
        "id '{}' does not match iris-{{ms}}-{{short_uuid}} format",
        id
    );
}

#[test]
fn test_store_evicts_oldest_on_overflow() {
    let mut s = LogStore::new(2, 60);
    let e1 = make_entry("tool1", 1);
    let e2 = make_entry("tool2", 2);
    let e3 = make_entry("tool3", 3);
    let id1 = s.store(e1);
    let _id2 = s.store(e2);
    let _id3 = s.store(e3);
    assert_eq!(s.entries.len(), 2, "should hold max 2 entries");
    // e1 (id1) must have been evicted
    assert!(
        matches!(s.get(&id1), GetResult::NotFound),
        "oldest entry should have been evicted"
    );
}

// ── T005: LogStore::get ───────────────────────────────────────────────────────

#[test]
fn test_get_found() {
    let mut s = LogStore::new(10, 60);
    let entry = make_entry("iris_search", 10);
    let id = s.store(entry);
    match s.get(&id) {
        GetResult::Found(v) => assert!(v.is_array()),
        other => panic!("expected Found, got {:?}", std::mem::discriminant(&other)),
    }
}

#[test]
fn test_get_not_found() {
    let s = LogStore::new(10, 60);
    assert!(matches!(
        s.get("iris-9999999999999-unknown1"),
        GetResult::NotFound
    ));
}

#[test]
fn test_get_expired_returns_expired_not_evicted() {
    // TTL of 0 minutes = any entry is immediately expired.
    // The entry must still be in the store (not evicted) but get() returns Expired.
    let mut s = LogStore::new(10, 0);
    let entry = make_entry("iris_compile", 5);
    let id = s.store(entry);
    // Entry is still in the store
    assert_eq!(
        s.entries.len(),
        1,
        "entry should still be in store before get()"
    );
    assert!(
        matches!(s.get(&id), GetResult::Expired),
        "get() should return Expired, not NotFound"
    );
    // Entry is still in the store AFTER get() (not evicted by get)
    assert_eq!(
        s.entries.len(),
        1,
        "get() must NOT evict the entry — only list()/evict_expired() does"
    );
}

// ── T006: LogStore::list ──────────────────────────────────────────────────────

#[test]
fn test_list_empty_store() {
    let mut s = LogStore::new(10, 60);
    let summaries = s.list();
    assert!(summaries.is_empty());
}

#[test]
fn test_list_two_entries() {
    let mut s = LogStore::new(10, 60);
    s.store(make_entry("iris_compile", 30));
    s.store(make_entry("iris_search", 100));
    let summaries = s.list();
    assert_eq!(summaries.len(), 2);
    let tools: Vec<&str> = summaries.iter().map(|s| s.tool.as_str()).collect();
    assert!(tools.contains(&"iris_compile"));
    assert!(tools.contains(&"iris_search"));
}

#[test]
fn test_list_excludes_expired() {
    let mut s = LogStore::new(10, 0); // TTL=0 → all expired instantly
    s.store(make_entry("iris_compile", 5));
    let summaries = s.list(); // calls evict_expired first
    assert!(
        summaries.is_empty(),
        "expired entries must not appear in list"
    );
}

// ── T007: LogStore::evict_expired ────────────────────────────────────────────

#[test]
fn test_evict_all_with_zero_ttl() {
    let mut s = LogStore::new(10, 0);
    s.store(make_entry("t1", 1));
    s.store(make_entry("t2", 2));
    s.store(make_entry("t3", 3));
    s.evict_expired();
    assert!(
        s.entries.is_empty(),
        "all entries should be evicted with ttl=0"
    );
}

#[test]
fn test_evict_none_with_large_ttl() {
    let mut s = LogStore::new(10, 999);
    s.store(make_entry("t1", 1));
    s.store(make_entry("t2", 2));
    s.store(make_entry("t3", 3));
    s.evict_expired();
    assert_eq!(
        s.entries.len(),
        3,
        "no entries should be evicted with ttl=999"
    );
}

// ── T018: iris_compile truncation — 25 errors → truncated ────────────────────

#[test]
fn test_apply_truncation_above_threshold() {
    let store = make_store(50, 60);
    let errors: Vec<Value> = (0..25)
        .map(|i| json!({"severity": "error", "code": format!("E{}", i), "text": "msg"}))
        .collect();
    let mut result = json!({
        "success": false,
        "errors": errors,
        "warnings": [],
    });
    apply_truncation(&mut result, "errors", 20, false, &store, "iris_compile");
    assert_eq!(result["truncated"], json!(true));
    assert!(result["log_id"].is_string(), "log_id must be present");
    assert_eq!(result["inline_count"], json!(20));
    assert_eq!(result["total_count"], json!(25));
    let inline_errors = result["errors"].as_array().unwrap();
    assert_eq!(
        inline_errors.len(),
        20,
        "errors array should be trimmed to 20"
    );
    // Store should have one entry
    assert_eq!(store.lock().unwrap().entries.len(), 1);
}

// ── T019: iris_compile truncation — 15 errors → not truncated ────────────────

#[test]
fn test_apply_truncation_below_threshold() {
    let store = make_store(50, 60);
    let errors: Vec<Value> = (0..15)
        .map(|i| json!({"severity": "error", "code": format!("E{}", i)}))
        .collect();
    let mut result = json!({
        "success": false,
        "errors": errors,
        "warnings": [],
    });
    apply_truncation(&mut result, "errors", 20, false, &store, "iris_compile");
    assert_eq!(result["truncated"], json!(false));
    assert!(
        result.get("log_id").is_none() || result["log_id"].is_null(),
        "log_id must not be present when not truncated"
    );
    let inline_errors = result["errors"].as_array().unwrap();
    assert_eq!(inline_errors.len(), 15, "all 15 errors should be present");
    assert_eq!(
        store.lock().unwrap().entries.len(),
        0,
        "no entry should be stored"
    );
}

// ── T020: iris_compile truncation — inline=true bypass ───────────────────────

#[test]
fn test_apply_truncation_inline_bypass() {
    let store = make_store(50, 60);
    let errors: Vec<Value> = (0..25)
        .map(|i| json!({"severity": "error", "code": format!("E{}", i)}))
        .collect();
    let mut result = json!({
        "success": false,
        "errors": errors,
        "warnings": [],
    });
    apply_truncation(&mut result, "errors", 20, true, &store, "iris_compile");
    // inline=true → truncated:false, no log_id, full 25 errors
    assert_eq!(result["truncated"], json!(false));
    assert!(
        result.get("log_id").is_none() || result["log_id"].is_null(),
        "log_id must not be present when inline=true"
    );
    let inline_errors = result["errors"].as_array().unwrap();
    assert_eq!(
        inline_errors.len(),
        25,
        "all 25 errors must be present with inline=true"
    );
    assert_eq!(
        store.lock().unwrap().entries.len(),
        0,
        "no store entry when inline=true"
    );
}

// ── T026: iris_search truncation — 50 results → truncated ────────────────────

#[test]
fn test_search_truncation_above_threshold() {
    let store = make_store(50, 60);
    let results: Vec<Value> = (0..50)
        .map(|i| json!({"document": format!("Doc{}.cls", i), "line": i, "content": "x"}))
        .collect();
    let mut result = json!({
        "success": true,
        "query": "foo",
        "results": results,
        "total_found": 50,
    });
    apply_truncation(&mut result, "results", 30, false, &store, "iris_search");
    assert_eq!(result["truncated"], json!(true));
    assert!(result["log_id"].is_string());
    assert_eq!(result["inline_count"], json!(30));
    assert_eq!(result["total_count"], json!(50));
    assert_eq!(result["results"].as_array().unwrap().len(), 30);
}

// ── T027: iris_search truncation — 20 results → not truncated ────────────────

#[test]
fn test_search_truncation_below_threshold() {
    let store = make_store(50, 60);
    let results: Vec<Value> = (0..20)
        .map(|i| json!({"document": format!("Doc{}.cls", i), "line": i}))
        .collect();
    let mut result = json!({
        "success": true,
        "query": "foo",
        "results": results,
    });
    apply_truncation(&mut result, "results", 30, false, &store, "iris_search");
    assert_eq!(result["truncated"], json!(false));
    assert_eq!(result["results"].as_array().unwrap().len(), 20);
    assert_eq!(store.lock().unwrap().entries.len(), 0);
}

// ── T033: iris_info truncation — 40 docs → truncated ─────────────────────────

#[test]
fn test_info_truncation_above_threshold() {
    let store = make_store(50, 60);
    let docs: Vec<Value> = (0..40)
        .map(|i| json!(format!("MyApp.Class{}.cls", i)))
        .collect();
    let mut result = json!({
        "success": true,
        "what": "documents",
        "documents": docs,
    });
    apply_truncation(&mut result, "documents", 30, false, &store, "iris_info");
    assert_eq!(result["truncated"], json!(true));
    assert_eq!(result["documents"].as_array().unwrap().len(), 30);
    assert_eq!(result["total_count"], json!(40));
}

// ── T034: iris_info truncation — 20 docs → not truncated ─────────────────────

#[test]
fn test_info_truncation_below_threshold() {
    let store = make_store(50, 60);
    let docs: Vec<Value> = (0..20)
        .map(|i| json!(format!("MyApp.Class{}.cls", i)))
        .collect();
    let mut result = json!({"success": true, "documents": docs});
    apply_truncation(&mut result, "documents", 30, false, &store, "iris_info");
    assert_eq!(result["truncated"], json!(false));
    assert_eq!(result["documents"].as_array().unwrap().len(), 20);
}

// ── T035: debug_get_error_logs truncation — 25 entries → truncated ───────────

#[test]
fn test_error_logs_truncation_above_threshold() {
    let store = make_store(50, 60);
    let logs: Vec<Value> = (0..25)
        .map(|i| json!({"ErrorCode": format!("E{}", i), "ErrorText": "text"}))
        .collect();
    let mut result = json!({"success": true, "logs": logs});
    apply_truncation(
        &mut result,
        "logs",
        20,
        false,
        &store,
        "debug_get_error_logs",
    );
    assert_eq!(result["truncated"], json!(true));
    assert_eq!(result["logs"].as_array().unwrap().len(), 20);
    assert_eq!(result["total_count"], json!(25));
    assert!(result["log_id"].is_string());
}

// ── T044: iris_get_log list with no id ────────────────────────────────────────

#[test]
fn test_list_returns_summaries() {
    let mut s = LogStore::new(50, 60);
    s.store(make_entry("iris_compile", 30));
    s.store(make_entry("iris_search", 100));
    let summaries = s.list();
    assert_eq!(summaries.len(), 2);
    // Both tools present
    assert!(summaries.iter().any(|s| s.tool == "iris_compile"));
    assert!(summaries.iter().any(|s| s.tool == "iris_search"));
    // total_count matches
    let compile = summaries.iter().find(|s| s.tool == "iris_compile").unwrap();
    assert_eq!(compile.total_count, 30);
}

// ── T045: iris_get_log by id — found, not found, expired ──────────────────────

#[test]
fn test_get_by_id_found() {
    let mut s = LogStore::new(50, 60);
    let entry = make_entry("iris_compile", 10);
    let id = s.store(entry);
    match s.get(&id) {
        GetResult::Found(v) => assert!(v.is_array()),
        _ => panic!("expected Found"),
    }
}

#[test]
fn test_get_by_id_not_found() {
    let s = LogStore::new(50, 60);
    assert!(matches!(
        s.get("iris-0000000000000-badid123"),
        GetResult::NotFound
    ));
}

#[test]
fn test_get_by_id_expired() {
    // Create a store with ttl=0 so any entry is immediately expired
    let mut s = LogStore::new(50, 0);
    let entry = make_entry("iris_compile", 5);
    let id = s.store(entry);
    assert!(matches!(s.get(&id), GetResult::Expired));
    // Entry still in store after get (not evicted by get)
    assert_eq!(s.entries.len(), 1);
}

// ── T046: iris_get_log pagination ─────────────────────────────────────────────

#[test]
fn test_get_paginated_has_more_true() {
    let mut s = LogStore::new(50, 60);
    let items: Vec<Value> = (0..5).map(|i| json!(i)).collect();
    let entry = LogEntry {
        id: new_log_id(),
        tool: "iris_search".to_string(),
        created_at: Instant::now(),
        preview: items[..2].to_vec(),
        full_result: Value::Array(items),
        total_count: 5,
    };
    let id = s.store(entry);
    let result = s.get_paginated(&id, Some(2), 0);
    assert!(result.is_some());
    let (items, has_more, total) = result.unwrap();
    assert_eq!(items.as_array().unwrap().len(), 2);
    assert!(has_more, "has_more should be true when more items remain");
    assert_eq!(total, 5);
}

#[test]
fn test_get_paginated_last_page() {
    let mut s = LogStore::new(50, 60);
    let items: Vec<Value> = (0..5).map(|i| json!(i)).collect();
    let entry = LogEntry {
        id: new_log_id(),
        tool: "iris_search".to_string(),
        created_at: Instant::now(),
        preview: vec![],
        full_result: Value::Array(items),
        total_count: 5,
    };
    let id = s.store(entry);
    // Offset=4, limit=2 → only 1 item left → has_more false
    let (items_val, has_more, _total) = s.get_paginated(&id, Some(2), 4).unwrap();
    assert_eq!(items_val.as_array().unwrap().len(), 1);
    assert!(!has_more, "has_more should be false on last page");
}

// ── read_inline_threshold ─────────────────────────────────────────────────────

#[test]
fn test_read_inline_threshold_default() {
    let mut env_guard = EnvGuard::new();
    // Ensure a non-existent env var returns the default
    env_guard.unset("IRIS_INLINE_TEST_MISSING");
    assert_eq!(read_inline_threshold("IRIS_INLINE_TEST_MISSING", 20), 20);
}

#[test]
fn test_read_inline_threshold_from_env() {
    let mut env_guard = EnvGuard::new();
    env_guard.set("IRIS_INLINE_TEST_SET", "42");
    assert_eq!(read_inline_threshold("IRIS_INLINE_TEST_SET", 20), 42);
}

#[test]
fn test_read_inline_threshold_zero_returns_default() {
    let mut env_guard = EnvGuard::new();
    env_guard.set("IRIS_INLINE_TEST_ZERO", "0");
    assert_eq!(read_inline_threshold("IRIS_INLINE_TEST_ZERO", 20), 20);
}

// ── get_paginated edge cases ─────────────────────────────────────────────────

#[test]
fn test_get_paginated_expired_returns_none() {
    // TTL=0 → all entries immediately expired → get_paginated returns None
    let mut s = LogStore::new(10, 0);
    let entry = make_entry("iris_compile", 5);
    let id = s.store(entry);
    let result = s.get_paginated(&id, None, 0);
    assert!(
        result.is_none(),
        "get_paginated should return None for expired entry"
    );
}

#[test]
fn test_get_paginated_limit_none_returns_full_result() {
    let mut s = LogStore::new(10, 60);
    let entry = make_entry("iris_compile", 3);
    let id = s.store(entry);
    // limit=None → returns full result without slicing
    let (val, has_more, total) = s.get_paginated(&id, None, 0).unwrap();
    assert_eq!(val.as_array().unwrap().len(), 3);
    assert!(!has_more);
    assert_eq!(total, 3);
}

#[test]
fn test_get_paginated_non_array_result_with_limit() {
    // full_result is a JSON object (not array) → returns as-is even with limit=Some
    let mut s = LogStore::new(10, 60);
    let entry = LogEntry {
        id: new_log_id(),
        tool: "iris_info".to_string(),
        created_at: std::time::Instant::now(),
        preview: vec![],
        full_result: json!({"status": "ok", "items": 5}),
        total_count: 5,
    };
    let id = s.store(entry);
    let (val, has_more, _) = s.get_paginated(&id, Some(2), 0).unwrap();
    // Non-array with limit → returns full result
    assert!(
        val.get("status").is_some(),
        "should return full object: {val}"
    );
    assert!(!has_more);
}

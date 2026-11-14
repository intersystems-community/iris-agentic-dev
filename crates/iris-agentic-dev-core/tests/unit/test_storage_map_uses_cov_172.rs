//! Coverage tests for uses.rs uncovered lines.
//!
//! Note: The `uses()` function calls IRIS, so we only test pure logic paths here.
//! Lines that require a live IRIS connection are documented but not tested.

use iris_agentic_dev_core::tools::storage_map::uses::*;

#[test]
fn test_use_to_json() {
    let u = Use {
        namespace: "USER".to_string(),
        class: "Test.Class".to_string(),
        storage: "SQLStorage".to_string(),
        map: "Master".to_string(),
        r#type: "data".to_string(),
    };

    let json = use_to_json(&u);
    assert_eq!(json["namespace"], "USER");
    assert_eq!(json["class"], "Test.Class");
    assert_eq!(json["storage"], "SQLStorage");
    assert_eq!(json["map"], "Master");
    assert_eq!(json["type"], "data");
}

// Line 105-107: query_failed is a private function.
// It is tested through integration tests that call uses() with live IRIS.

// Lines 44, 47, 71, 80-86 are inside the `uses()` async function which calls IRIS.
// These are control flow paths (continue statements and loops) that execute based on
// IRIS query results. They cannot be tested without a live IRIS connection, which
// violates the "never mock IRIS" policy.
//
// Specifically:
// - Line 44: `continue` when parent/Name/Type/_Global is None from IRIS query result
// - Line 47: `continue` when !same_global() check fails
// - Line 71: `continue` when parent is None in default storage loop
// - Lines 80-86: Loop iterations over location types and same_global checks
//
// These lines are exercised by integration tests that use a live IRIS container.

//! T076 (FR-030c): each map write leaves an audit record of what changed, never the code.

use iris_agentic_dev_core::tools::storage_map::write::map_write_record;
use serde_json::json;

#[test]
fn test_storage_map_audit_172_record_has_the_contracted_fields_and_no_values() {
    let changed = vec![
        "/maps/Master/data/Age/retrieval_code".to_string(),
        "/maps/Master/data/Age/piece".to_string(),
    ];
    let r = map_write_record(
        "IadSm172.Member",
        Some("Master"),
        "aaa1",
        "bbb2",
        &changed,
        true,
    );
    assert_eq!(
        r,
        json!({
            "class": "IadSm172.Member",
            "map": "Master",
            "version_before": "aaa1",
            "version_after": "bbb2",
            "changed_paths": changed,
            "code_changed": true,
        })
    );
    // A write across maps or storage keywords names no map.
    let r = map_write_record("X.Y", None, "a", "b", &[], false);
    assert!(r["map"].is_null(), "{r}");
    assert_eq!(r.as_object().unwrap().len(), 6, "{r}");
}

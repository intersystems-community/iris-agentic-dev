//! Unit tests for public functions in dict.rs
//! Focuses on cache management, confidence scoring, and parameter deserialization.

#[cfg(test)]
mod tests {
    use iris_agentic_dev_core::tools::dict::{
        confidence_for_count, metadata_cache_get, metadata_cache_set, ExtractMessageMapParams,
        FindSubclassImplementationsParams, ResolveDynamicDispatchParams,
    };
    use std::collections::HashMap;
    use std::sync::Mutex;

    // ── Cache basic operations ────────────────────────────────────────────────

    #[test]
    fn test_cache_set_and_get() {
        let cache: Mutex<HashMap<String, (serde_json::Value, std::time::Instant)>> =
            Mutex::new(HashMap::new());
        let val = serde_json::json!({"x": 42});
        metadata_cache_set(&cache, "key1".into(), val.clone());
        assert_eq!(metadata_cache_get(&cache, "key1"), Some(val));
    }

    #[test]
    fn test_cache_miss_returns_none() {
        let cache: Mutex<HashMap<String, (serde_json::Value, std::time::Instant)>> =
            Mutex::new(HashMap::new());
        assert_eq!(metadata_cache_get(&cache, "nonexistent"), None);
    }

    #[test]
    fn test_cache_overwrite() {
        let cache: Mutex<HashMap<String, (serde_json::Value, std::time::Instant)>> =
            Mutex::new(HashMap::new());
        metadata_cache_set(&cache, "k".into(), serde_json::json!(1));
        metadata_cache_set(&cache, "k".into(), serde_json::json!(2));
        assert_eq!(metadata_cache_get(&cache, "k"), Some(serde_json::json!(2)));
    }

    #[test]
    fn test_cache_multiple_keys() {
        let cache: Mutex<HashMap<String, (serde_json::Value, std::time::Instant)>> =
            Mutex::new(HashMap::new());
        metadata_cache_set(&cache, "k1".into(), serde_json::json!({"a": 1}));
        metadata_cache_set(&cache, "k2".into(), serde_json::json!({"b": 2}));
        metadata_cache_set(&cache, "k3".into(), serde_json::json!({"c": 3}));

        assert_eq!(
            metadata_cache_get(&cache, "k1"),
            Some(serde_json::json!({"a": 1}))
        );
        assert_eq!(
            metadata_cache_get(&cache, "k2"),
            Some(serde_json::json!({"b": 2}))
        );
        assert_eq!(
            metadata_cache_get(&cache, "k3"),
            Some(serde_json::json!({"c": 3}))
        );
    }

    #[test]
    fn test_cache_ttl_expiration() {
        let cache: Mutex<HashMap<String, (serde_json::Value, std::time::Instant)>> =
            Mutex::new(HashMap::new());
        let val = serde_json::json!({"expired": true});
        // Manually insert with expired timestamp (>60s old)
        cache.lock().unwrap().insert(
            "old".into(),
            (
                val,
                std::time::Instant::now() - std::time::Duration::from_secs(120),
            ),
        );
        assert_eq!(metadata_cache_get(&cache, "old"), None);
    }

    #[test]
    fn test_cache_recent_entry_not_expired() {
        let cache: Mutex<HashMap<String, (serde_json::Value, std::time::Instant)>> =
            Mutex::new(HashMap::new());
        let val = serde_json::json!({"fresh": true});
        metadata_cache_set(&cache, "fresh".into(), val.clone());
        assert_eq!(metadata_cache_get(&cache, "fresh"), Some(val));
    }

    #[test]
    fn test_cache_empty_value() {
        let cache: Mutex<HashMap<String, (serde_json::Value, std::time::Instant)>> =
            Mutex::new(HashMap::new());
        let empty = serde_json::json!({});
        metadata_cache_set(&cache, "empty".into(), empty.clone());
        assert_eq!(metadata_cache_get(&cache, "empty"), Some(empty));
    }

    #[test]
    fn test_cache_null_value() {
        let cache: Mutex<HashMap<String, (serde_json::Value, std::time::Instant)>> =
            Mutex::new(HashMap::new());
        let null_val = serde_json::json!(null);
        metadata_cache_set(&cache, "null".into(), null_val.clone());
        assert_eq!(metadata_cache_get(&cache, "null"), Some(null_val));
    }

    // ── confidence_for_count scoring ──────────────────────────────────────────

    #[test]
    fn test_confidence_no_candidates() {
        assert_eq!(confidence_for_count(0), 0.0);
    }

    #[test]
    fn test_confidence_single_candidate() {
        assert_eq!(confidence_for_count(1), 0.90);
    }

    #[test]
    fn test_confidence_few_candidates() {
        assert_eq!(confidence_for_count(2), 0.75);
        assert_eq!(confidence_for_count(3), 0.75);
        assert_eq!(confidence_for_count(5), 0.75);
    }

    #[test]
    fn test_confidence_moderate_candidates() {
        assert_eq!(confidence_for_count(6), 0.55);
        assert_eq!(confidence_for_count(10), 0.55);
        assert_eq!(confidence_for_count(20), 0.55);
    }

    #[test]
    fn test_confidence_many_candidates() {
        assert_eq!(confidence_for_count(21), 0.30);
        assert_eq!(confidence_for_count(100), 0.30);
        assert_eq!(confidence_for_count(1000), 0.30);
    }

    #[test]
    fn test_confidence_boundary_1_vs_2() {
        assert_eq!(confidence_for_count(1), 0.90);
        assert_eq!(confidence_for_count(2), 0.75);
        assert_ne!(confidence_for_count(1), confidence_for_count(2));
    }

    #[test]
    fn test_confidence_boundary_5_vs_6() {
        assert_eq!(confidence_for_count(5), 0.75);
        assert_eq!(confidence_for_count(6), 0.55);
        assert_ne!(confidence_for_count(5), confidence_for_count(6));
    }

    #[test]
    fn test_confidence_boundary_20_vs_21() {
        assert_eq!(confidence_for_count(20), 0.55);
        assert_eq!(confidence_for_count(21), 0.30);
        assert_ne!(confidence_for_count(20), confidence_for_count(21));
    }

    // ── Parameter struct defaults ─────────────────────────────────────────────

    #[test]
    fn test_resolve_params_with_defaults() {
        let p: ResolveDynamicDispatchParams =
            serde_json::from_str(r#"{"method_name": "Connect"}"#).unwrap();
        assert_eq!(p.method_name, "Connect");
        assert_eq!(p.namespace, "USER");
        assert!(p.package_prefix.is_none());
        assert!(p.limit.is_none());
    }

    #[test]
    fn test_resolve_params_with_prefix() {
        let p: ResolveDynamicDispatchParams =
            serde_json::from_str(r#"{"method_name": "Connect", "package_prefix": "EnsLib"}"#)
                .unwrap();
        assert_eq!(p.method_name, "Connect");
        assert_eq!(p.package_prefix.as_deref(), Some("EnsLib"));
    }

    #[test]
    fn test_resolve_params_with_limit() {
        let p: ResolveDynamicDispatchParams =
            serde_json::from_str(r#"{"method_name": "Connect", "limit": 25}"#).unwrap();
        assert_eq!(p.limit, Some(25));
    }

    #[test]
    fn test_resolve_params_custom_namespace() {
        let p: ResolveDynamicDispatchParams =
            serde_json::from_str(r#"{"method_name": "Connect", "namespace": "MYNS"}"#).unwrap();
        assert_eq!(p.namespace, "MYNS");
    }

    #[test]
    fn test_extract_message_map_params_with_defaults() {
        let p: ExtractMessageMapParams =
            serde_json::from_str(r#"{"class_name": "HS.Flash.Router"}"#).unwrap();
        assert_eq!(p.class_name, "HS.Flash.Router");
        assert_eq!(p.namespace, "USER");
    }

    #[test]
    fn test_extract_message_map_params_custom_namespace() {
        let p: ExtractMessageMapParams =
            serde_json::from_str(r#"{"class_name": "HS.Flash.Router", "namespace": "HSLIB"}"#)
                .unwrap();
        assert_eq!(p.namespace, "HSLIB");
    }

    #[test]
    fn test_find_subclass_params_with_defaults() {
        let p: FindSubclassImplementationsParams = serde_json::from_str(
            r#"{"method_name": "OnProcessInput", "base_classes": ["Ens.BusinessProcess"]}"#,
        )
        .unwrap();
        assert_eq!(p.method_name, "OnProcessInput");
        assert_eq!(p.namespace, "USER");
        assert!(p.limit.is_none());
        assert_eq!(p.base_classes.len(), 1);
    }

    #[test]
    fn test_find_subclass_params_multiple_bases() {
        let p: FindSubclassImplementationsParams = serde_json::from_str(
            r#"{"method_name": "OnProcessInput", "base_classes": ["Ens.BusinessProcess", "Ens.BusinessOperation"]}"#,
        )
        .unwrap();
        assert_eq!(p.base_classes.len(), 2);
        assert_eq!(p.base_classes[0], "Ens.BusinessProcess");
        assert_eq!(p.base_classes[1], "Ens.BusinessOperation");
    }

    #[test]
    fn test_find_subclass_params_custom_limit() {
        let p: FindSubclassImplementationsParams = serde_json::from_str(
            r#"{"method_name": "Execute", "base_classes": ["Ens.Adapter"], "limit": 50}"#,
        )
        .unwrap();
        assert_eq!(p.limit, Some(50));
    }

    // ── Confidence scoring edge cases ─────────────────────────────────────────

    #[test]
    fn test_confidence_formula_all_ranges() {
        // Verify all ranges are covered
        assert_eq!(confidence_for_count(0), 0.0, "0 candidates");
        assert_eq!(confidence_for_count(1), 0.90, "1 candidate");
        assert_eq!(confidence_for_count(4), 0.75, "4 candidates (2-5)");
        assert_eq!(confidence_for_count(15), 0.55, "15 candidates (6-20)");
        assert_eq!(confidence_for_count(50), 0.30, ">20 candidates");
    }

    #[test]
    fn test_confidence_formula_large_numbers() {
        assert_eq!(confidence_for_count(100), 0.30);
        assert_eq!(confidence_for_count(1000), 0.30);
        assert_eq!(confidence_for_count(10000), 0.30);
    }

    #[test]
    fn test_confidence_formula_decimal_values() {
        // Verify confidence is in valid range
        let conf = confidence_for_count(3);
        assert!((0.0..=1.0).contains(&conf), "confidence should be 0-1");
        assert_eq!(conf, 0.75);
    }

    // ── Parameter JSON deserialization ────────────────────────────────────────

    #[test]
    fn test_resolve_params_all_fields() {
        let p: ResolveDynamicDispatchParams = serde_json::from_str(
            r#"{"method_name": "Execute", "package_prefix": "HS", "namespace": "HSLIB", "limit": 75}"#,
        )
        .unwrap();
        assert_eq!(p.method_name, "Execute");
        assert_eq!(p.package_prefix.as_deref(), Some("HS"));
        assert_eq!(p.namespace, "HSLIB");
        assert_eq!(p.limit, Some(75));
    }

    #[test]
    fn test_extract_message_map_params_all_fields() {
        let p: ExtractMessageMapParams =
            serde_json::from_str(r#"{"class_name": "MyApp.Router", "namespace": "PROD"}"#).unwrap();
        assert_eq!(p.class_name, "MyApp.Router");
        assert_eq!(p.namespace, "PROD");
    }

    #[test]
    fn test_find_subclass_params_all_fields() {
        let p: FindSubclassImplementationsParams = serde_json::from_str(
            r#"{"method_name": "OnProcessInput", "base_classes": ["Ens.BusinessProcess"], "namespace": "ENSLIB", "limit": 100}"#,
        )
        .unwrap();
        assert_eq!(p.method_name, "OnProcessInput");
        assert_eq!(p.namespace, "ENSLIB");
        assert_eq!(p.limit, Some(100));
    }

    // ── Cache TTL edge case ───────────────────────────────────────────────────

    #[test]
    fn test_cache_exactly_at_ttl_boundary() {
        let cache: Mutex<HashMap<String, (serde_json::Value, std::time::Instant)>> =
            Mutex::new(HashMap::new());
        let val = serde_json::json!({"test": true});
        // Insert at exactly TTL (60s)
        cache.lock().unwrap().insert(
            "boundary".into(),
            (
                val,
                std::time::Instant::now() - std::time::Duration::from_secs(60),
            ),
        );
        // At exactly 60s, it should be considered expired (< METADATA_CACHE_TTL is false)
        assert_eq!(metadata_cache_get(&cache, "boundary"), None);
    }

    #[test]
    fn test_cache_just_before_ttl_boundary() {
        let cache: Mutex<HashMap<String, (serde_json::Value, std::time::Instant)>> =
            Mutex::new(HashMap::new());
        let val = serde_json::json!({"test": true});
        // Insert at 59s ago (before TTL)
        cache.lock().unwrap().insert(
            "recent".into(),
            (
                val.clone(),
                std::time::Instant::now() - std::time::Duration::from_secs(59),
            ),
        );
        // Should still be valid
        assert_eq!(metadata_cache_get(&cache, "recent"), Some(val));
    }

    // ── JSON parsing validation ───────────────────────────────────────────────

    #[test]
    fn test_json_parse_missing_method_name_fails() {
        let r: Result<ResolveDynamicDispatchParams, _> =
            serde_json::from_str(r#"{"namespace": "USER"}"#);
        assert!(r.is_err());
    }

    #[test]
    fn test_json_parse_missing_class_name_fails() {
        let r: Result<ExtractMessageMapParams, _> =
            serde_json::from_str(r#"{"namespace": "USER"}"#);
        assert!(r.is_err());
    }

    #[test]
    fn test_json_parse_missing_base_classes_fails() {
        let r: Result<FindSubclassImplementationsParams, _> =
            serde_json::from_str(r#"{"method_name": "Execute"}"#);
        assert!(r.is_err());
    }

    #[test]
    fn test_json_parse_empty_base_classes_allowed() {
        let r: Result<FindSubclassImplementationsParams, _> =
            serde_json::from_str(r#"{"method_name": "Execute", "base_classes": []}"#);
        // Empty base_classes should deserialize successfully
        assert!(r.is_ok());
        assert_eq!(r.unwrap().base_classes.len(), 0);
    }

    // ── Cache complex value types ─────────────────────────────────────────────

    #[test]
    fn test_cache_array_value() {
        let cache: Mutex<HashMap<String, (serde_json::Value, std::time::Instant)>> =
            Mutex::new(HashMap::new());
        let arr = serde_json::json!([1, 2, 3]);
        metadata_cache_set(&cache, "arr".into(), arr.clone());
        assert_eq!(metadata_cache_get(&cache, "arr"), Some(arr));
    }

    #[test]
    fn test_cache_nested_object() {
        let cache: Mutex<HashMap<String, (serde_json::Value, std::time::Instant)>> =
            Mutex::new(HashMap::new());
        let nested = serde_json::json!({
            "outer": {
                "inner": {
                    "value": 42
                }
            }
        });
        metadata_cache_set(&cache, "nested".into(), nested.clone());
        assert_eq!(metadata_cache_get(&cache, "nested"), Some(nested));
    }

    #[test]
    fn test_cache_string_value() {
        let cache: Mutex<HashMap<String, (serde_json::Value, std::time::Instant)>> =
            Mutex::new(HashMap::new());
        let s = serde_json::json!("test string");
        metadata_cache_set(&cache, "str".into(), s.clone());
        assert_eq!(metadata_cache_get(&cache, "str"), Some(s));
    }

    // ── Namespace defaults verification ───────────────────────────────────────

    #[test]
    fn test_all_params_have_user_default() {
        let r: ResolveDynamicDispatchParams =
            serde_json::from_str(r#"{"method_name": "M"}"#).unwrap();
        assert_eq!(r.namespace, "USER");

        let e: ExtractMessageMapParams = serde_json::from_str(r#"{"class_name": "C"}"#).unwrap();
        assert_eq!(e.namespace, "USER");

        let f: FindSubclassImplementationsParams =
            serde_json::from_str(r#"{"method_name": "M", "base_classes": ["B"]}"#).unwrap();
        assert_eq!(f.namespace, "USER");
    }
}

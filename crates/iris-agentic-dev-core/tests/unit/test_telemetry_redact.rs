//! Unit tests for parameter redaction (T012). No live IRIS required.

use iris_agentic_dev_core::iris::workspace_config::DataPolicy;
use iris_agentic_dev_core::telemetry::redact::redact_params;

#[test]
fn allow_policy_preserves_params() {
    let params = serde_json::json!({"query": "SELECT 1"});
    assert_eq!(redact_params(&params, &DataPolicy::Allow), Some(params));
}

#[test]
fn block_policy_redacts_params() {
    let params = serde_json::json!({"query": "SELECT 1"});
    assert_eq!(redact_params(&params, &DataPolicy::Block), None);
}

#[test]
fn redact_policy_redacts_params() {
    let params = serde_json::json!({"query": "SELECT 1"});
    assert_eq!(redact_params(&params, &DataPolicy::Redact), None);
}

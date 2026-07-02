//! Parameter redaction for telemetry records, reusing spec-051's `DataPolicy` model.
//! No new redaction mechanism — see specs/059-tool-telemetry-benchmark/research.md.

use crate::iris::workspace_config::DataPolicy;

/// Returns `params` unchanged only when `policy == DataPolicy::Allow`; otherwise `None`,
/// per FR-003 ("parameters MUST be redacted while tool name/outcome/duration are still
/// recorded").
pub fn redact_params(params: &serde_json::Value, policy: &DataPolicy) -> Option<serde_json::Value> {
    match policy {
        DataPolicy::Allow => Some(params.clone()),
        DataPolicy::Block | DataPolicy::Redact => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn allow_preserves_params() {
        let params = serde_json::json!({"query": "SELECT 1"});
        let result = redact_params(&params, &DataPolicy::Allow);
        assert_eq!(result, Some(params));
    }

    #[test]
    fn block_redacts_params() {
        let params = serde_json::json!({"query": "SELECT 1"});
        assert_eq!(redact_params(&params, &DataPolicy::Block), None);
    }

    #[test]
    fn redact_policy_redacts_params() {
        let params = serde_json::json!({"query": "SELECT 1"});
        assert_eq!(redact_params(&params, &DataPolicy::Redact), None);
    }
}

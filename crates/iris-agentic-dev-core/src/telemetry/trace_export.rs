//! Aggregates `ToolCallRecord`s into `{from, to, via, count, ts}` dispatch-trace records
//! matching 058-iris-graph's `record_trace` ingestion contract (verified in
//! specs/059-tool-telemetry-benchmark/research.md — no field-name translation permitted).

use super::ToolCallRecord;
use std::collections::HashMap;

/// The literal value used for `via` on every record this feature exports, distinguishing
/// tool-telemetry-sourced edges from other `record_trace` sources (e.g. Pierre's own
/// dispatch tracer uses `"direct"`/`"dispatch"`/`"workmgr"`).
pub const VIA: &str = "mcp";

/// Sentinel `from` value for a top-level tool invocation with no calling tool context —
/// this feature only has tool-level granularity, not method-level dispatch data.
pub const NO_CALLER_SENTINEL: &str = "mcp_client";

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct DispatchTraceRecord {
    pub from: String,
    pub to: String,
    pub via: String,
    pub count: u64,
    pub ts: String,
}

/// Groups records by `(from, to, via)` — `from` is always `NO_CALLER_SENTINEL` and `via`
/// is always `VIA` for this feature's records (tool-level granularity, no call-chain
/// tracking) — emitting one aggregated record per group with `count` = occurrences and
/// `ts` = the most recent timestamp in the group, per FR-009's aggregation rule.
pub fn aggregate_trace(records: &[ToolCallRecord]) -> Vec<DispatchTraceRecord> {
    let mut groups: HashMap<String, (u64, String)> = HashMap::new();
    for r in records {
        let entry = groups
            .entry(r.tool.clone())
            .or_insert((0, r.timestamp.clone()));
        entry.0 += 1;
        if r.timestamp > entry.1 {
            entry.1 = r.timestamp.clone();
        }
    }
    let mut out: Vec<DispatchTraceRecord> = groups
        .into_iter()
        .map(|(tool, (count, ts))| DispatchTraceRecord {
            from: NO_CALLER_SENTINEL.to_string(),
            to: tool,
            via: VIA.to_string(),
            count,
            ts,
        })
        .collect();
    out.sort_by(|a, b| a.to.cmp(&b.to));
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    fn record(tool: &str, ts: &str) -> ToolCallRecord {
        ToolCallRecord {
            tool: tool.to_string(),
            success: true,
            duration_ms: 1,
            timestamp: ts.to_string(),
            session_id: Uuid::new_v4(),
            params: None,
        }
    }

    #[test]
    fn repeated_identical_calls_aggregate_into_one_record_with_incremented_count() {
        let records = vec![
            record("iris_compile", "2026-07-01T10:00:00Z"),
            record("iris_compile", "2026-07-01T10:00:05Z"),
            record("iris_compile", "2026-07-01T10:00:10Z"),
        ];
        let out = aggregate_trace(&records);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].count, 3);
        assert_eq!(out[0].ts, "2026-07-01T10:00:10Z");
    }

    #[test]
    fn varied_calls_produce_one_record_per_distinct_combination() {
        let records = vec![
            record("iris_compile", "2026-07-01T10:00:00Z"),
            record("iris_execute", "2026-07-01T10:00:01Z"),
            record("iris_test", "2026-07-01T10:00:02Z"),
        ];
        let out = aggregate_trace(&records);
        assert_eq!(out.len(), 3);
    }

    #[test]
    fn output_records_have_exactly_the_required_fields() {
        let records = vec![record("iris_compile", "2026-07-01T10:00:00Z")];
        let out = aggregate_trace(&records);
        let json = serde_json::to_value(&out[0]).unwrap();
        let obj = json.as_object().unwrap();
        let mut keys: Vec<&String> = obj.keys().collect();
        keys.sort();
        assert_eq!(keys, vec!["count", "from", "to", "ts", "via"]);
    }

    #[test]
    fn from_and_via_use_fixed_literals() {
        let records = vec![record("iris_compile", "2026-07-01T10:00:00Z")];
        let out = aggregate_trace(&records);
        assert_eq!(out[0].from, NO_CALLER_SENTINEL);
        assert_eq!(out[0].via, VIA);
        assert_eq!(out[0].to, "iris_compile");
    }
}

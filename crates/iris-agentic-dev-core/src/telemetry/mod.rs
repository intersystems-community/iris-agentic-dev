//! Tool-call telemetry: in-memory ring buffer (authoritative for `agent_history`/
//! `agent_stats`) plus a best-effort dual-sink durable write (IRIS global when
//! connected, local JSONL file when not). See specs/059-tool-telemetry-benchmark/.

pub mod prune;
pub mod redact;
pub mod trace_export;

use crate::iris::connection::IrisConnection;
use std::path::Path;
use std::sync::Arc;
use uuid::Uuid;

/// One MCP server process lifetime. `id` is generated once and stays fixed until exit.
#[derive(Debug, Clone)]
pub struct Session {
    pub id: Uuid,
}

impl Session {
    pub fn new() -> Self {
        Self { id: Uuid::new_v4() }
    }
}

impl Default for Session {
    fn default() -> Self {
        Self::new()
    }
}

/// A single durable telemetry entry. Supersedes the old `ToolCallEntry` (which had no
/// `duration_ms`/`session_id`/`params` fields).
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ToolCallRecord {
    pub tool: String,
    pub success: bool,
    pub duration_ms: u64,
    /// RFC3339 wall-clock timestamp. Captured via `SystemTime` alongside the `Instant`
    /// used for duration, since `Instant` has no portable wall-clock representation.
    pub timestamp: String,
    pub session_id: Uuid,
    /// `None` when redacted per the active `DataPolicy` (see `redact.rs`), or when the
    /// caller simply didn't capture parameters for this call.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub params: Option<serde_json::Value>,
}

impl ToolCallRecord {
    pub fn now(tool: &str, success: bool, duration_ms: u64, session_id: Uuid) -> Self {
        Self {
            tool: tool.to_string(),
            success,
            duration_ms,
            timestamp: now_rfc3339(),
            session_id,
            params: None,
        }
    }
}

/// Current wall-clock time as an RFC3339 string, with no `chrono`-timezone-DB dependency
/// beyond what's already a workspace dependency (`chrono` is already used elsewhere in
/// this crate, e.g. `tools/log_store.rs`).
pub fn now_rfc3339() -> String {
    chrono::Utc::now().to_rfc3339()
}

/// Seconds elapsed since an RFC3339 timestamp was recorded, for `agent_history`'s
/// `ago_secs` display field. Returns 0 for unparseable/future timestamps rather than
/// erroring — this is a display convenience, not a correctness-critical path.
pub fn ago_secs(timestamp: &str) -> u64 {
    match chrono::DateTime::parse_from_rfc3339(timestamp) {
        Ok(ts) => {
            let now = chrono::Utc::now();
            let diff = now.signed_duration_since(ts.with_timezone(&chrono::Utc));
            diff.num_seconds().max(0) as u64
        }
        Err(_) => 0,
    }
}

/// Encode a `ToolCallRecord` as a `$LISTBUILD`-compatible ObjectScript literal fragment,
/// e.g. `$LISTBUILD("iris_compile",1,123,"2026-07-01T20:31:28Z")` (5th element omitted
/// when `params` is `None`, to keep the common case's record small — verified live in
/// research.md).
fn encode_listbuild(record: &ToolCallRecord) -> String {
    let tool = objectscript_string_literal(&record.tool);
    let success = if record.success { 1 } else { 0 };
    let timestamp = objectscript_string_literal(&record.timestamp);
    match &record.params {
        Some(p) => {
            let params_json = objectscript_string_literal(&p.to_string());
            format!(
                "$LISTBUILD({tool},{success},{},{timestamp},{params_json})",
                record.duration_ms
            )
        }
        None => format!(
            "$LISTBUILD({tool},{success},{},{timestamp})",
            record.duration_ms
        ),
    }
}

/// Escape a Rust string as an ObjectScript double-quoted string literal (doubling
/// embedded quotes — ObjectScript's only escape mechanism inside `"..."`).
fn objectscript_string_literal(s: &str) -> String {
    format!("\"{}\"", s.replace('"', "\"\""))
}

/// Decode a `$LISTTOSTRING(..., "|")`-piped line (see `read_durable`'s IRIS-side query)
/// back into a `ToolCallRecord`. Fields: tool|success|duration_ms|timestamp[|params_json].
/// Returns `None` for malformed lines rather than panicking — durable-sink corruption
/// must never crash a read.
fn decode_piped(line: &str, session_id: Uuid) -> Option<ToolCallRecord> {
    let mut parts = line.splitn(5, '|');
    let tool = parts.next()?.to_string();
    let success = parts.next()? == "1";
    let duration_ms = parts.next()?.parse().ok()?;
    let timestamp = parts.next()?.to_string();
    let params = parts
        .next()
        .filter(|s| !s.is_empty())
        .and_then(|s| serde_json::from_str(s).ok());
    Some(ToolCallRecord {
        tool,
        success,
        duration_ms,
        timestamp,
        session_id,
        params,
    })
}

fn jsonl_path(config_dir: &Path, session_id: Uuid) -> std::path::PathBuf {
    config_dir
        .join("telemetry")
        .join(format!("{session_id}.jsonl"))
}

/// Best-effort durable write. MUST NOT propagate errors to the caller (FR-014) — every
/// failure path here is swallowed, logged at `tracing::debug`, and returns.
pub async fn write_durable(
    record: &ToolCallRecord,
    iris: Option<Arc<IrisConnection>>,
    client: &reqwest::Client,
    config_dir: &Path,
) {
    match iris {
        Some(iris) => {
            let listbuild = encode_listbuild(record);
            let code = format!(
                "set seq=$INCREMENT(^IRISDEV(\"telemetry\",\"{sid}\"))\nset ^IRISDEV(\"telemetry\",\"{sid}\",seq)={lb}\n",
                sid = record.session_id,
                lb = listbuild
            );
            if let Err(e) = iris.execute_via_generator(&code, "USER", client).await {
                tracing::debug!("telemetry durable write (IRIS) failed, dropping: {e}");
            }
        }
        None => {
            if let Err(e) = write_jsonl(record, config_dir) {
                tracing::debug!("telemetry durable write (local file) failed, dropping: {e}");
            }
        }
    }
}

fn write_jsonl(record: &ToolCallRecord, config_dir: &Path) -> anyhow::Result<()> {
    let path = jsonl_path(config_dir, record.session_id);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let line = serde_json::to_string(record)?;
    use std::io::Write;
    let mut f = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)?;
    writeln!(f, "{line}")?;
    Ok(())
}

/// Read back durable records for a session (or all known sessions when `session_id` is
/// `None`) from whichever sink is active.
pub async fn read_durable(
    session_id: Option<Uuid>,
    iris: Option<Arc<IrisConnection>>,
    client: &reqwest::Client,
    config_dir: &Path,
) -> Vec<ToolCallRecord> {
    match iris {
        Some(iris) => read_durable_iris(session_id, &iris, client).await,
        None => read_durable_local(session_id, config_dir),
    }
}

async fn read_durable_iris(
    session_id: Option<Uuid>,
    iris: &IrisConnection,
    client: &reqwest::Client,
) -> Vec<ToolCallRecord> {
    let sessions: Vec<Uuid> = match session_id {
        Some(id) => vec![id],
        None => match list_iris_sessions(iris, client).await {
            Ok(ids) => ids,
            Err(e) => {
                tracing::debug!("telemetry session enumeration failed: {e}");
                return Vec::new();
            }
        },
    };

    let mut out = Vec::new();
    for sid in sessions {
        let code = format!(
            "set sid=\"{sid}\"\nset seq=\"\"\nfor {{\n set seq=$ORDER(^IRISDEV(\"telemetry\",sid,seq))\n quit:seq=\"\"\n set rec=^IRISDEV(\"telemetry\",sid,seq)\n write $LISTTOSTRING(rec,\"|\"),!\n}}\n"
        );
        match iris.execute_via_generator(&code, "USER", client).await {
            Ok(output) => {
                for line in output.lines() {
                    if let Some(rec) = decode_piped(line, sid) {
                        out.push(rec);
                    }
                }
            }
            Err(e) => tracing::debug!("telemetry durable read (IRIS) failed for {sid}: {e}"),
        }
    }
    out
}

async fn list_iris_sessions(
    iris: &IrisConnection,
    client: &reqwest::Client,
) -> anyhow::Result<Vec<Uuid>> {
    let code = "set sid=\"\"\nfor {\n set sid=$ORDER(^IRISDEV(\"telemetry\",sid))\n quit:sid=\"\"\n write sid,!\n}\n";
    let output = iris.execute_via_generator(code, "USER", client).await?;
    Ok(output
        .lines()
        .filter_map(|l| Uuid::parse_str(l.trim()).ok())
        .collect())
}

fn read_durable_local(session_id: Option<Uuid>, config_dir: &Path) -> Vec<ToolCallRecord> {
    let dir = config_dir.join("telemetry");
    let files: Vec<std::path::PathBuf> = match session_id {
        Some(id) => vec![jsonl_path(config_dir, id)],
        None => std::fs::read_dir(&dir)
            .map(|rd| {
                rd.filter_map(|e| e.ok())
                    .map(|e| e.path())
                    .filter(|p| p.extension().is_some_and(|e| e == "jsonl"))
                    .collect()
            })
            .unwrap_or_default(),
    };

    let mut out = Vec::new();
    for path in files {
        if let Ok(contents) = std::fs::read_to_string(&path) {
            for line in contents.lines() {
                if let Ok(rec) = serde_json::from_str::<ToolCallRecord>(line) {
                    out.push(rec);
                }
            }
        }
    }
    out
}

/// Pure filter over already-loaded records — by tool name, session id, time range, with
/// a `limit` and a `truncated` flag when more matches existed than `limit` allowed.
pub fn filter_records(
    records: &[ToolCallRecord],
    tool_name: Option<&str>,
    session_id: Option<Uuid>,
    since: Option<&str>,
    until: Option<&str>,
    limit: usize,
) -> (Vec<ToolCallRecord>, bool) {
    let matches: Vec<&ToolCallRecord> = records
        .iter()
        .filter(|r| tool_name.is_none_or(|t| r.tool == t))
        .filter(|r| session_id.is_none_or(|s| r.session_id == s))
        .filter(|r| since.is_none_or(|s| r.timestamp.as_str() >= s))
        .filter(|r| until.is_none_or(|u| r.timestamp.as_str() <= u))
        .collect();
    let truncated = matches.len() > limit;
    let out = matches.into_iter().take(limit).cloned().collect();
    (out, truncated)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn session_ids_are_distinct_and_non_nil() {
        let a = Session::new();
        let b = Session::new();
        assert_ne!(a.id, b.id);
        assert_ne!(a.id, Uuid::nil());
    }

    #[test]
    fn tool_call_record_round_trips_via_serde_json() {
        let sid = Uuid::new_v4();
        let record = ToolCallRecord::now("iris_compile", true, 42, sid);
        let json = serde_json::to_string(&record).unwrap();
        let back: ToolCallRecord = serde_json::from_str(&json).unwrap();
        assert_eq!(back.tool, "iris_compile");
        assert!(back.success);
        assert_eq!(back.duration_ms, 42);
        assert_eq!(back.session_id, sid);
        assert!(back.params.is_none());
    }

    #[test]
    fn listbuild_encode_decode_round_trip_without_params() {
        let sid = Uuid::new_v4();
        let record = ToolCallRecord::now("iris_execute", false, 7, sid);
        let encoded = encode_listbuild(&record);
        assert!(encoded.starts_with("$LISTBUILD("));
        // Simulate the pipe-joined line IRIS would produce via $LISTTOSTRING(rec,"|").
        let piped = format!(
            "{}|{}|{}|{}",
            record.tool,
            if record.success { 1 } else { 0 },
            record.duration_ms,
            record.timestamp
        );
        let decoded = decode_piped(&piped, sid).unwrap();
        assert_eq!(decoded.tool, "iris_execute");
        assert!(!decoded.success);
        assert_eq!(decoded.duration_ms, 7);
        assert!(decoded.params.is_none());
    }

    #[test]
    fn listbuild_encode_decode_round_trip_with_params() {
        let sid = Uuid::new_v4();
        let mut record = ToolCallRecord::now("iris_query", true, 15, sid);
        record.params = Some(serde_json::json!({"query": "SELECT 1"}));
        let piped = format!(
            "{}|{}|{}|{}|{}",
            record.tool,
            1,
            record.duration_ms,
            record.timestamp,
            record.params.as_ref().unwrap()
        );
        let decoded = decode_piped(&piped, sid).unwrap();
        assert_eq!(decoded.params, record.params);
    }

    #[test]
    fn decode_piped_returns_none_for_malformed_line() {
        assert!(decode_piped("not|enough", Uuid::new_v4()).is_none());
    }

    #[test]
    fn filter_records_by_tool_name() {
        let sid = Uuid::new_v4();
        let records = vec![
            ToolCallRecord::now("iris_compile", true, 1, sid),
            ToolCallRecord::now("iris_execute", true, 1, sid),
        ];
        let (out, truncated) = filter_records(&records, Some("iris_compile"), None, None, None, 10);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].tool, "iris_compile");
        assert!(!truncated);
    }

    #[test]
    fn filter_records_by_session_id() {
        let sid_a = Uuid::new_v4();
        let sid_b = Uuid::new_v4();
        let records = vec![
            ToolCallRecord::now("iris_compile", true, 1, sid_a),
            ToolCallRecord::now("iris_compile", true, 1, sid_b),
        ];
        let (out, _) = filter_records(&records, None, Some(sid_a), None, None, 10);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].session_id, sid_a);
    }

    #[test]
    fn filter_records_by_time_range() {
        let sid = Uuid::new_v4();
        let mut early = ToolCallRecord::now("a", true, 1, sid);
        early.timestamp = "2026-01-01T00:00:00Z".to_string();
        let mut late = ToolCallRecord::now("b", true, 1, sid);
        late.timestamp = "2026-12-01T00:00:00Z".to_string();
        let records = vec![early.clone(), late.clone()];
        let (out, _) = filter_records(&records, None, None, Some("2026-06-01T00:00:00Z"), None, 10);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].tool, "b");
        let (out2, _) =
            filter_records(&records, None, None, None, Some("2026-06-01T00:00:00Z"), 10);
        assert_eq!(out2.len(), 1);
        assert_eq!(out2[0].tool, "a");
    }

    #[test]
    fn filter_records_truncation_flag() {
        let sid = Uuid::new_v4();
        let records: Vec<_> = (0..5)
            .map(|i| ToolCallRecord::now(&format!("t{i}"), true, 1, sid))
            .collect();
        let (out, truncated) = filter_records(&records, None, None, None, None, 3);
        assert_eq!(out.len(), 3);
        assert!(truncated);
        let (out2, truncated2) = filter_records(&records, None, None, None, None, 10);
        assert_eq!(out2.len(), 5);
        assert!(!truncated2);
    }
}

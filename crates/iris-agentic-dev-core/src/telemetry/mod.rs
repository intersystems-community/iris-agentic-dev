//! Tool-call telemetry: in-memory ring buffer (authoritative for `agent_history`/
//! `agent_stats`) plus a best-effort durable write: always the local JSONL file, and the IRIS
//! global too when the operator opted in (spec 141 R5 C16). See specs/059-tool-telemetry-benchmark/.

pub mod prune;
pub mod redact;
pub mod trace_export;

use crate::iris::connection::IrisConnection;
use crate::iris::workspace_config::ConnectionRole;
use std::future::Future;
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use uuid::Uuid;

/// Class-name prefix for the scratch class a durable IRIS write runs through, under `IrisDevTmp`.
/// User code keeps `IrisDevRun`, so a leftover class says which of the two leaked it.
pub const TELEMETRY_SCRATCH_PREFIX: &str = "IrisDevTel";

/// Durable writes that have been spawned and not yet finished.
///
/// `record_call` must not wait for its write (FR-014 of 059), but a process must not exit in the
/// middle of one either: the IRIS write is a put/compile/query/delete cycle, and a task dropped
/// between compile and delete leaves its scratch class behind for good (130 round 4 found 78,183 of
/// them in USER). Spawn through this, then [`PendingWrites::flush_blocking`] before exiting.
pub struct PendingWrites {
    count: AtomicUsize,
}

/// The tracker `record_call` spawns through and the CLI flushes on exit.
pub static DURABLE_WRITES: PendingWrites = PendingWrites::new();

/// How long an exiting process waits for its telemetry writes.
pub const EXIT_FLUSH_TIMEOUT: Duration = Duration::from_secs(3);

struct PendingGuard(&'static PendingWrites);

impl Drop for PendingGuard {
    fn drop(&mut self) {
        self.0.count.fetch_sub(1, Ordering::SeqCst);
    }
}

impl PendingWrites {
    pub const fn new() -> Self {
        Self {
            count: AtomicUsize::new(0),
        }
    }

    /// Spawn `fut` on the current runtime and count it until it finishes or is dropped.
    pub fn spawn<F>(&'static self, fut: F) -> tokio::task::JoinHandle<()>
    where
        F: Future<Output = ()> + Send + 'static,
    {
        self.count.fetch_add(1, Ordering::SeqCst);
        let guard = PendingGuard(self);
        tokio::spawn(async move {
            let _guard = guard;
            fut.await;
        })
    }

    pub fn pending(&self) -> usize {
        self.count.load(Ordering::SeqCst)
    }

    /// Wait until nothing is pending. `false` means the timeout came first.
    pub async fn flush(&self, timeout: Duration) -> bool {
        let deadline = Instant::now() + timeout;
        while self.pending() > 0 {
            if Instant::now() >= deadline {
                return false;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        true
    }

    /// [`Self::flush`] from sync code, for the exit path.
    ///
    /// On a multi-thread runtime this goes through `block_in_place`, which hands the calling
    /// worker's queue to another thread. A task spawned from this worker sits in its LIFO slot,
    /// which no other worker steals, so sleeping here instead would starve the very write being
    /// waited for. A current-thread runtime cannot run anything while blocked, so it only reports.
    pub fn flush_blocking(&self, timeout: Duration) -> bool {
        match tokio::runtime::Handle::try_current() {
            Ok(handle) if handle.runtime_flavor() == tokio::runtime::RuntimeFlavor::MultiThread => {
                tokio::task::block_in_place(|| handle.block_on(self.flush(timeout)))
            }
            _ => self.pending() == 0,
        }
    }
}

impl Default for PendingWrites {
    fn default() -> Self {
        Self::new()
    }
}

/// Flush [`DURABLE_WRITES`] for up to [`EXIT_FLUSH_TIMEOUT`]; a write still running then is logged.
pub fn flush_before_exit() {
    if !DURABLE_WRITES.flush_blocking(EXIT_FLUSH_TIMEOUT) {
        tracing::warn!(
            "exiting with {} telemetry write(s) unfinished; an IrisDevTmp.{}* class may be left in USER",
            DURABLE_WRITES.pending(),
            TELEMETRY_SCRATCH_PREFIX
        );
    }
}

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
    /// Gauntlet eval session identity — populated from GAUNTLET_RUN_ID env var when set.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub eval_run_id: Option<String>,
    /// Gauntlet task identifier — populated from GAUNTLET_TASK_ID env var when set.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub eval_task_id: Option<String>,
    /// Gauntlet experiment condition (harness/tool_surface/model) — from GAUNTLET_CONDITION.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub eval_condition: Option<String>,
}

/// Read Gauntlet eval session identity from environment variables.
/// Returns `(run_id, task_id, condition)`, each `None` if the var is absent or empty.
pub fn eval_session_from_env() -> (Option<String>, Option<String>, Option<String>) {
    let read = |k: &str| std::env::var(k).ok().filter(|v| !v.is_empty());
    (
        read("GAUNTLET_RUN_ID"),
        read("GAUNTLET_TASK_ID"),
        read("GAUNTLET_CONDITION"),
    )
}

impl ToolCallRecord {
    pub fn now(tool: &str, success: bool, duration_ms: u64, session_id: Uuid) -> Self {
        let (eval_run_id, eval_task_id, eval_condition) = eval_session_from_env();
        Self {
            tool: tool.to_string(),
            success,
            duration_ms,
            timestamp: now_rfc3339(),
            session_id,
            params: None,
            eval_run_id,
            eval_task_id,
            eval_condition,
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
/// Strings go in through [`crate::objectscript::os_str_expr`]: the quote is doubled and a control
/// character becomes a `$CHAR` splice, so a line break in a tool name or a parameter cannot end the
/// line the record is spliced into (spec 141 T221).
pub fn encode_listbuild(record: &ToolCallRecord) -> String {
    let tool = crate::objectscript::os_str_expr(&record.tool);
    let success = if record.success { 1 } else { 0 };
    let timestamp = crate::objectscript::os_str_expr(&record.timestamp);
    match &record.params {
        Some(p) => {
            let params_json = crate::objectscript::os_str_expr(&p.to_string());
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
        eval_run_id: None,
        eval_task_id: None,
        eval_condition: None,
    })
}

fn jsonl_path(config_dir: &Path, session_id: Uuid) -> std::path::PathBuf {
    config_dir
        .join("telemetry")
        .join(format!("{session_id}.jsonl"))
}

/// Where the durable record is kept besides the local file (spec 141 R5 C16).
///
/// The local JSONL file is always written. `Iris` adds the `^IRISDEV("telemetry")` global, which
/// costs a put, a compile, a query and a delete of a scratch class per call, so only the operator
/// turns it on: `telemetry_sink = "iris"` in the operator file, or `IAD_TELEMETRY_SINK=iris` in the
/// launch settings. A workspace file cannot.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TelemetrySink {
    #[default]
    Local,
    Iris,
}

impl TelemetrySink {
    /// Parse a `telemetry_sink` value; `None` for anything but `local` and `iris`.
    pub fn parse(v: &str) -> Option<Self> {
        match v.trim().to_ascii_lowercase().as_str() {
            "local" => Some(Self::Local),
            "iris" => Some(Self::Iris),
            _ => None,
        }
    }

    /// The spelling `check_config` reports.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Local => "local",
            Self::Iris => "iris",
        }
    }
}

/// The sink the operator chose: the operator file's `telemetry_sink`, or `IAD_TELEMETRY_SINK` in
/// the launch environment (`env` is its value). A workspace file's key is set aside before it
/// reaches `operator`, so it cannot be what this reads.
pub fn sink_setting(
    operator: &crate::iris::config_sources::OperatorSettings,
    env: Option<&str>,
) -> TelemetrySink {
    if operator.telemetry_sink == TelemetrySink::Iris
        || env.and_then(TelemetrySink::parse) == Some(TelemetrySink::Iris)
    {
        TelemetrySink::Iris
    } else {
        TelemetrySink::Local
    }
}

/// What decides whether the IRIS sink may write to one server (spec 141 R5 C16, T208).
///
/// Every field describes the server the call acted on, not the default one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SinkGate {
    /// The operator asked for the IRIS sink. Without it nothing else here matters.
    pub sink: TelemetrySink,
    /// That server's write gate is on.
    pub writes_enabled: bool,
    /// `read_only = "strict"`.
    pub strict_read_only: bool,
    /// The server is under source control, so a class compiled per call is a change-control
    /// event per call (PLATFORM-25). iad has no per-namespace probe, and a probe would itself
    /// compile a scratch class; it reads the operator's own marker: a `subject` role is a server
    /// under change control, where iad already refuses source-control writes.
    pub source_controlled: bool,
}

impl SinkGate {
    pub fn for_server(
        writes_enabled: bool,
        strict_read_only: bool,
        role: ConnectionRole,
        sink: TelemetrySink,
    ) -> Self {
        Self {
            sink,
            writes_enabled,
            strict_read_only,
            source_controlled: role == ConnectionRole::Subject,
        }
    }
}

/// The connection the IRIS sink may use, or `None` for the local file alone.
///
/// The IRIS sink is a put, a compile, a query and a delete of a scratch class in `USER` on every
/// call, so it runs only when the operator opted in, and then only where iad may write and nobody
/// watches the write: not when the server's writes are off, not under strict read-only, and not on
/// a server under source control. Everywhere else the record goes to the local file, which costs
/// the server nothing.
pub fn durable_sink(
    iris: Option<Arc<IrisConnection>>,
    gate: SinkGate,
) -> Option<Arc<IrisConnection>> {
    iris.filter(|_| {
        gate.sink == TelemetrySink::Iris
            && gate.writes_enabled
            && !gate.strict_read_only
            && !gate.source_controlled
    })
}

/// Best-effort durable write. MUST NOT propagate errors to the caller (FR-014) — every
/// failure path here is swallowed, logged at `tracing::debug`, and returns.
///
/// The record always goes to the local file. `iris` is the connection [`durable_sink`] allowed,
/// and the record goes there as well.
pub async fn write_durable(
    record: &ToolCallRecord,
    iris: Option<Arc<IrisConnection>>,
    client: &reqwest::Client,
    config_dir: &Path,
) {
    if let Err(e) = write_jsonl(record, config_dir) {
        tracing::debug!("telemetry durable write (local file) failed, dropping: {e}");
    }
    let Some(iris) = iris else { return };
    let listbuild = encode_listbuild(record);
    let code = format!(
        // splice-ok: sid is a Uuid this process generated
        "set seq=$INCREMENT(^IRISDEV(\"telemetry\",\"{sid}\"))\nset ^IRISDEV(\"telemetry\",\"{sid}\",seq)={lb}\n",
        sid = record.session_id,
        lb = listbuild
    );
    if let Err(e) = iris
        .execute_via_generator_prefixed(&code, "USER", client, TELEMETRY_SCRATCH_PREFIX)
        .await
    {
        tracing::debug!("telemetry durable write (IRIS) failed, dropping: {e}");
    }
}

fn write_jsonl(record: &ToolCallRecord, config_dir: &Path) -> anyhow::Result<()> {
    let line = serde_json::to_string(record)?;
    // Mode 600 in a 700 directory: the file holds tool names and, with `params`, call arguments.
    crate::iris::private_file::append_private(&jsonl_path(config_dir, record.session_id), &line)?;
    Ok(())
}

/// Read back durable records for a session (or all known sessions when `session_id` is
/// `None`): the local file always, and the IRIS global too when [`durable_sink`] allowed `iris`.
/// A call both sinks hold is returned once.
pub async fn read_durable(
    session_id: Option<Uuid>,
    iris: Option<Arc<IrisConnection>>,
    client: &reqwest::Client,
    config_dir: &Path,
) -> Vec<ToolCallRecord> {
    let local = read_durable_local(session_id, config_dir);
    let remote = match iris {
        Some(iris) => read_durable_iris(session_id, &iris, client).await,
        None => Vec::new(),
    };
    merge_records(local, remote)
}

/// The records of both sinks with each call once, local copies first.
///
/// With the IRIS sink on, every call is written to both, so the same call arrives twice. The
/// IRIS sequence number is no key: it counts per server, and a failed write leaves a gap in one
/// sink and not the other. A call is a session, a timestamp (nanoseconds) and a tool; the local
/// copy wins because it also carries the eval identity the IRIS record drops.
pub fn merge_records(local: Vec<ToolCallRecord>, iris: Vec<ToolCallRecord>) -> Vec<ToolCallRecord> {
    let key = |r: &ToolCallRecord| {
        (
            r.session_id,
            r.timestamp.clone(),
            r.tool.clone(),
            r.duration_ms,
        )
    };
    let mut seen: std::collections::HashSet<_> = local.iter().map(key).collect();
    let mut out = local;
    out.extend(iris.into_iter().filter(|r| seen.insert(key(r))));
    out
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
            // splice-ok: sid is a Uuid this process generated
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

    #[test]
    fn eval_session_fields_round_trip_serde() {
        let sid = Uuid::new_v4();
        let mut record = ToolCallRecord::now("iris_execute", true, 5, sid);
        record.eval_run_id = Some("gauntlet-ab12cd34".to_string());
        record.eval_task_id = Some("G05".to_string());
        record.eval_condition = Some("raw_api/none/claude-opus-4-8".to_string());
        let json = serde_json::to_string(&record).unwrap();
        let back: ToolCallRecord = serde_json::from_str(&json).unwrap();
        assert_eq!(back.eval_run_id.as_deref(), Some("gauntlet-ab12cd34"));
        assert_eq!(back.eval_task_id.as_deref(), Some("G05"));
        assert_eq!(
            back.eval_condition.as_deref(),
            Some("raw_api/none/claude-opus-4-8")
        );
    }

    #[test]
    fn eval_session_absent_in_json_when_not_set() {
        let sid = Uuid::new_v4();
        let record = ToolCallRecord::now("iris_compile", true, 1, sid);
        // Fields default to None — must not appear in serialized JSON
        assert!(record.eval_run_id.is_none());
        assert!(record.eval_task_id.is_none());
        assert!(record.eval_condition.is_none());
        let json = serde_json::to_string(&record).unwrap();
        assert!(!json.contains("eval_run_id"));
        assert!(!json.contains("eval_task_id"));
        assert!(!json.contains("eval_condition"));
    }

    #[test]
    fn eval_session_from_env_reads_gauntlet_vars() {
        // Use a temp env scope to avoid contaminating other tests.
        // Safety: single-threaded test binary.
        unsafe {
            std::env::set_var("GAUNTLET_RUN_ID", "gauntlet-test1234");
            std::env::set_var("GAUNTLET_TASK_ID", "G06");
            std::env::set_var("GAUNTLET_CONDITION", "claude_code/iad_mcp/opus");
        }
        let (run_id, task_id, condition) = eval_session_from_env();
        unsafe {
            std::env::remove_var("GAUNTLET_RUN_ID");
            std::env::remove_var("GAUNTLET_TASK_ID");
            std::env::remove_var("GAUNTLET_CONDITION");
        }
        assert_eq!(run_id.as_deref(), Some("gauntlet-test1234"));
        assert_eq!(task_id.as_deref(), Some("G06"));
        assert_eq!(condition.as_deref(), Some("claude_code/iad_mcp/opus"));
    }

    // ── 141 C16: the IRIS sink needs a server iad may write to ─────────────────────────────────

    use crate::iris::connection::DiscoverySource;
    use crate::iris::workspace_config::ConnectionRole;

    /// A connection nothing answers on: a write that reaches it fails, and one that does not leaves
    /// the record in the local file, which is what these tests read.
    fn unroutable() -> Arc<IrisConnection> {
        Arc::new(IrisConnection::new(
            "http://127.0.0.1:1",
            "USER",
            "_SYSTEM",
            "SYS",
            DiscoverySource::ExplicitFlag,
        ))
    }

    /// A gate for an operator who opted in to the IRIS sink: only the three server conditions vary.
    fn gate(writes_enabled: bool, strict: bool, role: ConnectionRole) -> SinkGate {
        SinkGate::for_server(writes_enabled, strict, role, TelemetrySink::Iris)
    }

    async fn written_locally(gate: SinkGate) -> Vec<ToolCallRecord> {
        let dir = tempfile::tempdir().unwrap();
        let sid = Uuid::new_v4();
        let record = ToolCallRecord::now("iris_info", true, 3, sid);
        let client = reqwest::Client::new();
        write_durable(
            &record,
            durable_sink(Some(unroutable()), gate),
            &client,
            dir.path(),
        )
        .await;
        read_durable_local(Some(sid), dir.path())
    }

    #[tokio::test]
    async fn durable_write_goes_local_when_writes_are_off() {
        let got = written_locally(gate(false, false, ConnectionRole::Workbench)).await;
        assert_eq!(
            got.len(),
            1,
            "with the default server's writes off the record must land in the local file"
        );
        assert_eq!(got[0].tool, "iris_info");
    }

    #[tokio::test]
    async fn durable_write_goes_local_under_strict_read_only() {
        let got = written_locally(gate(true, true, ConnectionRole::Workbench)).await;
        assert_eq!(
            got.len(),
            1,
            "strict read-only keeps telemetry off the server"
        );
    }

    #[tokio::test]
    async fn durable_write_goes_local_by_default_with_a_writable_connection() {
        // Writes on, not strict, not a subject server, a connection present: the old default sent
        // this to IRIS and compiled a class per call. Without the operator's opt-in it stays local.
        let default = SinkGate::for_server(
            true,
            false,
            ConnectionRole::Workbench,
            TelemetrySink::default(),
        );
        assert_eq!(TelemetrySink::default(), TelemetrySink::Local);
        assert!(durable_sink(Some(unroutable()), default).is_none());
        let got = written_locally(default).await;
        assert_eq!(got.len(), 1, "{got:?}");
    }

    #[tokio::test]
    async fn the_opt_in_with_writes_on_writes_to_both_sinks() {
        // The IRIS sink is unreachable here, so its write fails and is swallowed; the local record
        // is the sign that the file is written first and regardless, and `is_some` the sign that
        // the connection was handed to the IRIS write as well.
        let open = gate(true, false, ConnectionRole::Workbench);
        assert!(durable_sink(Some(unroutable()), open).is_some());
        let got = written_locally(open).await;
        assert_eq!(
            got.len(),
            1,
            "the local file is written with the opt-in too"
        );
        assert!(durable_sink(None, open).is_none());
    }

    // ── 141 T208: a server under source control gets no telemetry write ────────────────────────

    #[tokio::test]
    async fn durable_write_goes_local_on_a_server_under_source_control() {
        // A `subject` server is the one iad treats as under change control: its source-control
        // writes are refused. One compile per call there is what PLATFORM-25 reports as a queue
        // entry per call.
        let got = written_locally(gate(true, false, ConnectionRole::Subject)).await;
        assert_eq!(got.len(), 1, "the record must land in the local file");
    }

    #[test]
    fn only_a_subject_role_reads_as_source_controlled() {
        assert!(gate(true, false, ConnectionRole::Subject).source_controlled);
        assert!(!gate(true, false, ConnectionRole::Workbench).source_controlled);
    }

    #[test]
    fn each_condition_alone_closes_the_sink() {
        let conn = || Some(unroutable());
        for (label, g) in [
            ("writes off", gate(false, false, ConnectionRole::Workbench)),
            ("strict", gate(true, true, ConnectionRole::Workbench)),
            ("source control", gate(true, false, ConnectionRole::Subject)),
            (
                "no opt-in",
                SinkGate::for_server(true, false, ConnectionRole::Workbench, TelemetrySink::Local),
            ),
        ] {
            assert!(durable_sink(conn(), g).is_none(), "{label}");
        }
    }

    // ── 141 C16: reading both sinks ────────────────────────────────────────────────────────────

    #[test]
    fn merge_keeps_one_copy_of_a_call_both_sinks_hold() {
        let sid = Uuid::new_v4();
        let both = ToolCallRecord::now("iris_info", true, 3, sid);
        let only_local = ToolCallRecord::now("iris_query", true, 4, sid);
        let only_iris = ToolCallRecord::now("iris_doc", false, 5, sid);
        let mut local_copy = both.clone();
        local_copy.eval_run_id = Some("r1".into());
        let merged = merge_records(vec![local_copy, only_local], vec![both, only_iris]);
        let tools: Vec<&str> = merged.iter().map(|r| r.tool.as_str()).collect();
        assert_eq!(tools, vec!["iris_info", "iris_query", "iris_doc"]);
        assert_eq!(
            merged[0].eval_run_id.as_deref(),
            Some("r1"),
            "the local copy wins: it carries fields the IRIS record does not"
        );
    }

    #[test]
    fn merge_does_not_fold_two_calls_of_one_session_into_one() {
        let sid = Uuid::new_v4();
        let mut a = ToolCallRecord::now("iris_info", true, 3, sid);
        let mut b = a.clone();
        a.timestamp = "2026-10-24T10:00:00.000000001+00:00".into();
        b.timestamp = "2026-10-24T10:00:00.000000002+00:00".into();
        assert_eq!(merge_records(vec![a, b], Vec::new()).len(), 2);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn the_local_file_is_private() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let sid = Uuid::new_v4();
        let record = ToolCallRecord::now("iris_info", true, 3, sid);
        write_durable(&record, None, &reqwest::Client::new(), dir.path()).await;
        let file = jsonl_path(dir.path(), sid);
        let mode = |p: &Path| std::fs::metadata(p).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode(&file), 0o600, "the telemetry file");
        assert_eq!(
            mode(file.parent().unwrap()),
            0o700,
            "the telemetry directory"
        );
    }
}

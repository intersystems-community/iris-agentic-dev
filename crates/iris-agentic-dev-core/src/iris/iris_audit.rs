//! Opt-in `%SYS.Audit` emission per connection (`irisAudit = true` in `[policy.<server>]`).
//!
//! One record per tool call. `EventData` format:
//!   `tool=<name> mode=<mcp|cli> ua=<marker> [client=<name>/<version>]`
//!
//! Emission is best-effort and never fails the tool call it describes (FR-023).
//! When the event definition is absent the tool warns once and counts subsequent failures
//! rather than repeating. The tool never creates or modifies security configuration (FR-024).
//!
//! See `docs/agent-attribution.md` for the operator setup recipe.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

/// One-time setup command the operator must run in `%SYS` before emission can succeed.
pub const SETUP_CMD: &str = r#"##class(Security.Events).Create("iris-agentic-dev","Tool","ToolCall","iris-agentic-dev tool invocation",1)"#;

/// The EventSource, EventType, and Event identifiers for `$SYSTEM.Security.Audit`.
pub const EVENT_SOURCE: &str = "iris-agentic-dev";
pub const EVENT_TYPE: &str = "Tool";
pub const EVENT_NAME: &str = "ToolCall";

/// Build the `EventData` string for a single tool call.
///
/// Format: `tool=<name> mode=<mcp|cli> ua=<marker> [client=<name>/<version>]`
///
/// ASCII-only: the caller marker is already ASCII-safe (sanitizer enforces this per DP-446307).
/// Parameters are not included — this record carries identity and intent, not payload.
pub fn build_event_data(
    tool_name: &str,
    caller_mode: crate::iris::connection::CallerMode,
    mcp_peer: Option<(String, String)>,
) -> String {
    let ua = crate::iris::connection::user_agent_with_peer(caller_mode, mcp_peer.as_ref());
    let mode_str = match caller_mode {
        crate::iris::connection::CallerMode::Mcp => "mcp",
        crate::iris::connection::CallerMode::Cli => "cli",
    };
    let mut data = format!("tool={tool_name} mode={mode_str} ua={ua}");
    if let Some((name, version)) = &mcp_peer {
        data.push_str(&format!(" client={name}/{version}"));
    }
    data
}

/// Build the ObjectScript that emits one `%SYS.Audit` record and writes `1` on success
/// or `0` on failure (event absent or disabled).
pub fn build_audit_os(event_data: &str, description: &str) -> String {
    // Escape double-quotes and backslashes for ObjectScript string embedding.
    let ed = escape_for_os_string(event_data);
    let desc = escape_for_os_string(description);
    format!(
        r#"Set tResult=$SYSTEM.Security.Audit("{source}","{etype}","{event}","{ed}","{desc}") Write tResult,!"#,
        source = EVENT_SOURCE,
        etype = EVENT_TYPE,
        event = EVENT_NAME,
        ed = ed,
        desc = desc,
    )
}

/// Escape a string for embedding as a quoted ObjectScript literal.
/// ObjectScript string delimiters are `"…"`. A `"` inside is doubled (`""`).
fn escape_for_os_string(s: &str) -> String {
    // In ObjectScript, double `"` to escape. No backslash escape needed.
    s.replace('"', "\"\"")
}

/// Per-connection failure counter for `%SYS.Audit` emission.
#[derive(Debug, Default)]
pub struct AuditEmitCounter {
    pub failures: AtomicU64,
}

impl AuditEmitCounter {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            failures: AtomicU64::new(0),
        })
    }

    /// Record a failure and return whether this is the first one.
    pub fn record_failure(&self) -> bool {
        self.failures.fetch_add(1, Ordering::Relaxed) == 0
    }

    pub fn failure_count(&self) -> u64 {
        self.failures.load(Ordering::Relaxed)
    }
}

/// The reason text emitted when `$SYSTEM.Security.Audit` returns `0`.
/// The tool never calls `Security.Events.Create` itself (FR-024).
pub fn refuse_and_instruct_text() -> String {
    format!(
        "audit emission returned 0 — the event definition may be absent or disabled. \
         To enable: run in %SYS: {SETUP_CMD}"
    )
}

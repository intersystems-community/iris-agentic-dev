//! T028 / T034 — Unit tests for `iris_audit.rs`.
//!
//! T028: EventData format, ASCII constraint, ua= value, no-params invariant.
//! T034: Failure counter — warn-once then count.

use iris_agentic_dev_core::iris::connection::CallerMode;
use iris_agentic_dev_core::iris::iris_audit::{
    build_audit_os, build_event_data, refuse_and_instruct_text, AuditEmitCounter, EVENT_NAME,
    EVENT_SOURCE, EVENT_TYPE, SETUP_CMD,
};

// ─── T028: EventData format ────────────────────────────────────────────────

/// Format in MCP mode with no label and no client info.
#[test]
fn event_data_mcp_no_label_no_client() {
    std::env::remove_var("IRIS_AGENT_LABEL");
    let ed = build_event_data("iris_execute", CallerMode::Mcp, None);
    assert!(
        ed.starts_with("tool=iris_execute "),
        "tool field first: {ed}"
    );
    assert!(ed.contains("mode=mcp "), "mode field present: {ed}");
    assert!(
        ed.contains("ua=iris-agentic-dev/"),
        "ua field present: {ed}"
    );
    assert!(
        !ed.contains("client="),
        "client field absent when no peer: {ed}"
    );
}

/// Format in CLI mode.
#[test]
fn event_data_cli_mode() {
    std::env::remove_var("IRIS_AGENT_LABEL");
    let ed = build_event_data("iris_doc", CallerMode::Cli, None);
    assert!(ed.contains("mode=cli "), "cli mode in event data: {ed}");
}

/// When MCP client info is present, the `client=name/version` field appears.
#[test]
fn event_data_includes_client_when_peer_present() {
    std::env::remove_var("IRIS_AGENT_LABEL");
    let peer = Some(("claude-code".to_string(), "2.1.0".to_string()));
    let ed = build_event_data("iris_query", CallerMode::Mcp, peer);
    assert!(
        ed.contains("client=claude-code/2.1.0"),
        "client field present: {ed}"
    );
}

/// The `ua=` value must match what `user_agent_with_peer` produces for the same inputs.
#[test]
fn event_data_ua_byte_identical_to_marker() {
    std::env::remove_var("IRIS_AGENT_LABEL");
    let peer = Some(("test-client".to_string(), "9.9.9".to_string()));
    let ed = build_event_data("iris_execute", CallerMode::Mcp, peer.clone());
    let expected_ua = iris_agentic_dev_core::iris::connection::user_agent_with_peer(
        CallerMode::Mcp,
        peer.as_ref(),
    );
    assert!(
        ed.contains(&format!("ua={expected_ua}")),
        "ua= must be byte-identical to the User-Agent marker; ed={ed}"
    );
}

/// EventData is ASCII-only (DP-446307 guard).
#[test]
fn event_data_is_ascii() {
    std::env::remove_var("IRIS_AGENT_LABEL");
    let ed = build_event_data("iris_global", CallerMode::Mcp, None);
    assert!(ed.is_ascii(), "EventData must be ASCII-only: {ed:?}");
}

/// The ObjectScript built by `build_audit_os` contains the source, type, event, and data.
#[test]
fn audit_os_contains_identifiers() {
    let os = build_audit_os(
        "tool=iris_execute mode=mcp ua=iris-agentic-dev/1.2.7 (mcp)",
        "iad",
    );
    assert!(os.contains(EVENT_SOURCE), "EventSource present");
    assert!(os.contains(EVENT_TYPE), "EventType present");
    assert!(os.contains(EVENT_NAME), "Event name present");
    assert!(os.contains("tool=iris_execute"), "EventData embedded");
}

/// Double-quotes in EventData are escaped for ObjectScript strings.
#[test]
fn audit_os_escapes_double_quotes() {
    let ed = r#"ua=test "quoted""#;
    let os = build_audit_os(ed, "desc");
    // In ObjectScript, " inside a string becomes "" — so the escaped form is ""quoted""
    assert!(
        os.contains(r#""quoted"""#) || os.contains("\"\"quoted\"\""),
        "double-quotes escaped in OS string: {os}"
    );
}

/// The refuse-and-instruct text contains the exact `Security.Events.Create` command.
#[test]
fn refuse_and_instruct_contains_setup_cmd() {
    let text = refuse_and_instruct_text();
    assert!(
        text.contains(SETUP_CMD),
        "refuse text must contain the setup command: {text}"
    );
    assert!(
        text.contains("iris-agentic-dev"),
        "refuse text must name the event source: {text}"
    );
}

// ─── T034: Failure counter ─────────────────────────────────────────────────

/// First call to `record_failure` returns `true` (is-first flag).
#[test]
fn failure_counter_first_is_first() {
    let counter = AuditEmitCounter::new();
    assert!(counter.record_failure(), "first failure must return true");
    assert_eq!(counter.failure_count(), 1);
}

/// Subsequent calls return `false` and increment the count.
#[test]
fn failure_counter_subsequent_returns_false() {
    let counter = AuditEmitCounter::new();
    counter.record_failure(); // first
    assert!(!counter.record_failure(), "second failure returns false");
    assert!(!counter.record_failure(), "third failure returns false");
    assert_eq!(counter.failure_count(), 3);
}

/// Fresh counter starts at zero.
#[test]
fn failure_counter_starts_at_zero() {
    let counter = AuditEmitCounter::new();
    assert_eq!(counter.failure_count(), 0);
}

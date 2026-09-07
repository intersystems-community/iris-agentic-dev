//! Batch 4 against live IRIS: the nineteen parameters of the six read-only administration tools
//! still reach their handlers, and the two counts are now enforced as integers at the wire.
//!
//! What is proven live, and how:
//!
//! - `iris_system_performance`: `mode` by the handler's own rejection of an unknown value, `run_id`
//!   by the echo in the `status` answer, `profile` by the validator naming the value it refused.
//!   None of these starts a real profile run — a started run cannot be cancelled.
//! - `journal_search`: `max_entries` by the returned count, `server` by SERVER_NOT_FOUND.
//! - `query_audit_log`: `limit` by the row count, `user`/`event_type` by every returned row
//!   carrying the filtered value, `start`/`end` by the timestamps of the rows that come back.
//! - `my_access` / `capability_matrix` / `iris_mirror_status`: `user` by the echoed account,
//!   `server` by SERVER_NOT_FOUND.
//!
//! # Two defects found while writing these tests
//!
//! `journal_search`'s `start`, `end` and `global_pattern` are **not** live-provable, and the reason
//! is that they do not work:
//!
//! 1. The generated scan compares timestamps with `If ts<"<start>" Continue`. `<` is a *numeric*
//!    operator in ObjectScript, so both sides collapse to their leading number — the year — and
//!    `"2026-09-07 00:00:00" < "2026-09-07 00:30:16"` is 0. Inside one calendar year the two
//!    filters do nothing at all. That is the `max_chars` bug again: a documented parameter accepted
//!    and silently discarded. `objectscript_numeric_comparison_is_why_time_filters_do_nothing`
//!    pins the language fact the defect rests on.
//! 2. Every filter skips with `Continue`, which jumps back to the `While` condition *without*
//!    executing the `Set rec=rec.Next` at the bottom of the loop. So the first record that fails a
//!    filter spins one IRIS job on the same record forever; the call returns `IRIS_UNREACHABLE`
//!    after the 90-second HTTP timeout. Reachable today with any `global_pattern` that misses, or
//!    any `start`/`end` in a different year. No test here sends such a value: a deliberate
//!    90-second hang in the suite is not worth the coverage, and any discriminating live assertion
//!    about the filters would have to send one.
//!
//! Both are runtime defects, outside this feature's scope (it changes schemas, not semantics), and
//! both are recorded in `specs/113-typed-tool-schemas/parameter-audit.md`.
//!
//! Requires `iris-dev-iris`: `IRIS_HOST=localhost IRIS_WEB_PORT=52780 IRIS_USERNAME=_SYSTEM
//! IRIS_PASSWORD=SYS IRIS_NAMESPACE=USER`.

use iris_agentic_dev_core::testing::{answer_text, live_env, require_iad_binary, McpSession};

fn payload(answer: &serde_json::Value) -> serde_json::Value {
    let text = answer
        .pointer("/result/content/0/text")
        .and_then(serde_json::Value::as_str)
        .unwrap_or_else(|| panic!("no text content in {}", answer_text(answer)));
    serde_json::from_str(text).unwrap_or_else(|e| panic!("content was not JSON ({e}): {text}"))
}

fn call(mcp: &mut McpSession, tool: &str, args: serde_json::Value) -> serde_json::Value {
    payload(&mcp.call(tool, &args))
}

fn entries(p: &serde_json::Value) -> Vec<serde_json::Value> {
    p.get("entries")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_else(|| panic!("no entries array in {p}"))
}

fn field(row: &serde_json::Value, key: &str) -> String {
    row.get(key)
        .and_then(|v| v.as_str())
        .unwrap_or_else(|| panic!("row has no string `{key}`: {row}"))
        .to_string()
}

/// `mode`, `run_id` and `profile` all reach the handler. Nothing here starts a profile: `start` is
/// exercised only with a name the validator refuses, because `run^SystemPerformance` has no cancel
/// entry point and the shortest shipped profile still collects for five minutes.
#[test]
#[ignore = "requires live IRIS (iris-dev-iris) and the built binary"]
fn system_performance_honors_mode_run_id_and_profile() {
    let Some(_bin) = require_iad_binary() else {
        return;
    };
    let mut mcp = McpSession::start(&live_env());

    // `mode`: the handler's own parse produces the message, which is the reason `mode` is not in
    // `required` — serde would replace this sentence with a deserialization failure.
    let unknown = call(
        &mut mcp,
        "iris_system_performance",
        serde_json::json!({"mode": "bogus"}),
    );
    let err = unknown
        .get("error")
        .and_then(|v| v.as_str())
        .unwrap_or_default();
    assert!(
        err.contains("bogus") && err.contains("start, status, last_runid"),
        "`mode` must reach the handler's parse: {unknown}"
    );

    // `mode=last_runid` is the read-only mode: it reports the instance's most recent run.
    let last = call(
        &mut mcp,
        "iris_system_performance",
        serde_json::json!({"mode": "last_runid"}),
    );
    assert_eq!(
        last.get("mode").and_then(|v| v.as_str()),
        Some("last_runid"),
        "mode=last_runid must be honored: {last}"
    );

    // `run_id`: echoed back in the status answer. A run ID that does not exist is answered by
    // `waittime^SystemPerformance` with `-2^no such runid`, which is proof the value arrived.
    let status = call(
        &mut mcp,
        "iris_system_performance",
        serde_json::json!({"mode": "status", "run_id": "IADP113_nosuchrun"}),
    );
    assert_eq!(
        status.get("run_id").and_then(|v| v.as_str()),
        Some("IADP113_nosuchrun"),
        "`run_id` must be echoed by mode=status: {status}"
    );

    // `profile`: refused by name before any run starts.
    let bad_profile = call(
        &mut mcp,
        "iris_system_performance",
        serde_json::json!({"mode": "start", "profile": "iadp113-bad"}),
    );
    let err = bad_profile
        .get("error")
        .and_then(|v| v.as_str())
        .unwrap_or_default();
    assert!(
        err.contains("iadp113-bad"),
        "`profile` must reach the validator, which names the value it refused: {bad_profile}"
    );
}

/// `max_entries` is the only journal filter that works (see the module docs), and it is the one this
/// batch changes the type of: `"3"` used to be discarded in favour of the default 100.
#[test]
#[ignore = "requires live IRIS (iris-dev-iris) and the built binary"]
fn journal_search_honors_max_entries() {
    let Some(_bin) = require_iad_binary() else {
        return;
    };
    let mut mcp = McpSession::start(&live_env());

    for want in [2u64, 7] {
        let answer = call(
            &mut mcp,
            "journal_search",
            serde_json::json!({"max_entries": want}),
        );
        assert_eq!(
            answer.get("returned").and_then(|v| v.as_u64()),
            Some(want),
            "max_entries={want} must cap the scan: {answer}"
        );
    }
}

/// The language fact behind defect 1: ObjectScript's `<` is numeric, so two timestamps in the same
/// year compare equal and `If ts<"<start>" Continue` never fires. This is why
/// `journal_search`'s `start`/`end` are accepted and do nothing, and why no test here asserts they
/// filter. If IRIS ever compares these as strings, this test fails and the audit entry gets revisited.
#[test]
#[ignore = "requires live IRIS (iris-dev-iris) and the built binary"]
fn objectscript_numeric_comparison_is_why_time_filters_do_nothing() {
    let Some(_bin) = require_iad_binary() else {
        return;
    };
    let mut env = live_env();
    env.push(("IRIS_WRITE_TOOLS_ENABLED".to_string(), "1".to_string()));
    let mut mcp = McpSession::start(&env);

    let answer = call(
        &mut mcp,
        "iris_execute",
        serde_json::json!({
            "code": "Write (\"2026-09-07 00:00:00\"<\"2026-09-07 00:30:16\"),\"|\",\
                     (\"2026-09-07 00:00:00\"]]\"2026-09-07 00:30:16\"),!",
            "namespace": "%SYS"
        }),
    );
    let out = answer
        .get("output")
        .and_then(|v| v.as_str())
        .unwrap_or_else(|| panic!("no output in {answer}"))
        .trim()
        .to_string();
    assert_eq!(
        out, "0|0",
        "`<` on two same-year timestamps must be 0 (numeric comparison of the year) and `]]` must \
         be 0 (the string operator, correctly ordering them); got {out}. journal_search uses `<`, \
         which is the defect."
    );
}

/// `limit`, `user`, `event_type`, `start` and `end` all reach the SQL. The audit log on
/// `iris-dev-iris` always holds `%Login` and `%Security` events, so the filters have something to
/// select — and something to exclude, which is what makes each assertion discriminating.
#[test]
#[ignore = "requires live IRIS (iris-dev-iris) and the built binary"]
fn query_audit_log_honors_user_event_type_limit_and_time_bounds() {
    let Some(_bin) = require_iad_binary() else {
        return;
    };
    let mut mcp = McpSession::start(&live_env());

    // `limit`: the row count is the proof. Before this batch, `"2"` here silently became 100.
    let two = call(&mut mcp, "query_audit_log", serde_json::json!({"limit": 2}));
    assert_eq!(
        two.get("count").and_then(|v| v.as_u64()),
        Some(2),
        "limit=2 must cap the rows: {two}"
    );

    let sample = entries(&call(
        &mut mcp,
        "query_audit_log",
        serde_json::json!({"limit": 200}),
    ));
    assert!(
        sample.len() > 5,
        "the audit log on this instance is too small to test filters against: {} rows",
        sample.len()
    );

    // `event_type`: exact match on the column. Rows are returned newest first, so a log with
    // several event types is proof the filter excluded the others.
    let want_type = field(&sample[0], "event_type");
    let filtered = entries(&call(
        &mut mcp,
        "query_audit_log",
        serde_json::json!({"event_type": want_type, "limit": 20}),
    ));
    assert!(
        !filtered.is_empty(),
        "event_type={want_type} returned zero rows"
    );
    for row in &filtered {
        assert_eq!(
            field(row, "event_type"),
            want_type,
            "event_type={want_type} was dropped — this row came back anyway: {row}"
        );
    }

    // `user`: same shape, on Username.
    let want_user = field(&sample[0], "username");
    let by_user = entries(&call(
        &mut mcp,
        "query_audit_log",
        serde_json::json!({"user": want_user, "limit": 20}),
    ));
    assert!(!by_user.is_empty(), "user={want_user} returned zero rows");
    for row in &by_user {
        assert_eq!(
            field(row, "username"),
            want_user,
            "user={want_user} was dropped: {row}"
        );
    }

    // `start` / `end`: bounds on UTCTimeStamp, which is the column the rows report. Unlike
    // journal_search these are SQL comparisons on a timestamp column, so they actually order.
    let newest = field(&sample[0], "timestamp");
    let oldest = field(&sample[sample.len() - 1], "timestamp");
    assert!(
        newest > oldest,
        "expected the sample to span a time range, got {oldest} .. {newest}"
    );
    let bounded = entries(&call(
        &mut mcp,
        "query_audit_log",
        serde_json::json!({"start": newest.clone(), "limit": 50}),
    ));
    for row in &bounded {
        assert!(
            field(row, "timestamp") >= newest,
            "start={newest} was dropped — this older row came back: {row}"
        );
    }
    let capped = entries(&call(
        &mut mcp,
        "query_audit_log",
        serde_json::json!({"end": oldest.clone(), "limit": 50}),
    ));
    for row in &capped {
        assert!(
            field(row, "timestamp") <= oldest,
            "end={oldest} was dropped — this newer row came back: {row}"
        );
    }
    assert!(
        !capped.is_empty(),
        "end={oldest} must still return the rows at or before it"
    );
}

/// `capability_matrix.user` names a different account than the connection's, which is the whole
/// point of the parameter — and `my_access` proves the two tools answer about different users.
#[test]
#[ignore = "requires live IRIS (iris-dev-iris) and the built binary"]
fn access_tools_honor_user_and_report_mirror_membership() {
    let Some(_bin) = require_iad_binary() else {
        return;
    };
    let mut mcp = McpSession::start(&live_env());

    let mine = call(&mut mcp, "my_access", serde_json::json!({}));
    let me = mine
        .get("username")
        .and_then(|v| v.as_str())
        .unwrap_or_else(|| panic!("my_access returned no username: {mine}"));
    assert_eq!(me, "_SYSTEM", "the suite connects as _SYSTEM: {mine}");

    let theirs = call(
        &mut mcp,
        "capability_matrix",
        serde_json::json!({"user": "irisowner"}),
    );
    assert_eq!(
        theirs.get("user").and_then(|v| v.as_str()),
        Some("irisowner"),
        "`user` must select the account looked up, not the connected one: {theirs}"
    );

    // `iris_mirror_status` takes only `server`. iris-dev-iris is not a mirror member, and saying so
    // is the answer — an error here would mean the tool never ran.
    let mirror = call(&mut mcp, "iris_mirror_status", serde_json::json!({}));
    assert_eq!(
        mirror.get("is_member").and_then(|v| v.as_bool()),
        Some(false),
        "iris-dev-iris is not a mirror member: {mirror}"
    );
}

/// `server` on all six. An unknown instance name comes back as a JSON-RPC error naming it, which is
/// only possible if the parameter survived deserialization.
#[test]
#[ignore = "requires the built binary; no IRIS call is reached"]
fn every_tool_in_the_batch_passes_server_to_the_pool() {
    let Some(_bin) = require_iad_binary() else {
        return;
    };
    let mut mcp = McpSession::start(&live_env());

    for tool in [
        "iris_system_performance",
        "iris_mirror_status",
        "journal_search",
        "query_audit_log",
        "my_access",
        "capability_matrix",
    ] {
        let answer = mcp.call(
            tool,
            &serde_json::json!({"server": "iadp113-no-such-server"}),
        );
        let text = answer_text(&answer);
        assert!(
            text.contains("iadp113-no-such-server"),
            "`{tool}` must pass `server` to the pool, which names it when the lookup fails: {text}"
        );
    }
}

/// The two counts, sent as strings. `max_entries: "3"` and `limit: "100"` were accepted and
/// discarded before this batch — the caller got 100 rows either way with nothing to explain it.
///
/// The refusal names the expected type, not the parameter: it is serde's own text relayed by the
/// router. T032 wraps this path in `error_code: "INVALID_PARAMS"`; tighten this test when it lands.
#[test]
#[ignore = "requires the built binary; no IRIS call is reached"]
fn wrongly_typed_counts_are_refused() {
    let Some(_bin) = require_iad_binary() else {
        return;
    };
    let mut mcp = McpSession::start(&live_env());

    for (tool, args) in [
        ("journal_search", serde_json::json!({"max_entries": "3"})),
        ("query_audit_log", serde_json::json!({"limit": "100"})),
    ] {
        let answer = mcp.call(tool, &args);
        let text = answer_text(&answer);
        assert_eq!(
            answer
                .pointer("/result/isError")
                .and_then(serde_json::Value::as_bool),
            Some(true),
            "`{tool}` must refuse a string count instead of falling back to the default: {answer}"
        );
        assert!(
            text.contains("invalid type") && text.contains("u32"),
            "the refusal from `{tool}` must say which type was expected: {text}"
        );
    }
}

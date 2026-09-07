//! Batch 5 against live IRIS: the thirty-three parameters of the five interoperability tools still
//! reach their handlers, and the two that accept an integer-or-string still accept both.
//!
//! The data these filters are asserted against is created by `seed_interop_fixture()`, not found.
//! This suite originally asserted against whatever the local container held — "several thousand
//! rows including complete sessions", "an Event Log with error entries", "two seeded rule sets" —
//! which is a description of one developer's container after months of work, not of a namespace.
//! Every such assertion passed locally and failed the first time CI ran it against a fresh
//! instance: `limit=2` got one row, `limit=20` got four, the session lookup found nothing to look
//! up. Filters are still asserted against data rather than error text; the data is now the
//! fixture's.
//!
//! What is proven live, and how:
//!
//! - `iris_interop_query`: `what` by the dispatcher's own INVALID_ACTION, `limit` by the row count,
//!   `log_type` and `component` by the count dropping to zero for a value nothing matches,
//!   `source`/`target`/`message_class`/`session_id` by every returned row carrying the filtered
//!   value, `since_id` by every returned ID being above the watermark, `body_class`/`body_where`/
//!   `body_select` by the joined column appearing in the rows and the echoed SQL, `search_table` by
//!   the lookup naming the property and extent it could not find, `namespace` by the 404 from a
//!   namespace that does not exist.
//! - `iris_production_item`: `action` by INVALID_ACTION listing the four verbs, `namespace` by 404.
//! - `iris_production_diff` / `iris_business_rule_info`: `namespace` by 404, `action` and
//!   `rule_name` by the rule set that comes back named.
//! - `iris_message_body`: `message_id` by the handler's required-parameter check, which runs before
//!   the PHI gate.
//! - All five: `server` by SERVER_NOT_FOUND.
//!
//! # What is not provable live, and why
//!
//! - `iris_production_item.item` and `.settings` need an open production. No production is running
//!   on `iris-dev-iris` and every action answers `INTEROP_ERROR: Cannot open production` before the
//!   item name is used, so a real item and a nonsense one are indistinguishable from outside.
//!   Starting a production to make the distinction would leave the shared dev container in a
//!   different state than it was found in.
//! - `iris_production_diff.production` is read but never reaches IRIS here: the namespace has no
//!   source control, so `NO_SCM` comes back for every production name including nonsense ones.
//! - `iris_message_body.namespace`, `.max_bytes`, `.acknowledgePhi` and `.dataPolicy` sit behind the
//!   bulk-PHI dispatch gate, which answers `DATA_POLICY_BLOCKED` from the *server* policy — the
//!   `dataPolicy` parameter does not open it, and the message says so explicitly. Reaching the body
//!   read would require a `.iris-agentic-dev.toml` in the server's working directory, which this
//!   repository deliberately does not have.
//!
//! Each of those is covered by the source contract in `tests/binary/schema_batch5.rs`
//! (`batch5_handlers_read_their_contract`), which reads the handler body rather than the response.
//!
//! Requires `iris-dev-iris`: `IRIS_HOST=localhost IRIS_WEB_PORT=52780 IRIS_USERNAME=_SYSTEM
//! IRIS_PASSWORD=SYS IRIS_NAMESPACE=USER`.

use iris_agentic_dev_core::testing::{
    answer_text, live_env, require_iad_binary, seed_interop_fixture, McpSession,
};

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

fn interop(mcp: &mut McpSession, args: serde_json::Value) -> serde_json::Value {
    call(mcp, "iris_interop_query", args)
}

fn rows(p: &serde_json::Value, key: &str) -> Vec<serde_json::Value> {
    p.get(key)
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_else(|| panic!("no `{key}` array in {p}"))
}

fn error_of(p: &serde_json::Value) -> String {
    p.get("error")
        .and_then(|v| v.as_str())
        .unwrap_or_else(|| panic!("expected an error in {p}"))
        .to_string()
}

fn code_of(p: &serde_json::Value) -> String {
    p.get("error_code")
        .and_then(|v| v.as_str())
        .unwrap_or_else(|| panic!("expected an error_code in {p}"))
        .to_string()
}

/// `what` picks the sub-query, and the two log filters narrow it.
///
/// `log_type` and `component` are asserted by contrast, because a zero on its own could just mean an
/// empty Event Log. The contrast is drawn against the fixture's own entries rather than against the
/// instance: they are all type Error under one `ConfigName`, so the same component asked for `error`
/// returns them and asked for `info` returns none. Both halves of that are guaranteed by the seed.
#[test]
#[ignore = "requires live IRIS (iris-dev-iris) and the built binary"]
fn interop_query_dispatches_on_what_and_narrows_the_event_log() {
    let Some(_bin) = require_iad_binary() else {
        return;
    };
    let fixture = seed_interop_fixture();
    let mut mcp = McpSession::start(&live_env());

    let bogus = interop(&mut mcp, serde_json::json!({"what": "nonsense"}));
    assert_eq!(code_of(&bogus), "INVALID_ACTION");
    assert!(
        error_of(&bogus).contains("logs, queues, or messages"),
        "`what` must reach the dispatcher, which lists the values it accepts: {bogus}"
    );

    let queues = interop(&mut mcp, serde_json::json!({"what": "queues"}));
    assert!(
        queues.get("queues").is_some(),
        "what=queues must answer with a queues array: {queues}"
    );

    let two = interop(&mut mcp, serde_json::json!({"what": "logs", "limit": 2}));
    assert_eq!(
        two.get("count").and_then(|v| v.as_u64()),
        Some(2),
        "limit=2 must cap the Event Log rows: {two}"
    );

    // `component` and `log_type` together, on the fixture's entries. Type 2 is Error on the
    // `Ens.Util.Log.Type` valuelist (`,1,2,3,4,5,6` against `,Assert,Error,Warning,Info,Trace,
    // Alert`), and asserting the code rather than just the row count is what catches a mapping that
    // is shifted one place — which it was, so `error` selected Warning.
    let errors = interop(
        &mut mcp,
        serde_json::json!({
            "what": "logs", "component": fixture.component, "log_type": "error", "limit": 5
        }),
    );
    let error_rows = rows(&errors, "logs");
    assert_eq!(
        error_rows.len(),
        5,
        "the fixture seeds {} Error entries under `{}`: {errors}",
        fixture.log_entries,
        fixture.component
    );
    for row in &error_rows {
        assert_eq!(
            row.get("ConfigName").and_then(|v| v.as_str()),
            Some(fixture.component),
            "`component` was dropped — this row came back anyway: {row}"
        );
        assert_eq!(
            row.get("Type").and_then(|v| v.as_i64()),
            Some(2),
            "log_type=error must select Ens.Util.Log.Type 2, not its neighbour: {row}"
        );
    }

    // The other half of the contrast: the fixture writes nothing of type Info, so the same component
    // asked for `info` is empty. Together the two prove `log_type` narrows rather than being ignored.
    let info = interop(
        &mut mcp,
        serde_json::json!({
            "what": "logs", "component": fixture.component, "log_type": "info", "limit": 5
        }),
    );
    assert_eq!(
        rows(&info, "logs").len(),
        0,
        "log_type=info must exclude the fixture's Error entries: {info}"
    );

    // An unrecognized name applies no filter, which used to widen the query silently. It is reported
    // now, and the report is part of the contract.
    let typo = interop(
        &mut mcp,
        serde_json::json!({"what": "logs", "log_type": "eror", "limit": 1}),
    );
    assert_eq!(
        typo.get("unknown_log_types")
            .and_then(|v| v.as_array())
            .map(|a| a.len()),
        Some(1),
        "an unrecognized log_type must be reported rather than quietly dropped: {typo}"
    );

    // `component`: same shape, on ConfigName.
    let no_component = interop(
        &mut mcp,
        serde_json::json!({"what": "logs", "component": "IADP113NoSuchComponent", "limit": 5}),
    );
    assert_eq!(
        rows(&no_component, "logs").len(),
        0,
        "component=<nonexistent> must exclude everything: {no_component}"
    );
}

/// The message-archive filters, each asserted row by row so a filter that is dropped comes back as
/// a row that should not be there rather than as a count that happens to match.
#[test]
#[ignore = "requires live IRIS (iris-dev-iris) and the built binary"]
fn interop_query_message_filters_reach_the_sql() {
    let Some(_bin) = require_iad_binary() else {
        return;
    };
    let fixture = seed_interop_fixture();
    let mut mcp = McpSession::start(&live_env());

    // The fixture guarantees 30 headers, so a 20-row window is full whatever else the namespace
    // holds. Reading the filter values off `sample[0]` is what made this suite container-dependent:
    // on a fresh instance there was no row 0 to read them from.
    let sample = rows(
        &interop(
            &mut mcp,
            serde_json::json!({"what": "messages", "limit": 20}),
        ),
        "messages",
    );
    assert_eq!(sample.len(), 20, "limit=20 must return exactly 20 rows");

    let source = fixture.source.to_string();
    let by_source = rows(
        &interop(
            &mut mcp,
            serde_json::json!({"what": "messages", "source": source, "limit": 10}),
        ),
        "messages",
    );
    assert!(!by_source.is_empty(), "source={source} returned nothing");
    for row in &by_source {
        assert_eq!(
            row.get("SourceConfigName").and_then(|v| v.as_str()),
            Some(source.as_str()),
            "source={source} was dropped — this row came back anyway: {row}"
        );
    }

    // `target` is a different column. The fixture's source and target names are distinct, so the two
    // halves here separate "the filter was applied" from "the filter was applied to the right
    // column": the target name returns the fixture's rows, and the source name returns none.
    let by_target = rows(
        &interop(
            &mut mcp,
            serde_json::json!({"what": "messages", "target": fixture.target, "limit": 10}),
        ),
        "messages",
    );
    assert!(
        !by_target.is_empty(),
        "target={} returned nothing",
        fixture.target
    );
    for row in &by_target {
        assert_eq!(
            row.get("TargetConfigName").and_then(|v| v.as_str()),
            Some(fixture.target),
            "target={} was applied to the wrong column: {row}",
            fixture.target
        );
    }
    let source_as_target = rows(
        &interop(
            &mut mcp,
            serde_json::json!({"what": "messages", "target": source, "limit": 10}),
        ),
        "messages",
    );
    assert!(
        source_as_target.is_empty(),
        "`target` must read TargetConfigName; nothing targets `{source}`: {source_as_target:?}"
    );

    let class = fixture.body_class.to_string();
    let by_class = rows(
        &interop(
            &mut mcp,
            serde_json::json!({"what": "messages", "message_class": class, "limit": 10}),
        ),
        "messages",
    );
    assert!(
        !by_class.is_empty(),
        "message_class={class} returned nothing"
    );
    for row in &by_class {
        assert_eq!(
            row.get("MessageBodyClassName").and_then(|v| v.as_str()),
            Some(class.as_str()),
            "message_class={class} was dropped: {row}"
        );
    }

    // `namespace`: a namespace that does not exist cannot answer, and the failure proves the value
    // was used to route the request rather than being ignored in favour of the connection's.
    let bad_ns = interop(
        &mut mcp,
        serde_json::json!({"what": "messages", "namespace": "IADP113NOSUCHNS", "limit": 1}),
    );
    assert_eq!(code_of(&bad_ns), "INTEROP_ERROR");
    assert!(
        error_of(&bad_ns).contains("404"),
        "namespace must route the request; a nonexistent one must fail rather than fall back: \
         {bad_ns}"
    );
}

/// FR-008: `session_id` and `since_id` accept a JSON number or a decimal string, and the two forms
/// must produce byte-identical answers. This is the guard that would have caught declaring either
/// one as a plain `integer`.
#[test]
#[ignore = "requires live IRIS (iris-dev-iris) and the built binary"]
fn session_id_and_since_id_accept_both_json_forms() {
    let Some(_bin) = require_iad_binary() else {
        return;
    };
    // The fixture's headers all share one nonzero SessionId, so the value under test is known before
    // the first call. This used to scan the newest rows for a sessioned message and take whatever it
    // found, which failed two different ways: on a fresh instance nothing had a SessionId at all, and
    // on a busy one a run of session-less headers at the head of the queue pushed the first sessioned
    // message past the window. Neither is anything to do with `session_id`.
    let fixture = seed_interop_fixture();
    let session = fixture.session_id;
    let mut mcp = McpSession::start(&live_env());

    let as_int = interop(
        &mut mcp,
        serde_json::json!({"what": "messages", "session_id": session, "limit": 10}),
    );
    let as_str = interop(
        &mut mcp,
        serde_json::json!({"what": "messages", "session_id": session.to_string(), "limit": 10}),
    );
    assert_eq!(
        as_int, as_str,
        "session_id={session} and \"{session}\" must answer identically (FR-008)"
    );
    let session_rows = rows(&as_int, "messages");
    assert!(
        !session_rows.is_empty(),
        "session_id={session} returned nothing: {as_int}"
    );
    for row in &session_rows {
        assert_eq!(
            row.get("SessionId").and_then(|v| v.as_i64()),
            Some(session),
            "session_id={session} was dropped: {row}"
        );
    }

    // `since_id` is a watermark: every returned ID must be above it, which also proves the value
    // was not silently replaced by a default. The fixture's own headers supply it, so there are
    // guaranteed to be rows on both sides of the mark.
    let ids: Vec<i64> = session_rows
        .iter()
        .filter_map(|r| r.get("ID").and_then(|v| v.as_i64()))
        .collect();
    assert!(
        ids.len() >= 2,
        "the watermark needs rows on both sides of it; got {ids:?}"
    );
    let watermark = ids[ids.len() - 1];

    let as_int = interop(
        &mut mcp,
        serde_json::json!({"what": "messages", "since_id": watermark, "limit": 10}),
    );
    let as_str = interop(
        &mut mcp,
        serde_json::json!({"what": "messages", "since_id": watermark.to_string(), "limit": 10}),
    );
    assert_eq!(
        as_int, as_str,
        "since_id={watermark} and \"{watermark}\" must answer identically (FR-008)"
    );
    let tail = rows(&as_int, "messages");
    assert!(
        !tail.is_empty(),
        "since_id={watermark} should still leave newer messages: {as_int}"
    );
    for row in &tail {
        let id = row
            .get("ID")
            .and_then(|v| v.as_i64())
            .unwrap_or_else(|| panic!("no ID in {row}"));
        assert!(
            id > watermark,
            "since_id={watermark} tails after the watermark; ID {id} is not above it: {row}"
        );
    }
}

/// The body-table join: three parameters that only make sense together, and the one place in this
/// batch where a parameter changes the *shape* of the answer rather than the number of rows.
#[test]
#[ignore = "requires live IRIS (iris-dev-iris) and the built binary"]
fn body_class_where_and_select_join_the_body_table() {
    let Some(_bin) = require_iad_binary() else {
        return;
    };
    // The fixture's bodies are `Ens.StringContainer`, so the join has something to join to. It used
    // to rely on the container already holding string-container messages.
    let fixture = seed_interop_fixture();
    let mut mcp = McpSession::start(&live_env());

    let joined = interop(
        &mut mcp,
        serde_json::json!({
            "what": "messages",
            "source": fixture.source,
            "body_class": fixture.body_class,
            "body_where": "1=1",
            "body_select": ["StringValue"],
            "limit": 2
        }),
    );
    assert_eq!(
        joined.get("body_table").and_then(|v| v.as_str()),
        Some(fixture.body_class),
        "`body_class` must resolve to a body table and be reported: {joined}"
    );
    let rows_joined = rows(&joined, "messages");
    assert!(
        !rows_joined.is_empty(),
        "the join returned nothing: {joined}"
    );
    for row in &rows_joined {
        assert!(
            row.get("StringValue").is_some(),
            "`body_select` must add the selected column to each row: {row}"
        );
    }

    // `body_where` is a SQL fragment, so a false predicate is the discriminating test: same class,
    // same select, no rows.
    let none = interop(
        &mut mcp,
        serde_json::json!({
            "what": "messages",
            "source": fixture.source,
            "body_class": fixture.body_class,
            "body_where": "1=0",
            "body_select": ["StringValue"],
            "limit": 2
        }),
    );
    assert_eq!(
        rows(&none, "messages").len(),
        0,
        "`body_where` must reach the SQL; 1=0 must exclude everything: {none}"
    );
}

/// `search_table` is a nested object, and the lookup that fails names both the property it wanted
/// and the extent it searched — so one call proves `prop` arrived and a second proves `extent`
/// overrides the default.
#[test]
#[ignore = "requires live IRIS (iris-dev-iris) and the built binary"]
fn search_table_filter_reaches_the_search_table_lookup() {
    let Some(_bin) = require_iad_binary() else {
        return;
    };
    let mut mcp = McpSession::start(&live_env());

    let default_extent = interop(
        &mut mcp,
        serde_json::json!({
            "what": "messages",
            "search_table": {"prop": "IADP113NoSuchProp", "value": "x"},
            "limit": 2
        }),
    );
    assert_eq!(code_of(&default_extent), "SEARCH_PROP_NOT_FOUND");
    let msg = error_of(&default_extent);
    assert!(
        msg.contains("IADP113NoSuchProp") && msg.contains("EnsLib.HL7.SearchTable"),
        "the lookup must name the property it was given and the extent it defaulted to: {msg}"
    );

    let own_extent = interop(
        &mut mcp,
        serde_json::json!({
            "what": "messages",
            // `value` is not optional in practice: the handler requires exactly one of
            // `value`/`value_like` and refuses with INVALID_PARAMS before the property lookup, so
            // the extent can only be observed on a filter that carries one.
            "search_table": {
                "prop": "IADP113NoSuchProp",
                "extent": "IADP113NoSuchExtent",
                "value": "x"
            },
            "limit": 2
        }),
    );
    assert!(
        error_of(&own_extent).contains("IADP113NoSuchExtent"),
        "`extent` must override the default extent: {own_extent}"
    );

    // `value` and `value_like` are mutually exclusive, and the handler says so. That check is the
    // only place either one is observable without a matching search table, and it proves both keys
    // deserialize: with neither the filter is refused, with both it is refused the same way, and
    // with exactly one it gets as far as the property lookup.
    for filter in [
        serde_json::json!({"prop": "IADP113NoSuchProp"}),
        serde_json::json!({"prop": "IADP113NoSuchProp", "value": "x", "value_like": "x%"}),
    ] {
        let refused = interop(
            &mut mcp,
            serde_json::json!({"what": "messages", "search_table": filter, "limit": 2}),
        );
        assert_eq!(code_of(&refused), "INVALID_PARAMS");
        assert!(
            error_of(&refused).contains("exactly one of value"),
            "the one-of check must run on the deserialized filter: {refused}"
        );
    }

    let by_value_like = interop(
        &mut mcp,
        serde_json::json!({
            "what": "messages",
            "search_table": {"prop": "IADP113NoSuchProp", "value_like": "x%"},
            "limit": 2
        }),
    );
    assert_eq!(
        code_of(&by_value_like),
        "SEARCH_PROP_NOT_FOUND",
        "`value_like` alone must satisfy the one-of check and reach the lookup: {by_value_like}"
    );
}

/// `iris_production_item` and `iris_production_diff`. Both are namespace-routed, and both answer
/// from IRIS rather than from a local guess, which is what the 404 proves.
#[test]
#[ignore = "requires live IRIS (iris-dev-iris) and the built binary"]
fn production_tools_honor_action_and_namespace() {
    let Some(_bin) = require_iad_binary() else {
        return;
    };
    let mut mcp = McpSession::start(&live_env());

    let bogus = call(
        &mut mcp,
        "iris_production_item",
        serde_json::json!({"action": "nonsense", "item": "OrderService"}),
    );
    assert_eq!(code_of(&bogus), "INVALID_ACTION");
    assert!(
        error_of(&bogus).contains("enable, disable, get_settings, or set_settings"),
        "`action` must reach the dispatcher, which lists the verbs it accepts: {bogus}"
    );

    // A real verb gets past the dispatcher and fails at IRIS instead — a different error, which is
    // how this call proves the first one was the dispatcher's and not a blanket refusal.
    let real_verb = call(
        &mut mcp,
        "iris_production_item",
        serde_json::json!({"action": "get_settings", "item": "OrderService"}),
    );
    assert_eq!(
        code_of(&real_verb),
        "INTEROP_ERROR",
        "a valid action must be dispatched, not rejected: {real_verb}"
    );

    let bad_ns = call(
        &mut mcp,
        "iris_production_item",
        serde_json::json!({
            "action": "get_settings", "item": "OrderService", "namespace": "IADP113NOSUCHNS"
        }),
    );
    assert!(
        error_of(&bad_ns).contains("404"),
        "`namespace` must route the request: {bad_ns}"
    );

    // `iris_production_diff` has no source control configured here, which is itself the answer —
    // and it is an answer from IRIS, so the tool ran.
    let diff = call(&mut mcp, "iris_production_diff", serde_json::json!({}));
    assert_eq!(code_of(&diff), "NO_SCM");
    let diff_bad_ns = call(
        &mut mcp,
        "iris_production_diff",
        serde_json::json!({"namespace": "IADP113NOSUCHNS"}),
    );
    assert!(
        error_of(&diff_bad_ns).contains("404"),
        "`namespace` must route the diff: {diff_bad_ns}"
    );
}

/// `iris_business_rule_info`: `action` selects list-or-get, `rule_name` selects which rule set, and
/// `namespace` routes. All three are asserted against the fixture's rule set.
#[test]
#[ignore = "requires live IRIS (iris-dev-iris) and the built binary"]
fn business_rule_info_honors_action_rule_name_and_namespace() {
    let Some(_bin) = require_iad_binary() else {
        return;
    };
    // The fixture writes one `Ens_Rule.RuleSet` row, which is the table `action=list` reads. This
    // asserted against "the seeded rule sets on this instance" and found none on a fresh one.
    let fixture = seed_interop_fixture();
    let mut mcp = McpSession::start(&live_env());

    let listed = call(
        &mut mcp,
        "iris_business_rule_info",
        serde_json::json!({"action": "list"}),
    );
    let rules = rows(&listed, "rules");
    assert!(
        rules
            .iter()
            .any(|r| r.get("name").and_then(|v| v.as_str()) == Some(fixture.rule_name)),
        "action=list must include the fixture's rule set `{}`: {listed}",
        fixture.rule_name
    );
    let name = fixture.rule_name.to_string();

    let got = call(
        &mut mcp,
        "iris_business_rule_info",
        serde_json::json!({"action": "get", "rule_name": name}),
    );
    assert_eq!(
        got.get("name").and_then(|v| v.as_str()),
        Some(name.as_str()),
        "`rule_name` must select which rule set is described: {got}"
    );
    assert!(
        got.get("rules").is_none(),
        "action=get must answer with one rule set, not the list: {got}"
    );

    let bogus = call(
        &mut mcp,
        "iris_business_rule_info",
        serde_json::json!({"action": "nonsense"}),
    );
    assert!(
        error_of(&bogus).contains("nonsense"),
        "`action` must reach the handler, which echoes what it refused: {bogus}"
    );

    let bad_ns = call(
        &mut mcp,
        "iris_business_rule_info",
        serde_json::json!({"action": "list", "namespace": "IADP113NOSUCHNS"}),
    );
    assert!(
        error_of(&bad_ns).contains("404"),
        "`namespace` must route the lookup: {bad_ns}"
    );
}

/// `message_id` is checked by the handler before the bulk-PHI dispatch gate runs, which is the only
/// window in which this tool's parameters are observable on a default-policy instance. The two
/// answers differ, so the parameter arrived.
#[test]
#[ignore = "requires live IRIS (iris-dev-iris) and the built binary"]
fn message_body_reads_message_id_before_the_phi_gate() {
    let Some(_bin) = require_iad_binary() else {
        return;
    };
    let mut mcp = McpSession::start(&live_env());

    let missing = call(&mut mcp, "iris_message_body", serde_json::json!({}));
    assert_eq!(code_of(&missing), "INVALID_PARAMS");
    assert!(
        error_of(&missing).contains("message_id is required"),
        "an absent message_id must reach the handler's own check, not a serde failure — that is \
         why `message_id` is not in `required`: {missing}"
    );

    let given = call(
        &mut mcp,
        "iris_message_body",
        serde_json::json!({"message_id": "1"}),
    );
    assert_eq!(
        code_of(&given),
        "DATA_POLICY_BLOCKED",
        "with a message_id the handler gets past its own check and the PHI gate answers instead: \
         {given}"
    );
}

/// `server` on all five. An unknown instance name comes back as a JSON-RPC error naming it, which
/// is only reachable if the parameter survived deserialization.
#[test]
#[ignore = "requires the built binary; no IRIS call is reached"]
fn every_tool_in_the_batch_passes_server_to_the_pool() {
    let Some(_bin) = require_iad_binary() else {
        return;
    };
    let mut mcp = McpSession::start(&live_env());

    for (tool, args) in [
        (
            "iris_interop_query",
            serde_json::json!({"what": "queues", "server": "iadp113-no-such-server"}),
        ),
        (
            "iris_production_item",
            serde_json::json!({"action": "get_settings", "server": "iadp113-no-such-server"}),
        ),
        (
            "iris_production_diff",
            serde_json::json!({"server": "iadp113-no-such-server"}),
        ),
        (
            "iris_message_body",
            serde_json::json!({"message_id": "1", "server": "iadp113-no-such-server"}),
        ),
        (
            "iris_business_rule_info",
            serde_json::json!({"action": "list", "server": "iadp113-no-such-server"}),
        ),
    ] {
        let text = answer_text(&mcp.call(tool, &args));
        assert!(
            text.contains("iadp113-no-such-server"),
            "`{tool}` must pass `server` to the pool, which names it when the lookup fails: {text}"
        );
    }
}

/// The typed slots, sent as the wrong JSON type. Every one of these was accepted and discarded
/// before this batch: `limit: "3"` returned 50 rows, `max_bytes: "100"` returned 65536 bytes,
/// `acknowledgePhi: "true"` read as false, and `settings: "x"` set nothing.
///
/// The refusal names the expected type rather than the parameter — it is serde's own text relayed
/// by the router. T032 wraps this path in `error_code: "INVALID_PARAMS"`; tighten this test then.
#[test]
#[ignore = "requires the built binary; no IRIS call is reached"]
fn wrongly_typed_values_are_refused() {
    let Some(_bin) = require_iad_binary() else {
        return;
    };
    let mut mcp = McpSession::start(&live_env());

    for (tool, expected_type, args) in [
        (
            "iris_interop_query",
            "u32",
            serde_json::json!({"what": "logs", "limit": "3"}),
        ),
        (
            "iris_message_body",
            "u32",
            serde_json::json!({"message_id": "1", "max_bytes": "100"}),
        ),
        (
            "iris_message_body",
            "bool",
            serde_json::json!({"message_id": "1", "acknowledgePhi": "true"}),
        ),
        (
            "iris_production_item",
            "map",
            serde_json::json!({"action": "get_settings", "settings": "Foo=1"}),
        ),
        (
            "iris_interop_query",
            "sequence",
            serde_json::json!({"what": "messages", "body_select": "StringValue"}),
        ),
    ] {
        let answer = mcp.call(tool, &args);
        let text = answer_text(&answer);
        assert_eq!(
            answer
                .pointer("/result/isError")
                .and_then(serde_json::Value::as_bool),
            Some(true),
            "`{tool}` must refuse {args} instead of discarding the value: {answer}"
        );
        assert!(
            text.contains("invalid type") && text.contains(expected_type),
            "the refusal from `{tool}` must say what was expected (`{expected_type}`): {text}"
        );
    }
}

/// A stray key is refused twice over: `#[serde(deny_unknown_fields)]` on the params struct would
/// already reject it, and the validation site T030 added replaces serde's message with one that
/// names the tool, the offending key, and the keys that would have worked. That last part is what
/// makes the refusal actionable — a caller who misspelled a parameter gets the spelling back.
#[test]
#[ignore = "requires the built binary; no IRIS call is reached"]
fn an_unknown_parameter_is_refused_with_the_accepted_names() {
    let Some(_bin) = require_iad_binary() else {
        return;
    };
    let mut mcp = McpSession::start(&live_env());

    let answer = mcp.call(
        "iris_interop_query",
        &serde_json::json!({"what": "queues", "iadp113_typo": 1}),
    );
    let text = answer_text(&answer);
    assert!(
        text.contains("UNKNOWN_PARAMETER"),
        "a misspelled parameter must be refused with the UNKNOWN_PARAMETER code: {text}"
    );
    assert!(
        text.contains("iris_interop_query") && text.contains("iadp113_typo"),
        "the refusal must name the tool and the key that was rejected: {text}"
    );
    for accepted in ["what", "component", "namespace", "server"] {
        assert!(
            text.contains(accepted),
            "the refusal must list `{accepted}` among the accepted parameters so the caller can \
             fix the call without reading the docs: {text}"
        );
    }
}

//! Batch 7 against live IRIS: the last twenty-one parameter slots of the conversion still reach
//! their handlers.
//!
//! Every tool here is read-only, so one session with the default environment covers the batch. The
//! discriminators, in order of strength:
//!
//! - `mermaid_class.depth` is echoed *and* changes the output: `depth=1` gives an
//!   immediate-superclass diagram, `depth=5` gives eight edges, and `depth=9` comes back as 5
//!   because the handler clamps it. Nothing else in the feature demonstrates a parameter this
//!   completely.
//! - `class`, `production`, `oid` and `schema` are echoed in their tools' answers, or named in the
//!   error when they miss.
//! - `namespace` is proven by a namespace that does not exist: every tool answers
//!   `IRIS_UNREACHABLE` with an HTTP 404 rather than falling back to the connection's namespace.
//! - `server` is proven by an instance that is not in the pool: `resolve_server` refuses with
//!   `SERVER_NOT_FOUND` before the tool runs.
//!
//! # What is not provable live, and why
//!
//! `hl7_schema_inspect.schema` and `.segment` have no discriminating assertion on `iris-dev-iris`.
//! The container is IRIS Community, which has no `EnsLib.HL7.Schema`, so every call answers
//! `HL7_NOT_AVAILABLE` before either parameter is used, and the message does not echo them. The
//! source-side check in `tests/binary/schema_batch7.rs` covers that the handler reads them; proving
//! it end to end needs IRIS for Health. Same shape of gap as batch 4's `iris_mirror_status` and
//! batch 5's `iris_production_item` — recorded rather than papered over.
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

fn str_of(p: &serde_json::Value, key: &str) -> String {
    p.get(key)
        .and_then(|v| v.as_str())
        .unwrap_or_else(|| panic!("no string `{key}` in {p}"))
        .to_string()
}

/// `class` and `depth`, the batch's one integer. `depth` is asserted three ways — echoed, effective,
/// and clamped — because it is the only parameter in the conversion where all three are visible.
#[test]
#[ignore = "requires live IRIS (iris-dev-iris) and the built binary"]
fn mermaid_class_honors_class_and_depth() {
    let Some(_bin) = require_iad_binary() else {
        return;
    };
    let mut mcp = McpSession::start(&live_env());

    let missing = call(
        &mut mcp,
        "mermaid_class",
        serde_json::json!({"class": "IADP113.NoSuchClass"}),
    );
    assert_eq!(code_of(&missing), "CLASS_NOT_FOUND");
    assert!(
        error_of(&missing).contains("IADP113.NoSuchClass"),
        "`class` must reach the lookup and be named back: {missing}"
    );

    let shallow = call(
        &mut mcp,
        "mermaid_class",
        serde_json::json!({"class": "Ens.MessageHeader", "depth": 1}),
    );
    assert_eq!(
        shallow.get("depth").and_then(|v| v.as_u64()),
        Some(1),
        "`depth` must be echoed as sent: {shallow}"
    );

    let deep = call(
        &mut mcp,
        "mermaid_class",
        serde_json::json!({"class": "Ens.MessageHeader", "depth": 5}),
    );
    let shallow_edges = str_of(&shallow, "diagram").lines().count();
    let deep_edges = str_of(&deep, "diagram").lines().count();
    assert!(
        deep_edges > shallow_edges,
        "`depth` must change the traversal, not just the echo: depth=1 gave {shallow_edges} lines, \
         depth=5 gave {deep_edges}"
    );

    // The clamp is the interesting case: the handler answers with 5, not 9, so a client can see what
    // it actually got. Declaring the type does not change that — it changes 9 from "a number the
    // caller had no way to know was capped" into a number in a declared integer field.
    let clamped = call(
        &mut mcp,
        "mermaid_class",
        serde_json::json!({"class": "Ens.MessageHeader", "depth": 9}),
    );
    assert_eq!(
        clamped.get("depth").and_then(|v| v.as_u64()),
        Some(5),
        "`depth` above the maximum must come back clamped to 5: {clamped}"
    );
}

/// `production` for the flowchart generator, `class` for the storage resolver. Neither errors on a
/// name that does not exist — `mermaid_production` draws a single node and `resolve_storage` returns
/// an empty list — so the echo is the evidence, and it is enough: an empty string could not produce
/// it.
#[test]
#[ignore = "requires live IRIS (iris-dev-iris) and the built binary"]
fn mermaid_production_and_resolve_storage_echo_their_subject() {
    let Some(_bin) = require_iad_binary() else {
        return;
    };
    let mut mcp = McpSession::start(&live_env());

    let drawn = call(
        &mut mcp,
        "mermaid_production",
        serde_json::json!({"production": "IADP113.NoSuchProduction"}),
    );
    assert_eq!(
        str_of(&drawn, "production"),
        "IADP113.NoSuchProduction",
        "`production` must be echoed: {drawn}"
    );
    assert!(
        str_of(&drawn, "diagram").contains("IADP113"),
        "the name must reach the diagram itself, not only the echo: {drawn}"
    );

    let storage = call(
        &mut mcp,
        "resolve_storage",
        serde_json::json!({"class": "Ens.MessageHeader"}),
    );
    assert_eq!(str_of(&storage, "class"), "Ens.MessageHeader");
    let locations = storage
        .get("storages")
        .and_then(|v| v.as_array())
        .unwrap_or_else(|| panic!("no storages array in {storage}"));
    assert!(
        locations
            .iter()
            .any(|s| s.get("data_location").and_then(|v| v.as_str()) == Some("^Ens.MessageHeaderD")),
        "`class` must select the class whose global maps come back: {storage}"
    );
}

/// `oid` for the stream reader. An id with no stream behind it is not an error — `%OpenId` hands back
/// an empty stream — so the echo is the discriminator here too.
#[test]
#[ignore = "requires live IRIS (iris-dev-iris) and the built binary"]
fn stream_inspect_echoes_the_oid_it_was_given() {
    let Some(_bin) = require_iad_binary() else {
        return;
    };
    let mut mcp = McpSession::start(&live_env());

    let inspected = call(&mut mcp, "stream_inspect", serde_json::json!({"oid": "1"}));
    assert_eq!(
        str_of(&inspected, "oid"),
        "1",
        "`oid` must be echoed: {inspected}"
    );
    assert!(
        inspected.get("type").is_some() && inspected.get("size").is_some(),
        "the answer must carry the stream's type and size: {inspected}"
    );
}

/// The HL7 pair. `namespace` is provable — it is resolved before the availability check — but
/// `schema` and `segment` are not, on a Community container that has no `EnsLib.HL7.Schema`.
#[test]
#[ignore = "requires live IRIS (iris-dev-iris) and the built binary"]
fn hl7_tools_report_their_absence_rather_than_ignoring_it() {
    let Some(_bin) = require_iad_binary() else {
        return;
    };
    let mut mcp = McpSession::start(&live_env());

    for (tool, args) in [
        ("hl7_schema_list", serde_json::json!({})),
        (
            "hl7_schema_inspect",
            serde_json::json!({"schema": "2.5", "segment": "MSH"}),
        ),
    ] {
        let answered = call(&mut mcp, tool, args);
        assert_eq!(
            code_of(&answered),
            "HL7_NOT_AVAILABLE",
            "`{tool}` on IRIS Community must say the schema library is missing, which is also why \
             `schema` and `segment` have no live assertion in this suite: {answered}"
        );
    }
}

/// `namespace` on every tool in the batch. A namespace that does not exist must fail rather than
/// quietly resolving to the connection's — the answer would otherwise be right for the wrong
/// namespace, which is the worst outcome available.
#[test]
#[ignore = "requires live IRIS (iris-dev-iris) and the built binary"]
fn namespace_routes_every_tool_in_the_batch() {
    let Some(_bin) = require_iad_binary() else {
        return;
    };
    let mut mcp = McpSession::start(&live_env());

    for (tool, mut args) in [
        ("hl7_schema_list", serde_json::json!({})),
        ("hl7_schema_inspect", serde_json::json!({"schema": "2.5"})),
        (
            "mermaid_class",
            serde_json::json!({"class": "%Library.Base"}),
        ),
        (
            "mermaid_production",
            serde_json::json!({"production": "IADP113.NoSuchProduction"}),
        ),
        (
            "resolve_storage",
            serde_json::json!({"class": "Ens.MessageHeader"}),
        ),
        ("stream_inspect", serde_json::json!({"oid": "1"})),
    ] {
        args["namespace"] = serde_json::json!("IADP113NOSUCHNS");
        let answered = call(&mut mcp, tool, args.clone());
        // Two shapes of proof, both acceptable. Most of these tools fail the HTTP request against a
        // namespace that has no web application, so the 404 surfaces. `mermaid_class` gets far enough
        // to look the class up and names the namespace it looked in, which is stronger evidence still.
        let failure = error_of(&answered);
        assert!(
            failure.contains("404") || failure.contains("IADP113NOSUCHNS"),
            "`{tool}` must send `namespace` to IRIS rather than falling back to the connection's: \
             {answered}"
        );
    }
}

/// `server` on every tool in the batch. `resolve_server` refuses an unregistered name with a
/// JSON-RPC error, before the tool body runs, so this is the one assertion in the batch that reads
/// the transport error rather than a tool payload.
#[test]
#[ignore = "requires the built binary; the pool refuses before IRIS is reached"]
fn server_selects_from_the_pool_on_every_tool_in_the_batch() {
    let Some(_bin) = require_iad_binary() else {
        return;
    };
    let mut mcp = McpSession::start(&live_env());

    for (tool, mut args) in [
        ("hl7_schema_list", serde_json::json!({})),
        ("hl7_schema_inspect", serde_json::json!({"schema": "2.5"})),
        (
            "mermaid_class",
            serde_json::json!({"class": "%Library.Base"}),
        ),
        (
            "mermaid_production",
            serde_json::json!({"production": "IADP113.NoSuchProduction"}),
        ),
        (
            "resolve_storage",
            serde_json::json!({"class": "Ens.MessageHeader"}),
        ),
        ("stream_inspect", serde_json::json!({"oid": "1"})),
    ] {
        args["server"] = serde_json::json!("iadp113_nosuch");
        let text = answer_text(&mcp.call(tool, &args));
        assert!(
            text.contains("SERVER_NOT_FOUND"),
            "`{tool}` must pass `server` to the pool: {text}"
        );
    }
}

/// The wrong JSON type in each slot. `depth: "5"` is the one that mattered in practice: the handler
/// reads `as_u64()`, so a quoted number was discarded and the caller silently got the default 3.
#[test]
#[ignore = "requires the built binary; no IRIS call is reached"]
fn wrongly_typed_values_are_refused() {
    let Some(_bin) = require_iad_binary() else {
        return;
    };
    let mut mcp = McpSession::start(&live_env());

    for (tool, args) in [
        (
            "mermaid_class",
            serde_json::json!({"class": "%Library.Base", "depth": "5"}),
        ),
        (
            "mermaid_class",
            serde_json::json!({"class": ["%Library.Base"]}),
        ),
        ("stream_inspect", serde_json::json!({"oid": 1})),
        ("resolve_storage", serde_json::json!({"class": {}})),
        ("hl7_schema_inspect", serde_json::json!({"segment": 3})),
        (
            "mermaid_production",
            serde_json::json!({"production": true}),
        ),
    ] {
        let text = answer_text(&mcp.call(tool, &args));
        assert!(
            text.contains("invalid type"),
            "`{tool}` must refuse {args} instead of discarding the value: {text}"
        );
    }
}

/// The bug that motivated the feature, run as a test. `stream_inspect` was documented with a
/// character cap no code read, so a caller asking for 10,000 characters got the whole stream and no
/// warning. The same call now comes back as `UNKNOWN_PARAMETER` naming the key and the three
/// parameters that do exist.
#[test]
#[ignore = "requires the built binary; no IRIS call is reached"]
fn the_parameter_that_named_this_feature_is_refused_by_name() {
    let Some(_bin) = require_iad_binary() else {
        return;
    };
    let mut mcp = McpSession::start(&live_env());

    let text = answer_text(&mcp.call(
        "stream_inspect",
        &serde_json::json!({"oid": "1", "max_chars": 10000}),
    ));
    assert!(
        text.contains("UNKNOWN_PARAMETER"),
        "the refusal must carry the UNKNOWN_PARAMETER code: {text}"
    );
    assert!(
        text.contains("stream_inspect") && text.contains("max_chars"),
        "the refusal must name the tool and the invented parameter: {text}"
    );
    for accepted in ["namespace", "oid", "server"] {
        assert!(
            text.contains(accepted),
            "the refusal must list `{accepted}` among the accepted parameters: {text}"
        );
    }
}

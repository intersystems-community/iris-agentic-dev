//! A value outside a declared enum is still rejected by the handler, with the same message.
//!
//! The enums added for FR-006 are schema annotations. `#[schemars(extend("enum" = [...]))]` changes
//! what `tools/list` advertises; it does not change what serde accepts, so `{"mode": "bogus"}` still
//! deserializes into `Option<String>` and still reaches the handler. That is deliberate (research
//! decision 2: handler validation is unchanged) and it is the part worth pinning — a client that
//! ignores the enum must keep getting the error that names the permitted values, not a serde
//! type-mismatch that names none of them.
//!
//! These run against live IRIS because both handlers acquire a connection before they validate.

use iris_agentic_dev_core::iris::connection::{DiscoverySource, IrisConnection};

fn make_iris_tools() -> Option<iris_agentic_dev_core::tools::IrisTools> {
    let host = std::env::var("IRIS_HOST").unwrap_or_default();
    if host.is_empty() {
        return None;
    }
    let web_port = std::env::var("IRIS_WEB_PORT").unwrap_or_else(|_| "52773".to_string());
    let username = std::env::var("IRIS_USERNAME").unwrap_or_else(|_| "_SYSTEM".to_string());
    let password = std::env::var("IRIS_PASSWORD").unwrap_or_else(|_| "SYS".to_string());
    let conn = IrisConnection::new(
        format!("http://{host}:{web_port}"),
        "USER",
        username,
        password,
        DiscoverySource::EnvVar,
    );
    Some(iris_agentic_dev_core::tools::IrisTools::new(Some(conn)).expect("IrisTools::new"))
}

fn parse_result(r: Result<rmcp::model::CallToolResult, String>) -> serde_json::Value {
    let r = r.expect("call_for_test returned Err");
    let text = r.content[0].as_text().unwrap().text.clone();
    serde_json::from_str(&text).unwrap_or_else(|_| serde_json::json!({"raw": text}))
}

/// One dispatcher parameter's current answer to a value outside its set.
///
/// `args` carries the sibling parameters needed to reach the dispatch site — `iris_global` refuses
/// the call at deserialization without `global_name`, so a bare `{"action": …}` would measure serde
/// rather than the handler. `field` is where the text lands, and it is not the same field for every
/// tool; recording it keeps a silent move from `error` to `message` (or back) from passing.
struct Baseline {
    tool: &'static str,
    param: &'static str,
    args: serde_json::Value,
    code: &'static str,
    field: &'static str,
    message: &'static str,
}

/// The value sent in every case below. Fixed so the recorded messages can be compared verbatim.
const OUTSIDE: &str = "zzbogus";

/// Measured against `iris-dev-iris` on 2026-09-07, before any of the twelve enums were declared.
///
/// Three error codes and two carrier fields across twelve dispatchers is not a design; it is what
/// the tree does today, and FR-010 says declaring the enums must not change any of it. The
/// inconsistency is a separate finding, recorded in the spec rather than fixed here — fixing it in
/// the same change would make this test unable to tell a deliberate repair from a regression.
fn baselines() -> Vec<Baseline> {
    vec![
        Baseline {
            tool: "iris_doc",
            param: "mode",
            args: serde_json::json!({"mode": OUTSIDE}),
            code: "INVALID_PARAMS",
            field: "error",
            message:
                "unknown mode \"zzbogus\". Valid: get, put, delete, head, fragment, compiled, \
                      list, insert, delete_lines.",
        },
        Baseline {
            tool: "iris_info",
            param: "what",
            args: serde_json::json!({"what": OUTSIDE}),
            code: "INVALID_PARAM",
            field: "error",
            message:
                "Unknown what='zzbogus'. Use: documents, modified, namespace, metadata, jobs, \
                      csp_apps, csp_debug, sa_schema",
        },
        Baseline {
            tool: "iris_macro",
            param: "action",
            args: serde_json::json!({"action": OUTSIDE}),
            code: "INVALID_PARAM",
            field: "error",
            message: "Unknown action='zzbogus'. Use: list, signature, location, definition, expand",
        },
        Baseline {
            tool: "iris_debug",
            param: "action",
            args: serde_json::json!({"action": OUTSIDE}),
            code: "INVALID_PARAM",
            field: "error",
            message: "Unknown action='zzbogus'. Use: map_int, error_logs, capture, source_map",
        },
        Baseline {
            tool: "skill",
            param: "action",
            args: serde_json::json!({"action": OUTSIDE}),
            code: "INVALID_PARAM",
            field: "error",
            message: "Unknown action='zzbogus'. Use: list, describe, search, forget, propose",
        },
        Baseline {
            tool: "skill_community",
            param: "action",
            args: serde_json::json!({"action": OUTSIDE}),
            code: "INVALID_PARAM",
            field: "error",
            message: "Unknown action='zzbogus'. Use: list, install",
        },
        Baseline {
            tool: "kb",
            param: "action",
            args: serde_json::json!({"action": OUTSIDE}),
            code: "INVALID_PARAM",
            field: "error",
            message: "Unknown action='zzbogus'. Use: index, recall",
        },
        Baseline {
            tool: "agent_info",
            param: "what",
            args: serde_json::json!({"what": OUTSIDE}),
            code: "INVALID_PARAM",
            field: "error",
            message: "Unknown what='zzbogus'. Use: stats, history",
        },
        Baseline {
            tool: "iris_source_control",
            param: "action",
            args: serde_json::json!({"action": OUTSIDE}),
            code: "INVALID_PARAM",
            field: "error",
            message: "Unknown action='zzbogus'. Use: status, menu, checkout, execute",
        },
        Baseline {
            tool: "iris_global",
            param: "action",
            args: serde_json::json!({"action": OUTSIDE, "global_name": "^zzProbe"}),
            code: "INVALID_ACTION",
            field: "message",
            message: "unknown action: zzbogus (expected: get, set, kill, list)",
        },
        Baseline {
            tool: "iris_production",
            param: "action",
            args: serde_json::json!({"action": OUTSIDE}),
            code: "INVALID_ACTION",
            field: "error",
            message:
                "iris_production: action must be status, start, stop, update, check, recover, \
                      get_autostart, or set_autostart",
        },
    ]
}

/// The whole point of FR-010: the enums are schema annotations, so every one of these answers must
/// survive the declaration byte for byte. Recorded before the declarations landed, which is what
/// makes a later pass mean something.
#[tokio::test]
async fn every_declared_dispatcher_rejects_an_outside_value_exactly_as_before() {
    let Some(tools) = make_iris_tools() else {
        return;
    };
    let mut problems: Vec<String> = Vec::new();
    for b in baselines() {
        let v = parse_result(tools.call_for_test(b.tool, b.args.clone()).await);
        if v.get("success") != Some(&serde_json::json!(false)) {
            problems.push(format!(
                "{}.{}: `{OUTSIDE}` was not refused at all: {v}",
                b.tool, b.param
            ));
            continue;
        }
        match v.get("error_code").and_then(|c| c.as_str()) {
            Some(code) if code == b.code => {}
            other => problems.push(format!(
                "{}.{}: error_code was `{}`, recorded as `{}`",
                b.tool,
                b.param,
                other.unwrap_or("<absent>"),
                b.code
            )),
        }
        match v.get(b.field).and_then(|m| m.as_str()) {
            Some(text) if text == b.message => {}
            Some(text) => problems.push(format!(
                "{}.{}: `{}` reads\n    {text}\n  recorded as\n    {}",
                b.tool, b.param, b.field, b.message
            )),
            None => problems.push(format!(
                "{}.{}: no `{}` field; the message moved: {v}",
                b.tool, b.param, b.field
            )),
        }
    }
    assert!(
        problems.is_empty(),
        "{} of the eleven dispatchers changed its rejection:\n  {}\n\
         Declaring an enum must not alter runtime validation (FR-010). Either a handler changed or \
         the enum is being enforced somewhere it should not be.",
        problems.len(),
        problems.join("\n  ")
    );
}

/// `iris_admin.type` is the twelfth parameter and the one that does not reject. It filters the
/// webapp list with `eq_ignore_ascii_case`, so a value outside the set matches nothing and returns an
/// empty list with `success: true`. Declaring the enum must not promote that into an error — a client
/// asking for a type no application uses is asking a legitimate question.
#[tokio::test]
async fn a_filter_parameter_outside_its_set_still_returns_an_empty_list() {
    let Some(tools) = make_iris_tools() else {
        return;
    };
    let v = parse_result(
        tools
            .call_for_test(
                "iris_admin",
                serde_json::json!({"action": "list_webapps", "type": OUTSIDE}),
            )
            .await,
    );
    assert_eq!(
        v.get("success"),
        Some(&serde_json::json!(true)),
        "an unmatched `type` filter is not an error: {v}"
    );
    assert_eq!(
        v.get("count"),
        Some(&serde_json::json!(0)),
        "no application has type `{OUTSIDE}`, so the filter must return nothing: {v}"
    );
}

/// The message must list the values, not merely say no.
#[tokio::test]
async fn a_mode_outside_the_enum_names_the_permitted_values() {
    let Some(tools) = make_iris_tools() else {
        return;
    };
    let v = parse_result(
        tools
            .call_for_test(
                "iris_system_performance",
                serde_json::json!({"mode": "bogus"}),
            )
            .await,
    );
    assert_eq!(
        v.get("success"),
        Some(&serde_json::json!(false)),
        "a mode outside the declared enum must fail: {v}"
    );
    let error = v
        .get("error")
        .and_then(|e| e.as_str())
        .unwrap_or_else(|| panic!("rejection must carry an `error` string: {v}"));
    assert!(
        error.contains("bogus"),
        "the error must quote the value the caller sent: {error}"
    );
    for value in ["start", "status", "last_runid"] {
        assert!(
            error.contains(value),
            "the error must list `{value}` among the valid modes so the caller can correct the \
             call without reading the docs: {error}"
        );
    }
}

/// `iris_admin` refuses through the dispatch table's catch-all arm, with its own error code. This is
/// the response the enum's twenty-five values are drawn from, so it has to keep saying INVALID_ACTION
/// and not degrade into a deserialization failure.
#[tokio::test]
async fn an_action_outside_the_enum_returns_invalid_action() {
    let Some(tools) = make_iris_tools() else {
        return;
    };
    let v = parse_result(
        tools
            .call_for_test("iris_admin", serde_json::json!({"action": "bogus"}))
            .await,
    );
    assert_eq!(
        v.get("error_code"),
        Some(&serde_json::json!("INVALID_ACTION")),
        "an unlisted action must come back as INVALID_ACTION: {v}"
    );
    let error = v
        .get("error")
        .and_then(|e| e.as_str())
        .unwrap_or_else(|| panic!("rejection must carry an `error` string: {v}"));
    assert!(
        error.contains("list_namespaces") && error.contains("mirror_failover"),
        "the error must enumerate the accepted actions: {error}"
    );
}

/// The other half of the same claim: a value *inside* the enum is not rejected. Without this, an
/// enum that accidentally excluded every working value would still pass the test above.
#[tokio::test]
async fn a_mode_inside_the_enum_is_not_refused_for_its_value() {
    let Some(tools) = make_iris_tools() else {
        return;
    };
    let v = parse_result(
        tools
            .call_for_test(
                "iris_system_performance",
                serde_json::json!({"mode": "last_runid"}),
            )
            .await,
    );
    let error = v.get("error").and_then(|e| e.as_str()).unwrap_or("");
    assert!(
        !error.contains("unknown mode"),
        "`last_runid` is in the declared enum and must not be refused as an unknown mode: {v}"
    );
}

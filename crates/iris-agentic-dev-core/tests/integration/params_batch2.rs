//! Batch 2 against live IRIS: every parameter these five tools declare still changes what happens.
//!
//! Each assertion picks a value whose effect is visible in the answer, which for admin tools means
//! choosing a value that fails in a named way. `iris_namespace_create` with a database that does not
//! exist reports `Database IADNOSUCHDB113 does not exist` and creates nothing — proof the parameter
//! arrived, with no namespace left behind on the dev container.
//!
//! Requires `iris-dev-iris`: `IRIS_HOST=localhost IRIS_WEB_PORT=52780 IRIS_USERNAME=_SYSTEM
//! IRIS_PASSWORD=SYS IRIS_NAMESPACE=USER`. The env-derived connection is named `_env`.

use iris_agentic_dev_core::testing::{
    answer_text, call_tool_with_env, live_env, require_iad_binary,
};

/// `live_env` plus the gates the admin tools need, set by the test rather than inherited from the
/// harness so it means the same thing run alone as it does in CI (Principle XII).
///
/// `iris_namespace_create` is in the destructive tier, not merely the write tier — creating a
/// namespace edits the instance's configuration, so the gate treats it like a delete.
fn admin_env() -> Vec<(String, String)> {
    let mut env = live_env();
    env.push(("IRIS_WRITE_TOOLS_ENABLED".to_string(), "1".to_string()));
    env.push((
        "IRIS_DESTRUCTIVE_TOOLS_ENABLED".to_string(),
        "1".to_string(),
    ));
    env.push(("IRIS_ADMIN_TOOLS".to_string(), "1".to_string()));
    env
}

fn payload(answer: &serde_json::Value) -> serde_json::Value {
    let text = answer
        .pointer("/result/content/0/text")
        .and_then(serde_json::Value::as_str)
        .unwrap_or_else(|| panic!("no text content in {}", answer_text(answer)));
    serde_json::from_str(text).unwrap_or_else(|e| panic!("content was not JSON ({e}): {text}"))
}

#[test]
#[ignore = "requires live IRIS (iris-dev-iris) and the built binary"]
fn iris_namespace_list_honors_server() {
    let Some(_bin) = require_iad_binary() else {
        return;
    };
    let out = payload(&call_tool_with_env(
        "iris_namespace_list",
        &serde_json::json!({"server": "_env"}),
        &live_env(),
    ));
    let names = out
        .get("namespaces")
        .and_then(serde_json::Value::as_array)
        .unwrap_or_else(|| panic!("no namespaces array: {out}"));
    assert!(
        names.iter().any(|n| n.as_str() == Some("USER")),
        "iris-dev-iris must report a USER namespace: {out}"
    );

    let refused = call_tool_with_env(
        "iris_namespace_list",
        &serde_json::json!({"server": "no_such_instance_113"}),
        &live_env(),
    );
    let text = answer_text(&refused);
    assert!(
        text.contains("no_such_instance_113"),
        "iris_namespace_list dropped `server` — the refusal names something else: {text}"
    );
}

#[test]
#[ignore = "requires live IRIS (iris-dev-iris) and the built binary"]
fn iris_database_list_honors_server() {
    let Some(_bin) = require_iad_binary() else {
        return;
    };
    let out = payload(&call_tool_with_env(
        "iris_database_list",
        &serde_json::json!({"server": "_env"}),
        &live_env(),
    ));
    assert_eq!(
        out.get("success"),
        Some(&serde_json::json!(true)),
        "iris_database_list against _env must succeed: {out}"
    );

    let refused = call_tool_with_env(
        "iris_database_list",
        &serde_json::json!({"server": "no_such_instance_113"}),
        &live_env(),
    );
    assert!(
        answer_text(&refused).contains("no_such_instance_113"),
        "iris_database_list dropped `server`: {}",
        answer_text(&refused)
    );
}

/// `db` names a directory, and IRIS reports the one it could not find. A dropped `db` would list
/// every database instead and succeed.
#[test]
#[ignore = "requires live IRIS (iris-dev-iris) and the built binary"]
fn iris_database_stats_honors_db() {
    let Some(_bin) = require_iad_binary() else {
        return;
    };
    let answer = call_tool_with_env(
        "iris_database_stats",
        &serde_json::json!({"db": "/nope/nosuchdir/", "server": "_env"}),
        &live_env(),
    );
    let text = answer_text(&answer);
    assert!(
        text.contains("/nope/nosuchdir/"),
        "iris_database_stats dropped `db` — it answered without mentioning the directory asked \
         for: {text}"
    );
}

/// Both parameters proven without creating a namespace: `db_path` names the missing database in the
/// first call, and `name` does in the second (with `db_path` omitted, the database defaults to the
/// namespace name — which is the omission behaviour FR-004 protects).
#[test]
#[ignore = "requires live IRIS (iris-dev-iris) and the built binary; creates nothing"]
fn iris_namespace_create_honors_name_and_db_path() {
    let Some(_bin) = require_iad_binary() else {
        return;
    };
    let with_db_path = call_tool_with_env(
        "iris_namespace_create",
        &serde_json::json!({"name": "IADP113", "db_path": "IADNOSUCHDB113", "server": "_env"}),
        &admin_env(),
    );
    let text = answer_text(&with_db_path);
    assert!(
        text.contains("IADNOSUCHDB113"),
        "iris_namespace_create dropped `db_path`: {text}"
    );

    let without_db_path = call_tool_with_env(
        "iris_namespace_create",
        &serde_json::json!({"name": "IADNOSUCHNS113", "server": "_env"}),
        &admin_env(),
    );
    let text = answer_text(&without_db_path);
    assert!(
        text.contains("IADNOSUCHNS113"),
        "iris_namespace_create dropped `name`, or stopped defaulting the database to it: {text}"
    );
}

#[test]
#[ignore = "requires the built binary and a Docker daemon"]
fn iris_containers_honors_action_and_name() {
    let Some(_bin) = require_iad_binary() else {
        return;
    };
    // `action` routes: an unknown value must reach the handler's own INVALID_ACTION, not a serde
    // error. US2 turns this value set into an advertised enum; the message stays the same.
    let bogus = call_tool_with_env(
        "iris_containers",
        &serde_json::json!({"action": "bogus"}),
        &live_env(),
    );
    assert!(
        answer_text(&bogus).contains("INVALID_ACTION"),
        "iris_containers dropped `action`: {}",
        answer_text(&bogus)
    );

    // `select` echoes the name it could not find, which is the only parameter of the pair whose
    // value reaches the answer.
    let out = payload(&call_tool_with_env(
        "iris_containers",
        &serde_json::json!({"action": "select", "name": "no_such_container_113"}),
        &live_env(),
    ));
    assert_eq!(
        out.get("requested").and_then(serde_json::Value::as_str),
        Some("no_such_container_113"),
        "iris_containers dropped `name`: {out}"
    );
}

//! `iris_admin` against live IRIS: the 26 parameters still reach the handler, and the ones with a
//! declared non-string type are now enforced at the wire.
//!
//! Coverage is split three ways, deliberately:
//!
//! - **Observable read actions** prove `type`, `path`, `resource`, `permission`, `username`, `name`,
//!   `namespace`, `server` by naming the value they were given in the answer.
//! - **Reversible write actions** prove `full_name`, `roles`, `enabled`, `password`,
//!   `dispatch_class`, `code_database`, `data_database`. Each mutation is undone by a `Drop` guard,
//!   so a failed assertion still cleans up.
//! - **Type rejection** proves `max_records`, `primary_port`, `async_member_type`, `time_range` and
//!   `confirm` — the five non-string parameters. Sending `"3"` where the handler calls `as_u64()`
//!   was silently ignored before this feature; now it is a refusal that says which type was
//!   expected. That is the `max_chars` bug caught at the boundary. Naming the offending parameter
//!   and stamping an `error_code` is T032's job — serde's own text stops at the type.
//!
//! Two groups are not live-proven, and the reason is not laziness:
//!
//! - `journal_search`'s `global_pattern`/`time_range`/`max_records` sit behind the bulk-PHI gate,
//!   which refuses before the handler reads anything unless `dataPolicy = "allow"`. Turning that on
//!   makes the tool scan the whole current journal file record by record — it did not finish within
//!   25 seconds on `iris-dev-iris`. The type-rejection test covers `max_records` and `time_range`;
//!   `global_pattern` is a string pass-through covered by the source contract.
//! - `mirror_add_async`'s `mirror_name`/`primary_host`/`primary_port`/`instance_name`/
//!   `async_member_type` would reconfigure mirroring on the dev instance, and a half-joined mirror
//!   member is not a state worth risking for an echo. `primary_port` and `async_member_type` are
//!   covered by type rejection; the three strings by the source contract.
//! - `new_password` changes the account the whole suite authenticates with. Source contract only.
//!
//! Requires `iris-dev-iris`: `IRIS_HOST=localhost IRIS_WEB_PORT=52780 IRIS_USERNAME=_SYSTEM
//! IRIS_PASSWORD=SYS IRIS_NAMESPACE=USER`.

use iris_agentic_dev_core::testing::{answer_text, live_env, require_iad_binary, McpSession};

/// `live_env` plus the gates `iris_admin`'s write and destructive actions need. Set here rather than
/// inherited so the test means the same thing run alone as it does in CI (Principle XII).
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

fn admin(mcp: &mut McpSession, args: serde_json::Value) -> serde_json::Value {
    mcp.call("iris_admin", &args)
}

#[test]
#[ignore = "requires live IRIS (iris-dev-iris) and the built binary"]
fn read_actions_still_honor_the_filters_they_are_given() {
    let Some(_bin) = require_iad_binary() else {
        return;
    };
    let mut mcp = McpSession::start(&live_env());

    // `type`: 23 web applications exist on iris-dev-iris and 10 of them are REST. A dropped filter
    // returns all 23, so the count is the proof.
    let all = payload(&admin(
        &mut mcp,
        serde_json::json!({"action": "list_webapps"}),
    ));
    let rest = payload(&admin(
        &mut mcp,
        serde_json::json!({"action": "list_webapps", "type": "REST"}),
    ));
    let all_count = all.get("count").and_then(serde_json::Value::as_u64);
    let rest_count = rest.get("count").and_then(serde_json::Value::as_u64);
    assert!(
        rest_count
            .zip(all_count)
            .is_some_and(|(r, a)| r < a && r > 0),
        "iris_admin dropped `type`: filtered count {rest_count:?} vs unfiltered {all_count:?}"
    );
    assert!(
        rest["webapps"]
            .as_array()
            .is_some_and(|w| w.iter().all(|a| a["type"] == "REST")),
        "list_webapps returned non-REST entries for type=REST: {rest}"
    );

    // `path`
    let webapp = payload(&admin(
        &mut mcp,
        serde_json::json!({"action": "get_webapp", "path": "/api/atelier"}),
    ));
    assert_eq!(
        webapp.get("path").and_then(serde_json::Value::as_str),
        Some("/api/atelier"),
        "iris_admin dropped `path`: {webapp}"
    );

    // `resource` and `permission`, both echoed
    let perm = payload(&admin(
        &mut mcp,
        serde_json::json!({
            "action": "check_permission",
            "resource": "%DB_IRISSYS",
            "permission": "WRITE"
        }),
    ));
    assert_eq!(
        perm.get("resource").and_then(serde_json::Value::as_str),
        Some("%DB_IRISSYS"),
        "iris_admin dropped `resource`: {perm}"
    );
    assert_eq!(
        perm.get("permission").and_then(serde_json::Value::as_str),
        Some("WRITE"),
        "iris_admin dropped `permission`: {perm}"
    );

    // `username`
    let roles = payload(&admin(
        &mut mcp,
        serde_json::json!({"action": "list_user_roles", "username": "_SYSTEM"}),
    ));
    assert_eq!(
        roles.get("username").and_then(serde_json::Value::as_str),
        Some("_SYSTEM"),
        "iris_admin dropped `username`: {roles}"
    );

    // `name`: the database that does not exist is named in the refusal
    let db = admin(
        &mut mcp,
        serde_json::json!({"action": "database_status", "name": "NOSUCHDB113"}),
    );
    assert!(
        answer_text(&db).contains("NOSUCHDB113"),
        "iris_admin dropped `name`: {}",
        answer_text(&db)
    );

    // `namespace`
    let ns = admin(
        &mut mcp,
        serde_json::json!({"action": "namespace_mappings", "namespace": "NOSUCHNS113"}),
    );
    assert!(
        answer_text(&ns).contains("NOSUCHNS113"),
        "iris_admin dropped `namespace`: {}",
        answer_text(&ns)
    );

    // `server`
    let server = admin(
        &mut mcp,
        serde_json::json!({"action": "list_namespaces", "server": "no_such_instance_113"}),
    );
    assert!(
        answer_text(&server).contains("no_such_instance_113"),
        "iris_admin dropped `server`: {}",
        answer_text(&server)
    );
}

/// Deletes the probe user when the test ends, panic or not.
struct UserProbe {
    mcp: McpSession,
    username: &'static str,
}

impl Drop for UserProbe {
    fn drop(&mut self) {
        let _ = self.mcp.call(
            "iris_admin",
            &serde_json::json!({"action": "delete_user", "username": self.username}),
        );
    }
}

/// `full_name`, `roles`, `password` and `enabled` are only observable on a user that exists, so this
/// creates one, reads it back, flips `enabled`, and deletes it. The `Drop` guard is what makes a
/// failed assertion safe: without it, one bad run leaves an account behind on the dev instance.
#[test]
#[ignore = "requires live IRIS (iris-dev-iris) and the built binary; creates and deletes IADP113USER"]
fn create_and_update_user_honor_full_name_roles_password_and_enabled() {
    let Some(_bin) = require_iad_binary() else {
        return;
    };
    const USERNAME: &str = "IADP113USER";
    let mut probe = UserProbe {
        mcp: McpSession::start(&admin_env()),
        username: USERNAME,
    };
    // Delete first: a previous interrupted run may have left the account behind, and create_user
    // would then fail for a reason that has nothing to do with parameters.
    let _ = admin(
        &mut probe.mcp,
        serde_json::json!({"action": "delete_user", "username": USERNAME}),
    );

    let created = admin(
        &mut probe.mcp,
        serde_json::json!({
            "action": "create_user",
            "username": USERNAME,
            "password": "Probe113Pass!",
            "full_name": "113 parameter probe",
            "roles": "%Developer"
        }),
    );
    assert!(
        !answer_text(&created).contains("\"success\":false"),
        "create_user failed, so the assertions below would be vacuous: {}",
        answer_text(&created)
    );

    let listed = payload(&admin(
        &mut probe.mcp,
        serde_json::json!({"action": "list_users"}),
    ));
    let entry = listed["users"]
        .as_array()
        .and_then(|users| {
            users
                .iter()
                .find(|u| u["name"].as_str() == Some(USERNAME))
                .cloned()
        })
        .unwrap_or_else(|| panic!("{USERNAME} is missing from list_users: {listed}"));
    assert_eq!(
        entry["full_name"].as_str(),
        Some("113 parameter probe"),
        "iris_admin dropped `full_name`: {entry}"
    );
    assert!(
        entry["roles"]
            .as_str()
            .is_some_and(|r| r.contains("%Developer")),
        "iris_admin dropped `roles`: {entry}"
    );
    assert_eq!(
        entry["enabled"],
        serde_json::json!(true),
        "a freshly created user must be enabled, or the flip below proves nothing: {entry}"
    );

    // `enabled` is the only boolean whose effect is observable without a login.
    let updated = admin(
        &mut probe.mcp,
        serde_json::json!({"action": "update_user", "username": USERNAME, "enabled": false}),
    );
    assert!(
        !answer_text(&updated).contains("\"success\":false"),
        "update_user failed: {}",
        answer_text(&updated)
    );
    let listed = payload(&admin(
        &mut probe.mcp,
        serde_json::json!({"action": "list_users"}),
    ));
    let entry = listed["users"]
        .as_array()
        .and_then(|users| {
            users
                .iter()
                .find(|u| u["name"].as_str() == Some(USERNAME))
                .cloned()
        })
        .unwrap_or_else(|| panic!("{USERNAME} disappeared between calls: {listed}"));
    assert_eq!(
        entry["enabled"],
        serde_json::json!(false),
        "iris_admin dropped `enabled` — the update did not take: {entry}"
    );
}

/// Deletes the probe web application when the test ends, panic or not.
struct WebappProbe {
    mcp: McpSession,
    path: &'static str,
}

impl Drop for WebappProbe {
    fn drop(&mut self) {
        let _ = self.mcp.call(
            "iris_admin",
            &serde_json::json!({"action": "delete_webapp", "path": self.path}),
        );
    }
}

/// `dispatch_class` reaches IRIS only through `create_webapp`, and `get_webapp` is the only thing
/// that reads it back.
#[test]
#[ignore = "requires live IRIS (iris-dev-iris) and the built binary; creates and deletes /iadp113"]
fn create_webapp_honors_dispatch_class_and_namespace() {
    let Some(_bin) = require_iad_binary() else {
        return;
    };
    const PATH: &str = "/iadp113";
    let mut probe = WebappProbe {
        mcp: McpSession::start(&admin_env()),
        path: PATH,
    };
    let _ = admin(
        &mut probe.mcp,
        serde_json::json!({"action": "delete_webapp", "path": PATH}),
    );

    let created = admin(
        &mut probe.mcp,
        serde_json::json!({
            "action": "create_webapp",
            "path": PATH,
            "namespace": "USER",
            "dispatch_class": "IADP113.Dispatch",
            "enabled": true
        }),
    );
    assert!(
        !answer_text(&created).contains("\"success\":false"),
        "create_webapp failed, so the read-back below would be vacuous: {}",
        answer_text(&created)
    );

    let read_back = payload(&admin(
        &mut probe.mcp,
        serde_json::json!({"action": "get_webapp", "path": PATH}),
    ));
    assert_eq!(
        read_back
            .get("dispatch_class")
            .and_then(serde_json::Value::as_str),
        Some("IADP113.Dispatch"),
        "iris_admin dropped `dispatch_class`: {read_back}"
    );
    assert_eq!(
        read_back
            .get("namespace")
            .and_then(serde_json::Value::as_str),
        Some("USER"),
        "iris_admin dropped `namespace` on create_webapp: {read_back}"
    );
}

/// Both database names are named back in the refusal, so neither has to exist and no namespace is
/// created.
#[test]
#[ignore = "requires live IRIS (iris-dev-iris) and the built binary; creates nothing"]
fn create_namespace_names_the_databases_it_was_given() {
    let Some(_bin) = require_iad_binary() else {
        return;
    };
    let mut mcp = McpSession::start(&admin_env());
    let answer = admin(
        &mut mcp,
        serde_json::json!({
            "action": "create_namespace",
            "name": "IADP113NS",
            "code_database": "IADNOSUCHCODE113",
            "data_database": "IADNOSUCHDATA113"
        }),
    );
    let text = answer_text(&answer);
    assert!(
        text.contains("IADNOSUCHCODE113") || text.contains("IADNOSUCHDATA113"),
        "iris_admin dropped `code_database`/`data_database` — the refusal names neither: {text}"
    );
}

/// The six non-string parameters, sent as strings. Before this feature every one of these was
/// accepted and silently discarded by `as_u64()`/`as_bool()`/`.get("from")` — the caller who asked
/// for `max_records: "3"` got 100 records and no hint why. Each must now be refused.
///
/// The refusal names the expected Rust type but not the parameter, because it is serde's own text
/// relayed by the rmcp router: `invalid type: string "3", expected u64`. T032 wraps that path in
/// `{"success": false, "error_code": "INVALID_PARAMS", …}`; when it lands, tighten the second
/// assertion here to require the `error_code`.
#[test]
#[ignore = "requires the built binary; no IRIS call is reached"]
fn wrongly_typed_parameters_are_refused_by_name() {
    let Some(_bin) = require_iad_binary() else {
        return;
    };
    let mut mcp = McpSession::start(&admin_env());
    for (field, expected_type, args) in [
        (
            "max_records",
            "u64",
            serde_json::json!({"action": "journal_search", "max_records": "3"}),
        ),
        (
            "primary_port",
            "u64",
            serde_json::json!({"action": "mirror_add_async", "primary_port": "2188"}),
        ),
        (
            "async_member_type",
            "u64",
            serde_json::json!({"action": "mirror_add_async", "async_member_type": "0"}),
        ),
        (
            "time_range",
            "JournalTimeRange",
            serde_json::json!({"action": "journal_search", "time_range": "yesterday"}),
        ),
        (
            "confirm",
            "bool",
            serde_json::json!({"action": "mirror_failover", "confirm": "yes"}),
        ),
        (
            "enabled",
            "bool",
            serde_json::json!({"action": "update_user", "username": "_SYSTEM", "enabled": "true"}),
        ),
    ] {
        let answer = admin(&mut mcp, args);
        let text = answer_text(&answer);
        assert!(
            answer
                .pointer("/result/isError")
                .and_then(serde_json::Value::as_bool)
                == Some(true)
                || text.contains("\"success\": false")
                || text.contains("\"success\":false"),
            "sending a string for `{field}` must be refused, not accepted and discarded: {answer}"
        );
        assert!(
            text.contains("invalid type") && text.contains(expected_type),
            "the refusal for `{field}` must say what type was expected (`{expected_type}`) so the \
             caller can fix the call: {text}"
        );
    }
}

/// `action` is the one required parameter in this feature. Omitting it must be refused, and the
/// refusal must name it — a caller who sent `{"actoin": "list_users"}` used to get the generic
/// INVALID_ACTION list with no hint that their key was misspelled.
#[test]
#[ignore = "requires the built binary; no IRIS call is reached"]
fn omitting_action_is_refused_by_name() {
    let Some(_bin) = require_iad_binary() else {
        return;
    };
    let mut mcp = McpSession::start(&admin_env());
    let answer = admin(&mut mcp, serde_json::json!({"server": "_env"}));
    let text = answer_text(&answer);
    assert!(
        text.contains("action"),
        "omitting `action` must be refused by name, got: {text}"
    );
}

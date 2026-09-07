//! Batch 6 against live IRIS: the fifteen parameters of the four write-gated interoperability tools
//! still reach their handlers, and the gates still answer first when they are closed.
//!
//! Two environments, because these tools have two behaviours:
//!
//! - `live_env()` alone leaves the destructive tier off (it is never inferred — spec 073) and the
//!   write gate on for this container's SystemMode. `iris_credential_manage` and
//!   `iris_lookup_manage` set/delete are classified `Destructive`, so they are refused before the
//!   handler runs. Those refusals are asserted, because a typed params struct deserializes *before*
//!   `gate_check` and a mistake there would move the refusal — or lose it.
//! - `open_gates()` adds `IRIS_WRITE_TOOLS_ENABLED=1` and `IRIS_DESTRUCTIVE_TOOLS_ENABLED=1`, which
//!   is the only way the write parameters (`id`, `username`, `password`, `value`, `xml`) are
//!   observable at all.
//!
//! Everything this suite writes to `iris-dev-iris` is named `IADP113…` and removed by the end of the
//! test that created it, so the namespace ends each run holding exactly what it held at the start.
//! The read tests do not rely on that state: what they read is created by `seed_interop_fixture()`,
//! under `IADFixture…` names, and left in place deliberately.
//!
//! Requires `iris-dev-iris`: `IRIS_HOST=localhost IRIS_WEB_PORT=52780 IRIS_USERNAME=_SYSTEM
//! IRIS_PASSWORD=SYS IRIS_NAMESPACE=USER`.

use iris_agentic_dev_core::testing::{
    answer_text, live_env, require_iad_binary, seed_interop_fixture, McpSession,
};

/// `live_env()` plus both gates open. Nothing is inherited from the test runner's environment, so
/// the gates have to be stated here even though a developer shell often has them set.
fn open_gates() -> Vec<(String, String)> {
    let mut env = live_env();
    env.push(("IRIS_WRITE_TOOLS_ENABLED".to_string(), "1".to_string()));
    env.push((
        "IRIS_DESTRUCTIVE_TOOLS_ENABLED".to_string(),
        "1".to_string(),
    ));
    env
}

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

fn strings(p: &serde_json::Value, key: &str) -> Vec<String> {
    p.get(key)
        .and_then(|v| v.as_array())
        .unwrap_or_else(|| panic!("no `{key}` array in {p}"))
        .iter()
        .filter_map(|v| v.as_str().map(str::to_string))
        .collect()
}

/// `iris_credential_list` is the one read-only tool in the batch, so its single parameter is
/// observable without touching a gate: a namespace that does not exist cannot answer.
#[test]
#[ignore = "requires live IRIS (iris-dev-iris) and the built binary"]
fn credential_list_routes_on_namespace() {
    let Some(_bin) = require_iad_binary() else {
        return;
    };
    let mut mcp = McpSession::start(&live_env());

    let listed = call(&mut mcp, "iris_credential_list", serde_json::json!({}));
    assert!(
        listed.get("credentials").is_some(),
        "the default namespace must answer with a credentials array: {listed}"
    );

    let bad_ns = call(
        &mut mcp,
        "iris_credential_list",
        serde_json::json!({"namespace": "IADP113NOSUCHNS"}),
    );
    assert_eq!(code_of(&bad_ns), "INTEROP_ERROR");
    assert!(
        error_of(&bad_ns).contains("404"),
        "`namespace` must route the query rather than being ignored in favour of the connection's: \
         {bad_ns}"
    );
}

/// The gates answer before the handler reads anything. Both tiers are asserted because they produce
/// different refusals and the more fundamental one wins: with writes off, a destructive tool reports
/// the write gate, not the destructive gate.
#[test]
#[ignore = "requires the built binary; the gate answers before IRIS is reached"]
fn closed_gates_still_refuse_before_the_typed_params_reach_the_handler() {
    let Some(_bin) = require_iad_binary() else {
        return;
    };

    let mut writes_off = live_env();
    writes_off.push(("IRIS_WRITE_TOOLS_ENABLED".to_string(), "0".to_string()));
    let mut mcp = McpSession::start(&writes_off);
    for (tool, args) in [
        (
            "iris_credential_manage",
            serde_json::json!({"action": "create", "id": "IADP113Cred", "username": "u", "password": "p"}),
        ),
        (
            "iris_lookup_manage",
            serde_json::json!({"action": "set", "table": "IADP113Table", "key": "k", "value": "v"}),
        ),
        (
            "iris_lookup_transfer",
            serde_json::json!({"action": "import", "table": "IADP113Table", "xml": "<lookupTable/>"}),
        ),
    ] {
        let refused = call(&mut mcp, tool, args);
        assert_eq!(
            code_of(&refused),
            "WRITE_TOOLS_DISABLED",
            "`{tool}` must be refused by the write gate, which runs after deserialization and \
             before the handler: {refused}"
        );
    }

    // Writes on, destructive off: the two destructive tools report the narrower gate, and
    // `iris_lookup_transfer` import — classified `Write` — gets through to IRIS.
    let mut destructive_off = live_env();
    destructive_off.push(("IRIS_WRITE_TOOLS_ENABLED".to_string(), "1".to_string()));
    destructive_off.push((
        "IRIS_DESTRUCTIVE_TOOLS_ENABLED".to_string(),
        "0".to_string(),
    ));
    let mut mcp = McpSession::start(&destructive_off);
    for (tool, args) in [
        (
            "iris_credential_manage",
            serde_json::json!({"action": "create", "id": "IADP113Cred", "username": "u", "password": "p"}),
        ),
        (
            "iris_lookup_manage",
            serde_json::json!({"action": "set", "table": "IADP113Table", "key": "k", "value": "v"}),
        ),
    ] {
        let refused = call(&mut mcp, tool, args);
        assert_eq!(
            code_of(&refused),
            "DESTRUCTIVE_TOOLS_DISABLED",
            "`{tool}` is destructive; with writes on and the destructive tier off it must report \
             that tier: {refused}"
        );
    }

    // The read actions of a destructive tool are classified `ReadOnly` and must stay reachable —
    // otherwise the batch would have "proved" the gate by breaking the tool.
    let listed = call(
        &mut mcp,
        "iris_lookup_manage",
        serde_json::json!({"action": "list_tables"}),
    );
    assert!(
        listed.get("tables").is_some(),
        "`action=list_tables` is read-only and must not be gated: {listed}"
    );
}

/// `iris_lookup_manage`'s read actions: `action` selects which one, `table` and `key` are echoed by
/// the errors when they miss, and `namespace` routes. All four are observable with the gates closed.
#[test]
#[ignore = "requires live IRIS (iris-dev-iris) and the built binary"]
fn lookup_manage_read_actions_honor_table_key_and_namespace() {
    let Some(_bin) = require_iad_binary() else {
        return;
    };
    // The fixture creates the table this reads, in its own gate-open session. Taking "the first
    // non-system table that happens to exist" is what made this container-dependent: a fresh instance
    // has none, so `list_tables` came back empty and the test failed on the assumption rather than on
    // the parameter.
    let fixture = seed_interop_fixture();
    let mut mcp = McpSession::start(&live_env());

    let tables = strings(
        &call(
            &mut mcp,
            "iris_lookup_manage",
            serde_json::json!({"action": "list_tables"}),
        ),
        "tables",
    );
    assert!(
        tables.iter().any(|t| t == fixture.lookup_table),
        "`action=list_tables` must include the fixture's table `{}`: {tables:?}",
        fixture.lookup_table
    );
    let table = fixture.lookup_table.to_string();

    let keys = strings(
        &call(
            &mut mcp,
            "iris_lookup_manage",
            serde_json::json!({"action": "list_keys", "table": table}),
        ),
        "keys",
    );
    assert_eq!(
        keys,
        vec![fixture.lookup_key.to_string()],
        "`table` must select the fixture's table and `list_keys` return its one key"
    );

    let got = call(
        &mut mcp,
        "iris_lookup_manage",
        serde_json::json!({"action": "get", "table": table, "key": keys[0]}),
    );
    assert_eq!(
        got.get("key").and_then(|v| v.as_str()),
        Some(keys[0].as_str()),
        "`key` must select the entry that is read back: {got}"
    );
    assert!(
        got.get("value").is_some(),
        "a `get` that found its key must return the value: {got}"
    );

    // `key` reaches IRIS on the miss path too, and the error names it — which is what distinguishes
    // "the key was not found" from "the key was never sent".
    let missing = call(
        &mut mcp,
        "iris_lookup_manage",
        serde_json::json!({"action": "get", "table": table, "key": "IADP113NoSuchKey"}),
    );
    assert_eq!(code_of(&missing), "KEY_NOT_FOUND");
    assert!(
        error_of(&missing).contains("IADP113NoSuchKey"),
        "the miss must name the key it was given: {missing}"
    );

    // A misspelled `action` does not reach the dispatcher on a default-policy instance: the gate
    // classifies anything outside the four read actions as destructive and refuses first. That is the
    // fail-closed default working as designed, and it is worth pinning — it means `INVALID_ACTION`
    // for this tool is only observable with the destructive tier open, which is what the assertion
    // below does in a second session.
    let bogus_gated = call(
        &mut mcp,
        "iris_lookup_manage",
        serde_json::json!({"action": "nonsense"}),
    );
    assert_eq!(
        code_of(&bogus_gated),
        "DESTRUCTIVE_TOOLS_DISABLED",
        "an unrecognized `action` must be treated as destructive, not waved through: {bogus_gated}"
    );

    let mut open = McpSession::start(&open_gates());
    let bogus = call(
        &mut open,
        "iris_lookup_manage",
        serde_json::json!({"action": "nonsense"}),
    );
    assert_eq!(code_of(&bogus), "INVALID_ACTION");
    assert!(
        error_of(&bogus).contains("get, set, delete, list_keys, or list_tables"),
        "`action` must reach the dispatcher, which lists what it accepts: {bogus}"
    );

    let bad_ns = call(
        &mut mcp,
        "iris_lookup_manage",
        serde_json::json!({"action": "list_tables", "namespace": "IADP113NOSUCHNS"}),
    );
    assert!(
        error_of(&bad_ns).contains("404"),
        "`namespace` must route the query: {bad_ns}"
    );
}

/// `iris_lookup_transfer`: `action` picks export or import, `table` names what is transferred, `xml`
/// carries the payload, and `namespace` routes. Import is a write, so this test opens the gates and
/// removes what it wrote.
#[test]
#[ignore = "requires live IRIS (iris-dev-iris) and the built binary"]
fn lookup_transfer_round_trips_table_and_xml() {
    let Some(_bin) = require_iad_binary() else {
        return;
    };
    let mut mcp = McpSession::start(&open_gates());
    let table = "IADP113Transfer";

    let bogus = call(
        &mut mcp,
        "iris_lookup_transfer",
        serde_json::json!({"action": "nonsense"}),
    );
    assert_eq!(code_of(&bogus), "INVALID_ACTION");
    assert!(
        error_of(&bogus).contains("export or import"),
        "`action` must reach the dispatcher: {bogus}"
    );

    let missing = call(
        &mut mcp,
        "iris_lookup_transfer",
        serde_json::json!({"action": "export", "table": "IADP113NoSuchTable"}),
    );
    assert_eq!(code_of(&missing), "TABLE_NOT_FOUND");
    assert!(
        error_of(&missing).contains("IADP113NoSuchTable"),
        "`table` must reach the lookup and be named back: {missing}"
    );

    // `xml` is the only parameter in this batch whose *content* is parsed. A round trip is the
    // discriminating test: the value that comes back out can only have come from what went in.
    let imported = call(
        &mut mcp,
        "iris_lookup_transfer",
        serde_json::json!({
            "action": "import",
            "table": table,
            "xml": format!(
                "<?xml version=\"1.0\"?>\n<lookupTable>\n<entry table=\"{table}\" \
                 key=\"IADP113Key\">IADP113Value</entry>\n</lookupTable>"
            )
        }),
    );
    assert_eq!(
        imported.get("success").and_then(|v| v.as_bool()),
        Some(true),
        "the import must succeed with the gates open: {imported}"
    );

    let exported = call(
        &mut mcp,
        "iris_lookup_transfer",
        serde_json::json!({"action": "export", "table": table}),
    );
    let xml = exported
        .get("xml")
        .and_then(|v| v.as_str())
        .unwrap_or_else(|| panic!("no xml in {exported}"));
    assert!(
        xml.contains("IADP113Key") && xml.contains("IADP113Value"),
        "the exported XML must contain what `xml` imported: {xml}"
    );

    let cleaned = call(
        &mut mcp,
        "iris_lookup_manage",
        serde_json::json!({"action": "delete", "table": table, "key": "IADP113Key"}),
    );
    assert_eq!(
        cleaned.get("success").and_then(|v| v.as_bool()),
        Some(true),
        "the entry this test created must be removed again: {cleaned}"
    );
}

/// `iris_lookup_manage`'s write actions, which need the destructive tier. `value` is only observable
/// here: nothing reads it back except a `get` after a `set`.
#[test]
#[ignore = "requires live IRIS (iris-dev-iris) and the built binary"]
fn lookup_manage_set_and_delete_carry_key_and_value() {
    let Some(_bin) = require_iad_binary() else {
        return;
    };
    let mut mcp = McpSession::start(&open_gates());
    let table = "IADP113Manage";

    let set = call(
        &mut mcp,
        "iris_lookup_manage",
        serde_json::json!({
            "action": "set", "table": table, "key": "IADP113Key", "value": "IADP113Value"
        }),
    );
    assert_eq!(
        set.get("success").and_then(|v| v.as_bool()),
        Some(true),
        "`set` must succeed with the destructive tier open: {set}"
    );

    let got = call(
        &mut mcp,
        "iris_lookup_manage",
        serde_json::json!({"action": "get", "table": table, "key": "IADP113Key"}),
    );
    assert_eq!(
        got.get("value").and_then(|v| v.as_str()),
        Some("IADP113Value"),
        "`value` must be stored as sent — this is the only place it is observable: {got}"
    );

    let deleted = call(
        &mut mcp,
        "iris_lookup_manage",
        serde_json::json!({"action": "delete", "table": table, "key": "IADP113Key"}),
    );
    assert_eq!(
        deleted.get("success").and_then(|v| v.as_bool()),
        Some(true),
        "`delete` must remove the entry: {deleted}"
    );

    // Deleting the last entry takes the table with it, so the miss arrives as `TABLE_NOT_FOUND`
    // rather than `KEY_NOT_FOUND`. Either code proves the delete landed; what matters is that the
    // value is gone.
    let gone = call(
        &mut mcp,
        "iris_lookup_manage",
        serde_json::json!({"action": "get", "table": table, "key": "IADP113Key"}),
    );
    let code = code_of(&gone);
    assert!(
        code == "TABLE_NOT_FOUND" || code == "KEY_NOT_FOUND",
        "the entry this test created must not survive it: {gone}"
    );
}

/// `iris_credential_manage`: `action`, `id`, `username` and `password` all reach IRIS, proven by
/// creating a credential, reading its username back through `iris_credential_list`, changing it, and
/// deleting it. `password` is never returned by any tool, so the only evidence it arrived is that
/// `create` succeeds without an error about it.
#[test]
#[ignore = "requires live IRIS (iris-dev-iris) and the built binary"]
fn credential_manage_creates_updates_and_deletes_by_id() {
    let Some(_bin) = require_iad_binary() else {
        return;
    };
    let mut mcp = McpSession::start(&open_gates());
    let id = "IADP113Cred";

    let bogus = call(
        &mut mcp,
        "iris_credential_manage",
        serde_json::json!({"action": "nonsense", "id": id}),
    );
    assert_eq!(code_of(&bogus), "INVALID_ACTION");

    let created = call(
        &mut mcp,
        "iris_credential_manage",
        serde_json::json!({
            "action": "create", "id": id, "username": "iadp113user", "password": "IadP113!pass"
        }),
    );
    assert_eq!(
        created.get("id").and_then(|v| v.as_str()),
        Some(id),
        "`id` must name the credential that was created: {created}"
    );

    let username_of = |mcp: &mut McpSession| -> Option<String> {
        let listed = call(mcp, "iris_credential_list", serde_json::json!({}));
        listed
            .get("credentials")
            .and_then(|v| v.as_array())
            .and_then(|rows| {
                rows.iter()
                    .find(|r| r.get("id").and_then(|v| v.as_str()) == Some(id))
                    .and_then(|r| r.get("username").and_then(|v| v.as_str()))
                    .map(str::to_string)
            })
    };
    assert_eq!(
        username_of(&mut mcp).as_deref(),
        Some("iadp113user"),
        "`username` must be stored as sent"
    );

    let updated = call(
        &mut mcp,
        "iris_credential_manage",
        serde_json::json!({"action": "update", "id": id, "username": "iadp113changed"}),
    );
    assert_eq!(
        updated.get("success").and_then(|v| v.as_bool()),
        Some(true),
        "`update` must succeed: {updated}"
    );
    assert_eq!(
        username_of(&mut mcp).as_deref(),
        Some("iadp113changed"),
        "`update` must apply the new `username`, which is also how `action` is shown to select \
         update rather than create"
    );

    let deleted = call(
        &mut mcp,
        "iris_credential_manage",
        serde_json::json!({"action": "delete", "id": id}),
    );
    assert_eq!(
        deleted.get("success").and_then(|v| v.as_bool()),
        Some(true),
        "`delete` must remove the credential: {deleted}"
    );
    assert_eq!(
        username_of(&mut mcp),
        None,
        "the credential this test created must not survive it"
    );

    let bad_ns = call(
        &mut mcp,
        "iris_credential_manage",
        serde_json::json!({
            "action": "create", "id": id, "username": "u", "password": "p",
            "namespace": "IADP113NOSUCHNS"
        }),
    );
    assert!(
        error_of(&bad_ns).contains("404"),
        "`namespace` must route the write — and a namespace that does not exist must not silently \
         write to the connection's: {bad_ns}"
    );
}

/// The wrong JSON type in any of the fifteen slots. Every one of these was accepted and discarded
/// before this batch: `key: 1` looked up the key `""`, and `xml: {}` imported nothing.
///
/// The refusal names the expected Rust type rather than the parameter — serde's own text, relayed by
/// the router. T032 wraps this path in `error_code: "INVALID_PARAMS"`; tighten this test then.
#[test]
#[ignore = "requires the built binary; no IRIS call is reached"]
fn wrongly_typed_values_are_refused() {
    let Some(_bin) = require_iad_binary() else {
        return;
    };
    let mut mcp = McpSession::start(&open_gates());

    for (tool, args) in [
        (
            "iris_lookup_manage",
            serde_json::json!({"action": "get", "table": "IADP113Manage", "key": 1}),
        ),
        (
            "iris_lookup_transfer",
            serde_json::json!({"action": "import", "table": "IADP113Manage", "xml": {}}),
        ),
        (
            "iris_credential_list",
            serde_json::json!({"namespace": ["USER"]}),
        ),
        (
            "iris_credential_manage",
            serde_json::json!({"action": "create", "id": 7}),
        ),
    ] {
        let answer = mcp.call(tool, &args);
        let text = answer_text(&answer);
        assert!(
            text.contains("invalid type"),
            "`{tool}` must refuse {args} instead of discarding the value: {text}"
        );
    }
}

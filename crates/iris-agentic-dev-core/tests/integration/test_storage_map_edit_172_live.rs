//! T065: `iris_storage_map check` compiles a proposed map in a scratch copy and never touches
//! the real class (spec 172 US4: "a map that fails to compile leaves the real class and table
//! working").
//!
//! Run: `IRIS_HOST=localhost IRIS_WEB_PORT=52780 cargo test --features testing --test integration \
//!   test_storage_map_edit_172 -- --test-threads=1 --include-ignored`

use crate::live_rig_171::{rig, Rig, Where};
use crate::storage172_rig::{ask, drop_fixture, load_fixture, Fixture};
use iris_agentic_dev_core::tools::Toolset;
use serde_json::{json, Value};

const CLASS: &str = "IadSm172.Member";

async fn source(r: &Rig) -> Value {
    let got = r
        .call(
            "iris_doc",
            json!({"mode": "get", "name": format!("{CLASS}.cls"), "namespace": "USER"}),
        )
        .await;
    assert_eq!(got["success"], true, "{got}");
    got["content"].clone()
}

async fn exists(r: &Rig, class: &str) -> bool {
    r.call(
        "iris_doc",
        json!({"mode": "get", "name": format!("{class}.cls"), "namespace": "USER"}),
    )
    .await["success"]
        == true
}

async fn master(r: &Rig) -> Value {
    let read = r
        .call(
            "iris_storage_map",
            json!({"action": "read", "class": CLASS, "namespace": "USER"}),
        )
        .await;
    assert_eq!(read["success"], true, "{read}");
    read["storage"]["maps"][0].clone()
}

#[tokio::test]
#[ignore = "live IRIS"]
async fn test_storage_map_edit_172_check_failure_is_explained_and_leaves_the_class() {
    let r = rig(&Where::live(), Toolset::Merged).await;
    let f = Fixture::read("Member");
    load_fixture(&r, &f).await;
    let before = source(&r).await;
    let mut bad = master(&r).await;
    // A field the class does not have passes the FR-014 rules and fails in IRIS.
    bad["data"]["Nobody"] = json!({"delimiter": "\"^\"", "piece": "4"});

    let got = r
        .call(
            "iris_storage_map",
            json!({"action": "check", "class": CLASS, "map": "Master", "model": bad,
                   "namespace": "USER"}),
        )
        .await;
    let after = source(&r).await;
    let rows = ask(&r, &f).await;
    let left = match got["scratch_class"].as_str() {
        Some(c) => exists(&r, c).await,
        None => false,
    };
    let problems = drop_fixture(&r, &f).await;

    assert_eq!(got["success"], false, "{got}");
    assert_eq!(got["error_code"], "STORAGE_MAP_COMPILE_FAILED", "{got}");
    let ex = &got["explanations"][0];
    assert!(ex["code"].is_u64(), "no explained IRIS error: {got}");
    assert!(
        got["iris_errors"]
            .to_string()
            .contains(&format!("#{}", ex["code"])),
        "{got}"
    );
    assert!(
        ex["text"].as_str().unwrap_or_default().contains("field"),
        "{got}"
    );
    assert!(!left, "the scratch class stayed: {got}");
    assert_eq!(after, before, "check changed the real class");
    let expected = f.seed["answer"].as_array().cloned().unwrap_or_default();
    assert_eq!(
        rows, expected,
        "the real class no longer answers its question"
    );
    assert!(problems.is_empty(), "cleanup: {problems:?}");
}

#[tokio::test]
#[ignore = "live IRIS"]
async fn test_storage_map_edit_172_check_success_leaves_no_scratch_class() {
    let r = rig(&Where::live(), Toolset::Merged).await;
    let f = Fixture::read("Member");
    load_fixture(&r, &f).await;
    let before = source(&r).await;
    let mut good = master(&r).await;
    // A real change: Age moves to piece 4, which compiles.
    good["data"]["Age"]["piece"] = json!("4");

    let with_model = r
        .call(
            "iris_storage_map",
            json!({"action": "check", "class": CLASS, "map": "Master", "model": good,
                   "namespace": "USER"}),
        )
        .await;
    let stored = r
        .call(
            "iris_storage_map",
            json!({"action": "check", "class": CLASS, "namespace": "USER"}),
        )
        .await;
    let mut left = Vec::new();
    for got in [&with_model, &stored] {
        if let Some(c) = got["scratch_class"].as_str() {
            if exists(&r, c).await {
                left.push(c.to_string());
            }
        }
    }
    let after = source(&r).await;
    let rows = ask(&r, &f).await;
    let problems = drop_fixture(&r, &f).await;

    for got in [&with_model, &stored] {
        assert_eq!(got["success"], true, "{got}");
        assert_eq!(got["action"], "check", "{got}");
        assert_eq!(got["compiled"], true, "{got}");
        assert!(
            got["scratch_class"]
                .as_str()
                .is_some_and(|c| c.starts_with("IrisDevTmp.Sm")),
            "{got}"
        );
    }
    assert!(left.is_empty(), "scratch classes stayed: {left:?}");
    assert_eq!(after, before, "check changed the real class");
    let expected = f.seed["answer"].as_array().cloned().unwrap_or_default();
    assert_eq!(rows, expected);
    assert!(problems.is_empty(), "cleanup: {problems:?}");
}

#[tokio::test]
#[ignore = "live IRIS"]
async fn test_storage_map_edit_172_check_refuses_bad_params() {
    let r = rig(&Where::live(), Toolset::Merged).await;
    let f = Fixture::read("Member");
    load_fixture(&r, &f).await;
    let good = master(&r).await;
    let mut unknown = good.clone();
    unknown["wibble"] = json!("x");

    let mismatch = r
        .call(
            "iris_storage_map",
            json!({"action": "check", "class": CLASS, "map": "Other", "model": good,
                   "namespace": "USER"}),
        )
        .await;
    let bad_key = r
        .call(
            "iris_storage_map",
            json!({"action": "check", "class": CLASS, "model": unknown, "namespace": "USER"}),
        )
        .await;
    let mut data_upper = good.clone();
    data_upper["type"] = json!("DATA");
    let invalid = r
        .call(
            "iris_storage_map",
            json!({"action": "check", "class": CLASS, "model": data_upper, "namespace": "USER"}),
        )
        .await;
    let default_storage = {
        let book = Fixture::read("DefaultBook");
        load_fixture(&r, &book).await;
        let got = r
            .call(
                "iris_storage_map",
                json!({"action": "check", "class": book.class, "namespace": "USER"}),
            )
            .await;
        drop_fixture(&r, &book).await;
        got
    };
    let problems = drop_fixture(&r, &f).await;

    assert_eq!(mismatch["error_code"], "INVALID_PARAMS", "{mismatch}");
    // `DATA` is refused by the FR-014 rules before any compile (R4: the type is case-sensitive).
    assert_eq!(invalid["error_code"], "STORAGE_MAP_INVALID", "{invalid}");
    assert_eq!(invalid["rules"][0]["rule"], "map_type", "{invalid}");
    assert_eq!(bad_key["error_code"], "INVALID_PARAMS", "{bad_key}");
    assert!(bad_key["error"].to_string().contains("wibble"), "{bad_key}");
    assert_eq!(
        default_storage["error_code"], "STORAGE_MAP_READ_ONLY",
        "{default_storage}"
    );
    assert!(problems.is_empty(), "cleanup: {problems:?}");
}

/// T066: under `read_only = "strict"` every action that compiles, writes or samples is refused
/// with `READ_ONLY_STRICT`, and `read`, `diagram` and `uses` still answer.
#[tokio::test]
#[ignore = "live IRIS"]
async fn test_storage_map_edit_172_strict_refuses_writes_and_keeps_reads() {
    use crate::live_rig_171::rig_with;
    let open = rig(&Where::live(), Toolset::Merged).await;
    let f = Fixture::read("Member");
    load_fixture(&open, &f).await;
    let map = master(&open).await;
    let version = map["version"].clone();
    let strict = rig_with(
        &Where::live(),
        Toolset::Merged,
        "write_tools_enabled = true\ndestructive_tools_enabled = true\nread_only = \"strict\"\n",
    )
    .await;

    let refused = [
        json!({"action": "check", "class": CLASS, "namespace": "USER"}),
        json!({"action": "edit", "class": CLASS, "version": version, "namespace": "USER",
               "ops": [{"op": "set", "path": "/maps/Master/data/Age/piece", "value": "4"}]}),
        json!({"action": "replace", "class": CLASS, "map": "Master", "version": version,
               "model": map, "namespace": "USER"}),
        json!({"action": "preview", "class": CLASS, "map": "Master", "model": map,
               "namespace": "USER"}),
        json!({"action": "compare", "class": CLASS, "map": "Master", "namespace": "USER"}),
        json!({"action": "propose", "global": "^IadSm172Member", "namespace": "USER"}),
    ];
    let answered = [
        json!({"action": "read", "class": CLASS, "namespace": "USER"}),
        json!({"action": "diagram", "class": CLASS, "namespace": "USER"}),
        json!({"action": "uses", "global": "^IadSm172Member", "namespace": "USER"}),
    ];
    let mut failures = Vec::new();
    for args in refused {
        let got = strict.call("iris_storage_map", args.clone()).await;
        if got["error_code"] != "READ_ONLY_STRICT" {
            failures.push(format!("{}: not refused: {got}", args["action"]));
        }
    }
    for args in answered {
        let got = strict.call("iris_storage_map", args.clone()).await;
        if got["success"] != true {
            failures.push(format!("{}: did not answer: {got}", args["action"]));
        }
    }
    let problems = drop_fixture(&open, &f).await;
    assert!(failures.is_empty(), "{}", failures.join("\n"));
    assert!(problems.is_empty(), "cleanup: {problems:?}");
}

// ── T072–T075: edit and replace write the real class ─────────────────────────

/// The stored text of `class`, as IRIS returns it.
async fn stored_text(r: &Rig, class: &str) -> String {
    let got = r
        .call(
            "iris_doc",
            json!({"mode": "get", "name": format!("{class}.cls"), "namespace": "USER"}),
        )
        .await;
    assert_eq!(got["success"], true, "{got}");
    match &got["content"] {
        Value::String(s) => s.clone(),
        Value::Array(a) => a
            .iter()
            .filter_map(Value::as_str)
            .collect::<Vec<_>>()
            .join("\n"),
        other => panic!("{other}"),
    }
}

async fn read_storage(r: &Rig, class: &str) -> Value {
    let read = r
        .call(
            "iris_storage_map",
            json!({"action": "read", "class": class, "namespace": "USER"}),
        )
        .await;
    assert_eq!(read["success"], true, "{read}");
    read["storage"].clone()
}

/// Put `text` as `class` and compile it, through iris_doc.
async fn put_class(r: &Rig, class: &str, text: &str) {
    let put = r
        .call(
            "iris_doc",
            json!({"mode": "put", "name": format!("{class}.cls"), "content": text,
                   "compile": true, "namespace": "USER"}),
        )
        .await;
    assert_eq!(put["success"], true, "put {class}: {put}");
}

/// T072: `guarded_put` refuses a write when the class changed after iad read it.
#[tokio::test]
#[ignore = "live IRIS"]
async fn test_storage_map_edit_172_guarded_put_refuses_a_stale_timestamp() {
    use iris_agentic_dev_core::tools::storage_map::write::{get_doc, guarded_put};
    let r = rig(&Where::live(), Toolset::Merged).await;
    let f = Fixture::read("Member");
    load_fixture(&r, &f).await;
    let (conn, client) = live_conn().await;
    let doc = format!("{CLASS}.cls");
    let (text, ts) = get_doc(&conn, &client, &doc, "USER")
        .await
        .unwrap()
        .expect("Member is there");
    // Another client changes the class after iad's GET.
    tokio::time::sleep(std::time::Duration::from_millis(1100)).await;
    let other = text.replacen(
        "Property Age As %Integer;",
        "Property Age As %Integer;\n\nParameter IADPROBE = 1;",
        1,
    );
    put_class(&r, CLASS, &other).await;
    let stale = guarded_put(&conn, &client, &doc, &text, "USER", &ts).await;
    let after = stored_text(&r, CLASS).await;
    // And with the current timestamp, the write goes through.
    let (_, now) = get_doc(&conn, &client, &doc, "USER")
        .await
        .unwrap()
        .unwrap();
    let fresh = guarded_put(&conn, &client, &doc, &text, "USER", &now).await;
    let problems = drop_fixture(&r, &f).await;

    let err = stale.expect_err("a stale timestamp was accepted");
    assert_eq!(err["error_code"], "STORAGE_MAP_VERSION_CONFLICT", "{err}");
    assert_eq!(
        after.trim_end(),
        other.trim_end(),
        "the stale PUT changed the class"
    );
    assert!(fresh.is_ok(), "{fresh:?}");
    assert!(problems.is_empty(), "cleanup: {problems:?}");
}

/// A connection and call client, as in the scratch tests.
async fn live_conn() -> (
    iris_agentic_dev_core::iris::connection::IrisConnection,
    reqwest::Client,
) {
    use iris_agentic_dev_core::iris::connection::{DiscoverySource, IrisConnection};
    let t = iris_agentic_dev_core::testing::LiveTarget::from_env();
    t.assert_running();
    let mut conn = IrisConnection::new(
        t.base_url(),
        &t.namespace,
        &t.username,
        &t.password,
        DiscoverySource::ExplicitFlag,
    );
    conn.probe().await;
    let client = conn
        .client(iris_agentic_dev_core::iris::http::ClientKind::Call)
        .expect("client");
    (conn, client)
}

/// T073 (a): a replace with the map as `read` returns it writes the class back byte for byte,
/// for every editable fixture.
#[tokio::test]
#[ignore = "live IRIS"]
async fn test_storage_map_edit_172_no_change_write_is_byte_identical() {
    let r = rig(&Where::live(), Toolset::Merged).await;
    let mut failures = Vec::new();
    for name in [
        "Member",
        "MemberByCity",
        "Branch",
        "Copy",
        "Genre",
        "Loan",
        "LoanEvent",
        "LoanList",
        "Notes",
        "Shelf",
        "BookMulti",
        "ActiveLoan",
        "LoanUnion",
        "MemberD11",
    ] {
        let f = Fixture::read(name);
        load_fixture(&r, &f).await;
        let storage = read_storage(&r, &f.class).await;
        for map in storage["maps"].as_array().cloned().unwrap_or_default() {
            let got = r
                .call(
                    "iris_storage_map",
                    json!({"action": "replace", "class": f.class, "map": map["name"],
                           "version": map["version"], "model": map, "namespace": "USER"}),
                )
                .await;
            if got["success"] != true {
                failures.push(format!("{name}/{}: {got}", map["name"]));
            }
        }
        let after = stored_text(&r, &f.class).await;
        if after.trim_end() != f.cls_text.trim_end() {
            failures.push(format!("{name}: the text changed"));
        }
        drop_fixture(&r, &f).await;
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// T073 (b): adding a data field writes the class, returns the new model, and the table still
/// answers its known question.
#[tokio::test]
#[ignore = "live IRIS"]
async fn test_storage_map_edit_172_add_a_field() {
    let r = rig(&Where::live(), Toolset::Merged).await;
    let f = Fixture::read("Member");
    load_fixture(&r, &f).await;
    let with_email = f.cls_text.replacen(
        "Property Age As %Integer;",
        "Property Age As %Integer;\n\nProperty Email As %String;",
        1,
    );
    // A property no data map files does not compile (#5502), so the class is stored with Email
    // and compiled by the edit that maps it.
    let put = r
        .call(
            "iris_doc",
            json!({"mode": "put", "name": format!("{CLASS}.cls"), "content": with_email,
                   "compile": false, "namespace": "USER"}),
        )
        .await;
    assert_eq!(put["success"], true, "{put}");
    let version = master(&r).await["version"].clone();
    let got = r
        .call(
            "iris_storage_map",
            json!({"action": "edit", "class": CLASS, "version": version, "namespace": "USER",
                   "ops": [{"op": "add", "path": "/maps/Master/data/Email",
                            "value": {"delimiter": "\"^\"", "piece": "4"}}]}),
        )
        .await;
    let rows = ask(&r, &f).await;
    let email = r
        .call(
            "iris_query",
            json!({"query": "SELECT Email FROM IadSm172.Member WHERE ID = 1", "namespace": "USER"}),
        )
        .await;
    let problems = drop_fixture(&r, &f).await;

    assert_eq!(got["success"], true, "{got}");
    assert_eq!(got["action"], "edit", "{got}");
    assert!(
        got["changed"]
            .to_string()
            .contains("/maps/Master/data/Email"),
        "{got}"
    );
    assert_eq!(
        got["storage"]["maps"][0]["data"]["Email"]["piece"], "4",
        "{got}"
    );
    assert_ne!(got["versions"]["before"], got["versions"]["after"], "{got}");
    let expected = f.seed["answer"].as_array().cloned().unwrap_or_default();
    assert_eq!(rows, expected);
    assert_eq!(email["success"], true, "{email}");
    assert!(problems.is_empty(), "cleanup: {problems:?}");
}

/// T073 (c): an edit the scratch copy passes but the real compile refuses is undone. The real
/// class has a method generator (code the scratch copy never carries) that refuses piece 9.
#[tokio::test]
#[ignore = "live IRIS"]
async fn test_storage_map_edit_172_failed_real_compile_restores() {
    use crate::live_rig_171::rig_with;
    // The generator is code that runs at compile time, which the compile-time gate refuses to
    // put; the operator turns the gate off for this test only.
    let r = rig_with(
        &Where::live(),
        Toolset::Merged,
        "write_tools_enabled = true\ndestructive_tools_enabled = true\ncompile_time_gate = false\n",
    )
    .await;
    let f = Fixture::read("Member");
    load_fixture(&r, &f).await;
    let guard = "ClassMethod StorageGuard() [ CodeMode = objectgenerator ]\n{\n\
                 \tFor i=1:1:%class.Storages.Count() {\n\
                 \t\tSet s=%class.Storages.GetAt(i)\n\
                 \t\tFor j=1:1:s.SQLMaps.Count() {\n\
                 \t\t\tSet m=s.SQLMaps.GetAt(j)\n\
                 \t\t\tFor k=1:1:m.Data.Count() {\n\
                 \t\t\t\tIf m.Data.GetAt(k).Piece=9 { Return $$$ERROR($$$GeneralError,\"piece 9 is refused\") }\n\
                 \t\t\t}\n\t\t}\n\t}\n\
                 \tQuit $$$OK\n}\n\n";
    let guarded = f.cls_text.replacen(
        "Storage SQLStorage",
        &format!("{guard}Storage SQLStorage"),
        1,
    );
    put_class(&r, CLASS, &guarded).await;
    let before = stored_text(&r, CLASS).await;
    let version = master(&r).await["version"].clone();

    let got = r
        .call(
            "iris_storage_map",
            json!({"action": "edit", "class": CLASS, "version": version, "namespace": "USER",
                   "ops": [{"op": "set", "path": "/maps/Master/data/Age/piece", "value": "9"}]}),
        )
        .await;
    let after = stored_text(&r, CLASS).await;
    let rows = ask(&r, &f).await;
    let problems = drop_fixture(&r, &f).await;

    assert_eq!(got["success"], false, "{got}");
    assert_eq!(got["error_code"], "STORAGE_MAP_RESTORED", "{got}");
    assert!(
        got["iris_errors"]
            .to_string()
            .contains("piece 9 is refused"),
        "{got}"
    );
    assert_eq!(
        after.trim_end(),
        before.trim_end(),
        "the previous text was not restored"
    );
    let expected = f.seed["answer"].as_array().cloned().unwrap_or_default();
    assert_eq!(
        rows, expected,
        "the restored class does not answer its question"
    );
    assert!(problems.is_empty(), "cleanup: {problems:?}");
}

/// T074: a stale map version is refused with the current map; read-only storage and a map
/// turned columnar are refused; nothing is written.
#[tokio::test]
#[ignore = "live IRIS"]
async fn test_storage_map_edit_172_edit_refusals() {
    let r = rig(&Where::live(), Toolset::Merged).await;
    let f = Fixture::read("Member");
    load_fixture(&r, &f).await;
    let old_version = master(&r).await["version"].clone();
    // The map changes behind the caller's back (iris_doc, before the hand-off of Phase 8).
    put_class(
        &r,
        CLASS,
        &f.cls_text
            .replacen("<Piece>3</Piece>", "<Piece>5</Piece>", 1),
    )
    .await;
    let before = stored_text(&r, CLASS).await;
    let stale = r
        .call(
            "iris_storage_map",
            json!({"action": "edit", "class": CLASS, "version": old_version, "namespace": "USER",
                   "ops": [{"op": "set", "path": "/maps/Master/data/Age/piece", "value": "4"}]}),
        )
        .await;
    let current = master(&r).await["version"].clone();
    let columnar = r
        .call(
            "iris_storage_map",
            json!({"action": "edit", "class": CLASS, "version": current, "namespace": "USER",
                   "ops": [{"op": "set", "path": "/maps/Master/type", "value": "columnar"}]}),
        )
        .await;
    let opt_in = r
        .call(
            "iris_storage_map",
            json!({"action": "edit", "class": CLASS, "version": current, "namespace": "USER",
                   "seed_counter": true,
                   "ops": [{"op": "set", "path": "/maps/Master/data/Age/piece", "value": "4"}]}),
        )
        .await;
    let after = stored_text(&r, CLASS).await;
    let book = Fixture::read("DefaultBook");
    load_fixture(&r, &book).await;
    let default = r
        .call(
            "iris_storage_map",
            json!({"action": "edit", "class": book.class, "version": "x", "namespace": "USER",
                   "ops": [{"op": "set", "path": "/keywords/IdLocation", "value": "^X"}]}),
        )
        .await;
    drop_fixture(&r, &book).await;
    let problems = drop_fixture(&r, &f).await;

    assert_eq!(
        stale["error_code"], "STORAGE_MAP_VERSION_CONFLICT",
        "{stale}"
    );
    assert_eq!(stale["current_version"], current, "{stale}");
    assert_eq!(stale["current"]["data"]["Age"]["piece"], "5", "{stale}");
    assert_eq!(
        columnar["error_code"], "STORAGE_MAP_READ_ONLY",
        "{columnar}"
    );
    assert_eq!(opt_in["error_code"], "NOT_IMPLEMENTED", "{opt_in}");
    assert_eq!(default["error_code"], "STORAGE_MAP_READ_ONLY", "{default}");
    assert_eq!(after, before, "a refused edit changed the class");
    assert!(problems.is_empty(), "cleanup: {problems:?}");
}

/// T075: a replace that removes elements lists them and writes only when confirmed. `Shelf`'s
/// subscript has a NextCode, which is optional: without it the map still compiles.
#[tokio::test]
#[ignore = "live IRIS"]
async fn test_storage_map_edit_172_replace_confirms_removal() {
    let r = rig(&Where::live(), Toolset::Merged).await;
    let f = Fixture::read("Shelf");
    load_fixture(&r, &f).await;
    let map = read_storage(&r, &f.class).await["maps"][0].clone();
    assert!(map["subscripts"][0]["next_code"].is_string(), "{map}");
    let mut smaller = map.clone();
    smaller["subscripts"][0]
        .as_object_mut()
        .unwrap()
        .remove("next_code");
    let args = json!({"action": "replace", "class": f.class, "map": "Master",
                      "version": map["version"], "model": smaller, "namespace": "USER"});
    let first = r.call("iris_storage_map", args.clone()).await;
    let untouched = stored_text(&r, &f.class).await;
    let mut confirmed = args.clone();
    confirmed["confirm_removal"] = json!(true);
    let second = r.call("iris_storage_map", confirmed).await;
    let after = read_storage(&r, &f.class).await["maps"][0].clone();
    let problems = drop_fixture(&r, &f).await;

    let removed = json!(["/maps/Master/subscripts/1/next_code"]);
    assert_eq!(
        first["error_code"], "STORAGE_MAP_CONFIRM_REMOVAL",
        "{first}"
    );
    assert_eq!(first["removed"], removed, "{first}");
    assert_eq!(
        untouched.trim_end(),
        f.cls_text.trim_end(),
        "the first call wrote"
    );
    assert_eq!(second["success"], true, "{second}");
    assert_eq!(second["removed"], removed, "{second}");
    assert!(after["subscripts"][0]["next_code"].is_null(), "{after}");
    assert!(problems.is_empty(), "cleanup: {problems:?}");
}

/// T076: a map write adds an audit entry naming the class, map, versions and changed paths,
/// with no code text, to the local audit log (on for a server with a policy block).
#[tokio::test]
#[ignore = "live IRIS"]
async fn test_storage_map_edit_172_a_write_is_audited_without_code() {
    use crate::live_rig_171::rig_with;
    let r = rig_with(
        &Where::live(),
        Toolset::Merged,
        "write_tools_enabled = true\ndestructive_tools_enabled = true\n\n[policy.default]\n",
    )
    .await;
    let f = Fixture::read("Member");
    load_fixture(&r, &f).await;
    let map = master(&r).await;
    let code = " s {L1}=$o(^IadSm172Member({L1}))";
    let got = r
        .call(
            "iris_storage_map",
            json!({"action": "edit", "class": CLASS, "version": map["version"], "namespace": "USER",
                   "ops": [{"op": "set", "path": "/maps/Master/data/Age/piece", "value": "3"},
                           {"op": "set", "path": "/maps/Master/subscripts/1/next_code", "value": code}]}),
        )
        .await;
    let log = std::env::var("HOME")
        .map(|h| std::path::PathBuf::from(h).join(".iris-agentic-dev/audit.jsonl"))
        .ok()
        .and_then(|p| std::fs::read_to_string(p).ok())
        .unwrap_or_default();
    let problems = drop_fixture(&r, &f).await;

    let entry: Value = log
        .lines()
        .rev()
        .filter_map(|l| serde_json::from_str::<Value>(l).ok())
        .find(|e| e["tool"] == "iris_storage_map" && e["status"] == "written")
        .unwrap_or_else(|| panic!("no written entry for the edit ({got}) in:\n{log}"));
    assert_eq!(got["success"], true, "{got}");
    let rec = &entry["params"];
    assert_eq!(rec["class"], CLASS, "{entry}");
    assert_eq!(rec["map"], "Master", "{entry}");
    assert_eq!(rec["version_before"], map["version"], "{entry}");
    assert_ne!(rec["version_after"], map["version"], "{entry}");
    assert_eq!(rec["code_changed"], true, "{entry}");
    assert!(
        rec["changed_paths"]
            .to_string()
            .contains("/maps/Master/subscripts/1/next_code"),
        "{entry}"
    );
    assert!(
        !log.contains("$o(^IadSm172Member"),
        "map code reached the audit log:\n{log}"
    );
    assert!(problems.is_empty(), "cleanup: {problems:?}");
}

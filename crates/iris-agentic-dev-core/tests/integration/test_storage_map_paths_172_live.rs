//! Spec 172: the write path's refusals and warnings that only a live IRIS reaches. Each test
//! makes the condition for real (a class XML put, a Storage block iad cannot parse, a second
//! class over the same counter global) and asserts the answer and that nothing was written.
//!
//! Run: `IRIS_HOST=localhost IRIS_WEB_PORT=52780 cargo test --features testing --test integration \
//!   test_storage_map_paths_172 -- --test-threads=1 --include-ignored`

use crate::live_rig_171::{rig, Rig, Where};
use crate::storage172_rig::{drop_fixture, load_fixture, Fixture};
use iris_agentic_dev_core::tools::Toolset;
use serde_json::{json, Value};

async fn put(r: &Rig, class: &str, content: &str) -> Value {
    r.call(
        "iris_doc",
        json!({"mode": "put", "name": format!("{class}.cls"), "content": content,
               "compile": true, "namespace": "USER"}),
    )
    .await
}

async fn exists(r: &Rig, class: &str) -> bool {
    r.call(
        "iris_doc",
        json!({"mode": "get", "name": format!("{class}.cls"), "namespace": "USER"}),
    )
    .await["success"]
        == true
}

async fn delete(r: &Rig, class: &str) {
    r.call(
        "iris_doc",
        json!({"mode": "delete", "name": format!("{class}.cls"), "namespace": "USER"}),
    )
    .await;
}

/// FR-012a: class XML with an SQL map is refused before the PUT, so nothing is stored.
#[tokio::test]
#[ignore = "live IRIS"]
async fn test_storage_map_paths_172_iris_doc_refuses_class_xml_with_a_map() {
    let r = rig(&Where::live(), Toolset::Merged).await;
    let class = "IadSm172.XmlMap";
    delete(&r, class).await;
    let xml = r#"<?xml version="1.0" encoding="UTF8"?>
<Export generator="IRIS" version="26">
<Class name="IadSm172.XmlMap">
<Super>%Persistent</Super>
<StorageStrategy>SQLStorage</StorageStrategy>
<Storage name="SQLStorage">
<Type>%Storage.SQL</Type>
<SQLMap name="Master">
<Global>^IadSm172XmlMap</Global>
</SQLMap>
</Storage>
</Class>
</Export>
"#;
    let got = put(&r, class, xml).await;
    let stored = exists(&r, class).await;

    assert_eq!(got["error_code"], "STORAGE_MAP_INVALID", "{got}");
    assert!(
        got["error"].as_str().unwrap_or("").contains("class XML"),
        "{got}"
    );
    assert!(!stored, "the class was stored: {got}");
}

/// A Storage block with an SQL map that iad cannot parse is refused with where it stopped.
#[tokio::test]
#[ignore = "live IRIS"]
async fn test_storage_map_paths_172_iris_doc_refuses_a_map_it_cannot_parse() {
    let r = rig(&Where::live(), Toolset::Merged).await;
    let class = "IadSm172.Broken";
    delete(&r, class).await;
    let text = Fixture::read("Member")
        .cls_text
        .replacen("Class IadSm172.Member ", &format!("Class {class} "), 1)
        .replacen(
            "<Global>^IadSm172Member</Global>",
            "<Global>^IadSm172Broken</Glob>",
            1,
        );
    let got = put(&r, class, &text).await;
    let stored = exists(&r, class).await;

    assert_eq!(got["error_code"], "STORAGE_MAP_PARSE_FAILED", "{got}");
    assert!(
        got["error"]
            .as_str()
            .unwrap_or("")
            .starts_with("cannot parse the Storage block: "),
        "{got}"
    );
    assert!(got["offset"].is_u64(), "{got}");
    assert!(!stored, "the class was stored: {got}");
}

/// The `iris_doc` hand-off judges with the gates of the call in progress; called outside one it
/// fails closed.
#[tokio::test]
#[ignore = "live IRIS"]
async fn test_storage_map_paths_172_doc_precheck_outside_a_call_fails_closed() {
    use iris_agentic_dev_core::iris::connection::{DiscoverySource, IrisConnection};
    use iris_agentic_dev_core::tools::storage_map::write::doc_precheck;
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
    // A class that is not on the server: the precheck reads it (nothing there) and then gates.
    let text = Fixture::read("Member").cls_text.replacen(
        "Class IadSm172.Member ",
        "Class IadSm172.NoCall ",
        1,
    );
    let got = match doc_precheck(&conn, &client, "IadSm172.NoCall.cls", &text, "USER", false).await
    {
        Ok(_) => panic!("a precheck outside a call must refuse"),
        Err(e) => e,
    };

    assert_eq!(got["error_code"], "TARGET_NOT_RESOLVED", "{got}");
    assert_eq!(
        got["error"], "no call is in progress, so the storage-map gates cannot judge this put",
        "{got}"
    );
}

/// FR-023a: `seed_counter` will not reset a counter global another class's map uses, and the
/// counter is left as it was.
#[tokio::test]
#[ignore = "live IRIS"]
async fn test_storage_map_paths_172_seed_counter_refuses_a_shared_counter() {
    let r = rig(&Where::live(), Toolset::Merged).await;
    let f = Fixture::read("Member");
    load_fixture(&r, &f).await;
    // A second class over the same global: a read view of the members.
    let twin = "IadSm172.MemberTwin";
    let twin_text = f
        .cls_text
        .replacen("Class IadSm172.Member ", &format!("Class {twin} "), 1);
    let twin_put = put(&r, twin, &twin_text).await;
    // The counter falls behind the highest ID (3).
    r.call(
        "iris_execute",
        json!({"code": "ZKill ^IadSm172Member Write 1", "namespace": "USER"}),
    )
    .await;
    let version = r
        .call(
            "iris_storage_map",
            json!({"action": "read", "class": f.class, "namespace": "USER"}),
        )
        .await["storage"]["version"]
        .clone();
    let refused = r
        .call(
            "iris_storage_map",
            json!({"action": "edit", "class": f.class, "version": version, "namespace": "USER",
                   "seed_counter": true,
                   "ops": [{"op": "set", "path": "/keywords/SqlIdExpression",
                            "value": "$INCREMENT(^IadSm172Member)"}]}),
        )
        .await;
    let counter = r
        .call(
            "iris_execute",
            json!({"code": "Write \"C=\",$Data(^IadSm172Member)#10", "namespace": "USER"}),
        )
        .await;
    delete(&r, twin).await;
    let problems = drop_fixture(&r, &f).await;

    assert_eq!(twin_put["success"], true, "{twin_put}");
    assert_eq!(refused["error_code"], "STORAGE_MAP_INVALID", "{refused}");
    assert_eq!(refused["rules"][0]["rule"], "counter_shared", "{refused}");
    assert_eq!(
        refused["rules"][0]["path"], "/keywords/SqlIdExpression",
        "{refused}"
    );
    assert!(
        counter["output"].as_str().unwrap_or("").contains("C=0"),
        "the counter was set: {counter}"
    );
    assert!(problems.is_empty(), "cleanup: {problems:?}");
}

/// An ID expression iad cannot read a counter from is written with a warning, not checked.
/// IRIS splices the argument of `$i(...)` into `$g(...)` and a second `$INCREMENT(...)`, so it must
/// be one global reference (`+0` and `,1` both fail to compile, measured 2026-11-14), and iad
/// refuses an extended one. A subscript built at run time compiles, and iad cannot read the
/// counter's node from it.
#[tokio::test]
#[ignore = "live IRIS"]
async fn test_storage_map_paths_172_an_unreadable_counter_is_a_warning() {
    let r = rig(&Where::live(), Toolset::Merged).await;
    let f = Fixture::read("Member");
    load_fixture(&r, &f).await;
    let version = r
        .call(
            "iris_storage_map",
            json!({"action": "read", "class": f.class, "namespace": "USER"}),
        )
        .await["storage"]["version"]
        .clone();
    let got = r
        .call(
            "iris_storage_map",
            json!({"action": "edit", "class": f.class, "version": version, "namespace": "USER",
                   "ops": [{"op": "set", "path": "/keywords/SqlIdExpression",
                            "value": "$INCREMENT(^IadSm172Member(\"c\"_\"tr\"))"}]}),
        )
        .await;
    let problems = drop_fixture(&r, &f).await;

    assert_eq!(got["success"], true, "{got}");
    let warnings = got["warnings"].to_string();
    assert!(
        warnings.contains("the ID counter was not checked: "),
        "{got}"
    );
    assert!(problems.is_empty(), "cleanup: {problems:?}");
}

/// FR-023 for an extent bitmap. A new `bitmapextent` map over rows answers `COUNT(*)` from its
/// empty bitmap, so the count is 0 for 3 rows (measured 2026-11-14, plan "Read extent bitmap").
/// iad refuses it unbuilt, and `build_indexes` builds it so the count is right.
#[tokio::test]
#[ignore = "live IRIS"]
async fn test_storage_map_paths_172_a_new_extent_bitmap_must_be_built() {
    let r = rig(&Where::live(), Toolset::Merged).await;
    let mut f = Fixture::read("Badge");
    // The class as it was before the extent map: same Index, no map, nothing to build.
    let start = f.cls_text.find("<SQLMap name=\"BadgeExtent\">").unwrap();
    let end = f.cls_text[start..].find("</SQLMap>\n").unwrap() + start + "</SQLMap>\n".len();
    f.cls_text.replace_range(start..end, "");
    f.seed["build_indexes"] = json!([]);
    load_fixture(&r, &f).await;
    let add = |build: bool, version: Value| {
        json!({"action": "edit", "class": f.class, "version": version, "namespace": "USER",
               "build_indexes": build,
               "ops": [{"op": "add", "path": "/maps/BadgeExtent",
                        "value": {"type": "bitmapextent", "global": "^IadSm172BadgeX",
                                  "subscripts": []}}]})
    };
    let version = || async {
        r.call(
            "iris_storage_map",
            json!({"action": "read", "class": f.class, "namespace": "USER"}),
        )
        .await["storage"]["version"]
            .clone()
    };
    let unbuilt = r
        .call("iris_storage_map", add(false, version().await))
        .await;
    let built = r.call("iris_storage_map", add(true, version().await)).await;
    let count = r
        .call(
            "iris_query",
            json!({"query": "SELECT COUNT(*) AS n FROM IadSm172.Badge", "namespace": "USER"}),
        )
        .await;
    let problems = drop_fixture(&r, &f).await;

    assert_eq!(
        unbuilt["error_code"], "STORAGE_MAP_INDEX_UNBUILT",
        "{unbuilt}"
    );
    assert_eq!(built["success"], true, "{built}");
    assert_eq!(
        built["opt_ins_run"]["build_indexes"]["built"],
        json!(["BadgeExtent"]),
        "{built}"
    );
    assert_eq!(count["rows"], json!([{"n": 3}]), "{count}");
    assert!(problems.is_empty(), "cleanup: {problems:?}");
}

/// T101e: a class written with the old storage type name. IRIS stores `%CacheSQLStorage` as
/// `%Storage.SQL`; the `iris_doc` hand-off reads that as IRIS's rename, not a writer mismatch, and
/// the table answers.
#[tokio::test]
#[ignore = "live IRIS"]
async fn test_storage_map_paths_172_the_old_storage_type_name_is_accepted() {
    let r = rig(&Where::live(), Toolset::Merged).await;
    let mut f = Fixture::read("Member");
    f.cls_text = f.cls_text.replacen(
        "<Type>%Storage.SQL</Type>",
        "<Type>%CacheSQLStorage</Type>",
        1,
    );
    let put_answer = put(&r, &f.class, &f.cls_text).await;
    let stored = r
        .call(
            "iris_doc",
            json!({"mode": "get", "name": format!("{}.cls", f.class), "namespace": "USER"}),
        )
        .await;
    let problems = drop_fixture(&r, &f).await;

    assert_eq!(put_answer["success"], true, "{put_answer}");
    assert!(put_answer["restored"].is_null(), "{put_answer}");
    let text = stored["content"].to_string();
    assert!(text.contains("<Type>%Storage.SQL</Type>"), "{stored}");
    assert!(!text.contains("CacheSQLStorage"), "{stored}");
    assert!(problems.is_empty(), "cleanup: {problems:?}");
}

/// FR-012a: a second Storage block is gated like the first. Its map over `^ROUTINE` (code
/// storage) is refused before the PUT, and the class is not stored.
#[tokio::test]
#[ignore = "live IRIS"]
async fn test_storage_map_paths_172_a_second_storage_block_is_gated() {
    let r = rig(&Where::live(), Toolset::Merged).await;
    let class = "IadSm172.TwoBlocks";
    delete(&r, class).await;
    let text = Fixture::read("Member").cls_text.replacen(
        "Class IadSm172.Member ",
        &format!("Class {class} "),
        1,
    );
    let end = text.rfind("\n}").unwrap();
    let text = format!(
        "{}\nStorage Other\n{{\n<SQLMap name=\"Master\">\n<Global>^ROUTINE</Global>\n\
         <Subscript name=\"1\">\n<Expression>{{ID}}</Expression>\n</Subscript>\n\
         <Type>data</Type>\n</SQLMap>\n<Type>%Storage.SQL</Type>\n}}\n{}",
        &text[..end],
        &text[end..]
    );
    let got = put(&r, class, &text).await;
    let stored = exists(&r, class).await;
    delete(&r, class).await;

    assert_ne!(got["success"], true, "{got}");
    assert!(got["refusal"].is_object(), "{got}");
    assert!(got.to_string().contains("ROUTINE"), "{got}");
    assert!(!stored, "the class was stored: {got}");
}

/// FR-027: an `iris_doc` put that adds or changes map code needs Execute as well as Compile; a
/// put that changes only a literal (a Piece) needs Compile alone.
#[tokio::test]
#[ignore = "live IRIS"]
async fn test_storage_map_paths_172_iris_doc_map_code_needs_execute() {
    use crate::live_rig_171::rig_with;
    let open = rig(&Where::live(), Toolset::Merged).await;
    let narrow = rig_with(
        &Where::live(),
        Toolset::Merged,
        "write_tools_enabled = true\n\n\
         [policy.default]\nallow = [\"compile\", \"query\", \"docs\", \"search\"]\n\n\
         [policy._env]\nallow = [\"compile\", \"query\", \"docs\", \"search\"]\n",
    )
    .await;
    let f = Fixture::read("Member");
    drop_fixture(&open, &f).await;
    let new_class = put(&narrow, &f.class, &f.cls_text).await;
    let stored_by_narrow = exists(&open, &f.class).await;
    load_fixture(&open, &f).await;
    let literal = put(
        &narrow,
        &f.class,
        &f.cls_text
            .replacen("<Piece>3</Piece>", "<Piece>4</Piece>", 1),
    )
    .await;
    let code = put(
        &narrow,
        &f.class,
        &f.cls_text.replacen(
            "<Expression>{ID}</Expression>",
            "<Expression>{ID}+0</Expression>",
            1,
        ),
    )
    .await;
    let problems = drop_fixture(&open, &f).await;

    assert_eq!(new_class["error_code"], "POLICY_GATE", "{new_class}");
    assert_eq!(new_class["blocked_category"], "execute", "{new_class}");
    assert!(!stored_by_narrow, "the class was stored: {new_class}");
    assert_eq!(literal["success"], true, "{literal}");
    assert_eq!(code["error_code"], "POLICY_GATE", "{code}");
    assert_eq!(code["blocked_category"], "execute", "{code}");
    assert!(problems.is_empty(), "cleanup: {problems:?}");
}

/// FR-015a: the scratch copy keeps the class's `Extends`, so a stale superclass is refused before
/// the copy compiles, and its generator never runs. (Before, `iris_doc`'s superclass check ran
/// after the scratch compile, which had already compiled the stale class.)
#[tokio::test]
#[ignore = "live IRIS"]
async fn test_storage_map_paths_172_a_stale_superclass_stops_the_scratch_copy() {
    use iris_agentic_dev_core::iris::connection::{DiscoverySource, IrisConnection};
    let r = rig(&Where::live(), Toolset::Merged).await;
    let t = iris_agentic_dev_core::testing::LiveTarget::from_env();
    let conn = IrisConnection::new(
        t.base_url(),
        "USER",
        &t.username,
        &t.password,
        DiscoverySource::ExplicitFlag,
    );
    let client = reqwest::Client::new();
    // Cleanup through the tools. `$system.OBJ.DeletePackage` through iris_execute is refused by
    // the code-edit gate, and a cleanup that silently did nothing left a compiled Mixin behind.
    let clean = || async {
        delete(&r, "IadSm172Sup.Mixin").await;
        r.call(
            "iris_global",
            json!({"action": "kill", "global_name": "IadSupProbe", "namespace": "USER"}),
        )
        .await;
    };
    clean().await;
    // Stored with a straight Atelier PUT and never compiled: stale, with a generator. Its code is
    // unique to this run, so the PUT changes the class (IsUpToDate ignores description changes,
    // and a PUT of the same text keeps the timestamp).
    let run_id = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let mixin = format!(
        "Class IadSm172Sup.Mixin [ Abstract ]\n{{\n\nClassMethod Gen() As %String [ CodeMode = objectgenerator ]\n{{\n    Set ^IadSupProbe(\"sm172\") = $USERNAME\n    Do %code.WriteLine(\"    Quit {run_id}\")\n    Quit $$$OK\n}}\n\n}}\n"
    );
    let url = conn.versioned_ns_url("USER", "/doc/IadSm172Sup.Mixin.cls?ignoreConflict=1");
    let lines: Vec<&str> = mixin.lines().collect();
    let stored = client
        .put(&url)
        .basic_auth(&conn.username, Some(&conn.password))
        .json(&json!({"enc": false, "content": lines}))
        .send()
        .await
        .expect("PUT must reach IRIS");
    assert!(stored.status().is_success(), "{}", stored.status());

    let class = "IadSm172.SupUser";
    delete(&r, class).await;
    let text = Fixture::read("Member").cls_text.replacen(
        "Class IadSm172.Member Extends %Persistent ",
        &format!("Class {class} Extends (%Persistent, IadSm172Sup.Mixin) "),
        1,
    );
    let got = put(&r, class, &text).await;
    let ran = r
        .call(
            "iris_execute",
            json!({"code": "Write \"RAN=\",$Data(^IadSupProbe(\"sm172\"))", "namespace": "USER"}),
        )
        .await;
    let class_stored = exists(&r, class).await;
    delete(&r, class).await;
    clean().await;

    assert_eq!(got["error_code"], "COMPILE_SUPERCLASS_STALE", "{got}");
    assert!(got.to_string().contains("IadSm172Sup.Mixin"), "{got}");
    assert!(
        ran["output"].as_str().unwrap_or("").contains("RAN=0"),
        "the stale superclass's generator ran: {ran}"
    );
    assert!(!class_stored, "the class was stored: {got}");
}

/// FR-010a: a restore replaces only the version iad wrote. A class someone changed after iad's
/// PUT is refused and left as they wrote it; over iad's own version the restore goes through.
#[tokio::test]
#[ignore = "live IRIS"]
async fn test_storage_map_paths_172_a_restore_never_overwrites_a_later_change() {
    use iris_agentic_dev_core::iris::connection::{DiscoverySource, IrisConnection};
    use iris_agentic_dev_core::tools::storage_map::write::{restore_over, CHANGED_SINCE};
    let r = rig(&Where::live(), Toolset::Merged).await;
    let f = Fixture::read("Member");
    let ours = load_fixture(&r, &f).await;
    let t = iris_agentic_dev_core::testing::LiveTarget::from_env();
    let mut conn = IrisConnection::new(
        t.base_url(),
        "USER",
        &t.username,
        &t.password,
        DiscoverySource::ExplicitFlag,
    );
    conn.probe().await;
    let client = conn
        .client(iris_agentic_dev_core::iris::http::ClientKind::Call)
        .expect("client");
    let doc = format!("{}.cls", f.class);
    let previous = f
        .cls_text
        .replacen("<Piece>3</Piece>", "<Piece>4</Piece>", 1);
    // Someone else's change after iad's write: a new property.
    let theirs = f.cls_text.replacen(
        "Property Age As %Integer;",
        "Property Age As %Integer;\n\nProperty Note As %String;",
        1,
    );
    // Stored, not compiled: a property the map does not read yet fails to compile (#5502).
    let changed = r
        .call(
            "iris_doc",
            json!({"mode": "put", "name": doc, "content": theirs, "compile": false,
                   "namespace": "USER"}),
        )
        .await;
    let refused = restore_over(&conn, &client, &doc, &previous, "USER", &ours).await;
    let after_refusal = r
        .call(
            "iris_doc",
            json!({"mode": "get", "name": doc, "namespace": "USER"}),
        )
        .await;
    let theirs_stored = after_refusal["content"].to_string();
    let theirs_now = load_text(&after_refusal);
    let allowed = restore_over(&conn, &client, &doc, &previous, "USER", &theirs_now).await;
    let after_restore = r
        .call(
            "iris_doc",
            json!({"mode": "get", "name": doc, "namespace": "USER"}),
        )
        .await;
    let problems = drop_fixture(&r, &f).await;

    assert_eq!(changed["success"], true, "{changed}");
    assert_eq!(refused, Err(CHANGED_SINCE.to_string()));
    assert!(
        theirs_stored.contains("Property Note"),
        "their change was overwritten: {after_refusal}"
    );
    assert_eq!(allowed, Ok(()));
    let restored = after_restore["content"].to_string();
    assert!(
        restored.contains("<Piece>4</Piece>") && !restored.contains("Property Note"),
        "{after_restore}"
    );
    assert!(problems.is_empty(), "cleanup: {problems:?}");
}

/// The text of an `iris_doc get` answer (a string, or a list of lines).
fn load_text(got: &Value) -> String {
    got["content"]
        .as_str()
        .map(str::to_string)
        .or_else(|| {
            got["content"].as_array().map(|l| {
                l.iter()
                    .filter_map(Value::as_str)
                    .collect::<Vec<_>>()
                    .join("\n")
            })
        })
        .unwrap_or_default()
}

/// FR-031: content with Windows line endings is judged as IRIS stores it. Before, the block reader
/// found no Storage block in CRLF text, so no safeguard ran, and IRIS stored the class with its
/// map; a map over `^ROUTINE` is refused, and nothing is stored.
#[tokio::test]
#[ignore = "live IRIS"]
async fn test_storage_map_paths_172_crlf_content_gets_the_safeguards() {
    let r = rig(&Where::live(), Toolset::Merged).await;
    let class = "IadSm172.Crlf";
    delete(&r, class).await;
    let text = Fixture::read("Member")
        .cls_text
        .replacen("Class IadSm172.Member ", &format!("Class {class} "), 1)
        .replacen(
            "<Global>^IadSm172Member</Global>",
            "<Global>^ROUTINE</Global>",
            1,
        )
        .replace('\n', "\r\n");
    let got = put(&r, class, &text).await;
    let stored = exists(&r, class).await;
    delete(&r, class).await;

    assert_ne!(got["success"], true, "{got}");
    assert!(got.to_string().contains("ROUTINE"), "{got}");
    assert!(
        !stored,
        "the CRLF class was stored past the safeguards: {got}"
    );
}

/// FR-012a with FR-027: map code changed in a second SQL Storage block needs Execute, as in the
/// first; a literal change in the first block needs Compile alone.
#[tokio::test]
#[ignore = "live IRIS"]
async fn test_storage_map_paths_172_map_code_in_a_second_block_needs_execute() {
    use crate::live_rig_171::rig_with;
    let open = rig(&Where::live(), Toolset::Merged).await;
    let narrow = rig_with(
        &Where::live(),
        Toolset::Merged,
        "write_tools_enabled = true\n\n\
         [policy.default]\nallow = [\"compile\", \"query\", \"docs\", \"search\"]\n\n\
         [policy._env]\nallow = [\"compile\", \"query\", \"docs\", \"search\"]\n",
    )
    .await;
    let class = "IadSm172.TwoMaps";
    delete(&open, class).await;
    let with_second = |expr: &str, piece: &str| {
        let text = Fixture::read("Member")
            .cls_text
            .replacen("Class IadSm172.Member ", &format!("Class {class} "), 1)
            .replacen("<Piece>3</Piece>", &format!("<Piece>{piece}</Piece>"), 1);
        let end = text.rfind("\n}").unwrap();
        format!(
            "{}\nStorage Other\n{{\n<SQLMap name=\"Master\">\n<Global>^IadSm172TwoOther</Global>\n\
             <Subscript name=\"1\">\n<Expression>{expr}</Expression>\n</Subscript>\n\
             <Type>data</Type>\n</SQLMap>\n<Type>%Storage.SQL</Type>\n}}\n{}",
            &text[..end],
            &text[end..]
        )
    };
    let first = put(&open, class, &with_second("{ID}", "3")).await;
    let code = put(&narrow, class, &with_second("{ID}+0", "3")).await;
    let literal = put(&narrow, class, &with_second("{ID}", "4")).await;
    delete(&open, class).await;

    assert_eq!(first["success"], true, "{first}");
    assert_eq!(code["error_code"], "POLICY_GATE", "{code}");
    assert_eq!(code["blocked_category"], "execute", "{code}");
    assert_eq!(literal["success"], true, "{literal}");
}

/// FR-026: `iris_compile` of a local file stores the class, so a class with an SQL map gets the
/// safeguards before its upload. A map over `^ROUTINE` is refused, and nothing is stored.
#[test]
#[ignore = "live IRIS"]
fn test_storage_map_paths_172_iris_compile_of_a_local_mapped_class_is_guarded() {
    use iris_agentic_dev_core::testing::{live_env, require_iad_binary, McpSession};
    let Some(_bin) = require_iad_binary() else {
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    let proj = std::fs::canonicalize(dir.path()).unwrap();
    let class = "IadSm172.LocalMap";
    let text = Fixture::read("Member")
        .cls_text
        .replacen("Class IadSm172.Member ", &format!("Class {class} "), 1)
        .replacen(
            "<Global>^IadSm172Member</Global>",
            "<Global>^ROUTINE</Global>",
            1,
        );
    std::fs::write(proj.join(format!("{class}.cls")), text).unwrap();
    let mut env = live_env();
    env.push(("IRIS_WRITE_TOOLS_ENABLED".into(), "1".into()));
    let mut mcp = McpSession::start_in(Some(&proj), &env);
    let body = |answer: &Value| -> Value {
        answer
            .pointer("/result/content/0/text")
            .and_then(Value::as_str)
            .and_then(|t| serde_json::from_str(t).ok())
            .unwrap_or_else(|| answer.clone())
    };
    let doc = format!("{class}.cls");
    let _ = mcp.call("iris_doc", &json!({"mode": "delete", "name": doc}));
    let got = body(&mcp.call("iris_compile", &json!({"target": format!("./{class}.cls")})));
    let stored = body(&mcp.call("iris_doc", &json!({"mode": "get", "name": doc})));
    let _ = mcp.call("iris_doc", &json!({"mode": "delete", "name": doc}));

    assert_ne!(got["success"], true, "{got}");
    assert!(got.to_string().contains("ROUTINE"), "{got}");
    assert_ne!(stored["success"], true, "the class was uploaded: {got}");
}

/// FR-022: `preview` stops at its time cap (10 s) and says so. The proposed map's `NextCode`
/// hangs 11 s on each row, so the read through the scratch copy runs past the cap.
#[tokio::test]
#[ignore = "live IRIS"]
async fn test_storage_map_paths_172_preview_stops_at_its_time_cap() {
    let r = rig(&Where::live(), Toolset::Merged).await;
    let f = Fixture::read("Member");
    load_fixture(&r, &f).await;
    let read = r
        .call(
            "iris_storage_map",
            json!({"action": "read", "class": f.class, "namespace": "USER"}),
        )
        .await;
    let mut slow = read["storage"]["maps"][0].clone();
    slow["subscripts"][0]["next_code"] = json!(" s {L1}=$o(^IadSm172Member({L1})) h 11");
    let started = std::time::Instant::now();
    let got = r
        .call(
            "iris_storage_map",
            json!({"action": "preview", "class": f.class, "namespace": "USER", "model": slow}),
        )
        .await;
    let took = started.elapsed();
    let problems = drop_fixture(&r, &f).await;

    assert_eq!(got["error_code"], "STORAGE_MAP_TIME_CAP", "{got}");
    assert_eq!(got["capped"], true, "{got}");
    assert_eq!(got["cap"]["seconds"], 10, "{got}");
    assert!(
        took.as_secs() < 30,
        "the cap did not stop the wait: {took:?}"
    );
    assert!(problems.is_empty(), "cleanup: {problems:?}");
}

/// FR-031: IRIS stores an indented class in its own form (members and Storage elements at the
/// start of a line, measured 2026-11-14), and the block reader takes only that form. An indented
/// class is judged in IRIS's form, read through an uncompiled scratch copy: a map over `^ROUTINE`
/// is refused and not stored, and the same class over an application global is written.
#[tokio::test]
#[ignore = "live IRIS"]
async fn test_storage_map_paths_172_an_indented_class_is_judged_as_iris_stores_it() {
    let r = rig(&Where::live(), Toolset::Merged).await;
    let class = "IadSm172.Indented";
    delete(&r, class).await;
    let indented = |global: &str| {
        Fixture::read("Member")
            .cls_text
            .replacen("Class IadSm172.Member ", &format!("Class {class} "), 1)
            .replacen(
                "<Global>^IadSm172Member</Global>",
                &format!("<Global>{global}</Global>"),
                1,
            )
            .lines()
            .map(|l| {
                if l.starts_with("Class ") || l == "{" && false {
                    l.to_string()
                } else if l.is_empty() {
                    String::new()
                } else {
                    format!("    {l}")
                }
            })
            .collect::<Vec<_>>()
            .join("\n")
            // The class's own braces stay at the margin.
            .replacen("    {\n", "{\n", 1)
            + "\n"
    };
    let bad = indented("^ROUTINE");
    assert!(bad.contains("\n    Storage SQLStorage\n"), "{bad}");
    let refused = put(&r, class, &bad).await;
    let stored_bad = exists(&r, class).await;
    delete(&r, class).await;
    let good = put(&r, class, &indented("^IadSm172Indented")).await;
    delete(&r, class).await;

    assert_ne!(refused["success"], true, "{refused}");
    assert!(refused.to_string().contains("ROUTINE"), "{refused}");
    assert!(
        !stored_bad,
        "the indented class over ^ROUTINE was stored: {refused}"
    );
    assert_eq!(good["success"], true, "{good}");
}

/// A new class through `iris_doc` (FR-031, warnings only): its master map's first subscript is a
/// constant, so iad cannot read the highest ID and says the counter was not checked; its index
/// map's first subscript is not a plain literal, so the index range is not checked either. The
/// class is written.
#[tokio::test]
#[ignore = "live IRIS"]
async fn test_storage_map_paths_172_iris_doc_warns_what_it_could_not_check() {
    let r = rig(&Where::live(), Toolset::Merged).await;
    let class = "IadSm172.Unchecked";
    delete(&r, class).await;
    let text = format!(
        "Class {class} Extends %Persistent [ StorageStrategy = SQLStorage ]\n{{\n\n\
         Property Name As %String;\n\nIndex NameIdx On Name;\n\n\
         Storage SQLStorage\n{{\n<SqlIdExpression>$i(^IadSm172Unchecked)</SqlIdExpression>\n\
         <SQLMap name=\"Master\">\n<Data name=\"Name\">\n<Delimiter>\"^\"</Delimiter>\n\
         <Piece>1</Piece>\n</Data>\n<Global>^IadSm172Unchecked</Global>\n\
         <Subscript name=\"1\">\n<Expression>\"x\"</Expression>\n</Subscript>\n\
         <Subscript name=\"2\">\n<Expression>{{ID}}</Expression>\n</Subscript>\n\
         <Type>data</Type>\n</SQLMap>\n<SQLMap name=\"NameIdx\">\n\
         <Global>^IadSm172UncheckedI</Global>\n\
         <Subscript name=\"1\">\n<Expression>\"a\"_\"b\"</Expression>\n</Subscript>\n\
         <Subscript name=\"2\">\n<Expression>{{Name}}</Expression>\n</Subscript>\n\
         <Subscript name=\"3\">\n<Expression>{{ID}}</Expression>\n</Subscript>\n\
         <Type>index</Type>\n</SQLMap>\n<StreamLocation>^IadSm172UncheckedS</StreamLocation>\n\
         <Type>%Storage.SQL</Type>\n}}\n\n}}\n"
    );
    let got = put(&r, class, &text).await;
    let stored = exists(&r, class).await;
    delete(&r, class).await;

    assert_eq!(got["success"], true, "{got}");
    let warnings = got["warnings"].to_string();
    assert!(
        warnings.contains("the highest ID could not be read from the master map"),
        "{got}"
    );
    assert!(
        warnings.contains("index map NameIdx: its range could not be checked"),
        "{got}"
    );
    assert!(stored, "the class was not written: {got}");
}

/// FR-031: through `iris_doc` an ID counter behind the highest stored ID is a warning, not a
/// refusal (no opt-ins there), and it names the fix.
#[tokio::test]
#[ignore = "live IRIS"]
async fn test_storage_map_paths_172_iris_doc_warns_of_a_counter_behind_the_ids() {
    let r = rig(&Where::live(), Toolset::Merged).await;
    let class = "IadSm172.Behind";
    let global = "IadSm172Behind";
    delete(&r, class).await;
    let kill = || async {
        r.call(
            "iris_global",
            json!({"action": "kill", "global_name": global, "namespace": "USER"}),
        )
        .await
    };
    kill().await;
    let seeded = r
        .call(
            "iris_global",
            json!({"action": "set", "global_name": global, "subscripts": ["5"], "value": "Ann",
                   "namespace": "USER"}),
        )
        .await;
    let text = format!(
        "Class {class} Extends %Persistent [ StorageStrategy = SQLStorage ]\n{{\n\n\
         Property Name As %String;\n\n\
         Storage SQLStorage\n{{\n<SqlIdExpression>$i(^{global})</SqlIdExpression>\n\
         <SQLMap name=\"Master\">\n<Data name=\"Name\">\n<Delimiter>\"^\"</Delimiter>\n\
         <Piece>1</Piece>\n</Data>\n<Global>^{global}</Global>\n\
         <Subscript name=\"1\">\n<Expression>{{ID}}</Expression>\n</Subscript>\n\
         <Type>data</Type>\n</SQLMap>\n<StreamLocation>^{global}S</StreamLocation>\n\
         <Type>%Storage.SQL</Type>\n}}\n\n}}\n"
    );
    let got = put(&r, class, &text).await;
    delete(&r, class).await;
    kill().await;

    assert_eq!(seeded["success"], true, "{seeded}");
    assert_eq!(got["success"], true, "{got}");
    let warnings = got["warnings"].to_string();
    assert!(
        warnings.contains("the highest ID in ^IadSm172Behind is 5"),
        "{got}"
    );
    assert!(warnings.contains("seed_counter: true"), "{got}");
}

/// FR-023: a counter iad can name but IRIS cannot read stops the write. `$i(^G(""))` passes iad's
/// literal check, and reading a null subscript fails in IRIS (`<SUBSCRIPT>`), so the check answers
/// with IRIS's error instead of the counter: `iris_storage_map` refuses with that error
/// (`IRIS_EXECUTE_ERROR`, not a map rule) and changes nothing, and
/// `iris_doc` (warnings only, FR-031) writes the class and says the counter was not checked.
#[tokio::test]
#[ignore = "live IRIS"]
async fn test_storage_map_paths_172_a_counter_iris_cannot_read() {
    let r = rig(&Where::live(), Toolset::Merged).await;
    let f = Fixture::read("Member");
    load_fixture(&r, &f).await;
    let read = || async {
        r.call(
            "iris_storage_map",
            json!({"action": "read", "class": f.class, "namespace": "USER"}),
        )
        .await["storage"]["version"]
            .clone()
    };
    let version = read().await;
    let refused = r
        .call(
            "iris_storage_map",
            json!({"action": "edit", "class": f.class, "version": version, "namespace": "USER",
                   "ops": [{"op": "set", "path": "/keywords/SqlIdExpression",
                            "value": "$i(^IadSm172Member(\"\"))"}]}),
        )
        .await;
    let after = read().await;
    let problems = drop_fixture(&r, &f).await;

    let class = "IadSm172.NullCtr";
    delete(&r, class).await;
    let text = Fixture::read("Member")
        .cls_text
        .replacen("Class IadSm172.Member ", &format!("Class {class} "), 1)
        .replacen(
            "<SqlIdExpression>$i(^IadSm172Member)</SqlIdExpression>",
            "<SqlIdExpression>$i(^IadSm172NullCtr(\"\"))</SqlIdExpression>",
            1,
        )
        .replace("^IadSm172Member", "^IadSm172NullCtr");
    let put_got = put(&r, class, &text).await;
    delete(&r, class).await;

    // The check did not run: IRIS's error, named as such, not a rule of the map.
    assert_eq!(refused["error_code"], "IRIS_EXECUTE_ERROR", "{refused}");
    assert_eq!(refused["check"], "id_counter", "{refused}");
    assert!(
        refused["error"]
            .as_str()
            .unwrap_or("")
            .contains("SUBSCRIPT"),
        "{refused}"
    );
    assert_eq!(after, version, "the refused edit changed the class");
    assert!(problems.is_empty(), "cleanup: {problems:?}");
    assert_eq!(put_got["success"], true, "{put_got}");
    assert!(
        put_got["warnings"]
            .to_string()
            .contains("could not be checked against the highest ID"),
        "{put_got}"
    );
}

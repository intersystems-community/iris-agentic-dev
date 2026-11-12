//! T060 (research R7 open item): a scratch copy of a class whose map points at the real global
//! compiles and is deleted without writing to that global (spec 172 US4).
//!
//! `check` compiles the proposed map under `IrisDevTmp.<id>` over the same globals. That is only
//! safe if neither the compile nor the delete touches the data, so this measures it: load
//! `Member`, checksum every node of its globals, compile a copy over them, delete the copy, and
//! checksum again. The real class must still answer its known question.
//!
//! Run: `IRIS_HOST=localhost IRIS_WEB_PORT=52780 cargo test --features testing --test integration \
//!   test_storage_map_scratch_172 -- --test-threads=1 --include-ignored`

use crate::live_rig_171::{rig, Rig, Where};
use crate::storage172_rig::{ask, drop_fixture, load_fixture, Fixture};
use iris_agentic_dev_core::tools::Toolset;
use serde_json::{json, Value};

/// Every global the `Member` map, its stream location and an index build could write.
const GLOBALS: [&str; 4] = [
    "^IadSm172Member",
    "^IadSm172MemberS",
    "^IadSm172MemberI",
    "^IadSm172MemberD",
];

/// `name:$Data:crc` for each global, the CRC over every node reference and value in `$Query`
/// order (the root node included).
async fn checksums(r: &Rig) -> String {
    let list = GLOBALS
        .iter()
        .map(|g| format!("\"{g}\""))
        .collect::<Vec<_>>()
        .join(",");
    let code = format!(
        "Set out=\"\" For i=1:1:$ListLength($lb({list})) {{ Set g=$List($lb({list}),i),d=$Data(@g),crc=0 \
         Set:d#2 crc=$ZCRC(g_\"=\"_@g,7,crc) Set n=g For {{ Set n=$Query(@n) Quit:n=\"\"  \
         Set crc=$ZCRC(n_\"=\"_@n,7,crc) }} Set out=out_g_\":\"_d_\":\"_crc_\";\" }} Write out"
    );
    let got = r
        .call("iris_execute", json!({"code": code, "namespace": "USER"}))
        .await;
    let out = got["output"]
        .as_str()
        .unwrap_or_default()
        .trim()
        .to_string();
    assert!(out.contains("^IadSm172Member:"), "checksum: {got}");
    out
}

/// The scratch copy: the stored class text under another name. Its table name comes from the
/// class name, so it cannot collide with the real table; the Storage block is unchanged, so it
/// names the real globals.
fn scratch_text(stored: &str, scratch: &str) -> String {
    stored.replacen("Class IadSm172.Member ", &format!("Class {scratch} "), 1)
}

async fn doc(r: &Rig, args: Value) -> Value {
    r.call("iris_doc", args).await
}

#[tokio::test]
#[ignore = "live IRIS"]
async fn test_storage_map_scratch_172_copy_over_real_globals_writes_nothing() {
    let r = rig(&Where::live(), Toolset::Merged).await;
    let f = Fixture::read("Member");
    let stored = load_fixture(&r, &f).await;
    let before = checksums(&r).await;

    let scratch = format!(
        "IrisDevTmp.Sm172Chk{}",
        &uuid::Uuid::new_v4().simple().to_string()[..10]
    );
    let name = format!("{scratch}.cls");
    let put = doc(
        &r,
        json!({"mode": "put", "name": name, "content": scratch_text(&stored, &scratch),
               "compile": true, "namespace": "USER"}),
    )
    .await;
    let after_compile = checksums(&r).await;
    let del = doc(
        &r,
        json!({"mode": "delete", "name": name, "namespace": "USER"}),
    )
    .await;
    let gone = doc(
        &r,
        json!({"mode": "get", "name": name, "namespace": "USER"}),
    )
    .await;
    let after_delete = checksums(&r).await;
    let rows = ask(&r, &f).await;
    // The checksum must see a change, or the two equalities above prove nothing.
    let poke = r
        .call(
            "iris_global",
            json!({"action": "set", "global_name": "^IadSm172Member", "subscripts": ["2"],
                   "value": "Bob^Seattle^43", "namespace": "USER"}),
        )
        .await;
    let poked = checksums(&r).await;
    let problems = drop_fixture(&r, &f).await;

    assert!(
        before.contains("^IadSm172Member:11:") && !before.contains("^IadSm172Member:11:0;"),
        "the data global was not seeded: {before}"
    );
    assert_eq!(poke["success"], true, "{poke}");
    assert_ne!(poked, before, "the checksum did not see a changed node");
    assert_eq!(put["success"], true, "scratch put and compile: {put}");
    assert_eq!(del["success"], true, "scratch delete: {del}");
    assert_ne!(
        gone["success"], true,
        "the scratch class is still there: {gone}"
    );
    assert_eq!(
        after_compile, before,
        "compiling the copy changed the globals"
    );
    assert_eq!(
        after_delete, before,
        "deleting the copy changed the globals"
    );
    let expected = f.seed["answer"].as_array().cloned().unwrap_or_default();
    assert_eq!(
        rows, expected,
        "the real class no longer answers its question"
    );
    assert!(problems.is_empty(), "cleanup: {problems:?}");
}

/// A connection and its call client to `iris-dev-iris`, for calling `compile_scratch_class`
/// directly.
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

async fn exists(r: &Rig, class: &str) -> bool {
    doc(
        r,
        json!({"mode": "get", "name": format!("{class}.cls"), "namespace": "USER"}),
    )
    .await["success"]
        == true
}

/// T061: a valid scratch source compiles and is gone afterwards, and the real globals are
/// unchanged; an invalid one (`<Type>DATA</Type>`, #5511) returns its errors and is gone too.
#[tokio::test]
#[ignore = "live IRIS"]
async fn test_storage_map_scratch_172_compile_scratch_class() {
    let r = rig(&Where::live(), Toolset::Merged).await;
    let f = Fixture::read("Member");
    let stored = load_fixture(&r, &f).await;
    let before = checksums(&r).await;
    let (conn, client) = live_conn().await;

    let ok = conn
        .compile_scratch_class(
            |name| scratch_text(&stored, name),
            "USER",
            &client,
            "Sm172Chk",
        )
        .await;
    let ok_gone = match &ok {
        Ok((name, _)) => !exists(&r, name).await,
        Err(_) => false,
    };
    let bad_text = stored.replacen("<Type>data</Type>", "<Type>DATA</Type>", 1);
    let bad = conn
        .compile_scratch_class(
            |name| scratch_text(&bad_text, name),
            "USER",
            &client,
            "Sm172Chk",
        )
        .await;
    let bad_gone = match &bad {
        Ok((name, _)) => !exists(&r, name).await,
        Err(_) => false,
    };
    let after = checksums(&r).await;
    let rows = ask(&r, &f).await;
    let problems = drop_fixture(&r, &f).await;

    assert_ne!(
        bad_text, stored,
        "the fixture has no <Type>data</Type> to break"
    );
    let (ok_name, ok_result) = ok.expect("valid scratch compile");
    assert!(ok_name.starts_with("IrisDevTmp.Sm172Chk"), "{ok_name}");
    assert!(
        ok_result.success(),
        "valid source did not compile: {ok_result:?}"
    );
    assert!(ok_gone, "{ok_name} is still in USER");
    let (bad_name, bad_result) = bad.expect("invalid scratch compile still answers");
    assert!(
        !bad_result.success(),
        "an invalid map compiled: {bad_result:?}"
    );
    assert!(
        bad_result.errors.iter().any(|e| e.contains("5511")),
        "no #5511 in {bad_result:?}"
    );
    assert!(bad_gone, "{bad_name} is still in USER");
    assert_eq!(after, before, "a scratch compile changed the real globals");
    let expected = f.seed["answer"].as_array().cloned().unwrap_or_default();
    assert_eq!(
        rows, expected,
        "the real class no longer answers its question"
    );
    assert!(problems.is_empty(), "cleanup: {problems:?}");
}

/// T062 live: the scratch copy `scratch_source` builds from each SQL-storage fixture compiles on
/// IRIS and leaves no class behind. `DefaultBook` is refused before any request: default storage
/// is read-only here (FR-002), and IRIS refuses a second class on its globals (#5564).
#[tokio::test]
#[ignore = "live IRIS"]
async fn test_storage_map_scratch_172_scratch_source_compiles_for_every_fixture() {
    use iris_agentic_dev_core::tools::storage_map::{tree, write};
    let r = rig(&Where::live(), Toolset::Merged).await;
    let (conn, client) = live_conn().await;
    let dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/storage172/classes");
    let mut names: Vec<String> = std::fs::read_dir(&dir)
        .unwrap()
        .filter_map(|e| {
            let p = e.ok()?.path();
            Some(p.file_stem()?.to_str()?.to_string())
        })
        .collect();
    names.sort();
    let mut failures = Vec::new();
    for name in &names {
        let f = Fixture::read(name);
        let stored = load_fixture(&r, &f).await;
        let span = tree::split_class(&stored).unwrap().remove(0);
        let nodes = tree::parse_body(&stored[span.body_start..span.body_end]).unwrap();
        let mut built = Err(String::new());
        let got = conn
            .compile_scratch_class(
                |n| {
                    built = write::scratch_source(&stored, &nodes, n);
                    built.clone().unwrap_or_default()
                },
                "USER",
                &client,
                "Sm172Src",
            )
            .await;
        match (&built, &got) {
            (Err(e), _) if name == "DefaultBook" && e.contains("default storage") => {}
            (Err(e), _) => failures.push(format!("{name}: scratch_source: {e}")),
            (Ok(_), _) if name == "DefaultBook" => {
                failures.push("DefaultBook: default storage got a scratch copy".into())
            }
            (_, Err(e)) => failures.push(format!("{name}: compile: {e}")),
            (Ok(text), Ok((cls, res))) => {
                if !res.success() {
                    failures.push(format!("{name}: {:?}\n{text}", res.errors));
                }
                if exists(&r, cls).await {
                    failures.push(format!("{name}: {cls} left behind"));
                }
            }
        }
        drop_fixture(&r, &f).await;
    }
    assert!(failures.is_empty(), "{}", failures.join("\n\n"));
}

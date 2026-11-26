//! Spec 173 T001 and T016: the rig loads each filer fixture, IRIS files the index global the seed
//! names, and `drop_fixture_173` leaves nothing behind.

use crate::live_rig_171::{rig, Rig, Where};
use crate::storage173_rig::*;
use iris_agentic_dev_core::tools::Toolset;

async fn live() -> Rig {
    let r = rig(&Where::live(), Toolset::Merged).await;
    drop_all(&r).await;
    r
}

/// How many nodes a global holds below its root.
async fn node_count(r: &Rig, global: &str) -> usize {
    dump_global(r, global).await.len()
}

#[tokio::test]
#[ignore]
async fn f01_to_f10_load_and_drop() {
    let r = live().await;
    for n in 1..=10 {
        let name = format!("F{n:02}");
        let f = Fixture::read(&name);
        let rows = f.rows().len();
        assert!(
            rows >= 40,
            "{name}: the 20-node floor needs at least 40 rows, has {rows}"
        );
        load_fixture_173(&r, &f).await;
        // The master holds one node per row, the index one node per row, and the class is gone.
        let master = f.seed["master_global"].as_str().unwrap();
        let index = f.seed["index_globals"][0].as_str().unwrap();
        let m = dump_global(&r, master).await;
        assert!(
            m.iter().filter(|l| l.contains("^")).count() >= rows,
            "{name}: master {m:?}"
        );
        assert_eq!(
            node_count(&r, index).await,
            rows,
            "{name}: one index node per row"
        );
        let listed = r
            .call(
                "iris_doc",
                serde_json::json!({"mode": "head", "name": format!("{}.cls", f.class), "namespace": "USER"}),
            )
            .await;
        assert_eq!(
            listed["exists"], false,
            "{name}: the class is still there: {listed}"
        );
        let left = drop_fixture_173(&r, &f).await;
        assert!(left.is_empty(), "{name}: left behind {left:?}");
    }
    drop_all(&r).await;
}

#[tokio::test]
#[ignore]
async fn a_fixture_loads_twice_after_a_drop() {
    let r = live().await;
    let f = Fixture::read("F02");
    for _ in 0..2 {
        load_fixture_173(&r, &f).await;
        let index = f.seed["index_globals"][0].as_str().unwrap();
        assert_eq!(node_count(&r, index).await, f.rows().len());
        assert!(drop_fixture_173(&r, &f).await.is_empty());
    }
    drop_all(&r).await;
}

/// The ObjectScript that turns the master's field value `v` into what the seed says the index
/// holds: the server's own documented function, so the answer is checked against IRIS, not
/// against the linker that will later claim it.
fn expected_subscript(answer: &serde_json::Value) -> String {
    let base = match answer["transform"].as_str().unwrap() {
        "exact" => "v".to_string(),
        t => {
            let m = t.to_uppercase();
            match answer["length"].as_i64() {
                Some(n) => format!("##class(%SYSTEM.SQL.Functions).{m}(v,{n})"),
                None => format!("##class(%SYSTEM.SQL.Functions).{m}(v)"),
            }
        }
    };
    match answer["piece"].as_i64() {
        Some(p) => format!("$Piece({base},\",\",{p})"),
        None => base,
    }
}

#[tokio::test]
#[ignore]
async fn the_known_answer_of_each_fixture_is_true() {
    let r = live().await;
    for n in 1..=10 {
        let name = format!("F{n:02}");
        let f = Fixture::read(&name);
        load_fixture_173(&r, &f).await;
        let answer = &f.seed["answer"];
        let piece_no = match answer["field"].as_str().unwrap() {
            "Name" => 1,
            "City" => 2,
            other => panic!("{name}: field {other}"),
        };
        let master = f.seed["master_global"].as_str().unwrap();
        let index = answer["index_global"].as_str().unwrap();
        let code = format!(
            r#"Set m={m},ix={ix},bad=0,n=0
Set id=""
For {{ Set id=$Order(@m@(id)) Quit:id=""  Set v=$Piece(@m@(id),"^",{piece_no}) Set e={expr}
  Set n=n+1 If '$Data(@ix@(e,id)) {{ Set bad=bad+1 }} }}
Write "CHECKED ",n," BAD ",bad
"#,
            m = os(master),
            ix = os(index),
            expr = expected_subscript(answer)
        );
        let out = exec(&r, &code).await;
        assert!(
            out.contains(&format!("CHECKED {} BAD 0", f.rows().len())),
            "{name}: {out}"
        );
        drop_fixture_173(&r, &f).await;
    }
    drop_all(&r).await;
}

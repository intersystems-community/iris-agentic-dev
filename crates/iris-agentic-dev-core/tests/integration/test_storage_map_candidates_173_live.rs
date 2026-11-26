//! Spec 173 T038 to T040: `propose` finds the index globals without being told. Naming only the
//! master finds a global whose name begins with it (`^M.x`, `^MAlpha`) and a sub-tree inside the
//! master (`^M("idx",...)`); more than eight candidates are capped and the rest named; a listing
//! that fails is said and the other sources still run. Known answers come from
//! `tests/fixtures/storage173/seed/<F>.json`, never from the answer under test.
//!
//! Run: `IRIS_HOST=localhost IRIS_WEB_PORT=52780 nice -n 15 cargo test --features testing -j 2 \
//!   --test integration test_storage_map_candidates_173 -- --test-threads=1 --include-ignored`

use crate::live_rig_171::Rig;
use crate::storage173_rig::*;
use crate::test_storage_map_link_173_live::{draft_name, live};
use iris_agentic_dev_core::tools::storage_map::candidates::LISTING_QUERY_FOR_TESTS;
use serde_json::{json, Value};

/// `propose` over the fixture's master alone: no `index_globals`.
async fn master_only(r: &Rig, f: &Fixture, extra: Value) -> Value {
    let mut args = json!({"global": f.seed["master_global"]});
    for (k, v) in extra.as_object().into_iter().flatten() {
        args[k] = v.clone();
    }
    let out = ask_propose(r, args).await;
    assert_eq!(out["success"], true, "{}: {out}", f.name);
    out
}

async fn done(r: &Rig, f: &Fixture) {
    drop_fixture_173(r, f).await;
    drop_all(r).await;
}

/// The candidate listed for `global` with the literal first subscripts `prefix` cut off.
fn candidate<'a>(out: &'a Value, global: &str, prefix: &[&str]) -> &'a Value {
    out["index_candidates"]["candidates"]
        .as_array()
        .and_then(|a| {
            a.iter().find(|c| {
                c["global"] == json!(global)
                    && c["sub_prefix"].as_array().map(Vec::as_slice)
                        == Some(
                            prefix
                                .iter()
                                .map(|p| json!(p))
                                .collect::<Vec<_>>()
                                .as_slice(),
                        )
            })
        })
        .unwrap_or_else(|| {
            panic!(
                "no candidate {global} {prefix:?} in {}",
                out["index_candidates"]
            )
        })
}

/// The `indexes` item for `global` found by `found_by`.
fn link<'a>(out: &'a Value, global: &str, found_by: &str) -> &'a Value {
    out["indexes"]
        .as_array()
        .and_then(|a| {
            a.iter()
                .find(|l| l["global"] == json!(global) && l["found_by"] == json!(found_by))
        })
        .unwrap_or_else(|| panic!("no link {global} by {found_by} in {}", out["indexes"]))
}

fn not_inferred_says(out: &Value, what: &str) -> bool {
    out["not_inferred"]
        .as_array()
        .into_iter()
        .flatten()
        .any(|s| s.as_str().is_some_and(|s| s.contains(what)))
        && out["not_inferred_detail"]
            .as_array()
            .into_iter()
            .flatten()
            .any(|d| d["what"].as_str().is_some_and(|w| w.contains(what)))
}

/// `^IadSm173F47` finds `^IadSm173F47.x` by its name and links it, with no `index_globals`.
#[tokio::test]
#[ignore]
async fn a_global_named_for_the_master_is_found_by_name_prefix_and_linked() {
    let r = live().await;
    let f = Fixture::read("F47");
    load_fixture_173(&r, &f).await;
    let out = master_only(&r, &f, json!({})).await;
    assert_eq!(out["index_candidates"]["listing"], "ok", "{out}");
    let want = f.seed["answer"]["index_global"].as_str().unwrap();
    let c = candidate(&out, want, &[]);
    assert_eq!(c["found_by"], "name_prefix", "{c}");
    assert_eq!(c["status"], "examined", "{c}");
    let l = link(&out, want, "name_prefix");
    assert_eq!(l["verdict"], f.seed["answer"]["link_verdict"], "{l}");
    assert_eq!(
        l["id"]["positions"], f.seed["answer"]["id_positions"],
        "{l}"
    );
    let sub = &l["subscripts"][0];
    assert_eq!(sub["field"], json!(draft_name(&f, "Name")), "{sub}");
    assert_eq!(sub["transform"], "sqlupper", "{sub}");
    assert!(
        !not_inferred_says(&out, "namespace listing was not available"),
        "{}",
        out["not_inferred"]
    );
    assert_eq!(out["limits"]["stopped"], json!([]), "{}", out["limits"]);
    done(&r, &f).await;
}

/// F31: a global that shares the master's prefix and links to nothing is listed, found by name,
/// with the verdict `not_linked` and the best agreement it reached.
#[tokio::test]
#[ignore]
async fn f31_an_unrelated_prefix_global_is_listed_not_linked_never_dropped() {
    let r = live().await;
    let f = Fixture::read("F31");
    load_fixture_173(&r, &f).await;
    let out = master_only(&r, &f, json!({})).await;
    let unrelated = f.seed["answer"]["index_global"].as_str().unwrap();
    assert_eq!(candidate(&out, unrelated, &[])["found_by"], "name_prefix");
    let l = link(&out, unrelated, "name_prefix");
    assert_eq!(l["verdict"], f.seed["answer"]["link_verdict"], "{l}");
    assert_eq!(
        l["id"]["agreement"].as_f64(),
        f.seed["answer"]["id_agreement"].as_f64(),
        "{l}"
    );
    assert!(l.get("map").is_none(), "a map was drafted: {l}");
    done(&r, &f).await;
}

/// F32: the index lives inside the master under the literal `"idx"`. It is a `sub_node`
/// candidate, linked with the prefix cut off, and its drafted map spells the literal level.
#[tokio::test]
#[ignore]
async fn f32_an_index_inside_the_master_is_a_sub_node_linked_with_the_prefix_cut_off() {
    let r = live().await;
    let f = Fixture::read("F32");
    load_fixture_173(&r, &f).await;
    let out = master_only(&r, &f, json!({})).await;
    let master = f.seed["master_global"].as_str().unwrap();
    let prefix: Vec<&str> = f.seed["answer"]["sub_prefix"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(Value::as_str)
        .collect();
    let c = candidate(&out, master, &prefix);
    assert_eq!(c["found_by"], "sub_node", "{c}");
    assert_eq!(c["status"], "examined", "{c}");
    let l = link(&out, master, "sub_node");
    assert_eq!(l["verdict"], f.seed["answer"]["link_verdict"], "{l}");
    // Positions count from after the prefix: the upper-cased name, then the row ID.
    assert_eq!(
        l["id"]["positions"], f.seed["answer"]["id_positions"],
        "{l}"
    );
    let sub = &l["subscripts"][0];
    assert_eq!(sub["position"], 1, "{sub}");
    assert_eq!(sub["field"], json!(draft_name(&f, "Name")), "{sub}");
    assert_eq!(sub["transform"], "sqlupper", "{sub}");
    // The map's levels: the literal, then what the link found.
    let levels: Vec<&str> = l["map"]["subscripts"]
        .as_array()
        .unwrap_or_else(|| panic!("no drafted map: {l}"))
        .iter()
        .map(|s| s["expression"].as_str().unwrap())
        .collect();
    assert_eq!(
        levels,
        vec![
            "\"idx\"".to_string(),
            format!("$$SQLUPPER({{{}}})", draft_name(&f, "Name")),
            "{ID}".to_string()
        ],
        "{l}"
    );
    assert_eq!(l["map"]["global"], json!(master), "{l}");
    assert_eq!(l["verified"]["compiled"], true, "{l}");
    assert_eq!(l["verified"]["row_found"], true, "{l}");
    // The class's own map X over ^IadSm173F32X is found by its name too, and linked the same way.
    let x = f.seed["answer"]["index_global"].as_str().unwrap();
    assert_eq!(link(&out, x, "name_prefix")["verdict"], "inferred");
    // Two drafts, two names.
    let names: Vec<&str> = out["indexes"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|i| i["map"]["name"].as_str())
        .collect();
    assert_eq!(names.len(), 2, "{}", out["indexes"]);
    assert_ne!(names[0], names[1]);
    done(&r, &f).await;
}

/// F48: ten globals begin with the master's name. Eight are examined; the other two are named in
/// `index_candidates.not_examined` and listed with status `not_examined`. A name the caller gives
/// is examined first, so the cap drops the listing's later names, never the caller's.
#[tokio::test]
#[ignore]
async fn ten_candidates_link_eight_and_name_the_other_two() {
    let r = live().await;
    let f = Fixture::read("F48");
    load_fixture_173(&r, &f).await;
    let strings = |v: &Value| -> Vec<String> {
        v.as_array()
            .unwrap()
            .iter()
            .filter_map(|x| x.as_str().map(String::from))
            .collect()
    };

    let out = master_only(&r, &f, json!({})).await;
    let linked: Vec<String> = out["indexes"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|l| l["global"].as_str().map(String::from))
        .collect();
    assert_eq!(linked, strings(&f.seed["answer"]["examined"]), "{out}");
    assert_eq!(
        strings(&out["index_candidates"]["not_examined"]),
        strings(&f.seed["answer"]["not_examined"]),
        "{}",
        out["index_candidates"]
    );
    for name in strings(&f.seed["answer"]["not_examined"]) {
        let c = candidate(&out, &name, &[]);
        assert_eq!(c["status"], "not_examined", "{c}");
        assert!(c["reason"].as_str().unwrap().contains('8'), "{c}");
    }
    assert!(
        not_inferred_says(&out, &strings(&f.seed["answer"]["not_examined"])[0]),
        "the cap is not in not_inferred: {}",
        out["not_inferred"]
    );

    // The caller names the last one: it is examined, and the cap drops two later listed names.
    let last = strings(&f.seed["answer"]["not_examined"])[1].clone();
    let out = master_only(&r, &f, json!({"index_globals": [last]})).await;
    assert_eq!(
        out["indexes"][0]["global"],
        json!(last),
        "{}",
        out["indexes"]
    );
    assert_eq!(out["indexes"][0]["found_by"], "parameter");
    assert_eq!(
        out["indexes"].as_array().unwrap().len(),
        8,
        "{}",
        out["indexes"]
    );
    let want_left: Vec<String> = strings(&f.seed["answer"]["candidates"])[7..9].to_vec();
    assert_eq!(
        strings(&out["index_candidates"]["not_examined"]),
        want_left,
        "{}",
        out["index_candidates"]
    );
    done(&r, &f).await;
}

/// Puts the listing's query name back when the test ends, on a panic too.
struct QueryOverride;

impl QueryOverride {
    fn set(query: &str) -> Self {
        *LISTING_QUERY_FOR_TESTS.lock().unwrap() = Some(query.to_string());
        QueryOverride
    }
}

impl Drop for QueryOverride {
    fn drop(&mut self) {
        *LISTING_QUERY_FOR_TESTS
            .lock()
            .unwrap_or_else(|e| e.into_inner()) = None;
    }
}

/// T040: the listing fails. On this container the service account can always list (a user with
/// only `%DB_USER` listed the namespace's globals when measured), and no namespace the account
/// cannot read leaves the master readable, so the failure is made by naming a class query that
/// does not exist: IRIS itself refuses it and the generated code reports IRIS's own text. The
/// answer says the listing is unavailable, says why, still links the name the caller gave and the
/// sub-tree inside the master, and `not_inferred` carries it.
#[tokio::test]
#[ignore]
async fn a_failed_listing_is_said_and_the_other_two_sources_still_link() {
    let r = live().await;
    let f = Fixture::read("F32");
    load_fixture_173(&r, &f).await;
    let explicit = f.seed["answer"]["index_global"].as_str().unwrap();
    let master = f.seed["master_global"].as_str().unwrap();
    let out = {
        let _q = QueryOverride::set("IadSm173.NoSuchClass:NoSuchQuery");
        master_only(&r, &f, json!({"index_globals": [explicit]})).await
    };
    let listing = &out["index_candidates"];
    assert_eq!(listing["listing"], "unavailable", "{listing}");
    let why = listing["listing_reason"].as_str().unwrap_or_default();
    // IRIS's own words for the missing query, with the query's name in them.
    assert!(
        why.contains("does not exist") && why.contains("NoSuchClass:NoSuchQuery"),
        "IRIS's own text: {listing}"
    );
    // The explicit name and the in-master sub-tree are examined and linked.
    assert_eq!(candidate(&out, explicit, &[])["found_by"], "parameter");
    assert_eq!(candidate(&out, master, &["idx"])["found_by"], "sub_node");
    assert_eq!(link(&out, explicit, "parameter")["verdict"], "inferred");
    assert_eq!(link(&out, master, "sub_node")["verdict"], "inferred");
    // Nothing came from the listing.
    let by_name = out["index_candidates"]["candidates"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|c| c["found_by"] == "name_prefix")
        .count();
    assert_eq!(by_name, 0, "{listing}");
    assert!(
        not_inferred_says(&out, "namespace listing was not available"),
        "{} / {}",
        out["not_inferred"],
        out["not_inferred_detail"]
    );
    // The same call with the real query lists, and the two globals above are all it adds.
    let ok = master_only(&r, &f, json!({"index_globals": [explicit]})).await;
    assert_eq!(ok["index_candidates"]["listing"], "ok", "{ok}");
    assert!(!not_inferred_says(
        &ok,
        "namespace listing was not available"
    ));
    done(&r, &f).await;
}

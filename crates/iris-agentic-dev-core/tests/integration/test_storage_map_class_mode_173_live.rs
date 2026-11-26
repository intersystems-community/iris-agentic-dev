//! Spec 173 T041 and T042: `propose class=...` drafts the index maps a class lacks, in the
//! class's own property names and row-ID parts. The master is the class's master map and is not
//! re-sampled; an index global the class already maps is `already_mapped`; storage iad only reads
//! is refused with `read`'s reason. Known answers come from `tests/fixtures/storage173/seed`.
//!
//! Run: `IRIS_HOST=localhost IRIS_WEB_PORT=52780 nice -n 15 cargo test --features testing -j 2 \
//!   --test integration test_storage_map_class_mode_173 -- --test-threads=1 --include-ignored`

use crate::live_rig_171::Rig;
use crate::storage173_rig::*;
use crate::test_storage_map_link_173_live::live;
use serde_json::{json, Value};

/// Load a fixture and keep its class: the globals and rows, the hand-written index, the class.
async fn load_keeping_class(r: &Rig, f: &Fixture) {
    put_class(r, &f.class, &f.cls_text).await;
    let columns = f.columns();
    let cols: Vec<&str> = columns.iter().map(String::as_str).collect();
    insert_rows(
        r,
        f.seed["table"].as_str().unwrap_or(&f.class),
        &cols,
        &f.rows(),
    )
    .await;
    if let Some(code) = f.seed["post_load"].as_str() {
        exec(r, code).await;
    }
}

async fn done(r: &Rig, f: &Fixture) {
    drop_fixture_173(r, f).await;
    drop_all(r).await;
}

/// `propose class=<fixture class>` with `extra` params.
async fn propose_class(r: &Rig, class: &str, extra: Value) -> Value {
    let mut args = json!({"class": class});
    for (k, v) in extra.as_object().into_iter().flatten() {
        args[k] = v.clone();
    }
    ask_propose(r, args).await
}

/// Put a class's text on the server without compiling it: `propose` only reads the text.
async fn put_uncompiled(r: &Rig, class: &str, text: &str) {
    let put = r
        .call(
            "iris_doc",
            json!({"mode": "put", "name": format!("{class}.cls"), "content": text,
                "compile": false, "namespace": "USER"}),
        )
        .await;
    assert_eq!(put["success"], true, "{class}: {put}");
}

/// F35: the class has a master map and the index global beside it is unmapped. The draft names
/// the class's own property, spells the row ID as the class does, verifies, and `check` compiles
/// the class with the draft added.
#[tokio::test]
#[ignore]
async fn f35_drafts_the_missing_index_in_the_classs_own_names() {
    let r = live().await;
    let f = Fixture::read("F35");
    load_keeping_class(&r, &f).await;
    let out = propose_class(&r, &f.class, json!({})).await;
    assert_eq!(out["success"], true, "{out}");
    // The master is the class's: not sampled, not drafted.
    for key in ["draft", "shape", "guesses"] {
        assert!(out.get(key).is_none(), "class mode returned {key}: {out}");
    }
    assert_eq!(out["class"], json!(f.class), "{out}");
    assert_eq!(out["global"], f.seed["master_global"], "{out}");
    let index = f.seed["answer"]["index_global"].as_str().unwrap();
    let cand = out["index_candidates"]["candidates"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["global"] == json!(index))
        .unwrap_or_else(|| panic!("no candidate {index}: {}", out["index_candidates"]));
    assert_eq!(cand["found_by"], "name_prefix", "{cand}");
    assert_eq!(cand["status"], "examined", "{cand}");
    let link = &out["indexes"][0];
    assert_eq!(link["global"], json!(index), "{link}");
    assert_eq!(link["verdict"], f.seed["answer"]["link_verdict"], "{link}");
    assert_eq!(
        link["id"]["positions"], f.seed["answer"]["id_positions"],
        "{link}"
    );
    let sub = &link["subscripts"][0];
    // `Name`, not `Field1`: the class's property.
    assert_eq!(
        sub["field"], f.seed["answer"]["subscripts"][0]["field"],
        "{sub}"
    );
    assert_eq!(sub["transform"], "sqlupper", "{sub}");
    let exprs: Vec<&str> = link["map"]["subscripts"]
        .as_array()
        .unwrap_or_else(|| panic!("no drafted map: {link}"))
        .iter()
        .map(|s| s["expression"].as_str().unwrap())
        .collect();
    assert_eq!(exprs, vec!["$$SQLUPPER({Name})", "{ID}"], "{link}");
    assert_eq!(link["verified"]["compiled"], true, "{link}");
    assert_eq!(link["verified"]["row_found"], true, "{link}");
    assert_ne!(link["map"]["name"], "Master", "{link}");
    // No master placeholder is a guess here; the index's own guesses are about its map.
    assert!(
        !out.to_string().contains("Field1"),
        "a placeholder name reached the answer: {out}"
    );
    assert!(
        !out["not_inferred"].to_string().contains("field names"),
        "{}",
        out["not_inferred"]
    );
    // `check` takes the draft as the class's new map and compiles the class.
    let checked = ask_check(&r, json!({"class": f.class, "model": link["map"].clone()})).await;
    assert_eq!(checked["success"], true, "{checked}");
    assert_eq!(checked["compiled"], true, "{checked}");
    done(&r, &f).await;
}

/// F36: the class already maps the index global. It is `already_mapped`, names the map, and
/// nothing is drafted for it.
#[tokio::test]
#[ignore]
async fn f36_an_index_global_the_class_maps_is_already_mapped_and_drafts_nothing() {
    let r = live().await;
    let f = Fixture::read("F36");
    load_keeping_class(&r, &f).await;
    let out = propose_class(&r, &f.class, json!({})).await;
    assert_eq!(out["success"], true, "{out}");
    let (global, map) = f.seed["answer"]["already_mapped"]
        .as_object()
        .unwrap()
        .iter()
        .next()
        .unwrap();
    let cand = out["index_candidates"]["candidates"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["global"] == json!(global))
        .unwrap_or_else(|| panic!("no candidate {global}: {}", out["index_candidates"]));
    assert_eq!(cand["status"], "already_mapped", "{cand}");
    assert_eq!(cand["reason"], *map, "{cand}");
    assert!(out.get("indexes").is_none(), "something was drafted: {out}");
    assert_eq!(out["index_candidates"]["not_examined"], json!([]), "{out}");
    // Naming it by hand does not link it either.
    let again = propose_class(&r, &f.class, json!({"index_globals": [global]})).await;
    assert_eq!(again["success"], true, "{again}");
    assert!(again.get("indexes").is_none(), "{again}");
    done(&r, &f).await;
}

/// Storage iad only reads, and a class with no master map, are refused with the reason `read`
/// gives; a class that is not there is `STORAGE_MAP_NOT_FOUND`.
#[tokio::test]
#[ignore]
async fn a_class_whose_storage_is_not_editable_is_refused_with_reads_reason() {
    let r = live().await;
    // Default storage, compiled by IRIS.
    put_class(
        &r,
        "IadSm173.Dflt",
        "Class IadSm173.Dflt Extends %Persistent\n{\n\nProperty Name As %String;\n}\n",
    )
    .await;
    let read = r
        .call(
            "iris_storage_map",
            json!({"action": "read", "class": "IadSm173.Dflt", "namespace": "USER"}),
        )
        .await;
    let why = read["storage"]["not_editable_reason"]
        .as_str()
        .unwrap_or_else(|| panic!("read gave no reason: {read}"))
        .to_string();
    let out = propose_class(&r, "IadSm173.Dflt", json!({})).await;
    assert_eq!(out["success"], false, "{out}");
    assert_eq!(out["error_code"], "STORAGE_MAP_READ_ONLY", "{out}");
    assert!(
        out["error"].as_str().unwrap().contains(&why),
        "{out} / {why}"
    );

    // A columnar map and a class with no data map, as text on the server.
    let storage = |maps: &str| {
        format!(
            "Class IadSm173.Txt Extends %Persistent [ StorageStrategy = SQLStorage ]\n{{\n\nStorage SQLStorage\n{{\n{maps}<Type>%Storage.SQL</Type>\n}}\n\n}}\n"
        )
    };
    let master = "<SQLMap name=\"Master\">\n<Global>^IadSm173Txt</Global>\n<Subscript name=\"1\">\n<Expression>{ID}</Expression>\n</Subscript>\n<Type>data</Type>\n</SQLMap>\n";
    let columnar =
        "<SQLMap name=\"Col\">\n<Global>^IadSm173TxtC</Global>\n<Type>columnar</Type>\n</SQLMap>\n";
    let index_only = "<SQLMap name=\"Ix\">\n<Global>^IadSm173TxtX</Global>\n<Subscript name=\"1\">\n<Expression>{Name}</Expression>\n</Subscript>\n<Type>index</Type>\n</SQLMap>\n";
    put_uncompiled(&r, "IadSm173.Txt", &storage(&format!("{master}{columnar}"))).await;
    let out = propose_class(&r, "IadSm173.Txt", json!({})).await;
    assert_eq!(out["error_code"], "STORAGE_MAP_READ_ONLY", "{out}");
    assert!(out["error"].as_str().unwrap().contains("columnar"), "{out}");
    put_uncompiled(&r, "IadSm173.Txt", &storage(index_only)).await;
    let out = propose_class(&r, "IadSm173.Txt", json!({})).await;
    assert_eq!(out["success"], false, "{out}");
    assert!(
        out["error"].as_str().unwrap().contains("no data map"),
        "{out}"
    );

    let out = propose_class(&r, "IadSm173.NoSuchClass", json!({})).await;
    assert_eq!(out["error_code"], "STORAGE_MAP_NOT_FOUND", "{out}");
    drop_all(&r).await;
}

/// One master per call, and a class names its own properties.
#[tokio::test]
#[ignore]
async fn class_with_global_or_field_names_is_invalid_params() {
    let r = live().await;
    let both = ask_propose(&r, json!({"class": "IadSm173.X", "global": "^IadSm173X"})).await;
    assert_eq!(both["error_code"], "INVALID_PARAMS", "{both}");
    let named = ask_propose(
        &r,
        json!({"class": "IadSm173.X", "field_names": ["A", "B"]}),
    )
    .await;
    assert_eq!(named["error_code"], "INVALID_PARAMS", "{named}");
    assert!(
        named["error"].as_str().unwrap().contains("field_names"),
        "{named}"
    );
    let neither = ask_propose(&r, json!({})).await;
    assert_eq!(neither["error_code"], "INVALID_PARAMS", "{neither}");
    drop_all(&r).await;
}

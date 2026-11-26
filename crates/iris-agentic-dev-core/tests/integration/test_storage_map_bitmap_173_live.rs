//! Spec 173 T048 to T050: a bitmap index is recognised, decoded and drafted as `Type bitmap`, and a
//! `^G(const, chunk) = $BIT` master gets its computed-level data map. Every known answer comes from
//! `tests/fixtures/storage173/seed/<F>.json` (the IDs the rows were given, the chunk positions the
//! engine files), never from the answer under test.
//!
//! Run: `IRIS_HOST=localhost IRIS_WEB_PORT=52780 nice -n 15 cargo test --features testing -j 2 \
//!   --test integration test_storage_map_bitmap_173 -- --test-threads=1 --include-ignored`

use crate::live_rig_171::Rig;
use crate::storage173_rig::*;
use crate::test_storage_map_link_173_live::live;
use iris_agentic_dev_core::tools::storage_map::bitmap::parse_master_bits;
use iris_agentic_dev_core::tools::storage_map::link_code::{master_bit_probe, Caps};
use serde_json::{json, Value};

async fn bitmap_of(r: &Rig, f: &Fixture) -> Value {
    load_fixture_173(r, f).await;
    let master = f.seed["master_global"].as_str().unwrap();
    let index = f.seed["index_globals"][0].as_str().unwrap();
    let out = ask_propose(r, json!({"global": master, "index_globals": [index]})).await;
    assert_eq!(out["success"], true, "{}: {out}", f.name);
    out
}

async fn done(r: &Rig, f: &Fixture) {
    drop_fixture_173(r, f).await;
    drop_all(r).await;
}

/// The bit positions a dump line holds: `^G("A",1)=<bits:2,5>` is `[2, 5]`.
fn dumped_positions(line: &str) -> Vec<u64> {
    let bits = line.split_once("=<bits:").expect("a bit string").1;
    bits.trim_end_matches('>')
        .split(',')
        .filter(|p| !p.is_empty())
        .map(|p| p.parse().unwrap())
        .collect()
}

/// The fixture's own bit strings are the ones the seed's IDs name (bit position = ID + 1).
async fn assert_fixture_bits_are_the_seed_ids(r: &Rig, f: &Fixture, global: &str) {
    let lines = dump_global(r, global).await;
    let by_status = f.seed["answer"]["ids_by_status"].as_object().unwrap();
    let mut checked = 0;
    for (status, ids) in by_status {
        let line = lines
            .iter()
            .find(|l| {
                l.to_uppercase()
                    .contains(&format!("\"{}\"", status.to_uppercase()))
            })
            .unwrap_or_else(|| panic!("{}: no node for {status} in {lines:?}", f.name));
        let want: Vec<u64> = ids
            .as_array()
            .unwrap()
            .iter()
            .map(|i| i.as_u64().unwrap() + 1)
            .collect();
        assert_eq!(dumped_positions(line), want, "{}: {status}", f.name);
        checked += 1;
    }
    assert!(
        checked >= 3,
        "{}: fewer statuses than the fixture holds",
        f.name
    );
}

fn subscript_expressions(map: &Value) -> Vec<String> {
    map["subscripts"]
        .as_array()
        .unwrap_or_else(|| panic!("no subscripts in {map}"))
        .iter()
        .map(|s| s["expression"].as_str().unwrap().to_string())
        .collect()
}

/// F19: a map-compiled bitmap. The chunk is the last subscript, every ID found, the draft is a
/// bitmap map over the field alone, and it verified in a scratch copy.
#[tokio::test]
#[ignore]
async fn f19_map_compiled_bitmap_is_decoded_drafted_and_verified() {
    let r = live().await;
    let f = Fixture::read("F19");
    let out = bitmap_of(&r, &f).await;
    assert_fixture_bits_are_the_seed_ids(&r, &f, "^IadSm173F19X").await;
    let answer = &f.seed["answer"];
    let items = out["bitmaps"]
        .as_array()
        .unwrap_or_else(|| panic!("no bitmaps in {out}"));
    assert_eq!(items.len(), 1, "{items:?}");
    let b = &items[0];
    assert_eq!(b["global"], answer["index_global"], "{b}");
    assert_eq!(b["chunk_position"], answer["chunk_position"], "{b}");
    assert_eq!(b["ids_checked"], answer["ids_checked"], "{b}");
    assert_eq!(b["ids_found_in_master"], answer["ids_found"], "{b}");
    assert_eq!(b["verdict"], answer["link_verdict"], "{b}");
    assert_eq!(b["map"]["type"], "bitmap", "{b}");
    // The field alone: Status is piece 5, which the master draft calls Field5.
    assert_eq!(subscript_expressions(&b["map"]), ["{Field5}"], "{b}");
    assert_eq!(b["verified"]["compiled"], true, "{b}");
    assert_eq!(b["verified"]["row_found"], true, "{b}");
    // The link item for the same candidate agrees.
    assert_eq!(
        out["indexes"][0]["verdict"], answer["link_verdict"],
        "{}",
        out["indexes"][0]
    );
    done(&r, &f).await;
}

/// F20: a class-compiled bitmap holds the index name first and the chunk last.
#[tokio::test]
#[ignore]
async fn f20_class_compiled_bitmap_has_the_index_name_first_and_the_chunk_last() {
    let r = live().await;
    let f = Fixture::read("F20");
    let out = bitmap_of(&r, &f).await;
    let answer = &f.seed["answer"];
    let b = &out["bitmaps"][0];
    assert_eq!(b["chunk_position"], answer["chunk_position"], "{b}");
    assert_eq!(b["index_name"], answer["lead"], "{b}");
    assert_eq!(b["ids_checked"], answer["ids_checked"], "{b}");
    assert_eq!(b["ids_found_in_master"], answer["ids_found"], "{b}");
    assert_eq!(b["verdict"], answer["link_verdict"], "{b}");
    assert_eq!(b["map"]["type"], "bitmap", "{b}");
    // The index name is a constant level, then the field under the collation the class gave it.
    // Status is the sixth element of the $LIST master, which the draft calls Field6.
    assert_eq!(
        subscript_expressions(&b["map"]),
        ["\"StatusIdx\"", "$$SQLUPPER({Field6})"],
        "{b}"
    );
    assert_eq!(b["verified"]["row_found"], true, "{b}");
    done(&r, &f).await;
}

/// A bit string whose decoded IDs are not in the master is `not_linked`, and no map is drafted.
#[tokio::test]
#[ignore]
async fn a_bitmap_whose_ids_are_not_in_the_master_is_not_linked() {
    let r = live().await;
    let f = Fixture::read("F19n");
    let out = bitmap_of(&r, &f).await;
    let b = &out["bitmaps"][0];
    assert_eq!(b["verdict"], "not_linked", "{b}");
    assert_eq!(
        b["ids_found_in_master"], f.seed["answer"]["ids_found"],
        "{b}"
    );
    assert!(b["ids_checked"].as_u64().unwrap() > 0, "{b}");
    assert!(b.get("map").is_none(), "{b}");
    done(&r, &f).await;
}

/// IDs from chunk 2 on that find no row leave the chunk size unconfirmed: `ambiguous`, and
/// `assumed` names the size.
#[tokio::test]
#[ignore]
async fn a_decode_that_needs_an_unconfirmed_chunk_size_is_ambiguous_and_says_so() {
    let r = live().await;
    let f = Fixture::read("F19c");
    let out = bitmap_of(&r, &f).await;
    let answer = &f.seed["answer"];
    let b = &out["bitmaps"][0];
    assert_eq!(b["verdict"], answer["link_verdict"], "{b}");
    assert_eq!(b["ids_checked"], answer["ids_checked"], "{b}");
    assert_eq!(b["ids_found_in_master"], answer["ids_found"], "{b}");
    let assumed = b["assumed"].to_string();
    assert!(assumed.contains(answer["assumed"].as_str().unwrap()), "{b}");
    done(&r, &f).await;
}

/// F21 has no class: its seed holds the code that writes the bit strings by hand.
async fn load_f21(r: &Rig) -> Value {
    let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/storage173/seed/F21.json");
    let seed: Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    exec(r, seed["post_load"].as_str().unwrap()).await;
    seed
}

/// F21: `^IadSm173Bits(0, chunk) = $BIT` gets a data map with the computed third level, and the
/// scratch copy returns the IDs the bits name: their count and their sum are the seed's.
#[tokio::test]
#[ignore]
async fn f21_a_bit_string_master_gets_a_chunk_data_map_that_returns_the_ids() {
    let r = live().await;
    let seed = load_f21(&r).await;
    let out = ask_propose(&r, json!({"global": "^IadSm173Bits"})).await;
    assert_eq!(out["success"], true, "{out}");
    let draft = &out["chunk_map_draft"];
    let answer = &seed["answer"];
    let map = &draft["map"];
    assert_eq!(map["type"], "data", "{draft}");
    assert_eq!(map["global"], "^IadSm173Bits", "{draft}");
    let subs = map["subscripts"].as_array().unwrap();
    assert_eq!(subs.len(), 3, "{draft}");
    assert_eq!(subs[0]["expression"], answer["constant"], "{draft}");
    assert_eq!(subs[2]["access_type"], "Other", "{draft}");
    assert!(
        subs[2]["next_code"].as_str().unwrap().contains("$bitfind"),
        "{draft}"
    );
    assert_eq!(draft["verified"]["compiled"], true, "{draft}");
    assert_eq!(draft["verified"]["row_found"], true, "{draft}");
    assert_eq!(draft["verified"]["rows"], answer["id_count"], "{draft}");
    assert_eq!(draft["verified"]["id_sum"], answer["id_sum"], "{draft}");
    assert!(
        out["guesses"]
            .to_string()
            .contains("/maps/Master/subscripts/3")
            || draft["guesses"].to_string().contains("subscripts/3"),
        "the computed level is a guess: {draft}"
    );
    drop_all(&r).await;
}

/// T049: the probe recognises F21's master and not an ordinary delimited master, and it writes
/// counts and positions only (the constant is the one name it returns).
#[tokio::test]
#[ignore]
async fn the_master_probe_recognises_a_bit_master_and_not_a_delimited_one() {
    let r = live().await;
    let seed = load_f21(&r).await;
    let code = master_bit_probe("^IadSm173Bits", Caps::for_node_cap(200)).unwrap();
    let probed = parse_master_bits(&exec(&r, &code).await)
        .unwrap()
        .expect("a bit master");
    let answer = &seed["answer"];
    assert_eq!(json!(probed.constant), answer["constant"]);
    assert_eq!(json!(probed.ids), answer["id_count"]);
    assert_eq!(json!(probed.id_sum), answer["id_sum"]);
    assert_eq!(json!(probed.chunks), answer["chunks"]);
    assert_eq!(probed.first, answer["ids"][0].as_u64().unwrap());
    assert_eq!(probed.last, answer["ids"][5].as_u64().unwrap());
    drop_all(&r).await;

    let plain = Fixture::read("F01");
    load_fixture_173(&r, &plain).await;
    let master = plain.seed["master_global"].as_str().unwrap();
    let code = master_bit_probe(master, Caps::for_node_cap(200)).unwrap();
    assert_eq!(parse_master_bits(&exec(&r, &code).await).unwrap(), None);
    // And propose on it drafts no chunk map.
    let out = ask_propose(&r, json!({"global": master})).await;
    assert!(out.get("chunk_map_draft").is_none(), "{out}");
    done(&r, &plain).await;
}

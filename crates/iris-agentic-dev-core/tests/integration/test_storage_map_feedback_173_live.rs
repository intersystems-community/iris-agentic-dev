//! Spec 173, T073 to T075 (Benjamin De Boe's feedback on the pre-release, 2026-11-26), live.
//!
//! Fixtures are the 172 ones (`IadSm172.Bits`, the look-alike of his iFind map, and `IadSm172.Member`)
//! and two classes written here under `IadSm173`. Globals are killed by each test.
//!
//! Run: `IRIS_HOST=localhost IRIS_WEB_PORT=52780 cargo test --features testing --test integration \
//!   test_storage_map_feedback_173 -- --test-threads=1 --include-ignored`

use crate::live_rig_171::{rig, Rig, Where};
use crate::storage172_rig::{drop_fixture, load_fixture, Fixture};
use crate::storage173_rig::put_class;
use iris_agentic_dev_core::tools::Toolset;
use serde_json::{json, Value};

async fn call(r: &Rig, args: Value) -> Value {
    r.call("iris_storage_map", args).await
}

async fn on(r: &Rig, action: &str, class: &str) -> Value {
    call(
        r,
        json!({"action": action, "class": class, "namespace": "USER"}),
    )
    .await
}

async fn kill(r: &Rig, global: &str) {
    let k = r
        .call(
            "iris_global",
            json!({"action": "kill", "global_name": global, "namespace": "USER"}),
        )
        .await;
    assert_eq!(k["success"], true, "{k}");
}

async fn delete_class(r: &Rig, class: &str) {
    r.call(
        "iris_doc",
        json!({"mode": "delete", "name": format!("{class}.cls"), "namespace": "USER"}),
    )
    .await;
}

/// The `IadSm172.Member` text with its own names moved under `IadSm173` and `extends` after the
/// class name, so a class with and without a superclass that may carry streams can be put.
fn member_class(name: &str, global: &str, extends: &str) -> String {
    let text = std::fs::read_to_string(
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/storage172/classes/Member.cls"),
    )
    .unwrap();
    text.replace(
        "IadSm172.Member Extends %Persistent",
        &format!("{name} Extends {extends}"),
    )
    .replace("^IadSm172MemberS", &format!("{global}S"))
    .replace("^IadSm172Member", global)
}

// ── T073 ─────────────────────────────────────────────────────────────────────

#[tokio::test]
#[ignore]
async fn test_storage_map_feedback_173_read_leaves_out_the_compiler_added_stream_location() {
    let r = rig(&Where::live(), Toolset::Merged).await;
    let f = Fixture::read("Bits");
    let stored = load_fixture(&r, &f).await;
    let got = on(&r, "read", "IadSm172.Bits").await;
    let problems = drop_fixture(&r, &f).await;
    assert!(problems.is_empty(), "{problems:?}");
    assert!(
        stored.contains("<StreamLocation>^IadSm172BitsS</StreamLocation>"),
        "IRIS still holds the element: {stored}"
    );
    assert_eq!(got["success"], true, "{got}");
    // What the agent reads no longer describes a stream location the class does not use, and
    // says it left it out.
    assert!(
        got["storage"]["keywords"].get("StreamLocation").is_none(),
        "{got}"
    );
    assert_eq!(got["compiler_added"], json!(["StreamLocation"]), "{got}");
    assert!(
        !got["storage"].to_string().contains("IadSm172BitsS"),
        "{got}"
    );
}

#[tokio::test]
#[ignore]
async fn test_storage_map_feedback_173_read_keeps_the_element_of_a_class_that_may_use_streams() {
    let r = rig(&Where::live(), Toolset::Merged).await;
    for c in ["IadSm173.FbBase", "IadSm173.FbPlain", "IadSm173.FbStream"] {
        delete_class(&r, c).await;
    }
    kill(&r, "^IadSm173FbPlain").await;
    kill(&r, "^IadSm173FbStream").await;
    put_class(
        &r,
        "IadSm173.FbBase",
        "Class IadSm173.FbBase Extends %RegisteredObject\n{\n\nParameter FbBase = 1;\n\n}\n",
    )
    .await;
    put_class(
        &r,
        "IadSm173.FbPlain",
        &member_class("IadSm173.FbPlain", "^IadSm173FbPlain", "%Persistent"),
    )
    .await;
    put_class(
        &r,
        "IadSm173.FbStream",
        &member_class(
            "IadSm173.FbStream",
            "^IadSm173FbStream",
            "(%Persistent, IadSm173.FbBase)",
        ),
    )
    .await;
    let plain = on(&r, "read", "IadSm173.FbPlain").await;
    let stream = on(&r, "read", "IadSm173.FbStream").await;
    for c in ["IadSm173.FbStream", "IadSm173.FbPlain", "IadSm173.FbBase"] {
        delete_class(&r, c).await;
    }
    kill(&r, "^IadSm173FbPlain").await;
    kill(&r, "^IadSm173FbStream").await;
    assert_eq!(
        plain["compiler_added"],
        json!(["StreamLocation"]),
        "{plain}"
    );
    assert!(plain["storage"]["keywords"].get("StreamLocation").is_none());
    // A class that extends something that may carry a stream property keeps the element.
    assert_eq!(stream["success"], true, "{stream}");
    assert!(stream.get("compiler_added").is_none(), "{stream}");
    assert_eq!(
        stream["storage"]["keywords"]["StreamLocation"], "^IadSm173FbStreamS",
        "{stream}"
    );
}

// ── T074 ─────────────────────────────────────────────────────────────────────

#[tokio::test]
#[ignore]
async fn test_storage_map_feedback_173_read_and_diagram_warn_about_each_computed_level() {
    let r = rig(&Where::live(), Toolset::Merged).await;
    let bits = Fixture::read("Bits");
    let member = Fixture::read("Member");
    load_fixture(&r, &bits).await;
    load_fixture(&r, &member).await;
    let read = on(&r, "read", "IadSm172.Bits").await;
    let diagram = on(&r, "diagram", "IadSm172.Bits").await;
    let plain_read = on(&r, "read", "IadSm172.Member").await;
    let plain_diagram = on(&r, "diagram", "IadSm172.Member").await;
    for f in [&bits, &member] {
        let problems = drop_fixture(&r, f).await;
        assert!(problems.is_empty(), "{problems:?}");
    }
    // Two lines: level 2's access variable and level 3's computed keys.
    let lines: Vec<String> = read["warnings"]
        .as_array()
        .unwrap_or_else(|| panic!("{read}"))
        .iter()
        .filter_map(|w| w.as_str().map(str::to_string))
        .collect();
    assert_eq!(lines.len(), 2, "{read}");
    let level3 = lines
        .iter()
        .find(|l| l.contains("level 3"))
        .expect("level 3");
    assert!(
        level3.contains("map Chunks")
            && level3.contains("AccessType Other")
            && level3.contains("NextCode")
            && level3.contains("iad does not evaluate")
            && level3.contains("`compare`"),
        "{level3}"
    );
    assert!(
        lines
            .iter()
            .any(|l| l.contains("access variable 2D2") && l.contains("level 2")),
        "{lines:?}"
    );
    // `diagram` carries the same lines, in its JSON and at the top of its Markdown.
    assert_eq!(diagram["warnings"], read["warnings"], "{diagram}");
    let md = diagram["markdown"].as_str().unwrap_or_default();
    let first_two: Vec<&str> = md.lines().take(2).collect();
    assert!(
        first_two.iter().all(|l| l.starts_with("**Warning:** ")),
        "{md}"
    );
    // A plain map: `read`'s own list stays empty, and `diagram` has no `warnings` key and no line.
    assert_eq!(plain_read["warnings"], json!([]), "{plain_read}");
    assert!(plain_diagram.get("warnings").is_none(), "{plain_diagram}");
    assert!(
        !plain_diagram["markdown"]
            .as_str()
            .unwrap_or_default()
            .contains("**Warning:**"),
        "{plain_diagram}"
    );
}

// ── T075 ─────────────────────────────────────────────────────────────────────

const NO_DATA_BITS: &str =
    "looked at ^IadSm172Bits in USER: it holds no data, so nothing here is checked against data";

#[tokio::test]
#[ignore]
async fn test_storage_map_feedback_173_compare_over_an_empty_global_says_it_found_no_data() {
    let r = rig(&Where::live(), Toolset::Merged).await;
    let f = Fixture::read("Bits");
    load_fixture(&r, &f).await;
    let with_data = on(&r, "compare", "IadSm172.Bits").await;
    kill(&r, "^IadSm172Bits").await;
    let got = on(&r, "compare", "IadSm172.Bits").await;
    let problems = drop_fixture(&r, &f).await;
    assert!(problems.is_empty(), "{problems:?}");
    assert_eq!(got["success"], true, "{got}");
    assert_eq!(got["no_data"], true, "{got}");
    assert_eq!(got["sampled_nodes"], 0, "{got}");
    assert_eq!(got["message"], NO_DATA_BITS, "{got}");
    assert_eq!(got["verified"], false, "{got}");
    // The same call over the seeded global says none of it.
    assert_eq!(with_data["verified"], true, "{with_data}");
    assert!(with_data.get("no_data").is_none(), "{with_data}");
}

#[tokio::test]
#[ignore]
async fn test_storage_map_feedback_173_preview_over_an_empty_global_says_it_found_no_data() {
    let r = rig(&Where::live(), Toolset::Merged).await;
    let f = Fixture::read("Member");
    load_fixture(&r, &f).await;
    let with_data = on(&r, "preview", "IadSm172.Member").await;
    kill(&r, "^IadSm172Member").await;
    let got = on(&r, "preview", "IadSm172.Member").await;
    let problems = drop_fixture(&r, &f).await;
    assert!(problems.is_empty(), "{problems:?}");
    assert_eq!(with_data["row_count"], 3, "{with_data}");
    assert!(with_data.get("no_data").is_none(), "{with_data}");
    assert_eq!(got["success"], true, "{got}");
    assert_eq!(got["row_count"], 0, "{got}");
    assert_eq!(got["no_data"], true, "{got}");
    assert_eq!(got["sampled_nodes"], 0, "{got}");
    assert_eq!(
        got["message"],
        "looked at ^IadSm172Member in USER: it holds no data, so nothing here is checked against data",
        "{got}"
    );
}

/// What `propose` answered over an empty global before T075 (measured 2026-11-26 on IRIS 2026.2,
/// `^IadSm173NoData` killed): `success: true`, `shape.nodes_scanned: 0`, and a `draft` of one
/// `{ID}` subscript with no data fields, with seven placeholder lines in `not_inferred`. A map
/// drafted from nothing, with no word that the global had been read and was empty.
#[tokio::test]
#[ignore]
async fn test_storage_map_feedback_173_propose_over_an_empty_global_drafts_nothing_and_says_why() {
    let r = rig(&Where::live(), Toolset::Merged).await;
    kill(&r, "^IadSm173NoData").await;
    let got = call(
        &r,
        json!({"action": "propose", "global": "^IadSm173NoData", "namespace": "USER"}),
    )
    .await;
    assert_eq!(got["success"], true, "{got}");
    assert_eq!(got["no_data"], true, "{got}");
    assert_eq!(got["sampled_nodes"], 0, "{got}");
    assert_eq!(
        got["message"],
        "looked at ^IadSm173NoData in USER: it holds no data, so nothing here is checked against data",
        "{got}"
    );
    assert!(got["draft"].is_null(), "no map from nothing: {got}");
    assert_eq!(got["guesses"], json!([]), "{got}");
    let not = got["not_inferred"].to_string();
    assert!(
        not.contains("a map") && not.contains("holds no data"),
        "{got}"
    );
    // Nothing was linked or typed from an empty master.
    for key in [
        "indexes",
        "index_candidates",
        "class_draft",
        "chunk_map_draft",
    ] {
        assert!(got.get(key).is_none(), "{key}: {got}");
    }
}

#[tokio::test]
#[ignore]
async fn test_storage_map_feedback_173_propose_over_data_has_no_no_data_key() {
    let r = rig(&Where::live(), Toolset::Merged).await;
    let f = Fixture::read("Member");
    load_fixture(&r, &f).await;
    let got = call(
        &r,
        json!({"action": "propose", "global": "^IadSm172Member", "namespace": "USER"}),
    )
    .await;
    let problems = drop_fixture(&r, &f).await;
    assert!(problems.is_empty(), "{problems:?}");
    assert_eq!(got["success"], true, "{got}");
    assert!(got.get("no_data").is_none(), "{got}");
    assert_eq!(
        got["draft"]["data"].as_object().map(|d| d.len()),
        Some(3),
        "{got}"
    );
}

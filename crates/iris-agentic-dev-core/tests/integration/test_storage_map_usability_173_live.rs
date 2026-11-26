//! Spec 173 US7 (T055 to T057): `check` says whether the planner would use an index map. The known
//! answers are the planner's own words, pinned by `fact_6_planner_words` in
//! `test_storage_map_measured_173_live.rs` (a default-collated `{Name}` is "no index subscript
//! collation", `$$SQLUPPER({Name})` is read "using the given", `$$SQLUPPER({Name},4)` names
//! `%SQLUPPER(4)`), never the output under test.
//!
//! Run: `IRIS_HOST=localhost IRIS_WEB_PORT=52780 nice -n 15 cargo test --features testing -j 2 \
//!   --test integration test_storage_map_usability_173 -- --test-threads=1 --include-ignored`

use crate::live_rig_171::{rig, Rig, Where};
use crate::storage173_rig::*;
use iris_agentic_dev_core::tools::Toolset;
use serde_json::{json, Value};

const COLS: [&str; 5] = ["Name", "City", "Age", "Born", "Status"];

async fn live() -> Rig {
    let r = rig(&Where::live(), Toolset::Merged).await;
    drop_all(&r).await;
    r
}

/// A class `IadSm173.<tag>` with 20 rows and `maps` (index maps filed over `^IadSm173<tag>X`, one
/// global per map name) in its Storage block.
async fn class_with(r: &Rig, tag: &str, maps: &[(&str, &[&str])]) -> String {
    let class = format!("IadSm173.{tag}");
    let xml: String = maps
        .iter()
        .map(|(name, subs)| {
            let mut s: Vec<&str> = subs.to_vec();
            s.push("{ID}");
            index_map(name, &format!("IadSm173{tag}{name}"), &s, "")
        })
        .collect();
    put_class(
        r,
        &class,
        &person_class(&class, &format!("IadSm173{tag}"), "", &xml),
    )
    .await;
    insert_rows(r, &class, &COLS, &people(20)).await;
    class
}

async fn check(r: &Rig, class: &str) -> Value {
    let got = ask_check(r, json!({"class": class})).await;
    assert_eq!(got["success"], true, "check refused: {got}");
    got
}

fn lines(got: &Value) -> Vec<Value> {
    got["usability"]
        .as_array()
        .unwrap_or_else(|| panic!("no usability in {got}"))
        .clone()
}

fn line<'a>(all: &'a [Value], map: &str, level: u64) -> &'a Value {
    all.iter()
        .find(|l| l["map"] == json!(map) && l["subscript_level"] == json!(level))
        .unwrap_or_else(|| panic!("no line for {map} level {level} in {all:?}"))
}

/// T055: the planner's own wording, one case per measured fact.
#[tokio::test]
#[ignore = "live IRIS"]
async fn usability_follows_the_planners_words() {
    let r = live().await;
    let raw = class_with(&r, "U1", &[("M", &["{Name}"])]).await;
    let upper = class_with(&r, "U2", &[("M", &["$$SQLUPPER({Name})"])]).await;
    let len = class_with(&r, "U3", &[("M", &["$$SQLUPPER({Name},4)"])]).await;

    let got = check(&r, &raw).await;
    let all = lines(&got);
    assert_eq!(all.len(), 1, "{all:?}");
    assert_eq!(all[0]["status"], "unused", "{got}");
    assert_eq!(all[0]["map"], "M", "{got}");
    assert_eq!(all[0]["field"], "Name", "{got}");
    assert_eq!(all[0]["subscript_level"], 1, "{got}");
    let text = all[0]["text"].as_str().unwrap();
    assert!(text.contains("no index subscript collation"), "{text}");
    assert!(
        text.contains("IadSm173.U1.M"),
        "the real class, not the scratch copy: {text}"
    );
    assert!(!text.contains("IrisDevTmp"), "{text}");
    // The line is also a warning, in the planner's words.
    assert!(
        got["warnings"]
            .as_array()
            .unwrap()
            .iter()
            .any(|w| w.as_str().unwrap().contains("no index subscript collation")),
        "{got}"
    );

    let got = check(&r, &upper).await;
    let all = lines(&got);
    assert_eq!(all.len(), 1, "{all:?}");
    assert_eq!(all[0]["status"], "used", "{got}");
    assert_eq!(all[0]["map"], "M", "{got}");
    assert!(
        all[0]["text"]
            .as_str()
            .unwrap()
            .contains("Read index map IadSm173.U2.M, using the given %SQLUPPER(Name)"),
        "{got}"
    );

    let got = check(&r, &len).await;
    let all = lines(&got);
    assert_eq!(all[0]["status"], "unused", "{got}");
    assert!(
        all[0]["text"].as_str().unwrap().contains("%SQLUPPER(4)"),
        "{got}"
    );
    drop_all(&r).await;
}

/// T055: a two-subscript map has one line for its first field and one for the first two fields.
#[tokio::test]
#[ignore = "live IRIS"]
async fn a_two_subscript_map_has_a_line_per_field_prefix() {
    let r = live().await;
    let class = class_with(&r, "U4", &[]).await;
    let model = json!({
        "name": "M", "type": "index", "global": "^IadSm173U4X",
        "subscripts": [
            {"level": 1, "expression": "$$SQLUPPER({City})"},
            {"level": 2, "expression": "{Age}"},
            {"level": 3, "expression": "{ID}"}
        ]
    });
    // A map the class does not have yet: `check` judges the proposal.
    let got = ask_check(&r, json!({"class": class, "map": "M", "model": model})).await;
    assert_eq!(got["success"], true, "{got}");
    let all = lines(&got);
    assert_eq!(
        all.len(),
        2,
        "one line per field subscript, none for {{ID}}: {all:?}"
    );
    let first = line(&all, "M", 1);
    let second = line(&all, "M", 2);
    assert_eq!(first["field"], "City", "{all:?}");
    assert_eq!(second["field"], "Age", "{all:?}");
    assert_eq!(first["status"], "used", "{all:?}");
    // The second line asks about City and Age together, so the planner reads the map with both.
    assert_eq!(second["status"], "used", "{all:?}");
    let t = second["text"].as_str().unwrap();
    assert!(t.contains("using the given") && t.contains("Age"), "{t}");
    drop_all(&r).await;
}

/// T055: a question iad cannot put to the planner is `unknown` and says why, and `check` still
/// succeeds; nothing is made up.
#[tokio::test]
#[ignore = "live IRIS"]
async fn an_unanswerable_question_is_unknown_and_check_succeeds() {
    let r = live().await;
    let class = class_with(&r, "U5", &[("M", &["{Age}+1"])]).await;
    let got = check(&r, &class).await;
    let all = lines(&got);
    assert_eq!(all.len(), 1, "{all:?}");
    assert_eq!(all[0]["status"], "unknown", "{got}");
    assert_eq!(all[0]["subscript_level"], 1, "{got}");
    assert!(
        all[0]["text"].as_str().unwrap().contains("not a field"),
        "the reason is given: {got}"
    );
    assert_eq!(got["compiled"], true, "{got}");
    drop_all(&r).await;
}

/// T055: a class with no index map gets an empty `usability` and the same answer as before; the
/// scratch copy is gone.
#[tokio::test]
#[ignore = "live IRIS"]
async fn a_class_with_no_index_map_has_no_usability_line() {
    let r = live().await;
    let class = class_with(&r, "U6", &[]).await;
    let got = check(&r, &class).await;
    assert_eq!(lines(&got).len(), 0, "{got}");
    assert_eq!(got["warnings"], json!([]), "{got}");
    let scratch = got["scratch_class"].as_str().unwrap();
    let gone = r
        .call(
            "iris_doc",
            json!({"mode": "get", "name": format!("{scratch}.cls"), "namespace": "USER"}),
        )
        .await;
    assert_ne!(gone["success"], true, "scratch class left behind: {gone}");
    drop_all(&r).await;
}

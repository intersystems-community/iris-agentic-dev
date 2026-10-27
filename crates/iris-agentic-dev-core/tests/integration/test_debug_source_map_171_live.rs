//! R15, against `iris-dev-iris`: `iris_debug` `source_map` (and the older `debug_source_map` of the
//! baseline toolset) maps each method of a compiled class to the `.INT` lines generated from it.
//!
//! The code behind it called `%Studio.Debugger.MapToINT`, which does not exist, and the older tool
//! escaped quotes with a backslash, which ObjectScript does not read, so on an HTTP connection the
//! answer was an error (`Atelier's answer to the generator query is not JSON`). The class below is
//! created, compiled, asked about and dropped by each run; a run that stopped half way is cleaned up by
//! the next one, which drops the class before it creates it.
//!
//! Serial, `#[ignore]`: run with
//! `lockf -k /tmp/iad-141-iris.lock cargo test --features testing --test integration debug_source_map_171 -- --ignored --test-threads=1`.

use super::live_rig_171::{rig, Rig, Where};
use iris_agentic_dev_core::testing::ThrowawayIris;
use iris_agentic_dev_core::tools::Toolset;
use serde_json::{json, Value};

const CLASS: &str = "Smoke171.Map";
const CLASS_TEXT: &str = "Class Smoke171.Map Extends %RegisteredObject
{

ClassMethod Alpha() As %Integer
{
	Set a = 1
	Set b = 2
	Quit a + b
}

ClassMethod Beta(x As %Integer) As %Integer
{
	Quit x * 2
}

}
";

async fn create_class(r: &Rig) {
    let put = r
        .call(
            "iris_doc",
            json!({"mode": "put", "name": format!("{CLASS}.cls"), "content": CLASS_TEXT, "compile": true}),
        )
        .await;
    assert_eq!(
        put["success"], true,
        "the scratch class must compile: {put}"
    );
}

async fn drop_class(r: &Rig) {
    let _ = r
        .call(
            "iris_doc",
            json!({"mode": "delete", "name": format!("{CLASS}.cls")}),
        )
        .await;
}

/// The text of line `n` (1-based) of the class's `.INT` routine.
async fn int_line_text(r: &Rig, int_name: &str, n: i64) -> String {
    let doc = r
        .call(
            "iris_doc",
            json!({"mode": "get", "name": format!("{int_name}.int")}),
        )
        .await;
    let text = doc["content"]
        .as_str()
        .map(str::to_string)
        .or_else(|| {
            doc["content"].as_array().map(|a| {
                a.iter()
                    .filter_map(|l| l.as_str())
                    .collect::<Vec<_>>()
                    .join("\n")
            })
        })
        .unwrap_or_else(|| panic!("no .INT text: {doc}"));
    // Atelier opens a routine with a `ROUTINE <name> [Type=INT,...]` header that is not a line of it.
    text.lines()
        .filter(|l| !l.starts_with("ROUTINE "))
        .nth((n - 1) as usize)
        .unwrap_or_else(|| panic!("the routine has no line {n}"))
        .to_string()
}

fn check_map(b: &Value) -> (String, Vec<i64>, Vec<i64>) {
    assert_eq!(b["success"], true, "{b}");
    let map = &b["source_map"];
    let alpha = &map["Alpha"];
    let beta = &map["Beta"];
    let ints = |m: &Value| -> Vec<i64> {
        m["lines"]
            .as_array()
            .unwrap_or_else(|| panic!("no lines: {m}"))
            .iter()
            .map(|v| v.as_i64().unwrap())
            .collect()
    };
    let int_name = alpha["int_name"].as_str().expect("int_name").to_string();
    assert_eq!(int_name, "Smoke171.Map.1", "{b}");
    assert_eq!(beta["int_name"], "Smoke171.Map.1", "{b}");
    let (a, bl) = (ints(alpha), ints(beta));
    // Alpha has three code lines and Beta one; nothing else of the class is a method of its own.
    assert_eq!(a.len(), 3, "Alpha's three lines: {b}");
    assert_eq!(bl.len(), 1, "Beta's one line: {b}");
    assert_eq!(alpha["int_line"], a[0], "{b}");
    assert_eq!(a[1], a[0] + 1, "{b}");
    assert_eq!(a[2], a[0] + 2, "{b}");
    assert!(bl[0] > a[2], "Beta follows Alpha in the routine: {b}");
    assert!(
        map.as_object().unwrap().len() == 2,
        "only the class's own methods are mapped, not %RegisteredObject's: {b}"
    );
    (int_name, a, bl)
}

async fn scenarios(at: &Where) -> usize {
    let r = rig(at, Toolset::Merged).await;
    drop_class(&r).await;
    create_class(&r).await;

    // iris_debug, the tool of the default toolset.
    let b = r
        .call(
            "iris_debug",
            json!({"action": "source_map", "class_name": CLASS}),
        )
        .await;
    {
        let (int_name, a, bl) = check_map(&b);
        // The line the map names holds the source line it was asked about.
        assert!(
            int_line_text(&r, &int_name, a[0]).await.contains("a = 1"),
            "INT line {} should hold `Set a = 1`: {b}",
            a[0]
        );
        assert!(
            int_line_text(&r, &int_name, a[2])
                .await
                .contains("Quit a + b"),
            "INT line {} should hold `Quit a + b`: {b}",
            a[2]
        );
        assert!(
            int_line_text(&r, &int_name, bl[0]).await.contains("x * 2"),
            "INT line {} should hold `Quit x * 2`: {b}",
            bl[0]
        );
        // A trailing .cls is accepted, as the older tool accepted it.
        let with_ext = r
            .call(
                "iris_debug",
                json!({"action": "source_map", "class_name": format!("{CLASS}.cls")}),
            )
            .await;
        check_map(&with_ext);
    }
    drop_class(&r).await;

    // A class that is not on the server is a named error, not an empty map and not a crash.
    let missing = r
        .call(
            "iris_debug",
            json!({"action": "source_map", "class_name": "Smoke171.NoSuchClass"}),
        )
        .await;
    assert_eq!(missing["success"], false, "{missing}");
    assert_eq!(missing["error_code"], "NOT_FOUND", "{missing}");

    // The baseline toolset keeps the older tool; it gives the same answer.
    let rb = rig(at, Toolset::Baseline).await;
    create_class(&rb).await;
    let old = rb
        .call("debug_source_map", json!({"cls_name": CLASS}))
        .await;
    drop_class(&rb).await;
    check_map(&old);
    3
}

#[tokio::test]
#[ignore = "requires live IRIS (iris-dev-iris)"]
async fn source_map_answers_on_iris_dev_iris() {
    assert_eq!(scenarios(&Where::live()).await, 3);
}

/// The same scenarios on the throwaway container: 2026.2, and the version `IAD_THROWAWAY_IMAGE`
/// names (the release gate runs it with `intersystemsdc/iris-community:2025.3`).
#[tokio::test]
#[ignore = "requires Docker (starts the throwaway IRIS)"]
async fn source_map_answers_on_the_throwaway() {
    let Some(t) = ThrowawayIris::start() else {
        return;
    };
    assert_eq!(scenarios(&Where::throwaway(&t)).await, 3);
}

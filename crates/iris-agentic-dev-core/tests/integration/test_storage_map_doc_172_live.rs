//! T079 (FR-031): an `iris_doc` put of a class with an SQL map goes through the storage-map
//! safeguards at `do_write`, the single write sink, and keeps `iris_doc`'s answer shape.
//!
//! Run: `IRIS_HOST=localhost IRIS_WEB_PORT=52780 cargo test --features testing --test integration \
//!   test_storage_map_doc_172 -- --test-threads=1 --include-ignored`

use crate::live_rig_171::{rig, Rig, Where};
use crate::storage172_rig::{ask, drop_fixture, load_fixture, Fixture};
use iris_agentic_dev_core::tools::Toolset;
use serde_json::{json, Value};

const CLASS: &str = "IadSm172.Member";

async fn stored(r: &Rig) -> String {
    let got = r
        .call(
            "iris_doc",
            json!({"mode": "get", "name": format!("{CLASS}.cls"), "namespace": "USER"}),
        )
        .await;
    match &got["content"] {
        Value::String(s) => s.clone(),
        Value::Array(a) => a
            .iter()
            .filter_map(Value::as_str)
            .collect::<Vec<_>>()
            .join("\n"),
        _ => panic!("{got}"),
    }
}

async fn put(r: &Rig, text: &str) -> Value {
    r.call(
        "iris_doc",
        json!({"mode": "put", "name": format!("{CLASS}.cls"), "content": text,
               "compile": true, "namespace": "USER"}),
    )
    .await
}

fn warned(v: &Value) -> bool {
    v.to_string()
        .contains("iris_storage_map edits maps with a version check")
}

#[tokio::test]
#[ignore = "live IRIS"]
async fn test_storage_map_doc_172_a_broken_map_is_refused_and_the_class_keeps_working() {
    let r = rig(&Where::live(), Toolset::Merged).await;
    let f = Fixture::read("Member");
    load_fixture(&r, &f).await;
    let before = stored(&r).await;
    let got = put(
        &r,
        &f.cls_text
            .replacen("<Type>data</Type>", "<Type>DATA</Type>", 1),
    )
    .await;
    let after = stored(&r).await;
    let rows = ask(&r, &f).await;
    let problems = drop_fixture(&r, &f).await;

    assert_eq!(got["success"], false, "{got}");
    assert_eq!(got["error_code"], "STORAGE_MAP_INVALID", "{got}");
    assert_eq!(after, before, "the refused put changed the class");
    let expected = f.seed["answer"].as_array().cloned().unwrap_or_default();
    assert_eq!(rows, expected);
    assert!(problems.is_empty(), "cleanup: {problems:?}");
}

#[tokio::test]
#[ignore = "live IRIS"]
async fn test_storage_map_doc_172_a_map_change_is_written_with_the_warning() {
    let r = rig(&Where::live(), Toolset::Merged).await;
    let f = Fixture::read("Member");
    load_fixture(&r, &f).await;
    // Name and City swap pieces: a real map change that compiles.
    let swapped = f
        .cls_text
        .replacen("<Piece>2</Piece>", "<Piece>X</Piece>", 1)
        .replacen("<Piece>1</Piece>", "<Piece>2</Piece>", 1)
        .replacen("<Piece>X</Piece>", "<Piece>1</Piece>", 1);
    let changed = put(&r, &swapped).await;
    // A put that changes no map: the warning is not added.
    let with_param = stored(&r).await.replacen(
        "Property Age As %Integer;",
        "Property Age As %Integer;\n\nParameter IADDOC = 1;",
        1,
    );
    let unchanged = put(&r, &with_param).await;
    let problems = drop_fixture(&r, &f).await;

    assert_eq!(changed["success"], true, "{changed}");
    assert_eq!(changed["compiled"], true, "{changed}");
    assert!(warned(&changed), "no warning on a map change: {changed}");
    // FR-031, FR-030c: the put is recorded as a map write.
    let record = &changed["storage_map_write"];
    assert_eq!(record["class"], f.class, "{changed}");
    assert_eq!(
        record["changed_paths"],
        serde_json::json!(["/maps/Master"]),
        "{changed}"
    );
    assert_eq!(record["code_changed"], false, "{changed}");
    assert_eq!(record["compiled"], true, "{changed}");
    assert_ne!(
        record["version_before"], record["version_after"],
        "{changed}"
    );
    assert_eq!(unchanged["success"], true, "{unchanged}");
    assert!(
        !warned(&unchanged),
        "warning without a map change: {unchanged}"
    );
    assert!(problems.is_empty(), "cleanup: {problems:?}");
}

#[tokio::test]
#[ignore = "live IRIS"]
async fn test_storage_map_doc_172_a_map_over_routine_is_refused() {
    let r = rig(&Where::live(), Toolset::Merged).await;
    let f = Fixture::read("Member");
    load_fixture(&r, &f).await;
    let before = stored(&r).await;
    let got = put(
        &r,
        &f.cls_text.replacen(
            "<Global>^IadSm172Member</Global>",
            "<Global>^ROUTINE</Global>",
            1,
        ),
    )
    .await;
    let after = stored(&r).await;
    let problems = drop_fixture(&r, &f).await;

    assert_eq!(got["success"], false, "{got}");
    assert_eq!(got["refusal"]["kind"], "fixed_rule", "{got}");
    assert_eq!(after, before, "a map over ^ROUTINE was written");
    assert!(problems.is_empty(), "cleanup: {problems:?}");
}

/// A put the scratch copy passes but the real compile refuses: the previous text is put back.
/// The class has a method generator that refuses piece 9 (code only the real compile runs).
#[tokio::test]
#[ignore = "live IRIS"]
async fn test_storage_map_doc_172_a_failed_compile_restores_the_previous_text() {
    use crate::live_rig_171::rig_with;
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
    let first = put(&r, &guarded).await;
    assert_eq!(first["success"], true, "{first}");
    let before = stored(&r).await;
    let got = put(
        &r,
        &guarded.replacen("<Piece>3</Piece>", "<Piece>9</Piece>", 1),
    )
    .await;
    let after = stored(&r).await;
    let rows = ask(&r, &f).await;
    let problems = drop_fixture(&r, &f).await;

    assert_eq!(got["success"], false, "{got}");
    assert_eq!(got["compiled"], false, "{got}");
    assert_eq!(got["storage_map"]["restored"], true, "{got}");
    // FR-031 with FR-017: IRIS's errors come with the explanations a map edit gives.
    // The generator's error is #5001, which the explanation table does not cover: no
    // explanation, and IRIS's own text is there.
    assert_eq!(got["explanations"], serde_json::json!([]), "{got}");
    assert!(
        got["compile_errors"]
            .to_string()
            .contains("piece 9 is refused"),
        "{got}"
    );
    assert_eq!(got["storage_map_write"]["compiled"], false, "{got}");
    assert_eq!(got["storage_map_write"]["restored"], true, "{got}");
    assert_eq!(
        after.trim_end(),
        before.trim_end(),
        "the previous text was not put back"
    );
    let expected = f.seed["answer"].as_array().cloned().unwrap_or_default();
    assert_eq!(rows, expected);
    assert!(problems.is_empty(), "cleanup: {problems:?}");
}

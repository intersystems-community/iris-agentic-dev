//! T095 (US10): the JSON a GUI builds on is pinned. For every fixture, `read`'s storage model and
//! `diagram`'s Mermaid, graph and Markdown are built from the class text (no IRIS) and compared
//! with `tests/fixtures/storage172/golden/<Name>.<part>.json`. A change to them is a contract
//! change: bump `MODEL_VERSION`, write the new number in `golden/VERSION`, and regenerate with
//! `IAD_REGEN_GOLDEN=1`, then say so in the release notes.

use iris_agentic_dev_core::tools::storage_map::diagram::{graph, markdown, mermaid};
use iris_agentic_dev_core::tools::storage_map::model::{derived_facts, from_tree, MODEL_VERSION};
use iris_agentic_dev_core::tools::storage_map::tree::{parse_body, split_class};
use serde_json::{json, Value};
use std::path::PathBuf;

fn dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/storage172")
}

fn regen() -> bool {
    std::env::var("IAD_REGEN_GOLDEN").is_ok_and(|v| v == "1")
}

/// The pinned parts of one fixture: `read` (storage and derived facts) and `diagram`.
fn parts(name: &str, text: &str) -> Vec<(&'static str, Value)> {
    let span = split_class(text).unwrap().remove(0);
    let nodes = parse_body(&text[span.body_start..span.body_end]).unwrap();
    let class = format!("IadSm172.{name}");
    let mut m = from_tree(&class, &span.name, &nodes);
    m.derived = derived_facts(&m);
    vec![
        ("read", json!({"storage": m.storage, "derived": m.derived})),
        (
            "diagram",
            json!({"mermaid": mermaid(&m, None), "graph": graph(&m, None), "markdown": markdown(&m, None)}),
        ),
    ]
}

#[test]
fn test_storage_map_golden_172_every_fixture_matches_its_golden_files() {
    let golden = dir().join("golden");
    let mut names: Vec<String> = std::fs::read_dir(dir().join("classes"))
        .unwrap()
        .filter_map(|e| Some(e.ok()?.path().file_stem()?.to_str()?.to_string()))
        .collect();
    names.sort();
    let mut failures = Vec::new();
    for name in &names {
        let text =
            std::fs::read_to_string(dir().join("classes").join(format!("{name}.cls"))).unwrap();
        for (part, value) in parts(name, &text) {
            let path = golden.join(format!("{name}.{part}.json"));
            let fresh = serde_json::to_string_pretty(&value).unwrap() + "\n";
            if regen() {
                std::fs::create_dir_all(&golden).unwrap();
                std::fs::write(&path, &fresh).unwrap();
                continue;
            }
            match std::fs::read_to_string(&path) {
                Ok(stored) if stored == fresh => {}
                Ok(_) => failures.push(format!("{}: differs", path.display())),
                Err(_) => failures.push(format!("{}: missing", path.display())),
            }
        }
    }
    assert!(
        failures.is_empty(),
        "the storage-map contract changed. If that is meant, bump MODEL_VERSION and golden/VERSION \
         and rerun with IAD_REGEN_GOLDEN=1:\n{}",
        failures.join("\n")
    );
    assert!(names.len() >= 15, "only {} fixtures", names.len());
}

#[test]
fn test_storage_map_golden_172_version_file_matches_model_version() {
    let path = dir().join("golden/VERSION");
    if regen() {
        std::fs::write(&path, format!("{MODEL_VERSION}\n")).unwrap();
    }
    let stored = std::fs::read_to_string(&path).expect("golden/VERSION");
    assert_eq!(
        stored.trim(),
        MODEL_VERSION.to_string(),
        "MODEL_VERSION changed without the golden files: regenerate them with IAD_REGEN_GOLDEN=1"
    );
}

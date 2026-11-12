//! T108/T109 (US11): the public corpus. Every fixture round-trips byte for byte, compiles and
//! answers its known question; and, with an internal corpus of feature vectors at hand, the
//! combinations no public fixture covers are listed.
//!
//! Run: `IRIS_HOST=localhost IRIS_WEB_PORT=52780 cargo test --features testing --test integration \
//!   test_storage_map_corpus_172 -- --test-threads=1 --include-ignored`

use crate::live_rig_171::{rig, Where};
use crate::storage172_rig::check_fixture;
use iris_agentic_dev_core::tools::storage_map::model::from_tree;
use iris_agentic_dev_core::tools::storage_map::tree::{parse_body, split_class};
use iris_agentic_dev_core::tools::Toolset;
use std::collections::BTreeSet;
use std::path::PathBuf;

fn fixture_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/storage172")
}

/// Fixture names: the hand-written ones, and the generated ones when there are any.
fn fixtures() -> Vec<String> {
    let mut out = Vec::new();
    for sub in ["classes", "generated/classes"] {
        if let Ok(rd) = std::fs::read_dir(fixture_dir().join(sub)) {
            for e in rd.flatten() {
                if let Some(stem) = e.path().file_stem().and_then(|s| s.to_str()) {
                    out.push(stem.to_string());
                }
            }
        }
    }
    out.sort();
    out
}

/// T108: every fixture, through the rig's whole check.
#[tokio::test]
#[ignore = "live IRIS"]
async fn test_storage_map_corpus_172_every_fixture_holds() {
    let r = rig(&Where::live(), Toolset::Merged).await;
    let names = fixtures();
    assert!(names.len() >= 15, "{names:?}");
    for name in names {
        check_fixture(&r, &name).await;
    }
}

/// The combination a map falls in, in the vectors' terms: map type, subscript depth (4 and over
/// together), node layout and delimiter class.
fn key(map_type: &str, depth: u64, layout: &str, delim: &str) -> String {
    let d = if depth >= 4 {
        "4+".to_string()
    } else {
        depth.to_string()
    };
    format!("{map_type}|{d}|{layout}|{delim}")
}

fn fixture_keys() -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    let dirs = ["classes", "generated/classes"].map(|d| fixture_dir().join(d));
    for e in dirs
        .iter()
        .filter_map(|d| std::fs::read_dir(d).ok())
        .flatten()
        .flatten()
    {
        let text = std::fs::read_to_string(e.path()).unwrap();
        let Some(span) = split_class(&text).ok().and_then(|s| s.into_iter().next()) else {
            continue;
        };
        let Ok(nodes) = parse_body(&text[span.body_start..span.body_end]) else {
            continue;
        };
        let m = from_tree("X.Y", &span.name, &nodes);
        for map in &m.storage.maps {
            let nodes: BTreeSet<&str> = map
                .data
                .values()
                .filter_map(|d| d.node.as_deref())
                .collect();
            let layout = if nodes.iter().any(|n| n.contains('{')) {
                "subscript-node"
            } else {
                match nodes.len() {
                    0 => "none",
                    1 => "single-node",
                    _ => "multi-node",
                }
            };
            let delims: BTreeSet<&str> = map
                .data
                .values()
                .filter_map(|d| d.delimiter.as_deref())
                .collect();
            let delim = match delims.len() {
                0 if !map.data.is_empty() => "none-$LIST".to_string(),
                0 => "none".to_string(),
                1 => {
                    let d = delims.iter().next().copied().unwrap_or("");
                    if d == "\"^\"" {
                        "^".into()
                    } else if d.to_ascii_lowercase().starts_with("$c(") {
                        "$c(n)".into()
                    } else {
                        "single-printable".into()
                    }
                }
                _ => "multi-delimiter".to_string(),
            };
            out.insert(key(
                &map.r#type,
                map.subscripts.len() as u64,
                layout,
                &delim,
            ));
        }
    }
    out
}

/// T109: local only. `IAD_STORAGE_CORPUS` names a directory of feature vectors (JSON lines, one
/// map each) produced outside the repository; the test lists the combinations seen there that no
/// public fixture has. Unset, it says so and passes only with `IAD_ALLOW_SKIP=1`.
#[test]
#[ignore = "local corpus"]
fn test_storage_map_corpus_172_coverage_against_a_local_corpus() {
    let Ok(dir) = std::env::var("IAD_STORAGE_CORPUS") else {
        assert!(
            std::env::var("IAD_ALLOW_SKIP").is_ok_and(|v| v == "1"),
            "IAD_STORAGE_CORPUS is not set, so the coverage check did not run (IAD_ALLOW_SKIP=1 to \
             accept that)"
        );
        eprintln!("NOT RUN: IAD_STORAGE_CORPUS is not set");
        return;
    };
    let have = fixture_keys();
    let mut seen: std::collections::BTreeMap<String, usize> = Default::default();
    for e in std::fs::read_dir(&dir)
        .expect("IAD_STORAGE_CORPUS")
        .flatten()
    {
        let text = std::fs::read_to_string(e.path()).unwrap_or_default();
        for line in text.lines() {
            let Ok(v) = serde_json::from_str::<serde_json::Value>(line) else {
                continue;
            };
            let delims: Vec<&str> = v["data_delimiters"]
                .as_array()
                .map(|a| a.iter().filter_map(|x| x.as_str()).collect())
                .unwrap_or_default();
            let delim = match delims.as_slice() {
                [] if v["data_fields"].as_u64().unwrap_or(0) > 0 => "none-$LIST".to_string(),
                [] => "none".to_string(),
                [one] => one.to_string(),
                _ => "multi-delimiter".to_string(),
            };
            let k = key(
                v["map_type"].as_str().unwrap_or(""),
                v["subscript_depth"].as_u64().unwrap_or(0),
                v["data_node_layout"].as_str().unwrap_or(""),
                &delim,
            );
            *seen.entry(k).or_default() += 1;
        }
    }
    assert!(!seen.is_empty(), "no feature vectors under {dir}");
    let missing: Vec<&String> = seen.keys().filter(|k| !have.contains(*k)).collect();
    eprintln!(
        "{} combinations in the corpus, {} covered by a public fixture, {} not:",
        seen.len(),
        seen.len() - missing.len(),
        missing.len()
    );
    for k in &missing {
        eprintln!("  {k}");
    }
}

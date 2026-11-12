//! T097: `docs/storage-maps.md` agrees with the code: every error code it names is emitted, every
//! action it lists is one `call_shape` reads, and every measured behaviour names a live test that
//! exists.

use std::path::PathBuf;

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn doc() -> String {
    std::fs::read_to_string(root().join("docs/storage-maps.md")).expect("docs/storage-maps.md")
}

/// Every `.rs` file under `dir`, read.
fn sources(dir: &std::path::Path) -> String {
    let mut out = String::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        for e in std::fs::read_dir(&d).unwrap() {
            let p = e.unwrap().path();
            if p.is_dir() {
                stack.push(p);
            } else if p.extension().is_some_and(|x| x == "rs") {
                out.push_str(&std::fs::read_to_string(&p).unwrap());
            }
        }
    }
    out
}

/// The backticked cells of the table rows in section `heading`, by column.
fn table(doc: &str, heading: &str, column: usize) -> Vec<String> {
    let start = doc.find(heading).unwrap_or_else(|| panic!("no {heading}"));
    let rest = &doc[start + heading.len()..];
    let end = rest.find("\n## ").unwrap_or(rest.len());
    rest[..end]
        .lines()
        .filter(|l| l.starts_with('|') && !l.contains("---"))
        .skip(1)
        .filter_map(|l| l.split('|').nth(column + 1))
        .filter_map(|c| {
            let c = c.trim();
            c.strip_prefix('`')
                .and_then(|r| r.strip_suffix('`'))
                .map(str::to_string)
        })
        .collect()
}

#[test]
fn test_storage_maps_doc_172_every_error_code_is_emitted() {
    let src = sources(&root().join("crates/iris-agentic-dev-core/src"));
    let re = regex::Regex::new(r"STORAGE_MAP_[A-Z_]+").unwrap();
    let doc = doc();
    let codes: std::collections::BTreeSet<&str> = re.find_iter(&doc).map(|m| m.as_str()).collect();
    assert!(codes.len() >= 12, "{codes:?}");
    for code in codes {
        assert!(
            src.contains(&format!("\"{code}\"")),
            "{code} is documented and never emitted"
        );
    }
}

#[test]
fn test_storage_maps_doc_172_every_action_is_one_the_tool_reads() {
    let shape = std::fs::read_to_string(
        root().join("crates/iris-agentic-dev-core/src/tools/storage_map/mod.rs"),
    )
    .unwrap();
    let actions = table(&doc(), "## Actions", 0);
    assert_eq!(actions.len(), 9, "{actions:?}");
    for a in actions {
        assert!(
            shape.contains(&format!("\"{a}\" => Some(Action::")),
            "{a} is documented and call_shape does not read it"
        );
    }
}

#[test]
fn test_storage_maps_doc_172_every_measured_behaviour_names_a_live_test() {
    let live = sources(&root().join("crates/iris-agentic-dev-core/tests/integration"));
    let tests = table(&doc(), "## Measured behaviour", 2);
    assert!(tests.len() >= 8, "{tests:?}");
    for t in tests {
        assert!(
            live.contains(&format!("fn {t}(")),
            "{t} is cited and is not a live test"
        );
    }
}

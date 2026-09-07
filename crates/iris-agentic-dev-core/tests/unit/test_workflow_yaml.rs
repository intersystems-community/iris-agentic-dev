//! Guards against unquoted YAML scalars that contain a colon in `.github/workflows/*.yml`.
//!
//! A workflow file that does not parse is not a test failure — it is a *startup* failure. GitHub
//! reports the run as failed in 0 seconds and none of the checks inside it execute, so every gate
//! that file was supposed to enforce silently stops enforcing. Same shape as the #110 serde
//! silent-drop bug: the thing looks green-adjacent while doing nothing.
//!
//! This shipped twice, both times for the same reason. Aggregating the test targets meant CI steps
//! had to name a target plus a module filter, and a module filter ends in `::`:
//!
//! ```yaml
//! run: cargo test --test unit test_docs_contract::
//! ```
//!
//! YAML reads the `::` as a nested mapping and rejects the document. Quoting the scalar fixes it.
//!
//! Rather than parse YAML — which would mean a new dependency, and the runner has no PyYAML — this
//! checks the one lexical property that broke: an unquoted `key: value` whose value contains a
//! colon that is followed by a space or the end of the line.

use std::path::{Path, PathBuf};

fn workflows_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("crates/<crate> has two ancestors")
        .join(".github/workflows")
}

fn workflow_files(dir: &Path) -> Vec<PathBuf> {
    let mut files: Vec<PathBuf> = std::fs::read_dir(dir)
        .unwrap_or_else(|e| panic!("cannot read {}: {e}", dir.display()))
        .map(|e| e.expect("dir entry").path())
        .filter(|p| p.extension().is_some_and(|x| x == "yml" || x == "yaml"))
        .collect();
    files.sort();
    files
}

/// True when `value` is a plain (unquoted, non-block) scalar that YAML would choke on because it
/// carries a colon that reads as a key separator.
fn is_unparseable_plain_scalar(value: &str) -> bool {
    let value = value.trim();
    if value.is_empty() {
        return false;
    }

    // Quoted scalars and block scalars (`|`, `>`) carry their colons safely. `&anchor`, `*alias`,
    // and flow collections (`{`, `[`) are structure, not prose, and are left to YAML.
    let first = value.as_bytes()[0];
    if matches!(
        first,
        b'"' | b'\'' | b'|' | b'>' | b'&' | b'*' | b'{' | b'[' | b'#'
    ) {
        return false;
    }

    // `${{ ... }}` expressions are the common case of a brace-heavy plain scalar; GitHub's own
    // parser accepts them and they do not contain a bare `: `.
    let bytes = value.as_bytes();
    for (i, &b) in bytes.iter().enumerate() {
        if b != b':' {
            continue;
        }
        match bytes.get(i + 1) {
            // `key: value` — a colon followed by whitespace starts a nested mapping.
            Some(&next) if next == b' ' || next == b'\t' => return true,
            // Trailing colon at end of line does the same thing.
            None => return true,
            _ => {}
        }
    }
    false
}

/// Splits `  - run: cargo test ...` into its key and value, or None when the line is not a
/// `key: value` mapping entry.
fn split_mapping_entry(line: &str) -> Option<(&str, &str)> {
    let trimmed = line.trim_start();
    if trimmed.starts_with('#') {
        return None;
    }
    // A sequence entry can hold a mapping on the same line: `- run: ...`.
    let body = trimmed.strip_prefix("- ").unwrap_or(trimmed);

    let (key, value) = body.split_once(':')?;
    // Keys are plain identifiers here. Anything with a space or quote in it is not a key, it is
    // prose that happens to contain a colon (e.g. a comment continuation).
    if key.is_empty() || key.contains(char::is_whitespace) || key.contains('"') {
        return None;
    }
    Some((key, value))
}

#[test]
fn every_workflow_scalar_with_a_colon_is_quoted() {
    let dir = workflows_dir();
    let files = workflow_files(&dir);
    assert!(
        !files.is_empty(),
        "no workflow files found under {} — this guard would pass vacuously",
        dir.display()
    );

    let mut offenders = Vec::new();
    for file in &files {
        let src = std::fs::read_to_string(file)
            .unwrap_or_else(|e| panic!("cannot read {}: {e}", file.display()));
        for (idx, line) in src.lines().enumerate() {
            let Some((key, value)) = split_mapping_entry(line) else {
                continue;
            };
            if is_unparseable_plain_scalar(value) {
                offenders.push(format!(
                    "{}:{} — `{key}` value is an unquoted scalar containing a colon: {}",
                    file.file_name().expect("named file").to_string_lossy(),
                    idx + 1,
                    value.trim()
                ));
            }
        }
    }

    assert!(
        offenders.is_empty(),
        "these workflow scalars would make GitHub reject the whole file at startup — the run \
         fails in 0s and every check inside it stops running. Wrap each value in double quotes.\n{}",
        offenders.join("\n")
    );
}

/// The detector has to actually fire, or the test above is decoration.
#[test]
fn the_detector_fires_on_the_shapes_that_broke_ci() {
    // Both real regressions: a module filter ending in `::`, and a mid-line `: `.
    assert!(is_unparseable_plain_scalar(
        "cargo test --test unit test_docs_contract::"
    ));
    assert!(is_unparseable_plain_scalar(
        "cargo test --test bin_integration test_mcp_binary_config:: -- --include-ignored"
    ));
    assert!(is_unparseable_plain_scalar("echo foo: bar"));

    // And it must not fire on what is already there and valid.
    assert!(!is_unparseable_plain_scalar(
        "\"cargo test --test unit test_docs_contract::\""
    ));
    assert!(!is_unparseable_plain_scalar("cargo fmt --all -- --check"));
    assert!(!is_unparseable_plain_scalar("${{ matrix.skill }}"));
    assert!(!is_unparseable_plain_scalar("-C link-arg=-fuse-ld=mold"));
    assert!(!is_unparseable_plain_scalar("|"));
    assert!(!is_unparseable_plain_scalar(""));
    // A URL's `//` follows the colon, so it is not a mapping separator.
    assert!(!is_unparseable_plain_scalar(
        "https://github.com/intersystems-community/iris-agentic-dev"
    ));
}

#[test]
fn split_mapping_entry_ignores_comments_and_prose() {
    assert_eq!(
        split_mapping_entry("  run: cargo test"),
        Some(("run", " cargo test"))
    );
    assert_eq!(
        split_mapping_entry("  - run: cargo test"),
        Some(("run", " cargo test"))
    );
    assert_eq!(split_mapping_entry("  # a comment: with a colon"), None);
    assert_eq!(split_mapping_entry("  no colon here"), None);
}

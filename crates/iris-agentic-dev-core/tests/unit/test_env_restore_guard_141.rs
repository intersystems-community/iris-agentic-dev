//! Guards on process-global state in tests (spec 141 T185, T186).
//!
//! A test that calls `set_var` or `remove_var` and does not put the value back leaks into every
//! test that runs after it in the same binary. One such leak (`IRIS_CONTAINER`, in a live test)
//! made a later test pass or fail depending on order. A test that calls `set_current_dir` does the
//! same to every test that reads the working directory, and `test_workspace_root_prefers_new_over_legacy`
//! failed once in four parallel runs because of it.
//!
//! Two rules, read straight off the source of `crates/*/tests/`:
//!
//! 1. The only way to change the environment in a test is `testing::EnvGuard`, which puts every
//!    value back when it is dropped, even on a panic. A raw call is allowed only on a line that
//!    carries `// env-restored: <how>`, naming what restores it.
//! 2. No test changes the working directory. Pass the directory to the code under test instead
//!    (every loader takes one). The one exception form is `// cwd-restored: <how>`.
//!
//! The needles are built from two halves so this file does not match itself, which means the scan
//! has no exempt file.

use std::path::{Path, PathBuf};

/// Lines per marker that are allowed to stay raw, by file name. Empty on purpose: add an entry only
/// with a reason, and prefer `EnvGuard`.
const ALLOWED_RAW_FILES: &[(&str, &str)] = &[];

fn tests_dirs() -> Vec<PathBuf> {
    let crates = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("crates directory")
        .to_path_buf();
    let mut dirs: Vec<PathBuf> = std::fs::read_dir(&crates)
        .expect("read crates/")
        .map(|e| e.unwrap().path().join("tests"))
        .filter(|p| p.is_dir())
        .collect();
    dirs.sort();
    dirs
}

fn rust_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let mut entries: Vec<PathBuf> = std::fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().path())
        .collect();
    entries.sort();
    for p in entries {
        if p.is_dir() {
            rust_files(&p, out);
        } else if p.extension().is_some_and(|e| e == "rs") {
            out.push(p);
        }
    }
}

fn all_test_files() -> Vec<PathBuf> {
    let mut files = Vec::new();
    for d in tests_dirs() {
        rust_files(&d, &mut files);
    }
    files
}

/// Every line of `crates/*/tests/**/*.rs` that holds `needle` in code (not a `//` comment) and no
/// `marker` comment with a reason after it, as `file:line: text`.
fn unmarked(needles: &[String], marker: &str) -> Vec<String> {
    let mut found = Vec::new();
    for file in all_test_files() {
        let name = file.file_name().unwrap().to_string_lossy().to_string();
        if ALLOWED_RAW_FILES.iter().any(|(f, _)| *f == name) {
            continue;
        }
        let text = std::fs::read_to_string(&file).unwrap();
        for (i, line) in text.lines().enumerate() {
            let trimmed = line.trim_start();
            if trimmed.starts_with("//") {
                continue;
            }
            // A needle inside a string literal is text, not a call: an odd number of quotes
            // before it means the match sits in a literal.
            let is_call = needles.iter().any(|n| {
                line.match_indices(n.as_str())
                    .any(|(at, _)| line[..at].matches('"').count() % 2 == 0)
            });
            if !is_call {
                continue;
            }
            let marked = line
                .split_once(marker)
                .is_some_and(|(_, how)| !how.trim().is_empty());
            if !marked {
                found.push(format!("{}:{}: {}", file.display(), i + 1, trimmed));
            }
        }
    }
    found
}

fn env_needles() -> Vec<String> {
    vec![
        format!("{}{}", "set_", "var("),
        format!("{}{}", "remove_", "var("),
    ]
}

#[test]
fn the_scan_sees_the_test_tree() {
    // A scan that walks nothing passes by default: pin that it reads both crates and many files.
    let files = all_test_files();
    assert!(
        files.len() > 100,
        "expected the scan to read every test file, read {}",
        files.len()
    );
    let dirs = tests_dirs();
    assert!(
        dirs.len() >= 2,
        "expected the core and bin tests directories, got {dirs:?}"
    );
}

#[test]
fn no_test_changes_the_environment_without_a_guard() {
    let found = unmarked(&env_needles(), "// env-restored:");
    assert!(
        found.is_empty(),
        "{} raw environment change(s) in tests. Use `testing::EnvGuard` (set/unset, restored on \
         drop), or mark the line `// env-restored: <how>`:\n{}",
        found.len(),
        found.join("\n")
    );
}

#[test]
fn no_test_changes_the_working_directory() {
    let found = unmarked(
        &[format!("{}{}", "set_current_", "dir(")],
        "// cwd-restored:",
    );
    assert!(
        found.is_empty(),
        "{} test(s) change the working directory, which every parallel test reads. Pass the \
         directory to the code under test instead:\n{}",
        found.len(),
        found.join("\n")
    );
}

#[test]
fn the_raw_file_allow_list_stays_short_and_reasoned() {
    assert!(
        ALLOWED_RAW_FILES.len() <= 3,
        "the exemptions are meant to be few: {ALLOWED_RAW_FILES:?}"
    );
    for (file, why) in ALLOWED_RAW_FILES {
        assert!(!why.trim().is_empty(), "{file} is exempt without a reason");
    }
}

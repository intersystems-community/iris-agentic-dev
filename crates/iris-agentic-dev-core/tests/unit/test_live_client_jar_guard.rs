//! Guard: a test's own HTTP client for IRIS keeps a cookie jar.
//!
//! A client with no jar sends every request without `CSPSESSIONID`, and IRIS opens a CSP session
//! for each write that nothing ends. On a Concurrent User key each one holds a license unit until
//! the web app's timeout: five cookieless PUTs took 25 units of a 128-unit Enterprise key, and the
//! Enterprise e2e job hit `<LICENSE LIMIT EXCEEDED>` minutes into its run (2026-11-11). A test gets
//! its client from `testing::live_client()` or `testing::iris_client(user, password)`.
//!
//! The rule, read off the source of every live test directory (`tests/unit` is left out: it talks
//! to local stubs, never to IRIS): no bare `reqwest::Client` constructor, unless its line or the
//! line before says `// no-jar: <why>`.
//!
//! The needles are built from two halves so this file does not match itself.

use std::path::{Path, PathBuf};

fn rust_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return;
    };
    let mut entries: Vec<PathBuf> = rd.map(|e| e.unwrap().path()).collect();
    entries.sort();
    for p in entries {
        if p.is_dir() {
            if p.file_name()
                .is_some_and(|n| n == "unit" || n == "fixtures")
            {
                continue;
            }
            rust_files(&p, out);
        } else if p.extension().is_some_and(|e| e == "rs") {
            out.push(p);
        }
    }
}

fn live_test_files() -> Vec<PathBuf> {
    let crates = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("crates directory")
        .to_path_buf();
    let mut files = Vec::new();
    let mut dirs: Vec<PathBuf> = std::fs::read_dir(&crates)
        .expect("read crates/")
        .map(|e| e.unwrap().path().join("tests"))
        .filter(|p| p.is_dir())
        .collect();
    dirs.sort();
    for d in dirs {
        rust_files(&d, &mut files);
    }
    files
}

fn needles() -> [String; 2] {
    let head = "reqwest::Client::";
    [
        format!("{head}{}", "new()"),
        format!("{head}{}", "builder()"),
    ]
}

/// `file:line` for every bare constructor without a `no-jar` reason.
fn offenders(files: &[PathBuf]) -> Vec<String> {
    let marker = format!("// no-{}:", "jar");
    let needles = needles();
    let mut out = Vec::new();
    for f in files {
        let text = std::fs::read_to_string(f).unwrap();
        let lines: Vec<&str> = text.lines().collect();
        for (i, line) in lines.iter().enumerate() {
            if !needles.iter().any(|n| line.contains(n.as_str())) {
                continue;
            }
            let explained = line.contains(&marker) || (i > 0 && lines[i - 1].contains(&marker));
            if !explained {
                out.push(format!("{}:{}", f.display(), i + 1));
            }
        }
    }
    out
}

#[test]
fn live_tests_build_no_client_without_a_cookie_jar() {
    let files = live_test_files();
    assert!(
        files.len() > 50,
        "found only {} live test files; the scan is looking in the wrong place",
        files.len()
    );
    let bad = offenders(&files);
    assert!(
        bad.is_empty(),
        "a live test builds a reqwest client with no cookie jar, so each write it sends opens a \
         CSP session IRIS never ends (a license unit each on a Concurrent User key). Use \
         `testing::live_client()` or `testing::iris_client(user, password)`, or say why on the \
         line: `// no-jar: <why>`.\n{}",
        bad.join("\n")
    );
}

#[test]
fn the_scan_sees_a_bare_constructor_and_honours_the_marker() {
    let dir = tempfile::tempdir().unwrap();
    let [new, builder] = needles();
    let marker = format!("// no-{}: a read", "jar");
    std::fs::write(dir.path().join("bare.rs"), format!("let c = {new};\n")).unwrap();
    std::fs::write(
        dir.path().join("marked.rs"),
        format!("{marker}\nlet c = {builder}\n    .build();\nlet d = {new}; {marker}\n"),
    )
    .unwrap();
    let mut files = Vec::new();
    rust_files(dir.path(), &mut files);
    let bad = offenders(&files);
    assert_eq!(bad.len(), 1, "{bad:?}");
    assert!(bad[0].ends_with("bare.rs:1"), "{bad:?}");
}

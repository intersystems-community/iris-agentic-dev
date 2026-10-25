//! Spec 141 T207: no test spawns the binary at "no IRIS here" without pinning it there.
//!
//! `IRIS_WEB_PORT=9` (the discard port) is how a test says "nothing listens". It says that only
//! together with `IRIS_HOST`: environment discovery needs both, and with the port alone it is
//! skipped, so the spawn falls through to docker and port discovery and adopts whatever IRIS the
//! developer has running, in the developer's `HOME`, from whatever directory holds a project file.
//! Several tests did exactly that (`env_remove("IRIS_HOST")` next to `IRIS_WEB_PORT=9`).
//!
//! Definition (the scan reads source text, so it is deliberately simple): every place under
//! `crates/*/tests/` that sets `IRIS_WEB_PORT` to `"9"` (an `.env(...)` call or a tuple in a
//! launch list) belongs to the enclosing function, or to the whole file when it sits outside any
//! function. That unit must
//!
//! 1. reach the spawn through `testing::clean_command`, `testing::clean_mcp_command` or a
//!    `start_with` on the `support_mcp_141` session (which itself calls `clean_mcp_command`; the
//!    last test below holds it to that), so `HOME`, the working directory, the keychain and every
//!    behavior variable are the test's; and
//! 2. pin `IRIS_HOST` to a non-empty value.

use regex::Regex;
use std::path::{Path, PathBuf};

const PORT_NINE: &str = r#""IRIS_WEB_PORT"\s*,\s*"9""#;
const HOST_PIN: &str = r#""IRIS_HOST"\s*,\s*"[^"]+""#;
const ISOLATORS: [&str; 3] = ["clean_command(", "clean_mcp_command(", ".start_with("];

/// The problems in one file's text, each as `line: what is missing`.
fn unpinned_spawns(text: &str) -> Vec<String> {
    let port = Regex::new(PORT_NINE).unwrap();
    let host = Regex::new(HOST_PIN).unwrap();
    let fn_start = Regex::new(r"(?m)^\s*(pub(\([a-z]+\))?\s+)?(async\s+)?fn\s").unwrap();
    let starts: Vec<usize> = fn_start.find_iter(text).map(|m| m.start()).collect();
    let mut problems = Vec::new();
    for hit in port.find_iter(text) {
        let begin = starts.iter().copied().rfind(|s| *s <= hit.start());
        let unit = match begin {
            Some(b) => {
                let end = starts.iter().copied().find(|s| *s > hit.start());
                &text[b..end.unwrap_or(text.len())]
            }
            None => text,
        };
        let line = text[..hit.start()].matches('\n').count() + 1;
        if !ISOLATORS.iter().any(|i| unit.contains(i)) {
            problems.push(format!(
                "{line}: IRIS_WEB_PORT=9 without clean_command / clean_mcp_command"
            ));
        }
        if !host.is_match(unit) {
            problems.push(format!(
                "{line}: IRIS_WEB_PORT=9 without a pinned IRIS_HOST"
            ));
        }
    }
    problems
}

fn rust_files(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            rust_files(&path, out);
        } else if path.extension().is_some_and(|e| e == "rs") {
            out.push(path);
        }
    }
}

#[test]
fn the_scan_flags_a_port_without_a_host_and_a_spawn_without_isolation() {
    let bare = "fn a() {\n    let mut c = Command::new(bin);\n    c.env_remove(\"IRIS_HOST\")\n        .env(\"IRIS_WEB_PORT\", \"9\");\n}\n";
    let found = unpinned_spawns(bare);
    assert_eq!(found.len(), 2, "{found:?}");
    assert!(found[0].starts_with("4:"), "{found:?}");

    let clean_but_no_host =
        "fn a() {\n    let mut c = clean_command(bin);\n    c.env(\"IRIS_WEB_PORT\", \"9\");\n}\n";
    let found = unpinned_spawns(clean_but_no_host);
    assert_eq!(found.len(), 1, "{found:?}");
    assert!(found[0].contains("IRIS_HOST"), "{found:?}");

    let host_but_not_clean =
        "fn a() {\n    c.env(\"IRIS_HOST\", \"127.0.0.1\").env(\"IRIS_WEB_PORT\", \"9\");\n}\n";
    let found = unpinned_spawns(host_but_not_clean);
    assert_eq!(found.len(), 1, "{found:?}");
    assert!(found[0].contains("clean_command"), "{found:?}");

    let good = "fn a() {\n    let mut c = clean_mcp_command(bin);\n    c.env(\"IRIS_HOST\", \"127.0.0.1\").env(\"IRIS_WEB_PORT\", \"9\");\n}\n";
    assert!(unpinned_spawns(good).is_empty());
}

#[test]
fn a_pin_in_a_neighbouring_function_does_not_count() {
    let text = "fn a() {\n    clean_command(b).env(\"IRIS_HOST\", \"h\");\n}\nfn b() {\n    Command::new(x).env(\"IRIS_WEB_PORT\", \"9\");\n}\n";
    assert_eq!(
        unpinned_spawns(text).len(),
        2,
        "{:?}",
        unpinned_spawns(text)
    );
}

#[test]
fn a_launch_list_outside_any_function_is_checked_against_the_file() {
    let text = "const LAUNCH: &[(&str, &str)] = &[(\"IRIS_HOST\", \"127.0.0.1\"), (\"IRIS_WEB_PORT\", \"9\")];\nfn t() { env.start_with(LAUNCH); }\n";
    assert!(
        unpinned_spawns(text).is_empty(),
        "{:?}",
        unpinned_spawns(text)
    );
    let no_spawn = text.replace("start_with", "other");
    assert_eq!(
        unpinned_spawns(&no_spawn).len(),
        1,
        "{:?}",
        unpinned_spawns(&no_spawn)
    );
}

#[test]
fn no_test_spawns_at_a_closed_port_without_pinning_the_host() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
    let mut files = Vec::new();
    for krate in std::fs::read_dir(&root).unwrap() {
        let tests = krate.unwrap().path().join("tests");
        if tests.is_dir() {
            rust_files(&tests, &mut files);
        }
    }
    let me = Path::new(file!()).file_name().unwrap().to_owned();
    let port = Regex::new(PORT_NINE).unwrap();
    let mut sites = 0;
    let mut problems = Vec::new();
    for f in files.iter().filter(|f| f.file_name() != Some(&me)) {
        let text = std::fs::read_to_string(f).unwrap();
        sites += port.find_iter(&text).count();
        for p in unpinned_spawns(&text) {
            problems.push(format!("{}:{p}", f.display()));
        }
    }
    assert!(
        sites >= 10,
        "the scan found only {sites} IRIS_WEB_PORT=9 sites in {} files; it is not reading the tree",
        files.len()
    );
    assert!(problems.is_empty(), "{}", problems.join("\n"));
}

#[test]
fn the_shared_session_helper_goes_through_clean_mcp_command() {
    let text = std::fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/binary/support_mcp_141.rs"),
    )
    .unwrap();
    assert!(
        text.contains("clean_mcp_command("),
        "start_with must isolate"
    );
}

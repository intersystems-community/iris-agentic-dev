//! Every `iris-agentic-dev …` line in a doc code fence has to parse under the real clap parser.
//!
//! README documented `iris-agentic-dev tool iris_query '{"sql":…}'` from v1.2.0 to v1.4.1. The JSON
//! goes in `--args`; `ToolCommand` takes exactly one positional, so the documented form has never
//! parsed. Nothing caught it because no test read the docs.
//!
//! Scope is README plus everything under `docs/`, minus `docs/release-notes/`, where notes quote
//! the wrong command on purpose when describing the fix.
//!
//! The check spawns the binary with the extracted argv plus `--help`. clap walks argv left to
//! right, so a bad positional or an unknown flag errors before `--help` short-circuits: a line
//! that parses exits 0 with usage text, a line that does not exits 2 with `error:`. Nothing runs,
//! so no IRIS connection is attempted.
//!
//! Run with:
//!   cargo build && IAD_BINARY=./target/debug/iris-agentic-dev \
//!   cargo test --features testing --test bin_integration doc_cli -- --include-ignored

fn workspace_root() -> std::path::PathBuf {
    let mut path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    path.pop(); // iris-agentic-dev-bin → crates
    path.pop(); // crates → workspace root
    path
}

/// README plus every `.md` under `docs/`, release notes excluded, sorted for a stable report.
fn doc_files() -> Vec<std::path::PathBuf> {
    let root = workspace_root();
    let mut files = vec![root.join("README.md")];
    let mut dirs = vec![root.join("docs")];
    while let Some(dir) = dirs.pop() {
        if dir.ends_with("release-notes") {
            continue;
        }
        let entries = std::fs::read_dir(&dir)
            .unwrap_or_else(|e| panic!("failed to read {}: {e}", dir.display()));
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                dirs.push(path);
            } else if path.extension().is_some_and(|e| e == "md") {
                files.push(path);
            }
        }
    }
    files.sort();
    files
}

/// One documented invocation: where it is written and the argv after `iris-agentic-dev`.
struct Example {
    /// Repo-relative path, so a failure names the file to open.
    file: String,
    /// 1-based line within that file.
    line_no: usize,
    argv: Vec<String>,
}

/// Split a shell line into words, honouring single and double quotes and dropping the quotes.
fn split_words(line: &str) -> Vec<String> {
    let mut words = Vec::new();
    let mut current = String::new();
    let mut started = false;
    let mut quote: Option<char> = None;

    for ch in line.chars() {
        match quote {
            Some(q) if ch == q => quote = None,
            Some(_) => current.push(ch),
            None if ch == '\'' || ch == '"' => {
                quote = Some(ch);
                started = true;
            }
            None if ch.is_whitespace() => {
                if started {
                    words.push(std::mem::take(&mut current));
                    started = false;
                }
            }
            None => {
                current.push(ch);
                started = true;
            }
        }
    }
    if started {
        words.push(current);
    }
    words
}

/// True for a shell redirect (`2>debug.log`, `>out`, `2>&1`), which is the shell's, not clap's.
fn is_redirect(word: &str) -> bool {
    // Output only. `<path>` and `<json>` in the Commands table are placeholders, not input
    // redirects, and they have to reach clap as ordinary values — dropping them turned
    // `benchmark --skill <path>` into a missing-value error that had nothing to do with README.
    let rest = word.trim_start_matches(|c: char| c.is_ascii_digit());
    rest.starts_with('>') || word.starts_with('|')
}

/// Pull every `iris-agentic-dev …` line out of one file's fenced code blocks.
///
/// Prose mentions are skipped: only lines inside a fence whose first word is the binary name
/// count. Leading `VAR=value` assignments, trailing `# comments`, and shell redirects are dropped.
fn doc_examples(file: &str, text: &str) -> Vec<Example> {
    let mut examples = Vec::new();
    let mut in_fence = false;

    for (idx, raw) in text.lines().enumerate() {
        let line = raw.trim();
        if line.starts_with("```") {
            in_fence = !in_fence;
            continue;
        }
        if !in_fence {
            continue;
        }
        let words = split_words(line);
        // Skip a `VAR=value` prefix so `IRIS_HOST=… iris-agentic-dev tool …` still gets checked.
        let start = words
            .iter()
            .position(|w| !w.contains('=') || w.starts_with('-'))
            .unwrap_or(words.len());
        if words.get(start).map(String::as_str) != Some("iris-agentic-dev") {
            continue;
        }
        let argv: Vec<String> = words[start + 1..]
            .iter()
            .take_while(|w| !w.starts_with('#'))
            .filter(|w| !is_redirect(w) && w.as_str() != "\\")
            .cloned()
            .collect();
        examples.push(Example {
            file: file.to_string(),
            line_no: idx + 1,
            argv,
        });
    }
    examples
}

#[test]
#[ignore = "spawns the built binary; needs IAD_BINARY or a prior cargo build"]
fn every_documented_invocation_parses() {
    let bin = iris_agentic_dev_core::testing::iad_binary_path();
    if !bin.exists() {
        eprintln!("skipping: {} not built", bin.display());
        return;
    }

    let root = workspace_root();
    let mut examples = Vec::new();
    for path in doc_files() {
        let text = std::fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("failed to read {}: {e}", path.display()));
        let rel = path
            .strip_prefix(&root)
            .unwrap_or(&path)
            .to_string_lossy()
            .to_string();
        examples.extend(doc_examples(&rel, &text));
    }

    // If the extractor stops matching (fence markers change, the binary is renamed), it would
    // silently pass on zero lines. README and docs/ carry well over two dozen invocations.
    assert!(
        examples.len() >= 25,
        "extracted only {} invocations from README and docs/ — the extractor is broken, not the \
         docs",
        examples.len()
    );

    let mut failures = Vec::new();
    for ex in &examples {
        // `clean_command` strips every behavior-changing variable, so the check means the same
        // thing in the `test` job and the `e2e-tests` job, which sets nine of them at job level.
        let out = iris_agentic_dev_core::testing::clean_command(&bin)
            .args(&ex.argv)
            .arg("--help")
            .output()
            .expect("spawning the binary should succeed");

        if !out.status.success() {
            let stderr = String::from_utf8_lossy(&out.stderr);
            let first = stderr.lines().next().unwrap_or("(no stderr)").to_string();
            failures.push(format!(
                "{}:{} — `iris-agentic-dev {}` → {}",
                ex.file,
                ex.line_no,
                ex.argv.join(" "),
                first
            ));
        }
    }

    assert!(
        failures.is_empty(),
        "{} documented invocation(s) do not parse:\n{}",
        failures.len(),
        failures.join("\n")
    );
}

#[test]
fn extractor_ignores_prose_and_keeps_fenced_lines() {
    let doc = "\
Run `iris-agentic-dev skill install` after upgrading.

```bash
iris-agentic-dev mcp --verbose 2>debug.log
iris-agentic-dev tool iris_query --args '{\"sql\":\"SELECT 1\"}'  # a comment
iris-agentic-dev benchmark --skill <path>        # placeholder, not a redirect
xattr -d com.apple.quarantine /usr/local/bin/iris-agentic-dev
```
";
    let examples = doc_examples("docs/example.md", doc);
    assert_eq!(
        examples.len(),
        3,
        "got {examples:?}",
        examples = examples
            .iter()
            .map(|e| e.argv.join(" "))
            .collect::<Vec<_>>()
    );
    assert_eq!(examples[0].argv, vec!["mcp", "--verbose"]);
    assert_eq!(examples[0].line_no, 4);
    assert_eq!(
        examples[1].argv,
        vec!["tool", "iris_query", "--args", "{\"sql\":\"SELECT 1\"}"]
    );
    assert_eq!(examples[2].argv, vec!["benchmark", "--skill", "<path>"]);
    assert_eq!(examples[2].file, "docs/example.md");
}

/// A line-continuation backslash and an env-var prefix are the shell's, not clap's.
#[test]
fn extractor_strips_env_prefixes_and_continuations() {
    let doc = "\
```bash
IRIS_HOST=localhost IRIS_WEB_PORT=52780 iris-agentic-dev tool check_config --args '{}' \\
```
";
    let examples = doc_examples("docs/example.md", doc);
    assert_eq!(examples.len(), 1);
    assert_eq!(
        examples[0].argv,
        vec!["tool", "check_config", "--args", "{}"]
    );
}

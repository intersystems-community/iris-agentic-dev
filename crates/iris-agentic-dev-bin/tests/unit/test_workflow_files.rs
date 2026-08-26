//! Structural guards on `.github/workflows/*.yml` that run in `cargo test`, i.e. locally,
//! before a push.
//!
//! Why this cannot be a CI step: on 2026-08-24 commit 4dec14f put an inline
//! `python -c "` heredoc inside a `run: |` block with its body at column 0. A line at
//! column 0 terminates the block scalar, so the whole file stopped being valid YAML.
//! GitHub's answer to an unparseable workflow is a 0-second run with no jobs and the
//! message "This run likely failed because of a workflow file issue" — a red X that looks
//! like an ordinary test failure. Every master push for the next two days recorded that,
//! and no test, lint or job ever executed. A guard inside ci.yml cannot catch this,
//! because a broken ci.yml never starts.
//!
//! So the check lives here, in the test suite that runs on a developer machine. It is
//! deliberately structural and dependency-free rather than a full YAML parse: the
//! semantic checks (jobs present, every step has `uses`/`run`) are in
//! `tests/e2e/test_workflow_files.py`, which does parse the files, and runs in CI for the
//! workflows other than the one it runs under.

use std::path::{Path, PathBuf};

fn workflows_dir() -> PathBuf {
    let mut path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    path.pop(); // iris-agentic-dev-bin → crates
    path.pop(); // crates → workspace root
    path.push(".github/workflows");
    path
}

fn workflow_files() -> Vec<PathBuf> {
    let dir = workflows_dir();
    let mut files: Vec<PathBuf> = std::fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("failed to read {}: {e}", dir.display()))
        .map(|entry| entry.expect("readdir entry").path())
        .filter(|p| {
            matches!(
                p.extension().and_then(|e| e.to_str()),
                Some("yml") | Some("yaml")
            )
        })
        .collect();
    files.sort();
    assert!(
        !files.is_empty(),
        "no workflow files found under {} — did the directory move?",
        dir.display()
    );
    files
}

/// A YAML key is a plain identifier (optionally quoted) followed by `:`. Anything else at
/// column 0 in a workflow file is loose text, which in practice means a block scalar lost
/// its indentation.
fn looks_like_a_key(line: &str) -> bool {
    let unquoted = line.trim_start_matches(['"', '\'']);
    let Some((name, _)) = unquoted.split_once(':') else {
        return false;
    };
    !name.is_empty()
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.'))
}

fn indent_of(line: &str) -> usize {
    line.len() - line.trim_start().len()
}

#[test]
fn no_workflow_line_at_column_zero_is_loose_text() {
    let mut problems = Vec::new();

    for file in workflow_files() {
        let content = read(&file);
        for (index, line) in content.lines().enumerate() {
            if line.trim().is_empty() || line.starts_with([' ', '\t', '#']) || line == "---" {
                continue;
            }
            if !looks_like_a_key(line) {
                problems.push(format!(
                    "{}:{}: {line}",
                    file.file_name().unwrap().to_string_lossy(),
                    index + 1
                ));
            }
        }
    }

    assert!(
        problems.is_empty(),
        "workflow lines at column 0 that are not top-level YAML keys — a line at column 0 \
         inside a `run: |` block ends the block scalar and makes the file unparseable, which \
         GitHub reports as a 0-second run with no jobs:\n  {}",
        problems.join("\n  ")
    );
}

#[test]
fn every_run_block_stays_indented_under_its_key() {
    let mut problems = Vec::new();

    for file in workflow_files() {
        let content = read(&file);
        let lines: Vec<&str> = content.lines().collect();

        for (index, line) in lines.iter().enumerate() {
            let trimmed = line.trim_start();
            if !(trimmed.starts_with("run: |") || trimmed.starts_with("run: >")) {
                continue;
            }
            let key_indent = indent_of(line);

            // Walk the block. The first non-blank line that is not indented deeper than
            // `run:` ends it, and that line has to be the next key or list item — not the
            // tail of a shell heredoc that dedented by accident.
            let mut body_lines = 0;
            for (offset, body) in lines[index + 1..].iter().enumerate() {
                if body.trim().is_empty() {
                    continue;
                }
                if indent_of(body) > key_indent {
                    body_lines += 1;
                    continue;
                }
                let ends_cleanly = looks_like_a_key(body.trim_start())
                    || body.trim_start().starts_with("- ")
                    || body.trim_start().starts_with('#');
                if !ends_cleanly {
                    problems.push(format!(
                        "{}:{}: block opened at line {} continues with `{}`, indented {} \
                         against a `run:` at {}",
                        file.file_name().unwrap().to_string_lossy(),
                        index + offset + 2,
                        index + 1,
                        body.trim_end(),
                        indent_of(body),
                        key_indent
                    ));
                }
                break;
            }

            assert!(
                body_lines > 0,
                "{}:{}: `run: |` with an empty body",
                file.file_name().unwrap().to_string_lossy(),
                index + 1
            );
        }
    }

    assert!(
        problems.is_empty(),
        "`run:` block scalars whose body escapes the block — the lines below run outside the \
         step, or stop the file parsing altogether:\n  {}",
        problems.join("\n  ")
    );
}

#[test]
fn every_workflow_declares_name_on_and_jobs() {
    for file in workflow_files() {
        let content = read(&file);
        let keys: Vec<String> = content
            .lines()
            .filter(|l| !l.starts_with([' ', '\t', '#']) && looks_like_a_key(l))
            .filter_map(|l| l.trim_start_matches(['"', '\'']).split(':').next())
            .map(|k| k.trim_matches(['"', '\'']).to_string())
            .collect();

        for required in ["name", "on", "jobs"] {
            assert!(
                keys.iter().any(|k| k == required),
                "{} has no top-level `{required}:` key (found: {keys:?}) — GitHub shows a \
                 workflow with no `name` by its file path, which is the tell that it failed \
                 to parse",
                file.file_name().unwrap().to_string_lossy()
            );
        }
    }
}

fn read(path: &Path) -> String {
    std::fs::read_to_string(path)
        .unwrap_or_else(|e| panic!("failed to read {}: {e}", path.display()))
}

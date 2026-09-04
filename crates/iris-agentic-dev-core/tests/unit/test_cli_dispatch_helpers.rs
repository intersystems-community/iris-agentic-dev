//! Tests for the pure helpers in `benchmark::cli_dispatch`: binary resolution, tool-call
//! parsing, the sentinel extractor, and the subprocess wrapper.
//!
//! The agentic loop itself needs an LLM and a container. These four are the parts that decide
//! whether the loop sees a tool call at all, and a parser that silently drops a malformed line
//! shows up as "the model did not call any tools".

use iris_agentic_dev_core::benchmark::cli_dispatch::{
    build_cli_dispatch_system_prompt, extract_sentinel_class, iris_dev_bin, parse_tool_invocations,
    run_tool_subprocess, ToolInvocation,
};
use std::path::{Path, PathBuf};

// ── iris_dev_bin ────────────────────────────────────────────────────────────────────────

/// The workspace root, derived the way the production code derives it: the core crate's
/// manifest directory, two levels up.
fn workspace_root() -> PathBuf {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let root = manifest
        .parent()
        .and_then(Path::parent)
        .expect("crates/<member> has two ancestors")
        .to_path_buf();
    assert!(
        root.join("Cargo.toml").is_file() && root.join("crates").is_dir(),
        "{} is not the workspace root",
        root.display()
    );
    root
}

#[test]
fn the_resolved_binary_sits_under_the_workspace_target_directory() {
    let p = iris_dev_bin();
    let root = workspace_root();

    assert!(p.is_absolute(), "got a relative path: {}", p.display());
    assert_eq!(
        p.file_name().and_then(|n| n.to_str()),
        Some("iris-agentic-dev")
    );
    assert!(
        p == root.join("target").join("debug").join("iris-agentic-dev")
            || p == root
                .join("target")
                .join("llvm-cov-target")
                .join("debug")
                .join("iris-agentic-dev"),
        "must be one of the two workspace debug builds, got {}",
        p.display()
    );
}

/// Under coverage the instrumented binary is the one that must run: a subprocess from
/// `target/debug` contributes nothing to the report and may be a different build entirely.
/// Which build exists depends on how the suite was invoked, so the expectation is computed
/// rather than skipped — a test that returns early here would assert nothing on a normal run.
#[test]
fn the_instrumented_build_is_preferred_over_the_plain_one() {
    let root = workspace_root();
    let cov = root
        .join("target")
        .join("llvm-cov-target")
        .join("debug")
        .join("iris-agentic-dev");
    let plain = root.join("target").join("debug").join("iris-agentic-dev");
    let expected = if cov.exists() { &cov } else { &plain };

    assert_eq!(
        &iris_dev_bin(),
        expected,
        "with llvm-cov build present={}, the resolver must pick {}",
        cov.exists(),
        expected.display()
    );
}

// ── parse_tool_invocations ──────────────────────────────────────────────────────────────

#[test]
fn a_quoted_invocation_is_parsed_into_name_and_args() {
    let got = parse_tool_invocations(
        "Let me look at the class.\n\
         iris-agentic-dev tool iris_symbols --args '{\"class\":\"My.Class\"}'\n\
         That should do it.",
    );
    assert_eq!(
        got,
        vec![ToolInvocation {
            tool_name: "iris_symbols".to_string(),
            args_json: r#"{"class":"My.Class"}"#.to_string(),
        }]
    );
}

/// The model does not reliably quote its JSON. Dropping the unquoted form would read as the
/// model never calling a tool.
#[test]
fn unquoted_args_are_taken_as_written() {
    let got = parse_tool_invocations("iris-agentic-dev tool iris_compile --args {\"x\":1}");
    assert_eq!(got.len(), 1);
    assert_eq!(got[0].args_json, r#"{"x":1}"#);
}

#[test]
fn invocations_come_back_in_document_order_and_indentation_is_ignored() {
    let got = parse_tool_invocations(
        "    iris-agentic-dev tool first --args '{}'\n\
         \tiris-agentic-dev tool second --args '{}'\n",
    );
    let names: Vec<&str> = got.iter().map(|i| i.tool_name.as_str()).collect();
    assert_eq!(names, vec!["first", "second"]);
}

#[test]
fn a_missing_args_flag_or_an_empty_name_yields_nothing() {
    assert!(
        parse_tool_invocations("iris-agentic-dev tool iris_compile").is_empty(),
        "no --args means the line is prose about the tool, not a call"
    );
    assert!(
        parse_tool_invocations("iris-agentic-dev tool  --args '{}'").is_empty(),
        "an empty tool name would spawn the binary with no subcommand"
    );
    assert!(parse_tool_invocations("").is_empty());
    assert!(
        parse_tool_invocations("run `iris-agentic-dev tool x --args '{}'` to fix it").is_empty(),
        "the marker has to start the line — prose that quotes it is not a call"
    );
}

// ── extract_sentinel_class ──────────────────────────────────────────────────────────────

#[test]
fn the_sentinel_block_is_extracted_and_trimmed() {
    let response = "Here you go.\n\
        ===FIXED_CLASS_START===\n\
        Class My.Fixed Extends %RegisteredObject { }\n\
        ===FIXED_CLASS_END===\n\
        Done.";
    assert_eq!(
        extract_sentinel_class(response).as_deref(),
        Some("Class My.Fixed Extends %RegisteredObject { }")
    );
}

#[test]
fn a_missing_or_empty_sentinel_block_is_none() {
    assert_eq!(extract_sentinel_class("no sentinel at all"), None);
    assert_eq!(
        extract_sentinel_class("===FIXED_CLASS_START===\nClass X {}"),
        None,
        "an unterminated block means the response was cut off mid-class"
    );
    assert_eq!(
        extract_sentinel_class("===FIXED_CLASS_START===\n   \n===FIXED_CLASS_END==="),
        None,
        "an empty block must not read as a successful fix"
    );
}

// ── build_cli_dispatch_system_prompt ────────────────────────────────────────────────────

#[test]
fn the_prompt_carries_the_call_format_and_both_sentinels() {
    let p = build_cli_dispatch_system_prompt("");
    assert!(!p.contains("# Skill guidance"), "no skill, no section");
    assert!(p.contains("iris-agentic-dev tool <tool_name> --args"));
    assert!(p.contains("===FIXED_CLASS_START==="));
    assert!(p.contains("===FIXED_CLASS_END==="));

    let with_skill = build_cli_dispatch_system_prompt("Use iris_symbols first.");
    assert!(with_skill.starts_with("# Skill guidance"));
    assert!(with_skill.contains("Use iris_symbols first."));
}

// ── run_tool_subprocess ─────────────────────────────────────────────────────────────────

fn invocation() -> ToolInvocation {
    ToolInvocation {
        tool_name: "iris_symbols".to_string(),
        args_json: "{}".to_string(),
    }
}

/// `/bin/echo` prints whatever it is handed, so this asserts the argv the wrapper builds:
/// `tool <name> --args <json>`, in that order.
#[test]
fn stdout_comes_back_and_the_argv_is_tool_name_args() {
    let out = run_tool_subprocess(Path::new("/bin/echo"), &invocation(), &[]);
    assert_eq!(out.trim(), "tool iris_symbols --args {}");
}

/// A stub that ignores its argv and prints its environment. `env`/`printenv` cannot be used
/// directly because the wrapper always passes `tool <name> …` as argv, which `env` would try to
/// execute.
#[test]
fn connection_env_reaches_the_child() {
    let dir = tempfile::tempdir().expect("tempdir");
    let script = dir.path().join("print-env.sh");
    std::fs::write(&script, "#!/bin/sh\nprintenv\n").expect("write stub");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755))
            .expect("chmod stub");
    }

    let env = vec![
        ("IRIS_HOST".to_string(), "sub-host".to_string()),
        ("IRIS_WEB_PORT".to_string(), "52780".to_string()),
    ];
    let out = run_tool_subprocess(&script, &invocation(), &env);
    assert!(
        out.contains("IRIS_HOST=sub-host"),
        "the child did not get IRIS_HOST: {out}"
    );
    assert!(
        out.contains("IRIS_WEB_PORT=52780"),
        "the child did not get IRIS_WEB_PORT: {out}"
    );
}

/// A tool that succeeds silently must not come back as an empty string — the agent would read
/// that as the tool having said nothing about a failure.
#[test]
fn a_silent_success_is_reported_as_no_output() {
    let out = run_tool_subprocess(Path::new("/usr/bin/true"), &invocation(), &[]);
    assert_eq!(out, "(no output)");
}

#[test]
fn a_silent_failure_names_the_exit_status() {
    let out = run_tool_subprocess(Path::new("/usr/bin/false"), &invocation(), &[]);
    assert!(
        out.starts_with("Error: process exited with status"),
        "a non-zero exit with no output must say so, got: {out}"
    );
}

/// The wrapper's contract is that it never panics, so the loop can report the failure to the
/// agent and try something else.
#[test]
fn a_missing_binary_is_returned_as_a_launch_error() {
    let out = run_tool_subprocess(
        Path::new("/nonexistent-iad-binary-for-tests/iris-agentic-dev"),
        &invocation(),
        &[],
    );
    assert!(
        out.starts_with("Error: failed to launch subprocess:"),
        "got: {out}"
    );
}

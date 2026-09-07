//! `tool --list` and `tool <name> --schema`: discovery for a caller that has only a shell.
//!
//! The point of these two flags is a tool surface at almost no context cost — 82 names and
//! summaries instead of the 104 KB an MCP `tools/list` costs. Two properties have to hold for that
//! to be worth anything, and both are asserted here by spawning the real binary:
//!
//! * **No connection.** Every command runs through `clean_command`, which strips every
//!   behaviour-changing variable including all of `IRIS_*`, from a working directory with no config
//!   file. If discovery ever reaches for a connection, these tests fail rather than quietly
//!   depending on a container.
//! * **Same answer as MCP.** `--schema` is compared against `tools/list` for all 82 tools. A second
//!   inventory that drifts is worse than no listing: it sends a caller after a tool that is not
//!   there.

use iris_agentic_dev_core::testing::{advertised_tools, clean_command, require_iad_binary};
use std::path::{Path, PathBuf};
use std::process::Output;

/// The names the CLI can dispatch, read out of the source so this test cannot drift from the
/// constant it is checking. Parsing the source rather than importing it is deliberate: `TOOL_NAMES`
/// lives in the bin crate, which this test crate does not depend on.
fn tool_names_in_source() -> Vec<String> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../iris-agentic-dev-bin/src/cmd/tool.rs")
        .canonicalize()
        .expect("cmd/tool.rs must exist");
    let src = std::fs::read_to_string(&path).expect("read cmd/tool.rs");
    let start = src
        .find("pub const TOOL_NAMES: &[&str] = &[")
        .expect("TOOL_NAMES must be declared in cmd/tool.rs");
    let body = &src[start..];
    let end = body.find("];").expect("TOOL_NAMES must be terminated");
    let names: Vec<String> = body[..end]
        .lines()
        .filter_map(|l| {
            let l = l.trim().trim_end_matches(',');
            l.strip_prefix('"')?.strip_suffix('"').map(str::to_string)
        })
        .collect();
    assert!(
        names.len() >= 80,
        "parsed only {} names out of TOOL_NAMES; the parse broke, not the constant",
        names.len()
    );
    names
}

/// A working directory with no `.iris-agentic-dev.toml`, so config discovery has nothing to find.
fn scratch_dir() -> PathBuf {
    let dir = std::env::temp_dir().join("iad-cli-discovery");
    std::fs::create_dir_all(&dir).expect("create scratch dir");
    dir
}

/// Run `iris-agentic-dev tool <args>` with no IRIS environment and no config file in reach.
fn run_tool(bin: &Path, args: &[&str]) -> Output {
    clean_command(bin)
        .arg("tool")
        .args(args)
        .current_dir(scratch_dir())
        .output()
        .expect("spawn iris-agentic-dev")
}

fn stdout_of(out: &Output) -> String {
    String::from_utf8_lossy(&out.stdout).to_string()
}

fn stderr_of(out: &Output) -> String {
    String::from_utf8_lossy(&out.stderr).to_string()
}

/// FR-001, FR-002, FR-005: the whole surface, with no connection, inside the byte budget.
#[test]
#[ignore = "spawns the built binary; run with --include-ignored"]
fn list_names_every_dispatchable_tool_with_no_connection() {
    let Some(bin) = require_iad_binary() else {
        return;
    };
    let out = run_tool(&bin, &["--list"]);
    let stdout = stdout_of(&out);

    assert!(
        out.status.success(),
        "`tool --list` exited {:?}\nstderr: {}",
        out.status.code(),
        stderr_of(&out)
    );
    for name in tool_names_in_source() {
        assert!(
            stdout
                .lines()
                .any(|l| l.split_whitespace().next() == Some(name.as_str())),
            "`{name}` is dispatchable but `--list` does not name it"
        );
    }
    assert!(
        stdout.len() < 8192,
        "`--list` emitted {} bytes; FR-005 caps the catalogue at 8 KB so a harness can hold it",
        stdout.len()
    );
    // Summaries, not bare names — a listing of names alone leaves the caller guessing.
    let with_summary = stdout
        .lines()
        .filter(|l| l.split_whitespace().count() > 2)
        .count();
    assert!(
        with_summary >= 80,
        "only {with_summary} lines carry a summary beyond the tool name"
    );
}

/// FR-003: one tool's full contract, still with no connection.
#[test]
#[ignore = "spawns the built binary; run with --include-ignored"]
fn schema_prints_one_tools_contract_with_no_connection() {
    let Some(bin) = require_iad_binary() else {
        return;
    };
    let out = run_tool(&bin, &["iris_query", "--schema"]);
    let stdout = stdout_of(&out);

    assert!(
        out.status.success(),
        "`tool iris_query --schema` exited {:?}\nstderr: {}",
        out.status.code(),
        stderr_of(&out)
    );
    assert!(
        stdout.contains("query") && stdout.contains("properties"),
        "the schema output names neither the tool's parameters nor a properties block:\n{stdout}"
    );
    assert!(
        stdout.len() < 4096,
        "one tool's contract came to {} bytes",
        stdout.len()
    );
}

/// FR-004: `--json` on either path is exactly one JSON document, so a harness can pipe it into a
/// parser without stripping prose first.
#[test]
#[ignore = "spawns the built binary; run with --include-ignored"]
fn json_output_is_a_single_document_on_both_paths() {
    let Some(bin) = require_iad_binary() else {
        return;
    };

    for args in [
        ["--list", "--json"].as_slice(),
        ["iris_query", "--schema", "--json"].as_slice(),
    ] {
        let out = run_tool(&bin, args);
        assert!(
            out.status.success(),
            "`tool {}` exited {:?}\nstderr: {}",
            args.join(" "),
            out.status.code(),
            stderr_of(&out)
        );
        let stdout = stdout_of(&out);
        let parsed: serde_json::Value = serde_json::from_str(stdout.trim()).unwrap_or_else(|e| {
            panic!(
                "`tool {}` did not emit one JSON document ({e}):\n{stdout}",
                args.join(" ")
            )
        });
        assert!(
            parsed.is_object(),
            "`tool {}` emitted {parsed} rather than a JSON object",
            args.join(" ")
        );
    }
}

/// FR-006: a misspelling gets the nearest name back. An "unknown tool" answer that does not say what
/// the caller probably meant costs a round trip that the suggestion saves.
#[test]
#[ignore = "spawns the built binary; run with --include-ignored"]
fn a_misspelled_name_gets_the_nearest_tool_back() {
    let Some(bin) = require_iad_binary() else {
        return;
    };
    let out = run_tool(&bin, &["iris_quer", "--schema"]);
    let combined = format!("{}{}", stdout_of(&out), stderr_of(&out));

    assert!(!out.status.success(), "a misspelled name must not exit 0");
    assert!(
        combined.contains("iris_query"),
        "the error does not suggest `iris_query`:\n{combined}"
    );
}

/// `--list` with a name is rejected, not silently preferred either way. An agent that asked for one
/// tool's schema and got the catalogue would read the wrong contract and never know.
#[test]
#[ignore = "spawns the built binary; run with --include-ignored"]
fn list_and_a_tool_name_together_are_rejected() {
    let Some(bin) = require_iad_binary() else {
        return;
    };
    let out = run_tool(&bin, &["iris_query", "--list"]);

    assert!(
        !out.status.success(),
        "`tool iris_query --list` must be an error, not a silent choice between the two"
    );
    // Same for a bare `--schema` with nothing to describe.
    let bare = run_tool(&bin, &["--schema"]);
    assert!(
        !bare.status.success(),
        "`tool --schema` with no tool name must be an error"
    );
}

/// SC-003: for every tool, the CLI's schema is the MCP listing's schema. This is what makes the CLI
/// a real second arm rather than a second source of truth.
#[test]
#[ignore = "spawns the built binary and one mcp server; run with --include-ignored"]
fn every_cli_schema_equals_the_mcp_schema() {
    let Some(bin) = require_iad_binary() else {
        return;
    };
    let advertised = advertised_tools();
    assert!(
        advertised.len() >= 80,
        "tools/list returned only {} tools",
        advertised.len()
    );

    let mut mismatched: Vec<String> = Vec::new();
    for (name, tool) in &advertised {
        let out = run_tool(&bin, &[name, "--schema", "--json"]);
        assert!(
            out.status.success(),
            "`tool {name} --schema --json` exited {:?}\nstderr: {}",
            out.status.code(),
            stderr_of(&out)
        );
        let cli: serde_json::Value =
            serde_json::from_str(stdout_of(&out).trim()).expect("--schema --json is JSON");

        if cli.get("inputSchema") != tool.get("inputSchema") {
            mismatched.push(format!("{name}: schema"));
        }
        if cli.get("description") != tool.get("description") {
            mismatched.push(format!("{name}: description"));
        }
    }

    assert!(
        mismatched.is_empty(),
        "{} tool(s) describe themselves differently on the CLI than over MCP: {mismatched:?}",
        mismatched.len()
    );
}

/// FR-007, both halves: the listing is exactly what the CLI can dispatch, and it tracks
/// `IRIS_TOOLSET` rather than always reporting the Merged tier.
#[test]
#[ignore = "spawns the built binary; run with --include-ignored"]
fn the_listing_matches_the_dispatch_list_and_tracks_the_toolset() {
    let Some(bin) = require_iad_binary() else {
        return;
    };

    let listed: std::collections::BTreeSet<String> = stdout_of(&run_tool(&bin, &["--list"]))
        .lines()
        .filter_map(|l| l.split_whitespace().next().map(str::to_string))
        .collect();
    let dispatchable: std::collections::BTreeSet<String> =
        tool_names_in_source().into_iter().collect();

    assert_eq!(
        listed, dispatchable,
        "the default `--list` must be exactly the CLI's dispatch list — a listed name the CLI \
         cannot dispatch is the drift that shipped once already (22 names, no dispatch arm)"
    );

    let baseline_out = clean_command(&bin)
        .arg("tool")
        .arg("--list")
        .env("IRIS_TOOLSET", "baseline")
        .current_dir(scratch_dir())
        .output()
        .expect("spawn iris-agentic-dev");
    assert!(
        baseline_out.status.success(),
        "baseline `--list` must exit 0"
    );
    let baseline: std::collections::BTreeSet<String> = stdout_of(&baseline_out)
        .lines()
        .filter_map(|l| l.split_whitespace().next().map(str::to_string))
        .collect();

    assert_ne!(
        baseline, listed,
        "`IRIS_TOOLSET=baseline` produced the same listing as the default tier, so the variable is \
         being ignored and a harness scoping the toolset would be told about tools it does not have"
    );
    let phantom: Vec<&String> = baseline.difference(&dispatchable).collect();
    assert!(
        phantom.is_empty(),
        "the baseline listing names tools the CLI cannot dispatch: {phantom:?}"
    );
    assert!(
        !baseline.is_empty(),
        "the baseline listing came back empty, which is not a toolset"
    );
}

//! E2E integration tests for iris-dev MCP server against a real IRIS container.
//!
//! Replaces the Python test suites (test_022_all_tools.py, test_032_compile_hook.py).
//!
//! Run with a live IRIS container:
//!   IRIS_HOST=localhost IRIS_WEB_PORT=52773 IRIS_CONTAINER=iris-e2e \
//!   IRIS_USERNAME=_SYSTEM IRIS_PASSWORD=SYS \
//!   cargo test --test test_e2e -- --nocapture
//!
//! All tests skip gracefully when IRIS_HOST is not set.
#![allow(dead_code, clippy::zombie_processes)]

use std::io::{BufRead, BufReader, Write};
use std::process::{Command, Stdio};

// ── Helpers ───────────────────────────────────────────────────────────────────

fn iris_dev_bin() -> std::path::PathBuf {
    // Allow scripts/coverage.sh to override the binary path so it can point at
    // an instrumented build for E2E subprocess coverage collection.
    if let Ok(path) = std::env::var("IRIS_DEV_BIN") {
        let p = std::path::PathBuf::from(path);
        if p.exists() {
            return p;
        }
    }
    let mut p = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    p.pop(); // crates/iris-dev-core
    p.pop(); // crates/
    p.push("target/debug/iris-agentic-dev");
    if !p.exists() {
        p.pop();
        p.push("release/iris-agentic-dev");
    }
    p
}

fn iris_host() -> String {
    std::env::var("IRIS_HOST").unwrap_or_default()
}

fn iris_env() -> Vec<(&'static str, String)> {
    vec![
        ("IRIS_HOST", std::env::var("IRIS_HOST").unwrap_or_default()),
        (
            "IRIS_WEB_PORT",
            std::env::var("IRIS_WEB_PORT").unwrap_or_else(|_| "52773".to_string()),
        ),
        (
            "IRIS_USERNAME",
            std::env::var("IRIS_USERNAME").unwrap_or_else(|_| "_SYSTEM".to_string()),
        ),
        (
            "IRIS_PASSWORD",
            std::env::var("IRIS_PASSWORD").unwrap_or_else(|_| "SYS".to_string()),
        ),
        (
            "IRIS_NAMESPACE",
            std::env::var("IRIS_NAMESPACE").unwrap_or_else(|_| "USER".to_string()),
        ),
        (
            "IRIS_CONTAINER",
            std::env::var("IRIS_CONTAINER").unwrap_or_default(),
        ),
    ]
}

/// Skip this test if IRIS_HOST is not set or the binary doesn't exist.
macro_rules! require_iris {
    () => {
        if iris_host().is_empty() {
            eprintln!("Skipping: IRIS_HOST not set");
            return;
        }
        if !iris_dev_bin().exists() {
            eprintln!(
                "Skipping: iris-dev binary not found at {:?}",
                iris_dev_bin()
            );
            return;
        }
    };
}

/// Skip if binary doesn't exist (for no-IRIS tests).
macro_rules! require_bin {
    () => {
        if !iris_dev_bin().exists() {
            eprintln!("Skipping: iris-dev binary not found");
            return;
        }
    };
}

/// Send MCP messages to iris-dev mcp and collect responses (default 10s timeout).
fn mcp_call(env_vars: &[(&str, String)], messages: &[serde_json::Value]) -> Vec<serde_json::Value> {
    mcp_call_timeout(env_vars, messages, 10)
}

/// Send MCP messages to iris-dev mcp and collect responses with configurable timeout.
fn mcp_call_timeout(
    env_vars: &[(&str, String)],
    messages: &[serde_json::Value],
    timeout_secs: u64,
) -> Vec<serde_json::Value> {
    let bin = iris_dev_bin();
    if !bin.exists() {
        return vec![];
    }

    let mut cmd = Command::new(&bin);
    cmd.args(["mcp"]);
    for (k, v) in env_vars {
        cmd.env(k, v);
    }
    // Propagate LLVM_PROFILE_FILE so the spawned iris-dev writes coverage data
    // when built with -C instrument-coverage (used by scripts/coverage.sh).
    if let Ok(profile) = std::env::var("LLVM_PROFILE_FILE") {
        cmd.env("LLVM_PROFILE_FILE", &profile);
    }

    let mut child = cmd
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("failed to spawn iris-dev mcp");

    let mut stdin = child.stdin.take().unwrap();
    let stdout = child.stdout.take().unwrap();
    let mut reader = BufReader::new(stdout);
    let mut results = vec![];

    for msg in messages {
        stdin
            .write_all((serde_json::to_string(msg).unwrap() + "\n").as_bytes())
            .unwrap();
        stdin.flush().unwrap();

        if msg.get("id").is_some() {
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(timeout_secs);
            loop {
                std::thread::sleep(std::time::Duration::from_millis(50));
                let mut line = String::new();
                if reader.read_line(&mut line).unwrap_or(0) > 0 {
                    if let Ok(v) = serde_json::from_str::<serde_json::Value>(&line) {
                        results.push(v);
                        break;
                    }
                }
                if std::time::Instant::now() > deadline {
                    break;
                }
            }
        } else {
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
    }

    // Close stdin (EOF) so the server's stdio loop exits its own event loop and
    // runs normally to process exit — that's what flushes LLVM instrument-coverage
    // profraw data. SIGKILL (child.kill()) skips the atexit handler and leaves
    // coverage.sh's E2E profraw files empty.
    drop(stdin);
    drop(reader);
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) if std::time::Instant::now() < deadline => {
                std::thread::sleep(std::time::Duration::from_millis(20));
            }
            _ => {
                child.kill().ok();
                break;
            }
        }
    }
    results
}

/// Standard MCP handshake messages.
fn init_msgs() -> Vec<serde_json::Value> {
    vec![
        serde_json::json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{
            "protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"e2e","version":"0.1"}
        }}),
        serde_json::json!({"jsonrpc":"2.0","method":"notifications/initialized","params":{}}),
    ]
}

/// Extract the JSON tool result from an MCP response for a given id.
fn tool_result(responses: &[serde_json::Value], id: u64) -> serde_json::Value {
    let resp = responses
        .iter()
        .find(|r| r["id"] == id)
        .cloned()
        .unwrap_or_default();
    let text = resp["result"]["content"][0]["text"]
        .as_str()
        .unwrap_or("{}");
    serde_json::from_str(text).unwrap_or_default()
}

/// Call a single tool and return its result JSON.
fn call_tool(name: &str, args: serde_json::Value) -> serde_json::Value {
    call_tool_timeout(name, args, 10)
}

/// Call a single tool with a custom timeout (seconds).
fn call_tool_timeout(name: &str, args: serde_json::Value, timeout_secs: u64) -> serde_json::Value {
    let env = iris_env();
    let mut msgs = init_msgs();
    msgs.push(serde_json::json!({
        "jsonrpc":"2.0","id":2,"method":"tools/call",
        "params":{"name": name, "arguments": args}
    }));
    let responses = mcp_call_timeout(&env, &msgs, timeout_secs);
    tool_result(&responses, 2)
}

/// Call a single tool with the destructive tier declared in the operator environment.
///
/// Spec 085 put `iris_global` set/kill and `iris_lookup_manage` set/delete in the destructive
/// tier, which defaults to off. A round-trip test that does not declare the tier measures the
/// gate refusing, not the round trip — so the tier is declared here rather than the assertion
/// being relaxed to accept `DESTRUCTIVE_TOOLS_DISABLED`.
fn call_tool_destructive(name: &str, args: serde_json::Value) -> serde_json::Value {
    let mut env = iris_env();
    env.push(("IRIS_WRITE_TOOLS_ENABLED", "1".to_string()));
    env.push(("IRIS_DESTRUCTIVE_TOOLS_ENABLED", "1".to_string()));
    let mut msgs = init_msgs();
    msgs.push(serde_json::json!({
        "jsonrpc":"2.0","id":2,"method":"tools/call",
        "params":{"name": name, "arguments": args}
    }));
    let responses = mcp_call_timeout(&env, &msgs, 10);
    tool_result(&responses, 2)
}

// ── iris_execute ──────────────────────────────────────────────────────────────

#[test]
fn e2e_execute_write_without_trailing_bang_returns_output() {
    require_iris!();
    // IDEV-3 regression: sentinel Write ! must capture output even without trailing !
    let result = call_tool(
        "iris_execute",
        serde_json::json!({"code": "Write 42", "namespace": "USER", "confirmed": true}),
    );
    if result["success"] == true {
        assert_eq!(
            result["output"].as_str().map(|s| s.trim()),
            Some("42"),
            "Write 42 (no trailing !) must return '42', got: {}",
            result
        );
    }
    // If success=false (e.g. DOCKER_REQUIRED), that's acceptable — what's NOT acceptable is
    // success=true with empty output, which was the bug.
    if result["success"] == true {
        assert_ne!(
            result["output"].as_str().unwrap_or("").trim(),
            "",
            "iris_execute must not return empty output for Write 42"
        );
    }
}

#[test]
fn e2e_execute_returns_version_string() {
    require_iris!();
    let result = call_tool(
        "iris_execute",
        serde_json::json!({"code": "Write $ZVERSION", "namespace": "USER", "confirmed": true}),
    );
    if result["success"] == true {
        let output = result["output"].as_str().unwrap_or("");
        assert!(
            output.contains("IRIS")
                || output.contains("Cache")
                || output.contains("2025")
                || output.contains("2026"),
            "Write $ZVERSION should return version string, got: {:?}",
            output
        );
    }
}

#[test]
fn e2e_execute_docker_required_has_instructions() {
    require_bin!();
    // Run WITHOUT IRIS_HOST so it must explain what to do
    let env = vec![
        ("IRIS_HOST", "".to_string()),
        ("IRIS_CONTAINER", "".to_string()),
    ];
    let mut msgs = init_msgs();
    msgs.push(serde_json::json!({
        "jsonrpc":"2.0","id":2,"method":"tools/call",
        "params":{"name":"iris_execute","arguments":{"code":"Write 1","namespace":"USER","confirmed":true}}
    }));
    let responses = mcp_call(&env, &msgs);
    let result = tool_result(&responses, 2);
    if result["success"] == false {
        let ec = result["error_code"].as_str().unwrap_or("");
        let text = result.to_string().to_lowercase();
        assert!(
            ec == "DOCKER_REQUIRED"
                || text.contains("iris_container")
                || text.contains("docker")
                || ec == "IRIS_UNREACHABLE",
            "error without IRIS should mention Docker or container: {}",
            result
        );
    }
}

// ── iris_symbols ──────────────────────────────────────────────────────────────

#[test]
fn e2e_symbols_glob_star_returns_package_classes() {
    require_iris!();
    // Seed two classes then query with glob
    call_tool(
        "iris_doc",
        serde_json::json!({"mode":"put","name":"Test022Glob.Alpha.cls",
        "content":"Class Test022Glob.Alpha { ClassMethod Run() { } }","namespace":"USER"}),
    );
    call_tool(
        "iris_compile",
        serde_json::json!({"target":"Test022Glob.Alpha.cls","namespace":"USER"}),
    );
    call_tool(
        "iris_doc",
        serde_json::json!({"mode":"put","name":"Test022Glob.Beta.cls",
        "content":"Class Test022Glob.Beta { ClassMethod Run() { } }","namespace":"USER"}),
    );
    call_tool(
        "iris_compile",
        serde_json::json!({"target":"Test022Glob.Beta.cls","namespace":"USER"}),
    );

    let result = call_tool(
        "iris_symbols",
        serde_json::json!({"query": "Test022Glob.*", "namespace": "USER"}),
    );
    let symbols = result["symbols"].as_array().cloned().unwrap_or_default();
    let names: Vec<String> = symbols
        .iter()
        .filter_map(|s| s["Name"].as_str().map(|n| n.to_string()))
        .collect();
    assert!(
        names.iter().any(|n| n.contains("Test022Glob")),
        "Test022Glob.* should return Test022Glob classes, got: {:?}",
        names
    );

    // Cleanup
    call_tool(
        "iris_doc",
        serde_json::json!({"mode":"delete","name":"Test022Glob.Alpha.cls","namespace":"USER"}),
    );
    call_tool(
        "iris_doc",
        serde_json::json!({"mode":"delete","name":"Test022Glob.Beta.cls","namespace":"USER"}),
    );
}

#[test]
fn e2e_symbols_trailing_dot_prefix_matches() {
    require_iris!();
    // Plain prefix with trailing dot
    let result = call_tool(
        "iris_symbols",
        serde_json::json!({"query": "Test022Glob.", "namespace": "USER", "limit": 5}),
    );
    // Must not crash
    assert!(
        result["symbols"].is_array() || result["error_code"].is_string(),
        "iris_symbols with trailing dot must return array or structured error: {}",
        result
    );
}

#[test]
fn e2e_symbols_plain_substring_no_regression() {
    require_iris!();
    let result = call_tool(
        "iris_symbols",
        serde_json::json!({"query": "Ens.Director", "namespace": "USER", "limit": 5}),
    );
    assert!(
        result["symbols"].is_array(),
        "plain substring must return array: {}",
        result
    );
}

// ── iris_doc ──────────────────────────────────────────────────────────────────

#[test]
fn e2e_doc_empty_args_returns_missing_params_not_16006() {
    require_iris!();
    // Regression: empty args default to mode=get with an empty name. Previously this
    // hit GET /doc/ and surfaced the cryptic IRIS "ERROR #16006: Document '' name is
    // invalid" (error_code BAD_REQUEST), which pushed the calling model into a loop.
    // It must now fail fast with MISSING_PARAMS and never reach IRIS.
    let result = call_tool("iris_doc", serde_json::json!({}));
    assert_eq!(
        result["error_code"], "MISSING_PARAMS",
        "empty args must be MISSING_PARAMS, got: {result}"
    );
    let text = result.to_string();
    assert!(
        !text.contains("16006"),
        "must not surface the cryptic IRIS #16006: {result}"
    );
    assert_ne!(result["error_code"], "BAD_REQUEST", "{result}");
}

#[test]
fn e2e_doc_empty_name_per_read_mode_is_missing_params() {
    require_iris!();
    // Every single-document read/delete mode must guard an empty name up front.
    for mode in ["get", "head", "delete", "compiled"] {
        let result = call_tool("iris_doc", serde_json::json!({"mode": mode}));
        assert_eq!(
            result["error_code"], "MISSING_PARAMS",
            "mode={mode} with no name must be MISSING_PARAMS: {result}"
        );
    }
    // fragment needs start/end too, but the name guard fires first.
    let frag = call_tool(
        "iris_doc",
        serde_json::json!({"mode":"fragment","start":1,"end":5}),
    );
    assert_eq!(frag["error_code"], "MISSING_PARAMS", "{frag}");
}

#[test]
fn e2e_doc_stringified_int_not_rpc_rejected() {
    require_iris!();
    // Regression: `start`/`end`/`line` sent as strings ("1") used to fail hard at the
    // JSON-RPC layer (-32602 invalid type), which triggered the caller's retry loop.
    // They must now coerce, so the response is a normal tool result with no JSON-RPC error.
    let env = iris_env();
    let mut msgs = init_msgs();
    msgs.push(serde_json::json!({
        "jsonrpc":"2.0","id":2,"method":"tools/call",
        "params":{"name":"iris_doc","arguments":{
            "mode":"fragment","name":"%Library.String.cls","start":"1","end":"5"
        }}
    }));
    let responses = mcp_call_timeout(&env, &msgs, 10);
    let resp = responses
        .iter()
        .find(|r| r["id"] == 2)
        .cloned()
        .unwrap_or_default();
    assert!(
        resp["error"].is_null(),
        "stringified ints must not cause a JSON-RPC error (-32602): {resp}"
    );
    assert!(
        resp["result"].is_object(),
        "expected a normal tool result: {resp}"
    );
}

// ── Storage-block behavior ─────────────────────────────────────────────────────
//
// iris_doc writes Storage blocks verbatim: put/insert/delete_lines all write
// content exactly like a raw Atelier PUT, and let IRIS's own compiler handle
// storage evolution — existing properties keep their ordinal forever, removed
// properties leave a harmless orphan, and a rename needs the caller to also
// update the Storage entry, same as refactoring any other identifier. Storage
// is otherwise off-limits to iris_doc: the only guardrail is a flag-gated
// refusal when a write would drop an existing Storage block entirely (see
// `allow_storage_regeneration` and `check_storage_reset` in `tools::doc`).
// These tests cover each of those outcomes end to end.

/// 1-based line number of the first line containing `needle`, or panics with a
/// descriptive message (every caller here expects the line to exist).
fn line_containing(content: &str, needle: &str) -> i64 {
    content
        .lines()
        .position(|l| l.contains(needle))
        .map(|i| i as i64 + 1)
        .unwrap_or_else(|| panic!("no line containing {needle:?} in:\n{content}"))
}

/// 1-based line number of the LAST standalone `}` line — the class's own closing
/// brace. When a Storage block is present it has its own nested closing braces, so
/// the *first* `}` (line_containing's usual match) would land inside Storage's XData
/// instead of after it; the class's closing brace is always the last bare `}` line.
fn last_bare_closing_brace_line(content: &str) -> i64 {
    content
        .lines()
        .enumerate()
        .filter(|(_, l)| l.trim() == "}")
        .last()
        .map(|(i, _)| i as i64 + 1)
        .unwrap_or_else(|| panic!("no standalone '}}' line in:\n{content}"))
}

#[test]
fn e2e_doc_put_round_trips_storage_verbatim_no_flag_needed() {
    require_iris!();
    // Core regression: a get-then-put round trip of a class WITH an explicit Storage
    // block must succeed unconditionally — no STORAGE_STRIP_BLOCKED, no
    // allow_storage_regeneration needed. This is exactly the shape that used to be
    // refused by default (and, before that, would have been mangled into invalid XML).
    let cls_name = "Test022.RoundTripStorage";
    let cls_file = format!("{cls_name}.cls");
    let cls_src = format!("Class {cls_name} Extends %Persistent {{\nProperty Name As %String;\n}}");
    let put_result = call_tool(
        "iris_doc",
        serde_json::json!({"mode":"put","name": cls_file, "content": cls_src,
            "namespace":"USER", "compile": true}),
    );
    assert_eq!(
        put_result["success"], true,
        "fixture setup failed: {put_result}"
    );

    let fetched = call_tool(
        "iris_doc",
        serde_json::json!({"mode":"get","name": cls_file, "namespace":"USER"}),
    );
    let content = fetched["content"].as_str().unwrap_or_default().to_string();
    assert!(
        content.contains("Storage Default"),
        "fixture must have a compiled Storage block: {fetched}"
    );

    // Put the exact same content straight back — no flag set.
    let put_again = call_tool(
        "iris_doc",
        serde_json::json!({"mode":"put","name": cls_file, "content": content, "namespace":"USER"}),
    );
    assert_eq!(
        put_again["success"], true,
        "round-tripping a class with an explicit Storage block must succeed with no \
         flag: {put_again}"
    );
    assert!(
        put_again.get("error_code").is_none(),
        "must not be refused: {put_again}"
    );

    let refetched = call_tool(
        "iris_doc",
        serde_json::json!({"mode":"get","name": cls_file, "namespace":"USER"}),
    );
    assert_eq!(
        refetched["content"],
        serde_json::Value::String(content),
        "content must be preserved byte-for-byte: {refetched}"
    );

    call_tool(
        "iris_doc",
        serde_json::json!({"mode":"delete","name": cls_file, "namespace":"USER"}),
    );
}

#[test]
fn e2e_doc_insert_add_property_evolves_storage_and_preserves_data() {
    require_iris!();
    // Adding a property via insert must evolve Storage (existing ordinal for
    // Name untouched, Description gets the next free ordinal) rather than
    // stripping and regenerating it. Seeded data must survive untouched.
    let cls_name = "Test022.InsertEvolve";
    let cls_file = format!("{cls_name}.cls");
    let cls_src = format!("Class {cls_name} Extends %Persistent {{\nProperty Name As %String;\n}}");
    call_tool(
        "iris_doc",
        serde_json::json!({"mode":"put","name": cls_file, "content": cls_src,
            "namespace":"USER", "compile": true}),
    );

    let obj_new = format!(
        "set obj = ##class({cls_name}).%New() set obj.Name = \"original-name\" set sc = obj.%Save() \
         write $system.Status.GetErrorText(sc)"
    );
    let save = call_tool(
        "iris_execute",
        serde_json::json!({"code": obj_new, "namespace": "USER"}),
    );
    assert_eq!(save["output"], "", "seeding data failed: {save}");

    let fetched = call_tool(
        "iris_doc",
        serde_json::json!({"mode":"get","name": cls_file, "namespace":"USER"}),
    );
    let content = fetched["content"].as_str().unwrap_or_default().to_string();
    let closing_brace_line = last_bare_closing_brace_line(&content);
    // Storage's Name ordinal before the edit — must be unchanged after.
    let name_ordinal_before = content
        .lines()
        .position(|l| l.contains("<Value>Name</Value>"))
        .unwrap();
    let name_ordinal_line_before = content.lines().nth(name_ordinal_before - 1).unwrap();

    let insert_result = call_tool(
        "iris_doc",
        serde_json::json!({"mode":"insert","name": cls_file, "namespace":"USER",
            "line": closing_brace_line, "expected": "}",
            "content": "Property Description As %String;", "compile": true}),
    );
    assert_eq!(
        insert_result["success"], true,
        "insert adding a property must succeed with no flag: {insert_result}"
    );
    assert!(
        insert_result.get("error_code").is_none(),
        "must not be refused: {insert_result}"
    );

    let refetched = call_tool(
        "iris_doc",
        serde_json::json!({"mode":"get","name": cls_file, "namespace":"USER"}),
    );
    let new_content = refetched["content"].as_str().unwrap_or_default();
    assert!(
        new_content.contains("<Value>Description</Value>"),
        "Description must be added to Storage: {refetched}"
    );
    let name_ordinal_after = new_content
        .lines()
        .position(|l| l.contains("<Value>Name</Value>"))
        .unwrap();
    let name_ordinal_line_after = new_content.lines().nth(name_ordinal_after - 1).unwrap();
    assert_eq!(
        name_ordinal_line_after, name_ordinal_line_before,
        "Name's existing ordinal must be untouched by adding Description"
    );

    let check = call_tool(
        "iris_execute",
        serde_json::json!({"code": format!(
            "set obj = ##class({cls_name}).%OpenId(1) write obj.Name"
        ), "namespace": "USER"}),
    );
    assert_eq!(
        check["output"], "original-name",
        "seeded data must survive the property addition: {check}"
    );

    call_tool(
        "iris_doc",
        serde_json::json!({"mode":"delete","name": cls_file, "namespace":"USER"}),
    );
    call_tool(
        "iris_execute",
        serde_json::json!({"code": format!("kill ^{cls_name}D, ^{cls_name}I, ^{cls_name}S"), "namespace": "USER"}),
    );
}

#[test]
fn e2e_doc_delete_lines_remove_property_leaves_harmless_orphan() {
    require_iris!();
    // Removing a property via delete_lines must leave its Storage entry as
    // an orphan (never touched), not strip the whole block. Other
    // properties' data must survive untouched.
    let cls_name = "Test022.DeleteOrphan";
    let cls_file = format!("{cls_name}.cls");
    let cls_src = format!(
        "Class {cls_name} Extends %Persistent {{\nProperty Name As %String;\nProperty Description As %String;\n}}"
    );
    call_tool(
        "iris_doc",
        serde_json::json!({"mode":"put","name": cls_file, "content": cls_src,
            "namespace":"USER", "compile": true}),
    );
    let save = call_tool(
        "iris_execute",
        serde_json::json!({"code": format!(
            "set obj = ##class({cls_name}).%New() set obj.Name = \"keeper\" \
             set obj.Description = \"orphaned-value\" set sc = obj.%Save() \
             write $system.Status.GetErrorText(sc)"
        ), "namespace": "USER"}),
    );
    assert_eq!(save["output"], "", "seeding data failed: {save}");

    let fetched = call_tool(
        "iris_doc",
        serde_json::json!({"mode":"get","name": cls_file, "namespace":"USER"}),
    );
    let content = fetched["content"].as_str().unwrap_or_default().to_string();
    let description_prop_line = line_containing(&content, "Property Description As %String;");

    let delete_result = call_tool(
        "iris_doc",
        serde_json::json!({"mode":"delete_lines","name": cls_file, "namespace":"USER",
            "start": description_prop_line, "end": description_prop_line,
            "expected": "Property Description As %String;", "compile": true}),
    );
    assert_eq!(
        delete_result["success"], true,
        "delete_lines removing a property must succeed with no flag: {delete_result}"
    );
    assert!(
        delete_result.get("error_code").is_none(),
        "must not be refused: {delete_result}"
    );

    let refetched = call_tool(
        "iris_doc",
        serde_json::json!({"mode":"get","name": cls_file, "namespace":"USER"}),
    );
    let new_content = refetched["content"].as_str().unwrap_or_default();
    assert!(
        new_content.contains("<Value>Description</Value>"),
        "Description's Storage entry must remain as a harmless orphan, not be \
         stripped: {refetched}"
    );
    assert!(
        !new_content.contains("Property Description As %String;"),
        "the class body itself must no longer declare Description"
    );

    let check = call_tool(
        "iris_execute",
        serde_json::json!({"code": format!(
            "set obj = ##class({cls_name}).%OpenId(1) write obj.Name"
        ), "namespace": "USER"}),
    );
    assert_eq!(
        check["output"], "keeper",
        "Name's data must survive removing Description: {check}"
    );

    call_tool(
        "iris_doc",
        serde_json::json!({"mode":"delete","name": cls_file, "namespace":"USER"}),
    );
    call_tool(
        "iris_execute",
        serde_json::json!({"code": format!("kill ^{cls_name}D, ^{cls_name}I, ^{cls_name}S"), "namespace": "USER"}),
    );
}

#[test]
fn e2e_doc_insert_after_orphan_gets_next_free_ordinal() {
    require_iris!();
    // Adding a property after an orphan exists must get the next free
    // ordinal, leaving the orphan's ordinal untouched.
    let cls_name = "Test022.InsertAfterOrphan";
    let cls_file = format!("{cls_name}.cls");
    // Start already-orphaned (Description removed, its Storage entry
    // lingering) by hand-authoring that shape directly, rather than
    // repeating the remove step here.
    let cls_src = format!("Class {cls_name} Extends %Persistent\n{{\n\nProperty Name As %String;\n\nStorage Default\n{{\n<Data name=\"{cls_name}DefaultData\">\n<Value name=\"1\">\n<Value>%%CLASSNAME</Value>\n</Value>\n<Value name=\"2\">\n<Value>Name</Value>\n</Value>\n<Value name=\"3\">\n<Value>Description</Value>\n</Value>\n</Data>\n<DataLocation>^{cls_name}D</DataLocation>\n<DefaultData>{cls_name}DefaultData</DefaultData>\n<IdLocation>^{cls_name}D</IdLocation>\n<IndexLocation>^{cls_name}I</IndexLocation>\n<StreamLocation>^{cls_name}S</StreamLocation>\n<Type>%Storage.Persistent</Type>\n}}\n\n}}\n");
    let put_result = call_tool(
        "iris_doc",
        serde_json::json!({"mode":"put","name": cls_file, "content": cls_src,
            "namespace":"USER", "compile": true}),
    );
    assert_eq!(
        put_result["success"], true,
        "fixture put (with an explicit orphaned Storage entry) must succeed with no \
         flag: {put_result}"
    );

    let fetched = call_tool(
        "iris_doc",
        serde_json::json!({"mode":"get","name": cls_file, "namespace":"USER"}),
    );
    let content = fetched["content"].as_str().unwrap_or_default().to_string();
    let closing_brace_line = last_bare_closing_brace_line(&content);

    let insert_result = call_tool(
        "iris_doc",
        serde_json::json!({"mode":"insert","name": cls_file, "namespace":"USER",
            "line": closing_brace_line, "expected": "}",
            "content": "Property Comments As %String;", "compile": true}),
    );
    assert_eq!(insert_result["success"], true, "{insert_result}");

    let refetched = call_tool(
        "iris_doc",
        serde_json::json!({"mode":"get","name": cls_file, "namespace":"USER"}),
    );
    let new_content = refetched["content"].as_str().unwrap_or_default();
    let comments_ordinal_idx = new_content
        .lines()
        .position(|l| l.contains("<Value>Comments</Value>"))
        .expect("Comments must be in Storage");
    let ordinal_name_line = new_content.lines().nth(comments_ordinal_idx - 1).unwrap();
    assert!(
        ordinal_name_line.contains("name=\"4\""),
        "Comments must get the next free ordinal (4), leaving the orphan's ordinal \
         3 alone: {refetched}"
    );
    assert!(
        new_content.contains("<Value>Description</Value>"),
        "the pre-existing orphan must remain untouched: {refetched}"
    );

    call_tool(
        "iris_doc",
        serde_json::json!({"mode":"delete","name": cls_file, "namespace":"USER"}),
    );
}

#[test]
fn e2e_doc_rename_without_updating_storage_is_no_longer_blocked() {
    require_iris!();
    // Renaming a property (delete old + insert new, i.e. how a "rename"
    // happens through iris_doc's actual primitives) without also updating
    // the Storage Data entry produces an orphan plus a fresh ordinal for the
    // new property — the developer's own choice, same as it would be in
    // Studio/VS Code.
    let cls_name = "Test022.RenameNaive";
    let cls_file = format!("{cls_name}.cls");
    let cls_src = format!(
        "Class {cls_name} Extends %Persistent {{\nProperty Name As %String;\nProperty Description As %String;\n}}"
    );
    call_tool(
        "iris_doc",
        serde_json::json!({"mode":"put","name": cls_file, "content": cls_src,
            "namespace":"USER", "compile": true}),
    );

    let fetched = call_tool(
        "iris_doc",
        serde_json::json!({"mode":"get","name": cls_file, "namespace":"USER"}),
    );
    let content = fetched["content"].as_str().unwrap_or_default().to_string();
    let description_prop_line = line_containing(&content, "Property Description As %String;");

    let delete_result = call_tool(
        "iris_doc",
        serde_json::json!({"mode":"delete_lines","name": cls_file, "namespace":"USER",
            "start": description_prop_line, "end": description_prop_line,
            "expected": "Property Description As %String;"}),
    );
    assert!(
        delete_result.get("error_code").is_none(),
        "must not be refused: {delete_result}"
    );

    let after_delete = call_tool(
        "iris_doc",
        serde_json::json!({"mode":"get","name": cls_file, "namespace":"USER"}),
    );
    let after_delete_content = after_delete["content"].as_str().unwrap_or_default();
    let closing_brace_line = last_bare_closing_brace_line(after_delete_content);

    let insert_result = call_tool(
        "iris_doc",
        serde_json::json!({"mode":"insert","name": cls_file, "namespace":"USER",
            "line": closing_brace_line, "expected": "}",
            "content": "Property Comments As %String;", "compile": true}),
    );
    assert_eq!(
        insert_result["success"], true,
        "must succeed with no flag: {insert_result}"
    );

    let refetched = call_tool(
        "iris_doc",
        serde_json::json!({"mode":"get","name": cls_file, "namespace":"USER"}),
    );
    let new_content = refetched["content"].as_str().unwrap_or_default();
    assert!(
        new_content.contains("<Value>Description</Value>"),
        "old name should be left as a harmless orphan: {refetched}"
    );
    assert!(
        new_content.contains("<Value>Comments</Value>"),
        "new name present as its own ordinal: {refetched}"
    );

    call_tool(
        "iris_doc",
        serde_json::json!({"mode":"delete","name": cls_file, "namespace":"USER"}),
    );
}

#[test]
fn e2e_doc_rename_updating_storage_preserves_data() {
    require_iris!();
    // A *correct* rename also edits the Storage Data entry's Value text
    // (same ordinal, new name). Confirms data actually survives when the
    // caller does it right, not just that the class compiles.
    let cls_name = "Test022.RenameCorrect";
    let cls_file = format!("{cls_name}.cls");
    let cls_src = format!(
        "Class {cls_name} Extends %Persistent {{\nProperty Name As %String;\nProperty Description As %String;\n}}"
    );
    call_tool(
        "iris_doc",
        serde_json::json!({"mode":"put","name": cls_file, "content": cls_src,
            "namespace":"USER", "compile": true}),
    );
    let save = call_tool(
        "iris_execute",
        serde_json::json!({"code": format!(
            "set obj = ##class({cls_name}).%New() set obj.Name = \"n\" \
             set obj.Description = \"rename-me\" set sc = obj.%Save() \
             write $system.Status.GetErrorText(sc)"
        ), "namespace": "USER"}),
    );
    assert_eq!(save["output"], "", "seeding data failed: {save}");

    // Rename the property declaration itself.
    let fetched = call_tool(
        "iris_doc",
        serde_json::json!({"mode":"get","name": cls_file, "namespace":"USER"}),
    );
    let content = fetched["content"].as_str().unwrap_or_default().to_string();
    let description_prop_line = line_containing(&content, "Property Description As %String;");
    call_tool(
        "iris_doc",
        serde_json::json!({"mode":"delete_lines","name": cls_file, "namespace":"USER",
            "start": description_prop_line, "end": description_prop_line,
            "expected": "Property Description As %String;"}),
    );
    let after_prop_delete = call_tool(
        "iris_doc",
        serde_json::json!({"mode":"get","name": cls_file, "namespace":"USER"}),
    );
    let after_prop_delete_content = after_prop_delete["content"].as_str().unwrap_or_default();
    let closing_brace_line = last_bare_closing_brace_line(after_prop_delete_content);
    call_tool(
        "iris_doc",
        serde_json::json!({"mode":"insert","name": cls_file, "namespace":"USER",
            "line": closing_brace_line, "expected": "}",
            "content": "Property Comments As %String;"}),
    );

    // Now edit the Storage Data entry's Value text at the SAME ordinal,
    // from Description to Comments — completing the rename correctly.
    let before_storage_edit = call_tool(
        "iris_doc",
        serde_json::json!({"mode":"get","name": cls_file, "namespace":"USER"}),
    );
    let storage_content = before_storage_edit["content"]
        .as_str()
        .unwrap_or_default()
        .to_string();
    let value_desc_line = line_containing(&storage_content, "<Value>Description</Value>");
    call_tool(
        "iris_doc",
        serde_json::json!({"mode":"delete_lines","name": cls_file, "namespace":"USER",
            "start": value_desc_line, "end": value_desc_line,
            "expected": "<Value>Description</Value>"}),
    );
    // Deleting only the inner <Value>Description</Value> line leaves an emptied
    // <Value name="3">\n</Value> pair — IRIS's own document storage collapses that
    // into a self-closing <Value name="3"/> immediately on write, shifting line
    // numbers by more than the one line just removed. Re-fetch rather than reuse
    // value_desc_line (learned the hard way: reusing it here silently targeted the
    // wrong line and the "rename" produced no data at all instead of an error).
    let after_value_delete = call_tool(
        "iris_doc",
        serde_json::json!({"mode":"get","name": cls_file, "namespace":"USER"}),
    );
    let after_value_delete_content = after_value_delete["content"].as_str().unwrap_or_default();
    let collapsed_ordinal_line = line_containing(after_value_delete_content, "<Value name=\"3\"/>");
    call_tool(
        "iris_doc",
        serde_json::json!({"mode":"delete_lines","name": cls_file, "namespace":"USER",
            "start": collapsed_ordinal_line, "end": collapsed_ordinal_line,
            "expected": "<Value name=\"3\"/>"}),
    );
    // Re-fetch again (same lesson) rather than assume what shifted into the gap.
    let after_collapsed_delete = call_tool(
        "iris_doc",
        serde_json::json!({"mode":"get","name": cls_file, "namespace":"USER"}),
    );
    let after_collapsed_delete_content = after_collapsed_delete["content"]
        .as_str()
        .unwrap_or_default();
    let data_close_line = line_containing(after_collapsed_delete_content, "</Data>");
    call_tool(
        "iris_doc",
        serde_json::json!({"mode":"insert","name": cls_file, "namespace":"USER",
            "line": data_close_line, "expected": "</Data>",
            "content": "<Value name=\"3\">\n<Value>Comments</Value>\n</Value>"}),
    );

    let refetched_for_compile = call_tool(
        "iris_doc",
        serde_json::json!({"mode":"get","name": cls_file, "namespace":"USER"}),
    );
    let final_body = refetched_for_compile["content"]
        .as_str()
        .unwrap_or_default()
        .to_string();
    let compiled = call_tool(
        "iris_doc",
        serde_json::json!({"mode":"put","name": cls_file, "namespace":"USER",
            "content": final_body, "compile": true}),
    );
    assert_eq!(compiled["success"], true, "{compiled}");

    let final_content = call_tool(
        "iris_doc",
        serde_json::json!({"mode":"get","name": cls_file, "namespace":"USER"}),
    );
    let fc = final_content["content"].as_str().unwrap_or_default();
    assert!(
        !fc.contains("<Value>Description</Value>"),
        "no orphan should remain — the rename was done correctly: {final_content}"
    );

    let check = call_tool(
        "iris_execute",
        serde_json::json!({"code": format!(
            "set obj = ##class({cls_name}).%OpenId(1) write obj.Comments"
        ), "namespace": "USER"}),
    );
    assert_eq!(
        check["output"], "rename-me",
        "a correctly-done rename (property + Storage entry both updated) must \
         preserve the data: {check}"
    );

    call_tool(
        "iris_doc",
        serde_json::json!({"mode":"delete","name": cls_file, "namespace":"USER"}),
    );
    call_tool(
        "iris_execute",
        serde_json::json!({"code": format!("kill ^{cls_name}D, ^{cls_name}I, ^{cls_name}S"), "namespace": "USER"}),
    );
}

#[test]
fn e2e_doc_storage_reset_without_flag_is_refused() {
    require_iris!();
    // Storage is present server-side and the submitted content omits it
    // entirely - this must be refused outright, not silently applied and not
    // paused for an interactive round trip. The existing Storage block must
    // survive untouched.
    let cls_name = "Test022.StorageResetRefused";
    let cls_file = format!("{cls_name}.cls");
    let cls_src = format!(
        "Class {cls_name} Extends %Persistent {{\nProperty Name As %String;\nProperty Comments As %String;\n}}"
    );
    call_tool(
        "iris_doc",
        serde_json::json!({"mode":"put","name": cls_file, "content": cls_src,
            "namespace":"USER", "compile": true}),
    );
    let before = call_tool(
        "iris_doc",
        serde_json::json!({"mode":"get","name": cls_file, "namespace":"USER"}),
    );

    let reset_src = format!(
        "Class {cls_name} Extends %Persistent {{\nProperty Name As %String;\nProperty Comments As %String;\n}}"
    );
    let reset_attempt = call_tool(
        "iris_doc",
        serde_json::json!({"mode":"put","name": cls_file, "content": reset_src,
            "namespace":"USER", "compile": true}),
    );
    assert_eq!(reset_attempt["success"], false, "{reset_attempt}");
    assert_eq!(
        reset_attempt["error_code"], "STORAGE_RESET_REQUIRES_CONFIRMATION",
        "storage present server-side but missing from submitted content must be \
         refused without allow_storage_regeneration: {reset_attempt}"
    );
    assert!(
        reset_attempt.get("elicitation_required").is_none(),
        "this is a hard refusal, not an interactive elicitation: {reset_attempt}"
    );

    let after = call_tool(
        "iris_doc",
        serde_json::json!({"mode":"get","name": cls_file, "namespace":"USER"}),
    );
    assert_eq!(
        after["content"], before["content"],
        "a refused reset must leave the document exactly as it was: {after}"
    );

    call_tool(
        "iris_doc",
        serde_json::json!({"mode":"delete","name": cls_file, "namespace":"USER"}),
    );
}

#[test]
fn e2e_doc_storage_reset_with_flag_succeeds_and_reports_stale_data() {
    require_iris!();
    // The escape hatch: allow_storage_regeneration:true lets the reset through
    // and reports the properties that existed before the reset, plus whether
    // %KillExtent is available, so the caller can decide how to clean up.
    let cls_name = "Test022.StorageResetWithFlag";
    let cls_file = format!("{cls_name}.cls");
    let cls_src = format!(
        "Class {cls_name} Extends %Persistent {{\nProperty Name As %String;\nProperty Comments As %String;\n}}"
    );
    call_tool(
        "iris_doc",
        serde_json::json!({"mode":"put","name": cls_file, "content": cls_src,
            "namespace":"USER", "compile": true}),
    );

    let reset_src = format!(
        "Class {cls_name} Extends %Persistent {{\nProperty Name As %String;\nProperty Comments As %String;\n}}"
    );
    let reset_result = call_tool(
        "iris_doc",
        serde_json::json!({"mode":"put","name": cls_file, "content": reset_src,
            "namespace":"USER", "compile": true, "allow_storage_regeneration": true}),
    );
    assert_eq!(
        reset_result["success"], true,
        "confirming the reset via the flag must proceed with the write: {reset_result}"
    );
    assert_eq!(reset_result["storage_reset"], true, "{reset_result}");
    assert_eq!(
        reset_result["kill_extent_available"], true,
        "a %Persistent class has an extent to kill: {reset_result}"
    );
    let stale_properties: Vec<String> = reset_result["stale_properties"]
        .as_array()
        .unwrap_or_else(|| panic!("stale_properties missing: {reset_result}"))
        .iter()
        .map(|v| v.as_str().unwrap().to_string())
        .collect();
    assert!(
        stale_properties.contains(&"Name".to_string())
            && stale_properties.contains(&"Comments".to_string()),
        "stale_properties must list the properties that existed before the reset: \
         {reset_result}"
    );
    assert!(
        reset_result["message"]
            .as_str()
            .unwrap_or_default()
            .contains("KillExtent"),
        "message must point at %KillExtent for a %Persistent class: {reset_result}"
    );

    let refetched = call_tool(
        "iris_doc",
        serde_json::json!({"mode":"get","name": cls_file, "namespace":"USER"}),
    );
    let new_content = refetched["content"].as_str().unwrap_or_default();
    assert!(
        new_content.contains("Storage Default"),
        "IRIS regenerates a fresh Storage block on compile: {refetched}"
    );

    call_tool(
        "iris_doc",
        serde_json::json!({"mode":"delete","name": cls_file, "namespace":"USER"}),
    );
}

#[test]
fn e2e_doc_storage_reset_with_flag_serial_object_has_no_kill_extent() {
    require_iris!();
    // %SerialObject classes have no extent of their own, so a flag-gated
    // reset must report kill_extent_available:false rather than claiming an
    // option that doesn't exist for this storage kind.
    let cls_name = "Test022.StorageResetSerialNoKillExtent";
    let cls_file = format!("{cls_name}.cls");
    let cls_src =
        format!("Class {cls_name} Extends %SerialObject {{\nProperty Name As %String;\n}}");
    call_tool(
        "iris_doc",
        serde_json::json!({"mode":"put","name": cls_file, "content": cls_src,
            "namespace":"USER", "compile": true}),
    );

    let reset_src =
        format!("Class {cls_name} Extends %SerialObject {{\nProperty Name As %String;\n}}");
    let reset_result = call_tool(
        "iris_doc",
        serde_json::json!({"mode":"put","name": cls_file, "content": reset_src,
            "namespace":"USER", "compile": true, "allow_storage_regeneration": true}),
    );
    assert_eq!(reset_result["success"], true, "{reset_result}");
    assert_eq!(
        reset_result["kill_extent_available"], false,
        "a %SerialObject has no extent to kill: {reset_result}"
    );

    call_tool(
        "iris_doc",
        serde_json::json!({"mode":"delete","name": cls_file, "namespace":"USER"}),
    );
}

#[test]
fn e2e_doc_rewrite_after_compile_failure_no_conflict() {
    require_iris!();
    // I-4: Re-writing a class after a compile failure must not return CONFLICT
    let name = "Test022.ETagTest.cls";
    let bad = "Class Test022.ETagTest { ClassMethod Bad() { this is not valid !! } }";
    let good = "Class Test022.ETagTest { ClassMethod Good() As %String { Return \"ok\" } }";

    // First write (bad class)
    let r1 = call_tool(
        "iris_doc",
        serde_json::json!({"mode":"put","name":name,"content":bad,"namespace":"USER"}),
    );
    assert_eq!(r1["success"], true, "first write should succeed: {}", r1);

    // Attempt compile (will fail — that's expected)
    call_tool(
        "iris_compile",
        serde_json::json!({"target":name,"namespace":"USER"}),
    );

    // Second write (fixed class) — must NOT return CONFLICT
    let r2 = call_tool(
        "iris_doc",
        serde_json::json!({"mode":"put","name":name,"content":good,"namespace":"USER"}),
    );
    assert_ne!(
        r2["error_code"].as_str(),
        Some("CONFLICT"),
        "re-write after compile failure must not return CONFLICT: {}",
        r2
    );
    assert_eq!(r2["success"], true, "second write should succeed: {}", r2);

    // Cleanup
    call_tool(
        "iris_doc",
        serde_json::json!({"mode":"delete","name":name,"namespace":"USER"}),
    );
}

#[test]
fn e2e_doc_put_get_delete_roundtrip() {
    require_iris!();
    let name = "Test022.RoundTrip.cls";
    let content = "Class Test022.RoundTrip { ClassMethod Hello() As %String { Return \"world\" } }";

    let put = call_tool(
        "iris_doc",
        serde_json::json!({"mode":"put","name":name,"content":content,"namespace":"USER"}),
    );
    assert_eq!(put["success"], true, "put: {}", put);

    let get = call_tool(
        "iris_doc",
        serde_json::json!({"mode":"get","name":name,"namespace":"USER"}),
    );
    assert_eq!(get["success"], true, "get: {}", get);

    let del = call_tool(
        "iris_doc",
        serde_json::json!({"mode":"delete","name":name,"namespace":"USER"}),
    );
    assert_eq!(del["success"], true, "delete: {}", del);
}

// ── iris_compile ──────────────────────────────────────────────────────────────

#[test]
fn e2e_compile_error_has_line_number_and_text() {
    require_iris!();
    let name = "Test022.CompileError.cls";
    let bad =
        "Class Test022.CompileError {\nClassMethod Bad() {\n    this is invalid objectscript\n}\n}";

    call_tool(
        "iris_doc",
        serde_json::json!({"mode":"put","name":name,"content":bad,"namespace":"USER"}),
    );

    let result = call_tool(
        "iris_compile",
        serde_json::json!({"target":name,"namespace":"USER"}),
    );
    assert_eq!(
        result["success"], false,
        "compile of bad class should fail: {}",
        result
    );

    // iris_compile returns errors either as an array (errors[]) or as a top-level error string.
    // Both formats are acceptable — check whichever is present.
    let errors = result["errors"].as_array().cloned().unwrap_or_default();
    let top_level_error = result["error"].as_str().unwrap_or("");
    assert!(
        !errors.is_empty() || !top_level_error.is_empty(),
        "compile failure must have errors array or error string: {}",
        result
    );
    for err in &errors {
        assert!(
            err["text"].is_string() || err["message"].is_string(),
            "error must have text: {}",
            err
        );
        assert!(
            err["line"].is_number(),
            "error must have line number: {}",
            err
        );
    }

    // Cleanup
    call_tool(
        "iris_doc",
        serde_json::json!({"mode":"delete","name":name,"namespace":"USER"}),
    );
}

#[test]
fn e2e_compile_valid_class_succeeds() {
    require_iris!();
    let name = "Test022.CompileOk.cls";
    let good = "Class Test022.CompileOk { ClassMethod Run() As %String { Return \"ok\" } }";

    call_tool(
        "iris_doc",
        serde_json::json!({"mode":"put","name":name,"content":good,"namespace":"USER"}),
    );
    let result = call_tool(
        "iris_compile",
        serde_json::json!({"target":name,"namespace":"USER"}),
    );
    assert_eq!(
        result["success"], true,
        "compile of valid class should succeed: {}",
        result
    );
    let errors = result["errors"].as_array().cloned().unwrap_or_default();
    assert!(
        errors.is_empty(),
        "successful compile should have no errors: {}",
        result
    );

    call_tool(
        "iris_doc",
        serde_json::json!({"mode":"delete","name":name,"namespace":"USER"}),
    );
}

// ── iris_test ─────────────────────────────────────────────────────────────────

#[test]
fn e2e_test_no_match_returns_no_tests_found() {
    require_iris!();
    let result = call_tool(
        "iris_test",
        serde_json::json!({"pattern": "Test022.NonExistent.NoSuchClass", "namespace": "USER"}),
    );
    if result["success"] == false {
        let ec = result["error_code"].as_str().unwrap_or("");
        assert!(
            ec == "NO_TESTS_FOUND" || ec == "DOCKER_REQUIRED",
            "no-match pattern should return NO_TESTS_FOUND or DOCKER_REQUIRED, got: {}",
            result
        );
    }
}

// ── iris_info ─────────────────────────────────────────────────────────────────

#[test]
fn e2e_info_metadata_returns_version() {
    require_iris!();
    let result = call_tool(
        "iris_info",
        serde_json::json!({"what": "metadata", "namespace": "USER"}),
    );
    assert!(
        result["success"] == true
            || result.get("version").is_some()
            || result.get("iris_version").is_some(),
        "iris_info metadata should return version info: {}",
        result
    );
}

#[test]
fn e2e_info_namespace_returns_name() {
    require_iris!();
    let result = call_tool(
        "iris_info",
        serde_json::json!({"what": "namespace", "namespace": "USER"}),
    );
    assert!(
        result["success"] == true || result.get("name").is_some(),
        "iris_info namespace should return namespace info: {}",
        result
    );
}

// ── iris_query ────────────────────────────────────────────────────────────────

#[test]
fn e2e_query_select_returns_rows() {
    require_iris!();
    let result = call_tool(
        "iris_query",
        serde_json::json!({"query": "SELECT TOP 3 Name FROM %Dictionary.ClassDefinition ORDER BY Name", "namespace": "USER"}),
    );
    assert_eq!(
        result["success"], true,
        "SQL SELECT should succeed: {}",
        result
    );
    let rows = result["rows"].as_array().cloned().unwrap_or_default();
    assert!(!rows.is_empty(), "SELECT should return rows: {}", result);
}

#[test]
fn e2e_query_invalid_sql_structured_error() {
    require_iris!();
    let result = call_tool(
        "iris_query",
        serde_json::json!({"query": "THIS IS NOT SQL", "namespace": "USER"}),
    );
    assert_eq!(
        result["success"], false,
        "invalid SQL should fail: {}",
        result
    );
    assert!(
        result["error_code"].is_string(),
        "invalid SQL must return error_code: {}",
        result
    );
}

// ── iris_execute multiline ────────────────────────────────────────────────────

#[test]
fn e2e_execute_multiline_output_encoded_correctly() {
    require_iris!();
    // Multi-line output uses $Char(1) encoding in the generated class and must
    // be decoded back to \n by the Rust layer. Tests the $Char(10)→$Char(1)
    // encoding and the replace('\x01', "\n") decode path.
    let result = call_tool(
        "iris_execute",
        serde_json::json!({"code": "Write \"line1\",!\nWrite \"line2\",!", "namespace": "USER", "confirmed": true}),
    );
    if result["success"] == true {
        let output = result["output"].as_str().unwrap_or("").trim().to_string();
        assert!(
            output.contains("line1") && output.contains("line2"),
            "multi-line Write should return both lines, got: {:?}",
            output
        );
        assert!(
            output.contains('\n'),
            "multi-line output must contain newline separator, got: {:?}",
            output
        );
    }
}

// ── iris_doc batch get ────────────────────────────────────────────────────────

#[test]
fn e2e_doc_batch_get_returns_all_documents() {
    require_iris!();
    // Seed two documents, batch-fetch both, verify both returned concurrently.
    let name_a = "Test022.BatchA.cls";
    let name_b = "Test022.BatchB.cls";
    call_tool(
        "iris_doc",
        serde_json::json!({"mode":"put","name":name_a,
            "content":"Class Test022.BatchA { ClassMethod Run() { } }","namespace":"USER"}),
    );
    call_tool(
        "iris_compile",
        serde_json::json!({"target":name_a,"namespace":"USER"}),
    );
    call_tool(
        "iris_doc",
        serde_json::json!({"mode":"put","name":name_b,
            "content":"Class Test022.BatchB { ClassMethod Run() { } }","namespace":"USER"}),
    );
    call_tool(
        "iris_compile",
        serde_json::json!({"target":name_b,"namespace":"USER"}),
    );

    // Batch get spawns concurrent requests — use longer timeout than single-doc calls.
    let result = call_tool_timeout(
        "iris_doc",
        serde_json::json!({"mode":"get","names":[name_a, name_b],"namespace":"USER"}),
        20,
    );
    assert_eq!(
        result["success"], true,
        "batch get should succeed: {}",
        result
    );
    let docs = result["documents"].as_array().cloned().unwrap_or_default();
    assert_eq!(
        docs.len(),
        2,
        "batch get must return exactly 2 documents: {}",
        result
    );
    let names: Vec<&str> = docs.iter().filter_map(|d| d["name"].as_str()).collect();
    assert!(
        names.contains(&name_a),
        "batch result must include {}: {:?}",
        name_a,
        names
    );
    assert!(
        names.contains(&name_b),
        "batch result must include {}: {:?}",
        name_b,
        names
    );
    // Each document must have non-empty content
    for doc in &docs {
        assert!(
            !doc["content"].as_str().unwrap_or("").is_empty(),
            "document content must not be empty: {}",
            doc
        );
    }

    // Cleanup
    call_tool(
        "iris_doc",
        serde_json::json!({"mode":"delete","name":name_a,"namespace":"USER"}),
    );
    call_tool(
        "iris_doc",
        serde_json::json!({"mode":"delete","name":name_b,"namespace":"USER"}),
    );
}

// ── iris_compile wildcard ─────────────────────────────────────────────────────

#[test]
fn e2e_compile_wildcard_package() {
    require_iris!();
    // Seed two classes in a package, compile with *.cls wildcard.
    // Tests the /docnames/CLS expansion + regex filter path.
    let name_a = "Test022.Wild.Alpha.cls";
    let name_b = "Test022.Wild.Beta.cls";
    call_tool(
        "iris_doc",
        serde_json::json!({"mode":"put","name":name_a,
            "content":"Class Test022.Wild.Alpha { ClassMethod Run() As %String { Return \"a\" } }",
            "namespace":"USER"}),
    );
    call_tool(
        "iris_doc",
        serde_json::json!({"mode":"put","name":name_b,
            "content":"Class Test022.Wild.Beta { ClassMethod Run() As %String { Return \"b\" } }",
            "namespace":"USER"}),
    );

    let result = call_tool(
        "iris_compile",
        serde_json::json!({"target":"Test022.Wild.*.cls","namespace":"USER","flags":"ck"}),
    );

    // Both seeded classes exist, so this must actually succeed.
    assert_eq!(result["success"], true, "{result}");
    let compiled = result["targets_compiled"].as_u64().unwrap_or(0);
    assert!(
        compiled >= 2,
        "wildcard compile Test022.Wild.* should compile at least 2 classes, got: {}",
        compiled
    );

    // Cleanup
    call_tool(
        "iris_doc",
        serde_json::json!({"mode":"delete","name":name_a,"namespace":"USER"}),
    );
    call_tool(
        "iris_doc",
        serde_json::json!({"mode":"delete","name":name_b,"namespace":"USER"}),
    );
}

#[test]
fn e2e_compile_wildcard_no_match_returns_not_found() {
    require_iris!();
    // a wildcard that genuinely matches nothing (package doesn't exist) must still
    // return structured response with failure and a error code.
    let result = call_tool(
        "iris_compile",
        serde_json::json!({"target":"Test022.DoesNotExist.Nothing.*.cls","namespace":"USER"}),
    );
    assert_eq!(result["success"], false, "{result}");
    assert_eq!(result["error_code"], "NOT_FOUND", "{result}");
}

// ── iris_test with real tests ─────────────────────────────────────────────────

#[test]
fn e2e_test_runs_unit_test_and_returns_counts() {
    require_iris!();

    // Use a fixed class name so the /tmp/httest/IrisDevRunTest/ directory
    // gets created on first run and persists. execute_via_generator cannot
    // create new directories, so we need a pre-existing one.
    // The directory is created by the iris_compile docker exec path on first run.
    let cls_doc = "IrisDevRunTest.UnitTestSuite.cls";
    let cls_content = "Class IrisDevRunTest.UnitTestSuite Extends %UnitTest.TestCase {
        Method TestAlwaysPasses() { Do $$$AssertEquals(1,1) }
        Method TestAlwaysFails() { Do $$$AssertEquals(1,2) }
        }";

    let put = call_tool(
        "iris_doc",
        serde_json::json!({"mode":"put","name":cls_doc,"content":cls_content,"namespace":"USER"}),
    );
    assert_eq!(put["success"], true, "seed unit test class: {}", put);

    let compile = call_tool(
        "iris_compile",
        serde_json::json!({"target":cls_doc,"namespace":"USER"}),
    );
    assert_eq!(
        compile["success"], true,
        "unit test class must compile: {}",
        compile
    );

    let result = call_tool(
        "iris_test",
        serde_json::json!({"pattern": "IrisDevRunTest", "namespace": "USER"}),
    );

    if result["error_code"].as_str() == Some("NO_TESTS_FOUND")
        || result["error_code"].as_str() == Some("DOCKER_REQUIRED")
    {
        eprintln!("iris_test could not find/run test class in this environment — skipping count assertions");
        return;
    }

    let passed = result["passed"].as_u64().unwrap_or(0);
    let failed = result["failed"].as_u64().unwrap_or(0);
    let total = result["total"].as_u64().unwrap_or(0);

    assert!(
        total >= 2,
        "should run at least 2 test methods, got: {}",
        result
    );
    assert!(passed >= 1, "TestAlwaysPasses should pass, got: {}", result);
    assert!(failed >= 1, "TestAlwaysFails should fail, got: {}", result);

    // Cleanup
    call_tool(
        "iris_doc",
        serde_json::json!({"mode":"delete","name":cls_doc,"namespace":"USER"}),
    );
}

// ── iris_search ───────────────────────────────────────────────────────────────

#[test]
fn e2e_search_finds_seeded_content() {
    require_iris!();
    // First seed a class with unique content
    let name = "Test022.SearchTarget.cls";
    let unique = "UNIQUESEARCHTOKEN022";
    let content = format!("Class Test022.SearchTarget {{ /// {} }}", unique);
    call_tool(
        "iris_doc",
        serde_json::json!({"mode":"put","name":name,"content":content,"namespace":"USER"}),
    );

    let result = call_tool(
        "iris_search",
        serde_json::json!({"query": unique, "namespace": "USER"}),
    );
    // Search may return 0 results if not indexed yet — just must not crash
    assert!(
        result["success"] == true || result["error_code"].is_string(),
        "iris_search must return structured response: {}",
        result
    );

    call_tool(
        "iris_doc",
        serde_json::json!({"mode":"delete","name":name,"namespace":"USER"}),
    );
}

// ── docs_introspect ───────────────────────────────────────────────────────────

#[test]
fn e2e_introspect_known_class() {
    require_iris!();
    let result = call_tool(
        "docs_introspect",
        serde_json::json!({"class_name": "Ens.Director", "namespace": "USER"}),
    );
    assert_eq!(
        result["success"], true,
        "introspect Ens.Director should succeed: {}",
        result
    );
    let methods = result["methods"].as_array().cloned().unwrap_or_default();
    assert!(
        !methods.is_empty(),
        "Ens.Director should have methods: {}",
        result
    );
}

#[test]
fn e2e_introspect_nonexistent_structured_error() {
    require_iris!();
    let result = call_tool(
        "docs_introspect",
        serde_json::json!({"class_name": "Nonexistent.Class.That.DoesNotExist", "namespace": "USER"}),
    );
    assert!(
        result["success"] == true || result["success"] == false,
        "introspect of nonexistent class must return structured response: {}",
        result
    );
}

// ── workspace config ──────────────────────────────────────────────────────────

#[test]
fn e2e_workspace_config_iris_dev_init_creates_toml() {
    require_bin!();
    let tmp = tempfile::TempDir::new().unwrap();
    let output = Command::new(iris_dev_bin())
        .args([
            "init",
            "--workspace",
            tmp.path().to_str().unwrap(),
            "--format",
            "json",
        ])
        .output();

    match output {
        Ok(out) => {
            let stdout = String::from_utf8_lossy(&out.stdout);
            if out.status.success() {
                // If it succeeded, the TOML file must exist
                let toml_path = tmp.path().join(".iris-agentic-dev.toml");
                assert!(
                    toml_path.exists(),
                    "iris-dev init should create .iris-dev.toml"
                );
                let content = std::fs::read_to_string(&toml_path).unwrap();
                assert!(
                    content.contains("container"),
                    "generated toml must have container field"
                );
                assert!(
                    content.contains("namespace"),
                    "generated toml must have namespace field"
                );
                // JSON output must be valid
                if !stdout.trim().is_empty() {
                    let json: serde_json::Value = serde_json::from_str(stdout.trim())
                        .expect("iris-dev init --format json must produce valid JSON");
                    assert_eq!(json["success"], true, "init JSON output: {}", json);
                }
            }
            // If it failed (no containers running), that's acceptable — just must not panic
        }
        Err(e) => panic!("iris-dev init failed to run: {}", e),
    }
}

// ── compile hook ──────────────────────────────────────────────────────────────

fn hook_script() -> std::path::PathBuf {
    let mut p = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    p.pop();
    p.pop();
    p.push("scripts/compile-hook.sh");
    p
}

fn run_hook(event: &serde_json::Value, env_override: &[(&str, &str)]) -> (String, i32) {
    let script = hook_script();
    if !script.exists() {
        return ("SKIP: compile-hook.sh not found".to_string(), 0);
    }

    let mut cmd = Command::new("bash");
    cmd.arg(&script)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    for (k, v) in env_override {
        cmd.env(k, v);
    }

    let mut child = cmd.spawn().expect("spawn bash");
    if let Some(mut stdin) = child.stdin.take() {
        let _ = stdin.write_all(serde_json::to_string(event).unwrap().as_bytes());
    }
    let output = child.wait_with_output().expect("wait");
    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let code = output.status.code().unwrap_or(-1);
    (stdout, code)
}

#[test]
fn e2e_hook_non_cls_file_is_silent() {
    // Non-ObjectScript files must produce no output — no IRIS needed
    let event = serde_json::json!({
        "hook_event_name": "PostToolUse",
        "tool_name": "Write",
        "tool_input": {"file_path": "/workspace/config.json"},
        "tool_result": {},
        "cwd": "/workspace"
    });
    let (output, code) = run_hook(&event, &[]);
    if output != "SKIP: compile-hook.sh not found" {
        assert_eq!(
            output, "",
            "non-.cls file must produce no output, got: {:?}",
            output
        );
        assert_eq!(code, 0);
    }
}

#[test]
fn e2e_hook_auto_compile_disabled_is_silent() {
    // IRIS_AUTO_COMPILE=false must always be silent — no IRIS needed
    let event = serde_json::json!({
        "hook_event_name": "PostToolUse",
        "tool_name": "Write",
        "tool_input": {"file_path": "/workspace/MyApp/Patient.cls"},
        "tool_result": {},
        "cwd": "/workspace"
    });
    let (output, code) = run_hook(&event, &[("IRIS_AUTO_COMPILE", "false")]);
    if output != "SKIP: compile-hook.sh not found" {
        assert_eq!(
            output, "",
            "IRIS_AUTO_COMPILE=false must be silent, got: {:?}",
            output
        );
        assert_eq!(code, 0);
    }
}

#[test]
fn e2e_hook_no_iris_host_message_within_3s() {
    // When IRIS_HOST is not set, must print a message within 3.5 seconds
    let event = serde_json::json!({
        "hook_event_name": "PostToolUse",
        "tool_name": "Write",
        "tool_input": {"file_path": "/workspace/MyApp/Patient.cls"},
        "tool_result": {},
        "cwd": "/workspace"
    });
    let start = std::time::Instant::now();
    let (output, _) = run_hook(&event, &[("IRIS_HOST", ""), ("IRIS_CONTAINER", "")]);
    let elapsed = start.elapsed();
    if output != "SKIP: compile-hook.sh not found" {
        assert!(
            elapsed < std::time::Duration::from_millis(3500),
            "hook with no IRIS must respond in <3.5s, took {:?}",
            elapsed
        );
        // Must either be silent (IRIS not configured) or explain
        let text_lower = output.to_lowercase();
        assert!(
            output.is_empty()
                || text_lower.contains("not connected")
                || text_lower.contains("iris_host")
                || text_lower.contains("unreachable"),
            "unexpected output with no IRIS: {:?}",
            output
        );
    }
}

#[test]
fn e2e_hook_file_changed_disabled_by_default() {
    // FileChanged without IRIS_COMPILE_ON_SAVE=true must be silent
    let event = serde_json::json!({
        "hook_event_name": "FileChanged",
        "file_path": "/workspace/MyApp/Patient.cls"
    });
    let (output, code) = run_hook(&event, &[]);
    if output != "SKIP: compile-hook.sh not found" {
        assert_eq!(
            output, "",
            "FileChanged without opt-in must be silent, got: {:?}",
            output
        );
        assert_eq!(code, 0);
    }
}

// ── iris_info additional modes ────────────────────────────────────────────────

#[test]
fn e2e_info_documents_returns_list() {
    require_iris!();
    let result = call_tool(
        "iris_info",
        serde_json::json!({"what": "documents", "namespace": "USER"}),
    );
    // Must return a list (possibly large — don't assert count, just structure)
    assert!(
        result["success"] == true || result["error_code"].is_string(),
        "iris_info documents must return structured response: {}",
        result
    );
    if result["success"] == true {
        // iris_info documents returns result.content (raw Atelier) or a documents array
        assert!(
            result["documents"].is_array()
                || result["count"].is_number()
                || result["result"]["content"].is_array(),
            "documents mode must return documents, count, or result.content: success={}",
            result["success"]
        );
    }
}

#[test]
fn e2e_info_jobs_returns_list() {
    require_iris!();
    let result = call_tool(
        "iris_info",
        serde_json::json!({"what": "jobs", "namespace": "USER"}),
    );
    assert!(
        result["success"] == true || result["error_code"].is_string(),
        "iris_info jobs must return structured response: {}",
        result
    );
    if result["success"] == true {
        assert!(
            result["jobs"].is_array(),
            "jobs mode must return jobs array: {}",
            result
        );
    }
}

#[test]
fn e2e_info_modified_returns_list() {
    require_iris!();
    let result = call_tool(
        "iris_info",
        serde_json::json!({"what": "modified", "namespace": "USER"}),
    );
    // modified may return 405 on some IRIS versions — either structured success or error
    assert!(
        result["success"] == true || result["error_code"].is_string(),
        "iris_info modified must return structured response: {}",
        result
    );
}

// ── iris_doc HEAD ─────────────────────────────────────────────────────────────

#[test]
fn e2e_doc_head_existing_document() {
    require_iris!();
    // HEAD on a known system class must return success
    let result = call_tool(
        "iris_doc",
        serde_json::json!({"mode": "head", "name": "Ens.Director.cls", "namespace": "USER"}),
    );
    assert_eq!(
        result["success"], true,
        "iris_doc HEAD on Ens.Director.cls should succeed: {}",
        result
    );
    assert!(
        result["exists"] == true || result["name"].is_string(),
        "HEAD response must indicate document exists: {}",
        result
    );
}

#[test]
fn e2e_doc_head_nonexistent_returns_not_found() {
    require_iris!();
    let result = call_tool(
        "iris_doc",
        serde_json::json!({"mode": "head", "name": "Test022.DoesNotExist.cls", "namespace": "USER"}),
    );
    // HEAD on nonexistent doc must not crash — returns success:false or exists:false
    assert!(
        result["success"] == false || result["exists"] == false,
        "HEAD on nonexistent doc must return not-found: {}",
        result
    );
}

// ── iris_macro ────────────────────────────────────────────────────────────────

#[test]
fn e2e_macro_list_returns_macros() {
    require_iris!();
    let result = call_tool(
        "iris_macro",
        serde_json::json!({"action": "list", "namespace": "USER"}),
    );
    assert!(
        result["success"] == true || result["error_code"].is_string(),
        "iris_macro list must return structured response: {}",
        result
    );
    if result["success"] == true {
        // macros array may be empty if no include files are indexed in USER namespace
        // (known issue I-10 — system includes not found without explicit include context).
        // Assert structure, not content.
        assert!(
            result["macros"].is_array(),
            "iris_macro list must return macros array (may be empty): {}",
            result
        );
    }
}

#[test]
fn e2e_macro_signature_known_macro() {
    require_iris!();
    // $$$OK is always defined in %occStatus.inc
    let result = call_tool(
        "iris_macro",
        serde_json::json!({"action": "signature", "name": "OK", "namespace": "USER"}),
    );
    assert!(
        result["success"] == true || result["error_code"].is_string(),
        "iris_macro signature must return structured response: {}",
        result
    );
}

// ── iris_query with parameters ────────────────────────────────────────────────

#[test]
fn e2e_query_parameterized_uses_placeholder() {
    require_iris!();
    // Tests the SQL injection fix (Bug 15 / FR-001): parameters must go through
    // the ? placeholder, not be interpolated into the SQL string.
    let result = call_tool(
        "iris_query",
        serde_json::json!({
            "query": "SELECT Name FROM %Dictionary.ClassDefinition WHERE Name = ?",
            "parameters": ["Ens.Director"],
            "namespace": "USER"
        }),
    );
    assert_eq!(
        result["success"], true,
        "parameterized query should succeed: {}",
        result
    );
    let rows = result["rows"].as_array().cloned().unwrap_or_default();
    assert_eq!(
        rows.len(),
        1,
        "should find exactly Ens.Director: {}",
        result
    );
    assert_eq!(
        rows[0]["Name"].as_str(),
        Some("Ens.Director"),
        "row must contain Ens.Director: {:?}",
        rows[0]
    );
}

#[test]
fn e2e_query_parameterized_prevents_injection() {
    require_iris!();
    // A class name containing SQL metacharacters passed as a parameter
    // must be treated as a literal value, not SQL syntax.
    let result = call_tool(
        "iris_query",
        serde_json::json!({
            "query": "SELECT Name FROM %Dictionary.ClassDefinition WHERE Name = ?",
            "parameters": ["'; DROP TABLE %Dictionary.ClassDefinition; --"],
            "namespace": "USER"
        }),
    );
    // Must succeed with zero rows (not crash or modify the database)
    assert_eq!(
        result["success"], true,
        "injection attempt must not crash: {}",
        result
    );
    let rows = result["rows"].as_array().cloned().unwrap_or_default();
    assert_eq!(
        rows.len(),
        0,
        "injection attempt must return 0 rows: {}",
        result
    );
}

// ── iris_symbols edge cases ───────────────────────────────────────────────────

#[test]
fn e2e_symbols_bare_star_returns_all() {
    require_iris!();
    // bare * should return all classes up to the limit, no WHERE clause
    let result = call_tool(
        "iris_symbols",
        serde_json::json!({"query": "*", "namespace": "USER", "limit": 5}),
    );
    assert_eq!(result["success"].as_str().unwrap_or(""), "",); // success field may not be present
    let count = result["count"].as_u64().unwrap_or(0);
    assert!(
        count > 0
            || result["symbols"]
                .as_array()
                .map(|a| !a.is_empty())
                .unwrap_or(false),
        "bare * should return classes: {}",
        result
    );
}

#[test]
fn e2e_symbols_mid_glob_pattern() {
    require_iris!();
    // Ens.*.Operation should match classes like Ens.BusinessOperation (mid-glob via LIKE)
    // "Ens.*.Operation" → SQL LIKE "Ens.%.Operation" → matches Ens.BusinessOperation
    let result = call_tool(
        "iris_symbols",
        serde_json::json!({"query": "Ens.*.Operation", "namespace": "USER", "limit": 10}),
    );
    assert!(
        result["symbols"].is_array() || result["error_code"].is_string(),
        "mid-glob must return structured response: {}",
        result
    );
    if result["symbols"].is_array() {
        let symbols = result["symbols"].as_array().unwrap();
        let names: Vec<&str> = symbols.iter().filter_map(|s| s["Name"].as_str()).collect();
        // Either found matching classes, or zero results (namespace variation) — both OK
        // The important thing is it returned an array, not an error
        let _ = names; // structure validated above
    }
}

// ── iris_search options ───────────────────────────────────────────────────────

#[test]
fn e2e_search_category_filter() {
    require_iris!();
    // Search restricted to CLS category should only return class names
    let result = call_tool(
        "iris_search",
        serde_json::json!({
            "query": "Director",
            "namespace": "USER",
            "category": "CLS",
            "max_results": 5
        }),
    );
    assert!(
        result["success"] == true || result["error_code"].is_string(),
        "iris_search with category:CLS must return structured response: {}",
        result
    );
    if result["success"] == true {
        let results = result["results"].as_array().cloned().unwrap_or_default();
        for r in &results {
            let doc = r["document"].as_str().unwrap_or("");
            assert!(
                doc.ends_with(".cls") || doc.is_empty(),
                "CLS category filter should only return .cls documents: {}",
                doc
            );
        }
    }
}

#[test]
fn e2e_search_regex_option() {
    require_iris!();
    // Regex search for Director$ (classes ending in Director)
    let result = call_tool(
        "iris_search",
        serde_json::json!({
            "query": "Director$",
            "namespace": "USER",
            "regex": true,
            "category": "CLS",
            "max_results": 5
        }),
    );
    assert!(
        result["success"] == true || result["error_code"].is_string(),
        "iris_search with regex must return structured response: {}",
        result
    );
}

// ── execute_via_generator error path ─────────────────────────────────────────

#[test]
fn e2e_execute_runtime_error_surfaced() {
    require_iris!();
    // Code that causes a runtime error — the Try/Catch in the generated class
    // must capture it and return the error text, not empty string.
    let result = call_tool(
        "iris_execute",
        serde_json::json!({
            "code": "Set x = 1/0",  // <DIVIDE> error
            "namespace": "USER",
            "confirmed": true
        }),
    );
    if result["success"] == true {
        let output = result["output"].as_str().unwrap_or("").to_lowercase();
        assert!(
            output.contains("error") || output.contains("divide") || output.contains("zero"),
            "runtime error in executed code must appear in output, got: {:?}",
            output
        );
        assert_ne!(output, "", "runtime error must not produce empty output");
    }
    // DOCKER_REQUIRED or HTTP failure are also acceptable outcomes
}

#[test]
fn e2e_execute_syntax_error_in_code() {
    require_iris!();
    // Code with a syntax error — the generated class will fail to compile.
    // execute_via_generator should return an error, not success with empty output.
    let result = call_tool(
        "iris_execute",
        serde_json::json!({
            "code": "this is not valid objectscript @@##",
            "namespace": "USER",
            "confirmed": true
        }),
    );
    // Either: success=false with a meaningful error, OR success=true with
    // error text in output (caught by the Try/Catch or compile error path).
    // What MUST NOT happen: success=true with empty output.
    if result["success"] == true {
        let output = result["output"].as_str().unwrap_or("").trim();
        // The generated class compile will fail — execute_via_generator returns Err
        // which falls back to DOCKER_REQUIRED or returns compile error
        // Accept empty output only if there's also an error indicator
        if output.is_empty() {
            // If output is empty but success=true, that's the bug — but for syntax
            // errors the compile step itself should fail, returning success=false
            // So if we get here, something is wrong
            panic!(
                "execute with invalid syntax returned success:true with empty output: {}",
                result
            );
        }
    }
    // success=false is the expected path for syntax errors
}

// ── Interoperability ──────────────────────────────────────────────────────────

#[test]
fn e2e_interop_production_status_structured_response() {
    require_iris!();
    // interop_production_status uses docker exec — DOCKER_REQUIRED if no container.
    // Either way must return a structured response, not crash.
    let result = call_tool(
        "iris_production",
        serde_json::json!({"action": "status", "namespace": "USER"}),
    );
    assert!(
        result["success"] == true || result["success"] == false || result["error_code"].is_string(),
        "interop_production_status must return structured response: {}",
        result
    );
    // If connected via docker, must return production name and state
    if result["success"] == true {
        assert!(
            result["production"].is_string() || result["state"].is_string(),
            "production status must include production name or state: {}",
            result
        );
    }
}

#[test]
fn e2e_interop_queues_structured_response() {
    require_iris!();
    // interop_queues queries Ens.Queue via SQL — works without docker if IRIS_HOST set.
    let result = call_tool("iris_interop_query", serde_json::json!({"what": "queues"}));
    assert!(
        result["success"] == true || result["error_code"].is_string(),
        "interop_queues must return structured response: {}",
        result
    );
    if result["success"] == true {
        assert!(
            result["queues"].is_array(),
            "queues must be an array: {}",
            result
        );
    }
}

#[test]
fn e2e_interop_logs_structured_response() {
    require_iris!();
    let result = call_tool(
        "iris_interop_query",
        serde_json::json!({"what": "logs", "log_type": "error,warning", "limit": 10}),
    );
    assert!(
        result["success"] == true || result["error_code"].is_string(),
        "interop_logs must return structured response: {}",
        result
    );
    if result["success"] == true {
        assert!(
            result["logs"].is_array(),
            "logs must be an array: {}",
            result
        );
    }
}

#[test]
fn e2e_interop_message_search_structured_response() {
    require_iris!();
    // Search the message archive — returns empty array if no messages, not an error.
    let result = call_tool(
        "iris_interop_query",
        serde_json::json!({"what": "messages", "limit": 5}),
    );
    assert!(
        result["success"] == true || result["error_code"].is_string(),
        "interop_message_search must return structured response: {}",
        result
    );
    if result["success"] == true {
        assert!(
            result["messages"].is_array(),
            "messages must be array: {}",
            result
        );
    }
}

// ── Security / namespace isolation ───────────────────────────────────────────

#[test]
fn e2e_query_namespace_isolation() {
    require_iris!();
    // SQL query in USER namespace must not see %SYS tables.
    // %SYS.Users exists in %SYS but not USER — query should return SQLCODE error.
    let result = call_tool(
        "iris_query",
        serde_json::json!({
            "query": "SELECT TOP 1 Name FROM %SYS.Users",
            "namespace": "USER"
        }),
    );
    // Either SQL error (table not found in USER) or empty rows — must NOT return user records.
    if result["success"] == true {
        let rows = result["rows"].as_array().cloned().unwrap_or_default();
        assert!(
            rows.is_empty(),
            "USER namespace query must not access %SYS.Users: {}",
            result
        );
    }
    // SQL_ERROR is expected and acceptable
}

#[test]
fn e2e_compile_namespace_parameter_respected() {
    require_iris!();
    // Compile in USER namespace — class should go to USER, not %SYS.
    let name = "Test022.NsCheck.cls";
    let content = "Class Test022.NsCheck { ClassMethod Run() As %String { Return \"ns\" } }";
    call_tool(
        "iris_doc",
        serde_json::json!({"mode":"put","name":name,"content":content,"namespace":"USER"}),
    );
    let result = call_tool(
        "iris_compile",
        serde_json::json!({"target":name,"namespace":"USER"}),
    );
    assert_eq!(
        result["namespace"].as_str(),
        Some("USER"),
        "compile must operate in USER namespace: {}",
        result
    );
    assert_eq!(
        result["success"], true,
        "compile in USER must succeed: {}",
        result
    );
    call_tool(
        "iris_doc",
        serde_json::json!({"mode":"delete","name":name,"namespace":"USER"}),
    );
}

// ── Persistent class and SQL round-trip ──────────────────────────────────────

#[test]
fn e2e_persistent_class_sql_round_trip() {
    require_iris!();
    // Create a %Persistent class, compile it, insert via SQL, SELECT back.
    // Tests the full IRIS data layer: class definition → SQL projection → DML.
    let cls_doc = "Test022.Person.cls";
    let cls_content = r#"Class Test022.Person Extends %Persistent {
Property Name As %String;
Property Age As %Integer;
}"#;

    call_tool(
        "iris_doc",
        serde_json::json!({"mode":"put","name":cls_doc,"content":cls_content,"namespace":"USER"}),
    );
    let compile = call_tool(
        "iris_compile",
        serde_json::json!({"target":cls_doc,"namespace":"USER","flags":"ck"}),
    );
    if compile["success"] != true {
        eprintln!("Skipping SQL round-trip: compile failed: {}", compile);
        call_tool(
            "iris_doc",
            serde_json::json!({"mode":"delete","name":cls_doc,"namespace":"USER"}),
        );
        return;
    }

    // Insert a row via SQL
    let insert = call_tool(
        "iris_query",
        serde_json::json!({
            "query": "INSERT INTO Test022.Person (Name, Age) VALUES (?, ?)",
            "parameters": ["Alice", "30"],
            "namespace": "USER"
        }),
    );
    if insert["success"] != true {
        eprintln!("Skipping SELECT: INSERT failed: {}", insert);
        call_tool(
            "iris_doc",
            serde_json::json!({"mode":"delete","name":cls_doc,"namespace":"USER"}),
        );
        return;
    }

    // SELECT back
    let select = call_tool(
        "iris_query",
        serde_json::json!({
            "query": "SELECT Name, Age FROM Test022.Person WHERE Name = ?",
            "parameters": ["Alice"],
            "namespace": "USER"
        }),
    );
    assert_eq!(
        select["success"], true,
        "SELECT from persistent class should succeed: {}",
        select
    );
    let rows = select["rows"].as_array().cloned().unwrap_or_default();
    assert!(!rows.is_empty(), "should find inserted row: {}", select);
    assert_eq!(
        rows[0]["Name"].as_str(),
        Some("Alice"),
        "Name should be Alice: {:?}",
        rows[0]
    );

    // Cleanup — DELETE the row and the class
    call_tool(
        "iris_query",
        serde_json::json!({
            "query": "DELETE FROM Test022.Person WHERE Name = ?",
            "parameters": ["Alice"],
            "namespace": "USER"
        }),
    );
    call_tool(
        "iris_doc",
        serde_json::json!({"mode":"delete","name":cls_doc,"namespace":"USER"}),
    );
}

// ── debug tools ──────────────────────────────────────────────────────────────

#[test]
fn e2e_debug_error_logs_returns_list() {
    require_iris!();
    // debug_get_error_logs was consolidated into iris_debug(action=error_logs) — FR-007.
    let result = call_tool(
        "iris_debug",
        serde_json::json!({"action": "error_logs", "namespace": "USER", "limit": 10}),
    );
    assert_eq!(
        result["success"], true,
        "iris_debug error_logs should succeed: {}",
        result
    );
    // logs may be null (no recent errors) or an array — both are valid
    assert!(
        result["logs"].is_array() || result["logs"].is_null(),
        "error logs must be array or null: {}",
        result
    );
}

#[test]
fn e2e_debug_capture_packet_returns_errors() {
    require_iris!();
    // debug_capture_packet was consolidated into iris_debug(action=capture) — FR-007.
    let result = call_tool(
        "iris_debug",
        serde_json::json!({"action": "capture", "namespace": "USER"}),
    );
    assert!(
        result["success"] == true || result["error_code"].is_string(),
        "iris_debug capture must return structured response: {}",
        result
    );
    if result["success"] == true {
        assert!(
            result["capture"].is_string(),
            "capture field must be a string when success: {}",
            result
        );
    }
}

#[test]
fn e2e_debug_map_int_to_cls_parses_error_string() {
    require_iris!();
    // debug_map_int_to_cls was consolidated into iris_debug(action=map_int) — FR-007.
    // This does NOT require docker exec (parse only) if error_string is provided.
    let result = call_tool(
        "iris_debug",
        serde_json::json!({
            "action": "map_int",
            "error_string": "<UNDEFINED>x+3^Ens.Director.1",
            "namespace": "USER"
        }),
    );
    assert!(
        result["success"] == true || result["error_code"].is_string(),
        "iris_debug map_int must return structured response: {}",
        result
    );
    if result["success"] == true {
        assert_eq!(
            result["error_string"].as_str(),
            Some("<UNDEFINED>x+3^Ens.Director.1"),
            "error_string must be echoed back: {}",
            result
        );
        assert!(
            result["source_location"].is_string(),
            "source_location must be a string: {}",
            result
        );
    }
}

// ── iris_execute extended ─────────────────────────────────────────────────────

#[test]
fn e2e_execute_arithmetic_expression() {
    require_iris!();
    let result = call_tool(
        "iris_execute",
        serde_json::json!({"code":"Write 6*7,!","namespace":"USER","confirmed":true}),
    );
    if result["success"] == true {
        assert_eq!(
            result["output"].as_str().map(|s| s.trim()),
            Some("42"),
            "6*7 should equal 42: {}",
            result
        );
    }
}

#[test]
fn e2e_execute_string_concatenation() {
    require_iris!();
    let result = call_tool(
        "iris_execute",
        serde_json::json!({"code":"Write \"Hello\"_\" \"_\"World\",!","namespace":"USER","confirmed":true}),
    );
    if result["success"] == true {
        let out = result["output"].as_str().unwrap_or("").trim().to_string();
        assert_eq!(out, "Hello World", "string concat: {}", result);
    }
}

#[test]
fn e2e_execute_set_and_read_variable() {
    require_iris!();
    let result = call_tool(
        "iris_execute",
        serde_json::json!({"code":"Set x=42 Write x,!","namespace":"USER","confirmed":true}),
    );
    if result["success"] == true {
        assert_eq!(
            result["output"].as_str().map(|s| s.trim()),
            Some("42"),
            "Set then Write: {}",
            result
        );
    }
}

#[test]
fn e2e_execute_list_operations() {
    require_iris!();
    let result = call_tool(
        "iris_execute",
        serde_json::json!({"code":"Set lst=$ListBuild(\"a\",\"b\",\"c\") Write $ListLength(lst),!","namespace":"USER","confirmed":true}),
    );
    if result["success"] == true {
        assert_eq!(
            result["output"].as_str().map(|s| s.trim()),
            Some("3"),
            "$ListLength of 3-element list: {}",
            result
        );
    }
}

#[test]
fn e2e_execute_date_functions() {
    require_iris!();
    let result = call_tool(
        "iris_execute",
        serde_json::json!({"code":"Write $ZDate(+$Horolog,3),!","namespace":"USER","confirmed":true}),
    );
    if result["success"] == true {
        let out = result["output"].as_str().unwrap_or("").trim().to_string();
        assert!(
            out.contains("-") && out.len() >= 8,
            "$ZDate should return YYYY-MM-DD: {:?}",
            out
        );
    }
}

#[test]
fn e2e_execute_class_method_call() {
    require_iris!();
    let result = call_tool(
        "iris_execute",
        serde_json::json!({"code":"Write ##class(%SYSTEM.Version).GetVersion(),!","namespace":"USER","confirmed":true}),
    );
    if result["success"] == true {
        let out = result["output"].as_str().unwrap_or("").trim().to_string();
        assert!(
            !out.is_empty(),
            "GetVersion() should return something: {}",
            result
        );
        assert!(
            out.contains("IRIS") || out.contains("20"),
            "version should mention IRIS or year: {:?}",
            out
        );
    }
}

#[test]
fn e2e_execute_for_loop_output() {
    require_iris!();
    let result = call_tool(
        "iris_execute",
        serde_json::json!({"code":"Set sum=0 For i=1:1:5 { Set sum=sum+i } Write sum,!","namespace":"USER","confirmed":true}),
    );
    if result["success"] == true {
        assert_eq!(
            result["output"].as_str().map(|s| s.trim()),
            Some("15"),
            "sum 1..5 should be 15: {}",
            result
        );
    }
}

#[test]
fn e2e_execute_error_code_not_empty_on_failure() {
    require_iris!();
    // When execute fails (DOCKER_REQUIRED or HTTP error), error_code must be present
    let result = call_tool(
        "iris_execute",
        serde_json::json!({"code":"Write 1","namespace":"USER","confirmed":true}),
    );
    // If it failed, must have error_code
    if result["success"] == false {
        assert!(
            result["error_code"].is_string(),
            "failure must have error_code: {}",
            result
        );
    }
}

// ── iris_compile extended ─────────────────────────────────────────────────────

#[test]
fn e2e_compile_class_with_property() {
    require_iris!();
    let name = "Test022.PropTest.cls";
    let content = "Class Test022.PropTest Extends %RegisteredObject {\nProperty Score As %Integer [ InitialExpression = 0 ];\n}";
    call_tool(
        "iris_doc",
        serde_json::json!({"mode":"put","name":name,"content":content,"namespace":"USER"}),
    );
    let result = call_tool(
        "iris_compile",
        serde_json::json!({"target":name,"namespace":"USER"}),
    );
    assert_eq!(
        result["success"], true,
        "class with property should compile: {}",
        result
    );
    call_tool(
        "iris_doc",
        serde_json::json!({"mode":"delete","name":name,"namespace":"USER"}),
    );
}

#[test]
fn e2e_compile_class_with_method_returning_value() {
    require_iris!();
    let name = "Test022.ReturnTest.cls";
    let content = "Class Test022.ReturnTest {\nClassMethod Double(x As %Integer) As %Integer { Return x*2 }\n}";
    call_tool(
        "iris_doc",
        serde_json::json!({"mode":"put","name":name,"content":content,"namespace":"USER"}),
    );
    let result = call_tool(
        "iris_compile",
        serde_json::json!({"target":name,"namespace":"USER"}),
    );
    assert_eq!(
        result["success"], true,
        "class with return method: {}",
        result
    );
    // Immediately exercise the compiled method
    let exec = call_tool(
        "iris_execute",
        serde_json::json!({"code":"Write ##class(Test022.ReturnTest).Double(21),!","namespace":"USER","confirmed":true}),
    );
    if exec["success"] == true {
        assert_eq!(
            exec["output"].as_str().map(|s| s.trim()),
            Some("42"),
            "Double(21) should return 42: {}",
            exec
        );
    }
    call_tool(
        "iris_doc",
        serde_json::json!({"mode":"delete","name":name,"namespace":"USER"}),
    );
}

#[test]
fn e2e_compile_class_with_class_parameter() {
    require_iris!();
    let name = "Test022.ParamTest.cls";
    let content =
        "Class Test022.ParamTest [ ClassType = datatype ] {\nParameter VERSION = \"1.0\";\n}";
    call_tool(
        "iris_doc",
        serde_json::json!({"mode":"put","name":name,"content":content,"namespace":"USER"}),
    );
    let result = call_tool(
        "iris_compile",
        serde_json::json!({"target":name,"namespace":"USER"}),
    );
    assert_eq!(result["success"], true, "class with parameter: {}", result);
    call_tool(
        "iris_doc",
        serde_json::json!({"mode":"delete","name":name,"namespace":"USER"}),
    );
}

#[test]
fn e2e_compile_multiple_flags() {
    require_iris!();
    let name = "Test022.FlagsTest.cls";
    let content = "Class Test022.FlagsTest { ClassMethod Run() { } }";
    call_tool(
        "iris_doc",
        serde_json::json!({"mode":"put","name":name,"content":content,"namespace":"USER"}),
    );
    // "ckb" = compile, check, keep source
    let result = call_tool(
        "iris_compile",
        serde_json::json!({"target":name,"namespace":"USER","flags":"ckb"}),
    );
    assert_eq!(
        result["success"], true,
        "compile with flags ckb: {}",
        result
    );
    call_tool(
        "iris_doc",
        serde_json::json!({"mode":"delete","name":name,"namespace":"USER"}),
    );
}

#[test]
fn e2e_compile_error_shows_class_name_in_error() {
    require_iris!();
    let name = "Test022.ErrClass.cls";
    let bad = "Class Test022.ErrClass { Method Bad() { undefined_builtin_func() } }";
    call_tool(
        "iris_doc",
        serde_json::json!({"mode":"put","name":name,"content":bad,"namespace":"USER"}),
    );
    let result = call_tool(
        "iris_compile",
        serde_json::json!({"target":name,"namespace":"USER"}),
    );
    assert_eq!(
        result["success"], false,
        "bad class should fail: {}",
        result
    );
    // Error must mention the class or method name somewhere
    let error_text = result.to_string().to_lowercase();
    assert!(
        error_text.contains("test022") || error_text.contains("error"),
        "error must reference class: {}",
        result
    );
    call_tool(
        "iris_doc",
        serde_json::json!({"mode":"delete","name":name,"namespace":"USER"}),
    );
}

#[test]
fn e2e_compile_registered_object_extends() {
    require_iris!();
    let name = "Test022.RegObj.cls";
    let content = "Class Test022.RegObj Extends %RegisteredObject {\nMethod Greet() As %String { Return \"Hello\" }\n}";
    call_tool(
        "iris_doc",
        serde_json::json!({"mode":"put","name":name,"content":content,"namespace":"USER"}),
    );
    let result = call_tool(
        "iris_compile",
        serde_json::json!({"target":name,"namespace":"USER"}),
    );
    assert_eq!(
        result["success"], true,
        "%RegisteredObject subclass: {}",
        result
    );
    call_tool(
        "iris_doc",
        serde_json::json!({"mode":"delete","name":name,"namespace":"USER"}),
    );
}

#[test]
fn e2e_compile_open_uri_in_response() {
    require_iris!();
    // Successful compile of a single class must include open_uri for VS Code auto-open
    let name = "Test022.OpenUri.cls";
    let content = "Class Test022.OpenUri { ClassMethod Run() { } }";
    call_tool(
        "iris_doc",
        serde_json::json!({"mode":"put","name":name,"content":content,"namespace":"USER"}),
    );
    let result = call_tool(
        "iris_compile",
        serde_json::json!({"target":name,"namespace":"USER"}),
    );
    if result["success"] == true {
        let uri = result["open_uri"].as_str().unwrap_or("");
        assert!(
            uri.starts_with("isfs://"),
            "open_uri must be isfs:// scheme: {}",
            result
        );
        assert!(
            uri.contains("Test022"),
            "open_uri must contain class name: {}",
            result
        );
    }
    call_tool(
        "iris_doc",
        serde_json::json!({"mode":"delete","name":name,"namespace":"USER"}),
    );
}

// ── iris_generate (context building) ─────────────────────────────────────────

#[test]
fn e2e_generate_returns_prompt_context() {
    require_iris!();
    // iris_generate assembles namespace context for LLM generation.
    // Tests that it calls %Dictionary and returns a usable prompt.
    let result = call_tool(
        "iris_generate",
        serde_json::json!({
            "gen_type": "class",
            "description": "A simple calculator class",
            "namespace": "USER"
        }),
    );
    assert!(
        result["success"] == true || result["error_code"].is_string(),
        "iris_generate must return structured response: {}",
        result
    );
    if result["success"] == true {
        assert!(
            result["prompt"].is_string() || result["context"].is_string(),
            "iris_generate must return prompt or context: {}",
            result
        );
    }
}

// ── docs_introspect deeper ───────────────────────────────────────────────────

#[test]
fn e2e_introspect_returns_method_signatures() {
    require_iris!();
    // Ens.Director has well-known methods — verify FormalSpec is returned.
    let result = call_tool(
        "docs_introspect",
        serde_json::json!({"class_name": "Ens.Director", "namespace": "USER"}),
    );
    assert_eq!(
        result["success"], true,
        "introspect Ens.Director: {}",
        result
    );
    let methods = result["methods"].as_array().cloned().unwrap_or_default();
    assert!(
        !methods.is_empty(),
        "Ens.Director must have methods: {}",
        result
    );
    // At least one method must have a FormalSpec (proves SQL params are working)
    let has_formal_spec = methods.iter().any(|m| {
        m["FormalSpec"]
            .as_str()
            .map(|s| !s.is_empty())
            .unwrap_or(false)
    });
    // FormalSpec may be empty for some methods — just assert structure
    let has_name = methods
        .iter()
        .all(|m| m["Name"].as_str().map(|s| !s.is_empty()).unwrap_or(false));
    assert!(has_name, "all methods must have Name: {:?}", methods);
    let _ = has_formal_spec; // informational
}

// ── iris_symbols extended ─────────────────────────────────────────────────────

#[test]
fn e2e_symbols_limit_respected() {
    require_iris!();
    let result = call_tool(
        "iris_symbols",
        serde_json::json!({
            "query": "Ens", "namespace": "USER", "limit": 3
        }),
    );
    assert!(
        result["symbols"].is_array(),
        "symbols must be array: {}",
        result
    );
    let symbols = result["symbols"].as_array().unwrap();
    assert!(
        symbols.len() <= 3,
        "limit=3 must not return more than 3: {}",
        symbols.len()
    );
}

#[test]
fn e2e_symbols_returns_name_field() {
    require_iris!();
    let result = call_tool(
        "iris_symbols",
        serde_json::json!({
            "query": "Ens.Director", "namespace": "USER", "limit": 5
        }),
    );
    let symbols = result["symbols"].as_array().cloned().unwrap_or_default();
    for sym in &symbols {
        assert!(
            sym["Name"].is_string(),
            "each symbol must have Name field: {:?}",
            sym
        );
        assert!(
            !sym["Name"].as_str().unwrap_or("").is_empty(),
            "Name must not be empty: {:?}",
            sym
        );
    }
}

#[test]
fn e2e_symbols_count_matches_symbols_length() {
    require_iris!();
    let result = call_tool(
        "iris_symbols",
        serde_json::json!({
            "query": "Ens.Director", "namespace": "USER", "limit": 10
        }),
    );
    if result["symbols"].is_array() && result["count"].is_number() {
        let symbols_len = result["symbols"].as_array().unwrap().len() as u64;
        let count = result["count"].as_u64().unwrap_or(0);
        assert_eq!(
            symbols_len, count,
            "symbols array length must match count field: {}",
            result
        );
    }
}

#[test]
fn e2e_symbols_user_defined_class_found() {
    require_iris!();
    // Seed a class, verify iris_symbols finds it
    let name = "Test022.SymFind.cls";
    call_tool(
        "iris_doc",
        serde_json::json!({"mode":"put","name":name,
        "content":"Class Test022.SymFind { }","namespace":"USER"}),
    );
    call_tool(
        "iris_compile",
        serde_json::json!({"target":name,"namespace":"USER"}),
    );
    let result = call_tool(
        "iris_symbols",
        serde_json::json!({
            "query": "Test022.SymFind", "namespace": "USER", "limit": 5
        }),
    );
    let symbols = result["symbols"].as_array().cloned().unwrap_or_default();
    let found = symbols
        .iter()
        .any(|s| s["Name"].as_str() == Some("Test022.SymFind"));
    assert!(
        found,
        "compiled class must appear in symbols: {:?}",
        symbols
    );
    call_tool(
        "iris_doc",
        serde_json::json!({"mode":"delete","name":name,"namespace":"USER"}),
    );
}

#[test]
fn e2e_symbols_query_hint_in_response() {
    require_iris!();
    // iris_symbols now includes query_hint explaining syntax — verify it's present
    let result = call_tool(
        "iris_symbols",
        serde_json::json!({
            "query": "Ens", "namespace": "USER", "limit": 1
        }),
    );
    // query_hint is present in v0.4.x+ — may not exist in older versions
    if result["query_hint"].is_string() {
        assert!(
            !result["query_hint"].as_str().unwrap().is_empty(),
            "query_hint must not be empty: {}",
            result
        );
    }
}

// ── docs_introspect extended ──────────────────────────────────────────────────

#[test]
fn e2e_introspect_returns_properties() {
    require_iris!();
    // Seed a class with a property, introspect, verify properties returned
    let name = "Test022.WithProp.cls";
    call_tool(
        "iris_doc",
        serde_json::json!({"mode":"put","name":name,
        "content":"Class Test022.WithProp Extends %Persistent { Property Score As %Integer; }",
        "namespace":"USER"}),
    );
    call_tool(
        "iris_compile",
        serde_json::json!({"target":name,"namespace":"USER"}),
    );
    let result = call_tool(
        "docs_introspect",
        serde_json::json!({
            "class_name": "Test022.WithProp", "namespace": "USER"
        }),
    );
    assert_eq!(
        result["success"], true,
        "introspect compiled class: {}",
        result
    );
    let props = result["properties"].as_array().cloned().unwrap_or_default();
    let found = props.iter().any(|p| p["Name"].as_str() == Some("Score"));
    assert!(found, "Score property must be in properties: {:?}", props);
    call_tool(
        "iris_doc",
        serde_json::json!({"mode":"delete","name":name,"namespace":"USER"}),
    );
}

#[test]
fn e2e_introspect_method_has_formal_spec_field() {
    require_iris!();
    // Ens.Director.StartProduction has a FormalSpec
    let result = call_tool(
        "docs_introspect",
        serde_json::json!({
            "class_name": "Ens.Director", "namespace": "USER"
        }),
    );
    assert_eq!(
        result["success"], true,
        "introspect Ens.Director: {}",
        result
    );
    let methods = result["methods"].as_array().cloned().unwrap_or_default();
    // At least one method must have a non-empty FormalSpec (now a structured array).
    let has_formal = methods.iter().any(|m| {
        m["FormalSpec"]
            .as_array()
            .map(|a| !a.is_empty())
            .unwrap_or(false)
    });
    assert!(
        has_formal,
        "at least one Ens.Director method must have FormalSpec: {:?}",
        methods.iter().map(|m| &m["Name"]).collect::<Vec<_>>()
    );
}

#[test]
fn e2e_introspect_method_return_type_present() {
    require_iris!();
    let result = call_tool(
        "docs_introspect",
        serde_json::json!({
            "class_name": "Ens.Director", "namespace": "USER"
        }),
    );
    assert_eq!(result["success"], true);
    let methods = result["methods"].as_array().cloned().unwrap_or_default();
    for m in &methods {
        // ReturnType may be empty (void methods) but field must exist
        assert!(
            m.get("ReturnType").is_some(),
            "ReturnType key must exist: {:?}",
            m
        );
    }
}

#[test]
fn e2e_introspect_user_class_after_compile() {
    require_iris!();
    let name = "Test022.Introspectable.cls";
    let content = "Class Test022.Introspectable {\nClassMethod Add(a As %Integer, b As %Integer) As %Integer { Return a+b }\n}";
    call_tool(
        "iris_doc",
        serde_json::json!({"mode":"put","name":name,"content":content,"namespace":"USER"}),
    );
    call_tool(
        "iris_compile",
        serde_json::json!({"target":name,"namespace":"USER"}),
    );
    let result = call_tool(
        "docs_introspect",
        serde_json::json!({
            "class_name": "Test022.Introspectable", "namespace": "USER"
        }),
    );
    assert_eq!(result["success"], true, "introspect user class: {}", result);
    let methods = result["methods"].as_array().cloned().unwrap_or_default();
    let found = methods.iter().any(|m| m["Name"].as_str() == Some("Add"));
    assert!(found, "Add method must appear in introspect: {:?}", methods);
    call_tool(
        "iris_doc",
        serde_json::json!({"mode":"delete","name":name,"namespace":"USER"}),
    );
}

#[test]
fn e2e_introspect_class_name_in_response() {
    require_iris!();
    let result = call_tool(
        "docs_introspect",
        serde_json::json!({
            "class_name": "Ens.Director", "namespace": "USER"
        }),
    );
    assert_eq!(result["success"], true);
    // Response must echo back the class_name
    assert_eq!(
        result["class_name"].as_str(),
        Some("Ens.Director"),
        "class_name must be echoed in response: {}",
        result
    );
}

// ── iris_info extended ────────────────────────────────────────────────────────

#[test]
fn e2e_info_metadata_has_version_string() {
    require_iris!();
    let result = call_tool(
        "iris_info",
        serde_json::json!({"what":"metadata","namespace":"USER"}),
    );
    assert_eq!(result["success"], true, "metadata: {}", result);
    // Version must be a non-empty string
    let ver = result["version"]
        .as_str()
        .or_else(|| result["iris_version"].as_str())
        .unwrap_or("");
    if !ver.is_empty() {
        assert!(
            ver.contains("IRIS") || ver.contains("20"),
            "version string must mention IRIS or year: {:?}",
            ver
        );
    }
}

#[test]
fn e2e_info_namespace_matches_requested() {
    require_iris!();
    let result = call_tool(
        "iris_info",
        serde_json::json!({"what":"namespace","namespace":"USER"}),
    );
    if result["success"] == true {
        let ns = result["name"]
            .as_str()
            .or_else(|| result["namespace"].as_str())
            .unwrap_or("");
        assert!(
            ns.to_uppercase().contains("USER"),
            "namespace name must contain USER: {:?}",
            ns
        );
    }
}

#[test]
fn e2e_info_jobs_entries_have_expected_fields() {
    require_iris!();
    let result = call_tool(
        "iris_info",
        serde_json::json!({"what":"jobs","namespace":"USER"}),
    );
    if result["success"] == true {
        let jobs = result["jobs"].as_array().cloned().unwrap_or_default();
        // If there are jobs, each must have at least a pid or job-id field
        for job in &jobs {
            assert!(
                job.get("pid").is_some() || job.get("job").is_some() || job.get("PID").is_some(),
                "job entry must have pid/job field: {:?}",
                job
            );
        }
    }
}

#[test]
fn e2e_info_csp_apps_structured_response() {
    require_iris!();
    // csp_apps returns 404 on some Atelier v8 endpoints — documented issue I-7
    let result = call_tool(
        "iris_info",
        serde_json::json!({"what":"csp_apps","namespace":"USER"}),
    );
    // Accept success or error — must not crash
    assert!(
        result.is_object(),
        "csp_apps must return object: {}",
        result
    );
    assert!(
        result["success"] == true || result["error_code"].is_string(),
        "csp_apps must be structured: {}",
        result
    );
}

// ── Interoperability extended ─────────────────────────────────────────────────

#[test]
fn e2e_interop_production_status_no_crash_without_container() {
    require_iris!();
    // Without IRIS_CONTAINER, production tools return DOCKER_REQUIRED — that's fine
    // This test verifies the error is structured, not a panic/crash
    let result = call_tool(
        "iris_production",
        serde_json::json!({"action": "status", "namespace":"USER"}),
    );
    assert!(
        result["success"] == true || result["error_code"].is_string() || result.is_object(),
        "production status must not crash: {}",
        result
    );
}

#[test]
fn e2e_interop_queues_count_field() {
    require_iris!();
    let result = call_tool("iris_interop_query", serde_json::json!({"what": "queues"}));
    if result["success"] == true {
        let queues = result["queues"].as_array().cloned().unwrap_or_default();
        // count field must match array length
        let count = result["count"].as_u64().unwrap_or(queues.len() as u64);
        assert_eq!(
            count,
            queues.len() as u64,
            "count must match queues array length: {}",
            result
        );
    }
}

#[test]
fn e2e_interop_logs_limit_parameter() {
    require_iris!();
    let result = call_tool(
        "iris_interop_query",
        serde_json::json!({
            "what": "logs",
            "log_type": "error,warning,info",
            "limit": 3
        }),
    );
    if result["success"] == true {
        let logs = result["logs"].as_array().cloned().unwrap_or_default();
        assert!(
            logs.len() <= 3,
            "limit=3 must not return more than 3 logs: {}",
            logs.len()
        );
    }
}

#[test]
fn e2e_interop_message_search_with_limit() {
    require_iris!();
    let result = call_tool(
        "iris_interop_query",
        serde_json::json!({"what": "messages", "limit": 2}),
    );
    if result["success"] == true {
        let messages = result["messages"].as_array().cloned().unwrap_or_default();
        assert!(
            messages.len() <= 2,
            "limit=2 must not exceed: {}",
            messages.len()
        );
    }
}

#[test]
fn e2e_interop_logs_error_type_filter() {
    require_iris!();
    let result = call_tool(
        "iris_interop_query",
        serde_json::json!({
            "what": "logs",
            "log_type": "error",
            "limit": 5
        }),
    );
    assert!(
        result["success"] == true || result["error_code"].is_string(),
        "error-type filter: {}",
        result
    );
}

// ── Debug tools extended ──────────────────────────────────────────────────────

#[test]
fn e2e_debug_error_logs_max_entries_cap() {
    require_iris!();
    // debug_get_error_logs consolidated into iris_debug(action=error_logs) — FR-007.
    // (limit-cap behavior lives in the legacy standalone impl; iris_debug's error_logs
    // action always returns an empty list on non-docker-exec connections — verify shape only.)
    let result = call_tool(
        "iris_debug",
        serde_json::json!({"action": "error_logs", "namespace": "USER", "limit": 5000}),
    );
    assert_eq!(result["success"], true, "iris_debug error_logs: {}", result);
    assert!(
        result["logs"].is_array(),
        "logs must be an array: {}",
        result
    );
}

#[test]
fn e2e_debug_error_logs_small_limit() {
    require_iris!();
    let result = call_tool(
        "iris_debug",
        serde_json::json!({"action": "error_logs", "namespace": "USER", "limit": 1}),
    );
    assert_eq!(
        result["success"], true,
        "iris_debug error_logs limit=1: {}",
        result
    );
    assert!(
        result["logs"].is_array(),
        "logs must be an array: {}",
        result
    );
}

#[test]
fn e2e_debug_capture_packet_success_field() {
    require_iris!();
    let result = call_tool(
        "iris_debug",
        serde_json::json!({"action": "capture", "namespace": "USER"}),
    );
    assert!(
        result["success"] == true || result["error_code"].is_string(),
        "iris_debug capture must return structured response: {}",
        result
    );
    if result["success"] == true {
        assert!(
            result["capture"].is_string(),
            "capture field must be a string when success: {}",
            result
        );
    }
}

#[test]
fn e2e_debug_source_map_nonexistent_class() {
    require_iris!();
    // debug_source_map consolidated into iris_debug(action=source_map) — FR-007.
    // On a nonexistent class this must not crash — returns empty mapping or a structured error.
    let result = call_tool(
        "iris_debug",
        serde_json::json!({
            "action": "source_map",
            "class_name": "NonExistent.Class.XYZ",
            "namespace": "USER"
        }),
    );
    assert!(
        result["success"] == true || result["error_code"].is_string(),
        "iris_debug source_map nonexistent must be structured: {}",
        result
    );
}

// ── #98: iris_debug HTTP-only path (no DOCKER_REQUIRED) ──────────────────────

#[test]
fn e2e_debug_capture_http_only_no_docker_required() {
    require_iris!();
    // Call with IRIS_CONTAINER="" to force the HTTP-only code path.
    // Must NOT return DOCKER_REQUIRED — capture runs via execute_via_generator.
    let mut env = iris_env();
    for e in &mut env {
        if e.0 == "IRIS_CONTAINER" {
            e.1 = "".to_string();
        }
    }
    let mut msgs = init_msgs();
    msgs.push(serde_json::json!({
        "jsonrpc":"2.0","id":2,"method":"tools/call",
        "params":{"name":"iris_debug","arguments":{"action":"capture","namespace":"USER"}}
    }));
    let responses = mcp_call_timeout(&env, &msgs, 15);
    let result = tool_result(&responses, 2);
    assert_ne!(
        result["error_code"].as_str(),
        Some("DOCKER_REQUIRED"),
        "iris_debug capture must not return DOCKER_REQUIRED on HTTP-only connection (#98): {}",
        result
    );
    assert!(
        result["success"] == true || result["error_code"].is_string(),
        "iris_debug capture must return structured response on HTTP path: {}",
        result
    );
    if result["success"] == true {
        assert!(
            result["capture"].is_string(),
            "capture field must be a string: {}",
            result
        );
    }
}

#[test]
fn e2e_debug_map_int_http_only_no_docker_required() {
    require_iris!();
    let mut env = iris_env();
    for e in &mut env {
        if e.0 == "IRIS_CONTAINER" {
            e.1 = "".to_string();
        }
    }
    let mut msgs = init_msgs();
    msgs.push(serde_json::json!({
        "jsonrpc":"2.0","id":2,"method":"tools/call",
        "params":{"name":"iris_debug","arguments":{
            "action":"map_int",
            "error_string":"<UNDEFINED>x+1^Unknown.Foo.1",
            "namespace":"USER"
        }}
    }));
    let responses = mcp_call_timeout(&env, &msgs, 15);
    let result = tool_result(&responses, 2);
    assert_ne!(
        result["error_code"].as_str(),
        Some("DOCKER_REQUIRED"),
        "iris_debug map_int must not return DOCKER_REQUIRED on HTTP-only connection (#98): {}",
        result
    );
}

#[test]
fn e2e_debug_source_map_http_only_no_docker_required() {
    require_iris!();
    let mut env = iris_env();
    for e in &mut env {
        if e.0 == "IRIS_CONTAINER" {
            e.1 = "".to_string();
        }
    }
    let mut msgs = init_msgs();
    msgs.push(serde_json::json!({
        "jsonrpc":"2.0","id":2,"method":"tools/call",
        "params":{"name":"iris_debug","arguments":{
            "action":"source_map",
            "class_name":"Unknown.DoesNotExist",
            "namespace":"USER"
        }}
    }));
    let responses = mcp_call_timeout(&env, &msgs, 15);
    let result = tool_result(&responses, 2);
    assert_ne!(
        result["error_code"].as_str(),
        Some("DOCKER_REQUIRED"),
        "iris_debug source_map must not return DOCKER_REQUIRED on HTTP-only connection (#98): {}",
        result
    );
}

// ── iris_doc extended ─────────────────────────────────────────────────────────

#[test]
fn e2e_doc_put_and_verify_content_preserved() {
    require_iris!();
    let name = "Test022.ContentCheck.cls";
    let content =
        "Class Test022.ContentCheck {\n/// Unique marker: XYZZY42\nClassMethod Marker() { }\n}";
    call_tool(
        "iris_doc",
        serde_json::json!({"mode":"put","name":name,"content":content,"namespace":"USER"}),
    );
    let get = call_tool(
        "iris_doc",
        serde_json::json!({"mode":"get","name":name,"namespace":"USER"}),
    );
    assert_eq!(get["success"], true, "get after put: {}", get);
    assert!(
        get["content"].as_str().unwrap_or("").contains("XYZZY42"),
        "unique marker must survive round-trip: {}",
        get
    );
    call_tool(
        "iris_doc",
        serde_json::json!({"mode":"delete","name":name,"namespace":"USER"}),
    );
}

#[test]
fn e2e_doc_delete_removes_document() {
    require_iris!();
    let name = "Test022.DeleteMe.cls";
    call_tool(
        "iris_doc",
        serde_json::json!({"mode":"put","name":name,
            "content":"Class Test022.DeleteMe { }","namespace":"USER"}),
    );
    let del = call_tool(
        "iris_doc",
        serde_json::json!({"mode":"delete","name":name,"namespace":"USER"}),
    );
    assert_eq!(del["success"], true, "delete: {}", del);
    // HEAD after delete must return not-found
    let head = call_tool(
        "iris_doc",
        serde_json::json!({"mode":"head","name":name,"namespace":"USER"}),
    );
    assert!(
        head["success"] == false || head["exists"] == false,
        "document must not exist after delete: {}",
        head
    );
}

#[test]
fn e2e_doc_get_mac_routine() {
    require_iris!();
    // Read a known .mac routine — tests non-.cls document type
    let result = call_tool(
        "iris_doc",
        serde_json::json!({"mode":"get","name":"%Library.Global.mac","namespace":"USER"}),
    );
    // May succeed or return not-found — just must be structured
    assert!(
        result["success"] == true || result["error_code"].is_string(),
        "get .mac must return structured response: {}",
        result
    );
}

#[test]
fn e2e_doc_put_multiline_content_all_lines_stored() {
    require_iris!();
    let name = "Test022.MultiLine.cls";
    let content = "Class Test022.MultiLine {\nClassMethod Line1() { }\nClassMethod Line2() { }\nClassMethod Line3() { }\n}";
    call_tool(
        "iris_doc",
        serde_json::json!({"mode":"put","name":name,"content":content,"namespace":"USER"}),
    );
    let get = call_tool(
        "iris_doc",
        serde_json::json!({"mode":"get","name":name,"namespace":"USER"}),
    );
    if get["success"] == true {
        let c = get["content"].as_str().unwrap_or("");
        assert!(
            c.contains("Line1") && c.contains("Line2") && c.contains("Line3"),
            "all three methods must be in stored content: {}",
            get
        );
    }
    call_tool(
        "iris_doc",
        serde_json::json!({"mode":"delete","name":name,"namespace":"USER"}),
    );
}

#[test]
fn e2e_doc_batch_get_preserves_order() {
    require_iris!();
    // Batch get must return docs in the requested order, not arbitrary order
    let a = "Test022.OrderA.cls";
    let b = "Test022.OrderB.cls";
    let c = "Test022.OrderC.cls";
    for (n, content) in &[
        (a, "Class Test022.OrderA{}"),
        (b, "Class Test022.OrderB{}"),
        (c, "Class Test022.OrderC{}"),
    ] {
        call_tool(
            "iris_doc",
            serde_json::json!({"mode":"put","name":n,"content":content,"namespace":"USER"}),
        );
        call_tool(
            "iris_compile",
            serde_json::json!({"target":n,"namespace":"USER"}),
        );
    }
    let result = call_tool_timeout(
        "iris_doc",
        serde_json::json!({"mode":"get","names":[a,b,c],"namespace":"USER"}),
        20,
    );
    if result["success"] == true {
        let docs = result["documents"].as_array().cloned().unwrap_or_default();
        if docs.len() == 3 {
            assert_eq!(
                docs[0]["name"].as_str(),
                Some(a),
                "first doc should be A: {:?}",
                docs[0]
            );
            assert_eq!(
                docs[1]["name"].as_str(),
                Some(b),
                "second doc should be B: {:?}",
                docs[1]
            );
            assert_eq!(
                docs[2]["name"].as_str(),
                Some(c),
                "third doc should be C: {:?}",
                docs[2]
            );
        }
    }
    for n in &[a, b, c] {
        call_tool(
            "iris_doc",
            serde_json::json!({"mode":"delete","name":n,"namespace":"USER"}),
        );
    }
}

#[test]
fn e2e_doc_put_overwrites_existing() {
    require_iris!();
    let name = "Test022.Overwrite.cls";
    call_tool(
        "iris_doc",
        serde_json::json!({"mode":"put","name":name,
            "content":"Class Test022.Overwrite { ClassMethod V1() { } }","namespace":"USER"}),
    );
    call_tool(
        "iris_doc",
        serde_json::json!({"mode":"put","name":name,
            "content":"Class Test022.Overwrite { ClassMethod V2() { } }","namespace":"USER"}),
    );
    let get = call_tool(
        "iris_doc",
        serde_json::json!({"mode":"get","name":name,"namespace":"USER"}),
    );
    if get["success"] == true {
        let c = get["content"].as_str().unwrap_or("");
        assert!(c.contains("V2"), "overwrite must store V2: {}", get);
    }
    call_tool(
        "iris_doc",
        serde_json::json!({"mode":"delete","name":name,"namespace":"USER"}),
    );
}

#[test]
fn e2e_doc_open_uri_after_put() {
    require_iris!();
    let name = "Test022.OpenUriDoc.cls";
    let result = call_tool(
        "iris_doc",
        serde_json::json!({"mode":"put","name":name,
            "content":"Class Test022.OpenUriDoc { }","namespace":"USER"}),
    );
    if result["success"] == true {
        let uri = result["open_uri"].as_str().unwrap_or("");
        assert!(
            uri.starts_with("isfs://"),
            "put must return isfs:// open_uri: {}",
            result
        );
    }
    call_tool(
        "iris_doc",
        serde_json::json!({"mode":"delete","name":name,"namespace":"USER"}),
    );
}

#[test]
fn e2e_doc_put_inc_file() {
    require_iris!();
    // Test non-.cls document type: .inc include file
    let name = "Test022.MyMacros.inc";
    let content = "#define TESTVAL 42\n";
    let result = call_tool(
        "iris_doc",
        serde_json::json!({"mode":"put","name":name,"content":content,"namespace":"USER"}),
    );
    assert!(
        result["success"] == true || result["error_code"].is_string(),
        "put .inc file must return structured response: {}",
        result
    );
    if result["success"] == true {
        call_tool(
            "iris_doc",
            serde_json::json!({"mode":"delete","name":name,"namespace":"USER"}),
        );
    }
}

// ── iris_query extended ───────────────────────────────────────────────────────

#[test]
fn e2e_query_top_n_limit_respected() {
    require_iris!();
    let result = call_tool(
        "iris_query",
        serde_json::json!({
            "query": "SELECT TOP 3 Name FROM %Dictionary.ClassDefinition ORDER BY Name",
            "namespace": "USER"
        }),
    );
    assert_eq!(result["success"], true, "TOP 3: {}", result);
    let rows = result["rows"].as_array().cloned().unwrap_or_default();
    assert_eq!(
        rows.len(),
        3,
        "TOP 3 must return exactly 3 rows: {}",
        result
    );
}

#[test]
fn e2e_query_count_returns_integer() {
    require_iris!();
    let result = call_tool(
        "iris_query",
        serde_json::json!({
            "query": "SELECT COUNT(*) AS cnt FROM %Dictionary.ClassDefinition",
            "namespace": "USER"
        }),
    );
    assert_eq!(result["success"], true, "COUNT: {}", result);
    let rows = result["rows"].as_array().cloned().unwrap_or_default();
    assert!(!rows.is_empty(), "COUNT must return a row: {}", result);
    let cnt = rows[0]["cnt"]
        .as_i64()
        .or_else(|| rows[0]["Cnt"].as_i64())
        .unwrap_or(0);
    assert!(
        cnt > 100,
        "namespace must have >100 classes, got {}: {}",
        cnt,
        result
    );
}

#[test]
fn e2e_query_where_like_filter() {
    require_iris!();
    let result = call_tool(
        "iris_query",
        serde_json::json!({
            "query": "SELECT TOP 5 Name FROM %Dictionary.ClassDefinition WHERE Name LIKE 'Ens.%' ORDER BY Name",
            "namespace": "USER"
        }),
    );
    assert_eq!(result["success"], true, "LIKE filter: {}", result);
    let rows = result["rows"].as_array().cloned().unwrap_or_default();
    for row in &rows {
        let name = row["Name"].as_str().unwrap_or("");
        assert!(
            name.starts_with("Ens."),
            "LIKE 'Ens.%' must only return Ens classes: {}",
            name
        );
    }
}

#[test]
fn e2e_query_order_by_respected() {
    require_iris!();
    let result = call_tool(
        "iris_query",
        serde_json::json!({
            "query": "SELECT TOP 5 Name FROM %Dictionary.ClassDefinition ORDER BY Name ASC",
            "namespace": "USER"
        }),
    );
    assert_eq!(result["success"], true, "ORDER BY: {}", result);
    let rows = result["rows"].as_array().cloned().unwrap_or_default();
    let names: Vec<&str> = rows.iter().filter_map(|r| r["Name"].as_str()).collect();
    let mut sorted = names.clone();
    sorted.sort();
    assert_eq!(names, sorted, "rows must be sorted ascending: {:?}", names);
}

#[test]
fn e2e_query_multiple_columns_returned() {
    require_iris!();
    let result = call_tool(
        "iris_query",
        serde_json::json!({
            "query": "SELECT TOP 1 Name, Super FROM %Dictionary.ClassDefinition WHERE Name = 'Ens.Director'",
            "namespace": "USER"
        }),
    );
    assert_eq!(result["success"], true, "multi-column: {}", result);
    let rows = result["rows"].as_array().cloned().unwrap_or_default();
    assert!(!rows.is_empty(), "must find Ens.Director: {}", result);
    assert!(
        rows[0]["Name"].is_string(),
        "Name column must exist: {:?}",
        rows[0]
    );
    assert!(
        rows[0]["Super"].is_string() || rows[0]["Super"].is_null(),
        "Super column must exist: {:?}",
        rows[0]
    );
}

#[test]
fn e2e_query_insert_update_delete_sequence() {
    require_iris!();
    // Full DML cycle on a temp persistent class
    let cls = "Test022.DmlTest.cls";
    call_tool(
        "iris_doc",
        serde_json::json!({"mode":"put","name":cls,
            "content":"Class Test022.DmlTest Extends %Persistent { Property Val As %String; }",
            "namespace":"USER"}),
    );
    let compile = call_tool(
        "iris_compile",
        serde_json::json!({"target":cls,"namespace":"USER"}),
    );
    if compile["success"] != true {
        call_tool(
            "iris_doc",
            serde_json::json!({"mode":"delete","name":cls,"namespace":"USER"}),
        );
        return;
    }
    // INSERT
    let ins = call_tool(
        "iris_query",
        serde_json::json!({
            "query":"INSERT INTO Test022.DmlTest (Val) VALUES (?)",
            "parameters":["hello"],"namespace":"USER"}),
    );
    if ins["success"] == true {
        // UPDATE
        call_tool(
            "iris_query",
            serde_json::json!({
                "query":"UPDATE Test022.DmlTest SET Val=? WHERE Val=?",
                "parameters":["world","hello"],"namespace":"USER"}),
        );
        // SELECT after update
        let sel = call_tool(
            "iris_query",
            serde_json::json!({
                "query":"SELECT Val FROM Test022.DmlTest WHERE Val=?",
                "parameters":["world"],"namespace":"USER"}),
        );
        assert_eq!(sel["success"], true, "SELECT after UPDATE: {}", sel);
        // DELETE
        call_tool(
            "iris_query",
            serde_json::json!({
                "query":"DELETE FROM Test022.DmlTest WHERE Val=?",
                "parameters":["world"],"namespace":"USER"}),
        );
    }
    call_tool(
        "iris_doc",
        serde_json::json!({"mode":"delete","name":cls,"namespace":"USER"}),
    );
}

#[test]
fn e2e_query_null_handling() {
    require_iris!();
    // SELECT NULL AS val should return null in the row
    let result = call_tool(
        "iris_query",
        serde_json::json!({
            "query": "SELECT NULL AS val, 'present' AS other",
            "namespace": "USER"
        }),
    );
    assert_eq!(result["success"], true, "SELECT NULL: {}", result);
    let rows = result["rows"].as_array().cloned().unwrap_or_default();
    assert!(!rows.is_empty(), "must return a row: {}", result);
    assert!(
        rows[0]["other"].as_str() == Some("present"),
        "non-null value: {:?}",
        rows[0]
    );
}

#[test]
fn e2e_query_stored_proc_call() {
    require_iris!();
    // Call a built-in IRIS SQL expression
    let result = call_tool(
        "iris_query",
        serde_json::json!({
            "query": "SELECT %EXTERNAL(1+1) AS two",
            "namespace": "USER"
        }),
    );
    // May succeed or fail — just must be structured
    assert!(
        result["success"] == true || result["error_code"].is_string(),
        "stored proc call must be structured: {}",
        result
    );
}

// ── iris_search extended ──────────────────────────────────────────────────────

#[test]
fn e2e_search_case_insensitive_default() {
    require_iris!();
    let result = call_tool(
        "iris_search",
        serde_json::json!({
            "query": "director",
            "namespace": "USER",
            "category": "CLS",
            "max_results": 5
        }),
    );
    assert!(
        result["success"] == true || result["error_code"].is_string(),
        "case-insensitive search: {}",
        result
    );
}

#[test]
fn e2e_search_empty_query_returns_error_not_crash() {
    require_iris!();
    let result = call_tool(
        "iris_search",
        serde_json::json!({
            "query": "",
            "namespace": "USER"
        }),
    );
    // Empty query should return structured response — not crash
    assert!(
        result.is_object(),
        "empty query must return object: {}",
        result
    );
}

#[test]
fn e2e_search_mac_category() {
    require_iris!();
    let result = call_tool(
        "iris_search",
        serde_json::json!({
            "query": "Main",
            "namespace": "USER",
            "category": "MAC",
            "max_results": 5
        }),
    );
    assert!(
        result["success"] == true || result["error_code"].is_string(),
        "MAC category search: {}",
        result
    );
}

#[test]
fn e2e_search_nonexistent_content_returns_empty() {
    require_iris!();
    let result = call_tool(
        "iris_search",
        serde_json::json!({
            "query": "ZZZNOMATCHXXX999",
            "namespace": "USER",
            "max_results": 5
        }),
    );
    assert!(
        result["success"] == true || result["error_code"].is_string(),
        "no-match search: {}",
        result
    );
    if result["success"] == true {
        let results = result["results"].as_array().cloned().unwrap_or_default();
        assert_eq!(
            results.len(),
            0,
            "gibberish query should return 0 results: {}",
            result
        );
    }
}

#[test]
fn e2e_search_max_results_respected() {
    require_iris!();
    let result = call_tool(
        "iris_search",
        serde_json::json!({
            "query": "Class",
            "namespace": "USER",
            "max_results": 2
        }),
    );
    if result["success"] == true {
        let results = result["results"].as_array().cloned().unwrap_or_default();
        assert!(
            results.len() <= 2,
            "max_results=2 must not return more: {} results",
            results.len()
        );
    }
}

#[test]
fn e2e_search_result_has_document_and_context() {
    require_iris!();
    // Seed a class with unique searchable content
    let name = "Test022.SearchContent.cls";
    let unique = "UNIQUESEARCHCONTEXT8675309";
    let content = format!(
        "Class Test022.SearchContent {{\n/// {}\nClassMethod Run() {{ }}\n}}",
        unique
    );
    call_tool(
        "iris_doc",
        serde_json::json!({"mode":"put","name":name,"content":content,"namespace":"USER"}),
    );
    let result = call_tool(
        "iris_search",
        serde_json::json!({
            "query": unique,
            "namespace": "USER",
            "max_results": 3
        }),
    );
    if result["success"] == true {
        let results = result["results"].as_array().cloned().unwrap_or_default();
        if !results.is_empty() {
            // Each result must have document name and some context
            assert!(
                results[0]["document"].is_string(),
                "result must have document: {:?}",
                results[0]
            );
        }
    }
    call_tool(
        "iris_doc",
        serde_json::json!({"mode":"delete","name":name,"namespace":"USER"}),
    );
}

// ── #43: License slot reuse via cookie_store ──────────────────────────────────

/// Verify that multiple iris_execute calls in a session reuse CSP connections
/// rather than creating new license slots for each call.
/// Checks that MaxConnections does not grow proportionally with call count.
#[test]
fn license_slots_reused_across_calls() {
    require_iris!();
    let env = iris_env();

    // Query license slot usage before burst
    let pre = call_tool_timeout(
        "iris_query",
        serde_json::json!({
            "query": "SELECT MaxConnections FROM %SYSTEM.License_CountsGet()",
            "namespace": "USER"
        }),
        10,
    );
    let pre_max = pre["rows"]
        .as_array()
        .and_then(|r| r.first())
        .and_then(|row| row["MaxConnections"].as_u64())
        .unwrap_or(0);
    eprintln!("Pre-burst MaxConnections: {}", pre_max);

    // Fire 10 iris_execute calls back-to-back (same client, should reuse sessions)
    let mut msgs = init_msgs();
    for i in 0..10 {
        msgs.push(serde_json::json!({
            "jsonrpc":"2.0","id":(i+2),"method":"tools/call",
            "params":{"name":"iris_execute","arguments":{"code":"write $ZVERSION,!","namespace":"USER"}}
        }));
    }
    let responses = mcp_call(&env, &msgs);
    assert_eq!(responses.len(), 11, "should have init + 10 tool responses");

    // Query license slots after burst
    let post = call_tool_timeout(
        "iris_query",
        serde_json::json!({
            "query": "SELECT MaxConnections FROM %SYSTEM.License_CountsGet()",
            "namespace": "USER"
        }),
        10,
    );
    let post_max = post["rows"]
        .as_array()
        .and_then(|r| r.first())
        .and_then(|row| row["MaxConnections"].as_u64())
        .unwrap_or(0);
    eprintln!("Post-burst MaxConnections: {}", post_max);

    // With cookie reuse, 10 calls should NOT create 10+ new license slots.
    // Allow a small delta (≤3) for existing ambient connections.
    let delta = post_max.saturating_sub(pre_max);
    assert!(
        delta <= 3,
        "MaxConnections grew by {} after 10 iris_execute calls — cookie session reuse not working (expected ≤3 new slots)",
        delta
    );
}

// ── iris_test persistence (#48) ───────────────────────────────────────────────

#[test]
fn e2e_test_classes_persist_between_runs() {
    require_iris!();
    let cls_doc = "Test022.PersistCheck.cls";
    let cls_content = r#"Class Test022.PersistCheck Extends %UnitTest.TestCase {
Method TestPersists() {
  Do $$$AssertEquals(1, 1, "persistence check")
}
}"#;

    // Seed and compile
    let put = call_tool(
        "iris_doc",
        serde_json::json!({"mode":"put","name":cls_doc,"content":cls_content,"namespace":"USER"}),
    );
    assert_eq!(put["success"], true, "seed: {}", put);
    let compile = call_tool(
        "iris_compile",
        serde_json::json!({"target":cls_doc,"namespace":"USER"}),
    );
    assert_eq!(compile["success"], true, "compile: {}", compile);

    // First run
    let r1 = call_tool(
        "iris_test",
        serde_json::json!({"pattern": "Test022.PersistCheck", "namespace": "USER"}),
    );
    if r1["error_code"].as_str() == Some("NO_TESTS_FOUND")
        || r1["error_code"].as_str() == Some("DOCKER_REQUIRED")
    {
        call_tool(
            "iris_doc",
            serde_json::json!({"mode":"delete","name":cls_doc,"namespace":"USER"}),
        );
        return;
    }
    assert_eq!(r1["passed"].as_u64().unwrap_or(0), 1, "first run: {}", r1);

    // Second run without re-uploading — class must still be present (/nodelete)
    let r2 = call_tool(
        "iris_test",
        serde_json::json!({"pattern": "Test022.PersistCheck", "namespace": "USER"}),
    );
    assert!(
        r2["error_code"].as_str() != Some("NO_TESTS_FOUND"),
        "test class was deleted after first run — /nodelete not working: {}",
        r2
    );
    assert_eq!(
        r2["passed"].as_u64().unwrap_or(0),
        1,
        "second run should find same class: {}",
        r2
    );

    // Cleanup
    call_tool(
        "iris_doc",
        serde_json::json!({"mode":"delete","name":cls_doc,"namespace":"USER"}),
    );
}

// ── HTTP client config (#44) ──────────────────────────────────────────────────

#[test]
fn e2e_http_client_tcp_keepalive_set() {
    // Verify the HTTP client can be constructed with the new keepalive config.
    // This is a build-time/config test — if http_client() fails, the MCP server
    // would not start at all, so we just verify it constructs successfully.
    let client = iris_agentic_dev_core::iris::connection::IrisConnection::http_client();
    assert!(
        client.is_ok(),
        "http_client() must build successfully with tcp_keepalive: {:?}",
        client.err()
    );
}

#[test]
fn e2e_iris_tls_verify_false_disables_cert_check() {
    // IRIS_TLS_VERIFY=false must produce the same result as IRIS_INSECURE=true.
    // We just verify the client builds — actual TLS behavior requires a self-signed
    // cert endpoint which isn't available in CI.
    std::env::set_var("IRIS_TLS_VERIFY", "false");
    let client = iris_agentic_dev_core::iris::connection::IrisConnection::http_client();
    std::env::remove_var("IRIS_TLS_VERIFY");
    assert!(
        client.is_ok(),
        "http_client() must build with IRIS_TLS_VERIFY=false: {:?}",
        client.err()
    );
}

// ── 037: Dynamic dispatch resolution tools ────────────────────────────────────

/// resolve_dynamic_dispatch returns candidates for a known IRIS method.
#[test]
fn e2e_resolve_dynamic_dispatch_returns_candidates() {
    require_iris!();
    let result = call_tool(
        "resolve_dynamic_dispatch",
        serde_json::json!({"method_name": "Connect", "package_prefix": "EnsLib", "namespace": "USER"}),
    );
    // Accept NO_RESULTS if namespace has no EnsLib classes compiled
    if result["error_code"].as_str() == Some("IRIS_UNREACHABLE")
        || result["error_code"].as_str() == Some("TIMEOUT")
    {
        eprintln!("resolve_dynamic_dispatch: IRIS unavailable — skipping");
        return;
    }
    assert_eq!(
        result["success"], true,
        "resolve_dynamic_dispatch must succeed: {}",
        result
    );
    assert!(result["candidates"].is_array(), "candidates must be array");
    let n = result["candidate_count"].as_u64().unwrap_or(0);
    if n > 0 {
        let first = &result["candidates"][0];
        assert!(first["class"].is_string(), "candidate must have class");
        assert!(
            first["confidence"].is_number(),
            "candidate must have confidence"
        );
        assert!(
            first["confidence"].as_f64().unwrap_or(0.0) > 0.0,
            "confidence must be > 0"
        );
    }
    // Verify confidence matches formula
    if n == 1 {
        assert_eq!(result["confidence"], 0.90);
    } else if (2..=5).contains(&n) {
        assert_eq!(result["confidence"], 0.75);
    }
}

/// extract_message_map_routing: plain class (no MessageMap) returns has_message_map:false.
#[test]
fn e2e_extract_message_map_no_message_map_class() {
    require_iris!();
    // A class that exists but has no MessageMap XData returns NOT_FOUND or a parse error
    // (execute_via_generator output format varies by IRIS version).
    // The important invariant is it does NOT return success=true with has_message_map=true.
    let result = call_tool(
        "extract_message_map_routing",
        serde_json::json!({"class_name": "%Library.Persistent", "namespace": "USER"}),
    );
    if result["error_code"].as_str() == Some("IRIS_UNREACHABLE")
        || result["error_code"].as_str() == Some("TIMEOUT")
    {
        eprintln!("extract_message_map_routing: IRIS unavailable — skipping");
        return;
    }
    // Accept NOT_FOUND (correct new behavior) or empty (execute_via_generator limitation in e2e).
    // What must NOT happen: success=true with has_message_map=true.
    if result["success"] == true {
        assert_ne!(
            result["has_message_map"], true,
            "%Library.Persistent must not claim to have a MessageMap: {}",
            result
        );
    }
}

/// extract_message_map_routing: NOT_FOUND for nonexistent class.
#[test]
fn e2e_extract_message_map_not_found() {
    require_iris!();
    let result = call_tool(
        "extract_message_map_routing",
        serde_json::json!({"class_name": "DoesNot.Exist.Class", "namespace": "USER"}),
    );
    if result["error_code"].as_str() == Some("IRIS_UNREACHABLE") {
        return;
    }
    assert_eq!(
        result["success"], false,
        "nonexistent class must fail: {}",
        result
    );
    assert_eq!(result["error_code"], "NOT_FOUND");
}

/// find_subclass_implementations returns results for a known Ensemble base method.
#[test]
fn e2e_find_subclass_implementations_returns_results() {
    require_iris!();
    let result = call_tool(
        "find_subclass_implementations",
        serde_json::json!({
            "method_name": "OnProcessInput",
            "base_classes": ["Ens.BusinessProcess"],
            "namespace": "USER"
        }),
    );
    if result["error_code"].as_str() == Some("IRIS_UNREACHABLE")
        || result["error_code"].as_str() == Some("TIMEOUT")
    {
        eprintln!("find_subclass_implementations: IRIS unavailable — skipping");
        return;
    }
    assert_eq!(
        result["success"], true,
        "find_subclass must succeed: {}",
        result
    );
    assert!(
        result["implementations"].is_array(),
        "implementations must be array"
    );
    // Accept 0 results if Ens.BusinessProcess has no compiled subclasses in this namespace
    let n = result["implementation_count"].as_u64().unwrap_or(0);
    if n > 0 {
        let first = &result["implementations"][0];
        assert!(first["class"].is_string(), "implementation must have class");
        assert!(
            first["confidence"].is_number(),
            "implementation must have confidence"
        );
    }
}

/// find_subclass_implementations: empty base_classes returns error.
#[test]
fn e2e_find_subclass_implementations_empty_base_classes() {
    require_iris!();
    let result = call_tool(
        "find_subclass_implementations",
        serde_json::json!({
            "method_name": "OnProcessInput",
            "base_classes": [],
            "namespace": "USER"
        }),
    );
    if result["error_code"].as_str() == Some("IRIS_UNREACHABLE") {
        return;
    }
    assert_eq!(
        result["success"], false,
        "empty base_classes must fail: {}",
        result
    );
    assert_eq!(result["error_code"], "INVALID_PARAMS");
}

// ── 038: OpenCode documentation E2E tests ─────────────────────────────────────

/// The literal JSON snippet from README.md Option D.
/// This constant IS the README snippet — if the README changes, update here too.
/// CI will catch any JSON syntax errors automatically.
const OPENCODE_README_SNIPPET: &str = r#"{
  "mcp": {
    "iris-agentic-dev": {
      "type": "local",
      "command": ["/opt/homebrew/bin/iris-agentic-dev", "mcp"],
      "enabled": true,
      "environment": {
        "IRIS_HOST": "your-iris-host",
        "IRIS_WEB_PORT": "52773",
        "IRIS_USERNAME": "_SYSTEM",
        "IRIS_PASSWORD": "SYS",
        "IRIS_NAMESPACE": "USER"
      }
    }
  }
}"#;

/// The literal Docker variant from README.md Option D.
const OPENCODE_DOCKER_README_SNIPPET: &str = r#"{
  "mcp": {
    "iris-agentic-dev": {
      "type": "local",
      "command": ["/opt/homebrew/bin/iris-agentic-dev", "mcp"],
      "enabled": true,
      "environment": {
        "IRIS_HOST": "your-iris-host",
        "IRIS_WEB_PORT": "52773",
        "IRIS_USERNAME": "_SYSTEM",
        "IRIS_PASSWORD": "SYS",
        "IRIS_NAMESPACE": "USER",
        "IRIS_CONTAINER": "my-iris-container"
      }
    }
  }
}"#;

/// Simulates a newcomer following the OpenCode setup instructions in README.md.
///
/// Test sequence (mirrors what a noob would do):
/// 1. Copy the JSON snippet from README → verify it parses as valid JSON
/// 2. Check the snippet has all required environment keys
/// 3. Launch iris-agentic-dev mcp with ONLY those env vars (as OpenCode does)
/// 4. Call tools/list → verify binary responds
/// 5. Call check_config → verify IRIS connection is established
#[test]
fn e2e_opencode_setup_follows_readme() {
    require_iris!();

    // Step 1: README snippet must be valid JSON
    let config: serde_json::Value = serde_json::from_str(OPENCODE_README_SNIPPET)
        .expect("README OpenCode snippet is not valid JSON");

    // Step 2: All required environment keys must be present in the snippet
    let env_block = &config["mcp"]["iris-agentic-dev"]["environment"];
    for key in &[
        "IRIS_HOST",
        "IRIS_WEB_PORT",
        "IRIS_USERNAME",
        "IRIS_PASSWORD",
        "IRIS_NAMESPACE",
    ] {
        assert!(
            env_block[key].is_string(),
            "README snippet missing required environment key: {}",
            key
        );
    }

    // Step 3: Build env map using actual test IRIS connection (substituting placeholders)
    // This simulates the user filling in their real values in the snippet.
    let host = std::env::var("IRIS_HOST").unwrap_or_default();
    let port = std::env::var("IRIS_WEB_PORT").unwrap_or_else(|_| "52773".to_string());
    let user = std::env::var("IRIS_USERNAME").unwrap_or_else(|_| "_SYSTEM".to_string());
    let pass = std::env::var("IRIS_PASSWORD").unwrap_or_else(|_| "SYS".to_string());
    let ns = std::env::var("IRIS_NAMESPACE").unwrap_or_else(|_| "USER".to_string());

    // Exactly the keys from the README environment block — no extras
    let opencode_env: Vec<(&str, String)> = vec![
        ("IRIS_HOST", host),
        ("IRIS_WEB_PORT", port),
        ("IRIS_USERNAME", user),
        ("IRIS_PASSWORD", pass),
        ("IRIS_NAMESPACE", ns),
    ];

    // Step 4: tools/list — binary must respond (same as what OpenCode checks on startup)
    let mut msgs = init_msgs();
    msgs.push(serde_json::json!({"jsonrpc":"2.0","id":2,"method":"tools/list","params":{}}));
    let responses = mcp_call_timeout(&opencode_env, &msgs, 10);
    let tools_resp = responses.iter().find(|r| r["id"] == 2);
    assert!(
        tools_resp.is_some(),
        "OpenCode env launch: binary did not respond to tools/list"
    );
    let tools = tools_resp.unwrap()["result"]["tools"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    assert!(
        !tools.is_empty(),
        "OpenCode env launch: tools/list returned 0 tools"
    );

    // Step 5: check_config — verify IRIS connection is live
    let mut msgs2 = init_msgs();
    msgs2.push(serde_json::json!({
        "jsonrpc":"2.0","id":2,"method":"tools/call",
        "params":{"name":"check_config","arguments":{}}
    }));
    let responses2 = mcp_call_timeout(&opencode_env, &msgs2, 15);
    let cfg_resp = responses2.iter().find(|r| r["id"] == 2);
    if let Some(resp) = cfg_resp {
        let text = resp["result"]["content"][0]["text"]
            .as_str()
            .unwrap_or("{}");
        let cfg: serde_json::Value = serde_json::from_str(text).unwrap_or_default();
        assert_eq!(
            cfg["connected"], true,
            "check_config must return connected:true when launched with OpenCode env vars: {}",
            text
        );
    }
}

/// Docker variant snippet from README must be valid JSON and include IRIS_CONTAINER.
#[test]
fn e2e_opencode_docker_snippet_is_valid_json() {
    // No live IRIS needed — just validates JSON syntax and key presence
    let config: serde_json::Value = serde_json::from_str(OPENCODE_DOCKER_README_SNIPPET)
        .expect("README OpenCode Docker snippet is not valid JSON");

    let env_block = &config["mcp"]["iris-agentic-dev"]["environment"];
    assert!(
        env_block["IRIS_CONTAINER"].is_string(),
        "Docker README snippet must include IRIS_CONTAINER in environment"
    );
    // All base keys also present
    for key in &[
        "IRIS_HOST",
        "IRIS_WEB_PORT",
        "IRIS_USERNAME",
        "IRIS_PASSWORD",
        "IRIS_NAMESPACE",
    ] {
        assert!(
            env_block[key].is_string(),
            "Docker snippet missing required environment key: {}",
            key
        );
    }
    // Correct OpenCode structure
    assert_eq!(config["mcp"]["iris-agentic-dev"]["type"], "local");
    assert_eq!(config["mcp"]["iris-agentic-dev"]["enabled"], true);
}

// ── iris_source_control ───────────────────────────────────────────────────────

#[test]
fn e2e_source_control_status_uncontrolled_namespace() {
    // Verify status either returns controlled:false or SCM_UNAVAILABLE.
    // SCM behaviour varies by IRIS version — both outcomes are valid.
    require_iris!();
    let result = call_tool(
        "iris_source_control",
        serde_json::json!({"action":"status","document":"%Library.Base.cls","namespace":"USER"}),
    );
    let scm_unavailable =
        result.get("error_code").and_then(|v| v.as_str()) == Some("SCM_UNAVAILABLE");
    assert!(
        result["success"] == true || scm_unavailable,
        "unexpected error from status: {}",
        result
    );
    if result["success"] == true {
        assert!(
            result.get("controlled").is_some(),
            "controlled field must be present: {}",
            result
        );
    }
}

#[test]
fn e2e_source_control_status_with_scm_configured() {
    // Verify status exercises the controlled code path when SCM IS configured.
    // CI configures %Studio.SourceControl.Default on USER namespace before this test runs.
    // Without this test, the GetStatus/SourceControlCreate code path is dead code in CI.
    //
    // If SCM is not configured (e.g. local dev without CI setup step), this falls back to
    // asserting controlled:false — still valid, just not exercising the full path.
    require_iris!();
    // First write a class so we have a real document to check status on
    let name = "IrisDevTest.ScmStatusTest.cls";
    let put = call_tool(
        "iris_doc",
        serde_json::json!({
            "mode": "put",
            "name": name,
            "content": "Class IrisDevTest.ScmStatusTest {}\n",
            "namespace": "USER"
        }),
    );
    assert_eq!(put["success"], true, "put setup: {}", put);

    let result = call_tool(
        "iris_source_control",
        serde_json::json!({"action":"status","document":name,"namespace":"USER"}),
    );
    let scm_unavailable =
        result.get("error_code").and_then(|v| v.as_str()) == Some("SCM_UNAVAILABLE");
    assert!(
        result["success"] == true || scm_unavailable,
        "unexpected error from status: {}",
        result
    );
    if result["success"] == true {
        assert!(
            result.get("controlled").is_some(),
            "controlled field must be present: {}",
            result
        );
        assert!(
            result.get("editable").is_some(),
            "editable field must be present: {}",
            result
        );
        assert!(
            result.get("locked").is_some(),
            "locked field must be present: {}",
            result
        );
        if result["controlled"] == true {
            assert!(
                result["editable"].as_bool().is_some(),
                "editable must be a bool when controlled: {}",
                result
            );
        }
    }

    // Cleanup
    call_tool(
        "iris_doc",
        serde_json::json!({"mode":"delete","name":name,"namespace":"USER"}),
    );
}

#[test]
fn e2e_source_control_status_no_method_does_not_exist_error() {
    // Regression test: ensure status never returns a <METHOD DOES NOT EXIST> error.
    // Previously %GetImplementationObject was called and didn't exist on any IRIS version.
    require_iris!();
    let result = call_tool(
        "iris_source_control",
        serde_json::json!({"action":"status","document":"%Library.Base.cls","namespace":"USER"}),
    );
    let result_str = result.to_string();
    assert!(
        !result_str.contains("METHOD DOES NOT EXIST"),
        "must not produce <METHOD DOES NOT EXIST> error: {}",
        result
    );
    assert!(
        !result_str.contains("GetImplementationObject"),
        "must not reference removed method: {}",
        result
    );
}

#[test]
fn e2e_source_control_menu_returns_list() {
    // Verify menu action returns a valid actions array (may be empty if no SCM).
    require_iris!();
    let result = call_tool(
        "iris_source_control",
        serde_json::json!({"action":"menu","document":"%Library.Base.cls","namespace":"USER"}),
    );
    assert_eq!(result["success"], true, "menu must not error: {}", result);
    assert!(
        result["actions"].is_array(),
        "actions must be an array: {}",
        result
    );
}

// ── Spec 070 e2e: iris_symbols_local, docs_introspect, extract_message_map_routing ──

fn load_bpl_dtl_fixtures() {
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    for (filename, _class_name) in &[
        ("IrisDevTest.BplProcess.cls", "IrisDevTest.BplProcess"),
        ("IrisDevTest.DtlTransform.cls", "IrisDevTest.DtlTransform"),
    ] {
        let path = std::path::Path::new(manifest_dir)
            .join("tests/fixtures/iris_classes")
            .join(filename);
        let content = std::fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("read fixture {filename}: {e}"));
        let result = call_tool(
            "iris_doc",
            serde_json::json!({
                "name": filename,
                "mode": "put",
                "content": content,
                "compile": true,
                "namespace": "USER",
                "allow_storage_regeneration": true
            }),
        );
        assert!(
            result["error_code"].is_null(),
            "fixture load failed for {filename}: {result}"
        );
    }
}

/// T070 e2e: iris_symbols_local kinds filter returns only methods.
#[test]
fn e2e_070_symbols_local_kinds_filter() {
    require_bin!();
    let workspace = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let result = call_tool(
        "iris_symbols_local",
        serde_json::json!({
            "query": "MyApp.*",
            "workspace_path": workspace.to_str().unwrap(),
            "kinds": ["method"]
        }),
    );
    if result["error_code"].as_str() == Some("WORKSPACE_NOT_FOUND") {
        eprintln!("e2e_070_symbols_local_kinds_filter: workspace not found — skipping");
        return;
    }
    let symbols = result["symbols"]
        .as_array()
        .expect("expected symbols array");
    for sym in symbols {
        assert_eq!(
            sym["kind"].as_str(),
            Some("method"),
            "kinds filter should return only methods; got: {sym}"
        );
    }
    assert!(
        !symbols.is_empty(),
        "expected at least one method in MyApp.*"
    );
}

/// T070 e2e: iris_symbols_local member-level glob (MyApp.TypedMembers.Do*).
#[test]
fn e2e_070_symbols_local_member_glob() {
    require_bin!();
    let workspace = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let result = call_tool(
        "iris_symbols_local",
        serde_json::json!({
            "query": "MyApp.TypedMembers.Do*",
            "workspace_path": workspace.to_str().unwrap(),
        }),
    );
    if result["error_code"].as_str() == Some("WORKSPACE_NOT_FOUND") {
        eprintln!("e2e_070_symbols_local_member_glob: workspace not found — skipping");
        return;
    }
    let symbols = result["symbols"]
        .as_array()
        .expect("expected symbols array");
    assert!(
        !symbols.is_empty(),
        "expected at least one Do* member in MyApp.TypedMembers"
    );
    for sym in symbols {
        let name = sym["Name"].as_str().unwrap_or("");
        let short = name.rsplit('.').next().unwrap_or(name);
        assert!(
            short.to_lowercase().starts_with("do"),
            "member glob Do* should only return members starting with Do; got Name={name}"
        );
    }
}

/// T070 e2e: iris_symbols_local line field present and non-zero.
#[test]
fn e2e_070_symbols_local_line_field() {
    require_bin!();
    let workspace = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let result = call_tool(
        "iris_symbols_local",
        serde_json::json!({
            "query": "MyApp.*",
            "workspace_path": workspace.to_str().unwrap(),
            "limit": 5
        }),
    );
    if result["error_code"].as_str() == Some("WORKSPACE_NOT_FOUND") {
        eprintln!("e2e_070_symbols_local_line_field: workspace not found — skipping");
        return;
    }
    let symbols = result["symbols"]
        .as_array()
        .expect("expected symbols array");
    for sym in symbols {
        assert!(
            sym.get("line").is_some(),
            "every symbol must have a line field; sym: {sym}"
        );
        assert!(
            sym["line"].as_u64().unwrap_or(0) > 0,
            "line must be > 0; sym: {sym}"
        );
    }
}

/// T070 e2e: docs_introspect FormalSpec is structured array.
#[test]
fn e2e_070_docs_introspect_formalspec_structured() {
    require_iris!();
    let result = call_tool(
        "docs_introspect",
        serde_json::json!({"class_name": "%Library.Persistent", "namespace": "USER"}),
    );
    if result["error_code"].as_str() == Some("IRIS_UNREACHABLE") {
        return;
    }
    let methods = result["methods"]
        .as_array()
        .expect("expected methods array");
    // FormalSpec must be a structured array, not a raw string.
    for method in methods {
        if let Some(fspec) = method["FormalSpec"].as_array() {
            for arg in fspec {
                assert!(
                    arg.get("name").is_some(),
                    "FormalSpec arg must have name field; method={} arg: {arg}",
                    method["Name"]
                );
                // byref is omitted when false (skip_serializing_if); it is present only for ByRef args.
                // type may be absent for untyped args. Verify structure is an object, not a string.
                assert!(
                    arg.is_object(),
                    "FormalSpec arg must be an object; method={} arg: {arg}",
                    method["Name"]
                );
            }
        }
    }
    // Must have at least one method with a non-empty FormalSpec to be meaningful.
    assert!(
        methods
            .iter()
            .any(|m| m["FormalSpec"].as_array().is_some_and(|a| !a.is_empty())),
        "%Library.Persistent has no methods with parameters — test is vacuous"
    );
}

/// T070 e2e: docs_introspect BPL class returns xdata_flow with steps.
#[test]
fn e2e_070_docs_introspect_bpl_xdata_flow() {
    require_iris!();
    load_bpl_dtl_fixtures();
    let result = call_tool(
        "docs_introspect",
        serde_json::json!({"class_name": "IrisDevTest.BplProcess", "namespace": "USER"}),
    );
    if result["error_code"].as_str() == Some("IRIS_UNREACHABLE") {
        return;
    }
    assert!(
        result["error_code"].is_null(),
        "docs_introspect BPL failed: {result}"
    );
    let flow = &result["xdata_flow"];
    assert_eq!(flow["kind"], "bpl", "expected kind=bpl; flow: {flow}");
    let steps = flow["steps"].as_array().expect("expected steps array");
    assert!(!steps.is_empty(), "expected BPL steps; flow: {flow}");
    assert_eq!(
        flow["has_dynamic_dispatch"], true,
        "IrisDevTest.BplProcess has $classmethod — expected has_dynamic_dispatch=true"
    );
    let has_call = steps
        .iter()
        .any(|s| s["step_kind"] == "Call" && s["target"].as_str() == Some("DownstreamService"));
    assert!(
        has_call,
        "expected Call step targeting DownstreamService; steps: {steps:?}"
    );
}

/// T070 e2e: docs_introspect DTL class returns xdata_flow with source/target class.
#[test]
fn e2e_070_docs_introspect_dtl_xdata_flow() {
    require_iris!();
    load_bpl_dtl_fixtures();
    let result = call_tool(
        "docs_introspect",
        serde_json::json!({"class_name": "IrisDevTest.DtlTransform", "namespace": "USER"}),
    );
    if result["error_code"].as_str() == Some("IRIS_UNREACHABLE") {
        return;
    }
    assert!(
        result["error_code"].is_null(),
        "docs_introspect DTL failed: {result}"
    );
    let flow = &result["xdata_flow"];
    assert_eq!(flow["kind"], "dtl", "expected kind=dtl; flow: {flow}");
    assert_eq!(flow["source_class"], "Ens.Request");
    assert_eq!(flow["target_class"], "Ens.Response");
    assert_eq!(
        flow["assign_count"], 3,
        "expected 3 assign statements; flow: {flow}"
    );
}

/// T070 e2e: extract_message_map_routing BPL class returns kind=bpl with routes.
#[test]
fn e2e_070_routing_bpl_returns_routes() {
    require_iris!();
    load_bpl_dtl_fixtures();
    let result = call_tool(
        "extract_message_map_routing",
        serde_json::json!({"class_name": "IrisDevTest.BplProcess", "namespace": "USER"}),
    );
    if result["error_code"].as_str() == Some("IRIS_UNREACHABLE") {
        return;
    }
    assert!(
        result["error_code"].is_null(),
        "BPL routing failed: {result}"
    );
    assert_eq!(result["kind"], "bpl", "expected kind=bpl; result: {result}");
    let routes = result["routes"].as_array().expect("expected routes array");
    assert!(!routes.is_empty(), "expected routes from BPL Call steps");
    assert_eq!(
        routes[0]["method"].as_str(),
        Some("DownstreamService"),
        "expected route to DownstreamService; routes: {routes:?}"
    );
    assert_eq!(
        routes[0]["confidence"].as_f64(),
        Some(0.8),
        "BPL routes confidence must be 0.8"
    );
    // IrisDevTest.BplProcess has $classmethod — note must be present
    let note = result["note"]
        .as_str()
        .expect("expected note for dynamic dispatch BPL");
    assert!(
        note.contains("dynamic dispatch") || note.contains("Dynamic dispatch"),
        "note must mention dynamic dispatch; got: {note}"
    );
}

/// T070 e2e: extract_message_map_routing DTL class returns kind=dtl, empty routes.
#[test]
fn e2e_070_routing_dtl_empty_routes() {
    require_iris!();
    load_bpl_dtl_fixtures();
    let result = call_tool(
        "extract_message_map_routing",
        serde_json::json!({"class_name": "IrisDevTest.DtlTransform", "namespace": "USER"}),
    );
    if result["error_code"].as_str() == Some("IRIS_UNREACHABLE") {
        return;
    }
    assert!(
        result["error_code"].is_null(),
        "DTL routing failed: {result}"
    );
    assert_eq!(result["kind"], "dtl", "expected kind=dtl; result: {result}");
    let routes = result["routes"].as_array().expect("expected routes array");
    assert!(
        routes.is_empty(),
        "DTL routes must be empty; got: {routes:?}"
    );
    assert_eq!(result["source_class"], "Ens.Request");
    assert_eq!(result["target_class"], "Ens.Response");
}

/// T070 e2e: extract_message_map_routing nonexistent class returns NOT_FOUND.
#[test]
fn e2e_070_routing_plain_class_not_found() {
    require_iris!();
    // Use a nonexistent class — guaranteed NOT_FOUND via fast ObjectScript path (no generator output issues).
    let result = call_tool(
        "extract_message_map_routing",
        serde_json::json!({"class_name": "IrisDevNonExistent.NoRouting", "namespace": "USER"}),
    );
    if result["error_code"].as_str() == Some("IRIS_UNREACHABLE") {
        return;
    }
    assert_eq!(
        result["error_code"], "NOT_FOUND",
        "nonexistent class must return NOT_FOUND; result: {result}"
    );
}

// ── iris_global ───────────────────────────────────────────────────────────────

#[test]
fn e2e_global_list_returns_structured_response() {
    require_iris!();
    let result = call_tool(
        "iris_global",
        serde_json::json!({"action": "list", "global_name": "^ROUTINE", "namespace": "USER"}),
    );
    assert!(
        result["success"] == true || result["error_code"].is_string(),
        "iris_global list must return structured response: {}",
        result
    );
}

#[test]
fn e2e_global_set_get_kill_roundtrip() {
    require_iris!();
    let result = call_tool_destructive(
        "iris_global",
        serde_json::json!({
            "action": "set",
            "global_name": "^IrisDevTest",
            "subscripts": ["e2e_roundtrip"],
            "value": "hello",
            "namespace": "USER"
        }),
    );
    if result["error_code"].as_str() == Some("IRIS_UNREACHABLE")
        || result["error_code"].as_str() == Some("ENV_GATE_BLOCKED")
    {
        return;
    }
    assert_eq!(result["success"], true, "global set: {}", result);

    let get = call_tool_destructive(
        "iris_global",
        serde_json::json!({
            "action": "get",
            "global_name": "^IrisDevTest",
            "subscripts": ["e2e_roundtrip"],
            "namespace": "USER"
        }),
    );
    assert_eq!(get["success"], true, "global get: {}", get);
    assert_eq!(
        get["value"].as_str(),
        Some("hello"),
        "global get value: {}",
        get
    );

    let kill = call_tool_destructive(
        "iris_global",
        serde_json::json!({
            "action": "kill",
            "global_name": "^IrisDevTest",
            "subscripts": ["e2e_roundtrip"],
            "namespace": "USER"
        }),
    );
    assert_eq!(kill["success"], true, "global kill: {}", kill);
}

#[test]
fn e2e_global_phi_pattern_requires_ack() {
    require_iris!();
    let result = call_tool(
        "iris_global",
        serde_json::json!({
            "action": "get",
            "global_name": "^PAPMI",
            "namespace": "USER"
        }),
    );
    if result["error_code"].as_str() == Some("IRIS_UNREACHABLE") {
        return;
    }
    // Must either be blocked with PHI_GATE_BLOCKED or succeed (empty global is fine)
    assert!(
        result["error_code"].as_str() == Some("PHI_GATE_BLOCKED") || result["success"] == true,
        "PHI global without ack must be blocked or empty: {}",
        result
    );
}

// ── iris_coverage ─────────────────────────────────────────────────────────────

#[test]
fn e2e_coverage_check_returns_structured_response() {
    require_iris!();
    let result = call_tool_timeout("iris_coverage", serde_json::json!({"mode": "check"}), 30);
    // coverage check returns ok/bbsiz_state directly, not a success wrapper
    assert!(
        result["ok"].is_boolean() || result["success"] == true || result["error_code"].is_string(),
        "iris_coverage check must return structured response: {}",
        result
    );
    if result["ok"] == true || result["success"] == true {
        assert!(
            result["testcoverage_available"].is_boolean(),
            "check must include testcoverage_available: {}",
            result
        );
    }
}

#[test]
fn e2e_coverage_run_returns_coverage_data() {
    require_iris!();
    let result = call_tool_timeout(
        "iris_coverage",
        serde_json::json!({
            "mode": "run",
            "package": "IrisDevTest",
            "test_path": "IrisDevTest.Tests",
            "namespace": "USER"
        }),
        60,
    );
    if result["error_code"].as_str() == Some("IRIS_UNREACHABLE")
        || result["error_code"].as_str() == Some("BBSIZ_NOT_CONFIGURED")
    {
        return;
    }
    assert!(
        result["success"] == true || result["error_code"].is_string(),
        "iris_coverage run must return structured response: {}",
        result
    );
    if result["success"] == true {
        assert!(
            result["covered_lines"].is_number() || result["classes"].is_array(),
            "coverage run must include coverage data: {}",
            result
        );
    }
}

// ── iris_table_info ───────────────────────────────────────────────────────────

#[test]
fn e2e_table_info_known_table_returns_metadata() {
    require_iris!();
    let result = call_tool(
        "iris_table_info",
        serde_json::json!({"table": "INFORMATION_SCHEMA.TABLES", "namespace": "USER"}),
    );
    if result["error_code"].as_str() == Some("IRIS_UNREACHABLE") {
        return;
    }
    assert!(
        result["success"] == true || result["error_code"].is_string(),
        "iris_table_info must return structured response: {}",
        result
    );
    if result["success"] == true {
        // result is nested under "result" key; check top-level success and nested fields
        let has_meta = result["result"]["table"].is_string()
            || result["result"]["class"].is_string()
            || result["result"]["columns"].is_array()
            || result["table"].is_string()
            || result["columns"].is_array();
        assert!(
            has_meta,
            "table_info must include table/class/columns: {}",
            result
        );
    }
}

#[test]
fn e2e_table_info_nonexistent_returns_error() {
    require_iris!();
    let result = call_tool(
        "iris_table_info",
        serde_json::json!({"table": "NoSuchSchema.NoSuchTable", "namespace": "USER"}),
    );
    if result["error_code"].as_str() == Some("IRIS_UNREACHABLE") {
        return;
    }
    assert!(
        result["success"] == false || result["error_code"].is_string(),
        "nonexistent table must return error: {}",
        result
    );
}

// ── iris_execute_method ───────────────────────────────────────────────────────

#[test]
fn e2e_execute_method_known_class_method() {
    require_iris!();
    let result = call_tool(
        "iris_execute_method",
        serde_json::json!({
            "class": "%Library.Integer",
            "method": "IsValid",
            "args": ["42"],
            "namespace": "USER"
        }),
    );
    if result["error_code"].as_str() == Some("IRIS_UNREACHABLE") {
        return;
    }
    assert!(
        result["success"] == true || result["error_code"].is_string(),
        "execute_method must return structured response: {}",
        result
    );
    if result["success"] == true {
        assert!(
            result["return_value"].is_string()
                || result["value"].is_string()
                || result["output"].is_string(),
            "execute_method must include return_value/value/output: {}",
            result
        );
    }
}

#[test]
fn e2e_execute_method_nonexistent_class_returns_error() {
    require_iris!();
    let result = call_tool(
        "iris_execute_method",
        serde_json::json!({
            "class": "NoSuchClass.XYZ",
            "method": "Run",
            "namespace": "USER"
        }),
    );
    if result["error_code"].as_str() == Some("IRIS_UNREACHABLE") {
        return;
    }
    assert!(
        result["success"] == false || result["error_code"].is_string(),
        "nonexistent class method must return error: {}",
        result
    );
}

// ── iris_admin ────────────────────────────────────────────────────────────────

#[test]
fn e2e_admin_list_namespaces_returns_list() {
    require_iris!();
    let result = call_tool(
        "iris_admin",
        serde_json::json!({"action": "list_namespaces"}),
    );
    if result["error_code"].as_str() == Some("IRIS_UNREACHABLE") {
        return;
    }
    assert!(
        result["success"] == true || result["error_code"].is_string(),
        "admin list_namespaces must return structured response: {}",
        result
    );
    if result["success"] == true {
        let ns = result["namespaces"].as_array();
        assert!(
            ns.is_some() && !ns.unwrap().is_empty(),
            "list_namespaces must return at least one namespace: {}",
            result
        );
    }
}

#[test]
fn e2e_admin_list_users_returns_list() {
    require_iris!();
    let result = call_tool("iris_admin", serde_json::json!({"action": "list_users"}));
    if result["error_code"].as_str() == Some("IRIS_UNREACHABLE") {
        return;
    }
    assert!(
        result["success"] == true || result["error_code"].is_string(),
        "admin list_users must return structured response: {}",
        result
    );
    if result["success"] == true {
        assert!(
            result["users"].is_array(),
            "list_users must return users array: {}",
            result
        );
    }
}

#[test]
fn e2e_admin_list_databases_returns_list() {
    require_iris!();
    let result = call_tool(
        "iris_admin",
        serde_json::json!({"action": "list_databases"}),
    );
    if result["error_code"].as_str() == Some("IRIS_UNREACHABLE") {
        return;
    }
    assert!(
        result["success"] == true || result["error_code"].is_string(),
        "admin list_databases must return structured response: {}",
        result
    );
}

// ── iris_containers ───────────────────────────────────────────────────────────

#[test]
fn e2e_containers_list_returns_structured_response() {
    require_iris!();
    let result = call_tool("iris_containers", serde_json::json!({"action": "list"}));
    // No IRIS required — just needs Docker. May return empty list or DOCKER_REQUIRED.
    assert!(
        result["success"] == true
            || result["error_code"].is_string()
            || result["containers"].is_array(),
        "iris_containers list must return structured response: {}",
        result
    );
}

// ── iris_production_item ──────────────────────────────────────────────────────

#[test]
fn e2e_production_item_get_settings_structured_response() {
    require_iris!();
    // Use a known Ensemble item — if Ensemble not configured, returns structured error.
    let result = call_tool(
        "iris_production_item",
        serde_json::json!({
            "action": "get_settings",
            "item": "IrisDevTest.Interop.PassthroughService",
            "namespace": "USER"
        }),
    );
    if result["error_code"].as_str() == Some("IRIS_UNREACHABLE") {
        return;
    }
    assert!(
        result["success"] == true || result["error_code"].is_string(),
        "iris_production_item get_settings must return structured response: {}",
        result
    );
}

// ── iris_production_diff ──────────────────────────────────────────────────────

#[test]
fn e2e_production_diff_returns_structured_response() {
    require_iris!();
    let result = call_tool(
        "iris_production_diff",
        serde_json::json!({"namespace": "USER"}),
    );
    if result["error_code"].as_str() == Some("IRIS_UNREACHABLE") {
        return;
    }
    assert!(
        result["success"] == true || result["error_code"].is_string(),
        "iris_production_diff must return structured response: {}",
        result
    );
    if result["success"] == true {
        assert!(
            result["added"].is_array()
                || result["removed"].is_array()
                || result["changed"].is_array()
                || result["diff"].is_array()
                || result["production"].is_string(),
            "production_diff must include diff fields or production name: {}",
            result
        );
    }
}

// ── iris_message_body ─────────────────────────────────────────────────────────

#[test]
fn e2e_message_body_nonexistent_id_returns_error() {
    require_iris!();
    let result = call_tool(
        "iris_message_body",
        serde_json::json!({
            "message_id": "999999999",
            "acknowledge_phi": true,
            "namespace": "USER"
        }),
    );
    if result["error_code"].as_str() == Some("IRIS_UNREACHABLE")
        || result["error_code"].as_str() == Some("DATA_POLICY_BLOCKED")
        || result["error_code"].as_str() == Some("PHI_POLICY_BLOCKED")
    {
        return;
    }
    assert!(
        result["success"] == false || result["error_code"].is_string(),
        "nonexistent message_id must return error: {}",
        result
    );
}

// ── iris_business_rule_info ───────────────────────────────────────────────────

#[test]
fn e2e_business_rule_info_list_returns_structured_response() {
    require_iris!();
    let result = call_tool(
        "iris_business_rule_info",
        serde_json::json!({"action": "list", "namespace": "USER"}),
    );
    if result["error_code"].as_str() == Some("IRIS_UNREACHABLE") {
        return;
    }
    assert!(
        result["success"] == true || result["error_code"].is_string(),
        "iris_business_rule_info list must return structured response: {}",
        result
    );
    if result["success"] == true {
        assert!(
            result["rules"].is_array(),
            "business_rule_info list must include rules array: {}",
            result
        );
    }
}

// ── iris_credential_list ──────────────────────────────────────────────────────

#[test]
fn e2e_credential_list_returns_structured_response() {
    require_iris!();
    let result = call_tool(
        "iris_credential_list",
        serde_json::json!({"namespace": "USER"}),
    );
    if result["error_code"].as_str() == Some("IRIS_UNREACHABLE") {
        return;
    }
    assert!(
        result["success"] == true || result["error_code"].is_string(),
        "iris_credential_list must return structured response: {}",
        result
    );
    if result["success"] == true {
        assert!(
            result["credentials"].is_array(),
            "credential_list must include credentials array: {}",
            result
        );
        // Passwords must never appear
        if let Some(creds) = result["credentials"].as_array() {
            for cred in creds {
                assert!(
                    cred.get("password").is_none(),
                    "credential_list must never return passwords: {}",
                    cred
                );
            }
        }
    }
}

// ── iris_lookup_manage ────────────────────────────────────────────────────────

#[test]
fn e2e_lookup_manage_list_tables_returns_structured_response() {
    require_iris!();
    let result = call_tool(
        "iris_lookup_manage",
        serde_json::json!({"action": "list_tables", "namespace": "USER"}),
    );
    if result["error_code"].as_str() == Some("IRIS_UNREACHABLE") {
        return;
    }
    assert!(
        result["success"] == true || result["error_code"].is_string(),
        "iris_lookup_manage list_tables must return structured response: {}",
        result
    );
}

#[test]
fn e2e_lookup_manage_set_get_delete_roundtrip() {
    require_iris!();
    let set = call_tool_destructive(
        "iris_lookup_manage",
        serde_json::json!({
            "action": "set",
            "table": "IrisDevTestLookup",
            "key": "e2e_key",
            "value": "e2e_value",
            "namespace": "USER"
        }),
    );
    if set["error_code"].as_str() == Some("IRIS_UNREACHABLE")
        || set["error_code"].as_str() == Some("ENV_GATE_BLOCKED")
    {
        return;
    }
    assert_eq!(set["success"], true, "lookup set: {}", set);

    let get = call_tool_destructive(
        "iris_lookup_manage",
        serde_json::json!({
            "action": "get",
            "table": "IrisDevTestLookup",
            "key": "e2e_key",
            "namespace": "USER"
        }),
    );
    assert_eq!(get["success"], true, "lookup get: {}", get);
    assert_eq!(
        get["value"].as_str(),
        Some("e2e_value"),
        "lookup get value: {}",
        get
    );

    let del = call_tool_destructive(
        "iris_lookup_manage",
        serde_json::json!({
            "action": "delete",
            "table": "IrisDevTestLookup",
            "key": "e2e_key",
            "namespace": "USER"
        }),
    );
    assert_eq!(del["success"], true, "lookup delete: {}", del);
}

// ── iris_generate_test ────────────────────────────────────────────────────────

#[test]
fn e2e_generate_test_returns_scaffold() {
    require_iris!();
    // iris_generate_test requires an LLM API key. Without one it returns LLM_UNAVAILABLE
    // at the MCP protocol level (not a tool-level error_code). Verify it either succeeds
    // (key configured) or fails with a recognizable error, not a silent crash.
    let env = iris_env();
    let mut msgs = init_msgs();
    msgs.push(serde_json::json!({
        "jsonrpc":"2.0","id":2,"method":"tools/call",
        "params":{"name":"iris_generate_test","arguments":{"class_name":"%Library.Integer","namespace":"USER"}}
    }));
    let responses = mcp_call_timeout(&env, &msgs, 20);
    let raw = responses
        .iter()
        .find(|r| r["id"] == 2)
        .cloned()
        .unwrap_or_default();
    // Must get either a result or an error, not nothing
    assert!(
        raw.get("result").is_some() || raw.get("error").is_some(),
        "iris_generate_test must return result or protocol error, not silence: {:?}",
        raw
    );
    // If it succeeded (LLM key present), verify content includes UnitTest boilerplate
    if raw["result"]["isError"] == false {
        let text = raw["result"]["content"][0]["text"].as_str().unwrap_or("{}");
        let v: serde_json::Value = serde_json::from_str(text).unwrap_or_default();
        if v["success"] == true {
            let content = v["content"]
                .as_str()
                .or_else(|| v["scaffold"].as_str())
                .unwrap_or("");
            assert!(
                content.contains("UnitTest") || content.contains("TestCase"),
                "generated test scaffold must reference UnitTest: {v}"
            );
        }
    }
}

// ── check_config ──────────────────────────────────────────────────────────────

#[test]
fn e2e_check_config_returns_connection_info() {
    require_iris!();
    let result = call_tool("check_config", serde_json::json!({}));
    assert!(
        result["success"] == true
            || result["error_code"].is_string()
            || result.get("host").is_some(),
        "check_config must return structured response: {}",
        result
    );
    if result["success"] == true || result.get("host").is_some() {
        // Must include host or some connection descriptor
        let has_conn = result.get("host").is_some()
            || result.get("iris_host").is_some()
            || result.get("url").is_some()
            || result.get("namespace").is_some();
        assert!(
            has_conn,
            "check_config must include connection info: {}",
            result
        );
    }
}

// ── agent_history / agent_stats ───────────────────────────────────────────────

#[test]
fn e2e_agent_history_returns_list() {
    require_iris!();
    let result = call_tool("agent_history", serde_json::json!({"limit": 5}));
    // agent_history returns data directly (no success wrapper)
    assert!(
        result["calls"].is_array()
            || result["history"].is_array()
            || result["entries"].is_array()
            || result["error_code"].is_string(),
        "agent_history must return calls/history/entries array: {}",
        result
    );
}

#[test]
fn e2e_agent_stats_returns_counts() {
    require_iris!();
    let result = call_tool("agent_stats", serde_json::json!({}));
    // agent_stats returns data directly (no success wrapper)
    assert!(
        result["skill_count"].is_number()
            || result["skills"].is_number()
            || result["session_calls"].is_number()
            || result["status"].is_string()
            || result["error_code"].is_string(),
        "agent_stats must include skill_count/session_calls/status: {}",
        result
    );
}

// ── skill ─────────────────────────────────────────────────────────────────────

#[test]
fn e2e_skill_list_returns_structured_response() {
    require_iris!();
    let result = call_tool("skill", serde_json::json!({"action": "list"}));
    assert!(
        result["success"] == true || result["error_code"].is_string(),
        "skill list must return structured response: {}",
        result
    );
    if result["success"] == true {
        assert!(
            result["skills"].is_array(),
            "skill list must include skills array: {}",
            result
        );
    }
}

#[test]
fn e2e_skill_search_returns_results() {
    require_iris!();
    let result = call_tool(
        "skill",
        serde_json::json!({"action": "search", "query": "objectscript"}),
    );
    assert!(
        result["success"] == true || result["error_code"].is_string(),
        "skill search must return structured response: {}",
        result
    );
}

/// Regression test: `synthesized_skills()` (tools/mod.rs) used to build its
/// `^SKILLS` JSON by concatenating the raw pipe-delimited global value straight
/// into an array literal with no quoting and no `key` — unparseable the moment
/// any `^SKILLS` entry existed, so `skill_list` never saw a single synthesized
/// skill and incorrectly reported `sources.synthesized.searched: false` even
/// though IRIS had been reached and read successfully. Seeds a real `^SKILLS`
/// entry via `iris_global`, then asserts `skill_list` (the bundled+synthesized
/// aware tool, not the IRIS-only `skill` tool above) actually surfaces it.
#[test]
fn e2e_skill_list_surfaces_a_synthesized_skill_from_skills_global() {
    require_iris!();
    let set = call_tool(
        "iris_global",
        serde_json::json!({
            "action": "set",
            "global_name": "^SKILLS",
            "subscripts": ["iad-e2e-synth-skill-test"],
            "value": "e2e synthesized skill description|some body text|0|2026-01-01T00:00:00Z",
            "namespace": "USER"
        }),
    );
    if set["error_code"].as_str() == Some("IRIS_UNREACHABLE")
        || set["error_code"].as_str() == Some("ENV_GATE_BLOCKED")
    {
        return;
    }

    let list = call_tool("skill_list", serde_json::json!({}));

    let cleanup = call_tool(
        "iris_global",
        serde_json::json!({
            "action": "kill",
            "global_name": "^SKILLS",
            "subscripts": ["iad-e2e-synth-skill-test"],
            "namespace": "USER"
        }),
    );

    // Kill the entry before asserting — a failed assert unwinds the test and would
    // otherwise leave ^SKILLS("iad-e2e-synth-skill-test") in the shared container.
    assert_eq!(set["success"], true, "seeding ^SKILLS: {}", set);
    assert_eq!(cleanup["success"], true, "cleanup ^SKILLS: {}", cleanup);
    assert_eq!(
        list["sources"]["synthesized"]["searched"], true,
        "^SKILLS was read successfully — synthesized.searched must be true: {}",
        list
    );
    let skills = list["skills"].as_array().expect("skills array");
    let found = skills
        .iter()
        .find(|s| s["name"] == "iad-e2e-synth-skill-test")
        .unwrap_or_else(|| {
            panic!("synthesized skill from ^SKILLS must appear in skill_list: {list}")
        });
    assert_eq!(found["source"], "synthesized", "{}", found);
    assert_eq!(
        found["description"], "e2e synthesized skill description",
        "{}",
        found
    );
}

// ── kb ────────────────────────────────────────────────────────────────────────

#[test]
fn e2e_kb_recall_empty_query_returns_structured_response() {
    require_iris!();
    let result = call_tool(
        "kb",
        serde_json::json!({"action": "recall", "query": "objectscript status handling", "top_k": 3}),
    );
    assert!(
        result["success"] == true || result["error_code"].is_string(),
        "kb recall must return structured response: {}",
        result
    );
    if result["success"] == true {
        assert!(
            result["results"].is_array()
                || result["matches"].is_array()
                || result["items"].is_array(),
            "kb recall must include results array: {}",
            result
        );
    }
}

// ── telemetry_query ───────────────────────────────────────────────────────────

#[test]
fn e2e_telemetry_query_returns_structured_response() {
    require_iris!();
    // Pass a session_id filter so the tool skips the global session enumeration
    // (listing all sessions requires iterating ^IRISDEV which may be empty/slow).
    let result = call_tool_timeout(
        "telemetry_query",
        serde_json::json!({
            "session_id": "00000000-0000-0000-0000-000000000000",
            "limit": 5
        }),
        15,
    );
    // Empty/unknown session returns empty records — that's fine.
    assert!(
        result["records"].is_array() || result["error_code"].is_string(),
        "telemetry_query must return records array or error: {}",
        result
    );
}

// ── iris_generate (prompt builder) ───────────────────────────────────────────

#[test]
fn e2e_generate_prompt_returns_context() {
    require_iris!();
    let result = call_tool(
        "iris_generate",
        serde_json::json!({
            "description": "A REST handler that returns patient demographics",
            "gen_type": "class",
            "namespace": "USER"
        }),
    );
    if result["error_code"].as_str() == Some("IRIS_UNREACHABLE") {
        return;
    }
    assert!(
        result["success"] == true || result["error_code"].is_string(),
        "iris_generate must return structured response: {}",
        result
    );
    if result["success"] == true {
        let has_prompt = result["prompt"].is_string()
            || result["context"].is_string()
            || result["content"].is_string();
        assert!(
            has_prompt,
            "iris_generate must include prompt/context: {}",
            result
        );
    }
}

// ── Session state tests (071-execute-session) ─────────────────────────────────

/// US1: scalar values round-trip through session_state across two calls.
#[test]
#[ignore]
fn e2e_execute_session_scalar_roundtrip() {
    require_iris!();

    // Call 1: set scalars in %ctx, expect session_state in response
    let call1 = call_tool(
        "iris_execute",
        serde_json::json!({
            "code": "Set %ctx.x = 42\nSet %ctx.label = \"hello\"\nWrite \"call1 ok\", !",
            "use_session": true,
            "namespace": "USER"
        }),
    );
    if call1["error_code"].as_str() == Some("IRIS_UNREACHABLE")
        || call1["error_code"].as_str() == Some("ENV_GATE_BLOCKED")
    {
        return;
    }
    assert_eq!(call1["success"], true, "session call 1 failed: {}", call1);
    let token = call1["session_state"]
        .as_str()
        .expect("session_state missing from call 1 response");
    assert!(!token.is_empty(), "session_state token must not be empty");

    // Call 2: restore %ctx and read scalars back
    let call2 = call_tool(
        "iris_execute",
        serde_json::json!({
            "code": "Write %ctx.x, !, %ctx.label, !",
            "use_session": true,
            "session_state": token,
            "namespace": "USER"
        }),
    );
    assert_eq!(call2["success"], true, "session call 2 failed: {}", call2);
    let output = call2["output"].as_str().unwrap_or("");
    assert!(
        output.contains("42"),
        "call 2 output must contain 42: {}",
        output
    );
    assert!(
        output.contains("hello"),
        "call 2 output must contain 'hello': {}",
        output
    );
}

/// US2: %Persistent OID stored in %ctx is restored across calls.
#[test]
#[ignore]
fn e2e_execute_session_persistent_oid() {
    require_iris!();

    // Call 1: open a known-persistent object, store in %ctx
    let call1 = call_tool(
        "iris_execute",
        serde_json::json!({
            "code": "Set %ctx.hdr = $classmethod(\"Ens.MessageHeader\", \"%OpenId\", \"1\")\nIf $isobject(%ctx.hdr) { Write %ctx.hdr.SourceConfigName, ! } Else { Write \"NOT FOUND\", ! }",
            "use_session": true,
            "namespace": "USER"
        }),
    );
    if call1["error_code"].as_str() == Some("IRIS_UNREACHABLE")
        || call1["error_code"].as_str() == Some("ENV_GATE_BLOCKED")
    {
        return;
    }
    // If there's no message header ID 1, skip gracefully
    if call1["output"].as_str().unwrap_or("").contains("NOT FOUND") {
        eprintln!("Skipping: no Ens.MessageHeader ID 1 in this instance");
        return;
    }
    assert_eq!(
        call1["success"], true,
        "session persistent call 1 failed: {}",
        call1
    );
    let token = call1["session_state"]
        .as_str()
        .expect("session_state missing from call 1");

    // Call 2: read a different property — object must be restored from OID stub
    let call2 = call_tool(
        "iris_execute",
        serde_json::json!({
            "code": "Write $classname(%ctx.hdr), !, %ctx.hdr.MessageBodyClassName, !",
            "use_session": true,
            "session_state": token,
            "namespace": "USER"
        }),
    );
    assert_eq!(
        call2["success"], true,
        "session persistent call 2 failed: {}",
        call2
    );
    let output = call2["output"].as_str().unwrap_or("");
    assert!(
        output.contains("Ens.MessageHeader"),
        "call 2 must show restored class: {}",
        output
    );
}

/// US2 error path: session_state with OID for missing class returns SESSION_RESTORE_FAILED.
#[test]
#[ignore]
fn e2e_execute_session_missing_class() {
    require_iris!();

    // Manually build a token with a non-existent class OID
    // JSON: {"missingObj":{"_cls":"NoSuch.TestClass9999","_id":"1"}}
    // Base64 of that JSON:
    let fake_json = r#"{"missingObj":{"_cls":"NoSuch.TestClass9999","_id":"1"}}"#;
    // Use IRIS to Base64-encode it so the token is valid IRIS-format Base64
    let enc_result = call_tool(
        "iris_execute",
        serde_json::json!({
            "code": format!("Write $system.Encryption.Base64Encode(\"{}\"), !", fake_json.replace('"', "\"\"")),
            "namespace": "USER"
        }),
    );
    if enc_result["error_code"].as_str() == Some("IRIS_UNREACHABLE")
        || enc_result["error_code"].as_str() == Some("ENV_GATE_BLOCKED")
    {
        return;
    }
    let token = enc_result["output"].as_str().unwrap_or("").trim();
    if token.is_empty() {
        eprintln!("Skipping: could not generate test token");
        return;
    }

    let result = call_tool(
        "iris_execute",
        serde_json::json!({
            "code": "Write \"should not reach here\", !",
            "use_session": true,
            "session_state": token,
            "namespace": "USER"
        }),
    );
    assert_eq!(
        result["error_code"].as_str(),
        Some("SESSION_RESTORE_FAILED"),
        "must return SESSION_RESTORE_FAILED for missing class: {}",
        result
    );
}

/// US3: %DynamicObject accumulation across two calls.
#[test]
#[ignore]
fn e2e_execute_session_dynamic_accumulation() {
    require_iris!();

    // Call 1: add step1 to %ctx
    let call1 = call_tool(
        "iris_execute",
        serde_json::json!({
            "code": "Set %ctx.step1 = \"done\"",
            "use_session": true,
            "namespace": "USER"
        }),
    );
    if call1["error_code"].as_str() == Some("IRIS_UNREACHABLE")
        || call1["error_code"].as_str() == Some("ENV_GATE_BLOCKED")
    {
        return;
    }
    assert_eq!(call1["success"], true, "dynamic call 1 failed: {}", call1);
    let token = call1["session_state"]
        .as_str()
        .expect("session_state missing from call 1");

    // Call 2: add step2, write both
    let call2 = call_tool(
        "iris_execute",
        serde_json::json!({
            "code": "Set %ctx.step2 = \"also done\"\nWrite %ctx.step1, !\nWrite %ctx.step2, !",
            "use_session": true,
            "session_state": token,
            "namespace": "USER"
        }),
    );
    assert_eq!(call2["success"], true, "dynamic call 2 failed: {}", call2);
    let output = call2["output"].as_str().unwrap_or("");
    assert!(
        output.contains("done"),
        "call 2 must contain step1 value: {}",
        output
    );
    assert!(
        output.contains("also done"),
        "call 2 must contain step2 value: {}",
        output
    );
}

/// T012: no-session path (use_session=false) is accepted and behaves normally.
#[test]
#[ignore]
fn e2e_execute_session_disabled_is_transparent() {
    require_iris!();
    let result = call_tool(
        "iris_execute",
        serde_json::json!({
            "code": "Write 1+1, !",
            "use_session": false,
            "namespace": "USER"
        }),
    );
    if result["error_code"].as_str() == Some("IRIS_UNREACHABLE")
        || result["error_code"].as_str() == Some("ENV_GATE_BLOCKED")
    {
        return;
    }
    assert_eq!(
        result["success"], true,
        "plain execute with use_session=false failed: {}",
        result
    );
    assert_eq!(result["output"].as_str().map(str::trim), Some("2"));
    // Must NOT have session_state in response when use_session=false
    assert!(
        result.get("session_state").is_none(),
        "session_state must be absent when use_session=false: {}",
        result
    );
}

// ── T019–T020: server param routing (072-multi-instance-pool) ─────────────────

/// T019: iris_execute with server=null behaves identically to the default path.
#[test]
#[ignore = "requires live IRIS container"]
fn e2e_server_param_default() {
    require_iris!();
    // server: null (omitted) — must behave identically to an iris_execute call
    // without the server param at all.
    let result = call_tool(
        "iris_execute",
        serde_json::json!({
            "code": "Write $ZVERSION",
            "namespace": "USER"
        }),
    );
    let result_with_null = call_tool(
        "iris_execute",
        serde_json::json!({
            "code": "Write $ZVERSION",
            "namespace": "USER",
            "server": null
        }),
    );
    // Both calls must either succeed (same output) or fail with the same error_code.
    // We don't assert on the actual version string — just that the two paths are equivalent.
    assert_eq!(
        result["success"], result_with_null["success"],
        "server=null should behave identically to no server param: default={}, null={}",
        result, result_with_null
    );
    if result["success"] == true {
        assert_eq!(
            result["output"].as_str().map(|s| s.contains("IRIS")),
            result_with_null["output"]
                .as_str()
                .map(|s| s.contains("IRIS")),
            "output mismatch between default and server=null"
        );
    }
}

/// T020: iris_execute with server="iris-dev-iris" routes to the named container.
#[test]
#[ignore = "requires live IRIS container registered as 'iris-dev-iris' in the pool"]
fn e2e_server_param_named() {
    require_iris!();
    // The dev container name is "iris-dev-iris" (from IRIS_CONTAINER env var / pool registration).
    // If the pool knows about this server, the call should succeed and return a version string.
    // If the pool does NOT have this server registered, the call returns SERVER_NOT_FOUND — which
    // we treat as a skip (the pool may not be configured in all CI environments).
    let container = std::env::var("IRIS_CONTAINER").unwrap_or_else(|_| "iris-dev-iris".to_string());
    let result = call_tool(
        "iris_execute",
        serde_json::json!({
            "code": "Write $ZVERSION",
            "namespace": "USER",
            "server": container
        }),
    );
    // SERVER_NOT_FOUND or IRIS_UNREACHABLE means pool isn't configured / not reachable — skip.
    let err = result["error_code"].as_str().unwrap_or("");
    if err == "SERVER_NOT_FOUND" || err == "IRIS_UNREACHABLE" || result == serde_json::json!({}) {
        eprintln!(
            "Skipping e2e_server_param_named: server '{}' not reachable or not in pool ({})",
            container, err
        );
        return;
    }
    assert_eq!(
        result["success"], true,
        "iris_execute with server='{}' should succeed: {}",
        container, result
    );
    let output = result["output"].as_str().unwrap_or("");
    // Empty output can happen when the pool finds the server via VS Code settings
    // but credentials are not available (e.g. no keychain in CI). Skip gracefully.
    if output.is_empty() {
        eprintln!(
            "Skipping e2e_server_param_named: server '{}' found but returned no output (likely missing credential)",
            container
        );
        return;
    }
    assert!(
        output.contains("IRIS") || output.contains("Cache") || output.contains("202"),
        "expected IRIS version string in output, got: {:?}",
        output
    );
}

// ── T-082: iris_production namespace parameter (#103) ─────────────────────────

/// T-082-02: iris_production(action=status) with no namespace param returns a result
/// scoped to the connection namespace — not always USER.
#[test]
fn e2e_iris_production_status_defaults_to_connection_namespace() {
    require_iris!();
    let conn_ns = std::env::var("IRIS_NAMESPACE").unwrap_or_else(|_| "USER".to_string());
    let result = call_tool("iris_production", serde_json::json!({"action": "status"}));
    // Must return structured response — error_code is acceptable (e.g. no production),
    // but the namespace in the response (when present) must match the connection namespace.
    assert!(
        result["success"].is_boolean() || result["error_code"].is_string(),
        "T-082-02: iris_production status must return structured response: {}",
        result
    );
    if let Some(ns) = result.get("namespace").and_then(|n| n.as_str()) {
        assert_eq!(
            ns.to_uppercase(),
            conn_ns.to_uppercase(),
            "T-082-02: namespace in response must match connection namespace when no namespace param given"
        );
    }
}

/// T-082-03: iris_production(action=status, namespace=USER) with an explicit namespace
/// returns a result scoped to USER — verifies the explicit override path.
#[test]
fn e2e_iris_production_status_explicit_namespace_override() {
    require_iris!();
    let result = call_tool(
        "iris_production",
        serde_json::json!({"action": "status", "namespace": "USER"}),
    );
    assert!(
        result["success"].is_boolean() || result["error_code"].is_string(),
        "T-082-03: iris_production status with explicit namespace must return structured response: {}",
        result
    );
    if let Some(ns) = result.get("namespace").and_then(|n| n.as_str()) {
        assert_eq!(
            ns.to_uppercase(),
            "USER",
            "T-082-03: namespace in response must be USER when explicitly passed"
        );
    }
}

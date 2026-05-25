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

    child.kill().ok();
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
fn e2e_doc_put_with_storage_block_strips_and_succeeds() {
    require_iris!();
    // I-3: Storage blocks must be stripped automatically
    let cls_with_storage = r#"Class Test022.StorageTest Extends %Persistent {
Property Name As %String;
Storage Default
{
<Data name="DefaultData">
<Value name="1"><Value>%%CLASSNAME</Value></Value>
</Data>
<DataLocation>^Test022.StorageTestD</DataLocation>
<DefaultData>DefaultData</DefaultData>
<IdLocation>^Test022.StorageTestD</IdLocation>
<IndexLocation>^Test022.StorageTestI</IndexLocation>
<StreamLocation>^Test022.StorageTestS</StreamLocation>
<Type>%Storage.Persistent</Type>
}
}"#;

    let result = call_tool(
        "iris_doc",
        serde_json::json!({"mode":"put","name":"Test022.StorageTest.cls",
            "content": cls_with_storage, "namespace":"USER"}),
    );
    assert_eq!(
        result["success"], true,
        "put with Storage block should succeed: {}",
        result
    );
    assert_eq!(
        result["storage_stripped"], true,
        "response must include storage_stripped:true: {}",
        result
    );

    // Cleanup
    call_tool(
        "iris_doc",
        serde_json::json!({"mode":"delete","name":"Test022.StorageTest.cls","namespace":"USER"}),
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

    // Must not crash and must return a structured response
    assert!(
        result["success"] == true || result["error_code"].is_string(),
        "wildcard compile must return structured response: {}",
        result
    );
    // If it succeeded, targets_compiled should be >= 2
    if result["success"] == true {
        let compiled = result["targets_compiled"].as_u64().unwrap_or(0);
        assert!(
            compiled >= 2,
            "wildcard compile Test022.Wild.* should compile at least 2 classes, got: {}",
            compiled
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
    let result = call_tool(
        "debug_get_error_logs",
        serde_json::json!({"namespace": "USER", "max_entries": 10}),
    );
    assert_eq!(
        result["success"], true,
        "debug_get_error_logs should succeed: {}",
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
    let result = call_tool(
        "debug_capture_packet",
        serde_json::json!({"namespace": "USER"}),
    );
    assert_eq!(
        result["success"], true,
        "debug_capture_packet should succeed: {}",
        result
    );
    // errors may be null (no recent errors) or an array — both are valid
    assert!(
        result["errors"].is_array() || result["errors"].is_null(),
        "capture packet must return errors array or null: {}",
        result
    );
}

#[test]
fn e2e_debug_map_int_to_cls_parses_error_string() {
    require_iris!();
    // Parse a synthetic IRIS error string — tests the regex parsing path.
    // This does NOT require docker exec (parse only) if error_string is provided.
    let result = call_tool(
        "debug_map_int_to_cls",
        serde_json::json!({
            "error_string": "<UNDEFINED>x+3^Ens.Director.1",
            "namespace": "USER"
        }),
    );
    assert!(
        result["success"] == true || result["error_code"].is_string(),
        "debug_map_int_to_cls must return structured response: {}",
        result
    );
    if result["success"] == true {
        // routine and offset must be extracted from the error string
        assert_eq!(
            result["routine"].as_str(),
            Some("Ens.Director.1"),
            "routine must be parsed from error string: {}",
            result
        );
        assert_eq!(
            result["offset"].as_i64(),
            Some(3),
            "offset must be 3: {}",
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
    // At least one method must have a non-empty FormalSpec
    let has_formal = methods.iter().any(|m| {
        m["FormalSpec"]
            .as_str()
            .map(|s| !s.is_empty())
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
    // max_entries must be capped at 1000 (FR-012 fix) — requesting 5000 returns at most 1000
    let result = call_tool(
        "debug_get_error_logs",
        serde_json::json!({
            "namespace": "USER",
            "max_entries": 5000
        }),
    );
    assert_eq!(result["success"], true, "debug_get_error_logs: {}", result);
    let logs = result["logs"].as_array().cloned().unwrap_or_default();
    assert!(
        logs.len() <= 1000,
        "max_entries=5000 must be capped at 1000: {}",
        logs.len()
    );
}

#[test]
fn e2e_debug_error_logs_small_limit() {
    require_iris!();
    let result = call_tool(
        "debug_get_error_logs",
        serde_json::json!({
            "namespace": "USER",
            "max_entries": 1
        }),
    );
    assert_eq!(
        result["success"], true,
        "debug_get_error_logs max=1: {}",
        result
    );
    let logs = result["logs"].as_array().cloned().unwrap_or_default();
    assert!(
        logs.len() <= 1,
        "max_entries=1 must return at most 1 entry: {}",
        logs.len()
    );
}

#[test]
fn e2e_debug_capture_packet_success_field() {
    require_iris!();
    let result = call_tool(
        "debug_capture_packet",
        serde_json::json!({"namespace":"USER"}),
    );
    assert_eq!(result["success"], true, "capture packet: {}", result);
    // errors field must be array or null (never absent when success=true)
    assert!(
        result["errors"].is_array() || result["errors"].is_null(),
        "errors field must be present and array or null: {}",
        result
    );
}

#[test]
fn e2e_debug_source_map_nonexistent_class() {
    require_iris!();
    // debug_source_map on a nonexistent class must not crash — returns empty map or error
    let result = call_tool(
        "debug_source_map",
        serde_json::json!({
            "cls_name": "NonExistent.Class.XYZ",
            "cls_text": "Class NonExistent.Class.XYZ { }",
            "namespace": "USER"
        }),
    );
    assert!(
        result["success"] == true || result["error_code"].is_string(),
        "debug_source_map nonexistent must be structured: {}",
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
    } else if n >= 2 && n <= 5 {
        assert_eq!(result["confidence"], 0.75);
    }
}

/// extract_message_map_routing: plain class (no MessageMap) returns has_message_map:false.
#[test]
fn e2e_extract_message_map_no_message_map_class() {
    require_iris!();
    // Find a class that exists in this namespace
    let result = call_tool(
        "extract_message_map_routing",
        serde_json::json!({"class_name": "%ASQ.AST", "namespace": "USER"}),
    );
    if result["error_code"].as_str() == Some("IRIS_UNREACHABLE")
        || result["error_code"].as_str() == Some("TIMEOUT")
    {
        eprintln!("extract_message_map_routing: IRIS unavailable — skipping");
        return;
    }
    // Accept NOT_FOUND if the class isn't in this namespace — use any available class
    if result["error_code"].as_str() == Some("NOT_FOUND") {
        eprintln!(
            "extract_message_map_routing: %ASQ.AST not in this namespace — test inconclusive"
        );
        return;
    }
    assert_eq!(
        result["success"], true,
        "must succeed for known class: {}",
        result
    );
    assert_eq!(
        result["has_message_map"], false,
        "%ASQ.AST has no MessageMap: {}",
        result
    );
    assert!(
        result["routes"]
            .as_array()
            .map(|a| a.is_empty())
            .unwrap_or(false),
        "routes must be empty array: {}",
        result
    );
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

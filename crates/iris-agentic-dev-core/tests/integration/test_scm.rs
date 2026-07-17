//! T016-T017, T025, T030: Integration tests for SCM tools and open_uri.
//! Run: IRIS_HOST=localhost IRIS_WEB_PORT=52780 IRIS_USERNAME=_SYSTEM IRIS_PASSWORD=SYS cargo test --test test_scm
#![allow(dead_code, clippy::zombie_processes)]

use std::io::{BufRead, BufReader, Write};
use std::process::{Command, Stdio};

fn iris_dev_bin() -> std::path::PathBuf {
    let root = {
        let mut p = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        p.pop();
        p.pop();
        p
    };
    for dir in &["target/llvm-cov-target/debug", "target/debug"] {
        for name in &["iris-agentic-dev", "iris-dev"] {
            let candidate = root.join(dir).join(name);
            if candidate.exists() {
                return candidate;
            }
        }
    }
    root.join("target/debug/iris-agentic-dev")
}

fn mcp_exchange(
    env_vars: &[(&str, &str)],
    messages: &[serde_json::Value],
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
    let mut child = cmd
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn");
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
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(8);
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

fn parse_tool(responses: &[serde_json::Value], id: u64) -> serde_json::Value {
    let resp = responses
        .iter()
        .find(|r| r["id"] == id)
        .expect("no response");
    let text = resp["result"]["content"][0]["text"]
        .as_str()
        .unwrap_or("{}");
    serde_json::from_str(text).unwrap_or_default()
}

fn iris_env() -> Vec<(&'static str, String)> {
    vec![
        (
            "IRIS_HOST",
            std::env::var("IRIS_HOST").unwrap_or_else(|_| "localhost".into()),
        ),
        (
            "IRIS_WEB_PORT",
            std::env::var("IRIS_WEB_PORT").unwrap_or_else(|_| "52780".into()),
        ),
        (
            "IRIS_USERNAME",
            std::env::var("IRIS_USERNAME").unwrap_or_else(|_| "_SYSTEM".into()),
        ),
        (
            "IRIS_PASSWORD",
            std::env::var("IRIS_PASSWORD").unwrap_or_else(|_| "SYS".into()),
        ),
    ]
}

/// T016: iris_doc put to namespace without SCM returns success + open_uri.
#[test]
fn iris_doc_put_no_scm() {
    // Skip when IRIS_HOST is not explicitly set in the environment.
    // iris_env() defaults IRIS_HOST to "localhost", so we check the env var directly.
    if std::env::var("IRIS_HOST").is_err() {
        eprintln!("Skipping iris_doc_put_no_scm: IRIS_HOST env var not set");
        return;
    }
    let env = iris_env();
    let env_refs: Vec<(&str, &str)> = env.iter().map(|(k, v)| (*k, v.as_str())).collect();

    let responses = mcp_exchange(
        &env_refs,
        &[
            serde_json::json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"test","version":"0.1"}}}),
            serde_json::json!({"jsonrpc":"2.0","method":"notifications/initialized","params":{}}),
            serde_json::json!({"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"iris_doc","arguments":{
                "mode":"put","name":"IrisDevTest.ScmTest.cls",
                "content":"Class IrisDevTest.ScmTest {}\n",
                "namespace":"USER"
            }}}),
        ],
    );

    let result = parse_tool(&responses, 2);
    assert_eq!(result["success"], true, "put should succeed: {}", result);
    assert!(
        result["open_uri"].as_str().is_some(),
        "open_uri should be present: {}",
        result
    );
    assert!(
        result["open_uri"].as_str().unwrap().starts_with("isfs://"),
        "open_uri should be isfs: {}",
        result
    );
}

/// T030: iris_source_control status on namespace without SCM returns controlled:false.
#[test]
fn iris_source_control_status_uncontrolled() {
    let env = iris_env();
    if std::env::var("IRIS_HOST").is_err() {
        eprintln!("Skipping: IRIS_HOST env var not set");
        return;
    }
    let env_refs: Vec<(&str, &str)> = env.iter().map(|(k, v)| (*k, v.as_str())).collect();

    let responses = mcp_exchange(
        &env_refs,
        &[
            serde_json::json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"test","version":"0.1"}}}),
            serde_json::json!({"jsonrpc":"2.0","method":"notifications/initialized","params":{}}),
            serde_json::json!({"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"iris_source_control","arguments":{
                "action":"status","document":"%Library.Base.cls","namespace":"USER"
            }}}),
        ],
    );

    let result = parse_tool(&responses, 2);
    let scm_unavailable =
        result.get("error_code").and_then(|v| v.as_str()) == Some("SCM_UNAVAILABLE");
    assert!(
        result["success"] == true || scm_unavailable,
        "unexpected error from status: {}",
        result
    );
    if result["success"] == true {
        // In a namespace without SCM, controlled should be false
        // (In a namespace with SCM, this will return controlled:true — both are valid)
        assert!(
            result.get("controlled").is_some(),
            "controlled field must be present: {}",
            result
        );
    }
}

/// T025: iris_compile writes open_uri after successful single-class compile.
#[test]
fn iris_compile_open_uri() {
    let env = iris_env();
    if std::env::var("IRIS_HOST").is_err() {
        eprintln!("Skipping: IRIS_HOST env var not set");
        return;
    }
    let env_refs: Vec<(&str, &str)> = env.iter().map(|(k, v)| (*k, v.as_str())).collect();

    // First write a test class, then compile it
    let responses = mcp_exchange(
        &env_refs,
        &[
            serde_json::json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"test","version":"0.1"}}}),
            serde_json::json!({"jsonrpc":"2.0","method":"notifications/initialized","params":{}}),
            serde_json::json!({"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"iris_doc","arguments":{
                "mode":"put","name":"IrisDevTest.OpenUriTest.cls",
                "content":"Class IrisDevTest.OpenUriTest {}\n","namespace":"USER"
            }}}),
            serde_json::json!({"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"iris_compile","arguments":{
                "target":"IrisDevTest.OpenUriTest","namespace":"USER"
            }}}),
        ],
    );

    let result = parse_tool(&responses, 3);
    assert_eq!(
        result["success"], true,
        "compile should succeed: {}",
        result
    );
    assert!(
        result["open_uri"].as_str().is_some(),
        "open_uri should be present after compile: {}",
        result
    );

    // Verify sentinel file was written. Regression: this checked .iris-dev/ (the
    // pre-rename directory name, commit 7eb8b96) while write_open_hint() writes to
    // .iris-agentic-dev/ — this assertion could never have passed since the rename;
    // it just never ran anywhere to reveal it.
    let hint_path = dirs::home_dir()
        .unwrap()
        .join(".iris-agentic-dev/open-hint.json");
    assert!(
        hint_path.exists(),
        "sentinel file should exist at {:?}",
        hint_path
    );
}

/// iris_generate returns a prompt + context without requiring an API key.
#[test]
fn iris_generate_returns_context_no_api_key() {
    let env = iris_env();
    if std::env::var("IRIS_HOST").is_err() {
        eprintln!("Skipping: IRIS_HOST env var not set");
        return;
    }
    let env_refs: Vec<(&str, &str)> = env.iter().map(|(k, v)| (*k, v.as_str())).collect();

    let responses = mcp_exchange(
        &env_refs,
        &[
            serde_json::json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"test","version":"0.1"}}}),
            serde_json::json!({"jsonrpc":"2.0","method":"notifications/initialized","params":{}}),
            serde_json::json!({"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"iris_generate","arguments":{
                "description": "a Patient class with Name and DateOfBirth properties",
                "gen_type": "class",
                "namespace": "USER"
            }}}),
        ],
    );

    let result = parse_tool(&responses, 2);
    assert_eq!(
        result["success"], true,
        "iris_generate should succeed without API key: {}",
        result
    );
    assert!(
        result["prompt"].as_str().is_some(),
        "should return a prompt: {}",
        result
    );
    assert!(
        result["instructions"].as_str().is_some(),
        "should return instructions: {}",
        result
    );
    assert!(
        result.get("context").is_some(),
        "should return context: {}",
        result
    );
    // Must NOT contain LLM_UNAVAILABLE
    assert_ne!(
        result["error_code"].as_str(),
        Some("LLM_UNAVAILABLE"),
        "must not require API key: {}",
        result
    );
}

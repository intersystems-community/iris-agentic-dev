// E2E tests for live connection hot-reload and check_config against iris-dev-iris.
// All tests are #[ignore] — run with:
//   IRIS_HOST=localhost IRIS_WEB_PORT=52780 cargo test --test test_live_reload_e2e -- --ignored --nocapture

#![allow(dead_code)]

use std::io::{BufRead, BufReader, Write};
use std::process::{Command, Stdio};

fn iris_dev_bin() -> std::path::PathBuf {
    if let Ok(path) = std::env::var("IRIS_DEV_BIN") {
        let p = std::path::PathBuf::from(path);
        if p.exists() {
            return p;
        }
    }
    let workspace_root = {
        let mut p = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        p.pop();
        p.pop();
        p
    };
    for target_subdir in [
        "target/debug/iris-agentic-dev",
        "target/release/iris-agentic-dev",
        "target/llvm-cov-target/debug/iris-agentic-dev",
        "target/llvm-cov-target/release/iris-agentic-dev",
    ] {
        let candidate = workspace_root.join(target_subdir);
        if candidate.exists() {
            return candidate;
        }
    }
    workspace_root.join("target/debug/iris-agentic-dev")
}

fn iris_host() -> String {
    std::env::var("IRIS_HOST").unwrap_or_default()
}

/// The container to select via iris_containers(action=select)/iris_select_container.
/// Was hardcoded to "iris-dev-iris" (a personal dev-machine convention) — broke
/// immediately in CI, where the container is named iris-e2e.
fn select_container_name() -> String {
    std::env::var("IRIS_CONTAINER").unwrap_or_else(|_| "iris-dev-iris".to_string())
}

/// One step of a scripted MCP session.
///
/// The reload edge cases need the config file to change *between* two requests to the same running
/// server, which the message-list helper cannot express — it builds every message before spawning.
/// `Do` is that seam.
enum Step<'a> {
    Send(serde_json::Value),
    Do(&'a dyn Fn()),
}

fn mcp_call_with_toml(
    toml_dir: Option<&std::path::Path>,
    extra_env: &[(&str, &str)],
    messages: &[serde_json::Value],
) -> Vec<serde_json::Value> {
    let steps: Vec<Step> = messages.iter().cloned().map(Step::Send).collect();
    mcp_session(toml_dir, extra_env, &steps)
}

fn mcp_session(
    toml_dir: Option<&std::path::Path>,
    extra_env: &[(&str, &str)],
    steps: &[Step],
) -> Vec<serde_json::Value> {
    let bin = iris_dev_bin();
    if !bin.exists() {
        return vec![];
    }
    let mut cmd = Command::new(&bin);
    cmd.args(["mcp"]);
    cmd.env_remove("IRIS_CONTAINER");
    for key in &[
        "IRIS_HOST",
        "IRIS_WEB_PORT",
        "IRIS_USERNAME",
        "IRIS_PASSWORD",
    ] {
        if let Ok(v) = std::env::var(key) {
            cmd.env(key, v);
        }
    }
    if let Some(dir) = toml_dir {
        cmd.env("OBJECTSCRIPT_WORKSPACE", dir);
    }
    for (k, v) in extra_env {
        cmd.env(k, v);
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

    for step in steps {
        let msg = match step {
            Step::Do(f) => {
                f();
                continue;
            }
            Step::Send(m) => m,
        };
        stdin
            .write_all((serde_json::to_string(msg).unwrap() + "\n").as_bytes())
            .unwrap();
        stdin.flush().unwrap();
        if msg.get("id").is_some() {
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
            loop {
                std::thread::sleep(std::time::Duration::from_millis(100));
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
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
    }
    child.kill().ok();
    child.wait().ok();
    results
}

fn init_msgs() -> Vec<serde_json::Value> {
    vec![
        serde_json::json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"e2e","version":"0.1"}}}),
        serde_json::json!({"jsonrpc":"2.0","method":"notifications/initialized","params":{}}),
    ]
}

fn tool_result(responses: &[serde_json::Value], id: u64) -> serde_json::Value {
    let resp = responses
        .iter()
        .find(|r| r["id"] == id)
        .cloned()
        .unwrap_or_default();
    let text = resp["result"]["content"]
        .as_array()
        .and_then(|a| a.first())
        .and_then(|c| c["text"].as_str())
        .unwrap_or("{}");
    serde_json::from_str(text).unwrap_or_default()
}

fn call_tool(tool: &str, args: serde_json::Value) -> serde_json::Value {
    call_tool_with_toml(None, &[], tool, args)
}

fn call_tool_with_toml(
    toml_dir: Option<&std::path::Path>,
    extra_env: &[(&str, &str)],
    tool: &str,
    args: serde_json::Value,
) -> serde_json::Value {
    let mut msgs = init_msgs();
    msgs.push(serde_json::json!({
        "jsonrpc":"2.0","id":2,"method":"tools/call",
        "params":{"name":tool,"arguments":args}
    }));
    let responses = mcp_call_with_toml(toml_dir, extra_env, &msgs);
    tool_result(&responses, 2)
}

/// T022: Config file pointing to unreachable container — next call returns IRIS_UNREACHABLE (not crash).
#[test]
#[ignore]
fn test_e2e_unreachable_container_returns_iris_unreachable() {
    if iris_host().is_empty() {
        eprintln!("Skipping: IRIS_HOST not set");
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    // Write a .iris-dev.toml pointing to a nonexistent container
    std::fs::write(
        dir.path().join(".iris-agentic-dev.toml"),
        "container = \"nonexistent-container-xyz\"\n",
    )
    .unwrap();
    let result = call_tool_with_toml(
        Some(dir.path()),
        &[],
        "iris_execute",
        serde_json::json!({"code": "write $ZVersion,!"}),
    );
    eprintln!(
        "T022 result: {}",
        serde_json::to_string_pretty(&result).unwrap()
    );
    // The spec says graceful degradation — no crash (no panic, no server crash).
    // If IRIS_HOST/IRIS_WEB_PORT env vars are set, the connection may succeed via those
    // even if the container config is wrong. The key check: the process did not crash
    // and returned a valid JSON response.
    assert!(
        result.is_object(),
        "should return a valid JSON object response (no crash)"
    );
    // Verify the session didn't produce a panic/fatal error
    assert!(
        result.get("success").is_some(),
        "response should have a 'success' field"
    );
}

/// T029: iris_select_container with iris-dev-iris → check_config shows iris_select_container source.
#[test]
#[ignore]
fn test_e2e_select_container_updates_check_config() {
    if iris_host().is_empty() {
        eprintln!("Skipping: IRIS_HOST not set");
        return;
    }
    // In a single MCP session: select container, then check_config
    // iris_select_container consolidated into iris_containers(action=select) — FR-007.
    let mut msgs = init_msgs();
    msgs.push(serde_json::json!({
        "jsonrpc":"2.0","id":2,"method":"tools/call",
        "params":{"name":"iris_containers","arguments":{"action":"select","name":select_container_name(),"namespace":"USER"}}
    }));
    msgs.push(serde_json::json!({
        "jsonrpc":"2.0","id":3,"method":"tools/call",
        "params":{"name":"check_config","arguments":{}}
    }));
    let responses = mcp_call_with_toml(None, &[], &msgs);
    let select_result = tool_result(&responses, 2);
    let config_result = tool_result(&responses, 3);
    eprintln!(
        "T029 select result: {}",
        serde_json::to_string_pretty(&select_result).unwrap()
    );
    eprintln!(
        "T029 check_config result: {}",
        serde_json::to_string_pretty(&config_result).unwrap()
    );

    assert_eq!(
        select_result["switched"], true,
        "iris_select_container should return switched:true"
    );
    assert_eq!(
        config_result["connection_source"], "iris_select_container",
        "check_config should show iris_select_container source"
    );
}

/// T030: iris_select_container → iris_execute returns output from the new container.
#[test]
#[ignore]
fn test_e2e_select_container_execute_uses_new_connection() {
    if iris_host().is_empty() {
        eprintln!("Skipping: IRIS_HOST not set");
        return;
    }
    let mut msgs = init_msgs();
    msgs.push(serde_json::json!({
        "jsonrpc":"2.0","id":2,"method":"tools/call",
        "params":{"name":"iris_containers","arguments":{"action":"select","name":select_container_name(),"namespace":"USER"}}
    }));
    msgs.push(serde_json::json!({
        "jsonrpc":"2.0","id":3,"method":"tools/call",
        "params":{"name":"iris_execute","arguments":{"code":"write $ZVersion,!","namespace":"USER"}}
    }));
    let responses = mcp_call_with_toml(None, &[], &msgs);
    let exec_result = tool_result(&responses, 3);
    eprintln!(
        "T030 exec result: {}",
        serde_json::to_string_pretty(&exec_result).unwrap()
    );

    // Should get output from IRIS (not IRIS_UNREACHABLE)
    assert_eq!(
        exec_result["success"], true,
        "iris_execute should succeed after container switch"
    );
    assert!(
        exec_result["output"]
            .as_str()
            .unwrap_or("")
            .contains("IRIS"),
        "output should contain IRIS version string"
    );
}

/// T037: check_config after session start returns all required fields.
#[test]
#[ignore]
fn test_e2e_check_config_returns_all_fields() {
    if iris_host().is_empty() {
        eprintln!("Skipping: IRIS_HOST not set");
        return;
    }
    let result = call_tool("check_config", serde_json::json!({}));
    eprintln!(
        "T037 result: {}",
        serde_json::to_string_pretty(&result).unwrap()
    );

    // Must contain all 9 required fields
    assert!(result.get("connected").is_some(), "missing: connected");
    assert!(result.get("host").is_some(), "missing: host");
    assert!(result.get("port").is_some(), "missing: port");
    assert!(result.get("namespace").is_some(), "missing: namespace");
    assert!(
        result.get("container").is_some(),
        "missing: container (may be null)"
    );
    assert!(
        result.get("config_file").is_some(),
        "missing: config_file (may be null)"
    );
    assert!(
        result.get("config_loaded_at").is_some(),
        "missing: config_loaded_at"
    );
    assert!(
        result.get("iris_version").is_some(),
        "missing: iris_version (may be null)"
    );
    // 085 T026: this was `result.get("write_tools_enabled").is_some()` — presence only, which a
    // permanently hardcoded `true` passes. Assert the value and the input that decided it, and that
    // the two are coherent. The value-pinning case (a config file declaring `false` must report
    // `false`) is `test_e2e_gate_value_follows_the_config_file` below (FR-028).
    let write_enabled = result["write_tools_enabled"]
        .as_bool()
        .unwrap_or_else(|| panic!("write_tools_enabled must be a bool, got: {result}"));
    let write_source = result["write_tools_source"]
        .as_str()
        .unwrap_or_else(|| panic!("write_tools_source must be a non-null string, got: {result}"));
    assert!(
        GATE_SOURCES.contains(&write_source),
        "write_tools_source {write_source:?} is not a GateSource wire value; expected one of \
         {GATE_SOURCES:?}"
    );
    // This session declares no gate — no config file on the search path sets one, and the helper
    // forwards only the connection env vars — so the decision comes from the inference chain. USER
    // on the dev container is neither a production namespace nor `SystemMode = Live`, and the
    // documented outcome for that is writes on. If the chain ever silently starts refusing here,
    // this fails instead of reporting a bool that happens to exist.
    if write_source.starts_with("inferred") {
        assert!(
            write_enabled,
            "USER on a non-Live instance must resolve to writes on, decided by {write_source}: \
             {result}"
        );
    }
    // The invariant, read off the operator-facing report rather than the internals.
    if result["destructive_tools_enabled"] == serde_json::json!(true) {
        assert!(
            write_enabled,
            "the destructive tier cannot be on with writes off: {result}"
        );
    }
    assert!(
        result.get("connection_source").is_some(),
        "missing: connection_source"
    );

    // Must not return IRIS_UNREACHABLE
    assert_ne!(result["error_code"], "IRIS_UNREACHABLE");

    let valid_sources = [
        "config_file",
        "env_vars",
        "iris_select_container",
        "auto_discovered",
    ];
    let src = result["connection_source"].as_str().unwrap_or("");
    assert!(
        valid_sources.contains(&src),
        "connection_source '{}' must be one of {:?}",
        src,
        valid_sources
    );
}

// ---------------------------------------------------------------------------
// 085: gate value follows the config file, and the reload edge cases
// ---------------------------------------------------------------------------

/// Every `GateSource` wire value. Hardcoded rather than derived so that renaming a variant fails
/// here instead of silently widening what the tests accept.
const GATE_SOURCES: &[&str] = &[
    "operator_env",
    "config_file",
    "legacy_allow_prod",
    "inferred_system_mode",
    "inferred_namespace",
    "inferred_default",
    "fail_closed",
];

/// A gate-declaring config, plus enough connection detail to reach the dev container.
fn gate_toml(write_enabled: bool) -> String {
    format!(
        "host = \"{}\"\nweb_port = {}\nnamespace = \"USER\"\nusername = \"_SYSTEM\"\n\
         password = \"SYS\"\nwrite_tools_enabled = {write_enabled}\n",
        iris_host(),
        std::env::var("IRIS_WEB_PORT").unwrap_or_else(|_| "52780".into()),
    )
}

/// Write the file and push its mtime forward.
///
/// `ConfigWatcher::has_changed` compares `new > old`, so two writes inside one filesystem timestamp
/// tick look like no change and the test would measure clock resolution instead of the reload.
fn write_and_bump(path: &std::path::Path, body: &str, tick: u64) {
    std::fs::write(path, body).expect("write config");
    let f = std::fs::OpenOptions::new()
        .write(true)
        .open(path)
        .expect("reopen config");
    f.set_modified(std::time::SystemTime::now() + std::time::Duration::from_secs(10 * (tick + 1)))
        .expect("bump mtime");
}

fn check_config_msg(id: u64) -> serde_json::Value {
    serde_json::json!({
        "jsonrpc":"2.0","id":id,"method":"tools/call",
        "params":{"name":"check_config","arguments":{}}
    })
}

/// A write attempt that leaves a probe global behind if — and only if — it was not refused.
fn global_set_msg(id: u64, subscript: &str) -> serde_json::Value {
    serde_json::json!({
        "jsonrpc":"2.0","id":id,"method":"tools/call",
        "params":{"name":"iris_global","arguments":{
            "action":"set","global_name":"IADGate085Reload","subscripts":[subscript],"value":"1"
        }}
    })
}

/// T026 (FR-028). The value-pinning half: a config file declaring `write_tools_enabled = false`
/// must be reported as `false`, sourced to the file, and must actually refuse a write.
///
/// `test_e2e_check_config_returns_all_fields` asserted only that the key existed, so a hardcoded
/// `true` passed it. This is the case that hardcoding cannot survive.
#[test]
#[ignore]
fn test_e2e_gate_value_follows_the_config_file() {
    if iris_host().is_empty() {
        eprintln!("Skipping: IRIS_HOST not set");
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join(".iris-agentic-dev.toml"), gate_toml(false)).unwrap();

    let mut msgs = init_msgs();
    msgs.push(check_config_msg(2));
    msgs.push(global_set_msg(3, "t026"));
    let responses = mcp_call_with_toml(Some(dir.path()), &[], &msgs);
    let cfg = tool_result(&responses, 2);
    let write = tool_result(&responses, 3);

    assert_eq!(
        cfg["write_tools_enabled"],
        serde_json::json!(false),
        "the file declared false, so check_config must report false: {cfg}"
    );
    assert_eq!(
        cfg["write_tools_source"],
        serde_json::json!("config_file"),
        "the file decided the gate, so the source must name the file: {cfg}"
    );
    assert_eq!(
        write["error_code"],
        serde_json::json!("WRITE_TOOLS_DISABLED"),
        "the report and the enforcement have to be the same gate: {write}"
    );
}

/// T028 (085 edge case 2). The config file is deleted mid-session: the gate falls back to the
/// documented default instead of keeping a value from a file that no longer exists.
///
/// The watcher reports a deletion as "not changed" — deliberately, so it can still detect the file
/// coming back — so before 085 a deleted file left the last declared gate in force indefinitely.
/// That is a stale declaration surviving the thing that declared it, which is the same class of
/// defect as the env-var latch.
#[test]
#[ignore]
fn test_e2e_deleted_config_falls_back_to_the_documented_default() {
    if iris_host().is_empty() {
        eprintln!("Skipping: IRIS_HOST not set");
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let cfg_path = dir.path().join(".iris-agentic-dev.toml");
    std::fs::write(&cfg_path, gate_toml(false)).unwrap();
    let to_delete = cfg_path.clone();

    let mut steps: Vec<Step> = init_msgs().into_iter().map(Step::Send).collect();
    steps.push(Step::Send(check_config_msg(2)));
    let del = move || {
        std::fs::remove_file(&to_delete).expect("delete config");
    };
    steps.push(Step::Do(&del));
    steps.push(Step::Send(check_config_msg(3)));
    let responses = mcp_session(Some(dir.path()), &[], &steps);
    let before = tool_result(&responses, 2);
    let after = tool_result(&responses, 3);
    eprintln!("T028 delete — before: {before}\nafter: {after}");

    assert_eq!(
        before["write_tools_enabled"],
        serde_json::json!(false),
        "precondition: the declared gate has to be in force before the file is removed: {before}"
    );

    assert_ne!(
        after["write_tools_source"],
        serde_json::json!("config_file"),
        "the file is gone, so nothing in it can still be the source of the gate: {after}"
    );
    // USER on a non-Live instance with nothing declared is the documented default, and the
    // documented default is writes on. The assertion that matters is that the deleted file's
    // `false` did not outlive the file.
    assert_eq!(
        after["write_tools_enabled"],
        serde_json::json!(true),
        "with no declaration left, USER on a non-Live instance is the documented default: {after}"
    );
}

/// T028 (085 edge case 3). The config file becomes unparseable: the last known-good gate stays, the
/// parse failure is reported, and access is never widened by a parse error.
///
/// The failure mode this pins down is the one that shipped in `workspace_config.rs`: the old code
/// bailed out of a bad config *before* it applied the gate, so a refused declaration left the
/// inference chain to answer instead — which for USER meant writes on. A parse error resolving to
/// more access than the last good file granted is the worst possible direction to fail in.
#[test]
#[ignore]
fn test_e2e_unparseable_config_keeps_the_last_known_good_gate() {
    if iris_host().is_empty() {
        eprintln!("Skipping: IRIS_HOST not set");
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let cfg_path = dir.path().join(".iris-agentic-dev.toml");
    std::fs::write(&cfg_path, gate_toml(false)).unwrap();
    let to_break = cfg_path.clone();

    let mut steps: Vec<Step> = init_msgs().into_iter().map(Step::Send).collect();
    steps.push(Step::Send(check_config_msg(2)));
    let corrupt = move || {
        // Not valid TOML in any reading: an unterminated string and a bare `=`.
        write_and_bump(
            &to_break,
            "host = \"local\nwrite_tools_enabled = = true\n",
            1,
        );
    };
    steps.push(Step::Do(&corrupt));
    steps.push(Step::Send(check_config_msg(3)));
    steps.push(Step::Send(global_set_msg(4, "t028")));
    let responses = mcp_session(Some(dir.path()), &[], &steps);
    let before = tool_result(&responses, 2);
    let after = tool_result(&responses, 3);
    let write = tool_result(&responses, 4);
    eprintln!("T028 unparseable — before: {before}\nafter: {after}\nwrite: {write}");

    assert_eq!(
        before["write_tools_enabled"],
        serde_json::json!(false),
        "precondition: the last known-good file declared writes off: {before}"
    );
    assert_eq!(
        after["write_tools_enabled"],
        serde_json::json!(false),
        "a parse error must keep the last known-good gate, never widen it: {after}"
    );
    assert_eq!(
        after["write_tools_source"],
        serde_json::json!("config_file"),
        "the gate still comes from the last file that parsed: {after}"
    );
    assert!(
        after["config_parse_error"].is_string(),
        "the operator has to be told the file no longer parses: {after}"
    );
    assert_eq!(
        write["error_code"],
        serde_json::json!("WRITE_TOOLS_DISABLED"),
        "enforcement must agree with the retained gate, not with the unparseable file: {write}"
    );
}

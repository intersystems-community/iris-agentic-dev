//! T023: MCP handshake integration test.
//! Spawns `iris-agentic-dev mcp` binary, sends JSON-RPC initialize + tools/list,
//! asserts ≥23 tools returned and response within 500ms.
//!
//! Tests written FIRST — must fail until T015–T022 are implemented.
#![allow(dead_code, clippy::zombie_processes)]

use std::io::{BufRead, BufReader, Write};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// The binary under test, or `None` only when an operator has explicitly asked for a quiet
/// skip.
///
/// These are the MCP protocol tests — initialize, tools/list, version negotiation. They used
/// to resolve `target/debug/iris-agentic-dev` themselves and `return` when it was absent, so
/// on any run that did not happen to have a debug build sitting there, seven `ok` lines
/// stood in for the entire handshake contract. `require_iad_binary` makes that absence a
/// failure unless IAD_ALLOW_SKIP says otherwise.
fn iris_dev_bin() -> Option<std::path::PathBuf> {
    iris_agentic_dev_core::testing::require_iad_binary()
}

fn send_jsonrpc(stdin: &mut impl Write, id: u64, method: &str, params: &str) {
    let msg = format!(
        "{{\"jsonrpc\":\"2.0\",\"id\":{},\"method\":\"{}\",\"params\":{}}}\n",
        id, method, params
    );
    stdin.write_all(msg.as_bytes()).unwrap();
    stdin.flush().unwrap();
}

fn read_jsonrpc(reader: &mut impl BufRead) -> serde_json::Value {
    let mut line = String::new();
    reader.read_line(&mut line).unwrap();
    serde_json::from_str(&line).expect("invalid JSON-RPC response")
}

/// iris-dev mcp starts and responds to initialize within 500ms.
#[test]
fn mcp_server_starts_and_responds_to_initialize() {
    // Give any previous test's spawned processes time to fully exit
    std::thread::sleep(std::time::Duration::from_millis(500));
    let Some(bin) = iris_dev_bin() else {
        return;
    };

    let mut child = Command::new(&bin)
        .arg("mcp")
        // Disable IRIS discovery for handshake tests — we only test MCP protocol, not tools
        .env("IRIS_WEB_PORT", "9") // Port 9 (discard) — instant ECONNREFUSED, no DNS lookup
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("failed to spawn iris-agentic-dev mcp");

    let mut stdin = child.stdin.take().unwrap();
    let stdout = child.stdout.take().unwrap();
    let mut reader = BufReader::new(stdout);

    let start = Instant::now();
    send_jsonrpc(
        &mut stdin,
        1,
        "initialize",
        r#"{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"test","version":"0.1"}}"#,
    );

    let response = read_jsonrpc(&mut reader);
    let elapsed = start.elapsed();
    // Send required initialized notification
    let init_notif = concat!(
        r#"{"jsonrpc":"2.0","method":"notifications/initialized","params":{}}"#,
        "
"
    );
    stdin.write_all(init_notif.as_bytes()).unwrap();
    stdin.flush().unwrap();

    assert!(
        elapsed < Duration::from_millis(500),
        "initialize took {}ms, expected <500ms",
        elapsed.as_millis()
    );
    assert!(
        response.get("result").is_some(),
        "initialize response missing 'result': {}",
        response
    );

    child.kill().ok();
}

/// tools/list returns ≥23 tools.
#[test]
fn mcp_server_tools_list_returns_23_tools() {
    let Some(bin) = iris_dev_bin() else {
        return;
    };

    let mut child = Command::new(&bin)
        .arg("mcp")
        // Disable IRIS discovery for handshake tests — we only test MCP protocol, not tools
        .env("IRIS_WEB_PORT", "9") // Port 9 (discard) — instant ECONNREFUSED, no DNS lookup
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("failed to spawn iris-agentic-dev mcp");

    let mut stdin = child.stdin.take().unwrap();
    let stdout = child.stdout.take().unwrap();
    let mut reader = BufReader::new(stdout);

    send_jsonrpc(
        &mut stdin,
        1,
        "initialize",
        r#"{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"test","version":"0.1"}}"#,
    );
    let _init = read_jsonrpc(&mut reader);
    let init_notif = concat!(
        r#"{"jsonrpc":"2.0","method":"notifications/initialized","params":{}}"#,
        "
"
    );
    stdin.write_all(init_notif.as_bytes()).unwrap();
    stdin.flush().unwrap();
    std::thread::sleep(std::time::Duration::from_millis(50));

    send_jsonrpc(&mut stdin, 2, "tools/list", "{}");
    let response = read_jsonrpc(&mut reader);

    let tools = response["result"]["tools"]
        .as_array()
        .expect("tools/list response missing tools array");

    let tool_names: Vec<_> = tools.iter().filter_map(|t| t["name"].as_str()).collect();

    assert!(
        tool_names.len() >= 23,
        "expected ≥23 tools, got {}: {:?}",
        tool_names.len(),
        tool_names
    );

    // Assert all required tools are present (no dots — Bedrock compatible)
    let required = [
        "iris_compile",
        "iris_test",
        "iris_symbols",
        "iris_debug",
        "docs_introspect",
        "skill_list",
        "kb_recall",
        "agent_stats",
        "iris_mirror_status",
        "iris_database_list",
        "iris_system_performance",
    ];
    for name in required {
        assert!(
            tool_names.contains(&name),
            "required tool '{}' missing from tools/list",
            name
        );
    }

    // Assert no tool has a dot in the name (Bedrock/VS Code requirement)
    for name in &tool_names {
        assert!(
            !name.contains('.'),
            "tool name '{}' contains dot — invalid for Bedrock/VS Code",
            name
        );
    }

    child.kill().ok();
}

/// 076-interface-modernization User Story 4: `tools/list` pagination works end-to-end over
/// the real JSON-RPC wire, not just via `paginate_tool_list`'s own pure-function unit tests
/// (`test_list_tools_pagination.rs`). `IRIS_LIST_TOOLS_PAGE_SIZE=5` forces real pagination
/// on the Baseline toolset's 81 tools; paging through with `cursor` must reconstruct the
/// exact same set `mcp_server_tools_list_returns_23_tools` sees in one unpaginated call,
/// with no duplicate and no omission.
#[test]
fn mcp_server_tools_list_pagination_works() {
    std::thread::sleep(std::time::Duration::from_millis(500));
    let Some(bin) = iris_dev_bin() else {
        return;
    };

    let mut child = Command::new(&bin)
        .arg("mcp")
        .env("IRIS_WEB_PORT", "9")
        .env("IRIS_LIST_TOOLS_PAGE_SIZE", "5")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("failed to spawn iris-agentic-dev mcp");

    let mut stdin = child.stdin.take().unwrap();
    let stdout = child.stdout.take().unwrap();
    let mut reader = BufReader::new(stdout);

    send_jsonrpc(
        &mut stdin,
        1,
        "initialize",
        r#"{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"test","version":"0.1"}}"#,
    );
    let _init = read_jsonrpc(&mut reader);
    let init_notif = concat!(
        r#"{"jsonrpc":"2.0","method":"notifications/initialized","params":{}}"#,
        "\n"
    );
    stdin.write_all(init_notif.as_bytes()).unwrap();
    stdin.flush().unwrap();
    std::thread::sleep(std::time::Duration::from_millis(50));

    let mut all_names: Vec<String> = Vec::new();
    let mut cursor: Option<String> = None;
    let mut request_id = 2u64;
    let mut page_count = 0;
    loop {
        let params = match &cursor {
            Some(c) => format!(r#"{{"cursor":"{c}"}}"#),
            None => "{}".to_string(),
        };
        send_jsonrpc(&mut stdin, request_id, "tools/list", &params);
        request_id += 1;
        let response = read_jsonrpc(&mut reader);

        let tools = response["result"]["tools"]
            .as_array()
            .expect("tools/list response missing tools array");
        assert!(
            !tools.is_empty() || page_count > 0,
            "first page must not be empty"
        );
        // Every page but a possible final empty one must respect the configured page size.
        assert!(
            tools.len() <= 5,
            "page {page_count} returned {} tools, expected <= 5 (IRIS_LIST_TOOLS_PAGE_SIZE)",
            tools.len()
        );
        all_names.extend(
            tools
                .iter()
                .filter_map(|t| t["name"].as_str().map(|s| s.to_string())),
        );
        page_count += 1;

        match response["result"]["nextCursor"].as_str() {
            Some(next) => cursor = Some(next.to_string()),
            None => break,
        }
        assert!(page_count < 100, "pagination did not terminate");
    }

    assert!(
        page_count > 1,
        "expected multiple pages with IRIS_LIST_TOOLS_PAGE_SIZE=5 on the Baseline toolset"
    );

    let unique: std::collections::HashSet<&String> = all_names.iter().collect();
    assert_eq!(
        unique.len(),
        all_names.len(),
        "a tool name appeared on more than one page"
    );
    assert!(
        all_names.len() >= 23,
        "paginated total ({}) must still cover at least the required-tool floor",
        all_names.len()
    );

    child.kill().ok();
}

/// Startup latency p50 < 100ms over 5 runs (SC-001).
///
/// SC-001 target is for release builds. Debug builds run ~2-3x slower due to
/// unoptimized code — threshold is relaxed to 500ms for debug builds.
#[test]
fn mcp_server_startup_latency_under_100ms() {
    let Some(bin) = iris_dev_bin() else {
        return;
    };

    let mut latencies = Vec::new();
    for _ in 0..5 {
        let mut child = Command::new(&bin)
            .arg("mcp")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .expect("failed to spawn iris-agentic-dev mcp");

        let mut stdin = child.stdin.take().unwrap();
        let stdout = child.stdout.take().unwrap();
        let mut reader = BufReader::new(stdout);

        let start = Instant::now();
        send_jsonrpc(
            &mut stdin,
            1,
            "initialize",
            r#"{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"bench","version":"0.1"}}"#,
        );
        let _resp = read_jsonrpc(&mut reader);
        latencies.push(start.elapsed());
        child.kill().ok();
    }

    latencies.sort();
    let p50 = latencies[latencies.len() / 2];

    // SC-001: p50 < 100ms on release builds; debug builds get 500ms
    #[cfg(debug_assertions)]
    let threshold = Duration::from_millis(500);
    #[cfg(not(debug_assertions))]
    let threshold = Duration::from_millis(100);

    assert!(
        p50 < threshold,
        "p50 startup latency {}ms exceeds {}ms (SC-001{})",
        p50.as_millis(),
        threshold.as_millis(),
        if cfg!(debug_assertions) {
            " — debug build, threshold relaxed"
        } else {
            ""
        }
    );
}

/// T009: discovery waits for IRIS — server returns tool list within 5s even with no env vars.
/// Uses port 9 (discard) so discovery fails fast, but server still returns tool list.
#[test]
fn discovery_waits_for_iris() {
    let Some(bin) = iris_dev_bin() else {
        return;
    };

    let mut child = Command::new(&bin)
        .arg("mcp")
        .env("IRIS_WEB_PORT", "9") // instant fail — tests that server doesn't hang
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("failed to spawn iris-agentic-dev mcp");

    let mut stdin = child.stdin.take().unwrap();
    let stdout = child.stdout.take().unwrap();
    let mut reader = BufReader::new(stdout);

    let start = Instant::now();
    send_jsonrpc(
        &mut stdin,
        1,
        "initialize",
        r#"{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"test","version":"0.1"}}"#,
    );
    let init = read_jsonrpc(&mut reader);
    assert!(init.get("result").is_some(), "initialize failed: {}", init);

    let init_notif = concat!(
        r#"{"jsonrpc":"2.0","method":"notifications/initialized","params":{}}"#,
        "\n"
    );
    stdin.write_all(init_notif.as_bytes()).unwrap();
    stdin.flush().unwrap();

    send_jsonrpc(&mut stdin, 2, "tools/list", "{}");
    let resp = read_jsonrpc(&mut reader);
    let elapsed = start.elapsed();

    assert!(
        elapsed < Duration::from_secs(5),
        "tools/list took {}ms, expected <5000ms",
        elapsed.as_millis()
    );

    let tools = resp["result"]["tools"]
        .as_array()
        .expect("tools array missing");
    assert!(
        !tools.is_empty(),
        "expected tools to be listed even without IRIS connection"
    );

    child.kill().ok();
}

/// T010: web prefix is included in Atelier request URL.
/// Verifies that IRIS_WEB_PREFIX is correctly incorporated into the base URL.
#[test]
fn web_prefix_in_connection_url() {
    use iris_agentic_dev_core::iris::connection::{DiscoverySource, IrisConnection};

    // Construct a connection with a prefix in the base_url (as mcp.rs does)
    let base_url = "http://localhost:80/irisaicore".to_string();
    let conn = IrisConnection::new(
        base_url,
        "USER",
        "_SYSTEM",
        "SYS",
        DiscoverySource::ExplicitFlag,
    );

    let url = conn.atelier_url("/v8/USER/action/compile");
    assert!(
        url.contains("/irisaicore/api/atelier/"),
        "prefix missing from URL: {}",
        url
    );
    assert_eq!(
        url,
        "http://localhost:80/irisaicore/api/atelier/v8/USER/action/compile"
    );
}

/// Issue #117: server negotiates all known protocol versions including 2026-07-28.
///
/// 2026-07-28 requires `ttlMs`/`cacheScope` on the `tools/list` response (SEP-2549).
/// We set those fields in list_tools, so the server can legitimately advertise and
/// echo 2026-07-28. All known versions are echoed back to clients that request them.
#[test]
fn mcp_server_negotiates_all_known_protocol_versions() {
    std::thread::sleep(std::time::Duration::from_millis(500));
    let Some(bin) = iris_dev_bin() else {
        return;
    };

    // All known protocol versions should be echoed back
    for client_version in &["2026-07-28", "2025-11-25", "2024-11-05"] {
        let mut child = Command::new(&bin)
            .arg("mcp")
            .env("IRIS_WEB_PORT", "9")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .expect("failed to spawn iris-agentic-dev mcp");

        let mut stdin = child.stdin.take().unwrap();
        let stdout = child.stdout.take().unwrap();
        let mut reader = BufReader::new(stdout);

        let params = format!(
            r#"{{"protocolVersion":"{}","capabilities":{{}},"clientInfo":{{"name":"test","version":"0.1"}}}}"#,
            client_version
        );
        send_jsonrpc(&mut stdin, 1, "initialize", &params);
        let response = read_jsonrpc(&mut reader);

        let server_version = response["result"]["protocolVersion"]
            .as_str()
            .unwrap_or("missing");

        assert_eq!(
            server_version, *client_version,
            "client sent {client_version}, server replied {server_version} — should echo known versions"
        );

        child.kill().ok();
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
}

/// Issue #117: tools/list response carries cache annotation when negotiating 2026-07-28.
///
/// SEP-2549 requires ttlMs and cacheScope on the ListToolsResult for 2026-07-28 peers.
/// ttlMs=0 means "do not cache" — correct for tools that query live IRIS state.
#[test]
fn mcp_server_tools_list_includes_cache_annotation_for_2026_07_28() {
    std::thread::sleep(std::time::Duration::from_millis(500));
    let Some(bin) = iris_dev_bin() else {
        return;
    };

    let mut child = Command::new(&bin)
        .arg("mcp")
        .env("IRIS_WEB_PORT", "9")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("failed to spawn iris-agentic-dev mcp");

    let mut stdin = child.stdin.take().unwrap();
    let stdout = child.stdout.take().unwrap();
    let mut reader = BufReader::new(stdout);

    send_jsonrpc(
        &mut stdin,
        1,
        "initialize",
        r#"{"protocolVersion":"2026-07-28","capabilities":{},"clientInfo":{"name":"test","version":"0.1"}}"#,
    );
    let _init = read_jsonrpc(&mut reader);
    let init_notif = concat!(
        r#"{"jsonrpc":"2.0","method":"notifications/initialized","params":{}}"#,
        "\n"
    );
    stdin.write_all(init_notif.as_bytes()).unwrap();
    stdin.flush().unwrap();
    std::thread::sleep(std::time::Duration::from_millis(50));

    send_jsonrpc(&mut stdin, 2, "tools/list", "{}");
    let response = read_jsonrpc(&mut reader);

    let ttl_ms = &response["result"]["ttlMs"];
    let cache_scope = &response["result"]["cacheScope"];

    assert!(
        !ttl_ms.is_null(),
        "tools/list response missing ttlMs for 2026-07-28 peer: {}",
        response
    );
    assert_eq!(ttl_ms, 0, "ttlMs should be 0 (do not cache): {}", response);
    assert!(
        !cache_scope.is_null(),
        "tools/list response missing cacheScope for 2026-07-28 peer: {}",
        response
    );

    child.kill().ok();
}

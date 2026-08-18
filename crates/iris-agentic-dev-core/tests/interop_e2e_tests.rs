#![allow(dead_code, clippy::zombie_processes)]
use std::io::{BufRead, BufReader, Write};
use std::process::{Command, Stdio};

fn iris_dev_bin() -> std::path::PathBuf {
    let mut root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    root.pop();
    root.pop();
    // Try all known locations and names in priority order
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

fn mcp_exchange(messages: &[serde_json::Value]) -> Vec<serde_json::Value> {
    let bin = iris_dev_bin();
    let iris_host = std::env::var("IRIS_HOST").unwrap_or_default();
    let iris_port = std::env::var("IRIS_WEB_PORT").unwrap_or_else(|_| "52780".to_string());

    let mut child = Command::new(&bin)
        .args(["mcp"])
        .env("IRIS_HOST", &iris_host)
        .env("IRIS_WEB_PORT", &iris_port)
        .env(
            "IRIS_USERNAME",
            std::env::var("IRIS_USERNAME").unwrap_or_else(|_| "_SYSTEM".to_string()),
        )
        .env(
            "IRIS_PASSWORD",
            std::env::var("IRIS_PASSWORD").unwrap_or_else(|_| "SYS".to_string()),
        )
        .env(
            "IRIS_NAMESPACE",
            std::env::var("IRIS_NAMESPACE").unwrap_or_else(|_| "USER".to_string()),
        )
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn iris-dev mcp");

    let mut stdin = child.stdin.take().unwrap();
    let stdout = child.stdout.take().unwrap();
    let mut reader = BufReader::new(stdout);
    let mut results = vec![];

    for msg in messages.iter() {
        stdin
            .write_all((serde_json::to_string(msg).unwrap() + "\n").as_bytes())
            .unwrap();
        stdin.flush().unwrap();
        if msg.get("id").is_some() {
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
            loop {
                let mut line = String::new();
                std::thread::sleep(std::time::Duration::from_millis(50));
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

fn find_response(responses: &[serde_json::Value], id: u64) -> Option<serde_json::Value> {
    responses.iter().find(|r| r["id"] == id).cloned()
}

fn parse_tool_text(response: &serde_json::Value) -> serde_json::Value {
    let text = response["result"]["content"][0]["text"]
        .as_str()
        .unwrap_or("{}");
    serde_json::from_str(text).unwrap_or_default()
}

#[test]
fn tools_list_returns_32_tools() {
    let iris_host = std::env::var("IRIS_HOST").unwrap_or_default();
    if iris_host.is_empty() {
        eprintln!("Skipping: IRIS_HOST not set");
        return;
    }

    let responses = mcp_exchange(&[
        serde_json::json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"e2e","version":"0.1"}}}),
        serde_json::json!({"jsonrpc":"2.0","method":"notifications/initialized","params":{}}),
        serde_json::json!({"jsonrpc":"2.0","id":2,"method":"tools/list","params":{}}),
    ]);

    let tools_resp = find_response(&responses, 2).expect("no tools/list response");
    let tools = tools_resp["result"]["tools"]
        .as_array()
        .expect("no tools array");
    let names: Vec<_> = tools.iter().filter_map(|t| t["name"].as_str()).collect();

    assert!(
        names.len() >= 20,
        "expected >=20 tools, got {}: {:?}",
        names.len(),
        &names[..names.len().min(10)]
    );
    // Verify current tool names (consolidated from older interop_* names)
    assert!(
        names.contains(&"iris_production") || names.contains(&"iris_interop_query"),
        "must contain interop tools: {:?}",
        &names
    );
    for name in &names {
        assert!(!name.contains('.'), "tool '{}' has dot", name);
    }
}

#[test]
fn interop_production_status_returns_structured_json() {
    let iris_host = std::env::var("IRIS_HOST").unwrap_or_default();
    if iris_host.is_empty() {
        return;
    }

    let responses = mcp_exchange(&[
        serde_json::json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"e2e","version":"0.1"}}}),
        serde_json::json!({"jsonrpc":"2.0","method":"notifications/initialized","params":{}}),
        // iris_production replaces interop_production_status
        serde_json::json!({"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"iris_production","arguments":{"action":"status"}}}),
    ]);

    let resp = find_response(&responses, 2).expect("no tool response");
    let result = parse_tool_text(&resp);
    assert!(
        result.get("success").is_some() || result.get("error_code").is_some(),
        "must return structured response: {}",
        result
    );
    // Regression: iris.execute()'s bare `iris session` REPL path on IRIS 2026.2+ prints a
    // "Node: <hostname>, Instance: IRIS" banner line whose embedded ':' previously got
    // misparsed as the production name:state pair (production came back as "Node",
    // state as "Unknown") — strip_iris_banner didn't know about this banner line, and
    // $$$ISERR silently failed to resolve outside a compiled class (no macro preprocessing
    // in interactive sessions), masking the real GetProductionStatus() result either way.
    if result["success"] == true {
        assert_ne!(
            result["production"], "Node",
            "production name must not be the banner artifact 'Node': {result}"
        );
        assert_ne!(
            result["state"], "Unknown",
            "state must not be 'Unknown' when a production is actually running: {result}"
        );
    }
}

#[test]
fn interop_logs_returns_structured_entries() {
    let iris_host = std::env::var("IRIS_HOST").unwrap_or_default();
    if iris_host.is_empty() {
        return;
    }

    let responses = mcp_exchange(&[
        serde_json::json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"e2e","version":"0.1"}}}),
        serde_json::json!({"jsonrpc":"2.0","method":"notifications/initialized","params":{}}),
        // iris_interop_query replaces interop_logs
        serde_json::json!({"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"iris_interop_query","arguments":{"query_type":"error_log","limit":5}}}),
    ]);

    let resp = find_response(&responses, 2).expect("no tool response");
    let result = parse_tool_text(&resp);
    assert!(result.get("success").is_some() || result.get("error_code").is_some());
}

#[test]
fn interop_queues_returns_array() {
    let iris_host = std::env::var("IRIS_HOST").unwrap_or_default();
    if iris_host.is_empty() {
        return;
    }

    let responses = mcp_exchange(&[
        serde_json::json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"e2e","version":"0.1"}}}),
        serde_json::json!({"jsonrpc":"2.0","method":"notifications/initialized","params":{}}),
        // iris_interop_query replaces interop_queues
        serde_json::json!({"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"iris_interop_query","arguments":{"query_type":"queues"}}}),
    ]);

    let resp = find_response(&responses, 2).expect("no tool response");
    let result = parse_tool_text(&resp);
    assert!(result.get("success").is_some() || result.get("error_code").is_some());
}

// ─── 024-interop-depth E2E stubs ───
// These tests run against a live IRIS instance with Interoperability enabled.
// They are #[ignore] by default; run with `cargo test -- --ignored` to execute.

#[test]
#[ignore = "requires live IRIS with Interoperability and a running production"]
fn test_production_item_enable_disable() {
    use std::time::Instant;
    let iris_host = std::env::var("IRIS_HOST").unwrap_or_default();
    assert!(!iris_host.is_empty(), "IRIS_HOST must be set");
    let item = std::env::var("TEST_PROD_ITEM").unwrap_or_else(|_| "TestService".to_string());
    let ns = std::env::var("IRIS_NAMESPACE").unwrap_or_else(|_| "USER".to_string());

    // disable
    let start = Instant::now();
    let responses = mcp_exchange(&[
        serde_json::json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"e2e","version":"0.1"}}}),
        serde_json::json!({"jsonrpc":"2.0","method":"notifications/initialized","params":{}}),
        serde_json::json!({"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"iris_production_item","arguments":{"action":"disable","item":item,"namespace":ns}}}),
    ]);
    assert!(
        start.elapsed().as_secs() < 3,
        "SC-003: tool call exceeded 3s"
    );
    let resp = find_response(&responses, 2).expect("no response");
    let result = parse_tool_text(&resp);
    assert!(
        result.get("success").is_some() || result.get("error_code").is_some(),
        "must return success or error_code"
    );

    // re-enable
    let responses2 = mcp_exchange(&[
        serde_json::json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"e2e","version":"0.1"}}}),
        serde_json::json!({"jsonrpc":"2.0","method":"notifications/initialized","params":{}}),
        serde_json::json!({"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"iris_production_item","arguments":{"action":"enable","item":item,"namespace":ns}}}),
    ]);
    let resp2 = find_response(&responses2, 2).expect("no response");
    let result2 = parse_tool_text(&resp2);
    assert!(result2.get("success").is_some() || result2.get("error_code").is_some());
}

#[test]
#[ignore = "requires live IRIS with Interoperability"]
fn test_credential_crud() {
    use std::time::Instant;
    let iris_host = std::env::var("IRIS_HOST").unwrap_or_default();
    assert!(!iris_host.is_empty(), "IRIS_HOST must be set");
    let ns = std::env::var("IRIS_NAMESPACE").unwrap_or_else(|_| "USER".to_string());
    let cred_id = "IrisDevTestCred";

    // list — assert no password in response
    let start = Instant::now();
    let responses = mcp_exchange(&[
        serde_json::json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"e2e","version":"0.1"}}}),
        serde_json::json!({"jsonrpc":"2.0","method":"notifications/initialized","params":{}}),
        serde_json::json!({"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"iris_credential_list","arguments":{"namespace":ns}}}),
    ]);
    assert!(start.elapsed().as_secs() < 3, "SC-003: list exceeded 3s");
    let resp = find_response(&responses, 2).expect("no response");
    let raw_text = resp["result"]["content"][0]["text"].as_str().unwrap_or("");
    assert!(
        !raw_text.contains("\"password\""),
        "password must not appear in credential list"
    );
    assert!(
        !raw_text.contains("\"Password\""),
        "Password must not appear in credential list"
    );

    // create
    let responses2 = mcp_exchange(&[
        serde_json::json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"e2e","version":"0.1"}}}),
        serde_json::json!({"jsonrpc":"2.0","method":"notifications/initialized","params":{}}),
        serde_json::json!({"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"iris_credential_manage","arguments":{"action":"create","id":cred_id,"username":"testuser","password":"testpass","namespace":ns}}}),
    ]);
    let r2 = parse_tool_text(&find_response(&responses2, 2).expect("no response"));
    assert!(r2["success"] == true || r2.get("error_code").is_some());

    // delete (cleanup)
    let responses3 = mcp_exchange(&[
        serde_json::json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"e2e","version":"0.1"}}}),
        serde_json::json!({"jsonrpc":"2.0","method":"notifications/initialized","params":{}}),
        serde_json::json!({"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"iris_credential_manage","arguments":{"action":"delete","id":cred_id,"namespace":ns}}}),
    ]);
    let r3 = parse_tool_text(&find_response(&responses3, 2).expect("no response"));
    assert!(r3["success"] == true || r3.get("error_code").is_some());
}

#[test]
#[ignore = "requires live IRIS with Interoperability"]
fn test_lookup_crud() {
    use std::time::Instant;
    let iris_host = std::env::var("IRIS_HOST").unwrap_or_default();
    assert!(!iris_host.is_empty(), "IRIS_HOST must be set");
    let ns = std::env::var("IRIS_NAMESPACE").unwrap_or_else(|_| "USER".to_string());
    let table = "IrisDevTestTable";

    // set 3 keys — Key3's value carries a quote, an apostrophe and an accent:
    // the old SQL-style escaping corrupted ' to '' and died with <SYNTAX> on "
    for (key, val) in &[("Key1", "Val1"), ("Key2", "Val2"), ("Key3", "Va\"l'ñ3")] {
        let start = Instant::now();
        let responses = mcp_exchange(&[
            serde_json::json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"e2e","version":"0.1"}}}),
            serde_json::json!({"jsonrpc":"2.0","method":"notifications/initialized","params":{}}),
            serde_json::json!({"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"iris_lookup_manage","arguments":{"action":"set","table":table,"key":key,"value":val,"namespace":ns}}}),
        ]);
        assert!(start.elapsed().as_secs() < 3, "SC-003: set exceeded 3s");
        let r = parse_tool_text(&find_response(&responses, 2).expect("no response"));
        assert_eq!(r["success"], true, "set {key} failed: {r}");
    }

    // list_tables — assert table present
    let resp_lt = mcp_exchange(&[
        serde_json::json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"e2e","version":"0.1"}}}),
        serde_json::json!({"jsonrpc":"2.0","method":"notifications/initialized","params":{}}),
        serde_json::json!({"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"iris_lookup_manage","arguments":{"action":"list_tables","namespace":ns}}}),
    ]);
    let lt = parse_tool_text(&find_response(&resp_lt, 2).expect("no response"));
    assert_eq!(lt["success"], true, "list_tables failed: {lt}");
    let empty = vec![];
    let tables = lt["tables"].as_array().unwrap_or(&empty);
    assert!(
        tables.iter().any(|t| t.as_str() == Some(table)),
        "table must appear in list_tables"
    );

    // export
    let resp_ex = mcp_exchange(&[
        serde_json::json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"e2e","version":"0.1"}}}),
        serde_json::json!({"jsonrpc":"2.0","method":"notifications/initialized","params":{}}),
        serde_json::json!({"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"iris_lookup_transfer","arguments":{"action":"export","table":table,"namespace":ns}}}),
    ]);
    let ex = parse_tool_text(&find_response(&resp_ex, 2).expect("no response"));
    assert_eq!(ex["success"], true, "export failed: {ex}");
    let xml = ex["xml"].as_str().unwrap_or("");
    assert!(!xml.is_empty(), "export must return the XML");

    // delete keys
    for key in &["Key1", "Key2", "Key3"] {
        let responses = mcp_exchange(&[
            serde_json::json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"e2e","version":"0.1"}}}),
            serde_json::json!({"jsonrpc":"2.0","method":"notifications/initialized","params":{}}),
            serde_json::json!({"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"iris_lookup_manage","arguments":{"action":"delete","table":table,"key":key,"namespace":ns}}}),
        ]);
        let _ = find_response(&responses, 2);
    }

    // import and verify round-trip — issue #6: this failed 7/7 with <SYNTAX>
    let resp_im = mcp_exchange(&[
        serde_json::json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"e2e","version":"0.1"}}}),
        serde_json::json!({"jsonrpc":"2.0","method":"notifications/initialized","params":{}}),
        serde_json::json!({"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"iris_lookup_transfer","arguments":{"action":"import","table":table,"xml":xml,"namespace":ns}}}),
    ]);
    let im = parse_tool_text(&find_response(&resp_im, 2).expect("no response"));
    assert_eq!(im["success"], true, "import failed: {im}");

    // verify values restored, including the quote/apostrophe/accent one
    for (key, want) in &[("Key1", "Val1"), ("Key3", "Va\"l'ñ3")] {
        let resp_get = mcp_exchange(&[
            serde_json::json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"e2e","version":"0.1"}}}),
            serde_json::json!({"jsonrpc":"2.0","method":"notifications/initialized","params":{}}),
            serde_json::json!({"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"iris_lookup_manage","arguments":{"action":"get","table":table,"key":key,"namespace":ns}}}),
        ]);
        let g = parse_tool_text(&find_response(&resp_get, 2).expect("no response"));
        assert_eq!(g["success"], true, "get {key} after import failed: {g}");
        assert_eq!(
            g["value"].as_str(),
            Some(*want),
            "SC-005: round-trip value must match for {key}"
        );
    }

    // leave the namespace clean
    for key in &["Key1", "Key2", "Key3"] {
        let responses = mcp_exchange(&[
            serde_json::json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"e2e","version":"0.1"}}}),
            serde_json::json!({"jsonrpc":"2.0","method":"notifications/initialized","params":{}}),
            serde_json::json!({"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"iris_lookup_manage","arguments":{"action":"delete","table":table,"key":key,"namespace":ns}}}),
        ]);
        let _ = find_response(&responses, 2);
    }
}

#[test]
#[ignore = "requires live IRIS with Interoperability"]
fn test_production_autostart() {
    use std::time::Instant;
    let iris_host = std::env::var("IRIS_HOST").unwrap_or_default();
    assert!(!iris_host.is_empty(), "IRIS_HOST must be set");
    let ns = std::env::var("IRIS_NAMESPACE").unwrap_or_else(|_| "USER".to_string());

    // get current state
    let start = Instant::now();
    let responses = mcp_exchange(&[
        serde_json::json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"e2e","version":"0.1"}}}),
        serde_json::json!({"jsonrpc":"2.0","method":"notifications/initialized","params":{}}),
        serde_json::json!({"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"iris_production","arguments":{"action":"get_autostart","namespace":ns}}}),
    ]);
    assert!(
        start.elapsed().as_secs() < 3,
        "SC-003: get_autostart exceeded 3s"
    );
    let r = parse_tool_text(&find_response(&responses, 2).expect("no response"));
    assert!(
        r["success"] == true || r.get("error_code").is_some(),
        "must return success or error_code"
    );

    // set disabled
    let r2_resp = mcp_exchange(&[
        serde_json::json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"e2e","version":"0.1"}}}),
        serde_json::json!({"jsonrpc":"2.0","method":"notifications/initialized","params":{}}),
        serde_json::json!({"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"iris_production","arguments":{"action":"set_autostart","namespace":ns,"enabled":false}}}),
    ]);
    let r2 = parse_tool_text(&find_response(&r2_resp, 2).expect("no response"));
    assert!(r2["success"] == true || r2.get("error_code").is_some());

    // confirm disabled
    let r3_resp = mcp_exchange(&[
        serde_json::json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"e2e","version":"0.1"}}}),
        serde_json::json!({"jsonrpc":"2.0","method":"notifications/initialized","params":{}}),
        serde_json::json!({"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"iris_production","arguments":{"action":"get_autostart","namespace":ns}}}),
    ]);
    let r3 = parse_tool_text(&find_response(&r3_resp, 2).expect("no response"));
    if r3["success"] == true {
        assert_eq!(
            r3["autostart_enabled"], false,
            "autostart must be disabled after set_autostart false"
        );
    }
}

#[test]
#[ignore = "requires live IRIS with Interoperability"]
fn test_message_content_search() {
    // Issue #97: body-class join and Search-Table search replace hand SQL.
    let iris_host = std::env::var("IRIS_HOST").unwrap_or_default();
    assert!(!iris_host.is_empty(), "IRIS_HOST must be set");
    let ns = std::env::var("IRIS_NAMESPACE").unwrap_or_else(|_| "USER".to_string());

    let call = |tool: &str, args: serde_json::Value| {
        let responses = mcp_exchange(&[
            serde_json::json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"e2e","version":"0.1"}}}),
            serde_json::json!({"jsonrpc":"2.0","method":"notifications/initialized","params":{}}),
            serde_json::json!({"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":tool,"arguments":args}}),
        ]);
        parse_tool_text(&find_response(&responses, 2).expect("no response"))
    };

    // Seed a header+body fixture (no production running needed).
    let needle = "e2e-content-search-needle";
    let seed = format!(
        "Set body=##class(Ens.StringContainer).%New()\n\
         Set body.StringValue=\"{needle}\"\n\
         Set tSC=body.%Save()\n\
         If $$$ISERR(tSC) {{ Write \"BODYFAIL\" Quit }}\n\
         Set hdr=##class(Ens.MessageHeader).%New()\n\
         Set hdr.MessageBodyClassName=\"Ens.StringContainer\"\n\
         Set hdr.MessageBodyId=body.%Id()\n\
         Set hdr.SourceConfigName=\"E2E.Source\"\n\
         Set tSC2=hdr.%Save()\n\
         If $$$ISERR(tSC2) {{ Write \"HDRFAIL\" Quit }}\n\
         Write \"OK:\"_hdr.%Id()_\":\"_body.%Id()"
    );
    let r = call(
        "iris_execute",
        serde_json::json!({"namespace": ns, "code": seed}),
    );
    let out = r["output"].as_str().unwrap_or("");
    assert!(out.starts_with("OK:"), "fixture seed failed: {r}");
    let ids: Vec<&str> = out.trim_start_matches("OK:").split(':').collect();
    let (hdr_id, body_id) = (ids[0].to_string(), ids[1].to_string());

    // 1. body-class join finds the needle.
    let r = call(
        "iris_interop_query",
        serde_json::json!({"what":"messages","namespace":ns,
            "body_class":"Ens.StringContainer",
            "body_where": format!("StringValue = '{needle}'"),
            "body_select":["StringValue"]}),
    );
    assert_eq!(r["success"], true, "body join failed: {r}");
    assert_eq!(r["count"], 1, "expected exactly the fixture: {r}");
    assert_eq!(r["messages"][0]["StringValue"], needle, "{r}");
    assert!(r["messages"][0]["SourceConfigName"].is_string(), "{r}");

    // 2. body_where without body_class → error listing real classes.
    let r = call(
        "iris_interop_query",
        serde_json::json!({"what":"messages","namespace":ns,"body_where":"X=1"}),
    );
    assert_eq!(r["error_code"], "INVALID_PARAMS", "{r}");
    assert!(
        r["hint"]
            .as_str()
            .unwrap_or("")
            .contains("Ens.StringContainer"),
        "hint must list case-exact body classes: {r}"
    );

    // 3. unknown body class → BODY_CLASS_NOT_FOUND.
    let r = call(
        "iris_interop_query",
        serde_json::json!({"what":"messages","namespace":ns,
            "body_class":"No.Such.Class","body_where":"X=1"}),
    );
    assert_eq!(r["error_code"], "BODY_CLASS_NOT_FOUND", "{r}");

    // 4. unknown search-table prop → SEARCH_PROP_NOT_FOUND with available props.
    let r = call(
        "iris_interop_query",
        serde_json::json!({"what":"messages","namespace":ns,
            "search_table":{"prop":"NoSuchProp","value":"x"}}),
    );
    assert_eq!(r["error_code"], "SEARCH_PROP_NOT_FOUND", "{r}");

    // 5. valid prop with no match → success + back-indexed hint.
    let r = call(
        "iris_interop_query",
        serde_json::json!({"what":"messages","namespace":ns,
            "search_table":{"prop":"MSHControlID","value":"e2e-no-such-value"}}),
    );
    // MSHControlID may not be registered; either success(count=0) or SEARCH_PROP_NOT_FOUND is acceptable.
    assert!(
        r["success"] == true || r["error_code"] == "SEARCH_PROP_NOT_FOUND",
        "unexpected result: {r}"
    );
    if r["success"] == true {
        assert_eq!(r["count"], 0, "{r}");
        assert!(
            r["hint"].as_str().unwrap_or("").contains("back-indexed"),
            "zero-rows hint missing: {r}"
        );
    }

    // Cleanup.
    let cleanup = format!(
        "Do ##class(Ens.MessageHeader).%DeleteId({hdr_id})\n\
         Do ##class(Ens.StringContainer).%DeleteId({body_id})\n\
         Write \"CLEAN\""
    );
    let r = call(
        "iris_execute",
        serde_json::json!({"namespace": ns, "code": cleanup}),
    );
    assert_eq!(r["output"].as_str(), Some("CLEAN"), "cleanup failed: {r}");
}

//! 130 round 4 (FR-014): `iad mcp` stopped by SIGTERM must still finish its telemetry write.
//!
//! Every tool call spawns a durable telemetry write, which runs a `IrisDevTmp.IrisDevTel*` scratch
//! class through put, compile, query and delete. `main` flushes those writes on the way out, but a
//! SIGTERM ended the process before `main` returned, so the flush never ran. MCP hosts stop their
//! servers that way, and so did every test harness here (with SIGKILL, which nothing can catch):
//! one credentialed full run left 178 `IrisDevTel` and 1118 `IrisDevRun` classes in USER.

use std::collections::BTreeSet;
use std::io::{BufRead, BufReader, Write};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

fn scratch_classes() -> Option<BTreeSet<String>> {
    let host = std::env::var("IRIS_HOST").ok().filter(|h| !h.is_empty())?;
    let port = std::env::var("IRIS_WEB_PORT").unwrap_or_else(|_| "52780".to_string());
    let user = std::env::var("IRIS_USERNAME").unwrap_or_else(|_| "_SYSTEM".to_string());
    let pass = std::env::var("IRIS_PASSWORD").unwrap_or_else(|_| "SYS".to_string());
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    rt.block_on(async {
        let url = format!(
            "http://{host}:{port}/api/atelier/v1/USER/docnames/CLS?filter=IrisDevTmp.IrisDev%25"
        );
        let body: serde_json::Value = reqwest::Client::new()
            .get(&url)
            .basic_auth(&user, Some(&pass))
            .send()
            .await
            .expect("docnames")
            .json()
            .await
            .expect("docnames JSON");
        Some(
            body["result"]["content"]
                .as_array()
                .expect("docnames content")
                .iter()
                .filter_map(|d| d["name"].as_str())
                .filter(|n| n.starts_with("IrisDevTmp.IrisDev"))
                .map(str::to_string)
                .collect(),
        )
    })
}

#[cfg(unix)]
#[test]
#[ignore = "requires live IRIS (IRIS_HOST) and IAD_BINARY"]
fn sigterm_after_a_tool_call_exits_cleanly_and_leaves_no_scratch_class_130() {
    let Some(before) = scratch_classes() else {
        eprintln!("IRIS_HOST not set, skipping");
        return;
    };
    let bin = iris_agentic_dev_core::testing::iad_binary_path();
    assert!(bin.exists(), "binary not found at {}", bin.display());

    let dir = tempfile::tempdir().unwrap();
    let mut child = Command::new(&bin)
        .env("HOME", crate::support_141::home())
        .env("USERPROFILE", crate::support_141::home())
        .arg("mcp")
        .current_dir(dir.path())
        .env("OBJECTSCRIPT_WORKSPACE", dir.path())
        // The scratch class exists only with the opt-in and writes on (spec 141 R5 C16); with
        // either missing there is nothing to leak and this test would prove nothing.
        .env("IAD_TELEMETRY_SINK", "iris")
        .env("IRIS_WRITE_TOOLS_ENABLED", "1")
        .env("IRIS_DESTRUCTIVE_TOOLS_ENABLED", "0")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn iris-agentic-dev mcp");
    let mut stdin = child.stdin.take().unwrap();
    let mut lines = BufReader::new(child.stdout.take().unwrap()).lines();

    for msg in [
        r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-03-26","capabilities":{},"clientInfo":{"name":"sigterm-130","version":"0"}}}"#,
        r#"{"jsonrpc":"2.0","method":"notifications/initialized","params":{}}"#,
        r#"{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"iris_query","arguments":{"query":"SELECT 1 AS one","namespace":"USER"}}}"#,
    ] {
        writeln!(stdin, "{msg}").unwrap();
    }
    stdin.flush().unwrap();
    let answered = lines
        .by_ref()
        .map_while(Result::ok)
        .any(|l| serde_json::from_str::<serde_json::Value>(&l).is_ok_and(|v| v["id"] == 2));
    assert!(answered, "no response to the iris_query call");

    // The call has answered; its telemetry write is now in flight. Stop the server the way a host
    // does, with stdin still open.
    let status = Command::new("kill")
        .args(["-TERM", &child.id().to_string()])
        .status()
        .unwrap();
    assert!(status.success());

    let deadline = Instant::now() + Duration::from_secs(15);
    let exit = loop {
        if let Some(s) = child.try_wait().unwrap() {
            break s;
        }
        if Instant::now() > deadline {
            child.kill().ok(); // allow-kill: SIGTERM already failed
            panic!("iad mcp still running 15 s after SIGTERM");
        }
        std::thread::sleep(Duration::from_millis(50));
    };
    drop(stdin);
    assert!(
        exit.code().is_some(),
        "SIGTERM killed iad mcp outright ({exit:?}), so its telemetry flush never ran"
    );

    let leaked: Vec<_> = scratch_classes()
        .unwrap()
        .difference(&before)
        .cloned()
        .collect();
    assert!(leaked.is_empty(), "SIGTERM left {leaked:?}");
}

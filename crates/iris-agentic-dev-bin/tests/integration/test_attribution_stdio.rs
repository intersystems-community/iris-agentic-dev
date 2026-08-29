//! T010 — subprocess test: binary in MCP mode emits the `mcp` marker, and the marker
//! names the MCP client that sent `clientInfo` during `initialize`.
//!
//! Requires a live iris-dev-iris container (the test calls `iris_execute` so IRIS must
//! be reachable) and a pre-built binary at `IAD_BINARY` or the default debug path.
//!
//! Run with:
//!   IRIS_HOST=localhost IRIS_WEB_PORT=52780 IRIS_USERNAME=_SYSTEM IRIS_PASSWORD=SYS \
//!   IAD_BINARY=./target/debug/iris-agentic-dev \
//!   cargo test --test test_attribution_stdio -- --include-ignored --test-threads=1

use std::io::{BufRead, BufReader, Write};
use std::process::{Command, Stdio};
use std::time::Duration;

fn iad_binary() -> std::path::PathBuf {
    if let Ok(p) = std::env::var("IAD_BINARY") {
        return std::path::PathBuf::from(p);
    }
    let mut p = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    p.pop(); // crates/iris-agentic-dev-bin
    p.pop(); // crates/
    p.push("target/debug/iris-agentic-dev");
    p
}

/// Spawn the binary in MCP mode with live IRIS env vars.
fn spawn_mcp_live() -> Option<(
    std::process::Child,
    std::process::ChildStdin,
    std::process::ChildStdout,
)> {
    let host = std::env::var("IRIS_HOST").unwrap_or_default();
    if host.is_empty() {
        return None;
    }
    let bin = iad_binary();
    if !bin.exists() {
        return None;
    }
    let mut cmd = Command::new(&bin);
    cmd.arg("mcp")
        .env("IRIS_HOST", &host)
        .env(
            "IRIS_WEB_PORT",
            std::env::var("IRIS_WEB_PORT").unwrap_or_else(|_| "52780".to_string()),
        )
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
        .stderr(Stdio::null());
    let mut child = cmd.spawn().ok()?;
    let stdin = child.stdin.take()?;
    let stdout = child.stdout.take()?;
    Some((child, stdin, stdout))
}

/// Read newline-delimited JSON until `predicate` returns Some or timeout elapses.
fn read_until<T, F>(
    stdout: std::process::ChildStdout,
    timeout_ms: u64,
    mut predicate: F,
) -> Option<T>
where
    T: Send + 'static,
    F: FnMut(&serde_json::Value) -> Option<T> + Send + 'static,
{
    let (tx, rx) = std::sync::mpsc::channel::<T>();
    std::thread::spawn(move || {
        let reader = BufReader::new(stdout);
        for line in reader.lines() {
            let Ok(line) = line else { break };
            if let Ok(v) = serde_json::from_str::<serde_json::Value>(line.trim()) {
                if let Some(result) = predicate(&v) {
                    let _ = tx.send(result);
                    return;
                }
            }
        }
    });
    rx.recv_timeout(std::time::Duration::from_millis(timeout_ms))
        .ok()
}

/// T010: Send `initialize` with a named MCP client, then call `iris_execute` to read
/// `%request.CgiEnvs("HTTP_USER_AGENT")` from inside IRIS. The marker must contain:
///   - `iris-agentic-dev/<version>`
///   - `mcp` (not `cli`)
///   - `test-client/9.9.9` (the clientInfo we sent)
#[test]
#[ignore]
fn mcp_mode_marker_names_connected_client() {
    let (mut child, mut stdin, stdout) = match spawn_mcp_live() {
        Some(t) => t,
        None => {
            eprintln!("IRIS_HOST not set or binary not found — skipping T010");
            return;
        }
    };

    // Handshake with a known clientInfo.
    let init = concat!(
        r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"#,
        r#""protocolVersion":"2025-03-26","capabilities":{},"#,
        r#""clientInfo":{"name":"test-client","version":"9.9.9"}}}"#,
        "\n"
    );
    let notif = "{\"jsonrpc\":\"2.0\",\"method\":\"notifications/initialized\",\"params\":{}}\n";
    stdin.write_all(init.as_bytes()).ok();
    stdin.write_all(notif.as_bytes()).ok();

    // Wait for initialize response.
    let _init_resp: Option<String> = read_until(
        {
            // We need stdout for two reads, but ChildStdout is !Clone. Use a channel trick:
            // this first read_until consumes the ChildStdout. We work around this by using
            // a single read_until for the tool call response (the initialize response is
            // consumed implicitly by the reader thread which stays alive until EOF).
            // The pattern from test_mcp_binary_config: read_until eats stdout. So we
            // pass stdout to the first read_until and it returns when the predicate matches.
            // For the second read we'd need a second stdout handle — but ChildStdout is not
            // cloneable. Instead: use the same read_until for the tool call response, and
            // accept that we skip past the initialize response.
            stdout
        },
        20000,
        |v| {
            // Match either the tool call response (has result.content) or skip.
            let content = v.get("result")?.get("content")?.as_array()?;
            for c in content {
                let text = c.get("text")?.as_str()?;
                if text.starts_with("iris-agentic-dev/") || text.contains("HTTP_USER_AGENT") {
                    return Some(text.to_string());
                }
            }
            // Also accept the raw text output of iris_execute which echoes the UA.
            for c in content {
                let text = c.get("text")?.as_str()?;
                if !text.is_empty() {
                    return Some(text.to_string());
                }
            }
            None
        },
    );

    // Send iris_execute to read the User-Agent from inside IRIS.
    let exec_req = concat!(
        r#"{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"#,
        r#""name":"iris_execute","arguments":{"#,
        r#""code":"write $Get(%request.CgiEnvs(\"HTTP_USER_AGENT\"),\"<none>\"),!"}}}"#,
        "\n"
    );
    stdin.write_all(exec_req.as_bytes()).ok();

    // Re-read stdout — but we already consumed it above. The read_until above will have
    // been running in its own thread and draining stdout. We need a different approach.
    //
    // Use the separate thread approach: since both reads need stdout and we only have one
    // handle, we read BOTH responses in a single read_until pass. The initialize response
    // is id=1; the exec response is id=2. We match on id=2.
    //
    // The code above already consumed stdout. This design needs rethinking.
    // Simplest fix: don't call read_until twice on the same stdout. Use one pass
    // that first sends the exec request and then reads until the exec response arrives.
    // But we already sent initialize before read_until. The reader thread in read_until
    // will read ALL lines from stdout, including initialize response AND exec response.
    // So: send exec request while the reader thread is still running, and the predicate
    // will eventually see it.

    // The _init_resp above is None (we never matched it in the predicate above because
    // we were looking for tool-call content, not initialize result). The reader thread
    // from that read_until ran to EOF or timeout. Let's use a cleaner design:
    let _ = child.kill();
    let _ = child.wait();

    // Restart with a fresh process and a single-pass approach.
    let (mut child2, mut stdin2, stdout2) = match spawn_mcp_live() {
        Some(t) => t,
        None => return,
    };

    let init2 = concat!(
        r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"#,
        r#""protocolVersion":"2025-03-26","capabilities":{},"#,
        r#""clientInfo":{"name":"test-client","version":"9.9.9"}}}"#,
        "\n"
    );
    stdin2.write_all(init2.as_bytes()).ok();
    stdin2
        .write_all(
            b"{\"jsonrpc\":\"2.0\",\"method\":\"notifications/initialized\",\"params\":{}}\n",
        )
        .ok();

    // Send the exec call immediately — the reader thread will see both id=1 and id=2.
    let exec_req2 = concat!(
        "{\"jsonrpc\":\"2.0\",\"id\":2,\"method\":\"tools/call\",\"params\":{",
        "\"name\":\"iris_execute\",\"arguments\":{",
        "\"code\":\"write $Get(%request.CgiEnvs(\\\"HTTP_USER_AGENT\\\"),\\\"<none>\\\"),!\"",
        "}}}\n"
    );
    stdin2.write_all(exec_req2.as_bytes()).ok();

    let ua = read_until(stdout2, 20000, |v| {
        // Match id=2 tool call response.
        if v.get("id")?.as_u64()? != 2 {
            return None;
        }
        let content = v.get("result")?.get("content")?.as_array()?;
        for c in content {
            let text = c.get("text")?.as_str()?;
            if text.trim().is_empty() {
                continue;
            }
            // iris_execute returns a JSON object with an "output" field.
            if let Ok(obj) = serde_json::from_str::<serde_json::Value>(text.trim()) {
                if let Some(output) = obj.get("output").and_then(|o| o.as_str()) {
                    return Some(output.trim().to_string());
                }
            }
            // Fallback: plain text (for backward compat).
            return Some(text.trim().to_string());
        }
        None
    });

    let _ = child2.kill();
    let _ = child2.wait();

    let ua = match ua {
        Some(u) => u,
        None => {
            panic!(
                "T010: no tool call response from iris_execute in MCP mode — \
                 is IRIS_HOST set and iris-dev-iris reachable?"
            );
        }
    };

    assert!(
        ua.starts_with("iris-agentic-dev/"),
        "T010: MCP mode must carry product marker; IRIS saw UA: {:?}",
        ua
    );
    assert!(
        ua.contains("mcp"),
        "T010: mcp marker expected, got cli or other; IRIS saw: {:?}",
        ua
    );
    assert!(
        ua.contains("test-client/9.9.9"),
        "T010: clientInfo name+version must appear in marker; IRIS saw: {:?}",
        ua
    );
}

/// T033: Subprocess wiring test for `irisAudit` (#111 pattern — "config key exists but never
/// wired"). Start the binary with a flat TOML config that sets `irisAudit = true` under
/// `[policy.default]` (the catchall for non-ServerManager connections), call `iris_execute`,
/// and assert the emission path was taken. The emission path is detectable from stderr: when
/// the `iris-agentic-dev` Security.Events entry does not exist, the tool warns exactly once.
///
/// A matching run without `irisAudit` must produce no such warning.
///
/// Requires a live IRIS connection (IRIS_HOST) and IAD_BINARY.
#[test]
#[ignore]
fn iris_audit_emission_wiring() {
    let host = match std::env::var("IRIS_HOST") {
        Ok(h) if !h.is_empty() => h,
        _ => {
            eprintln!("IRIS_HOST not set — skipping T033");
            return;
        }
    };
    let bin = iad_binary();
    if !bin.exists() {
        eprintln!("IAD_BINARY not found at {:?} — skipping T033", bin);
        return;
    }

    let port = std::env::var("IRIS_WEB_PORT").unwrap_or_else(|_| "52780".to_string());
    let username = std::env::var("IRIS_USERNAME").unwrap_or_else(|_| "_SYSTEM".to_string());
    let password = std::env::var("IRIS_PASSWORD").unwrap_or_else(|_| "SYS".to_string());
    let namespace = std::env::var("IRIS_NAMESPACE").unwrap_or_else(|_| "USER".to_string());

    // Use flat WorkspaceConfig format (top-level host/port/etc) so the connection is
    // built via workspace_config_to_connection. The policy uses the "default" catchall key —
    // flat configs have no server name, so active_server_manager_policy() looks for "default".
    let config_with_audit = format!(
        "host = \"{host}\"\nweb_port = {port}\nusername = \"{username}\"\npassword = \"{password}\"\nnamespace = \"{namespace}\"\n\n[policy.default]\nirisAudit = true\n",
    );

    let config_without_audit = format!(
        "host = \"{host}\"\nweb_port = {port}\nusername = \"{username}\"\npassword = \"{password}\"\nnamespace = \"{namespace}\"\n",
    );

    // Helper closure: spawn binary, run a tool, collect stderr.
    let run_with_config = |toml_content: &str| -> String {
        let dir = tempfile::tempdir().expect("tempdir");
        std::fs::write(dir.path().join(".iris-agentic-dev.toml"), toml_content).unwrap();
        let mut child = Command::new(&bin)
            .arg("mcp")
            .arg("--workspace")
            .arg(".")
            .current_dir(dir.path())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("spawn binary");
        let mut stdin = child.stdin.take().unwrap();
        let stdout = child.stdout.take().unwrap();
        let stderr_handle = child.stderr.take().unwrap();

        // Capture stderr in background.
        let (tx, rx) = std::sync::mpsc::channel::<String>();
        std::thread::spawn(move || {
            let reader = BufReader::new(stderr_handle);
            let mut buf = String::new();
            let mut lines = reader.lines();
            while let Some(Ok(line)) = lines.next() {
                buf.push_str(&line);
                buf.push('\n');
            }
            let _ = tx.send(buf);
        });

        // Send initialize + a simple tool call (check_config is policy-gated but low-cost).
        let init = concat!(
            r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"#,
            r#""protocolVersion":"2025-03-26","capabilities":{},"#,
            r#""clientInfo":{"name":"t033-client","version":"1.0.0"}}}"#,
            "\n"
        );
        let notif =
            "{\"jsonrpc\":\"2.0\",\"method\":\"notifications/initialized\",\"params\":{}}\n";
        stdin.write_all(init.as_bytes()).ok();
        stdin.write_all(notif.as_bytes()).ok();

        // Call `iris_execute` — it calls write_audit_entry on the policy path. With
        // irisAudit=true and no event definition, the emission attempt warns to stderr.
        let call = "{\"jsonrpc\":\"2.0\",\"id\":2,\"method\":\"tools/call\",\"params\":{\"name\":\"iris_execute\",\"arguments\":{\"code\":\"write 1\"}}}\n";
        stdin.write_all(call.as_bytes()).ok();

        // Wait for id=2 response.
        let _ = read_until(stdout, 15000, |v| {
            if v.get("id")?.as_u64()? == 2 {
                Some(())
            } else {
                None
            }
        });

        // Give the background audit task a moment to run (it's spawned async).
        std::thread::sleep(Duration::from_millis(500));
        drop(stdin);
        let _ = child.kill();
        let _ = child.wait();

        rx.recv_timeout(Duration::from_secs(5)).unwrap_or_default()
    };

    // With irisAudit = true: expect the warn about absent/disabled event definition.
    let stderr_with = run_with_config(&config_with_audit);
    eprintln!("T033 stderr (with irisAudit): {stderr_with}");

    // With irisAudit absent: expect NO such warn.
    let stderr_without = run_with_config(&config_without_audit);
    eprintln!("T033 stderr (without irisAudit): {stderr_without}");

    assert!(
        stderr_with.contains("Security.Events") || stderr_with.contains("audit emission"),
        "T033: with irisAudit=true, stderr must contain emission warning; got: {stderr_with}"
    );
    assert!(
        !stderr_without.contains("Security.Events") && !stderr_without.contains("audit emission"),
        "T033: without irisAudit, stderr must not contain emission warning; got: {stderr_without}"
    );
}

/// T038: `check_config` reports `iris_audit_failures` when emission has failed.
/// Start the binary with `irisAudit = true` and no event definition, call `iris_execute` to
/// trigger an emission failure, then call `check_config` and assert the response includes
/// `iris_audit_failures > 0`. With `irisAudit` absent the field must be absent.
///
/// Requires a live IRIS connection (IRIS_HOST) and IAD_BINARY.
#[test]
#[ignore]
fn iris_audit_failures_surfaced_in_check_config() {
    let host = match std::env::var("IRIS_HOST") {
        Ok(h) if !h.is_empty() => h,
        _ => {
            eprintln!("IRIS_HOST not set — skipping T038");
            return;
        }
    };
    let bin = iad_binary();
    if !bin.exists() {
        eprintln!("IAD_BINARY not found at {:?} — skipping T038", bin);
        return;
    }

    let port = std::env::var("IRIS_WEB_PORT").unwrap_or_else(|_| "52780".to_string());
    let username = std::env::var("IRIS_USERNAME").unwrap_or_else(|_| "_SYSTEM".to_string());
    let password = std::env::var("IRIS_PASSWORD").unwrap_or_else(|_| "SYS".to_string());
    let namespace = std::env::var("IRIS_NAMESPACE").unwrap_or_else(|_| "USER".to_string());

    let config_with_audit = format!(
        "host = \"{host}\"\nweb_port = {port}\nusername = \"{username}\"\npassword = \"{password}\"\nnamespace = \"{namespace}\"\n\n[policy.default]\nirisAudit = true\n",
    );
    let config_without_audit = format!(
        "host = \"{host}\"\nweb_port = {port}\nusername = \"{username}\"\npassword = \"{password}\"\nnamespace = \"{namespace}\"\n",
    );

    let run_and_get_check_config = |toml_content: &str| -> Option<serde_json::Value> {
        let dir = tempfile::tempdir().ok()?;
        std::fs::write(dir.path().join(".iris-agentic-dev.toml"), toml_content).ok()?;
        let mut child = std::process::Command::new(&bin)
            .arg("mcp")
            .arg("--workspace")
            .arg(".")
            .current_dir(dir.path())
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null())
            .spawn()
            .ok()?;
        let mut stdin = child.stdin.take()?;
        let stdout = child.stdout.take()?;

        let init = concat!(
            r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"#,
            r#""protocolVersion":"2025-03-26","capabilities":{},"#,
            r#""clientInfo":{"name":"t038-client","version":"1.0.0"}}}"#,
            "\n"
        );
        let notif =
            "{\"jsonrpc\":\"2.0\",\"method\":\"notifications/initialized\",\"params\":{}}\n";
        stdin.write_all(init.as_bytes()).ok();
        stdin.write_all(notif.as_bytes()).ok();

        // Trigger an emission attempt (will fail — no event definition exists).
        let exec_call = "{\"jsonrpc\":\"2.0\",\"id\":2,\"method\":\"tools/call\",\"params\":{\"name\":\"iris_execute\",\"arguments\":{\"code\":\"write 1\"}}}\n";
        stdin.write_all(exec_call.as_bytes()).ok();

        // Wait for exec response, then call check_config.
        let _ = read_until(stdout, 15000, |v| {
            if v.get("id")?.as_u64()? == 2 {
                Some(())
            } else {
                None
            }
        });

        // Give the background audit task a moment to record the failure.
        std::thread::sleep(Duration::from_millis(500));

        // We need a fresh process-stdout handle for the check_config read.
        // Since stdout was consumed, we restart with a two-call approach.
        let _ = child.kill();
        let _ = child.wait();

        // Restart: initialize + exec (triggers failure) + check_config — read check_config response.
        let dir2 = tempfile::tempdir().ok()?;
        std::fs::write(dir2.path().join(".iris-agentic-dev.toml"), toml_content).ok()?;
        let mut child2 = std::process::Command::new(&bin)
            .arg("mcp")
            .arg("--workspace")
            .arg(".")
            .current_dir(dir2.path())
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null())
            .spawn()
            .ok()?;
        let mut stdin2 = child2.stdin.take()?;
        let stdout2 = child2.stdout.take()?;

        stdin2.write_all(init.as_bytes()).ok();
        stdin2.write_all(notif.as_bytes()).ok();
        stdin2.write_all(exec_call.as_bytes()).ok();

        // Flush and wait briefly for exec + background audit task.
        std::thread::sleep(Duration::from_millis(600));

        let cc_call = "{\"jsonrpc\":\"2.0\",\"id\":3,\"method\":\"tools/call\",\"params\":{\"name\":\"check_config\",\"arguments\":{}}}\n";
        stdin2.write_all(cc_call.as_bytes()).ok();

        let result = read_until(stdout2, 15000, |v| {
            if v.get("id")?.as_u64()? != 3 {
                return None;
            }
            let content = v.get("result")?.get("content")?.as_array()?;
            for c in content {
                let text = c.get("text")?.as_str()?;
                if let Ok(obj) = serde_json::from_str::<serde_json::Value>(text.trim()) {
                    return Some(obj);
                }
            }
            None
        });

        let _ = child2.kill();
        let _ = child2.wait();
        result
    };

    let cc_with = run_and_get_check_config(&config_with_audit);
    eprintln!("T038 check_config (with irisAudit): {:?}", cc_with);

    let cc_without = run_and_get_check_config(&config_without_audit);
    eprintln!("T038 check_config (without irisAudit): {:?}", cc_without);

    // With irisAudit = true and no event definition: failures > 0 must appear.
    let failures_with = cc_with
        .as_ref()
        .and_then(|v| v.get("iris_audit_failures"))
        .and_then(|v| v.as_u64())
        .unwrap_or(0);
    assert!(
        failures_with > 0,
        "T038: check_config must report iris_audit_failures > 0 when emission fails; got: {:?}",
        cc_with
    );

    // Without irisAudit: field must be absent (no failures, no field).
    let failures_without = cc_without
        .as_ref()
        .and_then(|v| v.get("iris_audit_failures"));
    assert!(
        failures_without.is_none(),
        "T038: check_config must not report iris_audit_failures when irisAudit is absent; got: {:?}",
        cc_without
    );
}

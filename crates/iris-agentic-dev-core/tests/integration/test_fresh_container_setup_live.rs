// Live IRIS integration tests for 099-fresh-container-setup actions.
// Requires: iris-dev-iris running at localhost:52780
// Run: IRIS_HOST=localhost IRIS_WEB_PORT=52780
//        cargo test --test test_fresh_container_setup_live -- --ignored --test-threads=1
//
// The gates are pinned in `admin_call_live` below, not asked of the operator: an instruction in a
// comment is not enforcement, and the version of this header that asked for
// IRIS_WRITE_TOOLS_ENABLED=1 meant the suite silently measured the caller's shell instead.

#![allow(unused)]

fn admin_call_live(action: serde_json::Value, extra_env: &[(&str, &str)]) -> serde_json::Value {
    use std::io::{BufRead, BufReader, Write};
    use std::process::{Command, Stdio};

    let workspace_root = {
        let mut p = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        p.pop();
        p.pop();
        p
    };
    let bin = {
        let mut found = workspace_root.join("target/debug/iris-agentic-dev");
        for subdir in [
            "target/debug/iris-agentic-dev",
            "target/release/iris-agentic-dev",
            "target/llvm-cov-target/debug/iris-agentic-dev",
            "target/llvm-cov-target/release/iris-agentic-dev",
        ] {
            let c = workspace_root.join(subdir);
            if c.exists() {
                found = c;
                break;
            }
        }
        found
    };

    let iris_host = std::env::var("IRIS_HOST").unwrap_or_default();
    let iris_port = std::env::var("IRIS_WEB_PORT").unwrap_or_else(|_| "52780".to_string());

    let mut cmd = Command::new(&bin);
    cmd.args(["mcp"])
        .env("IRIS_HOST", &iris_host)
        .env("IRIS_WEB_PORT", &iris_port)
        .env(
            "IRIS_USERNAME",
            std::env::var("IRIS_USERNAME").unwrap_or_else(|_| "_SYSTEM".into()),
        )
        .env(
            "IRIS_PASSWORD",
            std::env::var("IRIS_PASSWORD").unwrap_or_else(|_| "SYS".into()),
        )
        .env("IRIS_NAMESPACE", "USER")
        .env("IRIS_TOOLSET", "merged")
        // Every test here calls a write action on `iris_admin` and asserts `success == true`, so the
        // gate state is part of the test, not part of the operator's shell. Declared here rather
        // than requested in the header: unpinned, these passed in the CI e2e job (which exports both
        // gates at job level) and returned WRITE_TOOLS_DISABLED for anyone who followed the run
        // instructions literally.
        .env("IRIS_WRITE_TOOLS_ENABLED", "1")
        // The three actions under test — clear_password_change_flag, unlock_user,
        // fresh_container_setup — are WriteClass::Write, while `iris_admin`'s default is
        // Destructive (write_gate.rs:542-548). Pinning this off is what keeps that classification
        // asserted instead of hidden behind a leaked destructive gate.
        .env("IRIS_DESTRUCTIVE_TOOLS_ENABLED", "0")
        // Admin writes are behind a second, independent gate (`admin::admin_writes_enabled`), so
        // without this every call answers ADMIN_WRITE_DISABLED before reaching IRIS.
        .env("IRIS_ADMIN_TOOLS", "1");

    for (k, v) in extra_env {
        cmd.env(k, v);
    }

    let mut child = cmd
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn iris-agentic-dev mcp");

    let mut stdin = child.stdin.take().unwrap();
    let stdout = child.stdout.take().unwrap();
    let mut reader = BufReader::new(stdout);

    let messages = vec![
        serde_json::json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"live-test","version":"0.1"}}}),
        serde_json::json!({"jsonrpc":"2.0","method":"notifications/initialized","params":{}}),
        serde_json::json!({"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"iris_admin","arguments":action}}),
    ];

    let mut results = vec![];
    for msg in &messages {
        stdin
            .write_all((serde_json::to_string(msg).unwrap() + "\n").as_bytes())
            .unwrap();
        stdin.flush().unwrap();
        if msg.get("id").is_some() {
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
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
    child.wait().ok();

    let resp = results.iter().find(|r| r["id"] == 2).cloned();
    resp.map(|r| {
        let text = r["result"]["content"][0]["text"].as_str().unwrap_or("{}");
        serde_json::from_str(text).unwrap_or_default()
    })
    .unwrap_or_default()
}

fn iris_available() -> bool {
    !std::env::var("IRIS_HOST").unwrap_or_default().is_empty()
}

const WRITE_ENV: &[(&str, &str)] = &[("IRIS_WRITE_TOOLS_ENABLED", "1"), ("IRIS_ADMIN_TOOLS", "1")];

// ── US1: clear_password_change_flag ─────────────────────────────────────────

#[test]
#[ignore = "requires live IRIS with IRIS_WRITE_TOOLS_ENABLED=1"]
fn test_clear_password_change_flag_idempotent() {
    assert!(iris_available(), "IRIS_HOST must be set");
    let r = admin_call_live(
        serde_json::json!({"action":"clear_password_change_flag"}),
        WRITE_ENV,
    );
    assert_eq!(
        r["success"], true,
        "clear_password_change_flag failed: {:?}",
        r
    );
    assert_eq!(
        r["flag_cleared"], true,
        "flag_cleared must be true: {:?}",
        r
    );
    assert_eq!(
        r["username"], "_SYSTEM",
        "username must default to _SYSTEM: {:?}",
        r
    );

    // Second call — idempotent
    let r2 = admin_call_live(
        serde_json::json!({"action":"clear_password_change_flag"}),
        WRITE_ENV,
    );
    assert_eq!(
        r2["success"], true,
        "second call (idempotent) failed: {:?}",
        r2
    );
}

// ── US3: unlock_user ─────────────────────────────────────────────────────────

#[test]
#[ignore = "requires live IRIS with IRIS_WRITE_TOOLS_ENABLED=1"]
fn test_unlock_user_idempotent() {
    assert!(iris_available(), "IRIS_HOST must be set");
    let r = admin_call_live(
        serde_json::json!({"action":"unlock_user","username":"_SYSTEM"}),
        WRITE_ENV,
    );
    assert_eq!(r["success"], true, "unlock_user failed: {:?}", r);
    assert_eq!(r["unlocked"], true, "unlocked must be true: {:?}", r);
    assert_eq!(r["username"], "_SYSTEM", "username: {:?}", r);

    // Second call — idempotent
    let r2 = admin_call_live(
        serde_json::json!({"action":"unlock_user","username":"_SYSTEM"}),
        WRITE_ENV,
    );
    assert_eq!(r2["success"], true, "second unlock_user failed: {:?}", r2);
}

// ── US2: fresh_container_setup ───────────────────────────────────────────────

#[test]
#[ignore = "requires live IRIS with IRIS_WRITE_TOOLS_ENABLED=1"]
fn test_fresh_container_setup_idempotent() {
    assert!(iris_available(), "IRIS_HOST must be set");
    let r = admin_call_live(
        serde_json::json!({"action":"fresh_container_setup"}),
        WRITE_ENV,
    );
    assert_eq!(r["success"], true, "fresh_container_setup failed: {:?}", r);
    assert_eq!(r["ready"], true, "ready must be true: {:?}", r);

    let steps = r["steps"].as_array().expect("steps must be array");
    assert_eq!(steps.len(), 2, "must have 2 steps: {:?}", steps);

    let step_actions: Vec<&str> = steps.iter().filter_map(|s| s["action"].as_str()).collect();
    assert!(
        step_actions.contains(&"clear_password_change_flag"),
        "missing clear_password_change_flag step: {:?}",
        step_actions
    );
    assert!(
        step_actions.contains(&"unlock_user"),
        "missing unlock_user step: {:?}",
        step_actions
    );

    for step in steps {
        assert!(
            step["status"].as_str() == Some("ok"),
            "step status must be ok: {:?}",
            step
        );
    }

    // Second call — idempotent
    let r2 = admin_call_live(
        serde_json::json!({"action":"fresh_container_setup"}),
        WRITE_ENV,
    );
    assert_eq!(r2["success"], true, "second call failed: {:?}", r2);
    assert_eq!(r2["ready"], true, "second call ready: {:?}", r2);
}

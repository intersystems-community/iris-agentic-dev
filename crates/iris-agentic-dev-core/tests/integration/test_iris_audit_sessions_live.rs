//! `irisAudit = true` must not open a CSP session per audited call.
//!
//! The emission ran its generator class through a client with no cookie jar, so every request it
//! sent (store, compile, run, delete) arrived without `CSPSESSIONID` and IRIS opened a session for
//! it that nothing ended. On a Concurrent User key each one is a license unit until the web app's
//! timeout: the Enterprise e2e job (IRIS 2026.1, 128 units) hit `<LICENSE LIMIT EXCEEDED>` minutes
//! into its run, and a lab on the same key went from 1 unit to 25 in the three
//! `test_attribution_stdio` tests (measured 2026-11-11).
//!
//! The count is of `^%cspSession` entries in the server, before and after ten audited calls. Other
//! clients of the same container add noise, so the bound is loose: the leak opened several
//! sessions per call.
//!
//! Requires `iris-dev-iris` (`IRIS_CONTAINER` names it) and the built binary.

use iris_agentic_dev_core::testing::{live_env, require_iad_binary, McpSession};

fn session_count(container: &str) -> i64 {
    let out = std::process::Command::new("docker")
        .args(["exec", "-i", container, "iris", "session", "IRIS", "-U", "%SYS"])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .spawn()
        .and_then(|mut c| {
            use std::io::Write;
            c.stdin.take().expect("stdin").write_all(
                b"Set n=0,s=\"\" For { Set s=$O(^%cspSession(s)) Quit:s=\"\"  Set n=n+1 } Write \"N=\",n,!\nHalt\n",
            )?;
            c.wait_with_output()
        })
        .expect("docker exec must run");
    let text = String::from_utf8_lossy(&out.stdout);
    text.lines()
        .find_map(|l| l.trim().strip_prefix("N=").map(str::to_string))
        .and_then(|n| n.trim().parse().ok())
        .unwrap_or_else(|| panic!("no session count in: {text}"))
}

#[test]
#[ignore = "requires live IRIS (iris-dev-iris) and the built binary"]
fn audited_calls_share_the_connections_csp_session() {
    let Some(_bin) = require_iad_binary() else {
        return;
    };
    let container = std::env::var("IRIS_CONTAINER").unwrap_or_else(|_| "iris-dev-iris".into());
    let dir = tempfile::tempdir().unwrap();
    let home = tempfile::tempdir().unwrap();
    let env_or = |k: &str, d: &str| std::env::var(k).unwrap_or_else(|_| d.to_string());
    std::fs::write(
        dir.path().join(".iris-agentic-dev.toml"),
        format!(
            "host = \"{}\"\nweb_port = {}\nusername = \"{}\"\npassword = \"{}\"\nnamespace = \"USER\"\n\n\
             [policy.default]\nirisAudit = true\n",
            env_or("IRIS_HOST", "localhost"),
            env_or("IRIS_WEB_PORT", "52780"),
            env_or("IRIS_USERNAME", "_SYSTEM"),
            env_or("IRIS_PASSWORD", "SYS"),
        ),
    )
    .unwrap();
    let mut env: Vec<(String, String)> = live_env()
        .into_iter()
        .filter(|(k, _)| k == "IRIS_CONTAINER")
        .collect();
    env.push(("HOME".into(), home.path().display().to_string()));

    let mut mcp = McpSession::start_in(Some(dir.path()), &env);
    // The first call opens the connection's session; count from after it.
    let _ = mcp.call("iris_query", &serde_json::json!({"query": "SELECT 1 AS x"}));
    std::thread::sleep(std::time::Duration::from_secs(3));
    let before = session_count(&container);
    for _ in 0..10 {
        let answer = mcp.call("iris_query", &serde_json::json!({"query": "SELECT 1 AS x"}));
        assert!(
            answer.to_string().contains("\\\"success\\\":true"),
            "the audited call itself must work: {answer}"
        );
    }
    // The emission runs in the background after each answer.
    std::thread::sleep(std::time::Duration::from_secs(5));
    let after = session_count(&container);
    assert!(
        after - before <= 3,
        "ten audited calls opened {} CSP sessions ({before} -> {after}); each one holds a \
         license unit until the web app's timeout",
        after - before
    );
}

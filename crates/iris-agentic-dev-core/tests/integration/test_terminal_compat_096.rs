//! Integration tests for spec 096 — terminal-mode ObjectScript compatibility.
//!
//! T024: Compile-and-run escape hatch — write a .mac with {} block syntax,
//! compile it, run the entry point via iris_execute, assert output.
//!
//! Requires live iris-dev-iris at localhost:52780.
//!
//! Run with:
//!   IRIS_HOST=localhost IRIS_WEB_PORT=52780 IRIS_USERNAME=_SYSTEM IRIS_PASSWORD=SYS \
//!   cargo test -p iris-agentic-dev-core --features testing \
//!     --test test_terminal_compat_096 -- --include-ignored --test-threads=1 --nocapture

use iris_agentic_dev_core::iris::connection::DiscoverySource;
use iris_agentic_dev_core::iris::IrisConnection;
use iris_agentic_dev_core::tools::IrisTools;

fn live_iris() -> IrisConnection {
    let host = std::env::var("IRIS_HOST").unwrap_or_else(|_| "localhost".into());
    let port: u16 = std::env::var("IRIS_WEB_PORT")
        .ok()
        .and_then(|p| p.parse().ok())
        .unwrap_or(52780);
    let user = std::env::var("IRIS_USERNAME").unwrap_or_else(|_| "_SYSTEM".into());
    let pass = std::env::var("IRIS_PASSWORD").unwrap_or_else(|_| "SYS".into());
    IrisConnection::new(
        format!("http://{}:{}", host, port),
        "USER",
        user,
        pass,
        DiscoverySource::EnvVar,
    )
}

fn parse_result(r: Result<rmcp::model::CallToolResult, String>) -> serde_json::Value {
    let result = r.expect("call_for_test returned Err");
    let text = result
        .content
        .first()
        .expect("result has no content")
        .as_text()
        .expect("content is not text")
        .text
        .clone();
    serde_json::from_str(&text).expect("response is not valid JSON")
}

/// T024 — compile-and-run escape hatch.
///
/// Writes a .mac routine containing `{}` block syntax via `iris_doc`,
/// compiles it with `iris_compile`, then calls `iris_execute Do entry^TERMTEST096`
/// and asserts the output contains `"compat_ok"`. Cleans up afterward.
#[tokio::test]
#[ignore]
async fn test_compile_and_run_escape_hatch() {
    let tools = IrisTools::new(Some(live_iris())).expect("IrisTools::new");

    const ROUTINE: &str = "TERMTEST096";

    // Clean up any stale routine from a previous aborted run.
    let _ = tools
        .call_for_test(
            "iris_doc",
            serde_json::json!({
                "mode": "delete",
                "name": format!("{ROUTINE}.mac"),
            }),
        )
        .await;

    // ── Step 1: Write .mac routine with {} block syntax ───────────────────────
    // Use double-quotes in the IRIS string literal, escaped for Rust.
    let mac_content = "TERMTEST096\nentry\n    If (1=1) { Write \"compat_ok\",! }\n    Quit\n";

    let write_v = parse_result(
        tools
            .call_for_test(
                "iris_doc",
                serde_json::json!({
                    "mode": "put",
                    "name": format!("{ROUTINE}.mac"),
                    "content": mac_content,
                }),
            )
            .await,
    );

    assert!(
        write_v.get("error").is_none() || write_v["success"].as_bool().unwrap_or(true),
        "T024: iris_doc put {ROUTINE}.mac must succeed; got: {write_v}"
    );

    // ── Step 2: Compile the routine ───────────────────────────────────────────
    let compile_v = parse_result(
        tools
            .call_for_test(
                "iris_compile",
                serde_json::json!({
                    "target": format!("{ROUTINE}.mac"),
                }),
            )
            .await,
    );

    let has_errors = compile_v["errors"]
        .as_array()
        .map(|a| !a.is_empty())
        .unwrap_or(false);
    assert!(
        !has_errors,
        "T024: compile must succeed with no errors; got: {compile_v}"
    );

    // ── Step 3: Run via iris_execute ──────────────────────────────────────────
    let exec_v = parse_result(
        tools
            .call_for_test(
                "iris_execute",
                serde_json::json!({
                    "code": format!("Do entry^{ROUTINE}"),
                }),
            )
            .await,
    );

    // ── Step 4: Clean up — delete routine ─────────────────────────────────────
    let _ = tools
        .call_for_test(
            "iris_doc",
            serde_json::json!({
                "mode": "delete",
                "name": format!("{ROUTINE}.mac"),
            }),
        )
        .await;

    // ── Assert after cleanup ───────────────────────────────────────────────────
    assert_eq!(
        exec_v["success"].as_bool(),
        Some(true),
        "T024: iris_execute must succeed; got: {exec_v}"
    );
    let output = exec_v["output"].as_str().unwrap_or("");
    assert!(
        output.contains("compat_ok"),
        "T024: output must contain 'compat_ok'; got output={output:?}, full={exec_v}"
    );
}

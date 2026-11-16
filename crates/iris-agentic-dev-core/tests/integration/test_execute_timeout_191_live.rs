//! `iris_execute`'s `timeout` reaches the request that runs the code (issue #191).
//!
//! The Atelier client had a fixed 30 s cap, and the call's `timeout` only wrapped the run from
//! outside, so `timeout = 120` with a 40 s run failed at 30 s as "may have run". The query request
//! now carries the call's timeout (plus a margin, so the outer `TIMEOUT` still fires first), up to
//! a cap, and a call above the cap is told what applied.
//!
//! IRIS_HOST=localhost IRIS_WEB_PORT=52780 cargo test --features testing --test integration \
//!   test_execute_timeout_191_live -- --include-ignored --test-threads=1

use iris_agentic_dev_core::iris::connection::{DiscoverySource, IrisConnection};
use iris_agentic_dev_core::tools::{IrisTools, Toolset};
use std::time::{Duration, Instant};

fn tools() -> IrisTools {
    let host = std::env::var("IRIS_HOST").expect("IRIS_HOST must name the live IRIS");
    let port: u16 = std::env::var("IRIS_WEB_PORT")
        .ok()
        .and_then(|p| p.parse().ok())
        .unwrap_or(52780);
    let user = std::env::var("IRIS_USERNAME").unwrap_or_else(|_| "_SYSTEM".into());
    let pass = std::env::var("IRIS_PASSWORD").unwrap_or_else(|_| "SYS".into());
    let conn = IrisConnection::new(
        format!("http://{host}:{port}"),
        "USER",
        user,
        pass,
        DiscoverySource::EnvVar,
    );
    IrisTools::for_test_gates_open(Some(conn), Toolset::Merged).unwrap()
}

async fn run(t: &IrisTools, code: &str, timeout: u64) -> (serde_json::Value, Duration) {
    let started = Instant::now();
    let r = t
        .call_gated(
            "iris_execute",
            serde_json::json!({"code": code, "timeout": timeout}),
        )
        .await
        .unwrap();
    let text = r.content[0].as_text().unwrap().text.clone();
    (serde_json::from_str(&text).unwrap(), started.elapsed())
}

/// The bug: a 35 s run under `timeout = 60` was cut at 30 s.
#[tokio::test]
#[ignore]
async fn a_run_longer_than_30_seconds_finishes_within_its_timeout() {
    let (body, took) = run(&tools(), r#"Hang 35 Write "done35""#, 60).await;
    assert_eq!(body["success"], true, "a 35 s run under timeout=60: {body}");
    assert_eq!(body["output"], "done35", "{body}");
    assert!(took >= Duration::from_secs(35), "it did not wait: {took:?}");
}

/// Control, passes before and after: a run that outlasts its own `timeout` is `TIMEOUT`, at that
/// timeout, not at some other time.
#[tokio::test]
#[ignore]
async fn a_run_that_outlasts_its_timeout_is_a_timeout_at_that_timeout() {
    let (body, took) = run(&tools(), "Hang 25 Write 1", 10).await;
    assert_eq!(body["error_code"], "TIMEOUT", "{body}");
    assert!(
        took >= Duration::from_secs(10) && took < Duration::from_secs(20),
        "TIMEOUT should fire at about 10 s, took {took:?}"
    );
}

/// A timeout above the cap runs under the cap, and the answer says so.
#[tokio::test]
#[ignore]
async fn a_timeout_above_the_cap_runs_under_the_cap_and_says_so() {
    let (body, _) = run(&tools(), "Write 1", 100_000).await;
    assert_eq!(body["success"], true, "{body}");
    assert_eq!(
        body["timeout_effective_s"], 600,
        "a call above the cap must be told the timeout that applied: {body}"
    );
    // Within the cap nothing is added.
    let (body, _) = run(&tools(), "Write 1", 120).await;
    assert!(
        body.get("timeout_effective_s").is_none(),
        "a timeout within the cap must not add a field: {body}"
    );
}

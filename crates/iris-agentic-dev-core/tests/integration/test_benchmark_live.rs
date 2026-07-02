//! Integration test for the benchmark harness (059-tool-telemetry-benchmark, US1).
//! Requires live IRIS on iris-dev-iris (port from IRIS_HOST/IRIS_WEB_PORT env) and
//! IRIS_GENERATE_CLASS_MODEL + an API key (OPENAI_API_KEY/ANTHROPIC_API_KEY) for the
//! LLM-fix step. Run with:
//!   cargo test -p iris-agentic-dev-core --test test_benchmark_live -- --ignored

use iris_agentic_dev_core::benchmark::container::{
    confirm_reachable, run_class_tests, write_and_compile,
};
use iris_agentic_dev_core::benchmark::{
    acquire_lock, load_tasks, release_lock, run_suite, LockResult,
};
use iris_agentic_dev_core::iris::connection::{DiscoverySource, IrisConnection};
use std::path::PathBuf;
use std::sync::Arc;

fn live_iris() -> IrisConnection {
    let host = std::env::var("IRIS_HOST").unwrap_or_else(|_| "localhost".into());
    let port: u16 = std::env::var("IRIS_WEB_PORT")
        .ok()
        .and_then(|p| p.parse().ok())
        .unwrap_or(52780);
    let user = std::env::var("IRIS_USERNAME").unwrap_or_else(|_| "_SYSTEM".into());
    let pass = std::env::var("IRIS_PASSWORD").unwrap_or_else(|_| "SYS".into());
    IrisConnection::new(
        format!("http://{host}:{port}"),
        "USER",
        user,
        pass,
        DiscoverySource::EnvVar,
    )
}

/// SC-004: 100% of the ported primary task suite's tasks execute to a pass/fail/error
/// verdict (none silently skipped or hung) in a single benchmark run.
#[tokio::test]
#[ignore]
async fn live_benchmark_run_covers_all_tasks_with_a_verdict() {
    let iris = live_iris();
    let client = IrisConnection::http_client().unwrap();
    let tasks_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src/benchmark/tasks/jira_bugs");
    let tasks = load_tasks(&tasks_dir).expect("tasks should load");
    assert_eq!(tasks.len(), 22);

    let result = run_suite(&iris, &client, "USER", &tasks, "", "test").await;

    assert_eq!(result.tasks_total, 22);
    assert_eq!(
        result.task_results.len(),
        22,
        "every task must produce exactly one TaskResult — none silently skipped"
    );
    for tr in &result.task_results {
        // Every outcome must be one of the three defined variants — this is guaranteed
        // by the enum's exhaustiveness, but assert a verdict was actually recorded
        // (iterations >= 1, elapsed_s recorded) rather than a hung/zeroed placeholder.
        assert!(tr.iterations >= 1);
        assert!(tr.elapsed_s >= 0.0);
    }
    assert!(result.pass_rate >= 0.0 && result.pass_rate <= 1.0);
}

/// US1 Acceptance Scenario 2: lift is present and computed as pass_rate - baseline_pass_rate
/// (absolute difference, FR-004) when a baseline run is included.
#[tokio::test]
#[ignore]
async fn live_benchmark_baseline_computes_absolute_difference_lift() {
    let iris = live_iris();
    let client = IrisConnection::http_client().unwrap();
    let tasks_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src/benchmark/tasks/jira_bugs");
    let tasks = load_tasks(&tasks_dir).expect("tasks should load");

    let mut result = run_suite(&iris, &client, "USER", &tasks, "", "test").await;
    let baseline = run_suite(&iris, &client, "USER", &tasks, "", "test").await;
    result.apply_baseline(baseline.pass_rate);

    assert!(result.baseline_pass_rate.is_some());
    let expected_lift = result.pass_rate - baseline.pass_rate;
    assert!((result.lift.unwrap() - expected_lift).abs() < 1e-9);
}

/// FR-013: acquiring the lock, then immediately attempting a second acquire, must report
/// AlreadyRunning; releasing it, then re-acquiring, must succeed.
#[tokio::test]
#[ignore]
async fn live_acquire_lock_blocks_second_run_until_released() {
    let iris = live_iris();
    let client = IrisConnection::http_client().unwrap();
    let container = format!("test-lock-{}", uuid::Uuid::new_v4());

    let first = acquire_lock(&iris, &client, "USER", &container, 600).await;
    assert_eq!(first, LockResult::Acquired);

    let second = acquire_lock(&iris, &client, "USER", &container, 600).await;
    assert_eq!(second, LockResult::AlreadyRunning);

    release_lock(&iris, &client, "USER", &container).await;

    let third = acquire_lock(&iris, &client, "USER", &container, 600).await;
    assert_eq!(third, LockResult::Acquired);
    release_lock(&iris, &client, "USER", &container).await;
}

/// FR-013: a lock older than max_age_secs is treated as abandoned and overridden.
#[tokio::test]
#[ignore]
async fn live_acquire_lock_overrides_stale_lock() {
    let iris = live_iris();
    let client = IrisConnection::http_client().unwrap();
    let container = format!("test-stale-lock-{}", uuid::Uuid::new_v4());

    let first = acquire_lock(&iris, &client, "USER", &container, 600).await;
    assert_eq!(first, LockResult::Acquired);

    tokio::time::sleep(std::time::Duration::from_secs(2)).await;
    // max_age_secs=0 with an existing lock >=1s old: age > max_age_secs is true.
    let second = acquire_lock(&iris, &client, "USER", &container, 0).await;
    assert_eq!(second, LockResult::Acquired);
    release_lock(&iris, &client, "USER", &container).await;
}

/// container.rs primitives: confirm_reachable succeeds against a live connection, and
/// run_class_tests reports CLASS_NOT_FOUND for a nonexistent class rather than panicking.
#[tokio::test]
#[ignore]
async fn live_confirm_reachable_and_run_class_tests_class_not_found() {
    let iris = Arc::new(live_iris());
    let client = IrisConnection::http_client().unwrap();
    confirm_reachable(&iris, &client)
        .await
        .expect("should be reachable");

    let (passed, detail) = run_class_tests(&iris, &client, "USER", "Bogus.DoesNotExist999")
        .await
        .unwrap();
    assert!(!passed);
    assert!(detail.contains("CLASS_NOT_FOUND"));
}

/// container.rs: write_and_compile surfaces a compile error (not a panic/Ok) for
/// syntactically invalid source.
#[tokio::test]
#[ignore]
async fn live_write_and_compile_reports_syntax_errors() {
    let iris = live_iris();
    let client = IrisConnection::http_client().unwrap();
    let bad_src = "Class ZzBadSyntax.Broken Extends %RegisteredObject\n{\nClassMethod Foo() {\n    this is not valid objectscript !!!\n}\n}\n";
    let errors = write_and_compile(&iris, &client, "USER", "ZzBadSyntax.Broken.cls", bad_src)
        .await
        .expect("PUT+compile request itself should succeed even if compile has errors");
    assert!(
        !errors.is_empty(),
        "syntactically invalid source must report compile errors"
    );
}

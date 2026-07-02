//! Unit tests for BenchmarkResult scoring (T013). No live IRIS required.

use iris_agentic_dev_core::benchmark::{BenchmarkResult, TaskOutcome, TaskResult};

fn task(outcome: TaskOutcome) -> TaskResult {
    TaskResult {
        task_id: "jira-001".to_string(),
        outcome,
        iterations: 1,
        elapsed_s: 1.0,
        reason: String::new(),
    }
}

#[test]
fn pass_rate_excludes_errored_tasks_from_denominator() {
    let results = vec![
        task(TaskOutcome::Pass),
        task(TaskOutcome::Pass),
        task(TaskOutcome::Fail),
        task(TaskOutcome::Error),
    ];
    let result = BenchmarkResult::from_task_results(results, "2026.1".to_string(), 10.0);
    // tasks_total (for pass_rate) = passed + failed = 3, not including the errored task.
    assert_eq!(result.tasks_passed, 2);
    assert_eq!(result.tasks_errored, 1);
    assert!((result.pass_rate - (2.0 / 3.0)).abs() < 1e-9);
}

#[test]
fn pass_rate_all_pass_is_one() {
    let results = vec![task(TaskOutcome::Pass), task(TaskOutcome::Pass)];
    let result = BenchmarkResult::from_task_results(results, "2026.1".to_string(), 5.0);
    assert_eq!(result.pass_rate, 1.0);
    assert_eq!(result.tasks_errored, 0);
}

#[test]
fn pass_rate_all_errored_is_zero_and_does_not_panic() {
    let results = vec![task(TaskOutcome::Error), task(TaskOutcome::Error)];
    let result = BenchmarkResult::from_task_results(results, "2026.1".to_string(), 5.0);
    assert_eq!(result.pass_rate, 0.0);
    assert_eq!(result.tasks_errored, 2);
    assert_eq!(result.tasks_passed, 0);
}

#[test]
fn baseline_and_lift_default_to_none() {
    let results = vec![task(TaskOutcome::Pass)];
    let result = BenchmarkResult::from_task_results(results, "2026.1".to_string(), 1.0);
    assert!(result.baseline_pass_rate.is_none());
    assert!(result.lift.is_none());
}

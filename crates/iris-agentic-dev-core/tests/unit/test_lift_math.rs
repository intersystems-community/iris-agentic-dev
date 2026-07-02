//! Unit tests for lift computation (T014, FR-004: absolute difference). No live IRIS.

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
fn lift_is_absolute_difference_not_relative_percentage() {
    let results = vec![
        task(TaskOutcome::Pass),
        task(TaskOutcome::Pass),
        task(TaskOutcome::Fail),
        task(TaskOutcome::Fail),
    ]; // pass_rate = 0.5
    let mut result = BenchmarkResult::from_task_results(results, "2026.1".to_string(), 10.0);
    result.apply_baseline(0.2);
    assert_eq!(result.baseline_pass_rate, Some(0.2));
    // 0.5 - 0.2 = 0.3, NOT (0.5-0.2)/0.2*100 = 150.
    assert!((result.lift.unwrap() - 0.3).abs() < 1e-9);
}

#[test]
fn lift_is_none_when_no_baseline_applied() {
    let results = vec![task(TaskOutcome::Pass)];
    let result = BenchmarkResult::from_task_results(results, "2026.1".to_string(), 1.0);
    assert!(result.lift.is_none());
}

#[test]
fn lift_can_be_negative_when_baseline_beats_skill() {
    let results = vec![task(TaskOutcome::Fail)]; // pass_rate = 0.0
    let mut result = BenchmarkResult::from_task_results(results, "2026.1".to_string(), 1.0);
    result.apply_baseline(0.5);
    assert!((result.lift.unwrap() - (-0.5)).abs() < 1e-9);
}

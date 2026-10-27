//! R3: the benchmark's IRIS steps go through the tool pipeline.
//!
//! `GatedTarget` sends each step through `call_gated`. A gate's refusal is recorded, the suite
//! stops at the task that met it, and the lock functions say nothing was taken. Nothing listens at
//! the rig's default address, so a step that got past a gate would fail with a connection error,
//! which is never taken for a refusal.

use crate::support_gate_141::*;
use iris_agentic_dev_core::benchmark::target::{BenchTarget, GatedTarget};
use iris_agentic_dev_core::benchmark::{
    acquire_lock_via, load_embedded_tasks, run_suite_via, TaskOutcome,
};

const GATES_OPEN: &str = "write_tools_enabled = true\ndestructive_tools_enabled = true\n";

#[tokio::test]
async fn a_refused_lock_is_recorded_and_names_the_gate() {
    let rig = rig(&format!(
        "{GATES_OPEN}[policy.default]\nmcpTemplate = \"live\"\n"
    ));
    let target = GatedTarget::new(&rig.tools);
    assert!(!target.refused());
    let _ = acquire_lock_via(&target, "USER", "c", 60).await;
    assert!(target.refused(), "the live template let the lock through");
    let body = target.take_refusal().expect("a recorded refusal");
    assert_eq!(body["error_code"], "ENV_GATE_BLOCKED", "{body}");
    assert!(body["refusal"].is_object(), "{body}");
    assert!(!target.refused(), "taking the refusal clears it");
}

#[tokio::test]
async fn the_suite_stops_at_the_task_that_met_a_refusal() {
    // `execute` allowed, `compile` not: the first fixture the first task stores is refused.
    let rig = rig(&format!(
        "{GATES_OPEN}[policy.default]\nallow = [\"execute\"]\n"
    ));
    let target = GatedTarget::new(&rig.tools);
    let tasks = load_embedded_tasks().unwrap();
    assert!(tasks.len() > 3);
    let result = run_suite_via(&target, "USER", &tasks, "", "test").await;
    assert_eq!(
        result.task_results.len(),
        1,
        "the suite ran past a refusal: {:?}",
        result.task_results
    );
    assert_eq!(result.task_results[0].outcome, TaskOutcome::Error);
    assert!(
        result.task_results[0].reason.contains("POLICY_GATE"),
        "{}",
        result.task_results[0].reason
    );
    let body = target.take_refusal().expect("a recorded refusal");
    assert_eq!(body["blocked_category"], "compile", "{body}");
}

#[tokio::test]
async fn a_failure_that_is_not_a_refusal_does_not_stop_the_suite() {
    // Every gate open, nothing listening: each task errors on the connection and the next runs.
    let rig = rig(GATES_OPEN);
    let target = GatedTarget::new(&rig.tools);
    let tasks = load_embedded_tasks().unwrap();
    let result = run_suite_via(&target, "USER", &tasks, "", "test").await;
    assert_eq!(result.task_results.len(), tasks.len());
    assert!(!target.refused());
    assert!(result
        .task_results
        .iter()
        .all(|r| r.outcome == TaskOutcome::Error));
}

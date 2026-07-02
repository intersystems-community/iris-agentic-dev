//! Unit tests for loading the ported jira_bugs task suite (T015). No live IRIS.

use iris_agentic_dev_core::benchmark::load_tasks;
use std::path::PathBuf;

fn tasks_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src/benchmark/tasks/jira_bugs")
}

#[test]
fn loads_exactly_22_primary_suite_tasks() {
    let tasks = load_tasks(&tasks_dir()).expect("tasks should load");
    assert_eq!(tasks.len(), 22);
}

#[test]
fn every_task_has_non_empty_required_fields() {
    let tasks = load_tasks(&tasks_dir()).expect("tasks should load");
    for task in &tasks {
        assert!(!task.task_id.is_empty(), "task_id must not be empty");
        assert!(
            !task.initial_code.files.is_empty(),
            "initial_code.files must not be empty for {}",
            task.task_id
        );
        assert!(
            !task.test_code.content.is_empty(),
            "test_code.content must not be empty for {}",
            task.task_id
        );
    }
}

#[test]
fn loading_nonexistent_dir_errors_rather_than_panics() {
    let result = load_tasks(&PathBuf::from("/nonexistent/path/that/does/not/exist"));
    assert!(result.is_err());
}

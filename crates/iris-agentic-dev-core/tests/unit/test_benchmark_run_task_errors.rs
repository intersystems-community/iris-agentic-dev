//! Unit tests for run_task's Error-outcome path (FR-012) — no live IRIS required,
//! since these fail before any HTTP call reaches the connection.

use iris_agentic_dev_core::benchmark::{
    run_task, BenchmarkTask, InitialCode, SourceFile, SuccessCriteria, TestCode,
};
use iris_agentic_dev_core::iris::connection::{DiscoverySource, IrisConnection};

fn malformed_task() -> BenchmarkTask {
    BenchmarkTask {
        task_id: "jira-999".to_string(),
        category: "jira_bugs".to_string(),
        difficulty: "easy".to_string(),
        description: "desc".to_string(),
        goal: "goal".to_string(),
        initial_code: InitialCode {
            files: vec![SourceFile {
                path: "src/Foo.cls".to_string(),
                // No "Class " declaration at all — extract_class_name returns None.
                content: "this is not a class declaration".to_string(),
            }],
        },
        test_code: TestCode {
            path: "tests/TestFoo.cls".to_string(),
            content: "Class TestFoo Extends %RegisteredObject {}".to_string(),
        },
        expected_behavior: "behaves".to_string(),
        hints: vec![],
        success_criteria: SuccessCriteria {
            compile_success: true,
            tests_pass: true,
            max_patch_lines: 30,
            requires_symbol_preservation: true,
        },
        metadata: serde_json::Value::Null,
    }
}

#[tokio::test]
async fn run_task_maps_malformed_source_to_error_outcome_not_fail() {
    let iris = IrisConnection::new(
        "http://127.0.0.1:1",
        "USER",
        "_SYSTEM",
        "SYS",
        DiscoverySource::ExplicitFlag,
    );
    let client = IrisConnection::http_client().unwrap();
    let task = malformed_task();

    let result = run_task(&iris, &client, "USER", &task, "").await;

    assert_eq!(result.task_id, "jira-999");
    assert_eq!(
        format!("{:?}", result.outcome),
        "Error",
        "a task with no extractable Class declaration must be Error, not Fail"
    );
    assert!(!result.reason.is_empty());
    assert!(result.iterations >= 1);
    assert!(result.elapsed_s >= 0.0);
}

#[tokio::test]
async fn run_task_maps_unreachable_iris_to_error_outcome() {
    let iris = IrisConnection::new(
        "http://127.0.0.1:1",
        "USER",
        "_SYSTEM",
        "SYS",
        DiscoverySource::ExplicitFlag,
    );
    let client = IrisConnection::http_client().unwrap();
    let task = iris_agentic_dev_core::benchmark::load_tasks(
        &std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/benchmark/tasks/jira_bugs"),
    )
    .unwrap()
    .into_iter()
    .next()
    .unwrap();

    let result = run_task(&iris, &client, "USER", &task, "").await;
    assert_eq!(
        format!("{:?}", result.outcome),
        "Error",
        "an unreachable IRIS connection must surface as Error, not silently Pass/Fail"
    );
}

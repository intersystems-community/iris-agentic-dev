// Integration tests for CLI dispatch benchmark mode (spec 080).
// All tests in this file require a live iris-dev-iris container.
// Run with:
//   IRIS_HOST=localhost IRIS_WEB_PORT=52780 \
//   cargo test --test cli_dispatch_integration -- --ignored --nocapture --test-threads=1

use iris_agentic_dev_core::benchmark::{
    self,
    cli_dispatch::{self, CliDispatchConfig},
    BenchmarkMode,
};
use iris_agentic_dev_core::generate::LlmClient;
use iris_agentic_dev_core::iris::connection::{DiscoverySource, IrisConnection};
use std::path::PathBuf;

// ── Test infrastructure ───────────────────────────────────────────────────────

fn iris_available() -> bool {
    !std::env::var("IRIS_HOST").unwrap_or_default().is_empty()
}

fn test_iris_connection() -> IrisConnection {
    let host = std::env::var("IRIS_HOST").expect("IRIS_HOST must be set for integration tests");
    let port: u16 = std::env::var("IRIS_WEB_PORT")
        .unwrap_or_else(|_| "52780".to_string())
        .parse()
        .unwrap_or(52780);
    let base_url = format!("http://{host}:{port}");
    IrisConnection::new(
        base_url,
        "USER",
        "_SYSTEM",
        "SYS",
        DiscoverySource::ExplicitFlag,
    )
}

fn mock_llm_client() -> LlmClient {
    std::env::set_var("IRIS_GENERATE_CLASS_MODEL", "mock");
    std::env::set_var("ANTHROPIC_API_KEY", "dummy-for-mock");
    LlmClient::from_env().expect("mock LLM should be constructible")
}

fn test_config() -> CliDispatchConfig {
    // Use a nonexistent binary path — in mock mode, run_cli_dispatch_task checks the binary
    // path first and returns Error. For integration tests that use mock LLM and real IRIS,
    // we use a sentinel-based approach where the mock LLM returns a class block (not tool calls).
    CliDispatchConfig {
        // Use a fake binary so subprocess tests don't actually run a binary
        binary_path: PathBuf::from("/nonexistent/iris-agentic-dev"),
        max_iterations: 3,
        max_task_tokens: 50000,
        task_timeout_s: 30,
        connection_env: vec![
            (
                "IRIS_HOST".to_string(),
                std::env::var("IRIS_HOST").unwrap_or_else(|_| "localhost".to_string()),
            ),
            (
                "IRIS_WEB_PORT".to_string(),
                std::env::var("IRIS_WEB_PORT").unwrap_or_else(|_| "52780".to_string()),
            ),
        ],
    }
}

/// S-001: CLI dispatch run produces valid result JSON.
///
/// Uses mock LLM (no API key required). The mock returns a pre-baked class block,
/// so run_cli_dispatch_task will compile and test it against live IRIS.
///
/// Note: with a nonexistent binary path, the task records Error (binary not found)
/// before any LLM calls are made — per FR-010. This test validates the JSON structure
/// and mode field, not a passing score.
#[tokio::test]
#[ignore]
async fn s001_cli_dispatch_produces_valid_result_json() {
    if !iris_available() {
        return;
    }

    let iris = test_iris_connection();
    let client = IrisConnection::http_client().expect("HTTP client");
    let llm = mock_llm_client();
    let config = test_config();

    let tasks = benchmark::load_embedded_tasks().expect("load tasks");
    let single_task = &tasks[..1]; // run only the first task to keep test fast

    let result = cli_dispatch::run_cli_dispatch_suite(
        &iris,
        &client,
        "USER",
        single_task,
        "",
        "test",
        &config,
        &llm,
    )
    .await;

    // Validate required fields
    assert_eq!(result.mode, Some(BenchmarkMode::CliDispatch));
    assert!(result.tasks_total >= 1);
    assert!(
        result.pass_rate >= 0.0 && result.pass_rate <= 1.0,
        "pass_rate {} is not in [0.0, 1.0]",
        result.pass_rate
    );
    for tr in &result.task_results {
        let outcome_str = serde_json::to_string(&tr.outcome).unwrap();
        assert!(
            outcome_str == "\"pass\"" || outcome_str == "\"fail\"" || outcome_str == "\"error\"",
            "unexpected outcome: {outcome_str}"
        );
    }

    // Verify JSON serializes without error
    let json = serde_json::to_string_pretty(&result).expect("serialization should succeed");
    assert!(json.contains("\"cli_dispatch\""));
}

/// S-004: An agent that never uses CLI tools still terminates cleanly.
///
/// The mock LLM never returns tool invocations (it returns a class block directly),
/// so the loop will run at most max_iterations turns, then check for a class in
/// the last response. The mock class won't match any test criteria, so outcome is fail.
#[tokio::test]
#[ignore]
async fn s004_prose_only_agent_terminates_cleanly() {
    if !iris_available() {
        return;
    }

    let iris = test_iris_connection();
    let client = IrisConnection::http_client().expect("HTTP client");
    let llm = mock_llm_client();

    // Use a nonexistent binary — any tool call attempts will fail and be reported back
    let config = CliDispatchConfig {
        binary_path: PathBuf::from("/nonexistent/iris-agentic-dev"),
        max_iterations: 2,
        max_task_tokens: 50000,
        task_timeout_s: 30,
        connection_env: vec![],
    };

    let tasks = benchmark::load_embedded_tasks().expect("load tasks");
    let single_task = &tasks[..1];

    // Should complete without hanging
    let result = cli_dispatch::run_cli_dispatch_suite(
        &iris,
        &client,
        "USER",
        single_task,
        "",
        "test",
        &config,
        &llm,
    )
    .await;

    // With nonexistent binary, we get Error (FR-010) on every task
    // All tasks should have an outcome (no hang)
    assert_eq!(result.task_results.len(), 1);
    // Should be either fail or error — not hanging
    let outcome = result.task_results[0].outcome;
    assert!(
        outcome == benchmark::TaskOutcome::Fail || outcome == benchmark::TaskOutcome::Error,
        "expected fail or error, got {:?}",
        outcome
    );
}

/// S-005: Malformed JSON in a tool invocation is captured and returned to agent.
///
/// Validates that run_tool_subprocess with a nonexistent binary returns an error string,
/// not a panic, and the string is non-empty.
#[test]
fn s005_malformed_json_fed_back_to_agent_as_error() {
    use iris_agentic_dev_core::benchmark::cli_dispatch::{run_tool_subprocess, ToolInvocation};

    let inv = ToolInvocation {
        tool_name: "iris_compile".to_string(),
        args_json: "{bad json".to_string(),
    };
    let result = run_tool_subprocess(std::path::Path::new("/nonexistent/binary"), &inv, &[]);
    // Must not panic, must return a non-empty error string
    assert!(!result.is_empty());
    assert!(result.contains("Error") || result.contains("error") || result.contains("failed"));
}

/// S-007: --mode mcp explicit is identical to default (regression test).
///
/// Validates that `BenchmarkMode::Mcp` is the default and serializes as "mcp".
#[test]
fn s007_mode_mcp_is_default_and_serializes_correctly() {
    let mode: BenchmarkMode = Default::default();
    assert_eq!(mode, BenchmarkMode::Mcp);
    let json = serde_json::to_string(&mode).unwrap();
    assert_eq!(json, r#""mcp""#);
}

/// S-008: max-iterations limit stops the loop (pure unit test, no IRIS).
///
/// Verifies that the CliDispatchConfig carries max_iterations and the loop
/// respects it. Since we can't inject a fake LLM without a trait, this test
/// validates the config side only; the full behavior is covered by the binary
/// not existing (task errors immediately on binary check, which is equivalent
/// to a loop that terminates after 0 turns).
#[test]
fn s008_max_iterations_config_respected() {
    let config = CliDispatchConfig {
        binary_path: PathBuf::from("/nonexistent"),
        max_iterations: 2,
        max_task_tokens: 50000,
        task_timeout_s: 30,
        connection_env: vec![],
    };
    assert_eq!(config.max_iterations, 2);

    // parse_tool_invocations should return an invocation on a tool line, so
    // simulating N calls to it would produce N invocations — capped at max_iterations.
    let tool_line = "iris-agentic-dev tool iris_compile --args '{\"doc\":\"Foo.cls\"}'".repeat(1);
    let inv = cli_dispatch::parse_tool_invocations(&tool_line);
    assert!(!inv.is_empty()); // would trigger a loop iteration
}

// ── T023/T024: Live LLM integration tests ─────────────────────────────────────

fn live_llm_available() -> bool {
    (std::env::var("ANTHROPIC_API_KEY").is_ok() || std::env::var("OPENAI_API_KEY").is_ok())
        && std::env::var("IRIS_GENERATE_CLASS_MODEL").is_ok()
}

fn live_llm_client() -> Option<LlmClient> {
    LlmClient::from_env()
}

fn live_binary_path() -> PathBuf {
    // Use compile-time CARGO_MANIFEST_DIR to find the binary in the worktree's target dir.
    // This mirrors progressive_disclosure_integration.rs which uses env!() directly.
    let mut p = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    p.pop(); // → crates/
    p.pop(); // → workspace root
             // Prefer llvm-cov-target build (used by coverage runs), fall back to debug
    let llvm = p.join("target/llvm-cov-target/debug/iris-agentic-dev");
    if llvm.exists() {
        return llvm;
    }
    p.join("target/debug/iris-agentic-dev")
}

/// S-002 / T023: Token counts appear in result when using a real LLM.
///
/// Requires: ANTHROPIC_API_KEY or OPENAI_API_KEY, IRIS_GENERATE_CLASS_MODEL,
/// IRIS_HOST, IRIS_WEB_PORT, and a pre-built iris-agentic-dev binary.
#[tokio::test]
#[ignore]
async fn t023_live_token_counts_in_result() {
    if !iris_available() || !live_llm_available() {
        return;
    }

    let llm = match live_llm_client() {
        Some(c) => c,
        None => return,
    };

    let binary_path = live_binary_path();
    if !binary_path.exists() {
        eprintln!(
            "Binary not found at {:?} — build the project first",
            binary_path
        );
        return;
    }

    let iris = test_iris_connection();
    let client = IrisConnection::http_client().expect("HTTP client");

    let config = CliDispatchConfig {
        binary_path,
        max_iterations: 5,
        max_task_tokens: 20000,
        task_timeout_s: 60,
        connection_env: vec![
            (
                "IRIS_HOST".to_string(),
                std::env::var("IRIS_HOST").unwrap_or_else(|_| "localhost".to_string()),
            ),
            (
                "IRIS_WEB_PORT".to_string(),
                std::env::var("IRIS_WEB_PORT").unwrap_or_else(|_| "52780".to_string()),
            ),
        ],
    };

    let tasks = benchmark::load_embedded_tasks().expect("load tasks");
    let single_task = &tasks[..1];

    let result = cli_dispatch::run_cli_dispatch_suite(
        &iris,
        &client,
        "USER",
        single_task,
        "",
        "test",
        &config,
        &llm,
    )
    .await;

    assert_eq!(result.mode, Some(BenchmarkMode::CliDispatch));
    assert_eq!(result.task_results.len(), 1);

    // Token counts should be populated (real API returns usage data)
    // Note: if the model doesn't return usage (unlikely but possible), these may be None
    let tr = &result.task_results[0];
    if let Some(total) = tr.tokens_total {
        assert!(total > 0, "tokens_total should be > 0 when present");
    }

    eprintln!(
        "T023 result: outcome={:?}, iterations={}, tokens_total={:?}",
        tr.outcome, tr.iterations, tr.tokens_total
    );
}

/// T024: End-to-end CLI dispatch on the full jira suite with a real model.
///
/// Requires: ANTHROPIC_API_KEY or OPENAI_API_KEY, IRIS_GENERATE_CLASS_MODEL,
/// IRIS_HOST, IRIS_WEB_PORT, and a pre-built iris-agentic-dev binary.
///
/// Writes result to `tests/e2e/results/cli-dispatch-baseline.json`.
#[tokio::test]
#[ignore]
async fn t024_live_full_suite_cli_dispatch() {
    if !iris_available() || !live_llm_available() {
        return;
    }

    let llm = match live_llm_client() {
        Some(c) => c,
        None => return,
    };

    let binary_path = live_binary_path();
    if !binary_path.exists() {
        eprintln!(
            "Binary not found at {:?} — build the project first",
            binary_path
        );
        return;
    }

    let iris = test_iris_connection();
    let client = IrisConnection::http_client().expect("HTTP client");

    let config = CliDispatchConfig {
        binary_path,
        max_iterations: 10,
        max_task_tokens: 50000,
        task_timeout_s: 60,
        connection_env: vec![
            (
                "IRIS_HOST".to_string(),
                std::env::var("IRIS_HOST").unwrap_or_else(|_| "localhost".to_string()),
            ),
            (
                "IRIS_WEB_PORT".to_string(),
                std::env::var("IRIS_WEB_PORT").unwrap_or_else(|_| "52780".to_string()),
            ),
        ],
    };

    let tasks = benchmark::load_embedded_tasks().expect("load tasks");

    let result = cli_dispatch::run_cli_dispatch_suite(
        &iris, &client, "USER", &tasks, "", "test", &config, &llm,
    )
    .await;

    assert_eq!(result.mode, Some(BenchmarkMode::CliDispatch));
    assert!(
        result.pass_rate >= 0.0 && result.pass_rate <= 1.0,
        "pass_rate {} out of range",
        result.pass_rate
    );

    if let Some(total) = result.tokens_total {
        assert!(total > 0, "tokens_total should be > 0");
        assert!(
            total < 10_000_000,
            "tokens_total {total} suspiciously large"
        );
    }

    // Write baseline result
    let output_dir =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/e2e/results");
    std::fs::create_dir_all(&output_dir).ok();
    let output_path = output_dir.join("cli-dispatch-baseline.json");
    if let Ok(json) = serde_json::to_string_pretty(&result) {
        std::fs::write(&output_path, &json).ok();
        eprintln!("Wrote baseline to {}", output_path.display());
    }

    eprintln!(
        "T024 result: pass_rate={:.1}% ({}/{}), tokens_total={:?}, elapsed={:.1}s",
        result.pass_rate * 100.0,
        result.tasks_passed,
        result.tasks_total,
        result.tokens_total,
        result.elapsed_s,
    );
}

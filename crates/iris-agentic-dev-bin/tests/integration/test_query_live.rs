use std::process::Command;

fn env_or(key: &str, default: &str) -> String {
    std::env::var(key).unwrap_or_else(|_| default.to_string())
}

fn iris_dev() -> Command {
    let bin = env!("CARGO_BIN_EXE_iris-agentic-dev");
    let mut cmd = Command::new(bin);
    cmd.env("IRIS_HOST", env_or("IRIS_HOST", "localhost"))
        .env("IRIS_WEB_PORT", env_or("IRIS_WEB_PORT", "52780"))
        .env("IRIS_NAMESPACE", env_or("IRIS_NAMESPACE", "USER"))
        .env("IRIS_USERNAME", env_or("IRIS_USERNAME", "_SYSTEM"))
        .env("IRIS_PASSWORD", env_or("IRIS_PASSWORD", "SYS"));
    cmd
}

#[test]
#[ignore]
fn test_query_dictionary_returns_tsv() {
    let out = iris_dev()
        .args([
            "query",
            "--namespace",
            "%SYS",
            "SELECT TOP 5 Name FROM %Dictionary.ClassDefinition",
        ])
        .output()
        .expect("failed to run iris-agentic-dev");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "expected exit 0\nstdout: {}", stdout);
    // First line must be the header
    let first = stdout.lines().next().unwrap_or("");
    assert_eq!(first, "Name", "expected TSV header 'Name', got: {}", first);
    // At least one data row
    assert!(
        stdout.lines().count() > 1,
        "expected at least one data row, got: {}",
        stdout
    );
}

#[test]
#[ignore]
fn test_query_zero_rows_exits_zero() {
    // Atelier returns empty content array for zero rows — no column metadata available.
    // The command exits 0 with empty stdout (no header possible without column info).
    let out = iris_dev()
        .args(["query", "SELECT 1 AS val WHERE 1=0"])
        .output()
        .expect("failed to run iris-agentic-dev");
    assert!(
        out.status.success(),
        "expected exit 0 for zero-row query, got {:?}",
        out.status
    );
}

#[test]
#[ignore]
fn test_query_sql_error_exits_nonzero() {
    let out = iris_dev()
        .args(["query", "SELECT FROM WHERE THIS IS NOT SQL"])
        .output()
        .expect("failed to run iris-agentic-dev");
    assert!(
        !out.status.success(),
        "expected non-zero exit for SQL syntax error"
    );
}

#[test]
#[ignore]
fn test_query_output_is_pipe_safe() {
    let out = iris_dev()
        .args([
            "query",
            "--namespace",
            "%SYS",
            "SELECT TOP 2 Name FROM %Dictionary.ClassDefinition",
        ])
        .output()
        .expect("failed to run iris-agentic-dev");
    let stdout = String::from_utf8_lossy(&out.stdout);
    // No spurious framing: every line is tab-separated tokens, no spinner/progress text
    for line in stdout.lines() {
        assert!(
            !line.starts_with('[') && !line.starts_with("Connecting"),
            "spurious framing line: {}",
            line
        );
    }
}

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
fn test_tool_iris_info_exits_zero() {
    let out = iris_dev()
        .args(["tool", "iris_info", "--args", r#"{"what":"namespace"}"#])
        .output()
        .expect("failed to run iris-agentic-dev");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "expected exit 0\nstdout: {}", stdout);
    assert!(
        !stdout.trim().is_empty(),
        "expected non-empty output from iris_info"
    );
    // SC-004: connection identity visible in output
    assert!(
        stdout.contains("USER") || stdout.contains("namespace") || stdout.contains("success"),
        "expected namespace info in output, got: {}",
        &stdout[..stdout.len().min(400)]
    );
}

#[test]
#[ignore]
fn test_tool_check_config_shows_host() {
    let out = iris_dev()
        .args(["tool", "check_config", "--args", "{}"])
        .output()
        .expect("failed to run iris-agentic-dev");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "expected exit 0\nstdout: {}", stdout);
    // SC-004: active IRIS host/namespace visible
    assert!(
        stdout.contains("localhost") || stdout.contains("USER") || stdout.contains("connected"),
        "expected host/namespace in check_config output, got: {}",
        &stdout[..stdout.len().min(400)]
    );
}

#[test]
#[ignore]
fn test_tool_unknown_name_exits_one() {
    let out = iris_dev()
        .args(["tool", "nonexistent_tool_xyz"])
        .output()
        .expect("failed to run iris-agentic-dev");
    assert_eq!(
        out.status.code(),
        Some(1),
        "expected exit code 1 for unknown tool"
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("iris_execute"),
        "expected tool list in stderr, got: {}",
        stderr
    );
    assert!(
        stderr.contains("iris_compile"),
        "expected iris_compile in tool list, got: {}",
        stderr
    );
}

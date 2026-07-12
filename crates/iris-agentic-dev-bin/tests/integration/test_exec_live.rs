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
        .env("IRIS_PASSWORD", env_or("IRIS_PASSWORD", "SYS"))
        .env("IRIS_ALLOW_PROD", "1");
    cmd
}

#[test]
#[ignore]
fn test_exec_zversion() {
    let out = iris_dev()
        .args(["exec", "write $ZVersion,!"])
        .output()
        .expect("failed to run iris-agentic-dev");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        out.status.success(),
        "expected exit 0, got {:?}\nstdout: {}",
        out.status,
        stdout
    );
    assert!(!stdout.trim().is_empty(), "expected non-empty output");
    assert!(
        stdout.contains("IRIS"),
        "expected version string to contain 'IRIS', got: {}",
        stdout
    );
}

#[test]
#[ignore]
fn test_exec_macro_ok() {
    let out = iris_dev()
        .args(["exec", "write $$$OK,!"])
        .output()
        .expect("failed to run iris-agentic-dev");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "expected exit 0\nstdout: {}", stdout);
    assert_eq!(stdout.trim(), "1", "expected $$$OK=1, got: {}", stdout);
}

#[test]
#[ignore]
fn test_exec_file() {
    let tmp = tempfile::NamedTempFile::with_suffix(".cos").unwrap();
    std::fs::write(tmp.path(), "write \"hello-from-file\",!\n").unwrap();
    let out = iris_dev()
        .args(["exec", "--file", tmp.path().to_str().unwrap()])
        .output()
        .expect("failed to run iris-agentic-dev");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "expected exit 0\nstdout: {}", stdout);
    assert!(
        stdout.contains("hello-from-file"),
        "expected output, got: {}",
        stdout
    );
}

#[test]
#[ignore]
fn test_exec_runtime_error_in_output() {
    // IRIS runtime errors are reported in stdout (the HTTP generator returns 200 with error text).
    // The binary exits 0 but the error is visible — callers should inspect stdout for ERROR:.
    let out = iris_dev()
        .args(["exec", "do ##class(Nonexistent.Class).Method()"])
        .output()
        .expect("failed to run iris-agentic-dev");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        !stdout.is_empty(),
        "expected error text in stdout for IRIS runtime error"
    );
    assert!(
        stdout.contains("CLASS DOES NOT EXIST") || stdout.contains("ERROR"),
        "expected IRIS error text in stdout, got: {}",
        stdout
    );
}

#[test]
#[ignore]
fn test_exec_namespace_flag() {
    let out = iris_dev()
        .args(["exec", "--namespace", "USER", "write $namespace,!"])
        .output()
        .expect("failed to run iris-agentic-dev");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "expected exit 0\nstdout: {}", stdout);
    assert!(
        stdout.trim().eq_ignore_ascii_case("user"),
        "expected namespace USER, got: {}",
        stdout
    );
}

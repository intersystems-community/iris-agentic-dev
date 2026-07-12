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
fn test_doc_get_system_class() {
    let out = iris_dev()
        .args([
            "doc",
            "--namespace",
            "%SYS",
            "get",
            "%Dictionary.ClassDefinition",
        ])
        .output()
        .expect("failed to run iris-agentic-dev");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "expected exit 0\nstdout: {}", stdout);
    assert!(
        stdout.contains("Class %Dictionary.ClassDefinition"),
        "expected UDL Class header, got: {}",
        &stdout[..stdout.len().min(300)]
    );
}

#[test]
#[ignore]
fn test_doc_get_nonexistent_exits_nonzero() {
    let out = iris_dev()
        .args(["doc", "get", "Nonexistent.IrisDevTmp063Class"])
        .output()
        .expect("failed to run iris-agentic-dev");
    assert!(
        !out.status.success(),
        "expected non-zero exit for missing class"
    );
}

#[test]
#[ignore]
fn test_doc_put_get_roundtrip() {
    let class_name = format!("IrisDevTmp.DocTest063x{}", std::process::id());
    let class_name = class_name.as_str();
    let udl = format!(
        "Class {} {{\nClassMethod Ping() As %String {{ Return \"pong\" }}\n}}\n",
        class_name
    );
    let tmp = tempfile::Builder::new().suffix(".cls").tempfile().unwrap();
    std::fs::write(tmp.path(), &udl).unwrap();

    // Put
    let put_out = iris_dev()
        .args([
            "doc",
            "put",
            class_name,
            "--file",
            tmp.path().to_str().unwrap(),
        ])
        .output()
        .expect("failed to run iris-agentic-dev put");
    let put_stdout = String::from_utf8_lossy(&put_out.stdout);
    assert!(
        put_out.status.success(),
        "doc put failed\nstdout: {}",
        put_stdout
    );
    assert!(
        put_stdout.contains("OK:"),
        "expected 'OK:' from put, got: {}",
        put_stdout
    );

    // Get and verify content round-trips
    let get_out = iris_dev()
        .args(["doc", "get", class_name])
        .output()
        .expect("failed to run iris-agentic-dev get");
    let get_stdout = String::from_utf8_lossy(&get_out.stdout);
    assert!(
        get_out.status.success(),
        "doc get failed after put\nstdout: {}",
        get_stdout
    );
    assert!(
        get_stdout.contains("IrisDevTmp.DocTest063"),
        "expected class name prefix in get output, got: {}",
        &get_stdout[..get_stdout.len().min(300)]
    );
}

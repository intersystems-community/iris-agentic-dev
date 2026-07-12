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

fn write_valid_cls(name: &str) -> tempfile::NamedTempFile {
    let tmp = tempfile::Builder::new().suffix(".cls").tempfile().unwrap();
    std::fs::write(
        tmp.path(),
        format!(
            "Class {} {{\nClassMethod Test() {{\n write 1,!\n}}\n}}\n",
            name
        ),
    )
    .unwrap();
    tmp
}

fn unique_class_name(base: &str) -> String {
    format!("{}{}", base, std::process::id())
}

fn write_invalid_cls() -> tempfile::NamedTempFile {
    let tmp = tempfile::Builder::new().suffix(".cls").tempfile().unwrap();
    std::fs::write(tmp.path(), "this is not valid objectscript!!!\n").unwrap();
    tmp
}

#[test]
#[ignore]
fn test_compile_valid_file_exits_zero() {
    let name = unique_class_name("IrisDevTmp.CompileTest063x");
    let cls = write_valid_cls(&name);
    let out = iris_dev()
        .args(["compile", cls.path().to_str().unwrap()])
        .output()
        .expect("failed to run iris-agentic-dev");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "expected exit 0\nstdout: {}", stdout);
    assert!(
        stdout.contains("OK:"),
        "expected 'OK:' in output, got: {}",
        stdout
    );
}

#[test]
#[ignore]
fn test_compile_invalid_file_exits_nonzero() {
    let cls = write_invalid_cls();
    let out = iris_dev()
        .args(["compile", cls.path().to_str().unwrap()])
        .output()
        .expect("failed to run iris-agentic-dev");
    let stdout = String::from_utf8_lossy(&out.stdout);
    // Compile errors: either non-zero exit or ERROR: in output
    assert!(
        !out.status.success() || stdout.contains("ERROR:"),
        "expected non-zero exit or ERROR: for invalid class, got status={:?} stdout={}",
        out.status,
        stdout
    );
}

#[test]
#[ignore]
fn test_compile_no_args_reads_toml() {
    // With no args and no iris-dev.toml present, it should fail gracefully (not panic)
    let out = iris_dev()
        .args(["compile"])
        .current_dir("/tmp")
        .output()
        .expect("failed to run iris-agentic-dev");
    // Either exits non-zero (no toml) or succeeds — it must not panic
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        !stderr.contains("thread 'main' panicked"),
        "command panicked: {}",
        stderr
    );
}

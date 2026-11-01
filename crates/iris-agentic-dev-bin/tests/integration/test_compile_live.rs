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

/// A project directory of its own holding one `.cls` file. The system temporary directory is not
/// a project directory (it is a broad root), so the file cannot sit directly in it.
fn project_with(body: String) -> (tempfile::TempDir, std::path::PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let path = std::fs::canonicalize(dir.path()).unwrap().join("T.cls");
    std::fs::write(&path, body).unwrap();
    (dir, path)
}

fn write_valid_cls(name: &str) -> (tempfile::TempDir, std::path::PathBuf) {
    project_with(format!(
        "Class {} {{\nClassMethod Test() {{\n write 1,!\n}}\n}}\n",
        name
    ))
}

fn unique_class_name(base: &str) -> String {
    format!("{}{}", base, std::process::id())
}

fn write_invalid_cls() -> (tempfile::TempDir, std::path::PathBuf) {
    project_with("this is not valid objectscript!!!\n".to_string())
}

#[test]
#[ignore]
fn test_compile_valid_file_exits_zero() {
    let name = unique_class_name("IrisDevTmp.CompileTest063x");
    let (_dir, cls) = write_valid_cls(&name);
    let out = iris_dev()
        .args(["compile", cls.to_str().unwrap()])
        // The file is compiled only from inside the project directory, which is the working one.
        .current_dir(cls.parent().unwrap())
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
    let (_dir, cls) = write_invalid_cls();
    let out = iris_dev()
        .args(["compile", cls.to_str().unwrap()])
        // The file is compiled only from inside the project directory, which is the working one.
        .current_dir(cls.parent().unwrap())
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

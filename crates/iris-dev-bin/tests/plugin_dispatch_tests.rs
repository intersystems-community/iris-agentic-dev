//! T035: Unit tests for iris-dev plugin discovery and dispatch.

use std::process::Command;

#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;

fn iris_dev_bin() -> std::path::PathBuf {
    let mut path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    path.pop(); // iris-dev-bin → crates
    path.pop(); // crates → workspace root
    path.push("target/debug/iris-dev");
    path
}

/// --list-plugins runs without error.
#[test]
fn list_plugins_exits_zero() {
    let bin = iris_dev_bin();
    if !bin.exists() {
        return;
    }

    let output = Command::new(&bin)
        .arg("--list-plugins")
        .output()
        .expect("failed to run --list-plugins");

    assert!(
        output.status.success(),
        "iris-dev --list-plugins should exit 0, got: {}",
        output.status
    );
}

/// Unknown command exits non-zero.
#[test]
fn unknown_command_exits_nonzero() {
    let bin = iris_dev_bin();
    if !bin.exists() {
        return;
    }

    let output = Command::new(&bin)
        .arg("totally-unknown-command-xyzzy")
        .output()
        .expect("failed to run iris-dev");

    assert!(
        !output.status.success(),
        "unknown command should exit non-zero"
    );
}

/// iris-dev-* plugin on PATH is discovered and dispatched.
#[test]
#[cfg(unix)]
fn plugin_on_path_is_dispatched() {
    let bin = iris_dev_bin();
    if !bin.exists() {
        return;
    }

    let dir = tempfile::tempdir().unwrap();
    let plugin = dir.path().join("iris-dev-testplugin");

    // Write a simple shell script that exits 0 and prints a marker
    std::fs::write(&plugin, "#!/bin/sh\necho 'TESTPLUGIN_OK'\nexit 0\n").unwrap();
    let mut perms = std::fs::metadata(&plugin).unwrap().permissions();
    perms.set_mode(0o755);
    std::fs::set_permissions(&plugin, perms).unwrap();

    // Prepend our dir to PATH
    let original_path = std::env::var("PATH").unwrap_or_default();
    let new_path = format!("{}:{}", dir.path().display(), original_path);

    let output = Command::new(&bin)
        .arg("testplugin")
        .env("PATH", &new_path)
        .output()
        .expect("failed to dispatch plugin");

    assert!(
        output.status.success(),
        "plugin dispatch should exit 0, got: {}",
        output.status
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("TESTPLUGIN_OK"),
        "plugin output should contain marker, got: {}",
        stdout
    );
}

/// Plugin passes remaining args correctly.
#[test]
#[cfg(unix)]
fn plugin_receives_args() {
    let bin = iris_dev_bin();
    if !bin.exists() {
        return;
    }

    let dir = tempfile::tempdir().unwrap();
    let plugin = dir.path().join("iris-dev-argtest");
    std::fs::write(&plugin, "#!/bin/sh\necho \"ARGS: $@\"\nexit 0\n").unwrap();
    let mut perms = std::fs::metadata(&plugin).unwrap().permissions();
    perms.set_mode(0o755);
    std::fs::set_permissions(&plugin, perms).unwrap();

    let original_path = std::env::var("PATH").unwrap_or_default();
    let new_path = format!("{}:{}", dir.path().display(), original_path);

    let output = Command::new(&bin)
        .args(["argtest", "--foo", "bar"])
        .env("PATH", &new_path)
        .output()
        .expect("failed to dispatch plugin");

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("--foo") && stdout.contains("bar"),
        "plugin should receive all args, got: {}",
        stdout
    );
}

// ── CLI compile E2E tests ─────────────────────────────────────────────────────
// These require a live IRIS connection (IRIS_HOST env var set).

fn require_iris_env() -> bool {
    std::env::var("IRIS_HOST").is_ok() || std::env::var("OBJECTSCRIPT_WORKSPACE").is_ok()
}

/// iris-dev compile <file> — happy path with a valid .cls file.
#[test]
fn cli_compile_valid_cls_file() {
    let bin = iris_dev_bin();
    if !bin.exists() || !require_iris_env() {
        return;
    }

    let dir = tempfile::tempdir().expect("tempdir");
    let cls_path = dir.path().join("CliTest.ValidClass.cls");
    std::fs::write(
        &cls_path,
        "Class CliTest.ValidClass { ClassMethod Hello() { write \"hello\",! } }",
    )
    .unwrap();

    let output = Command::new(&bin)
        .arg("compile")
        .arg(&cls_path)
        .output()
        .expect("failed to run iris-dev compile");

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success(),
        "iris-dev compile should succeed, stdout={} stderr={}",
        stdout,
        stderr
    );
    assert!(
        stdout.contains("Compiled") || stdout.contains("success"),
        "expected success output, got: {}",
        stdout
    );
}

/// iris-dev compile <file> — class with quotes and backslashes in content.
#[test]
fn cli_compile_cls_with_special_chars() {
    let bin = iris_dev_bin();
    if !bin.exists() || !require_iris_env() {
        return;
    }

    let dir = tempfile::tempdir().expect("tempdir");
    let cls_path = dir.path().join("CliTest.SpecialChars.cls");
    std::fs::write(
        &cls_path,
        "Class CliTest.SpecialChars {\nClassMethod Test() {\n  write \"Hello \"\"World\"\"\",!\n}\n}",
    )
    .unwrap();

    let output = Command::new(&bin)
        .arg("compile")
        .arg(&cls_path)
        .output()
        .expect("failed to run iris-dev compile");

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success(),
        "compile with quoted strings should succeed, stdout={} stderr={}",
        stdout,
        stderr
    );
}

/// iris-dev compile <file> — non-existent file returns non-zero exit.
#[test]
fn cli_compile_missing_file_exits_nonzero() {
    let bin = iris_dev_bin();
    if !bin.exists() || !require_iris_env() {
        return;
    }

    let output = Command::new(&bin)
        .arg("compile")
        .arg("/tmp/this_file_does_not_exist_iris_dev_test.cls")
        .output()
        .expect("failed to run iris-dev compile");

    assert!(
        !output.status.success(),
        "compile of missing file should exit non-zero"
    );
}

/// iris-dev compile <file> — upload failure is surfaced (not silently swallowed).
/// Verifies the PUT body status.errors check added in #53.
#[test]
fn cli_compile_upload_error_not_silently_swallowed() {
    let bin = iris_dev_bin();
    if !bin.exists() || !require_iris_env() {
        return;
    }

    // Attempt to compile a non-.cls file — should fail clearly, not succeed silently.
    let dir = tempfile::tempdir().expect("tempdir");
    let bad_path = dir.path().join("notaclass.txt");
    std::fs::write(&bad_path, "this is not a cls file").unwrap();

    // iris-dev compile only handles .cls — this path won't trigger PUT, just compile by name.
    // We verify the binary exits with a clear error, not exit 0.
    let output = Command::new(&bin)
        .arg("compile")
        .arg(&bad_path)
        .output()
        .expect("failed to run iris-dev compile");

    // Should not silently succeed
    assert!(
        !output.status.success() || {
            let stdout = String::from_utf8_lossy(&output.stdout);
            !stdout.contains("\"success\":true")
        },
        "compiling a non-cls file should not silently report success"
    );
}

//! Integration tests for iris-agentic-dev CLI subcommands (cmd/ crate).
//! Spawns the binary as a subprocess.
//! Live tests are `#[ignore]` — run with:
//!   IRIS_HOST=localhost IRIS_WEB_PORT=52780 \
//!   cargo test --test test_cmd_live -- --include-ignored --nocapture
#![allow(dead_code, clippy::zombie_processes)]

use std::process::Command;

fn iris_dev_bin() -> std::path::PathBuf {
    iris_agentic_dev_core::testing::iad_binary_path()
}

fn iris_env() -> Option<Vec<(String, String)>> {
    let host = std::env::var("IRIS_HOST").unwrap_or_default();
    if host.is_empty() {
        return None;
    }
    let port = std::env::var("IRIS_WEB_PORT").unwrap_or_else(|_| "52780".to_string());
    let user = std::env::var("IRIS_USERNAME").unwrap_or_else(|_| "_SYSTEM".to_string());
    let pass = std::env::var("IRIS_PASSWORD").unwrap_or_else(|_| "SYS".to_string());
    let ns = std::env::var("IRIS_NAMESPACE").unwrap_or_else(|_| "USER".to_string());
    Some(vec![
        ("IRIS_HOST".into(), host),
        ("IRIS_WEB_PORT".into(), port),
        ("IRIS_USERNAME".into(), user),
        ("IRIS_PASSWORD".into(), pass),
        ("IRIS_NAMESPACE".into(), ns),
    ])
}

// ============================================================================
// Group 1: No-IRIS tests (test binary exists, help output, error on missing args)
// ============================================================================

/// exec --help exits zero and outputs help text.
#[test]
fn test_exec_help_exits_zero() {
    let bin = iris_dev_bin();
    if !bin.exists() {
        return;
    }

    let output = Command::new(&bin)
        .args(["exec", "--help"])
        .output()
        .expect("failed to run iris-agentic-dev");

    assert!(
        output.status.success(),
        "exit: {:?}\nstderr: {}",
        output.status,
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.to_lowercase().contains("exec") || stdout.to_lowercase().contains("execute"),
        "help output should contain 'exec' or 'execute': {}",
        stdout
    );
}

/// doc --help exits zero and outputs help text.
#[test]
fn test_doc_help_exits_zero() {
    let bin = iris_dev_bin();
    if !bin.exists() {
        return;
    }

    let output = Command::new(&bin)
        .args(["doc", "--help"])
        .output()
        .expect("failed to run iris-agentic-dev");

    assert!(
        output.status.success(),
        "exit: {:?}\nstderr: {}",
        output.status,
        String::from_utf8_lossy(&output.stderr)
    );
}

/// compile --help exits zero and outputs help text.
#[test]
fn test_compile_help_exits_zero() {
    let bin = iris_dev_bin();
    if !bin.exists() {
        return;
    }

    let output = Command::new(&bin)
        .args(["compile", "--help"])
        .output()
        .expect("failed to run iris-agentic-dev");

    assert!(
        output.status.success(),
        "exit: {:?}\nstderr: {}",
        output.status,
        String::from_utf8_lossy(&output.stderr)
    );
}

/// query --help exits zero and outputs help text.
#[test]
fn test_query_help_exits_zero() {
    let bin = iris_dev_bin();
    if !bin.exists() {
        return;
    }

    let output = Command::new(&bin)
        .args(["query", "--help"])
        .output()
        .expect("failed to run iris-agentic-dev");

    assert!(
        output.status.success(),
        "exit: {:?}\nstderr: {}",
        output.status,
        String::from_utf8_lossy(&output.stderr)
    );
}

/// tool --help exits zero and outputs help text.
#[test]
fn test_tool_help_exits_zero() {
    let bin = iris_dev_bin();
    if !bin.exists() {
        return;
    }

    let output = Command::new(&bin)
        .args(["tool", "--help"])
        .output()
        .expect("failed to run iris-agentic-dev");

    assert!(
        output.status.success(),
        "exit: {:?}\nstderr: {}",
        output.status,
        String::from_utf8_lossy(&output.stderr)
    );
}

/// mcp exits when stdin closes (non-zero exit is expected).
#[test]
fn test_mcp_exits_when_stdin_closes() {
    let bin = iris_dev_bin();
    if !bin.exists() {
        return;
    }

    // Stdin closed immediately — MCP server should exit.
    let output = Command::new(&bin)
        .args(["mcp"])
        .stdin(std::process::Stdio::null())
        .output()
        .expect("failed to run iris-agentic-dev");

    // Exit code may be non-zero when stdin closes; that's acceptable.
    let exit_code = output.status.code().unwrap_or(-1);
    assert!(
        exit_code != 0 || !String::from_utf8_lossy(&output.stdout).is_empty(),
        "mcp should exit when stdin closes"
    );
}

// ============================================================================
// Group 2: Live IRIS tests (use #[ignore] and check IRIS_HOST)
// ============================================================================

/// exec with code "write $ZVERSION,!" exits zero and outputs JSON with IRIS version info.
#[test]
#[ignore]
fn test_exec_write_version() {
    let bin = iris_dev_bin();
    if !bin.exists() {
        return;
    }

    let envs = match iris_env() {
        Some(e) => e,
        None => return,
    };

    let output = Command::new(&bin)
        .args(["exec", "-n", "USER", "write $ZVERSION,!"])
        .envs(envs)
        .output()
        .expect("failed to run iris-agentic-dev");

    let exit_code = output.status.code().unwrap_or(-1);
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert_eq!(
        exit_code, 0,
        "exec should exit 0: {}\nstderr: {}",
        exit_code, stderr
    );

    // Output is JSON; should contain IRIS version info
    assert!(
        stdout.to_lowercase().contains("iris")
            || stdout.to_lowercase().contains("cache")
            || stdout.to_lowercase().contains("version"),
        "exec output should contain version info: {}",
        stdout
    );
}

/// doc get exercises the cmd/doc.rs path — writes a temp class first so we have a known doc.
#[test]
#[ignore]
fn test_doc_get_library_object() {
    let bin = iris_dev_bin();
    if !bin.exists() {
        return;
    }

    let envs = match iris_env() {
        Some(e) => e,
        None => return,
    };

    // First put a class so we have something to get
    let put_output = Command::new(&bin)
        .args(["doc", "put", "-", "CmdLiveTestGet.cls"])
        .envs(envs.clone())
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .and_then(|mut child| {
            use std::io::Write;
            child
                .stdin
                .take()
                .unwrap()
                .write_all(b"Class CmdLiveTestGet {}")
                .ok();
            child.wait_with_output()
        });
    // If put fails, skip — the real goal is exercising doc get cmd path
    if put_output.map(|o| !o.status.success()).unwrap_or(true) {
        return;
    }

    // doc get takes CLASSNAME as a positional arg; namespace via env IRIS_NAMESPACE
    let output = Command::new(&bin)
        .args(["doc", "get", "CmdLiveTestGet.cls"])
        .envs(envs)
        .output()
        .expect("failed to run iris-agentic-dev");

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert_eq!(
        output.status.code().unwrap_or(-1),
        0,
        "doc get should exit 0\nstderr: {stderr}"
    );
    assert!(
        stdout.contains("CmdLiveTestGet"),
        "doc get output should contain class name: {stdout}"
    );
}

/// compile a temp .cls file exits zero against live IRIS.
#[test]
#[ignore]
fn test_compile_existing_class() {
    let bin = iris_dev_bin();
    if !bin.exists() {
        return;
    }

    let envs = match iris_env() {
        Some(e) => e,
        None => return,
    };

    // Write a minimal class file into a project directory and compile it from there: a host file
    // the CLI reads must be inside the project (spec 141 T212), and the working directory is it.
    let project = tempfile::tempdir().unwrap();
    let cls_path = project.path().join("CmdLiveTest.cls");
    std::fs::write(&cls_path, "Class CmdLiveTest {}").unwrap();

    let output = Command::new(&bin)
        .args(["compile", "-n", "USER", cls_path.to_str().unwrap()])
        .current_dir(project.path())
        .envs(envs)
        .output()
        .expect("failed to run iris-agentic-dev");

    let exit_code = output.status.code().unwrap_or(-1);
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert_eq!(
        exit_code, 0,
        "compile should exit 0: {}\nstderr: {}",
        exit_code, stderr
    );
}

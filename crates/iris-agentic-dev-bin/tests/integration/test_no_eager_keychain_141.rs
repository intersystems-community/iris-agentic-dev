//! Layer 2 for spec 141 T187: listing, help and version never open the OS keychain, and neither
//! does `mcp` until a call needs a credential.
//!
//! `tool --list` once hung for over 20 minutes on a Mac: building the tool router also built the
//! connection pool, and the pool read every configured server's password, which opened the
//! keychain and its approval dialog. A pending dialog is invisible to a test, so the binary is
//! given `IAD_TEST_KEYSTORE_ABORT` (debug builds only): reaching keystore initialisation then ends
//! the process with exit code 97 and a message, and the test fails in a second instead of hanging.
//!
//! The home directory holds what makes the old code reach for the keychain: a `servers.json` entry
//! marked `credential = "server-manager"` and a Server Manager profile of the same name. No IRIS is
//! involved, and the real keychain is never touched because the abort fires before it.

#![cfg(unix)]

use std::io::{BufRead, Write};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use iris_agentic_dev_core::iris::server_manager::{KEYSTORE_ABORT_EXIT, KEYSTORE_ABORT_MESSAGE};

fn bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_iris-agentic-dev"))
}

/// A home with a configured server whose password lives in the Server Manager's keychain item.
struct Home {
    _tmp: tempfile::TempDir,
    path: PathBuf,
}

impl Home {
    fn new() -> Home {
        let tmp = tempfile::tempdir().unwrap();
        let path = std::fs::canonicalize(tmp.path()).unwrap();
        let servers = path.join(".config/iris-agentic-dev");
        std::fs::create_dir_all(&servers).unwrap();
        std::fs::write(
            servers.join("servers.json"),
            r#"{"version":1,"default":"dev","servers":{"dev":{"host":"127.0.0.1","port":1,
               "namespace":"USER","username":"_SYSTEM","credential":"server-manager"}}}"#,
        )
        .unwrap();
        let settings = if cfg!(target_os = "macos") {
            path.join("Library/Application Support/Code/User")
        } else {
            path.join(".config/Code/User")
        };
        std::fs::create_dir_all(&settings).unwrap();
        std::fs::write(
            settings.join("settings.json"),
            r#"{"intersystems.servers":{"dev":{"webServer":{"scheme":"http","host":"127.0.0.1",
               "port":1},"username":"_SYSTEM"}}}"#,
        )
        .unwrap();
        Home { _tmp: tmp, path }
    }

    /// The binary with a clean environment, this home, and the keychain abort armed. `IAD_NO_KEYCHAIN`
    /// is removed: with it set the old code never reached the keychain either, which hid the hang.
    fn command(&self) -> Command {
        let mut cmd = iris_agentic_dev_core::testing::clean_command(&bin());
        cmd.env("HOME", &self.path)
            .env("USERPROFILE", &self.path)
            .env_remove("IAD_NO_KEYCHAIN")
            .env("IAD_TEST_KEYSTORE_ABORT", "1")
            .current_dir(&self.path);
        cmd
    }
}

struct Ran {
    code: Option<i32>,
    stdout: String,
    stderr: String,
    elapsed: Duration,
}

/// Run to completion, or fail after `limit`: a hang is a failure here, never a wait.
fn run(mut cmd: Command, limit: Duration) -> Ran {
    let started = Instant::now();
    let mut child = cmd
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn the binary");
    let out = child.stdout.take().unwrap();
    let err = child.stderr.take().unwrap();
    let read = |r: std::process::ChildStdout| {
        std::thread::spawn(move || {
            let mut s = String::new();
            let _ = std::io::Read::read_to_string(&mut std::io::BufReader::new(r), &mut s);
            s
        })
    };
    let out_t = read(out);
    let err_t = std::thread::spawn(move || {
        let mut s = String::new();
        let _ = std::io::Read::read_to_string(&mut std::io::BufReader::new(err), &mut s);
        s
    });
    loop {
        if let Some(status) = child.try_wait().unwrap() {
            return Ran {
                code: status.code(),
                stdout: out_t.join().unwrap(),
                stderr: err_t.join().unwrap(),
                elapsed: started.elapsed(),
            };
        }
        if started.elapsed() > limit {
            iris_agentic_dev_core::testing::stop_server(&mut child);
            panic!(
                "the process was still running after {limit:?} (a hang, or a keychain prompt)\n\
                 stderr so far: {}",
                err_t.join().unwrap()
            );
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

const LIMIT: Duration = Duration::from_secs(3);

fn assert_no_keystore(what: &str, r: &Ran) {
    assert!(
        r.code != Some(KEYSTORE_ABORT_EXIT) && !r.stderr.contains(KEYSTORE_ABORT_MESSAGE),
        "{what} reached keystore initialisation\nstderr: {}",
        r.stderr
    );
}

#[test]
fn tool_list_finishes_without_opening_the_keychain() {
    let home = Home::new();
    let mut cmd = home.command();
    cmd.args(["tool", "--list"]);
    let r = run(cmd, LIMIT);
    assert_no_keystore("tool --list", &r);
    assert_eq!(r.code, Some(0), "stderr: {}", r.stderr);
    assert!(r.stdout.contains("iris_info"), "listing: {}", r.stdout);
    assert!(r.elapsed < LIMIT);
}

#[test]
fn tool_list_json_finishes_without_opening_the_keychain() {
    let home = Home::new();
    let mut cmd = home.command();
    cmd.args(["tool", "--list", "--json"]);
    let r = run(cmd, LIMIT);
    assert_no_keystore("tool --list --json", &r);
    assert_eq!(r.code, Some(0), "stderr: {}", r.stderr);
    let v: serde_json::Value = serde_json::from_str(r.stdout.trim()).expect("one JSON document");
    assert!(v["count"].as_u64().unwrap() > 50);
}

#[test]
fn tool_schema_finishes_without_opening_the_keychain() {
    let home = Home::new();
    let mut cmd = home.command();
    cmd.args(["tool", "iris_query", "--schema", "--json"]);
    let r = run(cmd, LIMIT);
    assert_no_keystore("tool iris_query --schema", &r);
    assert_eq!(r.code, Some(0), "stderr: {}", r.stderr);
    let v: serde_json::Value = serde_json::from_str(r.stdout.trim()).expect("one JSON document");
    assert_eq!(v["name"], "iris_query");
    assert!(v["inputSchema"]["properties"].is_object());
}

#[test]
fn version_and_help_never_open_the_keychain() {
    let home = Home::new();
    for args in [
        vec!["--version"],
        vec!["--help"],
        vec!["tool", "--help"],
        vec!["mcp", "--help"],
        vec!["skill", "--help"],
        vec!["check-config", "--help"],
    ] {
        let mut cmd = home.command();
        cmd.args(&args);
        let r = run(cmd, LIMIT);
        assert_no_keystore(&format!("iris-agentic-dev {}", args.join(" ")), &r);
    }
}

#[test]
fn version_prints_and_help_prints_usage() {
    let home = Home::new();
    let mut cmd = home.command();
    cmd.arg("--version");
    let r = run(cmd, LIMIT);
    assert_eq!(r.code, Some(0));
    assert!(r.stdout.starts_with("iris-agentic-dev "), "{}", r.stdout);
    let mut cmd = home.command();
    cmd.arg("--help");
    let r = run(cmd, LIMIT);
    assert_eq!(r.code, Some(0));
    assert!(r.stdout.contains("Usage"), "{}", r.stdout);
}

/// The control: the same home does reach the keychain when a call needs a credential, so the
/// tests above pass because listing does not need one, not because the fixture or the abort hook is
/// inert. `tool check_config` resolves the connection first, and the Server Manager profile's
/// password is read for it.
#[test]
fn a_call_that_needs_a_credential_does_reach_the_keystore() {
    let home = Home::new();
    let mut cmd = home.command();
    cmd.args(["tool", "check_config"]);
    let r = run(cmd, LIMIT);
    assert_eq!(
        r.code,
        Some(KEYSTORE_ABORT_EXIT),
        "the fixture should need the keychain here\nstdout: {}\nstderr: {}",
        r.stdout,
        r.stderr
    );
    assert!(r.stderr.contains(KEYSTORE_ABORT_MESSAGE));
}

fn send(stdin: &mut std::process::ChildStdin, line: &str) {
    writeln!(stdin, "{line}").unwrap();
    stdin.flush().unwrap();
}

/// Read stdout lines until the response to request `id` arrives, or fail after `limit`.
fn response(
    lines: &std::sync::mpsc::Receiver<String>,
    id: u64,
    limit: Duration,
) -> serde_json::Value {
    let deadline = Instant::now() + limit;
    loop {
        let left = deadline
            .checked_duration_since(Instant::now())
            .unwrap_or_default();
        let line = lines
            .recv_timeout(left)
            .unwrap_or_else(|_| panic!("no response to request {id} within {limit:?}"));
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(line.trim()) {
            if v["id"] == id {
                return v;
            }
        }
    }
}

/// `mcp` starts, lists its tools and answers a call that targets the default connection without
/// the keychain. The configured server is never asked for, so its password is never read; the
/// connection comes from `IRIS_HOST`, so discovery does not read the Server Manager's either.
#[test]
fn mcp_does_not_open_the_keychain_until_a_server_is_asked_for() {
    let home = Home::new();
    let err_path = home.path.join("mcp-stderr.txt");
    let mut cmd = home.command();
    cmd.arg("mcp")
        .env("IRIS_HOST", "127.0.0.1")
        .env("IRIS_WEB_PORT", "1");
    let mut child = cmd
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(std::fs::File::create(&err_path).unwrap())
        .spawn()
        .expect("spawn mcp");
    let mut stdin = child.stdin.take().unwrap();
    let stdout = child.stdout.take().unwrap();
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        for line in std::io::BufReader::new(stdout).lines() {
            let Ok(line) = line else { break };
            if tx.send(line).is_err() {
                break;
            }
        }
    });

    send(
        &mut stdin,
        r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-03-26","capabilities":{},"clientInfo":{"name":"t187","version":"0"}}}"#,
    );
    let init = response(&rx, 1, Duration::from_secs(10));
    send(
        &mut stdin,
        r#"{"jsonrpc":"2.0","method":"notifications/initialized","params":{}}"#,
    );
    send(
        &mut stdin,
        r#"{"jsonrpc":"2.0","id":2,"method":"tools/list","params":{}}"#,
    );
    let list = response(&rx, 2, Duration::from_secs(10));
    send(
        &mut stdin,
        r#"{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"skill_list","arguments":{}}}"#,
    );
    let call = response(&rx, 3, Duration::from_secs(10));

    iris_agentic_dev_core::testing::stop_server(&mut child);
    let stderr = std::fs::read_to_string(&err_path).unwrap_or_default();
    assert!(
        !stderr.contains(KEYSTORE_ABORT_MESSAGE),
        "mcp reached keystore initialisation\nstderr: {stderr}"
    );
    assert!(init["result"]["serverInfo"].is_object(), "{init}");
    assert!(
        list["result"]["tools"]
            .as_array()
            .is_some_and(|t| t.len() > 50),
        "{list}"
    );
    assert!(call["result"].is_object(), "{call}");
}

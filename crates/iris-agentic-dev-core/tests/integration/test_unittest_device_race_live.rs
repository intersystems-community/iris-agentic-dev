//! A `messages.log` line written while `%UnitTest.Manager` runs a test makes the manager drop
//! `$IO` to `$PRINCIPAL` (`tests/unit/test_unittest_job.rs` has the mechanism). Before 1.6.3,
//! `iris_test` over HTTP then answered NO_TESTS_FOUND for a test that had passed, and code run
//! through `iris_execute` could come back as an empty success. Here a second IRIS process writes
//! a unique console line 1.5 s into a 4 s test, every time.

use iris_agentic_dev_core::testing::{answer_text, live_env, require_iad_binary, McpSession};
use std::io::Write;
use std::process::{Command, Stdio};

const CLASS: &str = "IadRace.Slow";
const SRC: &str = "Class IadRace.Slow Extends %UnitTest.TestCase\n{\n\nMethod TestSlow()\n{\n    Hang 4\n    Do $$$AssertEquals(1, 1, \"slow test ran\")\n}\n\n}\n";

/// The container behind the live server. `IRIS_CONTAINER` when set; otherwise the container that
/// publishes `IRIS_WEB_PORT`, because tests earlier in this binary change the process
/// environment, and in the full CI order this test once found the variable gone.
fn container() -> String {
    if let Some(c) = std::env::var("IRIS_CONTAINER")
        .ok()
        .filter(|c| !c.trim().is_empty())
    {
        return c;
    }
    let port = std::env::var("IRIS_WEB_PORT").unwrap_or_else(|_| "52780".into());
    let out = Command::new("docker")
        .args([
            "ps",
            "--filter",
            &format!("publish={port}"),
            "--format",
            "{{.Names}}",
        ])
        .output()
        .expect("docker ps");
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .next()
        .map(str::to_string)
        .filter(|c| !c.is_empty())
        .unwrap_or_else(|| panic!("no IRIS_CONTAINER and no container publishes port {port}"))
}

fn docker_sh(script: &str, stdin: &str) {
    let mut child = Command::new("docker")
        .args(["exec", "-i", &container(), "sh", "-c", script])
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .spawn()
        .expect("docker exec");
    child
        .stdin
        .take()
        .unwrap()
        .write_all(stdin.as_bytes())
        .unwrap();
    assert!(
        child.wait().unwrap().success(),
        "docker exec sh -c {script}"
    );
}

/// Writes one unique line to messages.log from a separate IRIS process, `delay_ms` from now.
fn console_line_after(delay_ms: u64) -> std::thread::JoinHandle<()> {
    let c = container();
    std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(delay_ms));
        let mut child = Command::new("docker")
            .args(["exec", "-i", &c, "iris", "session", "IRIS", "-U", "%SYS"])
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .spawn()
            .expect("docker exec iris session");
        let code = "Do ##class(%SYS.System).WriteToConsoleLog(\"iad device race \"_$ZH)\nhalt\n";
        child
            .stdin
            .take()
            .unwrap()
            .write_all(code.as_bytes())
            .unwrap();
        child.wait().unwrap();
    })
}

/// The binary on the HTTP path: the manager then runs in a CSP process, whose principal is the
/// gateway socket. With IRIS_CONTAINER set, iris_test takes docker exec instead, which is immune,
/// so the child gets it empty (the parent's value would otherwise pass through) and runs in a
/// directory with no `.iris-agentic-dev.toml` to name a container either.
fn http_session() -> McpSession {
    let mut env: Vec<(String, String)> = live_env()
        .into_iter()
        .filter(|(k, _)| k != "IRIS_CONTAINER")
        .collect();
    env.push(("IRIS_CONTAINER".into(), String::new()));
    env.push(("IRIS_WRITE_TOOLS_ENABLED".into(), "1".into()));
    env.push(("IRIS_DESTRUCTIVE_TOOLS_ENABLED".into(), "1".into()));
    let dir = std::env::temp_dir().join("iad-device-race");
    std::fs::create_dir_all(&dir).unwrap();
    McpSession::start_in(Some(&dir), &env)
}

fn payload(answer: &serde_json::Value) -> serde_json::Value {
    let text = answer
        .pointer("/result/content/0/text")
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default();
    serde_json::from_str(text).unwrap_or_else(|_| serde_json::json!({ "text": text }))
}

fn setup(s: &mut McpSession) {
    let put = payload(&s.call(
        "iris_doc",
        &serde_json::json!({"mode": "put", "name": format!("{CLASS}.cls"), "content": SRC,
            "compile": true, "namespace": "USER"}),
    ));
    assert_eq!(put["success"], true, "put {CLASS}: {put}");
    // RunTest finds a package through a directory under ^UnitTestRoot that holds its source.
    docker_sh(
        "mkdir -p /tmp/httest/IadRace && cat > /tmp/httest/IadRace/Slow.cls",
        SRC,
    );
}

fn teardown(s: &mut McpSession) {
    s.call(
        "iris_doc",
        &serde_json::json!({"mode": "delete", "name": format!("{CLASS}.cls"), "namespace": "USER"}),
    );
    docker_sh("rm -rf /tmp/httest/IadRace", "");
}

#[test]
#[ignore = "requires live IRIS, docker exec into IRIS_CONTAINER, and the built binary"]
fn iris_test_reports_a_passing_test_when_the_console_log_grows_mid_run() {
    let Some(_bin) = require_iad_binary() else {
        return;
    };
    let mut s = http_session();
    setup(&mut s);
    let writer = console_line_after(1500);
    let got = payload(&s.call(
        "iris_test",
        &serde_json::json!({"pattern": "IadRace", "namespace": "USER"}),
    ));
    writer.join().unwrap();
    teardown(&mut s);
    assert_eq!(got["success"], true, "{got}");
    assert_eq!(got["passed"], 1, "{got}");
    assert_eq!(got["total"], 1, "{got}");
}

/// User code that runs the manager itself, with `$IO` saved and restored, still loses the
/// response when the line lands mid-test: the manager writes its own error text ahead of the
/// HTTP headers. That must come back as an error, not as success with no output.
#[test]
#[ignore = "requires live IRIS, docker exec into IRIS_CONTAINER, and the built binary"]
fn a_corrupted_execute_answer_is_an_error_not_an_empty_success() {
    let Some(_bin) = require_iad_binary() else {
        return;
    };
    let mut s = http_session();
    setup(&mut s);
    let writer = console_line_after(1500);
    let answer = s.call(
        "iris_execute",
        &serde_json::json!({"code": format!("Set io=$IO Do ##class(%UnitTest.Manager).DebugRunTestCase(\"\",\"{CLASS}\") Use io Write \"after-run\""),
            "namespace": "USER"}),
    );
    writer.join().unwrap();
    teardown(&mut s);
    let text = answer_text(&answer);
    let is_error = answer.pointer("/result/isError") == Some(&serde_json::Value::Bool(true));
    assert!(
        text.contains("after-run") || (is_error && text.contains("not JSON")),
        "neither the output nor a legible error: {text}"
    );
}

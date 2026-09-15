//! `iris-agentic-dev tool iris_ws_open` used to hand back a token nothing could use.
//!
//! A WebSocket session is a live socket held in a `HashMap` owned by one `IrisTools` instance. The
//! one-shot `tool` subcommand builds that instance, prints, and exits, so the next invocation gets
//! an empty pool — the token from the first call can never resolve. What a reporter saw was a
//! successful open followed by `SESSION_STALE: Session token references an unknown server or
//! expired session`, which describes a timeout that never happened.
//!
//! So the three session tools are refused under `tool` and point at `batch`, which runs the whole
//! open/exec/close sequence in one process. The refusal comes before the connection resolves, so
//! these tests need no IRIS.
//!
//! Run with:
//!   cargo build && IAD_BINARY=./target/debug/iris-agentic-dev \
//!   cargo test --features testing --test bin_integration ws_cli -- --include-ignored

use iris_agentic_dev_core::testing::{clean_command, iad_binary_path};

/// The phrase the guard is recognised by. `batch` is the whole point of the message: without it the
/// caller knows the call failed and still has nowhere to go.
const POINTS_AT_BATCH: &str = "iris-agentic-dev batch";

struct Run {
    code: Option<i32>,
    stdout: String,
    stderr: String,
}

fn run(args: &[&str]) -> Option<Run> {
    let bin = iad_binary_path();
    if !bin.exists() {
        eprintln!("skipping: {} not built", bin.display());
        return None;
    }
    let out = clean_command(&bin)
        .args(args)
        .output()
        .expect("spawning the binary should succeed");
    Some(Run {
        code: out.status.code(),
        stdout: String::from_utf8_lossy(&out.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&out.stderr).into_owned(),
    })
}

/// The reported bug: the open succeeded and its token was useless. Refuse it instead, and mint no
/// token — a token in stdout is a token someone will copy into the next command.
#[test]
#[ignore = "spawns the built binary; needs IAD_BINARY or a prior cargo build"]
fn one_shot_ws_open_is_refused_and_mints_no_token() {
    let Some(r) = run(&["tool", "iris_ws_open"]) else {
        return;
    };
    assert_ne!(r.code, Some(0), "must not succeed: {}", r.stderr);
    assert!(
        r.stderr.contains(POINTS_AT_BATCH),
        "the refusal must name the command that works: {}",
        r.stderr
    );
    assert!(
        !r.stdout.contains("ws:"),
        "a refused open must not print a session token: {}",
        r.stdout
    );
}

/// `iris_ws_exec` and `iris_ws_close` under `tool` can only ever be handed a token from another
/// process, so they get the same refusal rather than `SESSION_STALE` after a connection attempt.
#[test]
#[ignore = "spawns the built binary; needs IAD_BINARY or a prior cargo build"]
fn one_shot_ws_exec_and_close_are_refused_too() {
    for tool in ["iris_ws_exec", "iris_ws_close"] {
        let Some(r) = run(&[
            "tool",
            tool,
            "--args",
            r#"{"session":"ws:dev:USER:11111111-2222-3333-4444-555555555555","code":"Write 1"}"#,
        ]) else {
            return;
        };
        assert_ne!(r.code, Some(0), "{tool} must not succeed: {}", r.stderr);
        assert!(
            r.stderr.contains(POINTS_AT_BATCH),
            "{tool} must point at batch: {}",
            r.stderr
        );
        assert!(
            !r.stderr.contains("SESSION_STALE"),
            "{tool} must not report a stale session — no session was ever opened here: {}",
            r.stderr
        );
    }
}

/// Harnesses read `--envelope`, not stderr. The refusal has to reach them as `ok: false` with the
/// guidance in `error`, not as an empty envelope or a zero exit.
#[test]
#[ignore = "spawns the built binary; needs IAD_BINARY or a prior cargo build"]
fn the_refusal_reaches_envelope_callers() {
    let Some(r) = run(&["tool", "iris_ws_open", "--envelope"]) else {
        return;
    };
    assert_ne!(r.code, Some(0), "must not succeed: {}", r.stderr);
    let body: serde_json::Value = serde_json::from_str(r.stdout.trim())
        .unwrap_or_else(|e| panic!("envelope is not JSON ({e}): {}", r.stdout));
    assert_eq!(body["ok"], serde_json::json!(false), "envelope: {body}");
    assert_eq!(body["tool"], "iris_ws_open", "envelope: {body}");
    let error = body["error"].as_str().unwrap_or_default();
    assert!(
        error.contains(POINTS_AT_BATCH),
        "the envelope's error must point at batch: {body}"
    );
}

/// The guard lives in the `tool` subcommand, not in the shared dispatch `batch` also calls. Putting
/// it in dispatch would refuse the session tools everywhere, including in the one place they work.
///
/// With no connection this exits on the connection, and on a machine that has one it runs the open.
/// Either outcome is fine; what must never appear is the guard's own text.
#[test]
#[ignore = "spawns the built binary; needs IAD_BINARY or a prior cargo build"]
fn batch_is_not_touched_by_the_guard() {
    let script = tempfile::NamedTempFile::new().expect("tempfile");
    std::fs::write(
        script.path(),
        r#"[{"tool": "iris_ws_open", "args": {}}, {"tool": "iris_ws_close", "args": {"session": "{{0.session}}"}}]"#,
    )
    .expect("write script");
    let path = script.path().to_string_lossy().to_string();
    let Some(r) = run(&["batch", "--file", &path]) else {
        return;
    };
    assert!(
        !r.stderr.contains(POINTS_AT_BATCH),
        "batch must not be told to use batch: {}",
        r.stderr
    );
}

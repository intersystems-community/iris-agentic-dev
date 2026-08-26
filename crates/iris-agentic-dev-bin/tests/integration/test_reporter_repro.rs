//! The three reproductions from the 1.2.6 write-gate report, scripted (085 T040, SC-008).
//!
//! The report was a manual walk-through against the released macOS arm64 binaries: start the
//! server, edit the toml, watch `check_config` disagree with what a write actually does. A
//! walk-through cannot fail in CI, so it is here as a test the reporter can run themselves:
//!
//! ```text
//! cargo test --test test_reporter_repro -- --include-ignored --test-threads=1
//! ```
//!
//! Requires the live dev container (`docker ps --filter name=iris-dev-iris`, web port 52780).
//! Overridable: `IRIS_HOST` (localhost), `IRIS_WEB_PORT` (52780), `IRIS_NAMESPACE` (USER),
//! `IRIS_USERNAME` (_SYSTEM), `IRIS_PASSWORD` (SYS). `--test-threads=1` is required — these
//! tests spawn servers that watch config files and share the one container.
//!
//! Reproduction 2 is the one that needs IRIS: a refusal is decided before dispatch and needs no
//! connection at all, but "the global is not there afterward" only means something if a permitted
//! write to the same global *would* have landed. So each case that asserts absence also proves the
//! write path works, in a second session with the gate on.
//!
//! Overlap with `test_mcp_binary_config` is deliberate. `config_rewritten_twice_in_one_process...`
//! (T025) is the regression test for defect 1 and is written for a contributor reading the suite;
//! this file is written for the reporter, in their terms, ending in one command.

use std::io::{BufRead, Write};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};

// ── environment ──────────────────────────────────────────────────────────────

fn env_or(key: &str, default: &str) -> String {
    std::env::var(key).unwrap_or_else(|_| default.to_string())
}

/// A config declaring the gate, plus enough connection detail to reach the dev container.
fn toml_with(gates: &str) -> String {
    format!(
        "host = \"{}\"\nweb_port = {}\nnamespace = \"{}\"\nusername = \"{}\"\npassword = \"{}\"\n{gates}",
        env_or("IRIS_HOST", "localhost"),
        env_or("IRIS_WEB_PORT", "52780"),
        env_or("IRIS_NAMESPACE", "USER"),
        env_or("IRIS_USERNAME", "_SYSTEM"),
        env_or("IRIS_PASSWORD", "SYS"),
    )
}

/// Write the config and push its mtime forward.
///
/// `ConfigWatcher::has_changed` compares `new > old`, so two writes inside one filesystem timestamp
/// tick look like no change and the test would be measuring clock resolution instead of the reload.
fn write_config(path: &std::path::Path, body: &str, tick: u64) {
    std::fs::write(path, body).expect("write config");
    let f = std::fs::OpenOptions::new()
        .write(true)
        .open(path)
        .expect("reopen config");
    f.set_modified(std::time::SystemTime::now() + std::time::Duration::from_secs(10 * (tick + 1)))
        .expect("bump mtime");
}

// ── one scripted stdio session ───────────────────────────────────────────────

struct Session {
    child: Child,
    stdin: ChildStdin,
    reader: std::io::BufReader<ChildStdout>,
    next_id: u64,
}

impl Drop for Session {
    fn drop(&mut self) {
        self.child.kill().ok();
        self.child.wait().ok();
    }
}

impl Session {
    /// Spawn with workspace discovery, not `--config`: the watcher under test looks at
    /// `workspace_root()/.iris-agentic-dev.toml`, and `OBJECTSCRIPT_WORKSPACE` pins that path
    /// without depending on the test process's cwd.
    fn start(workspace: &std::path::Path) -> Self {
        let bin = env!("CARGO_BIN_EXE_iris-agentic-dev");
        let mut cmd = Command::new(bin);
        cmd.arg("mcp")
            .current_dir(workspace)
            .env("OBJECTSCRIPT_WORKSPACE", workspace)
            // An operator env var legitimately outranks the file (FR-003). One left over in the
            // reporter's shell would quietly become the thing under test.
            .env_remove("IRIS_WRITE_TOOLS_ENABLED")
            .env_remove("IRIS_DESTRUCTIVE_TOOLS_ENABLED")
            .env_remove("IRIS_ALLOW_PROD")
            .env_remove("IRIS_CONTAINER")
            .env_remove("IRIS_TOOLSET")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        let mut child = cmd.spawn().expect("failed to spawn iris-agentic-dev");
        let stdin = child.stdin.take().unwrap();
        let reader = std::io::BufReader::new(child.stdout.take().unwrap());
        let mut s = Session {
            child,
            stdin,
            reader,
            next_id: 1,
        };
        s.request(
            "initialize",
            serde_json::json!({
                "protocolVersion": "2025-03-26",
                "capabilities": {},
                "clientInfo": {"name": "reporter-repro", "version": "0.0.1"},
            }),
        );
        s.stdin
            .write_all(
                b"{\"jsonrpc\":\"2.0\",\"method\":\"notifications/initialized\",\"params\":{}}\n",
            )
            .expect("initialized notification");
        s.stdin.flush().ok();
        s
    }

    fn request(&mut self, method: &str, params: serde_json::Value) -> serde_json::Value {
        let id = self.next_id;
        self.next_id += 1;
        let line = serde_json::json!({
            "jsonrpc": "2.0", "id": id, "method": method, "params": params,
        })
        .to_string();
        self.stdin
            .write_all(line.as_bytes())
            .expect("write request");
        self.stdin.write_all(b"\n").expect("write newline");
        self.stdin.flush().ok();
        loop {
            let mut buf = String::new();
            let n = self.reader.read_line(&mut buf).expect("read response");
            assert!(n > 0, "server closed stdout before answering {method}");
            let Ok(v) = serde_json::from_str::<serde_json::Value>(buf.trim()) else {
                continue;
            };
            if v.get("id").and_then(|i| i.as_u64()) == Some(id) {
                return v;
            }
        }
    }

    /// Call a tool and return its decoded payload — structured content when present, otherwise the
    /// text block. A refusal arrives as structured content, so both live here.
    fn call(&mut self, tool: &str, args: serde_json::Value) -> serde_json::Value {
        let v = self.request(
            "tools/call",
            serde_json::json!({"name": tool, "arguments": args}),
        );
        if let Some(sc) = v.pointer("/result/structuredContent") {
            if !sc.is_null() {
                return sc.clone();
            }
        }
        v.pointer("/result/content/0/text")
            .and_then(|t| t.as_str())
            .and_then(|t| serde_json::from_str(t).ok())
            .unwrap_or(v)
    }

    fn check_config(&mut self) -> serde_json::Value {
        self.call("check_config", serde_json::json!({}))
    }
}

/// `true` when the dev container answers. Reproduction 2 asserts a write *lands*, which cannot be
/// faked, so it skips loudly rather than passing vacuously.
fn iris_reachable() -> bool {
    let out = Command::new(env!("CARGO_BIN_EXE_iris-agentic-dev"))
        .args(["exec", "write 1,!"])
        .env("IRIS_HOST", env_or("IRIS_HOST", "localhost"))
        .env("IRIS_WEB_PORT", env_or("IRIS_WEB_PORT", "52780"))
        .env("IRIS_NAMESPACE", env_or("IRIS_NAMESPACE", "USER"))
        .env("IRIS_USERNAME", env_or("IRIS_USERNAME", "_SYSTEM"))
        .env("IRIS_PASSWORD", env_or("IRIS_PASSWORD", "SYS"))
        .output();
    matches!(out, Ok(o) if o.status.success())
}

/// The probe global. Prefixed so a stray one is identifiable and safe to kill.
const PROBE: &str = "IADGate085Repro";

fn error_code(v: &serde_json::Value) -> Option<&str> {
    v.get("error_code").and_then(|c| c.as_str())
}

// ── Reproduction 1 ───────────────────────────────────────────────────────────

/// "I set `write_tools_enabled = false` in the toml, `check_config` still says `true`."
///
/// The reporter watched `config_loaded_at` advance while the value never moved, in either
/// direction. Cause: the config value was exported to `IRIS_WRITE_TOOLS_ENABLED` and only when that
/// variable was unset, so the first load of the process won permanently, and `check_config` read
/// the variable rather than the file.
///
/// One process throughout — restarting is what hid this for two releases.
#[test]
#[ignore]
fn repro_1_reported_gate_goes_stale_after_a_config_edit() {
    let dir = tempfile::tempdir().unwrap();
    let cfg = dir.path().join(".iris-agentic-dev.toml");
    write_config(&cfg, &toml_with("write_tools_enabled = true\n"), 0);

    let mut s = Session::start(dir.path());
    let first = s.check_config();
    assert_eq!(
        first["write_tools_enabled"],
        serde_json::json!(true),
        "step 1: the file says true; check_config says: {first}"
    );

    // The edit the reporter made.
    write_config(&cfg, &toml_with("write_tools_enabled = false\n"), 1);
    let second = s.check_config();
    assert_eq!(
        second["write_tools_enabled"],
        serde_json::json!(false),
        "step 2 — the reported defect. The toml now says false and the same process still reports \
         {}. config_loaded_at moved from {:?} to {:?}, so the file was re-read and the gate was \
         not: {second}",
        second["write_tools_enabled"],
        first["config_loaded_at"],
        second["config_loaded_at"]
    );
    assert_eq!(
        second["write_tools_source"],
        serde_json::json!("config_file"),
        "step 2: the file is what decided this, and check_config has to say so: {second}"
    );

    // And back, because a gate stuck on `false` would pass step 2 for the wrong reason.
    write_config(&cfg, &toml_with("write_tools_enabled = true\n"), 2);
    let third = s.check_config();
    assert_eq!(
        third["write_tools_enabled"],
        serde_json::json!(true),
        "step 3: the toml says true again — a gate that only ever narrows is still stuck: {third}"
    );
}

// ── Reproduction 2 ───────────────────────────────────────────────────────────

/// "With `write_tools_enabled = false` and `iris_doc(put)` provably blocked in the same session,
/// `iris_global` set and `iris_ws_exec` still wrote to IRIS."
///
/// The assertion is the reporter's: not the returned error code, but whether the global exists
/// afterward. `iris_ws_exec` was the severe one — `iris_ws_open` then `iris_ws_exec` runs arbitrary
/// ObjectScript, so it bypassed every per-tool guard at once.
#[test]
#[ignore]
fn repro_2_a_write_lands_with_the_gate_declared_off() {
    if !iris_reachable() {
        eprintln!(
            "SKIP repro_2: no IRIS at {}:{} — this case asserts that a write did NOT land, which \
             only means something when a permitted write would have. Start iris-dev-iris.",
            env_or("IRIS_HOST", "localhost"),
            env_or("IRIS_WEB_PORT", "52780")
        );
        return;
    }

    let dir = tempfile::tempdir().unwrap();
    let cfg = dir.path().join(".iris-agentic-dev.toml");
    write_config(&cfg, &toml_with("write_tools_enabled = false\n"), 0);

    // Session A: the gate is off.
    let mut a = Session::start(dir.path());
    let cc = a.check_config();
    assert_eq!(
        cc["write_tools_enabled"],
        serde_json::json!(false),
        "the gate has to be provably off before any of this means anything: {cc}"
    );
    assert_eq!(
        cc["connected"],
        serde_json::json!(true),
        "and the server has to be connected, or nothing could have been written either way: {cc}"
    );

    // The reporter's control: this one was refused on 1.2.6 too.
    let doc = a.call(
        "iris_doc",
        serde_json::json!({
            "mode": "put",
            "doc_name": "IADGate085Repro.cls",
            "content": "Class IADGate085Repro { }",
        }),
    );
    assert_eq!(
        error_code(&doc),
        Some("WRITE_TOOLS_DISABLED"),
        "iris_doc(put) is the control — it was gated even on 1.2.6: {doc}"
    );

    // Attempt 1: iris_global set.
    let g = a.call(
        "iris_global",
        serde_json::json!({
            "action": "set", "global_name": PROBE, "subscripts": ["global"], "value": "1",
        }),
    );
    let g_code = error_code(&g).map(str::to_string);

    // Attempt 2: a terminal session plus arbitrary ObjectScript.
    let open = a.call("iris_ws_open", serde_json::json!({}));
    let ws_session = open
        .get("session")
        .and_then(|t| t.as_str())
        .map(String::from);
    let exec = ws_session.as_ref().map(|token| {
        a.call(
            "iris_ws_exec",
            serde_json::json!({
                "session": token,
                "code": format!("set ^{PROBE}(\"ws\") = 1"),
            }),
        )
    });
    if let Some(token) = ws_session.as_ref() {
        a.call("iris_ws_close", serde_json::json!({"session": token}));
    }
    drop(a);

    // Session B: the gate is on, so the probe can be read and cleaned up. This is where the
    // reporter's measurement happens — existence, not error codes.
    write_config(&cfg, &toml_with("write_tools_enabled = true\n"), 1);
    let mut b = Session::start(dir.path());

    let mut landed: Vec<String> = Vec::new();
    for (which, sub) in [("iris_global set", "global"), ("iris_ws_exec", "ws")] {
        let got = b.call(
            "iris_global",
            serde_json::json!({"action": "get", "global_name": PROBE, "subscripts": [sub]}),
        );
        if got["defined"] == serde_json::json!(true) {
            landed.push(format!(
                "{which} wrote ^{PROBE}(\"{sub}\") = {} with the gate declared off",
                got["value"]
            ));
        }
    }

    // Clean up before asserting, so a failure does not leave the probe behind for the next run.
    b.call(
        "iris_global",
        serde_json::json!({"action": "kill", "global_name": PROBE}),
    );

    assert!(
        landed.is_empty(),
        "{} write(s) landed in IRIS while write_tools_enabled = false:\n  {}\n\
         Returned codes were iris_global {:?} and iris_ws_exec {:?} — note that a refusal code is \
         not the assertion. The global's absence is.",
        landed.len(),
        landed.join("\n  "),
        g_code,
        exec.as_ref().and_then(error_code),
    );

    // Both attempts must also *say* why, otherwise a caller cannot tell a gate from an outage.
    assert_eq!(
        g_code.as_deref(),
        Some("WRITE_TOOLS_DISABLED"),
        "iris_global(set) has to name the gate"
    );
    let exec = exec.expect("iris_ws_open returned no session token");
    assert_eq!(
        error_code(&exec),
        Some("WRITE_TOOLS_DISABLED"),
        "iris_ws_exec has to name the gate: {exec}"
    );

    // The writes did not land because they were refused — not because the write path is broken.
    // Without this, an unreachable global would make the whole test pass for the wrong reason.
    let set_on = b.call(
        "iris_global",
        serde_json::json!({
            "action": "set", "global_name": PROBE, "subscripts": ["control"], "value": "1",
        }),
    );
    assert_eq!(
        set_on["success"],
        serde_json::json!(true),
        "with the gate on, the same write must succeed — otherwise the absences above prove \
         nothing: {set_on}"
    );
    let control = b.call(
        "iris_global",
        serde_json::json!({"action": "get", "global_name": PROBE, "subscripts": ["control"]}),
    );
    assert_eq!(
        control["defined"],
        serde_json::json!(true),
        "the control write did not land either — this is an IRIS or connection problem, not a \
         gate result: {control}"
    );
    b.call(
        "iris_global",
        serde_json::json!({"action": "kill", "global_name": PROBE}),
    );
}

// ── Reproduction 3 ───────────────────────────────────────────────────────────

/// "`destructive_tools_enabled = true` with `write_tools_enabled = false` started the server with
/// writes ENABLED — `check_config` reported `write_tools_enabled: true`, `connected: true`,
/// `connection_source: config_file`."
///
/// The loader logged `DESTRUCTIVE_REQUIRES_WRITES` and returned `None` above its own gate export,
/// which dropped startup into the namespace heuristic — `USER` infers `true`. So the operator got
/// the warning they asked for and the inverse of the configuration they asked for.
///
/// The fixed behavior is a refusal to start, which docs/tools.md and specs/073-destructive-gate
/// have promised since 1.2.1: exit 2, the code on stderr, and no session.
#[test]
#[ignore]
fn repro_3_contradictory_config_starts_with_writes_enabled() {
    let dir = tempfile::tempdir().unwrap();
    write_config(
        &dir.path().join(".iris-agentic-dev.toml"),
        &toml_with("write_tools_enabled = false\ndestructive_tools_enabled = true\n"),
        0,
    );

    let bin = env!("CARGO_BIN_EXE_iris-agentic-dev");
    let mut child = Command::new(bin)
        .arg("mcp")
        .current_dir(dir.path())
        .env("OBJECTSCRIPT_WORKSPACE", dir.path())
        .env_remove("IRIS_WRITE_TOOLS_ENABLED")
        .env_remove("IRIS_DESTRUCTIVE_TOOLS_ENABLED")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("failed to spawn iris-agentic-dev");

    // Ask for exactly what the reporter asked for. Writing to a process that has already exited
    // fails with EPIPE, which is the expected outcome, so send errors are ignored.
    let mut stdin = child.stdin.take().unwrap();
    let _ = stdin.write_all(b"{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"initialize\",\"params\":{\"protocolVersion\":\"2025-03-26\",\"capabilities\":{},\"clientInfo\":{\"name\":\"reporter-repro\",\"version\":\"0.0.1\"}}}\n");
    let _ = stdin.write_all(
        b"{\"jsonrpc\":\"2.0\",\"method\":\"notifications/initialized\",\"params\":{}}\n",
    );
    let _ = stdin.write_all(b"{\"jsonrpc\":\"2.0\",\"id\":2,\"method\":\"tools/call\",\"params\":{\"name\":\"check_config\",\"arguments\":{}}}\n");
    drop(stdin);

    let out = child.wait_with_output().expect("wait failed");
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);

    assert_eq!(
        out.status.code(),
        Some(2),
        "expected exit 2, got {:?}. On 1.2.6 this exited 0 and served.\nstdout: {stdout}\n\
         stderr: {stderr}",
        out.status.code()
    );
    assert!(
        stderr.contains("DESTRUCTIVE_REQUIRES_WRITES"),
        "the refusal must name the code the docs promise; stderr: {stderr}"
    );
    assert!(
        !stdout.contains("write_tools_enabled"),
        "check_config answered at all, which means the server came up under a configuration it \
         refused. On 1.2.6 the answer was write_tools_enabled: true — the inverse of the file.\n\
         stdout: {stdout}"
    );
}

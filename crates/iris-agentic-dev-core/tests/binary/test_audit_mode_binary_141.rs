//! Spec 141 FR-028 (T-R4b-U13, binary layer): `check_config` reports whether the local audit log
//! keeps payload text, and only the operator can turn that on.
//!
//! Each test spawns `iris-agentic-dev mcp` with a temporary `HOME` and its own project directory.
//! No IRIS is involved.
//!
//! Run: `IAD_BINARY=$PWD/target/debug/iris-agentic-dev cargo test --features testing --test binary
//! test_audit_mode_binary_141 -- --ignored --test-threads=1`

use iris_agentic_dev_core::testing::{clean_mcp_command, iad_binary_path, stop_server};
use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Write};
use std::path::Path;
use std::process::{Child, Stdio};
use tempfile::TempDir;

struct Session {
    child: Child,
    reader: BufReader<std::process::ChildStdout>,
}

impl Session {
    fn start(home: &Path, cwd: &Path, env: &[(&str, &str)]) -> Session {
        let bin = iad_binary_path();
        assert!(
            bin.exists(),
            "no binary at {}; cargo build first",
            bin.display()
        );
        let mut cmd = clean_mcp_command(&bin);
        cmd.env("HOME", home)
            .env("USERPROFILE", home)
            .current_dir(cwd)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        for (k, v) in env {
            cmd.env(k, v);
        }
        let mut child = cmd.spawn().expect("spawn iris-agentic-dev mcp");
        let reader = BufReader::new(child.stdout.take().unwrap());
        let mut s = Session { child, reader };
        let init = json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{
            "protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"t141","version":"0"}}});
        writeln!(s.child.stdin.as_mut().unwrap(), "{init}").unwrap();
        s.read(1);
        writeln!(
            s.child.stdin.as_mut().unwrap(),
            "{}",
            json!({"jsonrpc":"2.0","method":"notifications/initialized","params":{}})
        )
        .unwrap();
        s
    }

    fn read(&mut self, id: u64) -> Value {
        let mut line = String::new();
        loop {
            line.clear();
            let n = self.reader.read_line(&mut line).expect("read stdout");
            assert!(n != 0, "the server closed stdout before answering {id}");
            if let Ok(v) = serde_json::from_str::<Value>(&line) {
                if v.get("id").and_then(Value::as_u64) == Some(id) {
                    return v;
                }
            }
        }
    }

    fn check_config(&mut self) -> Value {
        let req = json!({"jsonrpc":"2.0","id":2,"method":"tools/call",
            "params":{"name":"check_config","arguments":{}}});
        writeln!(self.child.stdin.as_mut().unwrap(), "{req}").unwrap();
        let answer = self.read(2);
        let text = answer["result"]["content"][0]["text"]
            .as_str()
            .unwrap_or_else(|| panic!("no text content: {answer}"));
        serde_json::from_str(text).unwrap_or_else(|e| panic!("not JSON ({e}): {text}"))
    }
}

fn report(home_toml: Option<&str>, workspace_toml: Option<&str>, env: &[(&str, &str)]) -> Value {
    let home = TempDir::new().unwrap();
    let project = TempDir::new().unwrap();
    if let Some(t) = home_toml {
        std::fs::write(home.path().join(".iris-agentic-dev.toml"), t).unwrap();
    }
    if let Some(t) = workspace_toml {
        std::fs::write(project.path().join(".iris-agentic-dev.toml"), t).unwrap();
    }
    let mut s = Session::start(home.path(), project.path(), env);
    let out = s.check_config();
    stop_server(&mut s.child);
    out
}

#[test]
#[ignore]
fn payloads_are_sizes_by_default() {
    let out = report(None, None, &[]);
    assert_eq!(out["audit"]["payloads"], "sizes", "{}", out["audit"]);
    assert!(
        out["audit"]["path"]
            .as_str()
            .is_some_and(|p| p.ends_with("audit.jsonl")),
        "{}",
        out["audit"]
    );
}

#[test]
#[ignore]
fn the_operator_file_turns_full_payloads_on() {
    let out = report(Some("audit_payloads = true\n"), None, &[]);
    assert_eq!(out["audit"]["payloads"], "full", "{}", out["audit"]);
}

#[test]
#[ignore]
fn the_launch_environment_turns_full_payloads_on() {
    let out = report(None, None, &[("IAD_AUDIT_PAYLOADS", "1")]);
    assert_eq!(out["audit"]["payloads"], "full", "{}", out["audit"]);
}

#[test]
#[ignore]
fn a_workspace_file_cannot_turn_it_on_and_the_server_says_it_set_it_aside() {
    let out = report(None, Some("audit_payloads = true\n"), &[]);
    assert_eq!(out["audit"]["payloads"], "sizes", "{}", out["audit"]);
    let aside = out["set_aside"].as_array().expect("set_aside array");
    assert!(
        aside
            .iter()
            .any(|i| i["id"] == "workspace:audit_payloads" && i["kind"] == "operator_only"),
        "audit_payloads from a workspace file must be set aside: {aside:?}"
    );
}

//! Layer 2 for spec 141 FR-027: over the wire, a `skills/skills` in the working directory is not
//! served without the operator's opt-in, and with it the copy is labelled `override`.
//!
//! The binary is copied to a directory of its own first. Beside a checkout it finds the
//! checkout's `skills/skills` through the executable-adjacent candidate and stops there, which
//! would hide the working directory from the test whatever the opt-in says.

use iris_agentic_dev_core::testing::{clean_mcp_command, iad_binary_path, stop_server};
use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Stdio};

const SENTINEL: &str = "SENTINEL-141-weaker-guardrails";

struct Session {
    child: Child,
    reader: BufReader<std::process::ChildStdout>,
    next_id: u64,
}

fn relocated_binary(dir: &Path) -> PathBuf {
    let bin = iad_binary_path();
    assert!(
        bin.exists(),
        "no binary at {}; cargo build first",
        bin.display()
    );
    let to = dir.join("bin").join(if cfg!(windows) {
        "iris-agentic-dev.exe"
    } else {
        "iris-agentic-dev"
    });
    std::fs::create_dir_all(to.parent().unwrap()).unwrap();
    std::fs::copy(&bin, &to).expect("copy the binary");
    to
}

impl Session {
    fn start(bin: &Path, home: &Path, cwd: &Path, env: &[(&str, &str)]) -> Session {
        let mut cmd = clean_mcp_command(bin);
        cmd.env("HOME", home)
            .env("USERPROFILE", home)
            .current_dir(cwd)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        for (k, v) in env {
            cmd.env(k, v);
        }
        // A freshly copied executable can refuse exec for a moment (ETXTBSY).
        let mut child = None;
        for _ in 0..10 {
            match cmd.spawn() {
                Ok(c) => {
                    child = Some(c);
                    break;
                }
                Err(_) => std::thread::sleep(std::time::Duration::from_millis(100)),
            }
        }
        let mut child = child.expect("spawn iris-agentic-dev mcp");
        let reader = BufReader::new(child.stdout.take().unwrap());
        let mut s = Session {
            child,
            reader,
            next_id: 1,
        };
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
            assert!(
                self.reader.read_line(&mut line).unwrap() > 0,
                "server exited"
            );
            if let Ok(v) = serde_json::from_str::<Value>(&line) {
                if v["id"].as_u64() == Some(id) {
                    return v;
                }
            }
        }
    }

    /// The JSON a tool returned in its text content.
    fn call(&mut self, tool: &str, args: Value) -> Value {
        self.next_id += 1;
        let id = self.next_id;
        let req = json!({"jsonrpc":"2.0","id":id,"method":"tools/call",
            "params":{"name":tool,"arguments":args}});
        writeln!(self.child.stdin.as_mut().unwrap(), "{req}").unwrap();
        let answer = self.read(id);
        let text = answer["result"]["content"][0]["text"]
            .as_str()
            .unwrap_or_else(|| panic!("no text content in {answer}"));
        serde_json::from_str(text).unwrap_or_else(|_| json!({ "text": text }))
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        stop_server(&mut self.child);
    }
}

/// A project directory whose `skills/skills/objectscript-guardrails/SKILL.md` carries the sentinel.
fn project_with_a_planted_guardrail() -> tempfile::TempDir {
    let ws = tempfile::tempdir().unwrap();
    let dir = ws.path().join("skills/skills/objectscript-guardrails");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("SKILL.md"),
        format!(
            "---\nname: objectscript-guardrails\ndescription: planted\ntier: internal\n---\n{SENTINEL}\n"
        ),
    )
    .unwrap();
    ws
}

#[test]
#[ignore = "spawns the built binary; run with --include-ignored"]
fn cwd_guardrail_copy_is_not_served() {
    let ws = project_with_a_planted_guardrail();
    let scratch = tempfile::tempdir().unwrap();
    let bin = relocated_binary(scratch.path());
    let mut s = Session::start(&bin, scratch.path(), ws.path(), &[]);

    let described = s.call("skill_describe", json!({"name": "objectscript-guardrails"}));
    let body = described["skill"]["body"].as_str().unwrap_or_default();
    assert!(
        !body.contains(SENTINEL),
        "the working directory's guardrail body was served: {described}"
    );
    assert_eq!(described["skill"]["source"], "bundled", "{described}");

    let cfg = s.call("check_config", json!({}));
    let ignored = cfg["skills"]["ignored_dir"].as_str().unwrap_or_default();
    assert!(
        ignored.contains("skills/skills"),
        "check_config.skills.ignored_dir must name the directory it did not read: {cfg}"
    );
    assert_eq!(cfg["skills"]["local_skills"], false, "{cfg}");
}

#[test]
#[ignore = "spawns the built binary; run with --include-ignored"]
fn opt_in_serves_the_cwd_copy_as_override() {
    let ws = project_with_a_planted_guardrail();
    let scratch = tempfile::tempdir().unwrap();
    let bin = relocated_binary(scratch.path());
    let mut s = Session::start(
        &bin,
        scratch.path(),
        ws.path(),
        &[("IAD_LOCAL_SKILLS", "1")],
    );

    let described = s.call("skill_describe", json!({"name": "objectscript-guardrails"}));
    let body = described["skill"]["body"].as_str().unwrap_or_default();
    assert!(
        body.contains(SENTINEL),
        "the opt-in did not serve it: {described}"
    );
    assert_eq!(described["skill"]["source"], "override", "{described}");
    assert!(
        described["skill"]["path"]
            .as_str()
            .unwrap_or_default()
            .contains("skills/skills/objectscript-guardrails"),
        "{described}"
    );
    // The copy says `internal`; the embedded tier (core) still wins, so it stays listed.
    assert_eq!(described["skill"]["tier"], "core", "{described}");
    let listed = s.call("skill_list", json!({}));
    let names: Vec<&str> = listed["skills"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|k| k["name"].as_str())
        .collect();
    assert!(names.contains(&"objectscript-guardrails"), "{names:?}");

    let cfg = s.call("check_config", json!({}));
    assert_eq!(cfg["skills"]["local_skills"], true, "{cfg}");
    assert_eq!(cfg["skills"]["override_kind"], "workspace", "{cfg}");
    assert!(cfg["skills"]["ignored_dir"].is_null(), "{cfg}");
}

#[test]
#[ignore = "spawns the built binary; run with --include-ignored"]
fn a_workspace_file_cannot_turn_the_opt_in_on() {
    let ws = project_with_a_planted_guardrail();
    std::fs::write(
        ws.path().join(".iris-agentic-dev.toml"),
        "local_skills = true\n",
    )
    .unwrap();
    let scratch = tempfile::tempdir().unwrap();
    let bin = relocated_binary(scratch.path());
    let mut s = Session::start(&bin, scratch.path(), ws.path(), &[]);

    let described = s.call("skill_describe", json!({"name": "objectscript-guardrails"}));
    assert!(
        !described["skill"]["body"]
            .as_str()
            .unwrap_or_default()
            .contains(SENTINEL),
        "{described}"
    );
    let cfg = s.call("check_config", json!({}));
    let set_aside = cfg["set_aside"].to_string();
    assert!(set_aside.contains("local_skills"), "{cfg}");
}

#[test]
#[ignore = "spawns the built binary; run with --include-ignored"]
fn the_operator_file_can_turn_the_opt_in_on() {
    let ws = project_with_a_planted_guardrail();
    let home = tempfile::tempdir().unwrap();
    std::fs::write(
        home.path().join(".iris-agentic-dev.toml"),
        "local_skills = true\n",
    )
    .unwrap();
    let scratch = tempfile::tempdir().unwrap();
    let bin = relocated_binary(scratch.path());
    let mut s = Session::start(&bin, home.path(), ws.path(), &[]);

    let described = s.call("skill_describe", json!({"name": "objectscript-guardrails"}));
    assert!(
        described["skill"]["body"]
            .as_str()
            .unwrap_or_default()
            .contains(SENTINEL),
        "{described}"
    );
}

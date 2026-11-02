//! Layer 2 for S17: strict read-only with `irisAudit = true` is refused by the binary.
//!
//! Writing each audit record compiles a class on the server, and strict means iad runs nothing
//! there. The MCP server exits 2 on the combination before the handshake. The `tool` command used
//! to build its session on it and run the call, audit record and all; it now refuses every tool
//! that talks to IRIS, and `check_config` reports the conflict. The connection is `127.0.0.1:1`,
//! where nothing listens, so a call that got past the refusal fails on the network.

#![cfg(unix)]

use std::io::Write;
use std::path::PathBuf;
use std::process::{Output, Stdio};

fn bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_iris-agentic-dev"))
}

const STRICT: &str = "read_only = \"strict\"\n";
const AUDIT: &str = "[policy.default]\nirisAudit = true\n";
const CODE: &str = "READ_ONLY_STRICT_WITH_AUDIT";

struct Env {
    _tmp: tempfile::TempDir,
    home: PathBuf,
    proj: PathBuf,
}

impl Env {
    fn new(operator: &str, project: Option<&str>) -> Env {
        let tmp = tempfile::tempdir().unwrap();
        let root = std::fs::canonicalize(tmp.path()).unwrap();
        let home = root.join("home");
        let proj = root.join("proj");
        std::fs::create_dir_all(&home).unwrap();
        std::fs::create_dir_all(&proj).unwrap();
        std::fs::write(home.join(".iris-agentic-dev.toml"), operator).unwrap();
        if let Some(p) = project {
            std::fs::write(proj.join(".iris-agentic-dev.toml"), p).unwrap();
        }
        Env {
            _tmp: tmp,
            home,
            proj,
        }
    }

    fn command(&self, args: &[&str], env: &[(&str, &str)]) -> std::process::Command {
        let mut c = iris_agentic_dev_core::testing::clean_command(&bin());
        c.env("HOME", &self.home)
            .env("USERPROFILE", &self.home)
            .current_dir(&self.proj)
            .args(args)
            .args(["--host", "127.0.0.1", "--web-port", "1"]);
        for (k, v) in env {
            c.env(k, v);
        }
        c
    }

    fn run(&self, args: &[&str], env: &[(&str, &str)]) -> Output {
        self.command(args, env).output().unwrap()
    }

    /// `mcp` over stdio, sent `initialize`, with stdin then closed.
    fn mcp(&self, env: &[(&str, &str)]) -> Output {
        let mut child = self
            .command(&["mcp"], env)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let mut stdin = child.stdin.take().unwrap();
        let _ = stdin.write_all(
            b"{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"initialize\",\"params\":{\"protocolVersion\":\"2025-03-26\",\"capabilities\":{},\"clientInfo\":{\"name\":\"t\",\"version\":\"0\"}}}\n",
        );
        drop(stdin);
        child.wait_with_output().unwrap()
    }
}

fn text(o: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&o.stdout),
        String::from_utf8_lossy(&o.stderr)
    )
}

/// A named way to set the two: its label, its files, and its launch environment.
type Combo = (&'static str, Env, Vec<(&'static str, &'static str)>);

/// Every way the two settings can meet: both in the operator file, strict from the launch
/// environment, audit from the project file, strict from the project file.
fn combos() -> Vec<Combo> {
    vec![
        (
            "operator file",
            Env::new(&format!("{STRICT}{AUDIT}"), None),
            vec![],
        ),
        (
            "IAD_READ_ONLY",
            Env::new(AUDIT, None),
            vec![("IAD_READ_ONLY", "strict")],
        ),
        ("project audit", Env::new(STRICT, Some(AUDIT)), vec![]),
        ("project strict", Env::new(AUDIT, Some(STRICT)), vec![]),
    ]
}

#[test]
fn the_mcp_server_refuses_to_start() {
    for (what, e, env) in combos() {
        let o = e.mcp(&env);
        let t = text(&o);
        assert_eq!(o.status.code(), Some(2), "{what}: {t}");
        assert!(t.contains(CODE), "{what}: {t}");
        assert!(
            String::from_utf8_lossy(&o.stdout).trim().is_empty(),
            "{what}: the server answered the handshake: {t}"
        );
    }
}

#[test]
fn the_tool_command_refuses_iris_tools_before_the_network() {
    for (what, e, env) in combos() {
        for (tool, args) in [
            ("iris_query", r#"{"query":"SELECT 1"}"#),
            ("iris_execute", r#"{"code":"Write 1"}"#),
        ] {
            let o = e.run(&["tool", tool, "--args", args], &env);
            let t = text(&o);
            assert_ne!(o.status.code(), Some(0), "{what} {tool}: {t}");
            assert!(t.contains(CODE), "{what} {tool}: {t}");
            assert!(
                t.contains("irisAudit") && t.contains("read_only"),
                "{what} {tool}: the message names both settings: {t}"
            );
            assert!(
                !t.contains("IRIS_UNREACHABLE") && !t.contains("error sending request"),
                "{what} {tool}: the call reached the network: {t}"
            );
        }
    }
}

#[test]
fn check_config_reports_the_conflict() {
    for (what, e, env) in combos() {
        let o = e.run(&["tool", "check_config", "--args", "{}"], &env);
        assert!(text(&o).contains(CODE), "{what}: {}", text(&o));
    }
}

#[test]
fn either_setting_alone_does_not_refuse() {
    // The control: the refusals above are the combination's.
    for (what, operator, env) in [
        ("strict alone", STRICT, vec![]),
        ("audit alone", AUDIT, vec![]),
        ("launch strict alone", "", vec![("IAD_READ_ONLY", "strict")]),
    ] {
        let e = Env::new(operator, None);
        let o = e.run(
            &["tool", "iris_query", "--args", r#"{"query":"SELECT 1"}"#],
            &env,
        );
        assert!(!text(&o).contains(CODE), "{what}: {}", text(&o));
    }
}

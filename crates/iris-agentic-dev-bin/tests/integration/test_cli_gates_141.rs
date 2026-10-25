//! Layer 2 for spec 141 FR-003 and R1 C1, C2: the CLI runs the same gates as the MCP server, and
//! the compile hook goes through the binary.
//!
//! `tool`, `exec`, `compile <file>`, `query` and `doc put` each ran with no write gate, or with
//! `is_write_allowed()`, which ignores the toml declaration. With writes off in the operator file
//! every one of them must exit 1 with the gate's code, the same body the MCP server sends.
//!
//! No IRIS is needed: the connection is `127.0.0.1:1`, where nothing listens, and a refusal has to
//! come before any request.

#![cfg(unix)]

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_iris-agentic-dev"))
}

struct Env {
    _tmp: tempfile::TempDir,
    home: PathBuf,
    proj: PathBuf,
}

impl Env {
    /// `operator` is the operator file; `project` the project file, when there is one.
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

    fn run(&self, args: &[&str]) -> Output {
        let mut c = iris_agentic_dev_core::testing::clean_command(&bin());
        c.env("HOME", &self.home)
            .env("USERPROFILE", &self.home)
            .current_dir(&self.proj)
            .args(args)
            .args(["--host", "127.0.0.1", "--web-port", "1"]);
        c.output().unwrap()
    }

    fn file(&self, name: &str, body: &str) -> PathBuf {
        let p = self.proj.join(name);
        std::fs::write(&p, body).unwrap();
        p
    }
}

fn text(o: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&o.stdout),
        String::from_utf8_lossy(&o.stderr)
    )
}

fn assert_gate_refusal(what: &str, o: &Output, code: &str) {
    let t = text(o);
    assert_eq!(
        o.status.code(),
        Some(1),
        "{what}: expected exit 1, got {:?}. Output: {t}",
        o.status.code()
    );
    assert!(
        t.contains(code),
        "{what}: expected {code} in the output, got: {t}"
    );
    assert!(
        !t.contains("IRIS_UNREACHABLE") && !t.contains("error sending request"),
        "{what}: the call reached the network instead of being refused: {t}"
    );
}

const WRITES_OFF: &str = "write_tools_enabled = false\n";

#[test]
fn tool_iris_execute_with_writes_off_exits_1_with_the_gate_code() {
    let e = Env::new(WRITES_OFF, None);
    let o = e.run(&["tool", "iris_execute", "--args", r#"{"code":"Write 1"}"#]);
    assert_gate_refusal("tool iris_execute", &o, "WRITE_TOOLS_DISABLED");
}

#[test]
fn tool_global_kill_and_iris_global_kill_are_refused_in_every_spelling() {
    let e = Env::new(WRITES_OFF, None);
    let o = e.run(&[
        "tool",
        "global_kill",
        "--args",
        r#"{"global":"^Sentinel141"}"#,
    ]);
    assert_gate_refusal("tool global_kill", &o, "WRITE_TOOLS_DISABLED");
    for args in [
        r#"{"action":"kill","global_name":"^Sentinel141"}"#,
        r#"{"mode":"kill","globalName":"^Sentinel141"}"#,
        r#"{"action":"kill","global":"^Sentinel141","confirmed":true}"#,
    ] {
        let o = e.run(&["tool", "iris_global", "--args", args]);
        assert_gate_refusal(
            &format!("tool iris_global {args}"),
            &o,
            "WRITE_TOOLS_DISABLED",
        );
    }
}

#[test]
fn exec_compile_query_and_doc_put_are_refused_with_the_same_code() {
    let e = Env::new(WRITES_OFF, None);
    let o = e.run(&["exec", "Write 1"]);
    assert_gate_refusal("exec", &o, "WRITE_TOOLS_DISABLED");

    let cls = e.file("Sentinel141.cls", "Class Sentinel141 {}\n");
    let o = e.run(&["compile", cls.to_str().unwrap()]);
    assert_gate_refusal("compile <file>", &o, "WRITE_TOOLS_DISABLED");

    let o = e.run(&["compile"]);
    assert!(
        !text(&o).contains("CompileAll failed") && o.status.code() == Some(1),
        "`compile` with no file must go through iris_compile or be refused, not run CompileAll \
         outside every gate: {}",
        text(&o)
    );

    let o = e.run(&["query", "UPDATE Sentinel141.T SET A = 1"]);
    assert_gate_refusal("query UPDATE", &o, "WRITE_TOOLS_DISABLED");

    let o = e.run(&["doc", "put", "Sentinel141", "--file", cls.to_str().unwrap()]);
    assert_gate_refusal("doc put", &o, "WRITE_TOOLS_DISABLED");
}

#[test]
fn the_cli_refuses_a_tool_the_toolset_does_not_advertise() {
    let e = Env::new("", None);
    let o = e.run(&[
        "tool",
        "skill_community_install",
        "--toolset",
        "merged",
        "--args",
        r#"{"name":"x"}"#,
    ]);
    assert_eq!(o.status.code(), Some(1), "{}", text(&o));
    assert!(
        text(&o).contains("not registered") || text(&o).contains("unknown tool"),
        "{}",
        text(&o)
    );
}

// T-R4a-B08
#[test]
fn a_cli_refusal_carries_the_untrusted_setting_and_the_set_aside_id() {
    // The project file raises writes and has not been trusted: the same body the MCP server sends.
    let e = Env::new(
        "",
        Some("host = \"127.0.0.1\"\nweb_port = 1\nnamespace = \"PROD\"\nwrite_tools_enabled = true\n"),
    );
    let mut c = iris_agentic_dev_core::testing::clean_command(&bin());
    c.env("HOME", &e.home)
        .env("USERPROFILE", &e.home)
        .current_dir(&e.proj)
        .args(["tool", "iris_execute", "--args", r#"{"code":"Write 1"}"#]);
    let o = c.output().unwrap();
    let t = text(&o);
    assert_eq!(o.status.code(), Some(1), "{t}");
    let body: serde_json::Value = t
        .lines()
        .find_map(|l| serde_json::from_str(l).ok())
        .unwrap_or_else(|| panic!("no JSON body in {t}"));
    assert_eq!(body["error_code"], "WRITE_TOOLS_DISABLED", "{body}");
    assert_eq!(body["refusal"]["kind"], "untrusted_setting", "{body}");
    assert_eq!(
        body["refusal"]["set_aside"], "workspace:write_tools_enabled",
        "{body}"
    );
}

// ── the compile hook ────────────────────────────────────────────────────────────────────────────

fn hook() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../scripts/compile-hook.sh")
}

fn stub(dir: &Path, name: &str, script: &str) {
    use std::os::unix::fs::PermissionsExt;
    let p = dir.join(name);
    std::fs::write(&p, format!("#!/bin/sh\n{script}\n")).unwrap();
    std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
}

#[test]
fn the_compile_hook_runs_the_binary_and_never_curl() {
    let tmp = tempfile::tempdir().unwrap();
    let stubs = tmp.path().join("stubs");
    std::fs::create_dir_all(&stubs).unwrap();
    let argv = tmp.path().join("argv.txt");
    let curl_marker = tmp.path().join("curl-was-run");
    stub(
        &stubs,
        "iris-agentic-dev",
        &format!("echo \"$@\" >> {}\necho 'Compiled OK'", argv.display()),
    );
    stub(
        &stubs,
        "curl",
        &format!("touch {}\nexit 1", curl_marker.display()),
    );
    let file = tmp.path().join("MyApp").join("Foo.cls");
    std::fs::create_dir_all(file.parent().unwrap()).unwrap();
    std::fs::write(&file, "Class MyApp.Foo {}\n").unwrap();

    let path = format!(
        "{}:{}",
        stubs.display(),
        std::env::var("PATH").unwrap_or_default()
    );
    let mut child = Command::new("bash")
        .arg(hook())
        .env("PATH", path)
        .env("IRIS_HOST", "127.0.0.1")
        .env("IRIS_WEB_PORT", "1")
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    {
        use std::io::Write;
        let input = serde_json::json!({
            "hook_event_name": "PostToolUse",
            "tool_input": {"file_path": file.to_string_lossy()},
        });
        child
            .stdin
            .take()
            .unwrap()
            .write_all(input.to_string().as_bytes())
            .unwrap();
    }
    let o = child.wait_with_output().unwrap();
    assert!(o.status.success(), "{}", text(&o));
    assert!(
        !curl_marker.exists(),
        "the hook ran curl; it posts to IRIS without any gate"
    );
    let recorded = std::fs::read_to_string(&argv).unwrap_or_default();
    assert!(
        recorded.contains("compile") && recorded.contains("Foo.cls"),
        "the hook must run `iris-agentic-dev compile <file>`, recorded argv: {recorded:?}"
    );
}

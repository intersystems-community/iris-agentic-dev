//! Layer 2 for spec 141 R5 C15: over MCP stdio, `read_only = "strict"` (a project file) or
//! `IAD_READ_ONLY=strict` (launch settings) hides every scratch-class tool from `tools/list` and
//! refuses a call to one with `READ_ONLY_STRICT`, before any request reaches IRIS.
//!
//! Nothing listens on the default connection. A refusal has to be the gate's answer: a connection
//! error would mean the call went past it.

use iris_agentic_dev_core::testing::McpSession;
use iris_agentic_dev_core::tools::write_gate::CLASSIFICATION;
use serde_json::{json, Value};

/// The default connection points at a port nothing listens on, so discovery never finds a running
/// container and a call that reaches IRIS fails to connect.
const NOTHING_LISTENS: [(&str, &str); 2] = [("IRIS_HOST", "127.0.0.1"), ("IRIS_WEB_PORT", "1")];

fn session(project: Option<&str>, env: &[(String, String)]) -> (McpSession, tempfile::TempDir) {
    let env: Vec<(String, String)> = NOTHING_LISTENS
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .chain(env.iter().cloned())
        .collect();
    let env = &env[..];
    let dir = tempfile::tempdir().unwrap();
    if let Some(body) = project {
        std::fs::write(dir.path().join(".iris-agentic-dev.toml"), body).unwrap();
    }
    let cwd = std::fs::canonicalize(dir.path()).unwrap();
    (McpSession::start_in(Some(&cwd), env), dir)
}

fn body(answer: &Value) -> Value {
    if let Some(v) = answer.pointer("/result/structuredContent") {
        if !v.is_null() {
            return v.clone();
        }
    }
    answer
        .pointer("/result/content/0/text")
        .and_then(Value::as_str)
        .and_then(|t| serde_json::from_str(t).ok())
        .unwrap_or_else(|| json!({"rpc": answer}))
}

const STRICT_FILE: &str = "read_only = \"strict\"\n";

fn scratch_tools() -> Vec<&'static str> {
    CLASSIFICATION
        .iter()
        .filter(|e| e.scratch)
        .map(|e| e.tool)
        .collect()
}

#[test]
#[ignore = "spawns the built binary; run with --include-ignored"]
fn without_strict_the_scratch_tools_are_listed() {
    let (mut s, _d) = session(None, &[]);
    let listed = s.list_tools();
    let scratch = scratch_tools();
    assert!(
        scratch.len() > 25,
        "the column is nearly empty: {scratch:?}"
    );
    for t in [
        "iris_table_info",
        "global_preview",
        "my_access",
        "iris_execute",
    ] {
        assert!(
            listed.iter().any(|n| n == t),
            "{t} must be listed: {listed:?}"
        );
    }
}

#[test]
#[ignore = "spawns the built binary; run with --include-ignored"]
fn strict_from_a_project_file_hides_every_scratch_tool() {
    let (mut s, _d) = session(Some(STRICT_FILE), &[]);
    let listed = s.list_tools();
    let leaked: Vec<&str> = scratch_tools()
        .into_iter()
        .filter(|t| listed.iter().any(|n| n == t))
        .collect();
    assert!(
        leaked.is_empty(),
        "strict read-only still lists scratch-class tools: {leaked:?}"
    );
    // The rest of the surface stays: strict is not "no tools".
    for t in [
        "iris_doc",
        "iris_query",
        "iris_servers",
        "check_config",
        "skill",
    ] {
        assert!(
            listed.iter().any(|n| n == t),
            "{t} must stay listed: {listed:?}"
        );
    }
}

#[test]
#[ignore = "spawns the built binary; run with --include-ignored"]
fn strict_from_launch_settings_hides_them_too() {
    let (mut s, _d) = session(None, &[("IAD_READ_ONLY".to_string(), "strict".to_string())]);
    let listed = s.list_tools();
    assert!(!listed.iter().any(|n| n == "iris_table_info"), "{listed:?}");
}

#[test]
#[ignore = "spawns the built binary; run with --include-ignored"]
fn a_call_to_a_scratch_tool_is_refused_before_iris_hears_of_it() {
    for (project, env) in [
        (Some(STRICT_FILE), vec![]),
        (
            None,
            vec![("IAD_READ_ONLY".to_string(), "strict".to_string())],
        ),
    ] {
        let (mut s, _d) = session(project, &env);
        for (tool, args) in [
            ("iris_table_info", json!({"table": "Sample.Person"})),
            ("global_preview", json!({"global": "^Nope"})),
            ("my_access", json!({})),
            ("iris_execute", json!({"code": "Write 1"})),
        ] {
            let answer = s.call(tool, &args);
            let b = body(&answer);
            assert_eq!(
                b["error_code"], "READ_ONLY_STRICT",
                "{tool}: expected the strict refusal, got {answer}"
            );
            assert_eq!(
                b["refusal"]["kind"], "operator_limit",
                "{tool}: the refusal is an operator limit: {b}"
            );
        }
    }
}

#[test]
#[ignore = "spawns the built binary; run with --include-ignored"]
fn a_tool_that_compiles_nothing_still_answers_under_strict() {
    let (mut s, _d) = session(Some(STRICT_FILE), &[]);
    let answer = s.call("iris_servers", &json!({}));
    let b = body(&answer);
    assert_ne!(b["error_code"], "READ_ONLY_STRICT", "{answer}");
}

#[test]
#[ignore = "spawns the built binary; run with --include-ignored"]
fn check_config_reports_read_only() {
    let (mut s, _d) = session(Some(STRICT_FILE), &[]);
    let b = body(&s.call("check_config", &json!({})));
    assert_eq!(b["read_only"], "strict", "{b}");
    assert!(
        b["read_only_from"]
            .as_str()
            .is_some_and(|f| f.ends_with(".iris-agentic-dev.toml")),
        "check_config names the file that asked: {b}"
    );
    let (mut s, _d) = session(None, &[]);
    let b = body(&s.call("check_config", &json!({})));
    assert_eq!(b["read_only"], "off", "{b}");

    let (mut s, _d) = session(None, &[("IAD_READ_ONLY".to_string(), "strict".to_string())]);
    let b = body(&s.call("check_config", &json!({})));
    assert_eq!(b["read_only"], "strict", "{b}");
    assert_eq!(b["read_only_from"], "IAD_READ_ONLY", "{b}");
}

/// C18 through the server: with writes on and the destructive tier off, a lookup import is refused
/// by the destructive gate (it replaces a whole table), and an export is not.
#[test]
#[ignore = "spawns the built binary; run with --include-ignored"]
fn lookup_import_meets_the_destructive_gate() {
    let env = [
        ("IRIS_WRITE_TOOLS_ENABLED".to_string(), "1".to_string()),
        (
            "IRIS_DESTRUCTIVE_TOOLS_ENABLED".to_string(),
            "0".to_string(),
        ),
    ];
    let (mut s, _d) = session(None, &env);
    let answer = s.call(
        "iris_lookup_transfer",
        &json!({"action": "import", "table": "T141", "xml": "<lookupTable/>"}),
    );
    assert_eq!(
        body(&answer)["error_code"],
        "DESTRUCTIVE_TOOLS_DISABLED",
        "{answer}"
    );
    let answer = s.call(
        "iris_lookup_transfer",
        &json!({"action": "export", "table": "T141"}),
    );
    assert_ne!(
        body(&answer)["error_code"],
        "DESTRUCTIVE_TOOLS_DISABLED",
        "{answer}"
    );
}

/// Strict read-only with `irisAudit = true` is a configuration error at startup (T178, R5 C15):
/// exit 2, both settings named on stderr, and no MCP session. Both spellings of strict are tried,
/// the file key and `IAD_READ_ONLY`.
#[test]
#[ignore = "spawns the built binary; run with --include-ignored"]
fn strict_with_iris_audit_exits_two_without_serving() {
    use iris_agentic_dev_core::testing::{clean_mcp_command, iad_binary_path};
    use std::io::Write;
    use std::process::Stdio;

    let bin = iad_binary_path();
    assert!(
        bin.exists(),
        "no binary at {}; cargo build first",
        bin.display()
    );
    for (label, file, strict_env) in [
        (
            "strict in the file",
            "read_only = \"strict\"\n[policy.default]\nirisAudit = true\n",
            false,
        ),
        (
            "strict from IAD_READ_ONLY",
            "[policy.default]\nirisAudit = true\n",
            true,
        ),
    ] {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join(".iris-agentic-dev.toml"), file).unwrap();
        let cwd = std::fs::canonicalize(dir.path()).unwrap();
        let mut cmd = clean_mcp_command(&bin);
        cmd.current_dir(&cwd)
            .env("OBJECTSCRIPT_WORKSPACE", &cwd)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        if strict_env {
            cmd.env("IAD_READ_ONLY", "strict");
        }
        let mut child = cmd.spawn().expect("spawn iris-agentic-dev mcp");
        let mut stdin = child.stdin.take().unwrap();
        let init = json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{
            "protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"t178","version":"0"}}});
        // The server may already be gone; a failed write is the expected outcome.
        let _ = writeln!(stdin, "{init}");
        drop(stdin);
        let out = child.wait_with_output().unwrap();
        let stderr = String::from_utf8_lossy(&out.stderr);
        let stdout = String::from_utf8_lossy(&out.stdout);
        assert_eq!(
            out.status.code(),
            Some(2),
            "[{label}] expected exit 2.\nstderr: {stderr}\nstdout: {stdout}"
        );
        assert!(
            stderr.contains("read_only") && stderr.contains("irisAudit"),
            "[{label}] the error must name both settings; stderr: {stderr}"
        );
        assert!(
            stdout.trim().is_empty(),
            "[{label}] the server spoke under a configuration it refused: {stdout}"
        );
    }
}

// ── Strict closes the write and destructive gates (T203) ─────────────────────────────────────────

/// An operator file in a temporary `HOME`, and launch settings that open both gates. Nothing listens on the default connection, so a refusal has
/// to be the gate's.
const OPEN: &str = "write_tools_enabled = true\ndestructive_tools_enabled = true\n";
const STRICT_AND_OPEN: &str =
    "read_only = \"strict\"\nwrite_tools_enabled = true\ndestructive_tools_enabled = true\n";

fn operator_session(file: &str, extra_env: &[(&str, &str)]) -> (McpSession, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    let root = std::fs::canonicalize(dir.path()).unwrap();
    let (home, proj) = (root.join("home"), root.join("proj"));
    std::fs::create_dir_all(&home).unwrap();
    std::fs::create_dir_all(&proj).unwrap();
    std::fs::write(home.join(".iris-agentic-dev.toml"), file).unwrap();
    let mut env: Vec<(String, String)> = vec![
        ("HOME".into(), home.display().to_string()),
        ("USERPROFILE".into(), home.display().to_string()),
        ("IRIS_HOST".into(), "127.0.0.1".into()),
        ("IRIS_WEB_PORT".into(), "1".into()),
        ("IRIS_WRITE_TOOLS_ENABLED".into(), "1".into()),
        ("IRIS_DESTRUCTIVE_TOOLS_ENABLED".into(), "1".into()),
    ];
    env.extend(
        extra_env
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string())),
    );
    (McpSession::start_in(Some(&proj), &env), dir)
}

#[test]
#[ignore = "spawns the built binary; run with --include-ignored"]
fn strict_in_the_operator_file_outranks_write_gates_opened_there_and_in_the_environment() {
    let (mut s, _d) = operator_session(STRICT_AND_OPEN, &[]);
    for (tool, args) in [
        (
            "iris_doc",
            json!({"mode": "put", "name": "Strict.T203.cls", "content": "Class Strict.T203 {}"}),
        ),
        (
            "iris_doc",
            json!({"action": "PUT", "name": "Strict.T203.cls", "content": "Class Strict.T203 {}"}),
        ),
        (
            "global_kill",
            json!({"global": "^StrictT203", "confirm_token": "x"}),
        ),
    ] {
        let answer = s.call(tool, &args);
        let b = body(&answer);
        assert_eq!(
            b["error_code"], "READ_ONLY_STRICT",
            "{tool} {args}: {answer}"
        );
        assert_eq!(
            b["gate"], "write_tools",
            "{tool} {args}: the write gate refused: {b}"
        );
        assert_eq!(b["gate_source"], "read_only_strict", "{b}");
    }
    let b = body(&s.call("check_config", &json!({})));
    assert_eq!(b["write_tools_enabled"], false, "{b}");
    assert_eq!(b["write_tools_source"], "read_only_strict", "{b}");
    assert_eq!(b["destructive_tools_enabled"], false, "{b}");
    assert_eq!(b["destructive_tools_source"], "read_only_strict", "{b}");
    assert_eq!(b["read_only"], "strict", "{b}");
}

#[test]
#[ignore = "spawns the built binary; run with --include-ignored"]
fn strict_from_launch_settings_outranks_an_operator_file_that_opens_the_gates() {
    // The operator file opens both gates and does not mention strict; the launch variable does.
    let (mut s, _d) = operator_session(OPEN, &[("IAD_READ_ONLY", "strict")]);
    let b = body(&s.call(
        "iris_doc",
        &json!({"mode": "put", "name": "Strict.T203.cls", "content": "x"}),
    ));
    assert_eq!(b["error_code"], "READ_ONLY_STRICT", "{b}");
    assert_eq!(b["gate"], "write_tools", "{b}");
}

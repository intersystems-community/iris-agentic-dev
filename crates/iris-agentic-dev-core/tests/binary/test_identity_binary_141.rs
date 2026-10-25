//! Layer 2 for spec 141 R2 (T-R2-B01 to B07): over MCP stdio, the agent's code runs as the
//! service account on every path, or the call is refused.
//!
//! `docker` and `ssh` are shell stubs first on `PATH`. They append their argv to a log and their
//! stdin to another, and exit 125 with nothing on stdout, so iad reports `CONTAINER_UNREACHABLE`
//! and the test reads what iad *sent*. No IRIS is needed.

use iris_agentic_dev_core::testing::McpSession;
use iris_agentic_dev_core::tools::exec::SERVICE_ACCOUNT_TOOLS;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

struct Rig {
    _dir: tempfile::TempDir,
    root: PathBuf,
    home: PathBuf,
    proj: PathBuf,
    stubs: PathBuf,
    logs: PathBuf,
}

impl Rig {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let root = std::fs::canonicalize(dir.path()).unwrap();
        let rig = Self {
            home: root.join("home"),
            proj: root.join("proj"),
            stubs: root.join("stubs"),
            logs: root.join("logs"),
            root,
            _dir: dir,
        };
        for d in [&rig.home, &rig.proj, &rig.stubs, &rig.logs] {
            std::fs::create_dir_all(d).unwrap();
        }
        for tool in ["docker", "ssh"] {
            let script = format!(
                "#!/bin/sh\n\
                 echo \"$@\" >> \"$STUB_LOGS/{tool}.argv\"\n\
                 cat >> \"$STUB_LOGS/{tool}.stdin\"\n\
                 exit 125\n"
            );
            let path = rig.stubs.join(tool);
            std::fs::write(&path, script).unwrap();
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
            }
        }
        rig
    }

    fn operator(&self, body: &str) {
        std::fs::write(self.home.join(".iris-agentic-dev.toml"), body).unwrap();
    }

    fn project(&self, body: &str) {
        std::fs::write(self.proj.join(".iris-agentic-dev.toml"), body).unwrap();
    }

    fn servers_json(&self, body: &str) {
        let dir = self.home.join(".config").join("iris-agentic-dev");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("servers.json"), body).unwrap();
    }

    fn session(&self, env: &[(&str, &str)]) -> McpSession {
        let path = format!(
            "{}:{}",
            self.stubs.display(),
            std::env::var("PATH").unwrap_or_default()
        );
        let mut vars: Vec<(String, String)> = vec![
            ("HOME".into(), self.home.display().to_string()),
            ("USERPROFILE".into(), self.home.display().to_string()),
            ("PATH".into(), path),
            ("STUB_LOGS".into(), self.logs.display().to_string()),
        ];
        vars.extend(env.iter().map(|(k, v)| (k.to_string(), v.to_string())));
        McpSession::start_in(Some(&self.proj), &vars)
    }

    fn log(&self, name: &str) -> String {
        std::fs::read_to_string(self.logs.join(name)).unwrap_or_default()
    }
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

/// Nothing listens on port 1: a connection error is not a refusal.
const DOCKER_ONLY: &str = "docker_only = true\ncontainer = \"c141\"\n";

/// An operator file that opens every gate for the servers the tests name, so a refusal can only
/// come from the identity.
const OPEN_GATES: &str = r#"
write_tools_enabled = true
destructive_tools_enabled = true

[policy.qa]
mcpTemplate = "dev"
writeTools = true
destructiveTools = true
dataPolicy = "allow"

[policy.default]
mcpTemplate = "dev"
writeTools = true
destructiveTools = true
dataPolicy = "allow"
"#;

// ── B01 ───────────────────────────────────────────────────────────────────────

#[test]
#[ignore = "spawns the built binary; run with --include-ignored"]
fn the_docker_path_sends_the_login_guard_first_and_no_password() {
    let rig = Rig::new();
    rig.operator(OPEN_GATES);
    rig.project(DOCKER_ONLY);
    let mut s = rig.session(&[
        ("IRIS_SERVICE_USERNAME", "svc"),
        ("IRIS_SERVICE_PASSWORD", "Pw141"),
    ]);
    let answer = s.call("iris_execute", &json!({"code": "Write 1"}));
    let b = body(&answer);
    drop(s);

    let stdin = rig.log("docker.stdin");
    let first = stdin.lines().next().unwrap_or_default();
    assert!(
        first.starts_with("Set %iad=$SYSTEM.Security.Login(\"svc\")"),
        "the first stdin line is the guard, got {first:?}; full stdin: {stdin:?}; answer: {answer}"
    );
    assert!(
        stdin.contains("Write 1"),
        "the agent's code follows the guard: {stdin:?}"
    );
    assert!(
        !stdin.contains("Pw141"),
        "the password is on stdin: {stdin:?}"
    );
    assert!(
        !rig.log("docker.argv").contains("Pw141"),
        "the password is on argv: {}",
        rig.log("docker.argv")
    );
    assert!(
        rig.log("docker.argv").contains("c141"),
        "the container is the toml's: {}",
        rig.log("docker.argv")
    );
    assert_eq!(b["identity"]["source"], json!("service_account_env"), "{b}");
    assert_eq!(b["identity"]["verified"], json!(false), "{b}");
    assert_eq!(b["identity"]["user"], json!("svc"), "{b}");
    assert_eq!(b["error_code"], json!("CONTAINER_UNREACHABLE"), "{b}");
}

// ── B02 ───────────────────────────────────────────────────────────────────────

#[test]
#[ignore = "spawns the built binary; run with --include-ignored"]
fn ssh_from_the_operator_file_is_batch_mode_with_a_checked_key_and_the_guard_first() {
    let rig = Rig::new();
    // The operator file is the connection file here: nothing else names the connection.
    rig.operator(&format!(
        "{DOCKER_ONLY}ssh_host = \"stub.example\"\n{OPEN_GATES}"
    ));
    let mut s = rig.session(&[("IRIS_SERVICE_USERNAME", "svc")]);
    let answer = s.call("iris_execute", &json!({"code": "Write 1"}));
    drop(s);

    let argv = rig.log("ssh.argv");
    assert!(argv.contains("BatchMode=yes"), "{argv:?}; answer: {answer}");
    assert!(!argv.contains("StrictHostKeyChecking=no"), "{argv:?}");
    assert!(
        argv.contains("StrictHostKeyChecking=accept-new"),
        "{argv:?}"
    );
    assert!(argv.contains("stub.example"), "{argv:?}");
    let stdin = rig.log("ssh.stdin");
    assert!(
        stdin.starts_with("Set %iad=$SYSTEM.Security.Login(\"svc\")"),
        "the guard is the first line sent: {stdin:?}"
    );
    assert!(
        rig.log("docker.argv").is_empty(),
        "the local docker was used: {}",
        rig.log("docker.argv")
    );
}

#[test]
#[ignore = "spawns the built binary; run with --include-ignored"]
fn ssh_host_from_a_repository_file_is_dropped_and_never_called() {
    let rig = Rig::new();
    rig.operator(OPEN_GATES);
    rig.project(&format!("{DOCKER_ONLY}ssh_host = \"evil.example\"\n"));
    let mut s = rig.session(&[("IRIS_SERVICE_USERNAME", "svc")]);
    let _ = s.call("iris_execute", &json!({"code": "Write 1"}));
    let cfg = body(&s.call("check_config", &json!({})));
    drop(s);

    assert!(
        rig.log("ssh.argv").is_empty(),
        "a repository chose the host the code runs on: {}",
        rig.log("ssh.argv")
    );
    let text = cfg.to_string();
    assert!(
        text.contains("ssh_host") && cfg["set_aside"].to_string().contains("ssh_host"),
        "check_config lists ssh_host as set aside: {cfg}"
    );
}

// ── B03 ───────────────────────────────────────────────────────────────────────

fn nine_calls(server: &str) -> Vec<(&'static str, Value)> {
    vec![
        ("iris_execute", json!({"code": "Write 1", "server": server})),
        (
            "iris_query",
            json!({"mode": "write", "query": "INSERT INTO T141.Who (U) VALUES ('x')", "server": server}),
        ),
        (
            "iris_global",
            json!({"action": "set", "global_name": "IadT141", "value": "1", "server": server}),
        ),
        (
            "iris_global",
            json!({"action": "kill", "global_name": "IadT141", "server": server, "confirm": true}),
        ),
        (
            "iris_execute_method",
            json!({"class": "%SYSTEM.Process", "method": "UserName", "server": server}),
        ),
        (
            "global_kill",
            json!({"global": "^IadT141", "confirm_token": "x", "server": server}),
        ),
        (
            "iris_test",
            json!({"pattern": "T141.Tests", "server": server}),
        ),
        (
            "iris_coverage",
            json!({"mode": "run", "test_path": "T141.Tests", "classes": ["T141.A"], "server": server}),
        ),
        ("iris_ws_open", json!({"server": server})),
    ]
}

#[test]
#[ignore = "spawns the built binary; run with --include-ignored"]
fn a_server_with_no_service_account_is_refused_every_tool_that_runs_the_agents_code() {
    let rig = Rig::new();
    rig.operator(OPEN_GATES);
    rig.servers_json(
        r#"{"version":1,"servers":{"qa":{"host":"127.0.0.1","port":9,"namespace":"USER","username":"u"}}}"#,
    );
    let mut s = rig.session(&[
        ("IRIS_SERVICE_USERNAME", "svc"),
        ("IRIS_HOST", "127.0.0.1"),
        ("IRIS_WEB_PORT", "2"),
    ]);
    let mut seen = std::collections::BTreeSet::new();
    for spelling in ["qa", " qa ", "QA"] {
        for (tool, args) in nine_calls(spelling) {
            let answer = s.call(tool, &args);
            let b = body(&answer);
            let code = b["error_code"].as_str().unwrap_or("");
            if code == "SERVICE_ACCOUNT_UNCONFIGURED_FOR_SERVER" {
                seen.insert(tool);
                continue;
            }
            // Pool names are case-sensitive: `QA` names no server, and the call stops at the pool
            // before any IRIS contact. It must still never run.
            assert!(
                spelling == "QA"
                    && answer
                        .to_string()
                        .contains("SERVER_NOT_FOUND: no server named 'QA'"),
                "{tool} server={spelling:?}: expected the identity refusal, got {answer}"
            );
        }
    }
    for t in SERVICE_ACCOUNT_TOOLS
        .iter()
        .filter(|t| **t != "iris_ws_exec")
    {
        assert!(seen.contains(t), "{t} was not exercised");
    }
}

// ── B04 ───────────────────────────────────────────────────────────────────────

#[test]
#[ignore = "spawns the built binary; run with --include-ignored"]
fn a_named_server_never_falls_back_to_the_default_connections_container() {
    let rig = Rig::new();
    rig.operator(OPEN_GATES);
    rig.servers_json(
        r#"{"version":1,"servers":{"qa":{"host":"127.0.0.1","port":9,"namespace":"USER","username":"u"}}}"#,
    );
    let mut s = rig.session(&[
        ("IRIS_HOST", "127.0.0.1"),
        ("IRIS_WEB_PORT", "2"),
        ("IRIS_CONTAINER", "c141"),
    ]);
    let answer = s.call("iris_execute", &json!({"code": "Write 1", "server": "qa"}));
    let b = body(&answer);
    drop(s);

    assert_eq!(b["error_code"], json!("HTTP_EXECUTION_FAILED"), "{answer}");
    assert_eq!(
        b["no_docker_route"],
        json!("NO_DOCKER_ROUTE_FOR_SERVER"),
        "{answer}"
    );
    assert!(
        rig.log("docker.argv").is_empty() && rig.log("docker.stdin").is_empty(),
        "docker was called for a named server: {}",
        rig.log("docker.argv")
    );
}

// ── B05 ───────────────────────────────────────────────────────────────────────

#[test]
#[ignore = "spawns the built binary; run with --include-ignored"]
fn the_os_user_opt_in_comes_from_the_operator_and_a_repository_file_cannot_set_it() {
    let rig = Rig::new();
    rig.project("exec_as_os_user = true\n");
    let mut s = rig.session(&[]);
    let cfg = body(&s.call("check_config", &json!({})));
    drop(s);
    assert_eq!(
        cfg["identity"]["os_user_opt_in"]["enabled"],
        json!(false),
        "{cfg}"
    );
    assert!(
        cfg["identity"]["os_user_opt_in"]["ignored"]
            .to_string()
            .contains("exec_as_os_user"),
        "{cfg}"
    );

    let mut s = rig.session(&[("IAD_EXEC_AS_OS_USER", "1")]);
    let cfg = body(&s.call("check_config", &json!({})));
    assert_eq!(
        cfg["identity"]["os_user_opt_in"]["enabled"],
        json!(true),
        "{cfg}"
    );
    assert_eq!(
        cfg["identity"]["os_user_opt_in"]["source"],
        json!("env"),
        "{cfg}"
    );
}

// ── B06 ───────────────────────────────────────────────────────────────────────

#[test]
#[ignore = "spawns the built binary; run with --include-ignored"]
fn the_websocket_terminal_names_the_service_account_it_tried() {
    let rig = Rig::new();
    rig.operator(OPEN_GATES);
    let mut s = rig.session(&[
        ("IRIS_SERVICE_USERNAME", "svc141"),
        ("IRIS_USERNAME", "primary141"),
        ("IRIS_HOST", "127.0.0.1"),
        ("IRIS_WEB_PORT", "1"),
    ]);
    let answer = s.call("iris_ws_open", &json!({}));
    let text = answer.to_string();
    assert!(
        !text.contains("primary141"),
        "the primary's user name appears: {text}"
    );
    let b = body(&answer);
    assert_eq!(b["identity"]["user"], json!("svc141"), "{answer}");
    assert_eq!(b["identity"]["path"], json!("ws_terminal"), "{answer}");
}

// ── B07 ───────────────────────────────────────────────────────────────────────

#[test]
#[ignore = "spawns the built binary; run with --include-ignored"]
fn check_config_lists_each_servers_identity_and_the_tools_covered() {
    let rig = Rig::new();
    rig.operator(OPEN_GATES);
    rig.servers_json(
        r#"{"version":1,"servers":{
            "a":{"host":"127.0.0.1","port":9,"namespace":"USER","username":"u","service_username":"s1"},
            "b":{"host":"127.0.0.1","port":10,"namespace":"USER","username":"u"}}}"#,
    );
    let mut s = rig.session(&[("IRIS_SERVICE_USERNAME", "envsvc")]);
    let cfg = body(&s.call("check_config", &json!({})));
    let servers = cfg["identity"]["servers"].as_array().expect("servers[]");
    let a = servers.iter().find(|x| x["name"] == json!("a")).expect("a");
    let b = servers.iter().find(|x| x["name"] == json!("b")).expect("b");
    assert_eq!(a["user"], json!("s1"), "{cfg}");
    assert_eq!(a["source"], json!("service_account_server"), "{cfg}");
    assert_eq!(
        b["refused"],
        json!("SERVICE_ACCOUNT_UNCONFIGURED_FOR_SERVER"),
        "{cfg}"
    );
    let tools: Vec<&str> = cfg["identity"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(Value::as_str)
        .collect();
    assert_eq!(tools, SERVICE_ACCOUNT_TOOLS, "{cfg}");
    let _ = Path::new(&rig.root);
}

#[test]
#[ignore = "spawns the built binary; run with --include-ignored"]
fn a_blank_service_username_is_no_service_account() {
    let rig = Rig::new();
    let mut s = rig.session(&[("IRIS_SERVICE_USERNAME", "  ")]);
    let cfg = body(&s.call("check_config", &json!({})));
    assert_eq!(
        cfg["identity"]["service_account"]["configured"],
        json!(false),
        "{cfg}"
    );
}

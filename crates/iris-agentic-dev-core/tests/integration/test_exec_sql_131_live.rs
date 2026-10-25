//! Spec 131 US4: `&sql` inside `iris_execute`, on both execution paths, against iris-dev-iris.
//!
//! The HTTP path compiles the code into a class method, so IRIS expands `&sql` itself and iad must
//! leave it alone. Before 131 iad rewrote it anyway, and `&sql(SELECT COUNT(*) INTO :n ...)` failed
//! with `<PROPERTY DOES NOT EXIST> COUNT(*)`. The docker path runs the code in `iris session`, which
//! has no `&sql`, so there the translation is what runs.
//!
//! Run: `IRIS_HOST=localhost IRIS_WEB_PORT=52780 IRIS_USERNAME=_SYSTEM IRIS_PASSWORD=SYS
//! IRIS_CONTAINER=iris-dev-iris IAD_BINARY=$PWD/target/debug/iris-agentic-dev cargo test
//! --features testing --test integration test_exec_sql_131 -- --include-ignored --test-threads=1`

use iris_agentic_dev_core::testing::{iad_binary_path, live_env, McpSession};

/// The JSON body of an `iris_execute` answer.
fn body(answer: &serde_json::Value) -> serde_json::Value {
    let text = answer["result"]["content"][0]["text"]
        .as_str()
        .unwrap_or_else(|| panic!("no text content: {answer}"));
    serde_json::from_str(text).unwrap_or_else(|e| panic!("not JSON ({e}): {text}"))
}

fn http_exec(code: &str) -> serde_json::Value {
    let mut env = live_env();
    env.retain(|(k, _)| k != "IRIS_CONTAINER");
    let mut s = McpSession::start(&env);
    body(&s.call(
        "iris_execute",
        &serde_json::json!({"code": code, "namespace": "USER"}),
    ))
}

/// `iris_execute` through a `docker_only` connection, which never touches Atelier.
fn docker_exec(code: &str) -> Option<serde_json::Value> {
    let container = iris_agentic_dev_core::testing::LiveTarget::from_env().container;
    let bin = iad_binary_path();
    assert!(bin.exists(), "no binary at {}", bin.display());
    let dir = tempfile::tempdir().expect("tempdir");
    std::fs::write(
        dir.path().join(".iris-agentic-dev.toml"),
        format!("docker_only = true\ncontainer = \"{container}\"\nnamespace = \"USER\"\n"),
    )
    .unwrap();
    let mut cmd = iris_agentic_dev_core::testing::clean_mcp_command(&bin);
    cmd.current_dir(dir.path())
        .env("IRIS_CONTAINER", &container)
        .env("IRIS_USERNAME", "_SYSTEM")
        .env("IRIS_PASSWORD", "SYS")
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null());
    let mut child = cmd.spawn().expect("spawn iad mcp");
    use std::io::{BufRead, Write};
    let mut stdin = child.stdin.take().unwrap();
    let mut out = std::io::BufReader::new(child.stdout.take().unwrap());
    let init = serde_json::json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{
        "protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"t131","version":"0"}}});
    let call = serde_json::json!({"jsonrpc":"2.0","id":2,"method":"tools/call","params":{
        "name":"iris_execute","arguments":{"code": code, "namespace": "USER"}}});
    writeln!(stdin, "{init}").unwrap();
    writeln!(
        stdin,
        "{}",
        serde_json::json!({"jsonrpc":"2.0","method":"notifications/initialized","params":{}})
    )
    .unwrap();
    writeln!(stdin, "{call}").unwrap();
    let mut line = String::new();
    let mut answer = None;
    while out.read_line(&mut line).unwrap_or(0) > 0 {
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(&line) {
            if v["id"] == 2 {
                answer = Some(body(&v));
                break;
            }
        }
        line.clear();
    }
    drop(stdin);
    iris_agentic_dev_core::testing::stop_server(&mut child);
    answer
}

fn output(v: &serde_json::Value) -> String {
    v["output"].as_str().unwrap_or_default().trim().to_string()
}

// ── HTTP path: IRIS expands &sql, iad sends it untouched ─────────────────────

#[test]
#[ignore = "live iris-dev-iris"]
fn http_count_star_into_runs_natively() {
    let v = http_exec(
        "Set n=\"\" &sql(SELECT COUNT(*) INTO :n FROM INFORMATION_SCHEMA.TABLES) Write SQLCODE,\":\",(n>0),!",
    );
    assert_eq!(v["success"], true, "{v}");
    assert_eq!(output(&v), "0:1", "{v}");
    assert!(v.get("sql_translated").is_none(), "{v}");
    assert!(v.get("translated_code").is_none(), "{v}");
}

#[test]
#[ignore = "live iris-dev-iris"]
fn http_top_alias_and_same_line_sqlcode() {
    let v = http_exec(
        "Set n=\"\" &sql(SELECT TOP 1 TABLE_NAME AS t INTO :n FROM INFORMATION_SCHEMA.TABLES) Write SQLCODE,\":\",(n'=\"\"),!",
    );
    assert_eq!(output(&v), "0:1", "{v}");
}

#[test]
#[ignore = "live iris-dev-iris"]
fn http_no_row_gives_sqlcode_100() {
    let v = http_exec(
        "Set n=\"x\" &sql(SELECT TABLE_NAME INTO :n FROM INFORMATION_SCHEMA.TABLES WHERE 1=0)\nWrite SQLCODE,!",
    );
    assert_eq!(output(&v), "100", "{v}");
}

// ── docker path: iad's translation is what runs ──────────────────────────────

#[test]
#[ignore = "live iris-dev-iris via docker exec"]
fn docker_count_star_into_runs_translated() {
    let Some(v) = docker_exec(
        "Set n=\"\" &sql(SELECT COUNT(*) INTO :n FROM INFORMATION_SCHEMA.TABLES) Write SQLCODE,\":\",(n>0),!",
    ) else {
        panic!("no answer from the docker_only server");
    };
    assert_eq!(v["sql_translated"], true, "{v}");
    assert!(output(&v).ends_with("0:1"), "{v}");
}

#[test]
#[ignore = "live iris-dev-iris via docker exec"]
fn docker_no_row_gives_sqlcode_100_and_empty_var() {
    let v = docker_exec(
        "Set n=\"x\" &sql(SELECT TABLE_NAME INTO :n FROM INFORMATION_SCHEMA.TABLES WHERE 1=0)\nWrite SQLCODE,\":[\",n,\"]\",!",
    )
    .expect("answer");
    assert!(output(&v).ends_with("100:[]"), "{v}");
}

#[test]
#[ignore = "live iris-dev-iris via docker exec"]
fn docker_dml_error_sets_sqlcode() {
    let v =
        docker_exec("&sql(DELETE FROM IadNope131.Missing WHERE 1=0)\nWrite \"code=\",SQLCODE,!")
            .expect("answer");
    let out = output(&v);
    assert!(out.contains("code=-30"), "{v}");
}

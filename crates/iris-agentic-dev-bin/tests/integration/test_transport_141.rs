//! Layer 2 for spec 141 FR-025 / FR-035 (T-R4b-B02): the MCP HTTP transport refuses what the
//! contract says it refuses, before rmcp sees the request.
//!
//! No IRIS is involved. The server points at a closed port, `initialize` does not need a
//! connection, and a refused request must not open one either.

use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use iris_agentic_dev_core::testing::{clean_mcp_command, stop_server};

const TOKEN: &str = "0123456789abcdef0123456789abcdef-transport";

const INIT: &str = r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-03-26","capabilities":{},"clientInfo":{"name":"t141","version":"0"}}}"#;

fn free_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

fn command(home: &Path, cwd: &Path, bind: &str, port: u16, extra: &[&str]) -> Command {
    let bin = env!("CARGO_BIN_EXE_iris-agentic-dev");
    let mut cmd = clean_mcp_command(Path::new(bin));
    cmd.env("HOME", home)
        .env("USERPROFILE", home)
        .env("IRIS_HOST", "127.0.0.1")
        .env("IRIS_WEB_PORT", "1")
        .current_dir(cwd)
        .args([
            "--transport",
            "http",
            "--bind",
            bind,
            "--port",
            &port.to_string(),
        ])
        .args(extra);
    cmd
}

struct Server {
    child: Child,
    port: u16,
}

impl Drop for Server {
    fn drop(&mut self) {
        stop_server(&mut self.child);
    }
}

fn start(cmd: &mut Command, port: u16) -> Server {
    let child = cmd
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn the server");
    // Owned from here on, so a panic below still stops it.
    let mut server = Server { child, port };
    let deadline = Instant::now() + Duration::from_secs(15);
    while Instant::now() < deadline {
        if let Some(status) = server.child.try_wait().unwrap() {
            panic!("the server exited early with {status}");
        }
        if TcpStream::connect(("127.0.0.1", port)).is_ok() {
            return server;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    panic!("the server did not listen on {port} within 15 s");
}

/// POST `initialize` and return the status code and the response head.
fn post(port: u16, headers: &[(&str, &str)]) -> (u16, String) {
    let mut req = format!(
        "POST /mcp HTTP/1.1\r\nContent-Type: application/json\r\n\
         Accept: application/json, text/event-stream\r\nContent-Length: {}\r\n\
         Connection: close\r\n",
        INIT.len()
    );
    for (k, v) in headers {
        req.push_str(&format!("{k}: {v}\r\n"));
    }
    req.push_str("\r\n");
    req.push_str(INIT);
    let mut stream = TcpStream::connect(("127.0.0.1", port)).expect("connect");
    stream
        .set_read_timeout(Some(Duration::from_secs(10)))
        .unwrap();
    stream.write_all(req.as_bytes()).unwrap();
    let mut head = String::new();
    let mut buf = [0u8; 2048];
    while !head.contains("\r\n\r\n") {
        match stream.read(&mut buf) {
            Ok(0) | Err(_) => break,
            Ok(n) => head.push_str(&String::from_utf8_lossy(&buf[..n])),
        }
    }
    let status = head
        .split_whitespace()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or_else(|| panic!("no status line in {head:?}"));
    (status, head)
}

fn scratch() -> tempfile::TempDir {
    tempfile::tempdir().unwrap()
}

fn bearer() -> String {
    format!("Bearer {TOKEN}")
}

#[test]
fn loopback_with_a_token_gives_401_then_200_and_refuses_a_foreign_host() {
    let dir = scratch();
    let port = free_port();
    let mut cmd = command(dir.path(), dir.path(), "127.0.0.1", port, &[]);
    cmd.env("IAD_HTTP_TOKEN", TOKEN);
    let s = start(&mut cmd, port);
    let host = format!("127.0.0.1:{port}");

    let (code, head) = post(s.port, &[("Host", &host)]);
    assert_eq!(code, 401, "{head}");
    assert!(
        head.to_lowercase()
            .contains("www-authenticate: bearer realm=\"iris-agentic-dev\""),
        "{head}"
    );

    let (code, head) = post(s.port, &[("Host", &host), ("Authorization", "Bearer nope")]);
    assert_eq!(code, 401, "{head}");

    let (code, head) = post(s.port, &[("Host", &host), ("Authorization", &bearer())]);
    assert_eq!(code, 200, "{head}");

    let (code, head) = post(
        s.port,
        &[("Host", "evil.example"), ("Authorization", &bearer())],
    );
    assert_eq!(code, 403, "{head}");
}

#[test]
fn loopback_without_a_token_still_serves_and_still_checks_host() {
    let dir = scratch();
    let port = free_port();
    let mut cmd = command(dir.path(), dir.path(), "127.0.0.1", port, &[]);
    let s = start(&mut cmd, port);
    let host = format!("127.0.0.1:{port}");
    assert_eq!(post(s.port, &[("Host", &host)]).0, 200);
    assert_eq!(post(s.port, &[("Host", "evil.example")]).0, 403);
    assert_eq!(
        post(
            s.port,
            &[("Host", &host), ("Origin", "https://evil.example")]
        )
        .0,
        403
    );
}

#[test]
fn all_interfaces_without_a_token_exits_2_and_says_how_to_set_one() {
    let dir = scratch();
    let out = command(dir.path(), dir.path(), "0.0.0.0", free_port(), &[])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2), "{out:?}");
    let err = String::from_utf8_lossy(&out.stderr);
    for needle in ["IAD_HTTP_TOKEN", "--auth-token-file", "http_token"] {
        assert!(err.contains(needle), "stderr lacks {needle}: {err}");
    }
}

#[test]
fn a_token_that_is_too_short_exits_2() {
    let dir = scratch();
    let mut cmd = command(dir.path(), dir.path(), "127.0.0.1", free_port(), &[]);
    let out = cmd.env("IAD_HTTP_TOKEN", "short").output().unwrap();
    assert_eq!(out.status.code(), Some(2), "{out:?}");
    assert!(String::from_utf8_lossy(&out.stderr).contains("openssl rand -hex 32"));
}

#[test]
fn a_token_in_a_workspace_file_does_not_let_a_public_bind_start() {
    let home = scratch();
    let ws = scratch();
    std::fs::write(
        ws.path().join(".iris-agentic-dev.toml"),
        format!("[transport]\nhttp_token = \"{TOKEN}\"\n"),
    )
    .unwrap();
    let out = command(home.path(), ws.path(), "0.0.0.0", free_port(), &[])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2), "{out:?}");
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("set aside"), "{err}");
    assert!(err.contains("check_config"), "{err}");
    assert!(!err.contains(TOKEN), "the token reached stderr: {err}");
}

#[test]
fn all_interfaces_with_a_token_serves_docker_hosts_and_refuses_a_foreign_origin() {
    let dir = scratch();
    let port = free_port();
    let mut cmd = command(dir.path(), dir.path(), "0.0.0.0", port, &[]);
    cmd.env("IAD_HTTP_TOKEN", TOKEN);
    let s = start(&mut cmd, port);
    let auth = bearer();

    let (code, head) = post(
        s.port,
        &[("Host", "host.docker.internal"), ("Authorization", &auth)],
    );
    assert_eq!(code, 200, "{head}");

    let (code, head) = post(
        s.port,
        &[
            ("Host", "host.docker.internal"),
            ("Authorization", &auth),
            ("Origin", "https://evil.example"),
        ],
    );
    assert_eq!(code, 403, "{head}");

    let (code, head) = post(s.port, &[("Host", "host.docker.internal")]);
    assert_eq!(code, 401, "{head}");
}

#[test]
fn the_token_can_come_from_a_file_and_an_allowed_host_flag_widens_the_host_rule() {
    let dir = scratch();
    let file = dir.path().join("token");
    std::fs::write(&file, format!("{TOKEN}\n")).unwrap();
    let port = free_port();
    let mut cmd = command(
        dir.path(),
        dir.path(),
        "0.0.0.0",
        port,
        &[
            "--auth-token-file",
            file.to_str().unwrap(),
            "--allowed-host",
            "iad.internal",
        ],
    );
    let s = start(&mut cmd, port);
    let auth = bearer();
    assert_eq!(
        post(
            s.port,
            &[("Host", "iad.internal:9"), ("Authorization", &auth)]
        )
        .0,
        200
    );
    assert_eq!(
        post(
            s.port,
            &[("Host", "other.internal"), ("Authorization", &auth)]
        )
        .0,
        403
    );
}

#[test]
fn the_token_can_come_from_the_operator_file() {
    let home = scratch();
    std::fs::write(
        home.path().join(".iris-agentic-dev.toml"),
        format!("[transport]\nhttp_token = \"{TOKEN}\"\n"),
    )
    .unwrap();
    let ws = scratch();
    let port = free_port();
    let mut cmd = command(home.path(), ws.path(), "127.0.0.1", port, &[]);
    let s = start(&mut cmd, port);
    let host = format!("127.0.0.1:{port}");
    assert_eq!(post(s.port, &[("Host", &host)]).0, 401);
    assert_eq!(
        post(s.port, &[("Host", &host), ("Authorization", &bearer())]).0,
        200
    );
}

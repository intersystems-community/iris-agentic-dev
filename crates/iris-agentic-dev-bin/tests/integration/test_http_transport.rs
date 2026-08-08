/// T-077-01: --transport invalid-value exits with code 1
///
/// This test requires no running IRIS — it validates the CLI argument
/// validation path and runs without --include-ignored.
#[test]
fn test_invalid_transport_exits_one() {
    let bin = env!("CARGO_BIN_EXE_iris-agentic-dev");
    let out = std::process::Command::new(bin)
        .args(["mcp", "--transport", "grpc"])
        .output()
        .expect("failed to run iris-agentic-dev");
    assert_eq!(
        out.status.code(),
        Some(1),
        "expected exit code 1 for unsupported transport"
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("grpc") || stderr.contains("transport") || stderr.contains("stdio"),
        "expected transport error message in stderr, got: {}",
        stderr
    );
}

/// T-077-02: HTTP server binds on an ephemeral port and accepts a TCP connection
///
/// Starts `iris-agentic-dev mcp --transport http --port <port>` and verifies
/// that the server starts listening (startup log line) and TCP connect succeeds.
#[test]
#[ignore]
fn test_http_transport_binds_and_accepts() {
    use std::time::Duration;

    let bin = env!("CARGO_BIN_EXE_iris-agentic-dev");
    let port = free_port();

    let mut child = std::process::Command::new(bin)
        .args(["mcp", "--transport", "http", "--port", &port.to_string()])
        .env("IRIS_HOST", "localhost")
        .env("IRIS_WEB_PORT", "52780")
        .env("IRIS_USERNAME", "_SYSTEM")
        .env("IRIS_PASSWORD", "SYS")
        .stderr(std::process::Stdio::piped())
        .stdout(std::process::Stdio::null())
        .spawn()
        .expect("failed to spawn iris-agentic-dev");

    // Give the server up to 5 seconds to start
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    let addr = format!("127.0.0.1:{}", port);
    let mut connected = false;
    while std::time::Instant::now() < deadline {
        if std::net::TcpStream::connect(&addr).is_ok() {
            connected = true;
            break;
        }
        std::thread::sleep(Duration::from_millis(100));
    }

    child.kill().ok();
    let _ = child.wait();

    assert!(
        connected,
        "HTTP server did not bind on {}:{} within 5s",
        "127.0.0.1", port
    );
}

/// T-077-03: HTTP /mcp endpoint responds 200 to an MCP initialize POST
///
/// Sends a minimal MCP initialize request and verifies the response is a
/// valid JSON object containing the server capabilities.
#[test]
#[ignore]
fn test_http_transport_initialize() {
    use std::io::{Read, Write};
    use std::net::TcpStream;
    use std::time::Duration;

    let bin = env!("CARGO_BIN_EXE_iris-agentic-dev");
    let port = free_port();

    let mut child = std::process::Command::new(bin)
        .args(["mcp", "--transport", "http", "--port", &port.to_string()])
        .env("IRIS_HOST", "localhost")
        .env("IRIS_WEB_PORT", "52780")
        .env("IRIS_USERNAME", "_SYSTEM")
        .env("IRIS_PASSWORD", "SYS")
        .stderr(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .spawn()
        .expect("failed to spawn iris-agentic-dev");

    wait_for_port(port, Duration::from_secs(5));

    // Send a minimal MCP initialize JSON-RPC request over HTTP
    let body = r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-03-26","capabilities":{},"clientInfo":{"name":"test","version":"0.0.1"}}}"#;
    let request = format!(
        "POST /mcp HTTP/1.1\r\nHost: 127.0.0.1:{}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nAccept: application/json, text/event-stream\r\n\r\n{}",
        port,
        body.len(),
        body
    );

    let mut stream = TcpStream::connect(format!("127.0.0.1:{}", port)).expect("TCP connect failed");
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    stream.write_all(request.as_bytes()).unwrap();

    let mut response = String::new();
    let mut buf = [0u8; 4096];
    loop {
        match stream.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => response.push_str(&String::from_utf8_lossy(&buf[..n])),
            Err(_) => break,
        }
        if response.contains("\r\n\r\n") && response.contains("serverInfo") {
            break;
        }
    }

    child.kill().ok();
    let _ = child.wait();

    assert!(
        response.starts_with("HTTP/1.1 200") || response.starts_with("HTTP/1.0 200"),
        "expected HTTP 200 response, got: {}",
        &response[..response.len().min(400)]
    );
    assert!(
        response.contains("serverInfo") || response.contains("capabilities"),
        "expected MCP initialize response, got: {}",
        &response[..response.len().min(800)]
    );
}

/// T-077-04: --bind flag restricts the listen address
///
/// Starts the server with --bind 127.0.0.2 (loopback alias) and verifies
/// that 127.0.0.1:<port> is refused while 127.0.0.2:<port> accepts.
/// This test is skipped if 127.0.0.2 is not a local interface.
#[test]
#[ignore]
fn test_http_transport_bind_flag() {
    use std::net::TcpStream;
    use std::time::Duration;

    // Skip if 127.0.0.2 is not a local loopback alias.
    // On macOS the default loopback only has 127.0.0.1. On Linux, all of
    // 127.0.0.0/8 is usually routable. Detect by trying to bind on 127.0.0.2:0.
    if std::net::TcpListener::bind("127.0.0.2:0").is_err() {
        eprintln!("Skipping test_http_transport_bind_flag: 127.0.0.2 is not a local interface");
        return;
    }

    let bin = env!("CARGO_BIN_EXE_iris-agentic-dev");
    let port = free_port();

    let mut child = std::process::Command::new(bin)
        .args([
            "mcp",
            "--transport",
            "http",
            "--port",
            &port.to_string(),
            "--bind",
            "127.0.0.2",
        ])
        .env("IRIS_HOST", "localhost")
        .env("IRIS_WEB_PORT", "52780")
        .stderr(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .spawn()
        .expect("failed to spawn iris-agentic-dev");

    wait_for_port_on("127.0.0.2", port, Duration::from_secs(5));

    // 127.0.0.2 should accept
    let bound_ok = TcpStream::connect(format!("127.0.0.2:{}", port)).is_ok();
    // 127.0.0.1 should refuse
    let default_refused = TcpStream::connect(format!("127.0.0.1:{}", port)).is_err();

    child.kill().ok();
    let _ = child.wait();

    assert!(bound_ok, "expected server to accept on 127.0.0.2:{}", port);
    assert!(
        default_refused,
        "expected server to refuse on 127.0.0.1:{} when bound to 127.0.0.2",
        port
    );
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn free_port() -> u16 {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    listener.local_addr().unwrap().port()
}

fn wait_for_port(port: u16, timeout: std::time::Duration) {
    wait_for_port_on("127.0.0.1", port, timeout);
}

fn wait_for_port_on(host: &str, port: u16, timeout: std::time::Duration) {
    let deadline = std::time::Instant::now() + timeout;
    let addr = format!("{}:{}", host, port);
    while std::time::Instant::now() < deadline {
        if std::net::TcpStream::connect(&addr).is_ok() {
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    panic!(
        "{}:{} did not start listening within {:?}",
        host, port, timeout
    );
}

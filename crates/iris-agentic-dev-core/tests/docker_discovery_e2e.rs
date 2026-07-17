// E2E regression harness for Docker discovery error messages.
// Community tests: run without --ignored (no license key needed).
// Enterprise tests: #[ignore] — requires IRIS_LICENSE_KEY_PATH env var.
//
// Run community: cargo test --test docker_discovery_e2e
// Run enterprise: IRIS_LICENSE_KEY_PATH=~/license/iris.key cargo test --test docker_discovery_e2e -- --ignored

#![allow(dead_code)]

use std::io::{BufRead, BufReader, Write};
use std::process::{Command, Stdio};

// ── Infrastructure ────────────────────────────────────────────────────────────

/// Locates the built `iris-agentic-dev` binary (the `[[bin]] name` in
/// `iris-agentic-dev-bin`'s Cargo.toml — NOT `iris-dev`, a stale name from before a
/// crate rename). Checks both `target/{debug,release}/` (plain `cargo build`/`cargo
/// test`) and `target/llvm-cov-target/{debug,release}/` (`cargo llvm-cov`, which
/// builds into a separate target dir) since this test is exercised by both.
fn iris_dev_bin() -> std::path::PathBuf {
    let workspace_root = {
        let mut p = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        p.pop();
        p.pop();
        p
    };
    for target_subdir in [
        "target/debug/iris-agentic-dev",
        "target/release/iris-agentic-dev",
        "target/llvm-cov-target/debug/iris-agentic-dev",
        "target/llvm-cov-target/release/iris-agentic-dev",
    ] {
        let candidate = workspace_root.join(target_subdir);
        if candidate.exists() {
            return candidate;
        }
    }
    workspace_root.join("target/debug/iris-agentic-dev")
}

// Skip the test if the iris-dev binary hasn't been built yet.
// Run `cargo build` first to enable these E2E tests.

/// Spawn iris-dev mcp subprocess with IRIS_CONTAINER set, capture stderr output.
/// Sends the MCP initialize handshake, waits up to 5 seconds, then kills.
fn run_iris_dev_mcp_capture_stderr(container_name: &str, extra_env: &[(&str, &str)]) -> String {
    let bin = iris_dev_bin();
    let mut cmd = Command::new(&bin);
    cmd.args(["mcp"])
        .env("IRIS_CONTAINER", container_name)
        .env("IRIS_USERNAME", "test")
        .env("IRIS_PASSWORD", "test")
        .env("IRIS_TOOLSET", "baseline")
        .env("RUST_LOG", "warn")
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped());

    // Remove env vars that would cause discovery to succeed via other paths
    cmd.env_remove("IRIS_HOST").env_remove("IRIS_WEB_PORT");

    for (k, v) in extra_env {
        cmd.env(k, v);
    }

    let mut child = cmd.spawn().expect("failed to spawn iris-dev mcp");
    let mut stdin = child.stdin.take().unwrap();
    let stderr = child.stderr.take().unwrap();

    // Send MCP initialize to trigger discovery
    let init = r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"e2e","version":"0.1"}}}"#;
    let _ = stdin.write_all((init.to_string() + "\n").as_bytes());
    let _ = stdin.flush();
    drop(stdin); // close stdin so child knows we're done writing

    // Read stderr in a thread with a 5-second timeout
    let (tx, rx) = std::sync::mpsc::channel::<String>();
    std::thread::spawn(move || {
        let reader = BufReader::new(stderr);
        let mut output = String::new();
        for line in reader.lines() {
            match line {
                Ok(l) => {
                    output.push_str(&l);
                    output.push('\n');
                }
                Err(_) => break,
            }
        }
        let _ = tx.send(output);
    });

    let output = rx
        .recv_timeout(std::time::Duration::from_secs(5))
        .unwrap_or_default();

    let _ = child.kill();
    let _ = child.wait();
    output
}

/// Start a fresh Docker container and return its name.
/// The container is removed on drop via a cleanup handle.
struct ContainerHandle {
    name: String,
}

impl Drop for ContainerHandle {
    fn drop(&mut self) {
        let _ = Command::new("docker")
            .args(["rm", "-f", &self.name])
            .output();
    }
}

fn start_fresh_container(
    image: &str,
    name: &str,
    web_port: Option<u16>,
    license_key: Option<&str>,
) -> ContainerHandle {
    // Remove any existing container with this name
    let _ = Command::new("docker").args(["rm", "-f", name]).output();

    let mut cmd = Command::new("docker");
    cmd.arg("run").arg("-d").arg("--name").arg(name);

    if let Some(port) = web_port {
        cmd.args(["-p", &format!("{}:52773", port)]);
    }

    if let Some(key) = license_key {
        cmd.args(["-v", &format!("{}:/usr/irissys/mgr/iris.key:ro", key)]);
    }

    cmd.args(["-e", "IRIS_PASSWORD=SYS"]);
    cmd.arg(image);
    cmd.args(["--check-caps", "false"]);

    let output = cmd.output().expect("docker run failed");
    if !output.status.success() {
        panic!(
            "Failed to start container {}: {}",
            name,
            String::from_utf8_lossy(&output.stderr)
        );
    }

    // Wait for IRIS to start
    std::thread::sleep(std::time::Duration::from_secs(25));

    // Create a test user via docker exec (bypass OS-auth-only default)
    let _ = Command::new("docker")
        .args([
            "exec",
            name,
            "iris",
            "session",
            "iris",
            "-U",
            "%SYS",
            "##class(Security.Users).Create(\"test\",\"%ALL\",\"test\")",
        ])
        .output();

    ContainerHandle {
        name: name.to_string(),
    }
}

// ── Phase 3/US1: Container not found ─────────────────────────────────────────

/// T017: IRIS_CONTAINER pointing to a nonexistent container — "not found in Docker"
#[test]
#[ignore = "requires `cargo build` to produce target/debug/iris-dev binary"]
fn test_container_not_found_message() {
    if std::env::var("IRIS_DOCKER_E2E_ENABLED").is_err() {
        return;
    }
    let stderr = run_iris_dev_mcp_capture_stderr("definitely-not-running-container-xyz", &[]);
    println!("stderr: {}", stderr);
    assert!(
        stderr.contains("not found in Docker"),
        "expected 'not found in Docker' in stderr, got:\n{}",
        stderr
    );
    assert!(
        !stderr.contains("not reachable via Docker"),
        "old generic message must not appear, got:\n{}",
        stderr
    );
}

// ── Phase 4/US3: Port not mapped ─────────────────────────────────────────────

/// T024: Container running but port 52773 NOT mapped — "port 52773 is not mapped"
#[test]
#[ignore = "requires `cargo build` + Docker with IRIS community image"]
fn test_port_not_mapped_message() {
    if std::env::var("IRIS_DOCKER_E2E_ENABLED").is_err() {
        return;
    }
    let _container = start_fresh_container(
        "containers.intersystems.com/intersystems/iris-community:2026.1",
        "e2e-nomapped",
        None, // no port mapping
        None,
    );

    let stderr = run_iris_dev_mcp_capture_stderr("e2e-nomapped", &[]);
    println!("stderr: {}", stderr);

    assert!(
        stderr.contains("port 52773 is not mapped"),
        "expected 'port 52773 is not mapped' in stderr, got:\n{}",
        stderr
    );
    assert!(
        stderr.contains("iris_execute") || stderr.contains("docker exec"),
        "expected docker exec note in stderr, got:\n{}",
        stderr
    );
    assert!(
        !stderr.contains("not reachable via Docker"),
        "old generic message must not appear"
    );
}

// ── Phase 6/US4: 401 dedup ────────────────────────────────────────────────────

/// T040: Community container without IRIS_PASSWORD — exactly one 401 warn
#[test]
#[ignore = "requires `cargo build` + Docker with IRIS community image"]
fn test_auth_401_single_warn() {
    // Start community container without IRIS_PASSWORD so _SYSTEM gets OS auth only
    let _ = Command::new("docker")
        .args(["rm", "-f", "e2e-nopassword"])
        .output();
    let mut cmd = Command::new("docker");
    cmd.args([
        "run",
        "-d",
        "--name",
        "e2e-nopassword",
        "-p",
        "52796:52773",
        "containers.intersystems.com/intersystems/iris-community:2026.1",
        "--check-caps",
        "false",
    ]);
    let _ = cmd.output();
    std::thread::sleep(std::time::Duration::from_secs(25));
    let _cleanup = ContainerHandle {
        name: "e2e-nopassword".to_string(),
    };

    let stderr = run_iris_dev_mcp_capture_stderr("e2e-nopassword", &[]);
    println!("stderr: {}", stderr);

    // Count lines containing "401"
    let warn_401_count = stderr.lines().filter(|l| l.contains("401")).count();
    assert!(
        warn_401_count <= 1,
        "expected at most 1 line mentioning 401, got {}:\n{}",
        warn_401_count,
        stderr
    );
    if warn_401_count == 1 {
        assert!(
            !stderr.contains("not found or not reachable"),
            "old generic second warn must not appear after 401"
        );
    }
}

// ── Phase 5/US2: Web server absent (enterprise) ───────────────────────────────

/// T032: Enterprise iris:2026.1 — "Atelier REST API is not responding" + enterprise hint
#[test]
#[ignore = "requires live enterprise container (IRIS_LICENSE_KEY_PATH env var)"]
fn test_enterprise_web_server_absent_message() {
    let key = match std::env::var("IRIS_LICENSE_KEY_PATH") {
        Ok(k) => k,
        Err(_) => return,
    };

    let _container = start_fresh_container(
        "containers.intersystems.com/intersystems/iris:2026.1",
        "e2e-enterprise",
        Some(52797),
        Some(&key),
    );

    let stderr = run_iris_dev_mcp_capture_stderr("e2e-enterprise", &[]);
    println!("stderr: {}", stderr);

    assert!(
        stderr.contains("Atelier REST API is not responding"),
        "expected 'Atelier REST API is not responding' in stderr, got:\n{}",
        stderr
    );
    assert!(
        stderr.contains("iris-community")
            || stderr.contains("Web Gateway")
            || stderr.contains("irishealth-community"),
        "expected enterprise hint text in stderr, got:\n{}",
        stderr
    );
    assert!(
        !stderr.contains("WebServer=1"),
        "must NOT suggest WebServer=1 CPF (crashes enterprise), got:\n{}",
        stderr
    );
}

// ── Phase 7/US5: Full regression harness ─────────────────────────────────────

/// T047: Community regression — iris-community:2026.1 and irishealth-community:2026.1
#[test]
#[ignore = "requires `cargo build` + Docker with IRIS community image"]
fn test_all_community_images() {
    if std::env::var("IRIS_DOCKER_E2E_ENABLED").is_err() {
        return;
    }
    // iris-community: port not mapped → port-not-mapped message
    let _c1 = start_fresh_container(
        "containers.intersystems.com/intersystems/iris-community:2026.1",
        "e2e-reg-community",
        None,
        None,
    );
    let stderr1 = run_iris_dev_mcp_capture_stderr("e2e-reg-community", &[]);
    assert!(
        stderr1.contains("port 52773 is not mapped"),
        "iris-community without port mapping: expected port-not-mapped message, got:\n{}",
        stderr1
    );

    // irishealth-community: port not mapped → same message
    let _c2 = start_fresh_container(
        "containers.intersystems.com/intersystems/irishealth-community:2026.1",
        "e2e-reg-irishealth-community",
        None,
        None,
    );
    let stderr2 = run_iris_dev_mcp_capture_stderr("e2e-reg-irishealth-community", &[]);
    assert!(
        stderr2.contains("port 52773 is not mapped"),
        "irishealth-community without port mapping: expected port-not-mapped message, got:\n{}",
        stderr2
    );
}

/// T048: Enterprise regression — iris:2026.1 and irishealth:2026.1
#[test]
#[ignore = "requires live enterprise containers (IRIS_LICENSE_KEY_PATH env var)"]
fn test_all_enterprise_images() {
    let key = match std::env::var("IRIS_LICENSE_KEY_PATH") {
        Ok(k) => k,
        Err(_) => return,
    };

    // iris enterprise: web server absent → Atelier not responding
    let _c1 = start_fresh_container(
        "containers.intersystems.com/intersystems/iris:2026.1",
        "e2e-reg-enterprise",
        Some(52798),
        Some(&key),
    );
    let stderr1 = run_iris_dev_mcp_capture_stderr("e2e-reg-enterprise", &[]);
    assert!(
        stderr1.contains("Atelier REST API is not responding"),
        "iris enterprise: expected Atelier-not-responding message, got:\n{}",
        stderr1
    );

    // irishealth enterprise: same
    let _c2 = start_fresh_container(
        "containers.intersystems.com/intersystems/irishealth:2026.1",
        "e2e-reg-irishealth-enterprise",
        Some(52799),
        Some(&key),
    );
    let stderr2 = run_iris_dev_mcp_capture_stderr("e2e-reg-irishealth-enterprise", &[]);
    assert!(
        stderr2.contains("Atelier REST API is not responding"),
        "irishealth enterprise: expected Atelier-not-responding message, got:\n{}",
        stderr2
    );
}

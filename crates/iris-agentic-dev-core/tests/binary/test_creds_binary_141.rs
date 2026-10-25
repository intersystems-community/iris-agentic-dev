//! Spec 141 FR-021 to FR-024 at the process level (T-R4b-B01, B03, B04, B05).
//!
//! Each test spawns the shipped `iris-agentic-dev` and aims it at listeners the test owns: a plain
//! `TcpListener` that records each request head and answers with a fixed reply, or the same behind
//! a rustls handshake. A listener posing as something else is the point of these tests, so it is
//! not a mocked IRIS. No IRIS is needed.
//!
//! Run: `IAD_BINARY=$PWD/target/debug/iris-agentic-dev cargo test --features testing --test binary
//! creds_binary_141 -- --ignored --test-threads=1`

use base64::Engine;
use iris_agentic_dev_core::testing::{answer_text, require_iad_binary, McpSession};
use serde_json::{json, Value};
use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

const SENTINEL: &str = "Zq9-SENTINEL-pw";

// ── Listeners ───────────────────────────────────────────────────────────────────────────────

type TlsConfig = Arc<tokio_rustls::rustls::ServerConfig>;

/// A listener that records every request head it can read and answers `reply(path)`.
struct Recorder {
    port: u16,
    heads: Arc<Mutex<Vec<String>>>,
    stop: Arc<AtomicBool>,
}

impl Recorder {
    fn plain(reply: impl Fn(&str) -> String + Send + Sync + 'static) -> Recorder {
        Self::start(None, reply)
    }

    fn tls(config: TlsConfig, reply: impl Fn(&str) -> String + Send + Sync + 'static) -> Recorder {
        Self::start(Some(config), reply)
    }

    fn start(
        tls: Option<TlsConfig>,
        reply: impl Fn(&str) -> String + Send + Sync + 'static,
    ) -> Recorder {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let port = listener.local_addr().unwrap().port();
        listener.set_nonblocking(true).unwrap();
        let heads = Arc::new(Mutex::new(Vec::new()));
        let stop = Arc::new(AtomicBool::new(false));
        let reply: Arc<dyn Fn(&str) -> String + Send + Sync> = Arc::new(reply);
        let (h, s) = (heads.clone(), stop.clone());
        std::thread::spawn(move || {
            while !s.load(Ordering::Relaxed) {
                match listener.accept() {
                    Ok((sock, _)) => {
                        let (h, reply, tls) = (h.clone(), reply.clone(), tls.clone());
                        std::thread::spawn(move || serve(sock, tls, &h, reply.as_ref()));
                    }
                    Err(_) => std::thread::sleep(std::time::Duration::from_millis(10)),
                }
            }
        });
        Recorder { port, heads, stop }
    }

    fn heads(&self) -> Vec<String> {
        self.heads.lock().unwrap().clone()
    }
}

impl Drop for Recorder {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

fn serve(
    sock: std::net::TcpStream,
    tls: Option<TlsConfig>,
    heads: &Mutex<Vec<String>>,
    reply: &dyn Fn(&str) -> String,
) {
    sock.set_nonblocking(false).ok();
    sock.set_read_timeout(Some(std::time::Duration::from_secs(5)))
        .ok();
    match tls {
        None => exchange(sock, heads, reply),
        Some(config) => {
            let Ok(conn) = tokio_rustls::rustls::ServerConnection::new(config) else {
                return;
            };
            exchange(
                tokio_rustls::rustls::StreamOwned::new(conn, sock),
                heads,
                reply,
            )
        }
    }
}

/// Read request heads off one connection until it closes, answering each. A failed TLS handshake
/// surfaces as a read error before any head, so it records nothing.
fn exchange(
    mut stream: impl Read + Write,
    heads: &Mutex<Vec<String>>,
    reply: &dyn Fn(&str) -> String,
) {
    let mut buf = Vec::new();
    let mut chunk = [0u8; 4096];
    loop {
        let Ok(n) = stream.read(&mut chunk) else {
            return;
        };
        if n == 0 {
            return;
        }
        buf.extend_from_slice(&chunk[..n]);
        while let Some(end) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
            let head = String::from_utf8_lossy(&buf[..end]).to_string();
            let path = head
                .lines()
                .next()
                .and_then(|l| l.split_whitespace().nth(1))
                .unwrap_or("/")
                .to_string();
            // A body after the head is not parsed: these requests carry JSON the test never reads.
            buf.drain(..end + 4);
            heads.lock().unwrap().push(head);
            if stream.write_all(reply(&path).as_bytes()).is_err() {
                return;
            }
            let _ = stream.flush();
        }
    }
}

fn response(status: &str, headers: &[&str], body: &str) -> String {
    let mut r = format!(
        "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n",
        body.len()
    );
    for h in headers {
        r.push_str(h);
        r.push_str("\r\n");
    }
    r.push_str("\r\n");
    r.push_str(body);
    r
}

fn basic_401(_path: &str) -> String {
    response("401 Unauthorized", &["WWW-Authenticate: Basic"], "")
}

fn not_found(_: &str) -> String {
    response("404 Not Found", &["Content-Type: application/json"], "{}")
}

fn has_authorization(head: &str) -> bool {
    head.lines()
        .any(|l| l.to_ascii_lowercase().starts_with("authorization:"))
}

/// The `user:password` text of a request head's Basic credentials.
fn basic_credentials(head: &str) -> Option<String> {
    let line = head
        .lines()
        .find(|l| l.to_ascii_lowercase().starts_with("authorization:"))?;
    let encoded = line.split_once(' ')?.1.trim().strip_prefix("Basic ")?;
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(encoded)
        .ok()?;
    String::from_utf8(bytes).ok()
}

fn text_body(answer: &Value) -> Value {
    let text = answer["result"]["content"][0]["text"]
        .as_str()
        .unwrap_or_else(|| panic!("no text content: {}", answer_text(answer)));
    serde_json::from_str(text).unwrap_or_else(|e| panic!("not JSON ({e}): {text}"))
}

fn env(pairs: &[(&str, &str)]) -> Vec<(String, String)> {
    pairs
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect()
}

fn home_env(home: &Path, more: &[(&str, &str)]) -> Vec<(String, String)> {
    let mut v = env(&[
        ("HOME", home.to_str().unwrap()),
        ("USERPROFILE", home.to_str().unwrap()),
    ]);
    v.extend(env(more));
    v
}

// ── T-R4b-B01: discovery sends nothing before IRIS identifies itself ──────────────────────────

#[test]
#[ignore = "spawns the built binary; run with --include-ignored"]
fn discovery_sends_no_credential_to_a_listener_that_is_not_iris() {
    let Some(_bin) = require_iad_binary() else {
        return;
    };

    // One answers 401 Basic everywhere, as any web server does. One answers 200 on every path with
    // a page that only mentions IRIS: no Atelier JSON, no session cookie, no metrics.
    let plain_401 = Recorder::plain(basic_401);
    let mentions_iris = Recorder::plain(|_| {
        response(
            "200 OK",
            &["Content-Type: text/html"],
            "<html>InterSystems IRIS Management Portal (not really)</html>",
        )
    });
    // A third is IRIS by its Portal login page: the cookie IRIS sets on `UtilHome.csp`.
    let portal = Recorder::plain(|path| {
        if path.starts_with("/csp/sys/UtilHome.csp") {
            response(
                "200 OK",
                &[
                    "Content-Type: text/html",
                    "Set-Cookie: CSPSESSIONID-SP-52773-UP-csp-sys-=abc; path=/csp/sys/",
                ],
                "<html>login</html>",
            )
        } else {
            basic_401(path)
        }
    });

    let ports = format!("{},{},{}", plain_401.port, mentions_iris.port, portal.port);
    // An empty directory to work in: the repository's own project file would choose the connection.
    let cwd = tempfile::tempdir().unwrap();
    let mut s = McpSession::start_in(
        Some(cwd.path()),
        &env(&[
            ("IAD_TEST_SCAN_PORTS", &ports),
            ("IRIS_USERNAME", "_SYSTEM"),
            ("IRIS_PASSWORD", SENTINEL),
            // No Docker: the scan after the localhost one must not find a real container.
            ("DOCKER_HOST", "unix:///nonexistent-iad-141.sock"),
        ]),
    );
    let body = text_body(&s.call("check_config", &json!({})));

    for (label, rec) in [("401 Basic", &plain_401), ("mentions IRIS", &mentions_iris)] {
        let heads = rec.heads();
        assert!(!heads.is_empty(), "the {label} listener was never visited");
        assert!(
            heads.iter().all(|h| !has_authorization(h)),
            "the {label} listener received a credential: {heads:?}"
        );
    }

    // The Portal listener identified itself, so it got exactly one credentialed request, after the
    // requests that identified it.
    let heads = portal.heads();
    let first_auth = heads
        .iter()
        .position(|h| has_authorization(h))
        .unwrap_or_else(|| {
            panic!("an identified listener should get the credentialed probe: {heads:?}")
        });
    assert_eq!(
        heads.iter().filter(|h| has_authorization(h)).count(),
        1,
        "{heads:?}"
    );
    assert!(
        heads[..first_auth]
            .iter()
            .any(|h| h.starts_with("GET /csp/sys/UtilHome.csp")),
        "identification came first: {heads:?}"
    );

    let skipped: Vec<u64> = body["discovery"]["skipped"]
        .as_array()
        .unwrap_or_else(|| panic!("check_config.discovery.skipped missing: {body}"))
        .iter()
        .map(|s| s["port"].as_u64().unwrap())
        .collect();
    assert!(
        skipped.contains(&(mentions_iris.port as u64)),
        "{skipped:?}"
    );
    assert!(
        !skipped.contains(&(portal.port as u64)),
        "an identified listener is not skipped: {skipped:?}"
    );
}

// ── T-R4b-B03: certificate checks are decided per connection ──────────────────────────────────

/// A self-signed certificate for 127.0.0.1, made by `openssl` into `dir`. The test fails, not
/// skips, when `openssl` is missing: no key is committed to the repository.
fn self_signed_config(dir: &Path) -> TlsConfig {
    use tokio_rustls::rustls::pki_types::{pem::PemObject, CertificateDer, PrivateKeyDer};
    let (cert, key) = (dir.join("cert.pem"), dir.join("key.pem"));
    let out = std::process::Command::new("openssl")
        .args([
            "req", "-x509", "-newkey", "rsa:2048", "-nodes", "-days", "1",
        ])
        .args([
            "-subj",
            "/CN=127.0.0.1",
            "-addext",
            "subjectAltName=IP:127.0.0.1",
        ])
        .arg("-keyout")
        .arg(&key)
        .arg("-out")
        .arg(&cert)
        .output()
        .expect("openssl must be installed to make the test certificate");
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let certs: Vec<CertificateDer<'static>> =
        CertificateDer::pem_slice_iter(&std::fs::read(&cert).unwrap())
            .collect::<Result<_, _>>()
            .expect("certificate PEM");
    let key = PrivateKeyDer::from_pem_slice(&std::fs::read(&key).unwrap()).expect("key PEM");
    Arc::new(
        tokio_rustls::rustls::ServerConfig::builder()
            .with_no_client_auth()
            .with_single_cert(certs, key)
            .expect("server config"),
    )
}

fn operator_file(port: u16, extra: &str) -> String {
    format!(
        "host = \"127.0.0.1\"\nweb_port = {port}\nscheme = \"https\"\nusername = \"u\"\n\
         password = \"p\"\n{extra}\n[policy.default]\nirisAudit = true\n"
    )
}

#[test]
#[ignore = "spawns the built binary and needs openssl; run with --include-ignored"]
fn a_connection_checks_its_certificate_unless_its_own_setting_says_not() {
    let Some(_bin) = require_iad_binary() else {
        return;
    };
    let tmp = tempfile::tempdir().unwrap();
    let config = self_signed_config(tmp.path());
    // The repository's own project file would choose the connection: work in an empty directory.
    let cwd = tmp.path().join("cwd");
    std::fs::create_dir_all(&cwd).unwrap();

    // 1. The default connection validates: the probe, the audit emission and the search all fail the
    // handshake, and the listener reads no request.
    let checked = Recorder::tls(config.clone(), not_found);
    let home = tmp.path().join("home-checked");
    std::fs::create_dir_all(&home).unwrap();
    std::fs::write(
        home.join(".iris-agentic-dev.toml"),
        operator_file(checked.port, ""),
    )
    .unwrap();
    {
        let mut s = McpSession::start_in(Some(&cwd), &home_env(&home, &[]));
        s.call(
            "iris_search",
            &json!({"query": "Zq9", "documents": ["MyPkg.*.cls"]}),
        );
        s.call("check_config", &json!({}));
    }
    assert_eq!(
        checked.heads(),
        Vec::<String>::new(),
        "a request got through an untrusted certificate"
    );

    // 2. The same server with `tls_verify = false` in the operator's file: the same clients now
    // reach it, so the listener works and the setting is what kept the requests out above.
    let unchecked = Recorder::tls(config.clone(), not_found);
    let home = tmp.path().join("home-unchecked");
    std::fs::create_dir_all(&home).unwrap();
    std::fs::write(
        home.join(".iris-agentic-dev.toml"),
        operator_file(unchecked.port, "tls_verify = false"),
    )
    .unwrap();
    {
        let mut s = McpSession::start_in(Some(&cwd), &home_env(&home, &[]));
        s.call(
            "iris_search",
            &json!({"query": "Zq9", "documents": ["MyPkg.*.cls"]}),
        );
        s.call("check_config", &json!({}));
    }
    let heads = unchecked.heads();
    assert!(
        heads.iter().any(|h| h.starts_with("GET /api/atelier/")),
        "the startup probe: {heads:?}"
    );
    assert!(
        heads.iter().any(|h| h.contains("/action/search")),
        "iris_search reached the server: {heads:?}"
    );
}

#[test]
#[ignore = "spawns the built binary and needs openssl; run with --include-ignored"]
fn servers_json_may_turn_checks_off_for_its_own_entry_and_a_project_file_may_not() {
    let Some(_bin) = require_iad_binary() else {
        return;
    };
    let tmp = tempfile::tempdir().unwrap();
    let config = self_signed_config(tmp.path());

    let strict = Recorder::tls(config.clone(), basic_401);
    let lax = Recorder::tls(config.clone(), basic_401);
    let from_project = Recorder::tls(config.clone(), basic_401);

    let home = tmp.path().join("home");
    let cfg_dir = home.join(".config").join("iris-agentic-dev");
    std::fs::create_dir_all(&cfg_dir).unwrap();
    let entry = |port: u16, extra: &str| {
        format!(
            "{{\"host\":\"127.0.0.1\",\"port\":{port},\"namespace\":\"USER\",\"username\":\"u\",\
             \"scheme\":\"https\"{extra}}}"
        )
    };
    std::fs::write(
        cfg_dir.join("servers.json"),
        format!(
            "{{\"version\":1,\"servers\":{{\"strict\":{},\"lax\":{}}}}}",
            entry(strict.port, ""),
            entry(lax.port, ",\"tls_verify\":false")
        ),
    )
    .unwrap();
    // A project file in a repository asks for the same on its instance. It is not trusted.
    let proj = tmp.path().join("proj");
    std::fs::create_dir_all(&proj).unwrap();
    std::fs::write(
        proj.join(".iris-agentic-dev.toml"),
        format!(
            "mode = \"operate\"\n[instance.w]\nhost = \"127.0.0.1\"\nweb_port = {}\nscheme = \"https\"\n\
             tls_verify = false\n",
            from_project.port
        ),
    )
    .unwrap();

    let mut s = McpSession::start_in(Some(&proj), &home_env(&home, &[("IAD_NO_KEYCHAIN", "1")]));
    let servers = text_body(&s.call("iris_servers", &json!({"probe": true})));
    let entries = servers["servers"]
        .as_array()
        .unwrap_or_else(|| panic!("{servers}"));
    let find = |name: &str| {
        entries
            .iter()
            .find(|e| e["name"] == name)
            .unwrap_or_else(|| panic!("no server {name} in {servers}"))
    };

    assert_eq!(find("lax")["reachable"], true, "{servers}");
    assert_eq!(find("strict")["reachable"], false, "{servers}");
    assert_eq!(find("w")["reachable"], false, "{servers}");
    assert!(!lax.heads().is_empty());
    assert_eq!(
        strict.heads(),
        Vec::<String>::new(),
        "validated, so no request"
    );
    assert_eq!(
        from_project.heads(),
        Vec::<String>::new(),
        "an untrusted project file cannot turn the check off"
    );

    // check_config says which setting each server runs with.
    let cfg = text_body(&s.call("check_config", &json!({})));
    assert_eq!(cfg["tls"]["servers"]["lax"]["tls_verify"], false, "{cfg}");
    assert_eq!(
        cfg["tls"]["servers"]["lax"]["source"], "servers_json",
        "{cfg}"
    );
    assert_eq!(cfg["tls"]["servers"]["strict"]["tls_verify"], true, "{cfg}");
    assert_eq!(cfg["tls"]["servers"]["w"]["tls_verify"], true, "{cfg}");
}

// ── T-R4b-B04: the compile path honours the configured scheme ─────────────────────────────────

#[test]
#[ignore = "spawns the built binary and needs openssl; run with --include-ignored"]
fn the_compile_command_uses_the_configured_scheme() {
    let Some(bin) = require_iad_binary() else {
        return;
    };
    let tmp = tempfile::tempdir().unwrap();
    let config = self_signed_config(tmp.path());
    let listener = Recorder::tls(config, not_found);

    let file = tmp.path().join("MyApp").join("Foo.cls");
    std::fs::create_dir_all(file.parent().unwrap()).unwrap();
    std::fs::write(&file, "Class MyApp.Foo {}\n").unwrap();

    let mut cmd = iris_agentic_dev_core::testing::clean_command(&bin);
    let out = cmd
        .current_dir(tmp.path())
        .env("IRIS_HOST", "127.0.0.1")
        .env("IRIS_WEB_PORT", listener.port.to_string())
        .env("IRIS_SCHEME", "https")
        // The launch settings describe this server, so they may turn the check off for it.
        .env("IRIS_TLS_VERIFY", "false")
        .args(["compile", "--"])
        .arg(&file)
        .output()
        .expect("run compile");
    let heads = listener.heads();
    assert!(
        heads.iter().any(|h| h.starts_with("GET /api/atelier/")),
        "the compile command should have reached the TLS listener over https: {heads:?}\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
}

#[test]
#[ignore = "T053 (the cli-bench branch) rewrites the hook to run the binary: unignore after that merge"]
fn the_compile_hook_never_runs_curl_and_reaches_the_server_over_the_configured_scheme() {
    let Some(bin) = require_iad_binary() else {
        return;
    };
    use std::os::unix::fs::PermissionsExt;
    let tmp = tempfile::tempdir().unwrap();
    let config = self_signed_config(tmp.path());
    let listener = Recorder::tls(config, not_found);

    // A `curl` that records that it ran, and the binary under the name the hook calls.
    let shims = tmp.path().join("shims");
    std::fs::create_dir_all(&shims).unwrap();
    let ran = tmp.path().join("curl-ran");
    let curl = shims.join("curl");
    std::fs::write(
        &curl,
        format!("#!/bin/sh\ntouch {}\nexit 1\n", ran.display()),
    )
    .unwrap();
    std::fs::set_permissions(&curl, std::fs::Permissions::from_mode(0o755)).unwrap();
    std::os::unix::fs::symlink(&bin, shims.join("iris-agentic-dev")).unwrap();

    let file = tmp.path().join("MyApp").join("Foo.cls");
    std::fs::create_dir_all(file.parent().unwrap()).unwrap();
    std::fs::write(&file, "Class MyApp.Foo {}\n").unwrap();

    let hook = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../scripts/compile-hook.sh");
    let mut child = std::process::Command::new("bash")
        .arg(hook)
        // Outside every checkout, so no workspace file found by walking up outranks IRIS_*.
        .current_dir(tmp.path())
        .env(
            "PATH",
            format!(
                "{}:{}",
                shims.display(),
                std::env::var("PATH").unwrap_or_default()
            ),
        )
        .env("HOME", tmp.path())
        .env("IRIS_HOST", "127.0.0.1")
        .env("IRIS_WEB_PORT", listener.port.to_string())
        .env("IRIS_SCHEME", "https")
        .env("IRIS_TLS_VERIFY", "false")
        .env("IRIS_PASSWORD", SENTINEL)
        // `compile` is a write, judged by the gate pipeline since T053: with the write gate shut
        // the binary is refused before it sends anything, so open it to see the request's scheme.
        .env("IRIS_WRITE_TOOLS_ENABLED", "1")
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    let input = json!({"hook_event_name": "PostToolUse", "tool_input": {"file_path": file}});
    child
        .stdin
        .take()
        .unwrap()
        .write_all(input.to_string().as_bytes())
        .unwrap();
    let out = child.wait_with_output().unwrap();
    assert!(out.status.success());
    assert!(
        !ran.exists(),
        "the hook ran curl, with the password on its command line"
    );
    assert!(
        listener
            .heads()
            .iter()
            .any(|h| h.starts_with("GET /api/atelier/")),
        "the hook's binary did not reach the server over https: {:?}\nhook output: {}{}",
        listener.heads(),
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
}

// ── T-R4b-B05: re-registering a server at another endpoint keeps no old password ───────────────

fn read_servers(home: &Path) -> Value {
    let p = home.join(".config/iris-agentic-dev/servers.json");
    serde_json::from_str(&std::fs::read_to_string(p).unwrap()).unwrap()
}

#[test]
#[ignore = "spawns the built binary; run with --include-ignored"]
fn re_registering_a_server_at_a_new_endpoint_sends_the_new_host_no_old_password() {
    let Some(_bin) = require_iad_binary() else {
        return;
    };
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path().join("home");
    std::fs::create_dir_all(&home).unwrap();
    let first = Recorder::plain(basic_401);
    let second = Recorder::plain(basic_401);

    // IAD_NO_KEYCHAIN keeps the real keychain out of the test on every platform; the keychain path
    // itself is covered with an in-memory store (`test_credential_store_141`).
    let mut s = McpSession::start_in(
        Some(tmp.path()),
        &home_env(&home, &[("IAD_NO_KEYCHAIN", "1")]),
    );
    let add = |s: &mut McpSession, port: u16, password: &str| {
        text_body(&s.call(
            "iris_add_server",
            &json!({"name": "x", "host": "127.0.0.1", "port": port, "namespace": "USER",
                    "username": "_SYSTEM", "password": password}),
        ))
    };

    let r1 = add(&mut s, first.port, SENTINEL);
    assert_eq!(r1["added"], true, "{r1}");
    assert_eq!(r1["credential_store"], "file", "{r1}");
    let saved = read_servers(&home);
    assert_eq!(saved["servers"]["x"]["password"], SENTINEL, "{saved}");
    assert_eq!(
        saved["servers"]["x"]["password_endpoint"],
        format!("http://127.0.0.1:{}", first.port),
        "{saved}"
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(home.join(".config/iris-agentic-dev/servers.json"))
            .unwrap()
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(mode, 0o600, "servers.json holds a password");
    }

    // The same name, another endpoint, no password.
    let r2 = add(&mut s, second.port, "");
    assert_eq!(r2["added"], true, "{r2}");
    assert_eq!(r2["password_cleared"], true, "{r2}");
    assert_eq!(r2["credential_store"], "none", "{r2}");
    let saved = read_servers(&home);
    assert!(saved["servers"]["x"].get("password").is_none(), "{saved}");

    text_body(&s.call("iris_reload_pool", &json!({})));
    text_body(&s.call("iris_servers", &json!({"probe": true})));

    let heads = second.heads();
    assert!(!heads.is_empty(), "the new endpoint was never probed");
    for h in &heads {
        let creds = basic_credentials(h).expect("a probe sends the account");
        assert_eq!(creds, "_SYSTEM:", "the new host got a password: {creds}");
    }
    assert!(
        first.heads().iter().all(|h| !h.contains(SENTINEL)),
        "nothing went to the old host after the move"
    );
}

#[test]
#[ignore = "spawns the built binary; run with --include-ignored"]
fn a_stored_password_is_not_sent_to_a_host_the_entry_was_edited_to() {
    let Some(_bin) = require_iad_binary() else {
        return;
    };
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path().join("home");
    let cfg_dir = home.join(".config").join("iris-agentic-dev");
    std::fs::create_dir_all(&cfg_dir).unwrap();
    let elsewhere = Recorder::plain(basic_401);
    // The password was saved for another port; the entry now points at `elsewhere`.
    std::fs::write(
        cfg_dir.join("servers.json"),
        format!(
            "{{\"version\":1,\"servers\":{{\"x\":{{\"host\":\"127.0.0.1\",\"port\":{},\
             \"namespace\":\"USER\",\"username\":\"_SYSTEM\",\"password\":\"{SENTINEL}\",\
             \"password_endpoint\":\"http://127.0.0.1:1\"}}}}}}",
            elsewhere.port
        ),
    )
    .unwrap();

    let mut s = McpSession::start_in(
        Some(tmp.path()),
        &home_env(&home, &[("IAD_NO_KEYCHAIN", "1")]),
    );
    text_body(&s.call("iris_servers", &json!({"probe": true})));
    let cfg = text_body(&s.call("check_config", &json!({})));

    let heads = elsewhere.heads();
    assert!(!heads.is_empty());
    assert!(
        heads
            .iter()
            .all(|h| basic_credentials(h).as_deref() == Some("_SYSTEM:")),
        "{heads:?}"
    );
    let item = cfg["set_aside"]
        .as_array()
        .and_then(|a| a.iter().find(|i| i["id"] == "x:stored_password"))
        .unwrap_or_else(|| panic!("no x:stored_password in set_aside: {cfg}"));
    assert_eq!(item["kind"], "credential_withheld", "{item}");
    assert!(
        !item.to_string().contains(SENTINEL),
        "the item shows the secret: {item}"
    );
}

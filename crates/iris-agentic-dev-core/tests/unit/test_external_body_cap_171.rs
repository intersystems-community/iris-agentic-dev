//! S11: a body from a host that is not IRIS is read up to a cap, and the compare tools put the
//! namespace in the URL encoded.
//!
//! The listeners here send an endless chunked body (no length to refuse up front), so what stops
//! the read is the cap. They stand in for the public hosts those readers talk to (an LLM API,
//! GitHub, the docs site); nothing about IRIS is mocked.

use iris_agentic_dev_core::iris::connection::{DiscoverySource, IrisConnection};
use iris_agentic_dev_core::iris::http::{external_json, external_text, MAX_EXTERNAL_BODY_BYTES};
use iris_agentic_dev_core::testing::EnvGuard;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::{Arc, Mutex};

/// A server that answers every request with a 200 and an endless chunked body of `filler`.
/// Returns its base URL. The write loop ends when the client closes the connection.
fn endless(filler: &'static str) -> String {
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = l.local_addr().unwrap().port();
    std::thread::spawn(move || {
        for stream in l.incoming() {
            let Ok(mut stream) = stream else { return };
            std::thread::spawn(move || {
                let mut buf = [0u8; 4096];
                let _ = stream.read(&mut buf);
                let _ = stream.write_all(
                    b"HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\nTransfer-Encoding: chunked\r\n\r\n",
                );
                let chunk = format!("{:x}\r\n{}\r\n", filler.len(), filler);
                while stream.write_all(chunk.as_bytes()).is_ok() {}
            });
        }
    });
    format!("http://127.0.0.1:{port}")
}

const FILLER: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

fn assert_capped(err: &anyhow::Error, what: &str) {
    let text = format!("{err:#}");
    assert!(
        text.contains("larger than") && text.contains(&MAX_EXTERNAL_BODY_BYTES.to_string()),
        "{what} did not stop at the cap: {text}"
    );
}

#[tokio::test]
async fn the_helpers_stop_at_the_cap() {
    let base = endless(FILLER);
    let resp = reqwest::get(&base).await.unwrap();
    let e = external_text(resp).await.unwrap_err();
    assert!(e.to_string().contains("larger than"), "{e}");
    let resp = reqwest::get(&base).await.unwrap();
    let e = external_json::<serde_json::Value>(resp).await.unwrap_err();
    assert!(e.to_string().contains("larger than"), "{e}");
}

#[tokio::test]
async fn a_declared_length_over_the_cap_is_refused_before_a_byte_is_read() {
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = l.local_addr().unwrap().port();
    std::thread::spawn(move || {
        if let Ok((mut s, _)) = l.accept() {
            let mut buf = [0u8; 4096];
            let _ = s.read(&mut buf);
            let _ = s.write_all(
                format!(
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    MAX_EXTERNAL_BODY_BYTES + 1
                )
                .as_bytes(),
            );
            std::thread::sleep(std::time::Duration::from_secs(5));
        }
    });
    let resp = reqwest::get(format!("http://127.0.0.1:{port}"))
        .await
        .unwrap();
    let started = std::time::Instant::now();
    let e = external_text(resp).await.unwrap_err();
    assert!(e.to_string().contains("larger than"), "{e}");
    assert!(
        started.elapsed() < std::time::Duration::from_secs(2),
        "the refusal waited for the body"
    );
}

#[tokio::test]
async fn the_llm_call_stops_at_the_cap() {
    let base = endless(FILLER);
    let mut g = EnvGuard::new();
    g.set("IRIS_GENERATE_CLASS_MODEL", "claude-3-5-sonnet")
        .set("ANTHROPIC_API_KEY", "sk-test-key")
        .set("ANTHROPIC_BASE_URL", &base)
        .unset("IRIS_GENERATE_TIMEOUT");
    let client = iris_agentic_dev_core::generate::LlmClient::from_env().unwrap();
    let err = client.complete("s", "u").await.unwrap_err();
    assert_capped(&err, "generate (Anthropic)");

    let mut g = EnvGuard::new();
    g.set("IRIS_GENERATE_CLASS_MODEL", "gpt-4o")
        .set("OPENAI_API_KEY", "sk-test-key")
        .set("OPENAI_BASE_URL", &base)
        .unset("IRIS_GENERATE_TIMEOUT");
    let client = iris_agentic_dev_core::generate::LlmClient::from_env().unwrap();
    let err = client.complete("s", "u").await.unwrap_err();
    assert_capped(&err, "generate (OpenAI)");
}

#[tokio::test]
async fn the_skill_pack_fetch_stops_at_the_cap() {
    let base = endless(FILLER);
    let _g = EnvGuard::with("GITHUB_RAW_BASE_URL", &base);
    let installer = iris_agentic_dev_core::skill_install::SkillPackInstaller::new();
    let err = installer.fetch_pack_manifest().await.unwrap_err();
    assert_capped(&err, "skill_install manifest");
    let err = installer.fetch_skill_content("skills/x").await.unwrap_err();
    assert_capped(&err, "skill_install content");
}

#[tokio::test]
async fn loading_skills_from_github_stops_at_the_cap() {
    let base = endless(FILLER);
    let _g = EnvGuard::with("GITHUB_RAW_BASE_URL", &base);
    let mut registry = iris_agentic_dev_core::skills::SkillRegistry::new();
    let err = registry.load_from_github("o/r").await.unwrap_err();
    assert_capped(&err, "skills load_from_github");
}

#[tokio::test]
async fn the_version_lookup_stops_at_the_cap() {
    use iris_agentic_dev_core::manifest::resolve::{resolve_github_version_async, ResolvedSource};
    let base = endless(FILLER);
    let _g = EnvGuard::with("GITHUB_API_BASE_URL", &base);
    let req = semver::VersionReq::parse("*").unwrap();
    let err = resolve_github_version_async(
        &req,
        &ResolvedSource::GitHub {
            owner: "o".into(),
            repo: "r".into(),
        },
    )
    .await
    .unwrap_err();
    assert_capped(&err, "manifest resolve");
}

#[tokio::test]
async fn the_docs_page_read_stops_at_the_cap() {
    let base = endless(FILLER);
    let client = reqwest::Client::new();
    let err = iris_agentic_dev_core::tools::doc_search::fetch_algolia_creds_at(&client, &base)
        .await
        .unwrap_err();
    assert!(err.contains("larger than"), "{err}");
}

// ── The compare tools encode the namespace ──────────────────────────────────────────────────

/// Records each request line and answers 404, which both compare helpers report as an error.
fn recorder() -> (u16, Arc<Mutex<Vec<String>>>) {
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = l.local_addr().unwrap().port();
    let seen = Arc::new(Mutex::new(Vec::new()));
    let log = seen.clone();
    std::thread::spawn(move || {
        for stream in l.incoming() {
            let Ok(mut s) = stream else { return };
            let mut buf = [0u8; 4096];
            let n = s.read(&mut buf).unwrap_or(0);
            let head = String::from_utf8_lossy(&buf[..n]).into_owned();
            log.lock()
                .unwrap()
                .push(head.lines().next().unwrap_or("").to_string());
            let _ = s.write_all(
                b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
            );
        }
    });
    (port, seen)
}

#[tokio::test]
async fn the_compare_tools_encode_the_namespace_in_the_url() {
    use iris_agentic_dev_core::tools::comparison_tools::{fetch_class_list, fetch_document_source};
    let (port, seen) = recorder();
    let conn = IrisConnection::new(
        format!("http://127.0.0.1:{port}"),
        "USER",
        "u",
        "p",
        DiscoverySource::ExplicitFlag,
    );
    let client = reqwest::Client::new();
    let ns = "US ER/../%SYS?x=1";
    let _ = fetch_document_source(&conn, &client, "A.B.cls", ns).await;
    let _ = fetch_class_list(&conn, &client, ns).await;
    let lines = seen.lock().unwrap().clone();
    assert_eq!(lines.len(), 2, "{lines:?}");
    for line in &lines {
        let path = line.split_whitespace().nth(1).unwrap();
        assert!(
            path.starts_with("/api/atelier/v1/US%20ER%2F..%2F%25SYS%3Fx%3D1/"),
            "the namespace reached the URL unencoded: {line}"
        );
    }
}

// ── A host that never answers ───────────────────────────────────────────────────────────────

/// A server that accepts, reads the request and never writes a byte.
fn silent() -> String {
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = l.local_addr().unwrap().port();
    std::thread::spawn(move || {
        let mut held = Vec::new();
        for stream in l.incoming() {
            let Ok(mut s) = stream else { return };
            let mut buf = [0u8; 4096];
            let _ = s.read(&mut buf);
            held.push(s);
        }
    });
    format!("http://127.0.0.1:{port}")
}

/// The longest a request to a host that never answers may take in these tests, with the timeouts
/// shortened to a few hundred milliseconds. Without any timeout the call would wait for ever.
const GIVE_UP: std::time::Duration = std::time::Duration::from_secs(20);

#[test]
fn the_public_host_timeouts_are_thirty_and_one_hundred_twenty_seconds() {
    assert_eq!(
        iris_agentic_dev_core::iris::http::PUBLIC_HOST_CONNECT_TIMEOUT,
        std::time::Duration::from_secs(30)
    );
    assert_eq!(
        iris_agentic_dev_core::iris::http::PUBLIC_HOST_TIMEOUT,
        std::time::Duration::from_secs(120)
    );
}

#[tokio::test]
async fn the_skill_clients_and_the_version_lookup_give_up_on_a_silent_host() {
    use iris_agentic_dev_core::manifest::resolve::{resolve_github_version_async, ResolvedSource};
    use iris_agentic_dev_core::testing::set_public_host_timeouts;
    let base = silent();
    let mut g = EnvGuard::new();
    g.set("GITHUB_RAW_BASE_URL", &base)
        .set("GITHUB_API_BASE_URL", &base);
    set_public_host_timeouts(Some(std::time::Duration::from_millis(400)));

    let started = std::time::Instant::now();
    let installer = iris_agentic_dev_core::skill_install::SkillPackInstaller::new();
    let r1 = installer.fetch_pack_manifest().await;
    let r2 = installer.fetch_skill_content("skills/x").await;
    let mut registry = iris_agentic_dev_core::skills::SkillRegistry::new();
    let r3 = registry.load_from_github("o/r").await;
    let req = semver::VersionReq::parse("*").unwrap();
    let r4 = resolve_github_version_async(
        &req,
        &ResolvedSource::GitHub {
            owner: "o".into(),
            repo: "r".into(),
        },
    )
    .await;
    set_public_host_timeouts(None);

    assert!(r1.is_err(), "skill_install manifest");
    assert!(r2.is_err(), "skill_install content");
    assert!(r3.is_err(), "skills load_from_github");
    assert!(r4.is_err(), "manifest resolve");
    assert!(
        started.elapsed() < GIVE_UP,
        "a silent host held a client for {:?}",
        started.elapsed()
    );
}

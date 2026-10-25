//! Spec 141 T221: the low-severity items that do not touch the SQL validators, one test each.

use iris_agentic_dev_core::iris::connection::{
    user_agent, user_agent_with_peer, CallerMode, MCP_PEER,
};
use iris_agentic_dev_core::iris::iris_audit::build_event_data;
use iris_agentic_dev_core::testing::EnvGuard;

/// `IRIS_AGENT_LABEL` unset for the test's length, and the guard keeps the label tests in
/// `test_user_agent` from racing these.
fn no_label() -> EnvGuard {
    let mut g = EnvGuard::new();
    g.unset("IRIS_AGENT_LABEL");
    g
}

// ── 1. the MCP client's name and version ────────────────────────────────────────────────────────

fn peer(name: &str, version: &str) -> Option<(String, String)> {
    Some((name.to_string(), version.to_string()))
}

/// A header value is printable ASCII, and the line it lands in has one `(`, one closing `)` and
/// one `;` after the mode: the shape `iris-agentic-dev/<v> (mcp; <client>)`.
fn assert_well_formed_ua(ua: &str) {
    assert!(
        ua.bytes().all(|b| (0x20..0x7f).contains(&b)),
        "not printable ASCII: {ua:?}"
    );
    assert!(reqwest::header::HeaderValue::from_str(ua).is_ok(), "{ua:?}");
    assert_eq!(ua.matches('(').count(), 1, "{ua:?}");
    assert_eq!(ua.matches(')').count(), 1, "{ua:?}");
    assert!(ua.ends_with(')'), "{ua:?}");
    assert_eq!(ua.matches(';').count(), 1, "{ua:?}");
}

#[test]
fn a_client_name_and_version_cannot_forge_the_user_agent_or_the_audit_event() {
    let _g = no_label();
    let hostile = [
        // Request splitting, and a second header.
        ("evil\r\nX-Injected: 1", "1.0\r\n"),
        // Closes the parenthesis and opens a field of its own.
        ("x); mode=cli (y", "1); admin=true"),
        // Looks like another key=value pair in the audit event.
        ("claude ua=iris-agentic-dev/9.9 (cli)", "2 client=other/1"),
        // Non-ASCII anywhere breaks %SYS.Audit's export.
        ("caf\u{e9}-\u{1F600}-agent", "\u{202e}1.0"),
        // Control characters of every kind.
        ("a\0b\x07c\x1bd\x7fe", "1\t2"),
    ];
    for (name, version) in hostile {
        let p = peer(name, version);
        let ua = user_agent_with_peer(CallerMode::Mcp, p.as_ref());
        assert_well_formed_ua(&ua);

        let event = build_event_data("iris_info", CallerMode::Mcp, p);
        assert!(
            event.bytes().all(|b| (0x20..0x7f).contains(&b)),
            "audit event is not printable ASCII: {event:?}"
        );
        assert_eq!(
            event.matches(" ua=").count(),
            1,
            "a client name added a field to the audit event: {event:?}"
        );
        assert_eq!(
            event.matches(" client=").count(),
            1,
            "a client name added a field to the audit event: {event:?}"
        );
        assert_eq!(event.matches(" mode=").count(), 1, "{event:?}");
    }
}

#[test]
fn a_client_name_and_version_are_length_capped() {
    let _g = no_label();
    let long = "n".repeat(500);
    let p = peer(&long, &"9".repeat(500));
    let ua = user_agent_with_peer(CallerMode::Mcp, p.as_ref());
    assert!(
        ua.len() < 200,
        "an unbounded client name floods every access-log line: {} bytes",
        ua.len()
    );
    assert!(ua.contains('n') && ua.contains('9'), "still present: {ua}");
    let event = build_event_data("iris_info", CallerMode::Mcp, p);
    assert!(event.len() < 300, "{} bytes", event.len());
}

#[test]
fn an_ordinary_client_name_and_version_pass_through_unchanged() {
    let _g = no_label();
    for (name, version) in [
        ("claude-code", "2.1.0"),
        ("cursor", "0.99.0"),
        ("Visual-Studio-Code", "1.96.2+build.5"),
        ("my_harness.v2", "2026-10-24"),
    ] {
        let p = peer(name, version);
        let ua = user_agent_with_peer(CallerMode::Mcp, p.as_ref());
        assert!(ua.ends_with(&format!("; {name}/{version})")), "{ua}");
        let event = build_event_data("iris_info", CallerMode::Mcp, p);
        assert!(
            event.ends_with(&format!(" client={name}/{version}")),
            "{event}"
        );
    }
}

#[test]
fn a_peer_that_cleans_to_nothing_is_dropped_rather_than_printed_empty() {
    let _g = no_label();
    for (name, version) in [("\u{1F600}\u{1F600}", "1"), ("\r\n", "1"), ("", "1")] {
        let p = peer(name, version);
        let ua = user_agent_with_peer(CallerMode::Mcp, p.as_ref());
        assert!(!ua.contains("/)") && !ua.contains("; /"), "{ua:?}");
        assert!(ua.ends_with("(mcp)"), "{ua:?}");
        let event = build_event_data("iris_info", CallerMode::Mcp, p);
        assert!(!event.contains(" client="), "{event:?}");
    }
}

#[test]
fn the_user_agent_read_inside_a_call_is_cleaned_too() {
    let _g = no_label();
    // `user_agent` reads the task-local the pipeline sets, the path every HTTP request takes.
    let ua = tokio::runtime::Runtime::new().unwrap().block_on(
        MCP_PEER.scope(peer("evil\r\nHost: x); (", "1"), async {
            user_agent(CallerMode::Mcp)
        }),
    );
    assert_well_formed_ua(&ua);
}

// ── 2. an LLM key goes only to its own provider ────────────────────────────────────────────────

mod llm_keys {
    use iris_agentic_dev_core::generate::LlmClient;
    use iris_agentic_dev_core::testing::EnvGuard;
    use serde_json::json;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    const OPENAI_KEY: &str = "sk-openai-141-secret";
    const ANTHROPIC_KEY: &str = "sk-ant-141-secret";

    /// The two providers, as two local servers that record what they are sent. Stand-ins for APIs on
    /// the public internet; nothing about IRIS is mocked.
    async fn providers() -> (MockServer, MockServer) {
        let openai = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/chat/completions"))
            .respond_with(ResponseTemplate::new(200).set_body_json(
                json!({"choices": [{"message": {"role": "assistant", "content": "ok"}}]}),
            ))
            .mount(&openai)
            .await;
        let anthropic = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/messages"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(json!({"content": [{"text": "ok"}]})),
            )
            .mount(&anthropic)
            .await;
        (openai, anthropic)
    }

    fn env(model: &str, openai: &str, anthropic: &str, keys: (bool, bool)) -> EnvGuard {
        let mut g = EnvGuard::new();
        g.unset("IRIS_GENERATE_TIMEOUT");
        g.set("IRIS_GENERATE_CLASS_MODEL", model);
        g.set("OPENAI_BASE_URL", openai);
        g.set("ANTHROPIC_BASE_URL", anthropic);
        match keys.0 {
            true => g.set("OPENAI_API_KEY", OPENAI_KEY),
            false => g.unset("OPENAI_API_KEY"),
        };
        match keys.1 {
            true => g.set("ANTHROPIC_API_KEY", ANTHROPIC_KEY),
            false => g.unset("ANTHROPIC_API_KEY"),
        };
        g
    }

    async fn everything_sent(server: &MockServer) -> String {
        server
            .received_requests()
            .await
            .unwrap()
            .iter()
            .map(|r| format!("{:?} {}", r.headers, String::from_utf8_lossy(&r.body)))
            .collect()
    }

    #[tokio::test]
    async fn an_openai_model_sends_the_openai_key_to_the_openai_url_and_nothing_to_anthropic() {
        let (openai, anthropic) = providers().await;
        let _g = env("gpt-4o", &openai.uri(), &anthropic.uri(), (true, true));
        let client = LlmClient::from_env().expect("a client");
        client.complete("s", "u").await.expect("completes");
        client
            .complete_with_usage("s", "u")
            .await
            .expect("completes");

        let sent = everything_sent(&openai).await;
        assert!(
            sent.contains(OPENAI_KEY),
            "the OpenAI key went to OpenAI: {sent}"
        );
        assert!(!sent.contains(ANTHROPIC_KEY), "{sent}");
        assert!(
            anthropic.received_requests().await.unwrap().is_empty(),
            "an OpenAI model must not reach Anthropic's URL at all"
        );
    }

    #[tokio::test]
    async fn an_anthropic_model_sends_the_anthropic_key_to_the_anthropic_url_and_nothing_to_openai()
    {
        let (openai, anthropic) = providers().await;
        let _g = env(
            "claude-3-5-sonnet",
            &openai.uri(),
            &anthropic.uri(),
            (true, true),
        );
        let client = LlmClient::from_env().expect("a client");
        client.complete("s", "u").await.expect("completes");
        client
            .complete_with_usage("s", "u")
            .await
            .expect("completes");

        let sent = everything_sent(&anthropic).await;
        assert!(sent.contains(ANTHROPIC_KEY), "{sent}");
        assert!(
            !sent.contains(OPENAI_KEY),
            "the OpenAI key went to Anthropic: {sent}"
        );
        assert!(openai.received_requests().await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn a_model_with_only_the_other_providers_key_gets_no_client_and_sends_nothing() {
        let (openai, anthropic) = providers().await;
        // An Anthropic model, and only the OpenAI key: the key used to be sent to Anthropic.
        let g = env(
            "claude-3-5-sonnet",
            &openai.uri(),
            &anthropic.uri(),
            (true, false),
        );
        assert!(
            LlmClient::from_env().is_none(),
            "the OpenAI key must not stand in for the Anthropic one"
        );
        drop(g);
        // The mirror: an OpenAI model, and only the Anthropic key.
        let g = env("gpt-4o", &openai.uri(), &anthropic.uri(), (false, true));
        assert!(
            LlmClient::from_env().is_none(),
            "the Anthropic key must not stand in for the OpenAI one"
        );
        drop(g);
        assert!(openai.received_requests().await.unwrap().is_empty());
        assert!(anthropic.received_requests().await.unwrap().is_empty());
    }
}

// ── 3. the local docker exec ────────────────────────────────────────────────────────────────────

#[cfg(unix)]
mod local_docker_exec {
    use iris_agentic_dev_core::iris::connection::{
        DiscoverySource, IrisConnection, TerminalTarget,
    };
    use iris_agentic_dev_core::testing::EnvGuard;
    use std::os::unix::fs::PermissionsExt;

    /// A `docker` that records its arguments, one per line, and reads its stdin: a stand-in for the
    /// docker CLI, so the arguments iad builds can be read. It does not stand in for IRIS.
    fn fake_docker(dir: &std::path::Path) -> std::path::PathBuf {
        let log = dir.join("argv.log");
        let script = dir.join("docker");
        std::fs::write(
            &script,
            format!(
                "#!/bin/sh\nprintf '%s\\n' \"$@\" > '{}'\ncat > /dev/null\nexit 0\n",
                log.display()
            ),
        )
        .unwrap();
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
        log
    }

    fn conn() -> IrisConnection {
        IrisConnection::new(
            "http://127.0.0.1:1",
            "USER",
            "_SYSTEM",
            "SYS",
            DiscoverySource::ExplicitFlag,
        )
    }

    fn target(container: &str) -> TerminalTarget {
        TerminalTarget {
            container: container.to_string(),
            ssh_host: None,
            login: None,
            ssh_strict: false,
        }
    }

    fn with_fake_docker(dir: &std::path::Path) -> EnvGuard {
        let mut g = EnvGuard::new();
        let old = std::env::var("PATH").unwrap_or_default();
        g.set("PATH", format!("{}:{old}", dir.display()));
        g
    }

    #[tokio::test]
    async fn a_valid_container_name_follows_a_double_dash_so_it_cannot_be_read_as_an_option() {
        let dir = tempfile::tempdir().unwrap();
        let log = fake_docker(dir.path());
        let _g = with_fake_docker(dir.path());
        // A name that starts with a letter, as docker requires, reads as a container either way;
        // the `--` is what holds for the names the check above it might one day let through.
        let _ = conn()
            .execute_terminal("Write 1", "USER", &target("iris-dev-iris"))
            .await;
        let argv: Vec<String> = std::fs::read_to_string(&log)
            .expect("docker was run")
            .lines()
            .map(String::from)
            .collect();
        assert_eq!(
            argv,
            [
                "exec",
                "-i",
                "--",
                "iris-dev-iris",
                "iris",
                "session",
                "IRIS",
                "-U",
                "USER"
            ],
            "the container follows `--`"
        );
    }

    #[tokio::test]
    async fn a_container_name_that_docker_would_read_as_an_option_never_reaches_docker() {
        let dir = tempfile::tempdir().unwrap();
        let log = fake_docker(dir.path());
        let _g = with_fake_docker(dir.path());
        for name in [
            "--privileged",
            "-v/:/host",
            "--user=root",
            "-it",
            "",
            "a b",
            "a;b",
            "$(id)",
            "a\nb",
            "caf\u{e9}",
            "_leading-underscore",
            ".dot",
        ] {
            let err = conn()
                .execute_terminal("Write 1", "USER", &target(name))
                .await
                .expect_err(&format!("{name:?} must be refused"));
            assert!(
                err.to_string().contains("INVALID_CONTAINER"),
                "{name:?}: {err}"
            );
            assert!(
                !log.exists(),
                "{name:?}: docker was run with an invalid container name"
            );
        }
    }
}

// ── 5. the telemetry and benchmark string literals ─────────────────────────────────────────────

#[test]
fn the_telemetry_listbuild_keeps_a_control_character_on_its_line() {
    use iris_agentic_dev_core::telemetry::{encode_listbuild, ToolCallRecord};
    let mut record = ToolCallRecord::now("iris_x\"\r\nSet ^Evil=1", true, 1, uuid::Uuid::new_v4());
    record.timestamp = "2026-10-24\nT".to_string();
    record.params = Some(serde_json::json!({"q": "a\nb"}));
    let lb = encode_listbuild(&record);
    // The record is spliced into one generated statement. Doubling the quote alone left the line
    // break in, and the rest of the name ran as a second statement.
    assert!(!lb.contains('\n') && !lb.contains('\r'), "{lb:?}");
    assert!(lb.contains("$CHAR(13,10)"), "{lb}");
    assert!(lb.contains("$CHAR(10)"), "{lb}");
    assert!(lb.contains("\"\""), "the quote is still doubled: {lb}");
    // A record with nothing to escape is what it was.
    let plain = ToolCallRecord::now("iris_compile", true, 5, uuid::Uuid::new_v4());
    let lb = encode_listbuild(&plain);
    assert!(lb.starts_with("$LISTBUILD(\"iris_compile\",1,5,\""), "{lb}");
}

#[test]
fn the_benchmark_test_runner_keeps_a_control_character_in_a_class_name_on_its_line() {
    use iris_agentic_dev_core::benchmark::container::build_run_all_code;
    let clean = build_run_all_code("Bench.Good");
    let hostile = build_run_all_code("Bench.A\nHalt\r\nB\"c");
    assert_eq!(
        hostile.lines().count(),
        clean.lines().count(),
        "a line break in the class name started a new line of generated code:\n{hostile}"
    );
    assert!(hostile.contains("$CHAR(10"), "{hostile}");
    assert!(
        clean.contains("%OpenId(\"Bench.Good\")"),
        "a clean name is spliced as before: {clean}"
    );
}

// ── 4. the cap on a body read from IRIS ────────────────────────────────────────────────────────

mod response_cap {
    use iris_agentic_dev_core::iris::http::{
        capped_json, capped_text, json_or_default, max_response_bytes, text_or_default, BodyError,
        DEFAULT_MAX_RESPONSE_BYTES,
    };
    use iris_agentic_dev_core::testing::EnvGuard;
    use serde_json::{json, Value};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use wiremock::matchers::method;
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn limit(bytes: &str) -> EnvGuard {
        let mut g = EnvGuard::new();
        g.set("IAD_MAX_RESPONSE_BYTES", bytes);
        g
    }

    async fn serving(body: Vec<u8>) -> (MockServer, reqwest::Response) {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(body))
            .mount(&server)
            .await;
        let resp = reqwest::get(server.uri()).await.unwrap();
        (server, resp)
    }

    /// A server that answers with `total` bytes of `a` as a chunked body, which has no length to
    /// check up front.
    async fn chunked(total: usize) -> String {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        tokio::spawn(async move {
            let (mut s, _) = listener.accept().await.unwrap();
            let mut buf = [0u8; 1024];
            let _ = s.read(&mut buf).await;
            let _ = s
                .write_all(b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n")
                .await;
            let chunk = vec![b'a'; 8192];
            let mut sent = 0;
            while sent < total {
                let head = format!("{:x}\r\n", chunk.len());
                if s.write_all(head.as_bytes()).await.is_err()
                    || s.write_all(&chunk).await.is_err()
                    || s.write_all(b"\r\n").await.is_err()
                {
                    return;
                }
                sent += chunk.len();
            }
            let _ = s.write_all(b"0\r\n\r\n").await;
        });
        url
    }

    #[test]
    fn the_default_cap_is_far_above_the_largest_answer_a_live_iris_gave() {
        // The largest body measured on iris-dev-iris (2026.2): the USER namespace's document list,
        // 1.6 MB. A namespace with a hundred thousand documents is still an order of magnitude below.
        let largest_measured = 1_619_033usize;
        assert!(
            DEFAULT_MAX_RESPONSE_BYTES >= 64 * largest_measured,
            "{DEFAULT_MAX_RESPONSE_BYTES}"
        );
        let mut g = EnvGuard::new();
        g.unset("IAD_MAX_RESPONSE_BYTES");
        assert_eq!(max_response_bytes(), DEFAULT_MAX_RESPONSE_BYTES);
    }

    #[test]
    fn the_cap_variable_is_a_behaviour_variable() {
        assert!(
            iris_agentic_dev_core::testing::BEHAVIOR_ENV_VARS.contains(&"IAD_MAX_RESPONSE_BYTES"),
            "a new behaviour variable belongs in BEHAVIOR_ENV_VARS (principle XII)"
        );
    }

    #[test]
    fn the_cap_is_a_launch_setting_and_a_bad_value_leaves_the_default() {
        for (raw, want) in [
            ("4096", 4096),
            (" 4096 ", 4096),
            ("0", DEFAULT_MAX_RESPONSE_BYTES),
            ("-1", DEFAULT_MAX_RESPONSE_BYTES),
            ("lots", DEFAULT_MAX_RESPONSE_BYTES),
            ("", DEFAULT_MAX_RESPONSE_BYTES),
        ] {
            let _g = limit(raw);
            assert_eq!(max_response_bytes(), want, "{raw:?}");
        }
    }

    #[tokio::test]
    async fn a_body_under_the_cap_reads_as_it_did() {
        let _g = limit("4096");
        let (_s, resp) = serving(json!({"a": [1, 2, 3]}).to_string().into_bytes()).await;
        let v: Value = capped_json(resp).await.unwrap();
        assert_eq!(v, json!({"a": [1, 2, 3]}));
        let (_s, resp) = serving(b"hello".to_vec()).await;
        assert_eq!(capped_text(resp).await.unwrap(), "hello");
    }

    #[tokio::test]
    async fn a_body_with_a_declared_length_over_the_cap_is_refused() {
        let _g = limit("1024");
        let (_s, resp) = serving(vec![b'x'; 10_000]).await;
        match capped_text(resp).await {
            Err(BodyError::TooLarge(e)) => {
                assert_eq!(e.limit, 1024);
                let text = e.to_string();
                for needle in ["RESPONSE_TOO_LARGE", "1024", "IAD_MAX_RESPONSE_BYTES"] {
                    assert!(text.contains(needle), "{needle} missing from {text}");
                }
            }
            other => panic!("expected TooLarge, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn a_chunked_body_with_no_length_is_cut_off_at_the_cap() {
        let _g = limit("65536");
        let url = chunked(8 * 1024 * 1024).await;
        let resp = reqwest::get(url).await.unwrap();
        assert!(resp.content_length().is_none(), "the case needs no length");
        assert!(matches!(
            capped_json::<Value>(resp).await,
            Err(BodyError::TooLarge(_))
        ));
    }

    #[tokio::test]
    async fn the_or_default_readers_keep_their_old_answer_for_a_bad_body_and_name_the_cap_for_a_big_one(
    ) {
        let _g = limit("1024");
        // Not JSON: Null, as `.json().await.unwrap_or_default()` gave.
        let (_s, resp) = serving(b"<html>".to_vec()).await;
        assert_eq!(json_or_default(resp).await, Value::Null);
        // Too big: an Atelier-shaped error, so a handler that reads `status.errors` names the cap.
        let (_s, resp) = serving(vec![b'x'; 5000]).await;
        let v = json_or_default(resp).await;
        let err = v["status"]["errors"][0]["error"].as_str().unwrap_or("");
        assert!(err.contains("RESPONSE_TOO_LARGE"), "{v}");
        let (_s, resp) = serving(vec![b'x'; 5000]).await;
        assert!(text_or_default(resp).await.contains("RESPONSE_TOO_LARGE"));
    }
}

/// The `iris_test_server` NoPWS probe runs `docker exec` like the executor does: the container
/// name is checked, and `--` ends docker's options before it.
#[test]
fn the_nopws_cpf_probe_checks_the_container_name_and_ends_options() {
    use iris_agentic_dev_core::tools::cpf_probe_args;
    for bad in ["--privileged", "-v/:/host", "", "a b", "x;rm"] {
        assert!(
            cpf_probe_args(bad, "/usr/irissys/iris.cpf").is_err(),
            "{bad:?}"
        );
    }
    let args = cpf_probe_args("iris-dev-iris", "/usr/irissys/iris.cpf").unwrap();
    assert_eq!(&args[..3], ["exec", "--", "iris-dev-iris"]);
}

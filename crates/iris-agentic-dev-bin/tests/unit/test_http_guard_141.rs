//! Spec 141 FR-025 / FR-035: the HTTP transport guard (T-R4b-U14, U15, U17).
//!
//! `http_guard::check` is a pure function over the request head, so none of this opens a socket.

use std::net::IpAddr;

use clap::Parser;
use iris_agentic_dev::cmd::benchmark::BenchmarkCommand;
use iris_agentic_dev::cmd::http_guard::{
    check, token_matches, GuardSettings, HttpGuard, Rejection, TokenSources,
};

const TOKEN: &str = "0123456789abcdef0123456789abcdef-token";

fn parts(headers: &[(&str, &str)]) -> hyper::http::request::Parts {
    let mut b = hyper::Request::builder().method("POST").uri("/mcp");
    for (k, v) in headers {
        b = b.header(*k, *v);
    }
    b.body(()).unwrap().into_parts().0
}

fn guard(bind: &str, token: Option<&str>, hosts: &[&str], origins: &[&str]) -> HttpGuard {
    HttpGuard::from_settings(
        bind.parse::<IpAddr>().unwrap(),
        GuardSettings {
            token: token.map(str::to_string),
            allowed_hosts: hosts.iter().map(|s| s.to_string()).collect(),
            allowed_origins: origins.iter().map(|s| s.to_string()).collect(),
        },
    )
    .expect("settings are valid")
}

fn bearer() -> String {
    format!("Bearer {TOKEN}")
}

#[test]
fn no_token_on_loopback_passes_a_plain_request() {
    let g = guard("127.0.0.1", None, &[], &[]);
    assert_eq!(check(&parts(&[("host", "127.0.0.1:8080")]), &g), Ok(()));
    assert_eq!(check(&parts(&[("host", "localhost")]), &g), Ok(()));
    assert_eq!(check(&parts(&[("host", "[::1]:8080")]), &g), Ok(()));
}

#[test]
fn a_configured_token_is_required_on_loopback_too() {
    let g = guard("127.0.0.1", Some(TOKEN), &[], &[]);
    let host = ("host", "127.0.0.1:8080");
    assert_eq!(check(&parts(&[host]), &g), Err(Rejection::Unauthorized));
    assert_eq!(
        check(&parts(&[host, ("authorization", "Bearer wrong")]), &g),
        Err(Rejection::Unauthorized)
    );
    // Same prefix, one byte short and one byte long.
    let short = format!("Bearer {}", &TOKEN[..TOKEN.len() - 1]);
    assert_eq!(
        check(&parts(&[host, ("authorization", &short)]), &g),
        Err(Rejection::Unauthorized)
    );
    let long = format!("Bearer {TOKEN}x");
    assert_eq!(
        check(&parts(&[host, ("authorization", &long)]), &g),
        Err(Rejection::Unauthorized)
    );
    // Another scheme with the right secret is still wrong.
    let basic = format!("Basic {TOKEN}");
    assert_eq!(
        check(&parts(&[host, ("authorization", &basic)]), &g),
        Err(Rejection::Unauthorized)
    );
}

#[test]
fn the_bearer_scheme_is_case_insensitive() {
    let g = guard("127.0.0.1", Some(TOKEN), &[], &[]);
    for scheme in ["Bearer", "bearer", "BEARER"] {
        let h = format!("{scheme} {TOKEN}");
        assert_eq!(
            check(&parts(&[("host", "localhost"), ("authorization", &h)]), &g),
            Ok(()),
            "{scheme}"
        );
    }
}

#[test]
fn an_unknown_host_is_refused_on_loopback_and_on_every_interface() {
    for bind in ["127.0.0.1", "0.0.0.0"] {
        let g = guard(bind, Some(TOKEN), &[], &[]);
        let h = bearer();
        assert_eq!(
            check(
                &parts(&[("host", "evil.example"), ("authorization", &h)]),
                &g
            ),
            Err(Rejection::BadHost),
            "{bind}"
        );
    }
}

#[test]
fn a_request_without_a_host_is_refused() {
    let g = guard("127.0.0.1", None, &[], &[]);
    assert_eq!(check(&parts(&[]), &g), Err(Rejection::BadHost));
}

#[test]
fn host_is_checked_before_the_token() {
    // Neither is right: the refusal says Host, so a probe learns nothing about the token.
    let g = guard("0.0.0.0", Some(TOKEN), &[], &[]);
    assert_eq!(
        check(&parts(&[("host", "evil.example")]), &g),
        Err(Rejection::BadHost)
    );
}

#[test]
fn an_ip_literal_passes_only_on_an_unspecified_bind() {
    let h = bearer();
    let req = parts(&[("host", "10.1.2.3:8080"), ("authorization", &h)]);
    assert_eq!(
        check(&req, &guard("0.0.0.0", Some(TOKEN), &[], &[])),
        Ok(())
    );
    assert_eq!(
        check(&req, &guard("127.0.0.1", Some(TOKEN), &[], &[])),
        Err(Rejection::BadHost)
    );
}

#[test]
fn a_pinned_bind_allows_its_own_address() {
    let g = guard("192.168.7.7", Some(TOKEN), &[], &[]);
    let h = bearer();
    assert_eq!(
        check(
            &parts(&[("host", "192.168.7.7:9"), ("authorization", &h)]),
            &g
        ),
        Ok(())
    );
    assert_eq!(
        check(
            &parts(&[("host", "192.168.7.8:9"), ("authorization", &h)]),
            &g
        ),
        Err(Rejection::BadHost)
    );
}

#[test]
fn host_docker_internal_passes_only_on_an_unspecified_bind() {
    let h = bearer();
    let req = parts(&[("host", "host.docker.internal:8080"), ("authorization", &h)]);
    assert_eq!(
        check(&req, &guard("0.0.0.0", Some(TOKEN), &[], &[])),
        Ok(())
    );
    assert_eq!(check(&req, &guard("::", Some(TOKEN), &[], &[])), Ok(()));
    assert_eq!(
        check(&req, &guard("127.0.0.1", Some(TOKEN), &[], &[])),
        Err(Rejection::BadHost)
    );
}

#[test]
fn an_allowed_host_name_passes_whatever_the_case_and_port() {
    let g = guard("0.0.0.0", Some(TOKEN), &["Iad.Internal"], &[]);
    let h = bearer();
    assert_eq!(
        check(
            &parts(&[("host", "iad.internal:81"), ("authorization", &h)]),
            &g
        ),
        Ok(())
    );
    assert_eq!(
        check(
            &parts(&[("host", "other.internal"), ("authorization", &h)]),
            &g
        ),
        Err(Rejection::BadHost)
    );
}

#[test]
fn a_cross_site_origin_is_refused_even_with_the_token() {
    let g = guard("0.0.0.0", Some(TOKEN), &[], &[]);
    let h = bearer();
    for origin in [
        "https://evil.example",
        "null",
        "http://localhost.evil.example",
    ] {
        assert_eq!(
            check(
                &parts(&[
                    ("host", "host.docker.internal"),
                    ("authorization", &h),
                    ("origin", origin)
                ]),
                &g
            ),
            Err(Rejection::BadOrigin),
            "{origin}"
        );
    }
}

#[test]
fn a_same_origin_or_listed_origin_passes_and_no_origin_passes() {
    let g = guard("127.0.0.1", None, &[], &["https://app.example:8443/"]);
    assert_eq!(check(&parts(&[("host", "localhost:3000")]), &g), Ok(()));
    assert_eq!(
        check(
            &parts(&[
                ("host", "localhost:3000"),
                ("origin", "http://localhost:3000")
            ]),
            &g
        ),
        Ok(())
    );
    assert_eq!(
        check(
            &parts(&[
                ("host", "localhost:3000"),
                ("origin", "https://APP.example:8443")
            ]),
            &g
        ),
        Ok(())
    );
    // Same host name, other port: a different origin.
    assert_eq!(
        check(
            &parts(&[
                ("host", "localhost:3000"),
                ("origin", "http://localhost:4000")
            ]),
            &g
        ),
        Err(Rejection::BadOrigin)
    );
}

#[test]
fn token_matches_agrees_for_every_length() {
    assert!(token_matches(b"", b""));
    for n in 1..=70usize {
        let expected = vec![b'a'; n];
        assert!(token_matches(&expected, &expected), "equal, len {n}");
        let mut last = expected.clone();
        last[n - 1] = b'b';
        assert!(!token_matches(&last, &expected), "last byte, len {n}");
        let mut first = expected.clone();
        first[0] = b'b';
        assert!(!token_matches(&first, &expected), "first byte, len {n}");
        assert!(
            !token_matches(&expected[..n - 1], &expected),
            "short, len {n}"
        );
        let mut long = expected.clone();
        long.push(b'a');
        assert!(!token_matches(&long, &expected), "long, len {n}");
        assert!(!token_matches(b"", &expected), "empty given, len {n}");
    }
}

// ── T-R4b-U15: where the token may come from ──────────────────────────────────────────────────

#[test]
fn a_token_under_32_bytes_is_an_error_from_every_source() {
    let short = "0123456789abcdef0123456789abcde"; // 31 bytes
    assert_eq!(short.len(), 31);
    for sources in [
        TokenSources {
            env: Some(short.into()),
            ..Default::default()
        },
        TokenSources {
            file: Some(format!("{short}\n")),
            ..Default::default()
        },
        TokenSources {
            operator: Some(short.into()),
            ..Default::default()
        },
    ] {
        let err = sources.resolve().unwrap_err();
        assert!(err.contains("32"), "{err}");
        assert!(err.contains("openssl rand -hex 32"), "{err}");
    }
}

#[test]
fn token_sources_resolve_in_order_and_a_file_loses_its_newline() {
    let other = "ffffffffffffffffffffffffffffffffffff".to_string();
    let all = TokenSources {
        env: Some(TOKEN.into()),
        file: Some(format!("{other}\n")),
        operator: Some(other.clone()),
    };
    assert_eq!(all.resolve().unwrap().as_deref(), Some(TOKEN));
    let file_and_operator = TokenSources {
        env: None,
        file: Some(format!("{TOKEN}\r\n")),
        operator: Some(other),
    };
    assert_eq!(file_and_operator.resolve().unwrap().as_deref(), Some(TOKEN));
    assert_eq!(TokenSources::default().resolve().unwrap(), None);
}

#[test]
fn a_short_source_does_not_fall_through_to_a_later_one() {
    // A too-short environment token is a mistake to report, not a reason to use the operator's.
    let s = TokenSources {
        env: Some("short".into()),
        file: None,
        operator: Some(TOKEN.into()),
    };
    assert!(s.resolve().is_err());
}

#[test]
fn a_non_loopback_bind_without_a_token_does_not_start() {
    for bind in ["0.0.0.0", "::", "192.168.7.7"] {
        let err =
            HttpGuard::from_settings(bind.parse::<IpAddr>().unwrap(), GuardSettings::default())
                .unwrap_err();
        assert!(err.contains("IAD_HTTP_TOKEN"), "{bind}: {err}");
        assert!(err.contains("--auth-token-file"), "{bind}: {err}");
        assert!(err.contains("http_token"), "{bind}: {err}");
    }
    for bind in ["127.0.0.1", "::1", "127.0.0.2"] {
        assert!(
            HttpGuard::from_settings(bind.parse::<IpAddr>().unwrap(), GuardSettings::default())
                .is_ok(),
            "{bind} is loopback"
        );
    }
}

fn load(
    home: &std::path::Path,
    ws: &std::path::Path,
) -> iris_agentic_dev_core::iris::config_sources::ConfigSnapshot {
    use iris_agentic_dev_core::iris::config_sources::{ConfigSnapshot, LoadInputs};
    let inputs = LoadInputs {
        cwd: ws.to_path_buf(),
        home: Some(home.to_path_buf()),
        ..Default::default()
    };
    ConfigSnapshot::load(&inputs)
}

#[test]
fn http_token_from_a_workspace_file_is_set_aside_and_never_used() {
    let home = tempfile::tempdir().unwrap();
    let ws = tempfile::tempdir().unwrap();
    std::fs::write(
        ws.path().join(".iris-agentic-dev.toml"),
        format!("[transport]\nhttp_token = \"{TOKEN}\"\n"),
    )
    .unwrap();
    let snap = load(home.path(), ws.path());
    assert_eq!(snap.operator.http_token, None);
    let item = snap
        .set_aside
        .iter()
        .find(|i| i.setting.starts_with("transport.http_token"))
        .expect("the workspace token is recorded as set aside");
    assert_eq!(item.kind.as_str(), "operator_only");
    assert!(
        !item.to_json().to_string().contains(TOKEN),
        "the token value never appears in a set-aside item"
    );
}

#[test]
fn http_token_from_the_operator_file_is_used() {
    let home = tempfile::tempdir().unwrap();
    let ws = tempfile::tempdir().unwrap();
    std::fs::write(
        home.path().join(".iris-agentic-dev.toml"),
        format!("[transport]\nhttp_token = \"{TOKEN}\"\n"),
    )
    .unwrap();
    let snap = load(home.path(), ws.path());
    assert_eq!(snap.operator.http_token.as_deref(), Some(TOKEN));
    let resolved = TokenSources {
        operator: snap.operator.http_token.clone(),
        ..Default::default()
    }
    .resolve()
    .unwrap();
    assert_eq!(resolved.as_deref(), Some(TOKEN));
}

// ── T-R4b-U17: benchmark honours the scheme ───────────────────────────────────────────────────

#[derive(Parser)]
struct Cli {
    #[command(flatten)]
    cmd: BenchmarkCommand,
}

fn bench(args: &[&str]) -> BenchmarkCommand {
    let mut argv = vec!["test-cli", "--skill", "x.md"];
    argv.extend_from_slice(args);
    Cli::try_parse_from(argv).expect("parses").cmd
}

#[test]
fn benchmark_scheme_flag_reaches_the_base_url() {
    let cmd = bench(&["--scheme", "https", "--host", "x", "--web-port", "443"]);
    assert_eq!(cmd.base_url().unwrap().as_deref(), Some("https://x:443"));
    let cmd = bench(&["--scheme", "http", "--host", "x", "--web-port", "52773"]);
    assert_eq!(cmd.base_url().unwrap().as_deref(), Some("http://x:52773"));
}

#[test]
fn benchmark_refuses_a_scheme_that_is_not_http_or_https() {
    let cmd = bench(&["--scheme", "ftp", "--host", "x"]);
    assert!(cmd.base_url().is_err());
}

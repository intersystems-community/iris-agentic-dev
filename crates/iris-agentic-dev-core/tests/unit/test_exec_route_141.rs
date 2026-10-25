//! Spec 141 R2: which container a call may run in, what happens when HTTP fails, which cookie
//! jar an identity uses, and how a WebSocket session remembers who opened it (T-R2-U07 to U11).

use iris_agentic_dev_core::iris::connection::{ssh_args, ERR_CONTAINER_UNREACHABLE};
use iris_agentic_dev_core::iris::csp_session::{
    all_logout_urls, shared_cookie_store, store_for_identity,
};
use iris_agentic_dev_core::iris::ws_session::WsSessionPool;
use iris_agentic_dev_core::tools::exec::{
    fallback_decision, resolve_route, ExecRoute, FallbackDecision, RouteTarget,
};
use reqwest::cookie::CookieStore;
use reqwest::header::HeaderValue;
use std::sync::Arc;

// ── U07: ExecRoute ────────────────────────────────────────────────────────────

#[test]
fn iris_container_applies_to_the_default_and_env_connections_only() {
    let default = resolve_route(RouteTarget::DefaultOrEnv, Some("c141"), None, false, None);
    assert_eq!(
        default,
        ExecRoute::HttpThenDocker {
            container: "c141".into(),
            ssh_host: None
        }
    );
    let member = resolve_route(RouteTarget::Member, Some("c141"), None, false, None);
    assert_eq!(
        member,
        ExecRoute::Http,
        "a named server never inherits IRIS_CONTAINER"
    );
}

#[test]
fn a_member_with_its_own_container_gets_it() {
    let r = resolve_route(
        RouteTarget::Member,
        Some("other"),
        Some("qa-iris"),
        false,
        None,
    );
    assert_eq!(
        r,
        ExecRoute::HttpThenDocker {
            container: "qa-iris".into(),
            ssh_host: None
        }
    );
}

#[test]
fn docker_only_skips_http() {
    let r = resolve_route(
        RouteTarget::DefaultOrEnv,
        Some("c141"),
        None,
        true,
        Some("host"),
    );
    assert_eq!(
        r,
        ExecRoute::DockerOnly {
            container: "c141".into(),
            ssh_host: Some("host".into())
        }
    );
}

#[test]
fn docker_only_with_no_container_is_http_so_the_missing_container_is_reported_not_guessed() {
    let r = resolve_route(RouteTarget::DefaultOrEnv, None, None, true, None);
    assert_eq!(r, ExecRoute::DockerOnlyNoContainer);
}

#[test]
fn a_blank_container_is_no_container() {
    let r = resolve_route(RouteTarget::DefaultOrEnv, Some("  "), None, false, None);
    assert_eq!(r, ExecRoute::Http);
}

// ── U08: the fallback rule ────────────────────────────────────────────────────

fn route_docker() -> ExecRoute {
    ExecRoute::HttpThenDocker {
        container: "c141".into(),
        ssh_host: None,
    }
}

#[test]
fn a_service_account_refused_by_http_does_not_fall_back_to_docker() {
    for text in [
        "PUT doc failed: HTTP 401 Unauthorized",
        "PUT doc failed: HTTP 403 Forbidden",
        "compile HTTP 403 Forbidden",
        "compile errors: <PROTECT> ^oddDEF",
    ] {
        let e = anyhow::anyhow!("{text}");
        assert_eq!(
            fallback_decision(true, &e, &route_docker(), false, true),
            FallbackDecision::ServiceAccountHttpDenied,
            "{text}"
        );
    }
}

#[test]
fn the_primary_refused_by_http_still_falls_back_as_before() {
    let e = anyhow::anyhow!("PUT doc failed: HTTP 401 Unauthorized");
    assert_eq!(
        fallback_decision(false, &e, &route_docker(), false, true),
        FallbackDecision::Docker
    );
}

#[test]
fn connection_refused_with_a_docker_route_goes_to_docker() {
    let e = anyhow::anyhow!("error sending request: connection refused");
    assert_eq!(
        fallback_decision(true, &e, &route_docker(), false, true),
        FallbackDecision::Docker
    );
}

#[test]
fn code_that_may_have_run_is_never_run_again() {
    let e: anyhow::Error =
        iris_agentic_dev_core::iris::connection::CodeMayHaveRun("x".into()).into();
    assert_eq!(
        fallback_decision(true, &e, &route_docker(), false, true),
        FallbackDecision::CodeMayHaveRun
    );
    assert_eq!(
        fallback_decision(false, &e, &route_docker(), false, true),
        FallbackDecision::CodeMayHaveRun
    );
}

#[test]
fn a_named_server_with_no_docker_route_reports_http_and_says_why_docker_was_not_used() {
    let e = anyhow::anyhow!("error sending request: connection refused");
    assert_eq!(
        fallback_decision(false, &e, &ExecRoute::Http, true, true),
        FallbackDecision::HttpFailed {
            no_docker_route: true
        }
    );
    assert_eq!(
        fallback_decision(false, &e, &ExecRoute::Http, true, false),
        FallbackDecision::HttpFailed {
            no_docker_route: false
        },
        "the note appears only when IRIS_CONTAINER is set and was not used"
    );
}

#[test]
fn the_unreachable_container_error_code_is_stable() {
    assert_eq!(ERR_CONTAINER_UNREACHABLE, "CONTAINER_UNREACHABLE");
}

// ── U09: one cookie jar per identity ──────────────────────────────────────────

fn set_cookie(store: &dyn CookieStore, session: &str, url: &str) {
    let h = HeaderValue::from_str(&format!(
        "CSPSESSIONID-SP-1-UP-api-atelier-={session}; Path=/api/atelier/"
    ))
    .unwrap();
    store.set_cookies(&mut std::iter::once(&h), &url.parse().unwrap());
}

#[test]
fn a_cookie_set_through_the_primary_store_is_not_sent_by_the_service_store() {
    let url = "http://127.0.0.1:39781/api/atelier/";
    let primary = shared_cookie_store();
    let service = store_for_identity("u09-service");
    set_cookie(&*primary, "PRIMARYCOOKIE", url);
    assert!(
        primary
            .cookies(&url.parse().unwrap())
            .is_some_and(|c| c.to_str().unwrap().contains("PRIMARYCOOKIE")),
        "the primary store holds its own cookie"
    );
    assert!(
        service.cookies(&url.parse().unwrap()).is_none(),
        "an identity that shares the primary's jar would be logged in as the primary"
    );
}

#[test]
fn two_clients_of_one_identity_share_a_store_and_two_identities_do_not() {
    let a = store_for_identity("u09-a");
    let a2 = store_for_identity("u09-a");
    let b = store_for_identity("u09-b");
    assert!(Arc::ptr_eq(&a, &a2));
    assert!(!Arc::ptr_eq(&a, &b));
    assert!(!Arc::ptr_eq(&a, &shared_cookie_store()));
}

#[test]
fn logout_covers_the_session_of_every_registered_store() {
    let store = store_for_identity("u09-logout");
    set_cookie(&*store, "SVC", "http://127.0.0.1:39799/api/atelier/");
    let urls = all_logout_urls();
    assert!(
        urls.iter()
            .any(|u| u == "http://127.0.0.1:39799/api/atelier/?IRISLogout=end"),
        "{urls:?}"
    );
}

// ── U10: a WebSocket session remembers who opened it ──────────────────────────

#[test]
fn a_session_opened_as_one_user_is_refused_once_the_current_identity_differs() {
    let pool = Arc::new(WsSessionPool::new());
    pool.record_target("ws:default:USER:1", None);
    pool.record_identity("ws:default:USER:1", "iadsvc");

    assert_eq!(
        pool.identity_of("ws:default:USER:1").as_deref(),
        Some("iadsvc")
    );
    assert!(pool.check_identity("ws:default:USER:1", "IADSVC").is_ok());

    let err = pool
        .check_identity("ws:default:USER:1", "_SYSTEM")
        .expect_err("the account changed since the session opened");
    assert!(err.contains("SESSION_IDENTITY_CHANGED"), "{err}");
    assert!(
        err.contains("iadsvc") && err.contains("_SYSTEM"),
        "the message names both users: {err}"
    );
    assert_eq!(
        pool.identity_of("ws:default:USER:1"),
        None,
        "a refused session's record is removed"
    );
    assert_eq!(pool.target_of("ws:default:USER:1"), None);
}

#[test]
fn a_session_with_no_recorded_identity_is_refused_not_trusted() {
    let pool = WsSessionPool::new();
    let err = pool
        .check_identity("ws:default:USER:2", "iadsvc")
        .unwrap_err();
    assert!(err.contains("SESSION_IDENTITY_CHANGED"), "{err}");
}

// ── U11: ssh arguments ────────────────────────────────────────────────────────

#[test]
fn ssh_never_turns_host_key_checking_off_and_never_prompts() {
    for strict in [false, true] {
        let args = ssh_args(
            "host.example",
            "docker exec -i 'c' iris session IRIS -U 'USER'",
            strict,
        );
        let joined = args.join(" ");
        assert!(!joined.contains("StrictHostKeyChecking=no"), "{joined}");
        assert!(joined.contains("BatchMode=yes"), "{joined}");
        assert_eq!(args[args.len() - 2], "host.example");
        assert!(args[args.len() - 1].starts_with("docker exec"));
    }
}

#[test]
fn ssh_accepts_a_new_host_by_default_and_requires_a_known_one_when_strict() {
    let lax = ssh_args("h", "cmd", false).join(" ");
    let strict = ssh_args("h", "cmd", true).join(" ");
    assert!(lax.contains("StrictHostKeyChecking=accept-new"), "{lax}");
    assert!(strict.contains("StrictHostKeyChecking=yes"), "{strict}");
}

// ── the syntax guard answers before a missing container does ──────────────────

fn docker_only_cx() -> iris_agentic_dev_core::tools::exec::ExecContext {
    use iris_agentic_dev_core::iris::connection::{DiscoverySource, IrisConnection};
    use iris_agentic_dev_core::tools::exec::{ExecContext, IdentitySource};
    ExecContext {
        ssh_strict: false,
        conn: Arc::new(IrisConnection::new(
            "http://127.0.0.1:1",
            "USER",
            "u",
            "p",
            DiscoverySource::ExplicitFlag,
        )),
        client: Arc::new(IrisConnection::http_client().unwrap()),
        expected: None,
        source: IdentitySource::Primary,
        server: "default".into(),
        route: ExecRoute::DockerOnlyNoContainer,
        opt_in: None,
        named_server: false,
        env_container_set: false,
    }
}

#[tokio::test]
async fn block_syntax_is_refused_before_a_missing_container_is_reported() {
    use iris_agentic_dev_core::tools::exec::RunOpts;
    let mut opts = RunOpts::with_terminal(std::time::Duration::from_secs(5), "IrisDevRun");
    opts.check_block_syntax = true;
    let cx = docker_only_cx();
    let blocked = cx
        .run("If x=1 { Write 1 }", "USER", &opts)
        .await
        .unwrap_err();
    assert_eq!(blocked.code, "TERMINAL_SYNTAX_UNSUPPORTED", "{blocked}");
    let classic = cx.run("Write 1", "USER", &opts).await.unwrap_err();
    assert_eq!(classic.code, "NOPWS_NO_CONTAINER", "{classic}");
}

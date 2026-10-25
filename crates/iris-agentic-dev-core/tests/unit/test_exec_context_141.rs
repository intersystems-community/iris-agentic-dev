//! Spec 141 R2: the parts of `tools::exec` that need no running IRIS. Wire values that `check_config`
//! and every response carry, the refusal and failure shapes a handler returns, and what a context
//! reports when the server it targets does not answer.
//!
//! Nothing here stands in for IRIS. The context points at a closed port, so every HTTP attempt
//! fails the way a server that is down fails, and the assertions are about what iad reports.

use iris_agentic_dev_core::iris::connection::{DiscoverySource, IrisConnection};
use iris_agentic_dev_core::testing::EnvGuard;
use iris_agentic_dev_core::tools::exec::{
    attach_identity, is_identity_failure, ExecContext, ExecError, ExecPath, ExecRoute,
    IdentityReport, IdentitySource, MarkerError, OptInSource, RunOpts,
    ERR_DOCKER_TERMINAL_LOGIN_REQUIRED, ERR_NO_DOCKER_ROUTE_FOR_SERVER,
    ERR_SERVICE_ACCOUNT_HTTP_DENIED, ERR_SERVICE_ACCOUNT_IDENTITY_MISMATCH,
    ERR_SERVICE_ACCOUNT_LOGIN_FAILED, ERR_SERVICE_ACCOUNT_TERMINAL_DENIED,
};
use serde_json::json;
use std::sync::Arc;
use std::time::Duration;

// ── wire values ───────────────────────────────────────────────────────────────

#[test]
fn every_identity_source_has_its_documented_wire_value() {
    for (source, wire) in [
        (IdentitySource::ServiceAccountEnv, "service_account_env"),
        (
            IdentitySource::ServiceAccountServer,
            "service_account_server",
        ),
        (IdentitySource::Primary, "primary"),
        (IdentitySource::OsUserOptIn, "os_user_opt_in"),
    ] {
        assert_eq!(source.as_str(), wire);
    }
}

#[test]
fn every_opt_in_source_and_exec_path_has_its_documented_wire_value() {
    assert_eq!(OptInSource::Env.as_str(), "env");
    assert_eq!(OptInSource::HomeFile.as_str(), "home_file");
    for (path, wire) in [
        (ExecPath::Atelier, "atelier"),
        (ExecPath::DockerExecLocal, "docker_exec_local"),
        (ExecPath::DockerExecSsh, "docker_exec_ssh"),
        (ExecPath::WsTerminal, "ws_terminal"),
    ] {
        assert_eq!(path.as_str(), wire);
    }
}

/// A route reports the path its docker leg takes: SSH when a host is named, a local `docker exec`
/// otherwise, and the label `check_config` shows.
#[test]
fn a_route_names_its_path_and_its_label() {
    let local = ExecRoute::DockerOnly {
        container: "c".into(),
        ssh_host: None,
    };
    let ssh = ExecRoute::HttpThenDocker {
        container: "c".into(),
        ssh_host: Some("host".into()),
    };
    assert_eq!(local.path(), ExecPath::DockerExecLocal);
    assert_eq!(ssh.path(), ExecPath::DockerExecSsh);
    assert_eq!(ExecRoute::Http.path(), ExecPath::DockerExecLocal);

    assert_eq!(ExecRoute::Http.label(), "http");
    assert_eq!(ssh.label(), "http_then_docker");
    assert_eq!(local.label(), "docker_only");
    assert_eq!(ExecRoute::DockerOnlyNoContainer.label(), "docker_only");

    assert_eq!(local.container(), Some("c"));
    assert_eq!(ssh.ssh_host(), Some("host"));
    assert_eq!(ExecRoute::Http.container(), None);
    assert_eq!(ExecRoute::DockerOnlyNoContainer.ssh_host(), None);
}

/// Only a failure about who would run the code stops a fallback; every other code lets the caller
/// try the next path.
#[test]
fn only_identity_failures_are_final() {
    for code in [
        ERR_SERVICE_ACCOUNT_LOGIN_FAILED,
        ERR_SERVICE_ACCOUNT_IDENTITY_MISMATCH,
        ERR_SERVICE_ACCOUNT_TERMINAL_DENIED,
        ERR_DOCKER_TERMINAL_LOGIN_REQUIRED,
        ERR_SERVICE_ACCOUNT_HTTP_DENIED,
    ] {
        assert!(is_identity_failure(code), "{code}");
    }
    for code in ["TIMEOUT", "EXECUTION_FAILED", "CONTAINER_UNREACHABLE", ""] {
        assert!(!is_identity_failure(code), "{code}");
    }
}

// ── errors ────────────────────────────────────────────────────────────────────

fn report(user: &str, verified: bool) -> IdentityReport {
    IdentityReport {
        user: user.to_string(),
        source: IdentitySource::ServiceAccountEnv,
        path: ExecPath::Atelier,
        verified,
        server: "default".to_string(),
    }
}

#[test]
fn an_exec_error_displays_its_code_and_message_and_serialises_everything_it_holds() {
    let mut e = ExecError::new("SOME_CODE", "it broke");
    assert_eq!(e.to_string(), "SOME_CODE: it broke");
    let bare = e.to_json();
    assert_eq!(bare["success"], json!(false));
    assert_eq!(bare["error_code"], json!("SOME_CODE"));
    assert_eq!(bare["error"], json!("it broke"));
    assert!(bare.get("identity").is_none(), "no identity was recorded");

    e.extra.insert("execution_path".into(), json!("atelier"));
    e.identity = Some(Box::new(report("svc", false)));
    let full = e.to_json();
    assert_eq!(full["execution_path"], json!("atelier"));
    assert_eq!(full["identity"]["user"], json!("svc"));
    assert_eq!(full["identity"]["verified"], json!(false));
    assert_eq!(full["identity"]["source"], json!("service_account_env"));
}

/// Each way a terminal call's identity check fails has its own code and a message that says what
/// to do. `expected` names the account when there is one and falls back to a generic phrase.
#[test]
fn every_marker_error_has_a_code_and_a_remedy() {
    let cases = [
        (
            MarkerError::LoginFailed,
            ERR_SERVICE_ACCOUNT_LOGIN_FAILED,
            "be enabled",
        ),
        (
            MarkerError::TerminalLoginRequired,
            ERR_DOCKER_TERMINAL_LOGIN_REQUIRED,
            "operating-system",
        ),
        (
            MarkerError::TerminalDenied,
            ERR_SERVICE_ACCOUNT_TERMINAL_DENIED,
            "%Development:USE",
        ),
        (
            MarkerError::Mismatch {
                ran_as: "irisowner".into(),
            },
            ERR_SERVICE_ACCOUNT_IDENTITY_MISMATCH,
            "irisowner",
        ),
    ];
    for (err, code, remedy) in cases {
        assert_eq!(err.code(), code);
        assert!(
            err.message(None).contains(remedy),
            "{code}: {}",
            err.message(None)
        );
        assert!(err.to_string().starts_with(&format!("{code}: ")), "{err}");
        // Running as the OS user is an operator-only setting: the agent is not told how (FR-039).
        assert!(
            !err.message(None).contains("exec_as_os_user")
                && !err.message(None).contains("IAD_EXEC_AS_OS_USER"),
            "{code}: {}",
            err.message(None)
        );
    }
    let named = MarkerError::LoginFailed.message(Some("svc"));
    assert!(named.contains("as svc"), "{named}");
    assert!(MarkerError::LoginFailed
        .message(None)
        .contains("the service account"));
    assert!(MarkerError::Mismatch { ran_as: "x".into() }
        .message(Some("svc"))
        .contains("not as svc"));
}

// ── a context ─────────────────────────────────────────────────────────────────

fn closed_port_conn() -> Arc<IrisConnection> {
    Arc::new(IrisConnection::new(
        "http://127.0.0.1:1",
        "USER",
        "u",
        "p",
        DiscoverySource::ExplicitFlag,
    ))
}

fn context(route: ExecRoute) -> ExecContext {
    ExecContext {
        ssh_strict: false,
        conn: closed_port_conn(),
        client: Arc::new(IrisConnection::http_client().unwrap()),
        expected: None,
        source: IdentitySource::Primary,
        server: "default".into(),
        route,
        opt_in: None,
        named_server: false,
        env_container_set: false,
    }
}

fn opts(terminal: bool) -> RunOpts<'static> {
    if terminal {
        RunOpts::with_terminal(Duration::from_secs(20), "IrisDevRun")
    } else {
        RunOpts::http_only(Duration::from_secs(20), "IrisDevRun")
    }
}

/// No docker leg and a server that does not answer: the failure says HTTP failed and why no
/// fallback ran, and it names who would have run the code.
#[tokio::test]
async fn a_dead_server_with_no_docker_route_reports_http_failed_and_the_identity() {
    let mut cx = context(ExecRoute::Http);
    cx.expected = Some("svc".into());
    cx.source = IdentitySource::ServiceAccountEnv;
    let e = cx.run("Write 1", "USER", &opts(true)).await.unwrap_err();
    assert_eq!(e.code, "HTTP_EXECUTION_FAILED", "{e}");
    assert!(e.message.contains("No docker fallback available"), "{e}");
    assert!(e.extra.contains_key("http_error"));
    assert!(!e.extra.contains_key("no_docker_route"));
    let id = e.identity.expect("the error names who would have run it");
    assert_eq!(id.user, "svc");
    assert_eq!(id.source, IdentitySource::ServiceAccountEnv);
    assert_eq!(id.path, ExecPath::Atelier);
    assert!(!id.verified);
}

/// A named server that shares the launch settings' `IRIS_CONTAINER` does not get a docker leg, and
/// the response says that is why.
#[tokio::test]
async fn a_named_server_without_its_own_container_says_why_docker_was_not_used() {
    let mut cx = context(ExecRoute::Http);
    cx.named_server = true;
    cx.env_container_set = true;
    cx.server = "prod2".into();
    let e = cx.run("Write 1", "USER", &opts(true)).await.unwrap_err();
    assert_eq!(e.code, "HTTP_EXECUTION_FAILED", "{e}");
    assert!(e.message.contains(ERR_NO_DOCKER_ROUTE_FOR_SERVER), "{e}");
    assert!(e.message.contains("`prod2`"), "{e}");
    assert_eq!(
        e.extra.get("no_docker_route"),
        Some(&json!(ERR_NO_DOCKER_ROUTE_FOR_SERVER))
    );
    assert_eq!(e.identity.expect("named").server, "prod2");
}

/// A tool with no docker leg gets the plain HTTP error, on either route.
#[tokio::test]
async fn a_tool_with_no_terminal_leg_reports_the_http_error_as_it_is() {
    for route in [
        ExecRoute::Http,
        ExecRoute::HttpThenDocker {
            container: "c".into(),
            ssh_host: None,
        },
    ] {
        let e = context(route)
            .run("Write 1", "USER", &opts(false))
            .await
            .unwrap_err();
        assert_eq!(e.code, "EXECUTION_FAILED", "{e}");
        assert!(!e.message.is_empty());
        assert_eq!(e.identity.expect("named").path, ExecPath::Atelier);
    }
}

#[tokio::test]
async fn a_call_that_does_not_finish_in_time_reports_a_timeout() {
    let mut o = opts(true);
    o.timeout = Duration::ZERO;
    let e = context(ExecRoute::Http)
        .run("Write 1", "USER", &o)
        .await
        .unwrap_err();
    assert_eq!(e.code, "TIMEOUT", "{e}");
    assert!(e.message.contains("timed out after 0s"), "{e}");
}

/// The syntax guard answers on a docker-only route before any container is touched, and the error
/// names the path the docker leg would have used: local, or SSH when a host is set.
#[tokio::test]
async fn block_syntax_on_a_docker_only_route_is_refused_with_the_docker_path() {
    let mut o = opts(true);
    o.check_block_syntax = true;
    for (ssh_host, path) in [
        (None, ExecPath::DockerExecLocal),
        (Some("host".to_string()), ExecPath::DockerExecSsh),
    ] {
        let cx = context(ExecRoute::DockerOnly {
            container: "no-such-container".into(),
            ssh_host,
        });
        let e = cx.run("If x=1 { Write 1 }", "USER", &o).await.unwrap_err();
        assert_eq!(e.code, "TERMINAL_SYNTAX_UNSUPPORTED", "{e}");
        assert_eq!(e.identity.expect("named").path, path);
    }
}

/// With a container, a docker-first call tries the terminal and then HTTP; when both fail the
/// error carries both reasons. A terminal refusal about identity or a timeout is final instead.
#[tokio::test]
async fn docker_first_reports_both_legs_when_both_fail() {
    let mut o = opts(true);
    o.check_block_syntax = true;
    let cx = context(ExecRoute::HttpThenDocker {
        container: "no-such-container".into(),
        ssh_host: None,
    });
    let e = cx
        .run_docker_first("If x=1 { Write 1 }", "USER", &o)
        .await
        .unwrap_err();
    assert_eq!(e.code, "DOCKER_REQUIRED", "{e}");
    assert!(e.message.contains("docker exec failed"), "{e}");
    assert!(e.message.contains("HTTP fallback also failed"), "{e}");
    assert!(e.identity.is_some());

    o.timeout = Duration::ZERO;
    let e = cx
        .run_docker_first("If x=1 { Write 1 }", "USER", &o)
        .await
        .unwrap_err();
    assert_eq!(e.code, "TIMEOUT", "{e}");
}

/// With no container the docker-first call is the plain run.
#[tokio::test]
async fn docker_first_without_a_container_is_a_plain_run() {
    let e = context(ExecRoute::Http)
        .run_docker_first("Write 1", "USER", &opts(true))
        .await
        .unwrap_err();
    assert_eq!(e.code, "HTTP_EXECUTION_FAILED", "{e}");
}

#[test]
fn a_primary_context_runs_as_the_connection_and_reports_it_unverified() {
    let _env = EnvGuard::without("IRIS_CONTAINER");
    let conn = Arc::new(IrisConnection::new(
        "http://localhost:52780",
        "USER",
        "conn_user",
        "p",
        DiscoverySource::ExplicitFlag,
    ));
    let cx = ExecContext::primary(conn, Arc::new(IrisConnection::http_client().unwrap()));
    assert!(cx.expected.is_none());
    assert_eq!(cx.source, IdentitySource::Primary);
    assert_eq!(cx.server, "default");
    assert!(!cx.named_server);
    assert!(!cx.env_container_set);
    assert_eq!(cx.route, ExecRoute::Http);
    let r = cx.helper_report();
    assert_eq!(r.user, "conn_user");
    assert_eq!(r.path, ExecPath::Atelier);
    assert!(!r.verified, "the primary's answer is not checked");
}

#[test]
fn a_context_with_a_service_account_reports_it_verified() {
    let mut cx = context(ExecRoute::Http);
    cx.expected = Some("svc".into());
    cx.source = IdentitySource::ServiceAccountServer;
    let r = cx.helper_report();
    assert_eq!(r.user, "svc");
    assert_eq!(r.source, IdentitySource::ServiceAccountServer);
    assert!(r.verified);
}

// ── attaching the identity to a result ────────────────────────────────────────

#[test]
fn the_identity_is_added_to_an_object_result_and_the_error_flag_survives() {
    use rmcp::model::CallToolResult;
    let id = report("svc", true);

    let ok = attach_identity(
        Ok(CallToolResult::structured(json!({"success": true}))),
        &id,
    )
    .unwrap();
    let body = ok.structured_content.expect("structured");
    assert_eq!(body["success"], json!(true));
    assert_eq!(body["identity"]["user"], json!("svc"));
    assert_ne!(ok.is_error, Some(true));

    let err = attach_identity(
        Ok(CallToolResult::structured_error(json!({"success": false}))),
        &id,
    )
    .unwrap();
    assert_eq!(err.is_error, Some(true));
    assert_eq!(
        err.structured_content.expect("structured")["identity"]["verified"],
        json!(true)
    );
}

#[test]
fn a_result_that_is_not_an_object_comes_back_as_it_was() {
    use rmcp::model::CallToolResult;
    let id = report("svc", true);
    let array = attach_identity(Ok(CallToolResult::structured(json!([1, 2]))), &id).unwrap();
    assert_eq!(array.structured_content, Some(json!([1, 2])));

    let failed = attach_identity(Err(rmcp::ErrorData::internal_error("x", None)), &id);
    assert!(failed.is_err());
}

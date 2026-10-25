//! Spec 141 R2: how a terminal session is made to run as the service account (T-R2-U04 to U06).
//!
//! `docker exec` and SSH reach `iris session`, which takes no username under OS authentication.
//! The first stdin line therefore drops to the service account inside IRIS, and the answer names
//! who ran. These are the pure halves of that: the line, the marker parser, and the generator's
//! `$USERNAME` suffix. What IRIS does with the line is `test_login_guard_141_live`.

use iris_agentic_dev_core::iris::connection::{query_output_identified, IrisConnection};
use iris_agentic_dev_core::tools::exec::{
    build_login_guard, split_identity_marker, validate_service_username, MarkerError,
    IDENTITY_PROBE_LINE,
};

// ── U04: the guard line ───────────────────────────────────────────────────────

#[test]
fn guard_is_one_line_with_no_braces() {
    let g = build_login_guard("iadsvc");
    assert!(!g.contains('\n') && !g.contains('\r'), "{g:?}");
    assert!(
        !g.contains('{') && !g.contains('}'),
        "the terminal does not take block syntax: {g}"
    );
}

#[test]
fn guard_halts_when_the_login_fails_and_when_the_user_is_not_the_one_asked_for() {
    let g = build_login_guard("iadsvc");
    assert!(g.contains("$SYSTEM.Security.Login(\"iadsvc\")"), "{g}");
    assert!(
        g.contains("Halt:'%iad"),
        "a failed Login must end the process: {g}"
    );
    assert!(
        g.contains("$ZCVT($USERNAME,\"U\")'=$ZCVT(\"iadsvc\",\"U\")"),
        "Login can succeed and leave another user: {g}"
    );
    assert!(
        g.trim_end().ends_with("Write \"IAD-ID:\",$USERNAME,!"),
        "the guard ends by naming who runs: {g}"
    );
}

#[test]
fn guard_sends_no_password() {
    let g = build_login_guard("iadsvc");
    assert!(
        g.matches("Login(").count() == 1 && !g.contains("Login(\"iadsvc\","),
        "the one-argument form only; the two-argument form puts a password on stdin: {g}"
    );
}

#[test]
fn guard_escapes_a_double_quote_in_the_user_name() {
    let g = build_login_guard("a\"b");
    assert!(g.contains("Login(\"a\"\"b\")"), "{g}");
    assert!(!g.contains("Login(\"a\"b\")"), "{g}");
}

#[test]
fn a_service_user_with_a_line_break_is_refused_before_it_can_become_a_stdin_line() {
    for bad in ["svc\nHalt", "svc\r", "svc\u{0}", "", "   "] {
        let err = validate_service_username(bad).expect_err(&format!("{bad:?} must be refused"));
        assert!(err.contains("SERVICE_ACCOUNT_INVALID"), "{err}");
    }
    assert!(validate_service_username("iadsvc").is_ok());
    assert!(validate_service_username("_SYSTEM").is_ok());
}

#[test]
fn the_probe_line_for_a_call_with_no_service_account_names_the_user_too() {
    assert_eq!(IDENTITY_PROBE_LINE, "Write \"IAD-ID:\",$USERNAME,!");
}

// ── U05: reading the marker ───────────────────────────────────────────────────

fn session(lines: &[&str]) -> String {
    let mut s = String::from("Node: abc, Instance: IRIS\n\nUSER>\n");
    for l in lines {
        s.push_str(l);
        s.push_str("\n\nUSER>\n");
    }
    s
}

#[test]
fn the_marker_for_the_expected_user_is_removed_from_the_output() {
    let raw = session(&["IAD-ID:IADSVC", "hello"]);
    let m = split_identity_marker(&raw, Some("iadsvc")).expect("same user, any case");
    assert_eq!(m.user.as_deref(), Some("IADSVC"));
    assert!(!m.rest.contains("IAD-ID:"), "{}", m.rest);
    assert!(m.rest.contains("hello"), "{}", m.rest);
}

#[test]
fn no_marker_with_a_service_account_expected_is_a_login_failure() {
    let raw = session(&["hello"]);
    let err = split_identity_marker(&raw, Some("iadsvc")).unwrap_err();
    assert_eq!(err, MarkerError::LoginFailed);
    assert_eq!(err.code(), "SERVICE_ACCOUNT_LOGIN_FAILED");
}

#[test]
fn a_password_prompt_means_the_terminal_wants_a_login() {
    for text in ["Username: \nPassword: ", "Username:", "Access Denied"] {
        let err = split_identity_marker(text, Some("iadsvc")).unwrap_err();
        assert_eq!(err, MarkerError::TerminalLoginRequired, "{text:?}");
        assert_eq!(err.code(), "DOCKER_TERMINAL_LOGIN_REQUIRED");
    }
}

#[test]
fn a_marker_followed_by_access_denied_means_the_account_cannot_use_the_terminal() {
    // Measured on 2026.2: after Login the terminal ends the session unless the account holds
    // %Development:USE. The marker printed, the agent's code did not run.
    let raw = "Node: abc, Instance: IRIS\n\nUSER>\nIAD-ID:iadsvc\n\nAccess Denied\n";
    let err = split_identity_marker(raw, Some("iadsvc")).unwrap_err();
    assert_eq!(err, MarkerError::TerminalDenied);
    assert_eq!(err.code(), "SERVICE_ACCOUNT_TERMINAL_DENIED");
}

#[test]
fn a_marker_for_another_user_is_a_mismatch_that_names_who_ran() {
    let raw = session(&["IAD-ID:irisowner", "hello"]);
    let err = split_identity_marker(&raw, Some("iadsvc")).unwrap_err();
    assert_eq!(
        err,
        MarkerError::Mismatch {
            ran_as: "irisowner".into()
        }
    );
    assert_eq!(err.code(), "SERVICE_ACCOUNT_IDENTITY_MISMATCH");
}

#[test]
fn with_no_service_account_the_marker_reports_the_os_user_and_a_missing_one_is_not_an_error() {
    let raw = session(&["IAD-ID:irisowner", "hello"]);
    let m = split_identity_marker(&raw, None).unwrap();
    assert_eq!(m.user.as_deref(), Some("irisowner"));
    assert!(!m.rest.contains("IAD-ID:"));

    let none = split_identity_marker(&session(&["hello"]), None).unwrap();
    assert_eq!(none.user, None);
}

#[test]
fn agent_output_that_imitates_the_marker_does_not_pick_the_identity() {
    // Only the first marker line counts: the guard is the first line sent, so it is the first
    // thing written.
    let raw = session(&["IAD-ID:iadsvc", "IAD-ID:irisowner"]);
    let m = split_identity_marker(&raw, Some("iadsvc")).unwrap();
    assert_eq!(m.user.as_deref(), Some("iadsvc"));
    assert!(m.rest.contains("IAD-ID:irisowner"), "{}", m.rest);
}

// ── U06: the generator's $USERNAME suffix ─────────────────────────────────────

#[test]
fn the_generator_returns_the_user_name_ahead_of_the_output() {
    let lines = IrisConnection::build_exec_class_for_test("IrisDevTmp.X", "", "Write 1");
    let quit = lines
        .iter()
        .find(|l| l.trim_start().starts_with("Quit $Char(2)"))
        .expect("the executor ends with a Quit that carries the result");
    assert_eq!(quit.trim(), "Quit $Char(2)_$USERNAME_$Char(3)_out");
}

fn body(result: &str) -> String {
    serde_json::json!({"status": {"errors": []}, "result": {"content": [{"result": result}]}})
        .to_string()
}

#[test]
fn the_user_name_and_the_output_are_split_at_the_first_separator() {
    let (out, ran_as) = query_output_identified(200, &body("\u{2}iadsvc\u{3}hello")).unwrap();
    assert_eq!(out, "hello");
    assert_eq!(ran_as.as_deref(), Some("iadsvc"));
}

#[test]
fn a_separator_inside_the_output_stays_in_the_output() {
    let (out, ran_as) = query_output_identified(200, &body("\u{2}svc\u{3}a\u{3}b")).unwrap();
    assert_eq!(out, "a\u{3}b");
    assert_eq!(ran_as.as_deref(), Some("svc"));
}

#[test]
fn an_answer_with_no_separator_has_no_identity_and_keeps_its_text() {
    let (out, ran_as) = query_output_identified(200, &body("\u{2}plain")).unwrap();
    assert_eq!(out, "plain");
    assert_eq!(ran_as, None);
}

#[test]
fn a_failed_status_is_still_an_error() {
    assert!(query_output_identified(401, "denied").is_err());
}

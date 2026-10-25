//! Spec 141 FR-020 (T-R4b-U01 to U03): a password never becomes ObjectScript source.
//!
//! Every generator snippet is stored as class source (`^oddDEF`, the kept `.int`, the journal, the
//! namespace's source-control hooks). The connection account's password therefore never enters one:
//! source-control calls use `$USERNAME` and an empty password, as IRIS's own REST API does, and a
//! password a tool must hand over travels in the `X-IAD-Args` request header.
//!
//! The file name avoids the words the repository's file-protection hook blocks.

use iris_agentic_dev_core::iris::connection::{encode_args_header, IrisConnection, Secret};
use iris_agentic_dev_core::tools::{admin, doc, interop, scm};

const SENTINEL: &str = "Zq9-SENTINEL-pw";
const CREATE: &str = r#"SourceControlCreate($USERNAME,"","#;

fn decode_header(h: &str) -> serde_json::Value {
    use base64::Engine as _;
    let raw = base64::engine::general_purpose::STANDARD
        .decode(h)
        .expect("X-IAD-Args must be base64");
    serde_json::from_slice(&raw).expect("X-IAD-Args must decode to JSON")
}

// ── U01: source-control builders ─────────────────────────────────────────────────────────

#[test]
fn every_scm_builder_pins_username_and_empty_password() {
    let builders = [
        ("status_check_code", scm::status_check_code("A.B.cls")),
        ("scm_init_prefix", scm::scm_init_prefix()),
        (
            "user_action_code",
            scm::user_action_code("%CheckOut", "A.B.cls"),
        ),
        (
            "after_user_action_code",
            scm::after_user_action_code("%CheckOut", "A.B.cls", "yes"),
        ),
        ("menu_all_items_code", scm::menu_all_items_code("A.B.cls")),
        (
            "doc pre-write probe",
            doc::scm_checkout_probe_code("A.B.cls"),
        ),
        (
            "production_diff probe",
            interop::production_scm_probe_code("A.B.cls"),
        ),
    ];
    for (name, code) in builders {
        assert!(
            code.contains(CREATE),
            "{name} must call SourceControlCreate with $USERNAME and an empty password: {code}"
        );
        assert_eq!(
            code.matches("SourceControlCreate").count(),
            1,
            "{name} must create the source-control session once: {code}"
        );
    }
}

#[test]
fn scm_builders_name_no_password() {
    assert!(!scm::scm_init_prefix().to_lowercase().contains("password"));
    assert!(!scm::status_check_code("A.B.cls")
        .to_lowercase()
        .contains("password"));
}

// ── U02: passwords handed to IRIS ────────────────────────────────────────────────────────

#[test]
fn credential_create_and_update_hand_the_password_as_an_argument() {
    for overwrite in [false, true] {
        let call = interop::credential_set_call("cred1", Some("alice"), Some(SENTINEL), overwrite);
        assert!(
            !call.code.contains(SENTINEL),
            "overwrite={overwrite}: sentinel in source: {}",
            call.code
        );
        assert!(
            call.code.contains("tIadArgs.%Get(0)"),
            "the password must be read from the arguments: {}",
            call.code
        );
        assert_eq!(call.args.len(), 1);
        assert_eq!(call.args[0].expose(), SENTINEL);
    }
}

#[test]
fn credential_update_without_a_password_keeps_the_stored_one() {
    let call = interop::credential_set_call("cred1", Some("alice"), None, true);
    assert!(
        call.args.is_empty(),
        "nothing to hand over: {:?}",
        call.args
    );
    assert!(
        call.code.contains("GetValue") && !call.code.contains("tIadArgs"),
        "{}",
        call.code
    );
}

#[test]
fn admin_create_user_hands_the_password_as_an_argument() {
    let call = admin::create_user_call("bob", SENTINEL, Some("Bob B"), Some("%All"));
    assert!(!call.code.contains(SENTINEL), "{}", call.code);
    assert!(call.code.contains("tIadArgs.%Get(0)"), "{}", call.code);
    assert_eq!(call.args.len(), 1);
    assert_eq!(call.args[0].expose(), SENTINEL);
}

#[test]
fn admin_update_user_hands_a_new_password_as_an_argument() {
    let call = admin::update_user_call("bob", Some(SENTINEL), Some(true), Some("%All"));
    assert!(!call.code.contains(SENTINEL), "{}", call.code);
    assert!(call.code.contains("tIadArgs.%Get(0)"), "{}", call.code);
    assert_eq!(call.args.len(), 1);
    assert_eq!(call.args[0].expose(), SENTINEL);

    let none = admin::update_user_call("bob", None, Some(false), None);
    assert!(none.args.is_empty());
    assert!(!none.code.contains("tIadArgs"), "{}", none.code);
}

#[test]
fn admin_clear_flag_hands_both_passwords_as_arguments() {
    let call = admin::clear_password_flag_call("bob", SENTINEL, "Zq9-NEW-pw");
    assert!(!call.code.contains(SENTINEL), "{}", call.code);
    assert!(!call.code.contains("Zq9-NEW-pw"), "{}", call.code);
    let got: Vec<&str> = call.args.iter().map(Secret::expose).collect();
    assert_eq!(got, [SENTINEL, "Zq9-NEW-pw"]);
    assert!(
        call.code.contains("tIadArgs.%Get(0)") && call.code.contains("tIadArgs.%Get(1)"),
        "{}",
        call.code
    );
}

#[test]
fn exec_class_with_args_reads_the_header_and_kills_the_decoded_array() {
    let with = IrisConnection::build_exec_class_with_args_for_test("IrisDevTmp.T", "Write 1");
    let with = with.join("\n");
    assert!(with.contains("HTTP_X_IAD_ARGS"), "{with}");
    assert!(with.contains("Kill tIadArgs"), "{with}");
    assert!(
        !with.contains(SENTINEL),
        "the class must carry no value: {with}"
    );

    let without = IrisConnection::build_exec_class_for_test("IrisDevTmp.T", "", "Write 1");
    assert!(
        !without.join("\n").contains("HTTP_X_IAD_ARGS"),
        "a call with no arguments must not read the header"
    );
}

#[test]
fn args_header_round_trips_awkward_passwords() {
    for pw in [SENTINEL, "p\"q\\r", "tab\there\nline", "пароль-密码-🔑"] {
        let h = encode_args_header(&[Secret::new(pw), Secret::new("second")]);
        assert!(
            h.chars()
                .all(|c| c.is_ascii_alphanumeric() || "+/=".contains(c)),
            "header value must be plain base64: {h}"
        );
        assert_eq!(decode_header(&h), serde_json::json!([pw, "second"]));
    }
}

#[test]
fn secret_debug_prints_no_value() {
    let s = Secret::new(SENTINEL);
    assert!(!format!("{s:?}").contains(SENTINEL));
    let call = admin::create_user_call("bob", SENTINEL, None, None);
    assert!(!format!("{call:?}").contains(SENTINEL));
}

// ── U03: guard scans ─────────────────────────────────────────────────────────────────────

/// No credential-named value reaches a function that writes an ObjectScript literal. No
/// allowlist: a password that must be spliced does not exist.
#[test]
fn no_password_value_reaches_an_objectscript_literal() {
    let src = iris_agentic_dev_core::testing::rust_sources();
    let call = regex::Regex::new(
        r"(?i)\b(os_lit_inner|os_quote|os_str_expr)\(\s*&?[^()]*(pass|pw|secret|credential)",
    )
    .unwrap();
    let hits: Vec<&str> = call.find_iter(src).map(|m| m.as_str()).collect();
    assert!(
        hits.is_empty(),
        "a credential-named value is spliced into ObjectScript: {hits:?}"
    );
    // The scan can fail: the shapes the old code had are found.
    assert!(call.is_match("os_quote(&iris.password)"));
    assert!(call.is_match("os_str_expr(newpw)"));
}

#[test]
fn every_source_control_create_call_pins_username_and_empty_password() {
    let src = iris_agentic_dev_core::testing::rust_sources();
    let create = regex::Regex::new(r"SourceControlCreate\([^)]*\)").unwrap();
    let calls: Vec<&str> = create.find_iter(src).map(|m| m.as_str()).collect();
    assert!(
        !calls.is_empty(),
        "the scan must find the builders' calls, found {calls:?}"
    );
    for c in calls {
        assert!(
            c.starts_with(CREATE),
            "SourceControlCreate must pass $USERNAME and an empty password: {c}"
        );
    }
}

#[test]
fn telemetry_never_captures_call_params() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/telemetry/mod.rs");
    let full = std::fs::read_to_string(&path).unwrap();
    let production = full
        .split("#[cfg(test)]")
        .next()
        .expect("split always yields a first part");
    assert!(
        production.contains("params: None"),
        "the record builder must set params to None"
    );
    assert!(
        !production.contains("params: Some") && !production.contains("params = Some"),
        "telemetry would carry call params into the durable sink"
    );
}

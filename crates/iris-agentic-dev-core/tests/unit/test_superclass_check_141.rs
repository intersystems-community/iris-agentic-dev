//! Spec 141 FR-015a (T209): a compile whose target has an out-of-date superclass outside the
//! library packages is refused, naming the superclass to compile first.
//!
//! IRIS compiles a target's out-of-date superclasses with it, and the stored-source check reads
//! only the classes named in the compile. These tests pin the pure parts: the library rule, the
//! walk's reply format, the refusal body, and the fail-closed paths when the server does not
//! answer. The walk itself runs on live IRIS in `test_superclass_141_live`.

use iris_agentic_dev_core::iris::connection::{DiscoverySource, IrisConnection};
use iris_agentic_dev_core::policy::refusal::REFUSAL_CODES;
use iris_agentic_dev_core::testing::EnvGuard;
use iris_agentic_dev_core::tools::stored_check::Outcome;
use iris_agentic_dev_core::tools::superclass_check::{
    check_exec, check_http, class_names, is_library_class, parse_walk, refusal_for, walk_code,
    Stale, Walk, ERR_SUPERCLASS_STALE, ERR_SUPERCLASS_UNCHECKED, WALK_CHUNK,
};

fn closed_port() -> (IrisConnection, reqwest::Client) {
    (
        IrisConnection::new(
            "http://127.0.0.1:1",
            "USER",
            "u",
            "p",
            DiscoverySource::ExplicitFlag,
        ),
        IrisConnection::http_client().unwrap(),
    )
}

fn strings(v: &[&str]) -> Vec<String> {
    v.iter().map(|s| s.to_string()).collect()
}

// ── the library rule ──────────────────────────────────────────────────────────

#[test]
fn library_packages_are_not_checked() {
    for name in [
        "%Persistent",
        "%Library.Persistent",
        "%Dictionary.ClassDefinition",
        "Ens.BusinessOperation",
        "Ens.Host",
        "EnsLib.HTTP.OutboundAdapter",
        "EnsPortal.Template.standardPage",
        "HS.FHIRServer.Storage.Json.InteractionsStrategy",
        "HSLIB.Foo",
        "HSMOD.Foo",
    ] {
        assert!(is_library_class(name), "{name} is a library class");
    }
}

/// The conservative reading of `Ens*` and `HS*`: a package is library only when its whole first
/// segment is one of the shipped ones. A user package that merely starts with the letters is not,
/// nor is the package HealthShare reserves for the customer's own classes, nor a name in another
/// case (class names are case sensitive).
#[test]
fn a_user_class_is_checked_even_when_its_name_starts_like_a_library() {
    for name in [
        "MyApp.Pred",
        "Pred2",
        "User.Pred2",
        "IadSuper.Pred",
        "EnsembleDemo.Foo",
        "EnsLibrary.Foo",
        "HSApp.Foo",
        "HSCUSTOM.Foo",
        "HS.Local.Foo",
        "HS.Local.Deep.Foo",
        "ens.BusinessOperation",
        "hs.Foo",
        "Ens",
        "",
    ] {
        assert!(!is_library_class(name), "{name:?} is a user class");
    }
}

#[test]
fn class_documents_name_their_classes_and_nothing_else_does() {
    assert_eq!(
        class_names(&strings(&[
            "A.B.cls", "C.mac", "D.inc", "E.int", "X.CLS", "Plain"
        ])),
        strings(&["A.B", "User.X", "User.Plain"])
    );
    assert!(class_names(&strings(&["r.mac"])).is_empty());
}

// ── the walk's code and its reply ─────────────────────────────────────────────

#[test]
fn the_walk_names_the_classes_quoted_and_uses_the_compilers_own_test() {
    let code = walk_code(&strings(&["My.A", "Quo\"te"]));
    assert!(code.contains("\"My.A,Quo\"\"te\""), "{code}");
    assert!(
        code.contains("$SYSTEM.OBJ.IsUpToDate("),
        "the up-to-date test is the compiler's own: {code}"
    );
    assert!(code.contains("-DONE~"), "{code}");
    assert!(!code.contains('\n'), "the terminal reads one line: {code}");
    // The rule the Rust side applies is the one the snippet applies.
    for prefix in ["Ens.", "EnsLib.", "HS.", "HS.Local."] {
        assert!(code.contains(prefix), "{prefix} missing from {code}");
    }
}

#[test]
fn a_reply_with_no_stale_superclass_and_the_end_marker_is_clear() {
    assert_eq!(parse_walk("\n~IAD-DONE~\n", &[]), Walk::Clear);
    // Terminal noise around the markers does not matter.
    assert_eq!(
        parse_walk("USER>\r\n\r\n~IAD-DONE~\r\nUSER>", &[]),
        Walk::Clear
    );
}

#[test]
fn stale_superclasses_come_back_with_the_class_that_needs_them() {
    let out = "~IAD-STALE~ My.Pred My.Child\n~IAD-STALE~ My.Grand My.Pred\n~IAD-DONE~\n";
    assert_eq!(
        parse_walk(out, &strings(&["My.Child"])),
        Walk::Stale(vec![
            Stale {
                superclass: "My.Pred".into(),
                needed_by: "My.Child".into()
            },
            Stale {
                superclass: "My.Grand".into(),
                needed_by: "My.Pred".into()
            },
        ])
    );
}

/// A superclass that is itself a target is compiled by this call, and the stored-source check read
/// it. Naming it stale would refuse a compile of a whole package.
#[test]
fn a_stale_superclass_that_is_also_a_target_is_not_a_finding() {
    let out = "~IAD-STALE~ My.Pred My.Child\n~IAD-DONE~\n";
    assert_eq!(
        parse_walk(out, &strings(&["My.Child", "My.Pred"])),
        Walk::Clear
    );
}

#[test]
fn a_library_class_the_snippet_reported_is_not_a_finding() {
    let out =
        "~IAD-STALE~ Ens.Host My.Child\n~IAD-STALE~ %Library.Persistent My.Child\n~IAD-DONE~\n";
    assert_eq!(parse_walk(out, &strings(&["My.Child"])), Walk::Clear);
}

#[test]
fn a_superclass_named_twice_is_named_once() {
    let out = "~IAD-STALE~ My.Pred My.A\n~IAD-STALE~ My.Pred My.B\n~IAD-DONE~\n";
    match parse_walk(out, &[]) {
        Walk::Stale(list) => assert_eq!(list.len(), 1, "{list:?}"),
        other => panic!("{other:?}"),
    }
}

/// Fail closed: an answer that is not a finished walk is not a clear one.
#[test]
fn a_reply_that_is_not_a_finished_walk_is_a_failure() {
    for out in [
        "",
        "USER>",
        "~IAD-ERROR~ could not open the definition of My.A\n",
        "~IAD-STALE~ My.Pred My.Child\n", // cut off before the end marker
        "ERROR: <PROTECT> %Dictionary.ClassDefinition",
    ] {
        match parse_walk(out, &[]) {
            Walk::Failed(_) => {}
            Walk::Stale(_) if out.contains("~IAD-STALE~") => {}
            other => panic!("{out:?} must not read as {other:?}"),
        }
    }
    match parse_walk("~IAD-ERROR~ could not open My.A\n", &[]) {
        Walk::Failed(reason) => assert!(reason.contains("could not open My.A"), "{reason}"),
        other => panic!("{other:?}"),
    }
}

#[test]
fn a_finding_wins_over_a_walk_that_broke_later() {
    let out = "~IAD-STALE~ My.Pred My.Child\n~IAD-ERROR~ boom\n";
    assert!(matches!(parse_walk(out, &[]), Walk::Stale(_)));
}

// ── the refusal ───────────────────────────────────────────────────────────────

#[test]
fn the_refusal_names_the_superclass_and_says_to_compile_it_first() {
    let v = refusal_for(
        &[Stale {
            superclass: "My.Pred".into(),
            needed_by: "My.Child".into(),
        }],
        "USER",
    );
    assert_eq!(v["success"], serde_json::json!(false));
    assert_eq!(v["error_code"], serde_json::json!(ERR_SUPERCLASS_STALE));
    assert_eq!(ERR_SUPERCLASS_STALE, "COMPILE_SUPERCLASS_STALE");
    assert_eq!(v["superclasses"], serde_json::json!(["My.Pred"]));
    let msg = v["message"].as_str().unwrap();
    assert!(msg.contains("My.Pred"), "{msg}");
    assert!(msg.contains("My.Child"), "{msg}");
    assert!(msg.contains("compile"), "{msg}");
    assert!(msg.contains("Nothing was compiled"), "{msg}");
    let fix = v["remediation"].as_str().unwrap();
    assert!(fix.contains("iris_compile"), "{fix}");
    assert!(fix.contains("My.Pred"), "{fix}");
    // The text must not hand the agent a way round the rule.
    assert!(!msg.to_lowercase().contains("compile_time_gate"), "{msg}");
}

#[test]
fn the_refusal_lists_every_stale_superclass_once_in_order() {
    let v = refusal_for(
        &[
            Stale {
                superclass: "My.Pred".into(),
                needed_by: "My.Child".into(),
            },
            Stale {
                superclass: "My.Grand".into(),
                needed_by: "My.Pred".into(),
            },
        ],
        "USER",
    );
    assert_eq!(
        v["superclasses"],
        serde_json::json!(["My.Pred", "My.Grand"])
    );
    let msg = v["message"].as_str().unwrap();
    assert!(msg.contains("My.Pred") && msg.contains("My.Grand"), "{msg}");
}

#[test]
fn the_stale_superclass_code_is_a_registered_refusal() {
    assert!(
        REFUSAL_CODES.contains(&ERR_SUPERCLASS_STALE),
        "COMPILE_SUPERCLASS_STALE must be in REFUSAL_CODES"
    );
    assert!(
        !REFUSAL_CODES.contains(&ERR_SUPERCLASS_UNCHECKED),
        "a check that could not run is an error, like STORED_SOURCE_UNREADABLE"
    );
}

// ── fail closed when the server does not answer ───────────────────────────────

#[tokio::test]
async fn http_walk_against_a_dead_server_is_refused_not_passed() {
    let (iris, client) = closed_port();
    match check_http(&iris, &client, "USER", &strings(&["My.A.cls"])).await {
        Outcome::Refused(v) => {
            assert_eq!(v["error_code"], serde_json::json!(ERR_SUPERCLASS_UNCHECKED));
            assert!(v["message"]
                .as_str()
                .unwrap()
                .contains("Nothing was compiled"));
        }
        other => panic!("a walk that could not run must refuse: {other:?}"),
    }
}

#[tokio::test]
async fn exec_walk_with_no_container_is_a_transport_failure() {
    let (iris, _) = closed_port();
    let _env = EnvGuard::without("IRIS_CONTAINER");
    for ssh in [None, Some("host")] {
        match check_exec(&iris, ssh, "USER", &strings(&["My.A.cls"])).await {
            Outcome::Transport(msg) => assert!(msg.contains("DOCKER_REQUIRED"), "{msg}"),
            other => panic!("expected transport, got {other:?}"),
        }
    }
}

/// Only class documents have superclasses: a routine or include target needs no walk, and so
/// no server.
#[tokio::test]
async fn no_class_target_means_no_walk() {
    let (iris, client) = closed_port();
    let docs = strings(&["r.mac", "i.inc"]);
    assert!(matches!(
        check_http(&iris, &client, "USER", &docs).await,
        Outcome::Clear
    ));
    let _env = EnvGuard::without("IRIS_CONTAINER");
    assert!(matches!(
        check_exec(&iris, None, "USER", &docs).await,
        Outcome::Clear
    ));
}

#[test]
fn a_full_chunk_of_long_names_keeps_the_generated_line_short() {
    let names: Vec<String> = (0..WALK_CHUNK)
        .map(|i| format!("Some.Rather.Long.Package.Name.Class{i:05}"))
        .collect();
    let code = walk_code(&names);
    assert!(code.len() < 16_000, "{} bytes", code.len());
    assert!(!code.contains('\n'));
}

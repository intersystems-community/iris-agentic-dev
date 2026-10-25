//! Spec 141 T197: a source-control hook's answer is read from a labelled record, every documented
//! `UserAction` action code is classified, and an answer that cannot be read or is not documented
//! refuses. 0 means "no dialog needed", so a reply that failed to parse must never read as 0.
//!
//! These are the pure parts: what a given output means. `test_scm_action_codes_141_live` runs the
//! same cases against a real hook in IRIS.

use iris_agentic_dev_core::iris::config_sources::{ConfigSnapshot, LoadInputs, CONFIG_FILE_NAME};
use iris_agentic_dev_core::policy::refusal::{classify_refusal, REFUSAL_CODES};
use iris_agentic_dev_core::tools::doc::{probe_outcome, scm_checkout_probe_code, ProbeAnswer};
use iris_agentic_dev_core::tools::scm::{
    action_gate, after_action_outcome, after_user_action_code, user_action_code,
    user_action_outcome, ActionGate, UserActionCode, SCM_RECORD,
};
use serde_json::{json, Value};

/// What the generated snippet writes: the sentinel, then one JSON object.
fn record(v: Value) -> String {
    format!("{SCM_RECORD}{v}\n")
}

fn action(code: Value) -> String {
    record(json!({"kind": "action", "code": code, "msg": "", "target": "Check out?", "ok": 1}))
}

fn refusal_code(out: &str) -> &'static str {
    user_action_outcome(out).expect_err(out).0
}

#[test]
fn every_documented_code_has_a_meaning() {
    let expected = [
        (0, UserActionCode::NoDialog),
        (1, UserActionCode::Confirm),
        (2, UserActionCode::OpenPage),
        (3, UserActionCode::RunExe),
        (4, UserActionCode::InsertText),
        (5, UserActionCode::OpenDocuments),
        (6, UserActionCode::Alert),
        (7, UserActionCode::Prompt),
    ];
    for (n, meaning) in expected {
        assert_eq!(UserActionCode::from_code(n), Some(meaning), "{n}");
        let got = user_action_outcome(&action(json!(n))).expect("a documented code is an answer");
        assert_eq!(got.code, meaning, "{n}");
    }
    for n in [8, 9, 42, 255, 256, 100000, -1, -7] {
        assert_eq!(UserActionCode::from_code(n), None, "{n}");
        assert_eq!(refusal_code(&action(json!(n))), "SCM_ACTION_UNKNOWN", "{n}");
    }
}

#[test]
fn a_string_of_a_canonical_integer_is_that_integer() {
    for (s, meaning) in [
        ("0", UserActionCode::NoDialog),
        ("1", UserActionCode::Confirm),
    ] {
        assert_eq!(
            user_action_outcome(&action(json!(s))).unwrap().code,
            meaning
        );
    }
    assert_eq!(refusal_code(&action(json!("42"))), "SCM_ACTION_UNKNOWN");
}

#[test]
fn an_unreadable_code_is_unparseable_never_zero() {
    for code in [
        json!("abc"),
        json!(""),
        json!(" 1"),
        json!("01"),
        json!("+1"),
        json!("1.0"),
        json!(1.5),
        json!(0.0),
        json!(null),
        json!(true),
        json!([1]),
        json!({"a": 1}),
    ] {
        assert_eq!(
            refusal_code(&action(code.clone())),
            "SCM_ACTION_UNPARSEABLE",
            "{code}"
        );
    }
    // No code at all.
    let no_code = record(json!({"kind": "action", "msg": "", "target": "", "ok": 1}));
    assert_eq!(refusal_code(&no_code), "SCM_ACTION_UNPARSEABLE");
}

#[test]
fn output_that_is_not_a_record_is_never_an_answer() {
    for out in [
        "",
        "\n\n",
        "0|",
        "0|ok\n",
        "1|Check out?\n",
        "ERROR: <PROTECT> *%Studio.SourceControl\n",
        "Connected to repo X\n1|Check out?\n",
        "NO_SCM\n",
        "PROCEED|\n",
    ] {
        assert_eq!(refusal_code(out), "SCM_ACTION_UNPARSEABLE", "{out:?}");
    }
    assert_eq!(refusal_code("SCM_UNAVAILABLE"), "SCM_UNAVAILABLE");
    assert_eq!(
        refusal_code("Connected\nSCM_UNAVAILABLE\n"),
        "SCM_UNAVAILABLE"
    );
}

#[test]
fn the_record_is_found_wherever_the_hook_put_its_own_output() {
    let rec = action(json!(1));
    let rec = rec.trim_end();
    for out in [
        format!("{rec}\n"),
        format!("Connected to repo X\nNOTICE: no user\n{rec}\n"),
        // The hook never ended its line, so the record shares one with its output.
        format!("CMD: p4 opened{rec}\n"),
        format!("{rec}\ntrailing chatter\n"),
        format!("\r\n  {rec}\r\n"),
    ] {
        let got = user_action_outcome(&out).unwrap_or_else(|e| panic!("{out:?}: {e:?}"));
        assert_eq!(got.code, UserActionCode::Confirm, "{out:?}");
        assert_eq!(got.text, "Check out?", "{out:?}");
    }
}

#[test]
fn the_last_record_wins_over_one_the_hook_wrote() {
    let spoof = action(json!(0));
    let real = action(json!(1));
    let got = user_action_outcome(&format!("{spoof}{real}")).unwrap();
    assert_eq!(got.code, UserActionCode::Confirm);
    // A final record that cannot be read is not replaced by an earlier one that can.
    let broken = format!("{SCM_RECORD}{{\"kind\":\"action\",\"code\":\n");
    assert_eq!(
        refusal_code(&format!("{spoof}{broken}")),
        "SCM_ACTION_UNPARSEABLE"
    );
}

#[test]
fn a_message_that_mentions_the_sentinel_is_still_the_message() {
    let rec = record(json!({
        "kind": "action", "code": 1, "ok": 1, "target": "",
        "msg": format!("see {SCM_RECORD}{{\"code\":0}} for details")
    }));
    let got = user_action_outcome(&rec).unwrap();
    assert_eq!(got.code, UserActionCode::Confirm);
    assert!(got.text.contains("for details"), "{got:?}");
}

#[test]
fn a_hook_that_returned_an_error_status_is_not_an_answer() {
    let out = record(json!({
        "kind": "action", "code": 0, "ok": 0, "msg": "", "target": "",
        "err": "ERROR #5001: hook failed"
    }));
    let (code, detail) = user_action_outcome(&out).unwrap_err();
    assert_eq!(code, "SCM_ERROR");
    assert!(detail.contains("hook failed"), "{detail}");
    // A record with no `ok` at all is held to the same rule.
    let no_ok = record(json!({"kind": "action", "code": 0, "msg": "", "target": ""}));
    assert_eq!(refusal_code(&no_ok), "SCM_ERROR");
}

#[test]
fn a_record_of_another_kind_is_not_an_answer() {
    let out = record(json!({"kind": "after", "ok": 1}));
    assert_eq!(refusal_code(&out), "SCM_ACTION_UNPARSEABLE");
}

#[test]
fn message_falls_back_to_the_dialog_target() {
    let with_msg = record(
        json!({"kind": "action", "code": 1, "ok": 1, "msg": "from msg", "target": "from target"}),
    );
    assert_eq!(user_action_outcome(&with_msg).unwrap().text, "from msg");
    let target_only =
        record(json!({"kind": "action", "code": 1, "ok": 1, "msg": "", "target": "from target"}));
    assert_eq!(
        user_action_outcome(&target_only).unwrap().text,
        "from target"
    );
}

#[test]
fn each_code_has_one_decision_for_a_site_that_cannot_open_a_window() {
    let gate = |n: i64| {
        let ans = user_action_outcome(&action(json!(n))).unwrap();
        action_gate(&ans, "Demo.A.cls")
    };
    assert!(matches!(gate(0), ActionGate::Proceed));
    assert!(matches!(gate(1), ActionGate::Dialog(m) if m == "Check out?"));
    assert!(matches!(gate(7), ActionGate::Prompt(m) if m == "Check out?"));
    assert!(matches!(gate(6), ActionGate::Refuse("SCM_REJECTED", _)));
    for n in 2..=5 {
        assert!(
            matches!(gate(n), ActionGate::Refuse("SCM_NEEDS_UI", _)),
            "{n}"
        );
    }
}

// ── AfterUserAction ─────────────────────────────────────────────────────────────────────────────

#[test]
fn after_user_action_is_finished_only_on_an_ok_record() {
    let ok = record(json!({"kind": "after", "ok": 1, "err": ""}));
    assert!(after_action_outcome(&ok).is_ok());
    assert!(after_action_outcome(&format!("NOTICE: committed\n{ok}")).is_ok());

    let failed = record(json!({"kind": "after", "ok": 0, "err": "ERROR #5001: depot down"}));
    let (code, detail) = after_action_outcome(&failed).unwrap_err();
    assert_eq!(code, "SCM_CHECKOUT_FAILED");
    assert!(detail.contains("depot down"), "{detail}");
}

#[test]
fn silence_from_after_user_action_is_not_success() {
    for out in [
        "",
        "\n",
        "NOTICE: committed\n",
        "0|\n",
        "ERROR: <PROTECT>\n",
    ] {
        let (code, _) = after_action_outcome(out).expect_err(out);
        assert!(
            [
                "SCM_NO_OUTPUT",
                "SCM_ACTION_UNPARSEABLE",
                "SCM_CHECKOUT_FAILED"
            ]
            .contains(&code),
            "{out:?}: {code}"
        );
    }
    let (code, _) = after_action_outcome("SCM_UNAVAILABLE").unwrap_err();
    assert_eq!(code, "SCM_UNAVAILABLE");
    // A record of the wrong kind means the finalizer did not run.
    let wrong = record(json!({"kind": "action", "code": 0, "ok": 1}));
    assert!(after_action_outcome(&wrong).is_err());
}

// ── the pre-write probe ─────────────────────────────────────────────────────────────────────────

#[test]
fn the_probe_answers_in_records_of_three_kinds() {
    let none = record(json!({"kind": "none"}));
    assert!(matches!(
        probe_outcome(&none).unwrap(),
        ProbeAnswer::NoSourceControl
    ));
    let hold = record(json!({"kind": "hold"}));
    assert!(matches!(
        probe_outcome(&hold).unwrap(),
        ProbeAnswer::WeHoldIt
    ));
    match probe_outcome(&format!("banner\n{}", action(json!(1)))).unwrap() {
        ProbeAnswer::Action(a) => assert_eq!(a.code, UserActionCode::Confirm),
        other => panic!("{other:?}"),
    }
}

#[test]
fn the_probe_never_reads_the_old_bare_words_or_a_code_pipe_message() {
    for out in [
        "NO_SCM",
        "PROCEED|",
        "0|",
        "1|Check out?",
        "Connected\n0|\n",
        "",
    ] {
        let err = probe_outcome(out).expect_err(out);
        assert_eq!(err.0, "SCM_ACTION_UNPARSEABLE", "{out:?}");
    }
    // The probe's `action` record is classified like any other.
    assert_eq!(
        probe_outcome(&action(json!(42))).unwrap_err().0,
        "SCM_ACTION_UNKNOWN"
    );
    assert_eq!(
        probe_outcome(&action(json!("abc"))).unwrap_err().0,
        "SCM_ACTION_UNPARSEABLE"
    );
}

// ── the generated snippets ──────────────────────────────────────────────────────────────────────

#[test]
fn the_snippets_write_one_labelled_record() {
    let ua = user_action_code("%CheckOut", "A.B.cls");
    let after = after_user_action_code("%CheckOut", "A.B.cls", "yes");
    let probe = scm_checkout_probe_code("A.B.cls");
    for (name, code) in [("user_action", &ua), ("after", &after), ("probe", &probe)] {
        assert!(code.contains(SCM_RECORD), "{name}: {code}");
        assert!(code.contains("%ToJSON()"), "{name}: {code}");
    }
    // The old reply formats are gone: the action code is not glued to a message with a pipe, and
    // the finalizer does not answer with the bare error text.
    assert!(!ua.contains("action_\"|\""), "{ua}");
    assert!(
        !after.contains("write $system.Status.GetErrorText(sc)"),
        "{after}"
    );
    assert!(!probe.contains("write \"NO_SCM\""), "{probe}");
    assert!(!probe.contains("write \"PROCEED|\""), "{probe}");
    // UserAction's own status travels with its code.
    assert!(
        ua.contains("IsError(sc)") || ua.contains("IsOK(sc)"),
        "{ua}"
    );
    assert!(
        after.contains("IsOK(sc)") || after.contains("IsError(sc)"),
        "{after}"
    );
}

// ── refusals ────────────────────────────────────────────────────────────────────────────────────

#[test]
fn an_answer_that_blocks_a_write_is_a_fixed_rule_refusal() {
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path().join("home");
    let proj = tmp.path().join("proj");
    std::fs::create_dir_all(&home).unwrap();
    std::fs::create_dir_all(&proj).unwrap();
    std::fs::write(proj.join(CONFIG_FILE_NAME), "").unwrap();
    let snap = ConfigSnapshot::load(&LoadInputs {
        cwd: std::fs::canonicalize(&proj).unwrap(),
        home: Some(std::fs::canonicalize(&home).unwrap()),
        ..Default::default()
    });
    for code in [
        "SCM_ACTION_UNPARSEABLE",
        "SCM_ACTION_UNKNOWN",
        "SCM_NEEDS_UI",
        "SCM_NEEDS_INPUT",
    ] {
        assert!(REFUSAL_CODES.contains(&code), "{code}");
        let out = classify_refusal(
            json!({"success": false, "error_code": code, "error": "x"}),
            "default",
            &snap.set_aside,
        );
        assert_eq!(out["refusal"]["kind"], "fixed_rule", "{code}: {out}");
        assert!(out["refusal"]["lift"].is_null(), "{code}: {out}");
    }
}

/// A source-control class is set but `SourceControlCreate` made no object (its `Login` failed, or
/// it could not be instantiated). That is not "no source control": the put must stop.
#[test]
fn a_hook_with_no_object_is_unavailable_not_absent() {
    let code = scm_checkout_probe_code("A.B.cls");
    let branch = code
        .split("if '$IsObject(obj) {")
        .nth(1)
        .expect("the probe tests for the source-control object");
    let first = branch.split('}').next().unwrap_or("");
    assert!(
        first.contains(r#"""kind"":""unavailable"""#),
        "the no-object branch must answer `unavailable`: {first}"
    );
    let err = probe_outcome(&record(json!({"kind": "unavailable"}))).unwrap_err();
    assert_eq!(err.0, "SCM_UNAVAILABLE");
}

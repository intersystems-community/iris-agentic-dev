//! Spec 141 C8: `journal_search` withholds the value of a journaled blocklisted global.
//!
//! The journal records every SET and KILL, so `global_pattern` could read `SYS("Security",…)` or
//! `Ens.Config` values that `iris_global` refuses. The record's global reference comes back in
//! the extended form `^|"<dir>"|name(subs)` or as a bare name. The name is normalized the way the
//! blocklist gate reads it, then matched; a match drops the value and sets `redacted`.

use iris_agentic_dev_core::tools::observability::{journal_global_name, journal_value_withheld};

#[test]
fn the_normalizer_strips_the_extended_reference_and_subscripts() {
    assert_eq!(
        journal_global_name(r#"^|"^^/usr/irissys/mgr/"|SYS("Security","UsersD","x")"#),
        "SYS"
    );
    assert_eq!(
        journal_global_name("^Ens.Config.CredentialsD(1)"),
        "Ens.Config.CredentialsD"
    );
    // The form IRIS 2026.2 journals: `^["<dir>"]name`, seen on a live record.
    assert_eq!(
        journal_global_name(r#"^["^^/usr/irissys/mgr/user/"]IadV6Secret141"#),
        "IadV6Secret141"
    );
    assert_eq!(journal_global_name("^IadV6Secret141"), "IadV6Secret141");
    assert_eq!(journal_global_name("IadV6Secret141"), "IadV6Secret141");
}

#[test]
fn system_blocklisted_values_are_withheld() {
    for global_ref in [
        r#"^|"^^/usr/irissys/mgr/"|SYS("Security","UsersD","x")"#,
        "^Ens.Config.CredentialsD(1)",
        "^%SYS.Foo",
        "^oddDEF(1)",
    ] {
        assert!(
            journal_value_withheld(global_ref, &[], false),
            "{global_ref} must be withheld"
        );
        assert!(
            journal_value_withheld(global_ref, &[], true),
            "{global_ref} stays withheld with acknowledgePhi"
        );
    }
}

#[test]
fn a_server_blocklist_entry_withholds_in_either_spelling() {
    let custom = vec!["^IadV6Secret*".to_string()];
    for global_ref in ["^IadV6Secret141(1)", "IadV6Secret141"] {
        assert!(
            journal_value_withheld(global_ref, &custom, false),
            "{global_ref} matches the server blocklist"
        );
    }
    assert!(!journal_value_withheld("^IadOther(1)", &custom, false));
}

#[test]
fn an_ordinary_global_is_not_withheld() {
    assert!(!journal_value_withheld("^IrisDevTest.Glob(1)", &[], false));
}

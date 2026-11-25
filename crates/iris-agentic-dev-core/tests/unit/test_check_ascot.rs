//! `iris_check` without IRIS: a routine checked on its own, and the levels. The live half is
//! `tests/integration/test_check_ascot_live.rs`.

use ascot::bindings::DiagnosticSeverity;
use ascot::IrisContext;
use iris_agentic_dev_core::tools::check::{check, declarations, Level};

#[test]
fn a_routine_calling_a_label_it_lacks_is_an_error_without_iris() {
    let diagnostics = check(
        "/w/Test.mac",
        "ROUTINE Test [Type=MAC]\n Do Missing\n Quit\n",
        None,
    );
    assert_eq!(diagnostics.len(), 1, "{diagnostics:?}");
    assert_eq!(diagnostics[0].severity, DiagnosticSeverity::Error);
    assert!(
        diagnostics[0].message.contains("Missing"),
        "{diagnostics:?}"
    );
}

#[test]
fn a_level_admits_its_own_severity_and_the_ones_above_it() {
    use DiagnosticSeverity as S;
    assert!(Level::Error.admits(S::Error) && !Level::Error.admits(S::Warning));
    assert!(Level::Warning.admits(S::Error) && Level::Warning.admits(S::Warning));
    assert!(!Level::Warning.admits(S::Information));
    assert!(Level::Information.admits(S::Information) && !Level::Information.admits(S::Hint));
    assert_eq!(Level::default(), Level::Error);
    assert_eq!(
        serde_json::from_str::<Level>("\"warning\"").expect("a level"),
        Level::Warning
    );
}

#[test]
fn the_bundled_declarations_load_and_know_the_system_classes() {
    let declarations = declarations().expect("the bundled asset parses");
    let string = declarations
        .get_cls("%Library.String")
        .expect("lookup")
        .expect("%Library.String is declared");
    assert!(string.members.iter().any(|m| m.name == "LogicalToDisplay"));
}

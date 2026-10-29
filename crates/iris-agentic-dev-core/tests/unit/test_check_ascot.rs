//! `iris_check` without IRIS: a routine checked on its own. The live half is
//! `tests/integration/test_check_ascot_live.rs`.

use ascot::bindings::DiagnosticSeverity;
use iris_agentic_dev_core::tools::check::check;

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

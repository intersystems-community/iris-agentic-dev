//! 1.6.4: when the called code moves the current device to `$PRINCIPAL` (`Use 0`,
//! `Use $PRINCIPAL`, or closing the device it was writing to), output written after the move is
//! not lost: Atelier returns it in the answer's `console` array. iad reported `ERROR($DEVICE)` and
//! "the output is lost", which was false. The generator now marks that case with `$Char(4)` and
//! `query_output` appends the console lines. A move to any other device is still an error.
//! Measured on iris-dev-iris 2026-10-22: the console array held exactly the lines written after
//! `Use $PRINCIPAL`.

use iris_agentic_dev_core::iris::connection::query_output;

#[test]
fn a_move_to_principal_is_marked_not_refused() {
    let code = iris_agentic_dev_core::iris::connection::IrisConnection::build_exec_class_for_test(
        "IrisDevTmp.X",
        "",
        "Write 1",
    )
    .join("\n");
    assert!(code.contains("devio=$PRINCIPAL"), "{code}");
    assert!(
        code.contains("$Char(4)"),
        "the principal case is marked: {code}"
    );
    assert!(
        code.contains("ERROR($DEVICE)"),
        "other devices still refuse: {code}"
    );
}

#[test]
fn console_lines_follow_the_captured_output_when_marked() {
    let body = r#"{"status":{"errors":[]},"console":["after-move","second"],"result":{"content":[{"result":"\u0002_SYSTEM\u0003\u0004before-move\n"}]}}"#;
    assert_eq!(
        query_output(200, body).unwrap(),
        "before-move\nafter-move\nsecond\n"
    );
}

#[test]
fn console_lines_are_ignored_without_the_mark() {
    let body = r#"{"status":{"errors":[]},"console":["noise"],"result":{"content":[{"result":"\u0002ok\n"}]}}"#;
    assert_eq!(query_output(200, body).unwrap(), "ok\n");
}

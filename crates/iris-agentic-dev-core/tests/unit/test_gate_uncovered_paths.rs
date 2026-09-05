// Gate branches that shipped without a test covering them.
//
// Found by reading uncovered lines out of the lcov report after the coverage gate started
// producing trustworthy numbers again (the stale-object dilution fixed in scripts/coverage.sh).
// Each test below names the branch it reaches, because a gate branch with no test is the
// same thing as a gate that fails open.
//
// These live in tests/unit/ rather than an inline `#[cfg(test)] mod tests`. An inline test's
// assert-message lines never execute while the test passes, so they count as uncovered lines
// in the file under test — adding inline tests lowers the measured coverage of the code they
// test. Tests in tests/unit/ cover src lines without adding dark lines to them.

use iris_agentic_dev_core::policy::code_edit_gate::check_objectscript_code_edit;
use iris_agentic_dev_core::tools::write_gate::{
    contains_global_kill, contains_terminal_block_syntax,
};

// ── code-edit gate: %Dictionary match that only appears after punctuation flattening ─────

#[test]
fn classmethod_indirection_at_the_dictionary_is_blocked() {
    // `%Dictionary.ClassDefinition` is not present in the whitespace-stripped text — the
    // quotes and comma sit where the dot belongs. Flattening punctuation to dots is what
    // makes this reach the dictionary check.
    let code = r#"Set sc=$classmethod("%Dictionary","ClassDefinition","%OpenId","Foo")"#;
    let r = check_objectscript_code_edit(code, "iris-dev");
    assert!(
        r.is_some(),
        "$classmethod at %Dictionary.*Definition must be blocked: {code}"
    );
    let j = r.unwrap();
    assert_eq!(j["error_code"], "CODE_EDIT_BLOCKED");
    assert_eq!(
        j["matched"], "%DICTIONARY.CLASSDEFINITION",
        "the flattened form is what matched, and the error must say so"
    );
}

#[test]
fn quoted_dictionary_definition_is_blocked() {
    let code = r#"Set x=##class("%Dictionary"."ClassDefinition").%OpenId("Foo")"#;
    assert!(
        check_objectscript_code_edit(code, "iris-dev").is_some(),
        "quoted dotted form must be blocked: {code}"
    );
}

// ── code-edit gate: a word ending in a command name is not a command ─────────────────────

#[test]
fn extrinsic_function_before_a_caret_is_a_global_not_a_routine_call() {
    // `Do ^ROUTINE` and `Goto ^ROUTINE` name routines, so the gate lets the caret pass. The
    // word before the caret has to *be* the command, not merely end with it: in `$$d ^ROUTINE`
    // the `d` is the tail of an extrinsic function call, so `^ROUTINE` is a global reference
    // and the code-storage blocklist applies.
    let r = check_objectscript_code_edit("Set x=$$d ^ROUTINE", "iris-dev");
    assert!(
        r.is_some(),
        "`$$d ^ROUTINE` must be read as a write to ^ROUTINE, not a routine call"
    );
    assert_eq!(r.unwrap()["error_code"], "CODE_EDIT_BLOCKED");
}

#[test]
fn digit_prefixed_word_before_a_caret_is_not_a_command() {
    assert!(
        check_objectscript_code_edit("Set x=1do ^ROUTINE", "iris-dev").is_some(),
        "`1do` is not the Do command; ^ROUTINE stays a global reference"
    );
}

#[test]
fn real_do_command_still_permits_a_routine_call() {
    assert!(
        check_objectscript_code_edit("Do ^ROUTINE", "iris-dev").is_none(),
        "`Do ^ROUTINE` names a routine and must stay permitted"
    );
}

// ── destructive gate: postconditional with no argument separator ──────────────────────────

#[test]
fn kill_postconditional_without_a_separator_is_not_a_kill() {
    // ObjectScript needs whitespace between a postconditional and the command argument, so
    // `Kill:1^Foo` is not valid syntax and the scanner declines to read it as one. This test
    // pins that decision: the branch exists on purpose and a future edit that "fixes" it by
    // treating the caret as an argument is changing behaviour, not fixing a bug.
    assert!(
        !contains_global_kill("Kill:1^Foo"),
        "no whitespace after the postconditional means no argument"
    );
}

#[test]
fn kill_postconditional_with_a_separator_is_a_kill() {
    assert!(
        contains_global_kill("Kill:$D(x) ^Foo"),
        "`Kill:$D(x) ^Foo` is a global kill"
    );
}

// ── terminal block syntax: the unquoted-brace scanner ─────────────────────────────────────

#[test]
fn braced_if_is_terminal_block_syntax() {
    assert!(
        contains_terminal_block_syntax("If x=1 {\n  Write 1\n}"),
        "a braced If is block syntax the terminal cannot take a line at a time"
    );
}

#[test]
fn brace_inside_a_string_literal_is_not_block_syntax() {
    assert!(
        !contains_terminal_block_syntax(r#"If x="{" Write 1"#),
        "a brace inside a string literal does not open a block"
    );
}

#[test]
fn escaped_quote_does_not_end_the_string_early() {
    // ObjectScript escapes a double-quote inside a string by doubling it. If the scanner
    // treated `""` as close-then-open, the brace after it would read as unquoted.
    assert!(
        !contains_terminal_block_syntax(r#"If x="a""{""b" Write 1"#),
        "`\"\"` is an escaped quote, so the brace stays inside the literal"
    );
}

#[test]
fn blank_and_non_keyword_lines_are_skipped() {
    assert!(
        !contains_terminal_block_syntax("\n\n  Set x={\n"),
        "Set is not a block keyword, and blank lines have nothing to check"
    );
}

#[test]
fn braced_for_after_blank_lines_is_still_found() {
    assert!(
        contains_terminal_block_syntax("\n  Set x=1\n  For i=1:1:3 {\n"),
        "skipping earlier lines must not stop the scan"
    );
}

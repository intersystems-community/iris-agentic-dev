//! Spec 141 C2: every generated-code builder that takes caller text splices it as one expression.
//!
//! The splice gate now reads positional `{}` placeholders. These tests hold the sites it sent us to:
//! a quote or a line break in the text must neither close the literal nor start a source line.

use iris_agentic_dev_core::objectscript::{
    enable_namespace_code, export_class_xml_code, namespace_exists_code, skill_forget_code,
    skill_get_code, source_line_code,
};

const HOSTILE: &str = "A\"\nKill ^IadSentinel141\nSet x=\"";

fn assert_one_line(label: &str, code: &str) {
    assert_eq!(code.lines().count(), 1, "{label} grew a line:\n{code}");
    assert!(code.contains("$CHAR(10)"), "{label}:\n{code}");
    assert!(
        !code.contains("\nKill") && !code.contains("\"\nSet"),
        "{label}:\n{code}"
    );
}

#[test]
fn every_builder_keeps_a_hostile_name_on_one_line() {
    assert_one_line("enable_namespace", &enable_namespace_code(HOSTILE));
    assert_one_line("namespace_exists", &namespace_exists_code(HOSTILE));
    assert_one_line("source_line", &source_line_code(HOSTILE, 7));
    assert_one_line("skill_get", &skill_get_code(HOSTILE));
    assert_one_line("skill_forget", &skill_forget_code(HOSTILE));
    assert_one_line("export_class_xml", &export_class_xml_code(HOSTILE, "bpl"));
}

#[test]
fn a_benign_name_reads_as_hand_written_code() {
    assert_eq!(
        namespace_exists_code("USER"),
        "write ##class(%SYS.Namespace).Exists(\"USER\")"
    );
    assert_eq!(
        source_line_code("MyApp.Foo.1", 12),
        "Write ##class(%Studio.Debugger).SourceLine(\"MyApp.Foo.1\",12)"
    );
    assert_eq!(
        skill_get_code("a-skill"),
        "Write $Get(^SKILLS(\"a-skill\"))"
    );
    assert_eq!(
        skill_forget_code("a-skill"),
        "Kill ^SKILLS(\"a-skill\") Write \"OK\""
    );
    assert!(export_class_xml_code("My.Proc", "bpl").contains("ExportToStream(\"My.Proc.bpl\","));
}

#[test]
fn a_quote_in_a_name_is_doubled_not_backslashed() {
    let code = skill_get_code("a\"b");
    assert_eq!(code, "Write $Get(^SKILLS(\"a\"\"b\"))");
    assert!(!code.contains('\\'));
}

#[test]
fn a_dml_macro_with_a_quote_and_a_line_break_stays_one_expression() {
    let input = "  &sql(DELETE FROM T\n       WHERE Name = 'a\"b')\n  Write 1";
    let result = iris_agentic_dev_core::tools::translate_sql_macros(input);
    assert!(result.found, "{:?}", result.warnings);
    let line = result
        .translated_code
        .lines()
        .find(|l| l.contains("%ExecDirect"))
        .unwrap_or_else(|| panic!("no ExecDirect line:\n{}", result.translated_code));
    assert!(line.contains("$CHAR(10)"), "{line}");
    assert!(line.contains("a\"\"b"), "{line}");
}

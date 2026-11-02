//! S18: the SQL code-edit gate, explain mode and the role gate's read check read a statement
//! through `sql_lex`, the way IRIS does.
//!
//! Measured on `iris-dev-iris` (2026.2): IRIS accepts a comment between the package and the dot of
//! a table name (`%Dictionary/**/.ClassDefinition`, `%Dictionary -- c` newline `.ClassDefinition`)
//! and reads it as the same table. The gate used to strip white space but not comments, so those
//! spellings had no `%DICTIONARY.` in them and reached the code dictionary.

use iris_agentic_dev_core::policy::code_edit_gate::check_sql_code_edit;

fn refused(sql: &str) -> bool {
    check_sql_code_edit(sql, "srv").is_some()
}

#[test]
fn a_comment_inside_a_dictionary_table_name_does_not_hide_it() {
    for sql in [
        "UPDATE %Dictionary/**/.ClassDefinition SET Name = Name WHERE 1 = 0",
        "UPDATE %Dictionary./**/ClassDefinition SET Name = Name WHERE 1 = 0",
        "UPDATE %Dictionary/**/./**/ClassDefinition SET Name = Name WHERE 1 = 0",
        "UPDATE %Dictionary -- c\n.ClassDefinition SET Name = Name WHERE 1 = 0",
        "UPDATE %Dictionary.\n-- c\nClassDefinition SET Name = Name WHERE 1 = 0",
        "UPDATE /* a */ %Dictionary /* b */ . /* c */ ClassDefinition SET Name = Name",
        "DELETE FROM \"%Dictionary\"/**/.\"ClassDefinition\" WHERE 1 = 0",
        "DELETE FROM %Dictionary/**/.MethodDefinition WHERE 1 = 0",
        "DELETE FROM %Library/**/.Routine WHERE 1 = 0",
        "DELETE FROM %Library -- c\n.Routine WHERE 1 = 0",
        "DELETE FROM %DICTIONARY\t\r\n/**/.classdefinition",
    ] {
        assert!(refused(sql), "got past the gate: {sql:?}");
    }
}

#[test]
fn the_plain_spellings_are_still_refused() {
    for sql in [
        "UPDATE %Dictionary.ClassDefinition SET Name = Name",
        "UPDATE %Dictionary . ClassDefinition SET Name = Name",
        "UPDATE \"%Dictionary\".\"ClassDefinition\" SET Name = Name",
        "DELETE FROM %Library.Routine",
        "SELECT Name FROM %Dictionary.ClassDefinition",
    ] {
        assert!(refused(sql), "{sql:?}");
    }
}

#[test]
fn a_comment_that_only_mentions_the_dictionary_is_not_a_reference() {
    for sql in [
        "/* never touches %Dictionary.ClassDefinition */ UPDATE t SET a = 1",
        "UPDATE t SET a = 1 -- not %Dictionary.ClassDefinition",
        "UPDATE t SET a = 1 /* %Library.Routine */ WHERE b = 2",
    ] {
        assert!(!refused(sql), "a comment was read as code: {sql:?}");
    }
}

#[test]
fn a_literal_that_names_the_dictionary_is_still_refused() {
    // The gate cannot tell a string that is data from one that is passed to a procedure that
    // reads it, and a false block prints a legible error. Only comments are set aside.
    assert!(refused("UPDATE t SET Name = '%Dictionary.ClassDefinition'"));
}

#[test]
fn an_unterminated_comment_hides_nothing_that_runs() {
    // IRIS refuses a comment that never closes, so nothing after it runs; the gate has nothing
    // to refuse, and a reference before the comment is still seen.
    assert!(!refused(
        "UPDATE t SET a = 1 /* %Dictionary.ClassDefinition"
    ));
    assert!(refused(
        "UPDATE %Dictionary.ClassDefinition SET a = 1 /* never closed"
    ));
}

#[test]
fn multibyte_text_around_a_comment_does_not_panic() {
    assert!(!refused("UPDATE t SET a = 'ŉé日本' /* ŉé */ WHERE b = 1"));
    assert!(refused(
        "UPDATE %Dictionary/* ŉé日本 */.ClassDefinition SET a = 1"
    ));
}

#[test]
fn an_objectscript_body_is_read_with_its_dashes() {
    // In the body of a `CREATE PROCEDURE ... LANGUAGE OBJECTSCRIPT`, `--` is two unary minuses and
    // not a comment, so the call after it runs. The lexer reads it as a SQL comment and drops it,
    // and the text-with-comments reading is what still refuses it.
    for sql in [
        "CREATE PROCEDURE p() LANGUAGE OBJECTSCRIPT { Set x = --##class(%Dictionary.ClassDefinition).%DeleteId(\"X\")\n }",
        "CREATE PROCEDURE p() LANGUAGE OBJECTSCRIPT { Set x = -- do ##class(%Library.Routine).%New(\"a.mac\")\n }",
        "create function f() returns int language objectscript { quit --##class(%Dictionary.MethodDefinition).%DeleteId(\"A||b\") }",
        "ALTER PROCEDURE p() LANGUAGE OBJECTSCRIPT { Set x = --##class(%Dictionary.ClassDefinition).%DeleteId(\"X\")\n }",
    ] {
        assert!(refused(sql), "got past the gate: {sql:?}");
    }
}

#[test]
fn a_dml_comment_is_still_not_a_reference() {
    // The text-with-comments reading starts at the first CREATE or ALTER, as the directive scan
    // does, so DML that has no such word and merely has a comment is untouched.
    for sql in [
        "UPDATE t SET a = 1 -- %Dictionary.ClassDefinition",
        "UPDATE t SET Created = 1 /* %Library.Routine */",
    ] {
        assert!(!refused(sql), "a comment was read as code: {sql:?}");
    }
}

// ── explain mode and the role gate, through the pipeline ────────────────────────────────────────

mod pipeline {
    use crate::support_gate_141::*;
    use serde_json::{json, Value};

    async fn query(rig: &Rig, args: Value) -> Value {
        match rig.tools.call_gated("iris_query", args).await {
            Ok(r) => body_of(&r),
            Err(e) => json!({"error_code": "CALL_ERROR", "error": e}),
        }
    }

    fn explain(sql: &str) -> Value {
        json!({"query": sql, "mode": "explain"})
    }

    #[tokio::test]
    async fn explain_reads_the_statement_through_the_lexer() {
        let rig = rig("");
        // A comment or space before the keyword is not a reason to refuse. The default server is
        // at an address nothing listens on, so the answer for a statement that is accepted is a
        // connection failure, never EXPLAIN_REQUIRES_SELECT.
        for sql in [
            "/* plan this */ SELECT 1",
            "-- plan this\nSELECT 1",
            "\n\t SELECT 1",
            "/* a */ -- b\n WITH x AS (SELECT 1 AS a) SELECT a FROM x",
        ] {
            let b = query(&rig, explain(sql)).await;
            assert_ne!(b["error_code"], "EXPLAIN_REQUIRES_SELECT", "{sql:?}: {b}");
            assert_ne!(b["error_code"], "SQL_WRITE_BLOCKED", "{sql:?}: {b}");
        }
    }

    #[tokio::test]
    async fn explain_refuses_what_is_not_one_select() {
        let rig = rig("");
        for sql in [
            "DELETE FROM t",
            "/* SELECT */ DELETE FROM t",
            "EXPLAIN SELECT 1",
            "( SELECT 1 )",
            "; SELECT 1",
        ] {
            let b = query(&rig, explain(sql)).await;
            assert_eq!(b["error_code"], "EXPLAIN_REQUIRES_SELECT", "{sql:?}: {b}");
        }
        // A second statement, a write keyword and SELECT ... INTO are what read mode refuses, and
        // explain mode no longer sends them to IRIS.
        for sql in [
            "SELECT 1; DROP TABLE t",
            "SELECT 1 -- c\n; DELETE FROM t",
            "SELECT 1 INTO :v",
            "SELECT 1 /* c */ INTO #t",
        ] {
            let b = query(&rig, explain(sql)).await;
            assert_eq!(b["error_code"], "SQL_WRITE_BLOCKED", "{sql:?}: {b}");
        }
        let b = query(&rig, explain("  /* nothing */  ")).await;
        assert_eq!(b["error_code"], "EMPTY_QUERY", "{b}");
    }

    const PROD_SUBJECT: &str = r#"
mode = "operate"

[instance.prod]
host = "127.0.0.1"
web_port = 1
role = "subject"

[policy.prod]
mcpTemplate = "dev"
writeTools = true
destructiveTools = true
"#;

    #[tokio::test]
    async fn the_role_gate_reads_the_first_statement_word_through_the_lexer() {
        let rig = rig(PROD_SUBJECT);
        // A read behind a comment is a read: no confirmation is asked for.
        for sql in [
            "/* c */ SELECT 1",
            "-- c\nselect 1",
            "\n WITH x AS (SELECT 1 AS a) SELECT a FROM x",
        ] {
            let b = query(&rig, json!({"query": sql, "server": "prod"})).await;
            assert_ne!(b["error_code"], "SUBJECT_CONFIRM_REQUIRED", "{sql:?}: {b}");
        }
        // Anything else asks, whatever precedes it.
        for sql in [
            "CALL p()",
            "/* SELECT */ CALL p()",
            "-- SELECT\nCALL p()",
            "( SELECT 1 )",
            "EXPLAIN SELECT 1",
        ] {
            let b = query(&rig, json!({"query": sql, "server": "prod"})).await;
            assert_eq!(b["error_code"], "SUBJECT_CONFIRM_REQUIRED", "{sql:?}: {b}");
        }
    }
}

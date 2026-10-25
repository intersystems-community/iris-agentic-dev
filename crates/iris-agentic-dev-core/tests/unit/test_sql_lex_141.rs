//! Spec 141 T217, T221: the SQL lexer follows IRIS (rules measured on `iris-dev-iris`), is
//! lossless, and splits qualified names at unquoted dots only.

use iris_agentic_dev_core::sql_lex::{
    code, is_unterminated, lex, split_qualified_name, text, Kind,
};
use iris_agentic_dev_core::tools::needs_rows_precheck;

fn kinds(sql: &str) -> Vec<(Kind, String)> {
    code(&lex(sql)).map(|t| (t.kind, t.text.clone())).collect()
}

#[test]
fn a_literal_runs_to_the_next_undoubled_quote_and_backslash_is_ordinary() {
    assert_eq!(
        kinds("'a--b' 'it''s' 'back\\' x"),
        [
            (Kind::Literal, "'a--b'".to_string()),
            (Kind::Literal, "'it''s'".to_string()),
            (Kind::Literal, "'back\\'".to_string()),
            (Kind::Word, "x".to_string()),
        ]
    );
}

#[test]
fn a_delimited_identifier_doubles_its_quote() {
    assert_eq!(
        kinds("\"a\"\"b\" \"x--y\" \"p/*q\""),
        [
            (Kind::QuotedIdent, "\"a\"\"b\"".to_string()),
            (Kind::QuotedIdent, "\"x--y\"".to_string()),
            (Kind::QuotedIdent, "\"p/*q\"".to_string()),
        ]
    );
}

#[test]
fn comments_are_recognised_only_outside_literals() {
    let tokens = lex("a -- c1 /* not a block\nb /* x -- y */ c '--' d");
    let comments: Vec<&str> = tokens
        .iter()
        .filter(|t| t.is_comment())
        .map(|t| t.text.as_str())
        .collect();
    assert_eq!(comments, ["-- c1 /* not a block", "/* x -- y */"]);
    let words: Vec<&str> = code(&tokens)
        .filter(|t| t.kind == Kind::Word)
        .map(|t| t.text.as_str())
        .collect();
    assert_eq!(words, ["a", "b", "c", "d"]);
}

#[test]
fn block_comments_do_not_nest_and_adjacent_dashes_are_a_comment() {
    let tokens = lex("1 /* a /* b */ 2 */");
    let block = tokens
        .iter()
        .find(|t| t.kind == Kind::BlockComment)
        .unwrap();
    assert_eq!(block.text, "/* a /* b */");
    // `5--1` is 5 and a comment, as IRIS reads it.
    assert_eq!(kinds("5--1").len(), 1);
}

#[test]
fn an_unclosed_delimiter_is_reported() {
    for open in ["'abc", "\"abc", "/* abc", "a 'b' 'c"] {
        assert!(is_unterminated(&lex(open)), "{open}");
    }
    for closed in ["'abc'", "\"abc\"", "/* abc */", "-- abc", "'it''s'"] {
        assert!(!is_unterminated(&lex(closed)), "{closed}");
    }
}

#[test]
fn a_line_comment_ends_at_either_newline_character() {
    // Ending at `\r` as well is the stricter reading: code after it is seen, not hidden.
    for sql in ["a -- c\nDROP", "a -- c\rDROP"] {
        let words: Vec<String> = code(&lex(sql))
            .filter(|t| t.kind == Kind::Word)
            .map(|t| t.text.clone())
            .collect();
        assert_eq!(words, ["a", "DROP"], "{sql:?}");
    }
}

#[test]
fn the_tokens_give_the_input_back() {
    for sql in [
        "SELECT 'é' AS \"日本\" /* ŉ */ FROM t -- ß",
        "",
        "   ",
        "'",
        "\"",
        "/*",
        "--",
        "a'b\"c",
        "😀 ﬃ İ",
    ] {
        assert_eq!(text(&lex(sql)), sql);
    }
}

#[test]
fn word_matching_ignores_ascii_case_only() {
    let tokens = lex("select SeLeCt ſelect");
    let words: Vec<bool> = code(&tokens).map(|t| t.is_word("SELECT")).collect();
    assert_eq!(words, [true, true, false]);
}

#[test]
fn qualified_names_split_at_unquoted_dots_only() {
    for (name, want) in [
        ("Person", vec!["Person"]),
        ("Sample.Person", vec!["Sample", "Person"]),
        ("\"A.B\".C", vec!["A.B", "C"]),
        ("A.\"B.C\"", vec!["A", "B.C"]),
        ("\"A.B\"", vec!["A.B"]),
        ("\"x\"\"y\".z", vec!["x\"y", "z"]),
        (
            "%Dictionary.CompiledClass",
            vec!["%Dictionary", "CompiledClass"],
        ),
        ("\"日本.é\".ŉ", vec!["日本.é", "ŉ"]),
    ] {
        assert_eq!(split_qualified_name(name), want, "{name}");
    }
}

#[test]
fn qualified_name_splitting_does_not_panic_on_odd_input() {
    for name in [
        "", ".", "..", "\"", "\"a", "\".", "a.", ".a", "\"\"", "\"\"\"", "é.\"ü",
    ] {
        let parts = split_qualified_name(name);
        assert!(!parts.is_empty(), "{name:?}");
    }
}

#[test]
fn only_update_and_delete_get_a_row_count() {
    for yes in [
        "UPDATE t SET a = 1",
        "  delete from t",
        "/* c */ DELETE FROM t",
        "-- c\nUPDATE t SET a = 1",
        "DELETE %NOLOCK FROM t",
    ] {
        assert!(needs_rows_precheck(yes), "{yes}");
    }
    for no in [
        "INSERT INTO t VALUES (1)",
        "CALL p()",
        "TRUNCATE TABLE t",
        "DELETED FROM t",
        "'DELETE' FROM t",
        "-- DELETE FROM t",
        "",
    ] {
        assert!(!needs_rows_precheck(no), "{no}");
    }
}

#[test]
fn table_info_splits_the_table_name_at_unquoted_dots() {
    use iris_agentic_dev_core::tools::info::{quote_sql_identifier, table_schema_and_name};
    for (given, schema, table) in [
        ("Person", "SQLUser", "Person"),
        ("Sample.Person", "Sample", "Person"),
        // The old split took the first dot, so this was schema `"A` and table `B".C`.
        ("\"A.B\".C", "A.B", "C"),
        ("A.\"B.C\"", "A", "B.C"),
        ("\"A.B\"", "SQLUser", "A.B"),
        ("%Dictionary.CompiledClass", "%Dictionary", "CompiledClass"),
    ] {
        assert_eq!(
            table_schema_and_name(given),
            (schema.to_string(), table.to_string()),
            "{given}"
        );
    }
    // Re-quoted for SQL, a part with a dot or a quote in it stays one identifier.
    assert_eq!(quote_sql_identifier("A.B"), "\"A.B\"");
    assert_eq!(quote_sql_identifier("x\"y"), "\"x\"\"y\"");
}

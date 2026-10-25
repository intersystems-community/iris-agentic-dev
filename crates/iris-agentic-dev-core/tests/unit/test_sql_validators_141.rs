//! Spec 141 T214, T217, T221: the SQL validators read text the way IRIS does and never panic.
//!
//! IRIS's SQL lexer (measured on `iris-dev-iris`, 2026.2): a single-quoted literal ends at the
//! next `'` that is not doubled, and `\` is an ordinary character in it; a double-quoted delimited
//! identifier works the same way with `""`; `--` and `/* */` start a comment only outside a
//! literal or an identifier; block comments do not nest and a line comment ends at the newline.
//! The validators used to strip `--` and `/* */` before looking at quotes, treated `\'` as an
//! escape, and indexed character vectors with byte offsets.

use iris_agentic_dev_core::tools::interop::check_body_where;
use iris_agentic_dev_core::tools::{
    build_rows_precheck_query, validate_dml_sql, validate_read_only_sql,
};

// ── validate_read_only_sql follows IRIS ────────────────────────────────────────────────────

#[test]
fn dashes_inside_a_literal_do_not_start_a_comment() {
    // The old pre-pass cut the line at the `--` inside the literal and never saw the DROP.
    assert_eq!(
        validate_read_only_sql("SELECT '--' AS a FROM t WHERE b = 1 ; DROP TABLE t"),
        Err("DROP".to_string())
    );
    assert!(validate_read_only_sql("SELECT 'a--b' AS a FROM t").is_ok());
}

#[test]
fn a_backslash_does_not_escape_the_closing_quote() {
    // IRIS ends the literal at `\'`. The old walker took `\'` as an escape and swallowed the DROP.
    assert_eq!(
        validate_read_only_sql("SELECT 'a\\' ; DROP TABLE t"),
        Err("DROP".to_string())
    );
    assert!(validate_read_only_sql("SELECT 'a\\' AS a FROM t").is_ok());
}

#[test]
fn a_doubled_quote_inside_a_delimited_identifier_stays_inside_it() {
    // One identifier, `a" DROP "b`: IRIS runs it, and a keyword in it is not a keyword.
    assert!(validate_read_only_sql("SELECT 1 AS \"a\"\" DROP \"\"b\" FROM t").is_ok());
    // Doubled quote in a literal likewise.
    assert!(validate_read_only_sql("SELECT 'it''s DROP TABLE' FROM t").is_ok());
}

#[test]
fn a_line_comment_hides_a_block_comment_opener() {
    // IRIS: `-- /*` is a line comment, so the DROP on the next line runs. The old pre-pass
    // removed `/*\nDROP TABLE t /*` as a block comment first and saw nothing.
    assert_eq!(
        validate_read_only_sql("SELECT 1 -- /*\nDROP TABLE t /* */"),
        Err("DROP".to_string())
    );
}

#[test]
fn block_comments_do_not_nest() {
    // IRIS ends the comment at the first `*/`; the DROP after it is code.
    assert_eq!(
        validate_read_only_sql("SELECT 1 /* a /* b */ DROP TABLE t */"),
        Err("DROP".to_string())
    );
}

#[test]
fn a_keyword_in_a_comment_is_not_a_keyword() {
    assert!(validate_read_only_sql("SELECT /* DROP */ 1 FROM t -- DELETE").is_ok());
}

#[test]
fn into_inside_a_literal_is_not_select_into() {
    assert!(validate_read_only_sql("SELECT 'copy INTO x' FROM t").is_ok());
    assert_eq!(
        validate_read_only_sql("SELECT a INTO v FROM t"),
        Err("SELECT INTO".to_string())
    );
    assert!(validate_read_only_sql("SELECT a FROM t WHERE b IN (SELECT 1 INTO (x))").is_ok());
}

#[test]
fn select_into_after_multibyte_text_is_still_found_and_never_panics() {
    // Byte offsets from the uppercased string indexed a vector of characters.
    assert_eq!(
        validate_read_only_sql("SELECT '日本語日本語' AS ñ INTO v FROM t"),
        Err("SELECT INTO".to_string())
    );
    assert!(validate_read_only_sql("SELECT 'ŉŉŉŉ' AS é FROM t").is_ok());
    assert_eq!(
        validate_read_only_sql("SELECT 'ŉ' ; ŉDROP TABLE t ; DROP TABLE t"),
        Err("DROP".to_string())
    );
}

// ── validate_dml_sql follows IRIS ──────────────────────────────────────────────────────────

#[test]
fn dml_first_word_is_found_past_comments_the_way_iris_reads_them() {
    // The old pre-pass turned this into an empty statement; IRIS runs `SELECT 1`.
    assert_eq!(
        validate_dml_sql("-- /*\nSELECT 1 /* */"),
        Err("SELECT_IN_WRITE".to_string())
    );
    assert_eq!(validate_dml_sql("/* c */ DELETE FROM t"), Ok(()));
    assert_eq!(
        validate_dml_sql("-- only a comment"),
        Err("EMPTY".to_string())
    );
}

#[test]
fn dml_literals_with_dashes_and_backslashes_do_not_confuse_the_first_word() {
    assert_eq!(
        validate_dml_sql("UPDATE t SET a = '--' WHERE b = 'x\\'"),
        Ok(())
    );
    assert_eq!(
        validate_dml_sql("DROP TABLE t -- 'x"),
        Err("DROP".to_string())
    );
}

#[test]
fn dml_with_multibyte_text_does_not_panic() {
    assert_eq!(validate_dml_sql("UPDATE \"日本\" SET é = 'ß'"), Ok(()));
    assert_eq!(
        validate_dml_sql("/* ŉ */ SELECT 'ŉ'"),
        Err("SELECT_IN_WRITE".to_string())
    );
}

// ── the rows pre-check reads the statement it will run ─────────────────────────────────────

#[test]
fn delete_precheck_takes_the_delete_target_not_the_first_from() {
    // A comment holds a `FROM`; the target is `t`.
    assert_eq!(
        build_rows_precheck_query("DELETE /* FROM x */ FROM t WHERE a = 1").as_deref(),
        Some("SELECT COUNT(*) FROM t WHERE a = 1")
    );
    // The `WHERE` holds a subquery with its own FROM and WHERE.
    assert_eq!(
        build_rows_precheck_query("DELETE FROM t WHERE id IN (SELECT id FROM u WHERE x = 1)")
            .as_deref(),
        Some("SELECT COUNT(*) FROM t WHERE id IN (SELECT id FROM u WHERE x = 1)")
    );
    // A schema-qualified and a quoted target.
    assert_eq!(
        build_rows_precheck_query("DELETE FROM Sample.Person WHERE Age > 3").as_deref(),
        Some("SELECT COUNT(*) FROM Sample.Person WHERE Age > 3")
    );
    assert_eq!(
        build_rows_precheck_query("DELETE FROM \"My Schema\".\"My Table\"").as_deref(),
        Some("SELECT COUNT(*) FROM \"My Schema\".\"My Table\"")
    );
}

#[test]
fn delete_precheck_skips_statement_modifiers() {
    assert_eq!(
        build_rows_precheck_query("DELETE %NOLOCK FROM t WHERE a = 1").as_deref(),
        Some("SELECT COUNT(*) FROM t WHERE a = 1")
    );
}

#[test]
fn precheck_survives_leading_space_and_comments() {
    // `upper.starts_with("DELETE")` on the untrimmed text said "not a DELETE" and skipped the check.
    assert_eq!(
        build_rows_precheck_query("  \n DELETE FROM t WHERE a = 1").as_deref(),
        Some("SELECT COUNT(*) FROM t WHERE a = 1")
    );
    assert_eq!(
        build_rows_precheck_query("/* note */ DELETE FROM t WHERE a = 1").as_deref(),
        Some("SELECT COUNT(*) FROM t WHERE a = 1")
    );
    assert_eq!(
        build_rows_precheck_query("-- note\nUPDATE t SET a = 1 WHERE b = 2").as_deref(),
        Some("SELECT COUNT(*) FROM t WHERE b = 2")
    );
}

#[test]
fn update_precheck_takes_the_top_level_where() {
    assert_eq!(
        build_rows_precheck_query("UPDATE t SET a = (SELECT b FROM u WHERE u.id = 1) WHERE c = 2")
            .as_deref(),
        Some("SELECT COUNT(*) FROM t WHERE c = 2")
    );
    assert_eq!(
        build_rows_precheck_query("UPDATE t SET a = 'where x' WHERE c = 2").as_deref(),
        Some("SELECT COUNT(*) FROM t WHERE c = 2")
    );
    assert_eq!(
        build_rows_precheck_query("UPDATE t SET a = 'where x'").as_deref(),
        Some("SELECT COUNT(*) FROM t")
    );
}

#[test]
fn precheck_with_multibyte_text_is_exact() {
    // `to_uppercase` changes the length of `ŉ`, so offsets taken from it pointed elsewhere.
    assert_eq!(
        build_rows_precheck_query("DELETE FROM \"ŉ\" WHERE a = 'ß日本'").as_deref(),
        Some("SELECT COUNT(*) FROM \"ŉ\" WHERE a = 'ß日本'")
    );
    assert_eq!(
        build_rows_precheck_query("UPDATE \"ŉŉ\" SET é = 1 WHERE ü = 'ŉ'").as_deref(),
        Some("SELECT COUNT(*) FROM \"ŉŉ\" WHERE ü = 'ŉ'")
    );
}

#[test]
fn precheck_is_none_for_other_statements() {
    assert_eq!(build_rows_precheck_query("INSERT INTO t VALUES (1)"), None);
    assert_eq!(build_rows_precheck_query("DELETE"), None);
    assert_eq!(build_rows_precheck_query("UPDATE"), None);
    // Not a DELETE word.
    assert_eq!(build_rows_precheck_query("DELETED FROM t"), None);
}

// ── body_where is one boolean expression ───────────────────────────────────────────────────

#[test]
fn body_where_accepts_ordinary_filters() {
    for ok in [
        "Name = 'x' AND (A > 1 OR B < 2)",
        "StringValue = 'a--b'",
        "StringValue LIKE '%/*%'",
        "StringValue = 'select union drop'",
        "StringValue = 'x;y'",
        "StringValue = 'it''s'",
        "StringValue = 'back\\'",
        "\"Odd\"\"Name\" = 1",
        "\"UNION\" = 1",
        "Name = 'é日本ŉ'",
        "(A = 1)",
        "((A = 1) OR (B = 2))",
        "r.Status = 'Open'",
    ] {
        assert!(
            check_body_where(ok).is_ok(),
            "{ok}: {:?}",
            check_body_where(ok)
        );
    }
}

#[test]
fn body_where_refuses_each_way_out_of_the_expression() {
    for bad in [
        // closes the parenthesis it is wrapped in, then a second query
        "1=1) UNION SELECT Password FROM Security.Users WHERE (1=1",
        "Name = 'x') UNION ALL SELECT 1 FROM Foo WHERE ('1'='1",
        "1=1 UNION SELECT 1",
        "1=1 INTERSECT SELECT 1",
        "1=1 EXCEPT SELECT 1",
        // unbalanced either way
        "(A = 1",
        "A = 1)",
        "((A = 1) OR B = 2",
        ") OR (",
        // subquery
        "ID IN (SELECT ID FROM Other.Table)",
        "EXISTS (select 1)",
        // statement separator and comments outside a literal
        "A = 1; DELETE FROM t",
        "A = 1 -- rest",
        "A = 1 /* rest */",
        "A = 1 /* never closed",
        // a literal that never closes
        "A = 'x",
        "A = \"x",
        // write keywords
        "A = 1 OR DROP TABLE t",
        "UPDATE x",
        "A = 1 INTO v",
        // a backslash does not escape the quote: the literal ends and the UNION is code
        "A = 'x\\' UNION SELECT 1 --",
        // keywords after multibyte text
        "Name = 'é日本' ) UNION SELECT 1 FROM X WHERE (1=1",
    ] {
        let e = check_body_where(bad).expect_err(bad);
        assert_eq!(e["error_code"], "INVALID_PARAMS", "{bad}: {e}");
    }
}

// ── nothing here panics ────────────────────────────────────────────────────────────────────

/// Pieces that exercise every lexer state; a deterministic generator joins them.
const PIECES: &[&str] = &[
    "SELECT",
    "FROM",
    "WHERE",
    "DELETE",
    "UPDATE",
    "INSERT",
    "INTO",
    "UNION",
    "DROP",
    "t",
    "\"t\"",
    "x.y",
    "(",
    ")",
    ",",
    ";",
    "'",
    "''",
    "\"",
    "\"\"",
    "\\",
    "--",
    "/*",
    "*/",
    " ",
    "\n",
    "\r",
    "\t",
    "=",
    "1",
    "%",
    "é",
    "ß",
    "ŉ",
    "İ",
    "ǆ",
    "ﬃ",
    "ΐ",
    "日本語",
    "😀",
    "\u{2028}",
    "\u{a0}",
    "\u{0}",
    "ı",
    "ſ",
];

fn generate(seed: u64, len: usize) -> String {
    let mut state = seed
        .wrapping_mul(6364136223846793005)
        .wrapping_add(1442695040888963407);
    let mut out = String::new();
    for _ in 0..len {
        state = state
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        out.push_str(PIECES[(state >> 33) as usize % PIECES.len()]);
    }
    out
}

#[test]
fn no_validator_panics_on_multibyte_or_malformed_text() {
    let mut cases = 0usize;
    for seed in 0..4000u64 {
        let len = 1 + (seed as usize % 24);
        let text = generate(seed, len);
        let _ = validate_read_only_sql(&text);
        let _ = validate_dml_sql(&text);
        let _ = build_rows_precheck_query(&text);
        let _ = check_body_where(&text);
        // The same text behind each statement head, so the parsers get past their first word.
        for head in [
            "DELETE FROM ",
            "UPDATE ",
            "DELETE FROM t WHERE ",
            "UPDATE t SET a = ",
        ] {
            let stmt = format!("{head}{text}");
            let _ = validate_dml_sql(&stmt);
            let _ = build_rows_precheck_query(&stmt);
        }
        cases += 1;
    }
    assert_eq!(cases, 4000);
}

#[test]
fn multibyte_text_in_each_position_does_not_panic() {
    let atoms = ["é", "ß", "ŉ", "İ", "日本語", "😀", "ﬃ", "ΐ"];
    let mut cases = 0usize;
    for a in atoms {
        for template in [
            "SELECT '{}' FROM t",
            "SELECT \"{}\" FROM t",
            "SELECT 1 /* {} */ FROM t",
            "SELECT 1 FROM t -- {}",
            "SELECT {} FROM t",
            "SELECT 1 INTO {}",
            "DELETE FROM \"{}\" WHERE a = '{}'",
            "UPDATE \"{}\" SET b = '{}' WHERE c = '{}'",
            "{} DELETE FROM t",
            "{}",
        ] {
            let text = template.replace("{}", a);
            let _ = validate_read_only_sql(&text);
            let _ = validate_dml_sql(&text);
            let _ = build_rows_precheck_query(&text);
            let _ = check_body_where(&text);
            cases += 1;
        }
    }
    assert_eq!(cases, atoms.len() * 10);
}

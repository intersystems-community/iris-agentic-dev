//! Unit tests for validate_dml_sql and build_rows_precheck_query.
//! These are pure functions — no IRIS or feature flag needed.

use iris_agentic_dev_core::tools::{build_rows_precheck_query, validate_dml_sql};

// ── validate_dml_sql ──────────────────────────────────────────────────────────

#[test]
fn dml_empty_query_returns_empty_err() {
    assert_eq!(validate_dml_sql(""), Err("EMPTY".to_string()));
    assert_eq!(validate_dml_sql("   "), Err("EMPTY".to_string()));
}

#[test]
fn dml_comment_only_returns_empty_err() {
    assert_eq!(
        validate_dml_sql("/* just a comment */"),
        Err("EMPTY".to_string())
    );
    assert_eq!(
        validate_dml_sql("-- line comment"),
        Err("EMPTY".to_string())
    );
}

#[test]
fn dml_select_returns_select_in_write_err() {
    assert_eq!(
        validate_dml_sql("SELECT 1"),
        Err("SELECT_IN_WRITE".to_string())
    );
    assert_eq!(
        validate_dml_sql("select Name from Sample.Person"),
        Err("SELECT_IN_WRITE".to_string())
    );
}

#[test]
fn dml_insert_ok() {
    assert!(validate_dml_sql("INSERT INTO Foo (x) VALUES (1)").is_ok());
}

#[test]
fn dml_update_ok() {
    assert!(validate_dml_sql("UPDATE Foo SET x=1 WHERE id=2").is_ok());
}

#[test]
fn dml_delete_ok() {
    assert!(validate_dml_sql("DELETE FROM Foo WHERE id=1").is_ok());
}

#[test]
fn dml_call_ok() {
    assert!(validate_dml_sql("CALL MyProc(1,2)").is_ok());
}

#[test]
fn dml_truncate_ok() {
    assert!(validate_dml_sql("TRUNCATE TABLE Foo").is_ok());
}

#[test]
fn dml_create_returns_ddl_err() {
    let r = validate_dml_sql("CREATE TABLE Foo (id INT)");
    assert!(r.is_err());
    let e = r.unwrap_err();
    // DDL keyword branch returns the keyword itself
    assert!(e.contains("CREATE") || e == "UNKNOWN_STATEMENT", "got: {e}");
}

#[test]
fn dml_drop_returns_ddl_err() {
    let r = validate_dml_sql("DROP TABLE Foo");
    assert!(r.is_err());
}

#[test]
fn dml_alter_returns_ddl_err() {
    let r = validate_dml_sql("ALTER TABLE Foo ADD COLUMN bar INT");
    assert!(r.is_err());
}

#[test]
fn dml_grant_returns_ddl_err() {
    let r = validate_dml_sql("GRANT SELECT ON Foo TO PUBLIC");
    assert!(r.is_err());
}

#[test]
fn dml_revoke_returns_ddl_err() {
    let r = validate_dml_sql("REVOKE SELECT ON Foo FROM PUBLIC");
    assert!(r.is_err());
}

#[test]
fn dml_unknown_statement_returns_err() {
    let r = validate_dml_sql("MERGE INTO Foo USING bar ON (1=1)");
    assert!(r.is_err());
}

#[test]
fn dml_strips_block_comment_before_keyword() {
    // keyword follows a block comment
    assert!(validate_dml_sql("/* preamble */ INSERT INTO Foo VALUES (1)").is_ok());
}

#[test]
fn dml_strips_line_comment_before_keyword() {
    assert!(validate_dml_sql("-- preamble\nDELETE FROM Foo").is_ok());
}

// ── build_rows_precheck_query ─────────────────────────────────────────────────

#[test]
fn precheck_update_with_where_produces_count_query() {
    let q = build_rows_precheck_query("UPDATE Foo SET x=1 WHERE id=2");
    assert!(q.is_some(), "should produce a precheck query");
    let s = q.unwrap();
    assert!(s.to_uppercase().contains("SELECT COUNT"), "got: {s}");
    assert!(
        s.to_uppercase().contains("FROM FOO") || s.to_uppercase().contains("FROM Foo"),
        "got: {s}"
    );
}

#[test]
fn precheck_delete_with_where_produces_count_query() {
    let q = build_rows_precheck_query("DELETE FROM Foo WHERE id=1");
    assert!(q.is_some(), "should produce a precheck query");
    let s = q.unwrap();
    assert!(s.to_uppercase().contains("SELECT COUNT"), "got: {s}");
}

#[test]
fn precheck_insert_returns_none() {
    // INSERT doesn't need a precheck
    let q = build_rows_precheck_query("INSERT INTO Foo VALUES (1)");
    assert!(q.is_none(), "INSERT should not need a precheck");
}

#[test]
fn precheck_update_no_where_returns_some_or_none() {
    // Parser may or may not handle this — just must not panic
    let _q = build_rows_precheck_query("UPDATE Foo SET x=1");
    // No assertion — just exercises the code path
}

#[test]
fn precheck_empty_returns_none() {
    let q = build_rows_precheck_query("");
    assert!(q.is_none());
}

// ── S13: a statement that cannot be counted is not run under max_rows_affected ─

use iris_agentic_dev_core::tools::needs_rows_precheck;

fn count_sql(dml: &str) -> Option<String> {
    build_rows_precheck_query(dml)
}

#[test]
fn precheck_counts_a_delete_without_from() {
    assert_eq!(
        count_sql("DELETE t WHERE a = 1").as_deref(),
        Some("SELECT COUNT(*) FROM t WHERE a = 1")
    );
    assert_eq!(
        count_sql("DELETE Sample.Person").as_deref(),
        Some("SELECT COUNT(*) FROM Sample.Person")
    );
    assert_eq!(
        count_sql("DELETE %NOLOCK t WHERE a = 1").as_deref(),
        Some("SELECT COUNT(*) FROM t WHERE a = 1")
    );
}

#[test]
fn precheck_reads_every_modifier_iris_takes() {
    for modifier in [
        "%NOJOURN",
        "%NOCHECK",
        "%NOFPLAN",
        "%NOINDEX",
        "%NOLOCK",
        "%NOTRIGGER",
        "%NOUNIQCHECK",
        "%PROFILE",
        "%PROFILE_ALL",
        "%STARTTABLE",
    ] {
        for stmt in [
            format!("DELETE {modifier} FROM t WHERE a = 1"),
            format!("DELETE {modifier} t WHERE a = 1"),
            format!("UPDATE {modifier} t SET a = 2 WHERE a = 1"),
            format!("DELETE {modifier} %NOLOCK FROM t WHERE a = 1"),
        ] {
            assert_eq!(
                count_sql(&stmt).as_deref(),
                Some("SELECT COUNT(*) FROM t WHERE a = 1"),
                "{stmt}"
            );
        }
    }
}

#[test]
fn precheck_counts_truncate_by_its_table() {
    for (stmt, table) in [
        ("TRUNCATE TABLE t", "t"),
        ("truncate  table Sample.Person", "Sample.Person"),
        ("/* c */ TRUNCATE TABLE \"Odd Name\";", "\"Odd Name\""),
        ("-- c\nTRUNCATE TABLE %Dictionary.X", "%Dictionary.X"),
    ] {
        assert!(needs_rows_precheck(stmt), "{stmt}");
        assert_eq!(
            count_sql(stmt),
            Some(format!("SELECT COUNT(*) FROM {table}")),
            "{stmt}"
        );
    }
}

#[test]
fn precheck_keeps_a_table_alias_for_the_count() {
    assert_eq!(
        count_sql("DELETE FROM t AS a WHERE a.x = 1").as_deref(),
        Some("SELECT COUNT(*) FROM t AS a WHERE a.x = 1")
    );
    assert_eq!(
        count_sql("UPDATE t a SET x = 2 WHERE a.x = 1").as_deref(),
        Some("SELECT COUNT(*) FROM t a WHERE a.x = 1")
    );
}

#[test]
fn precheck_gives_up_on_a_statement_it_cannot_read() {
    // Each of these needs a count and cannot get one: the caller refuses them.
    for stmt in [
        "TRUNCATE TABLE",
        "TRUNCATE t",
        "TRUNCATE TABLE a, b",
        "DELETE",
        "DELETE FROM",
        "DELETE FROM t1, t2 WHERE a = 1",
        "UPDATE t1, t2 SET a = 1",
        "DELETE %BOGUS FROM t",
        "DELETE FROM t JOIN u ON 1 = 1",
        "UPDATE t",
        "UPDATE t WHERE a = 1",
        "DELETE FROM t AS",
    ] {
        assert!(needs_rows_precheck(stmt), "{stmt} needs a count");
        assert_eq!(count_sql(stmt), None, "{stmt}");
    }
}

#[test]
fn dml_validation_and_the_count_read_the_same_first_token() {
    // validate_dml_sql used to skip a leading `;` or `(` and accept the next word, while the
    // count looked at the first token only, so the statement ran uncounted.
    for stmt in [
        "; DELETE FROM t",
        ";UPDATE t SET a = 1",
        "( DELETE FROM t",
        "'x' DELETE FROM t",
    ] {
        assert_eq!(
            validate_dml_sql(stmt),
            Err("UNKNOWN_STATEMENT".to_string()),
            "{stmt}"
        );
    }
    // Comments and space before the verb are skipped by both.
    for stmt in ["/* a */ -- b\n DELETE FROM t", "\n\t UPDATE t SET a = 1"] {
        assert!(validate_dml_sql(stmt).is_ok(), "{stmt}");
        assert!(needs_rows_precheck(stmt), "{stmt}");
    }
}

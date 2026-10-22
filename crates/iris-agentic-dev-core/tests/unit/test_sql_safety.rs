// Unit tests for validate_read_only_sql() — SQL safety gate for iris_query.
// These tests make no IRIS connections.

use iris_agentic_dev_core::tools::validate_read_only_sql;

// ── Basic pass ────────────────────────────────────────────────────────────────

#[test]
fn test_select_passes() {
    assert!(validate_read_only_sql("SELECT * FROM foo").is_ok());
}

#[test]
fn test_select_with_where_passes() {
    assert!(
        validate_read_only_sql("SELECT id, name FROM MyApp.Patient WHERE Status = 'active'")
            .is_ok()
    );
}

// ── All 14 blocked keywords ───────────────────────────────────────────────────

#[test]
fn test_blocked_insert() {
    let r = validate_read_only_sql("INSERT INTO foo VALUES (1)");
    assert_eq!(r, Err("INSERT".to_string()));
}

#[test]
fn test_blocked_update() {
    let r = validate_read_only_sql("UPDATE foo SET x = 1");
    assert_eq!(r, Err("UPDATE".to_string()));
}

#[test]
fn test_blocked_delete() {
    let r = validate_read_only_sql("DELETE FROM foo");
    assert_eq!(r, Err("DELETE".to_string()));
}

#[test]
fn test_blocked_drop() {
    let r = validate_read_only_sql("DROP TABLE foo");
    assert_eq!(r, Err("DROP".to_string()));
}

#[test]
fn test_blocked_alter() {
    let r = validate_read_only_sql("ALTER TABLE foo ADD COLUMN x INT");
    assert_eq!(r, Err("ALTER".to_string()));
}

#[test]
fn test_blocked_create() {
    let r = validate_read_only_sql("CREATE TABLE foo (id INT)");
    assert_eq!(r, Err("CREATE".to_string()));
}

#[test]
fn test_blocked_merge() {
    let r = validate_read_only_sql("MERGE INTO foo USING bar");
    assert_eq!(r, Err("MERGE".to_string()));
}

#[test]
fn test_blocked_truncate() {
    let r = validate_read_only_sql("TRUNCATE TABLE foo");
    assert_eq!(r, Err("TRUNCATE".to_string()));
}

#[test]
fn test_blocked_exec() {
    let r = validate_read_only_sql("EXEC sp_something");
    assert_eq!(r, Err("EXEC".to_string()));
}

#[test]
fn test_blocked_execute() {
    let r = validate_read_only_sql("EXECUTE sp_something");
    assert_eq!(r, Err("EXECUTE".to_string()));
}

#[test]
fn test_blocked_bulk() {
    let r = validate_read_only_sql("BULK INSERT foo FROM 'file.csv'");
    assert_eq!(r, Err("BULK".to_string()));
}

#[test]
fn test_blocked_load() {
    let r = validate_read_only_sql("LOAD DATA INTO foo");
    assert_eq!(r, Err("LOAD".to_string()));
}

#[test]
fn test_blocked_kill() {
    let r = validate_read_only_sql("KILL 1234");
    assert_eq!(r, Err("KILL".to_string()));
}

#[test]
fn test_blocked_lock() {
    let r = validate_read_only_sql("LOCK TABLE foo IN EXCLUSIVE MODE");
    assert_eq!(r, Err("LOCK".to_string()));
}

// ── Comment stripping ─────────────────────────────────────────────────────────

#[test]
fn test_block_comment_stripped_allows_select() {
    assert!(validate_read_only_sql("/* DROP TABLE foo */ SELECT 1").is_ok());
}

#[test]
fn test_line_comment_stripped_allows_select() {
    assert!(validate_read_only_sql("-- DROP TABLE foo\nSELECT 1").is_ok());
}

#[test]
fn test_block_comment_with_delete_stripped() {
    assert!(validate_read_only_sql("SELECT /* DELETE */ * FROM foo").is_ok());
}

// ── Quoted identifiers and string literals ────────────────────────────────────

#[test]
fn test_double_quoted_drop_allowed() {
    assert!(validate_read_only_sql(r#"SELECT "DROP" FROM foo"#).is_ok());
}

#[test]
fn test_single_quoted_delete_allowed() {
    assert!(validate_read_only_sql("SELECT 'DELETE' FROM foo").is_ok());
}

#[test]
fn test_call_in_string_literal_allowed() {
    assert!(validate_read_only_sql("SELECT 'CALL me' FROM foo").is_ok());
}

// ── SELECT INTO ───────────────────────────────────────────────────────────────

#[test]
fn test_select_into_table_blocked() {
    let r = validate_read_only_sql("SELECT name INTO #temp FROM foo");
    assert_eq!(r, Err("SELECT INTO".to_string()));
}

#[test]
fn test_select_into_variable_blocked() {
    let r = validate_read_only_sql("SELECT COUNT(*) INTO @count FROM foo");
    assert_eq!(r, Err("SELECT INTO".to_string()));
}

#[test]
fn test_select_subquery_not_blocked() {
    assert!(validate_read_only_sql("SELECT * FROM (SELECT id FROM foo) sub").is_ok());
}

// ── Empty SQL ─────────────────────────────────────────────────────────────────

#[test]
fn test_empty_string_blocked() {
    assert_eq!(validate_read_only_sql(""), Err("EMPTY".to_string()));
}

#[test]
fn test_whitespace_only_blocked() {
    assert_eq!(
        validate_read_only_sql("   \t\n  "),
        Err("EMPTY".to_string())
    );
}

#[test]
fn test_comment_only_blocked() {
    assert_eq!(
        validate_read_only_sql("/* just a comment */"),
        Err("EMPTY".to_string())
    );
}

// ── Case insensitivity ────────────────────────────────────────────────────────

#[test]
fn test_mixed_case_delete_blocked() {
    assert!(validate_read_only_sql("DeLeTe FROM foo").is_err());
}

#[test]
fn test_lowercase_drop_blocked() {
    assert!(validate_read_only_sql("drop table foo").is_err());
}

#[test]
fn test_uppercase_select_passes() {
    assert!(validate_read_only_sql("SELECT id FROM foo").is_ok());
}

// ── Semicolon injection ───────────────────────────────────────────────────────

#[test]
fn test_semicolon_injection_drop_blocked() {
    let r = validate_read_only_sql("SELECT 1; DROP TABLE foo");
    assert!(r.is_err());
}

#[test]
fn test_semicolon_injection_delete_blocked() {
    let r = validate_read_only_sql("SELECT * FROM foo; DELETE FROM foo");
    assert!(r.is_err());
}

// ── Word boundary (false positive prevention) ─────────────────────────────────

#[test]
fn test_created_at_column_not_blocked() {
    assert!(validate_read_only_sql("SELECT CREATED_AT FROM foo").is_ok());
}

#[test]
fn test_dropped_column_not_blocked() {
    assert!(validate_read_only_sql("SELECT DROPPED FROM foo").is_ok());
}

#[test]
fn test_executor_id_not_blocked() {
    assert!(validate_read_only_sql("SELECT EXECUTOR_ID FROM foo").is_ok());
}

#[test]
fn test_killing_column_not_blocked() {
    assert!(validate_read_only_sql("SELECT KILLING FROM foo").is_ok());
}

// ── CALL stays a read (033), with its limit stated ───────────────────────────

/// 033 left CALL out: a procedure may write, but blocking CALL breaks reading through projected
/// class queries, and `SELECT Pkg.Proc()` calls the same procedure as a function, which no
/// lexical check can tell from `SELECT LENGTH(x)`. Re-decided 2026-10-22 (spec 140): kept, and
/// the `iris_query` description says read mode does not stop a procedure with side effects.
#[test]
fn test_call_not_blocked() {
    assert!(validate_read_only_sql("CALL MyStoredProc()").is_ok());
}

// ── SC-003: Representative SELECT queries — zero false positives ──────────────

#[test]
fn test_sc003_fifty_select_queries() {
    let queries = [
        "SELECT * FROM MyApp.Patient",
        "SELECT ID, Name FROM MyApp.Patient WHERE Status = 'active'",
        "SELECT COUNT(*) FROM MyApp.Orders",
        "SELECT TOP 10 * FROM MyApp.Log ORDER BY %ID DESC",
        "SELECT p.ID, p.Name FROM MyApp.Patient p WHERE p.DOB > '2000-01-01'",
        "SELECT DISTINCT Namespace FROM %SYS.Namespace",
        "SELECT Name, Super FROM %Dictionary.ClassDefinition WHERE Abstract = 0",
        "SELECT ID, TimeStamp, ErrorCode FROM %SYSTEM.Error ORDER BY TimeStamp DESC",
        "SELECT * FROM MyApp.Orders WHERE Total > 100.00",
        "SELECT SUM(Amount) FROM MyApp.Transactions WHERE Year = 2026",
        "SELECT a.ID, b.Name FROM TableA a JOIN TableB b ON a.BID = b.ID",
        "SELECT * FROM MyApp.Patient WHERE Name LIKE '%Smith%'",
        "SELECT ID FROM Ens.MessageHeader WHERE Status = 'Complete'",
        "SELECT ConfigName, Text FROM Ens_Util.Log WHERE TimeLogged > '2026-01-01'",
        "SELECT * FROM %UnitTest_Result.TestInstance ORDER BY %ID DESC",
        "SELECT Name FROM %Dictionary.MethodDefinition WHERE parent = 'MyClass'",
        "SELECT ID, Value FROM %Library.Global_Get('%SYS', '^%SYS(\"SystemMode\")')",
        "SELECT * FROM MyApp.Config WHERE Active = 1",
        "SELECT ID, Name, Status FROM MyApp.Production",
        "SELECT COUNT(DISTINCT PatientID) FROM MyApp.Visit",
        "SELECT * FROM MyApp.Audit WHERE UserID = ? AND EventDate > ?",
        "SELECT TOP 1 * FROM MyApp.Session WHERE Token = ?",
        "SELECT p.*, o.Total FROM Patient p LEFT JOIN Orders o ON p.ID = o.PatID",
        "SELECT * FROM (SELECT ID, Name FROM MyApp.Patient WHERE Active = 1) sub",
        "SELECT COALESCE(Name, 'Unknown') FROM MyApp.Contact",
        "SELECT CASE WHEN Total > 1000 THEN 'High' ELSE 'Low' END FROM MyApp.Orders",
        "SELECT * FROM MyApp.Inventory WHERE Qty BETWEEN 10 AND 100",
        "SELECT ID FROM MyApp.Task WHERE Status IN ('Open', 'Pending')",
        "SELECT * FROM MyApp.Log WHERE Message IS NOT NULL",
        "SELECT MAX(Version), MIN(Version) FROM MyApp.Package",
        "SELECT Name, COUNT(*) cnt FROM MyApp.Tag GROUP BY Name ORDER BY cnt DESC",
        "SELECT * FROM MyApp.Mapping WHERE Source LIKE 'PROD%'",
        "SELECT * FROM Config.CPF WHERE Section = 'Startup'",
        "SELECT ID, Description FROM MyApp.Error WHERE Severity > 2",
        "SELECT * FROM MyApp.Schedule WHERE NextRun < NOW()",
        "SELECT p.Name, COUNT(v.ID) visits FROM Patient p JOIN Visit v ON p.ID = v.PID GROUP BY p.Name",
        "SELECT UPPER(Name) FROM MyApp.Contact",
        "SELECT SUBSTRING(Code, 1, 3) AS Prefix FROM MyApp.ICD",
        "SELECT * FROM Ens.Queue_Enumerate()",
        "SELECT * FROM MyApp.Cache WHERE Expiry > CURRENT_TIMESTAMP",
        "SELECT * FROM %SYS.Namespace WHERE Status = 'Active'",
        "SELECT ID FROM MyApp.Alert WHERE Acknowledged = 0 ORDER BY Created",
        "SELECT Name FROM %Library.ClassDefinition WHERE System = 0",
        "SELECT * FROM MyApp.Token WHERE UserID = ? AND Revoked = 0",
        "SELECT AVG(ResponseTime) FROM MyApp.APILog WHERE Date = TODAY()",
        "SELECT * FROM MyApp.Feature WHERE Enabled = 1 AND Tier <= ?",
        "SELECT YEAR(OrderDate), SUM(Total) FROM MyApp.Order GROUP BY YEAR(OrderDate)",
        "SELECT * FROM MyApp.Import WHERE Status <> 'Complete'",
        "SELECT ID, Hash FROM MyApp.Document WHERE Size > 0",
        "SELECT * FROM MyApp.Permission WHERE Role = ? AND Resource = ?",
    ];
    for q in &queries {
        assert!(
            validate_read_only_sql(q).is_ok(),
            "False positive for query: {q}"
        );
    }
}

// ---------------------------------------------------------------------------
// Additional coverage tests for validate_read_only_sql escaping logic
// ---------------------------------------------------------------------------

#[test]
fn validate_read_only_sql_single_quoted_with_backslash_escape() {
    // Single-quoted string with backslash-escaped quote: 'it\'s'
    assert_eq!(
        validate_read_only_sql("SELECT * FROM t WHERE note = 'it\\'s fine'"),
        Ok(())
    );
}

#[test]
fn validate_read_only_sql_single_quoted_with_multiple_escapes() {
    // Multiple backslash escapes in a single-quoted string
    assert_eq!(
        validate_read_only_sql("SELECT name, msg FROM t WHERE msg = 'it\\'s a \\\\ backslash'"),
        Ok(())
    );
}

#[test]
fn validate_read_only_sql_double_quoted_identifier_at_start() {
    // Double-quoted identifier before SELECT keyword
    assert_eq!(validate_read_only_sql("\"CREATE\" SELECT * FROM t"), Ok(()));
}

#[test]
fn validate_read_only_sql_double_quoted_identifiers_multiple() {
    // Multiple double-quoted identifiers before SELECT
    assert_eq!(
        validate_read_only_sql("\"INSERT\" \"DELETE\" SELECT id FROM t"),
        Ok(())
    );
}

#[test]
fn validate_read_only_sql_mixed_quotes_before_select() {
    // Mix of single-quoted and double-quoted before SELECT keyword
    assert_eq!(
        validate_read_only_sql("'INSERT_LITERAL' \"CREATE\" SELECT COUNT(*) FROM t"),
        Ok(())
    );
}

/// GRANT and REVOKE change privileges; neither is a read. They passed the read-only check (AVA
/// scan, 2026-10-22).
#[test]
fn grant_and_revoke_are_not_reads() {
    for sql in [
        "GRANT %ALL TO UnknownUser",
        "revoke SELECT ON Sample.Person FROM X",
    ] {
        assert!(validate_read_only_sql(sql).is_err(), "{sql}");
    }
    assert!(validate_read_only_sql("SELECT Callback, Granted FROM T WHERE x = 'GRANT'").is_ok());
}

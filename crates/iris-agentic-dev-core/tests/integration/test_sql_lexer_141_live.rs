//! Spec 141 T214, T217, T221 against `iris-dev-iris`: the validators read SQL the way IRIS does.
//!
//! Each lexing rule is measured here against IRIS itself, and then the tool that depends on it is
//! driven through the binary: a read that IRIS accepts is not refused, a statement IRIS would run
//! as two is refused before it is sent, `DELETE`/`UPDATE` row counts are taken from the statement's
//! own target, `iris_table_info` reads a delimited table name, and `body_where` still
//! filters.
//!
//! Run, serialised behind the shared lock:
//!
//! ```text
//! lockf -k /tmp/iad-141-iris.lock cargo test --features testing --test integration \
//!     sql_lexer_141 -- --ignored --test-threads=1
//! ```

use iris_agentic_dev_core::testing::{
    data_reads_allowed_env, live_env, require_iad_binary, seed_interop_fixture, McpSession,
};
use serde_json::{json, Value};

const SCHEMA: &str = "IadT141";

fn session() -> McpSession {
    let mut env = live_env();
    env.push(("IRIS_WRITE_TOOLS_ENABLED".to_string(), "1".to_string()));
    env.extend(data_reads_allowed_env());
    McpSession::start(&env)
}

fn call(mcp: &mut McpSession, tool: &str, args: Value) -> Value {
    let answer = mcp.call(tool, &args);
    let text = answer
        .pointer("/result/content/0/text")
        .and_then(Value::as_str)
        .unwrap_or_else(|| panic!("no text content in {answer}"));
    serde_json::from_str(text).unwrap_or_else(|e| panic!("not JSON ({e}): {text}"))
}

fn sql(mcp: &mut McpSession, query: &str) -> Value {
    call(mcp, "iris_query", json!({"query": query}))
}

fn exec(mcp: &mut McpSession, code: &str) -> Value {
    call(mcp, "iris_execute", json!({"code": code}))
}

fn count(mcp: &mut McpSession, table: &str) -> i64 {
    let r = sql(mcp, &format!("SELECT COUNT(*) AS c FROM {table}"));
    r["rows"][0]["c"]
        .as_i64()
        .unwrap_or_else(|| panic!("no count in {r}"))
}

/// A table with three rows, one of them with multibyte text, and a table whose name needs quotes.
fn make_tables(mcp: &mut McpSession) {
    drop_tables(mcp);
    let r = exec(
        mcp,
        &format!(
            r#"Set tSC=1
For tDDL="CREATE TABLE {SCHEMA}.Items (Name VARCHAR(50))","CREATE TABLE {SCHEMA}.""Odd Name"" (Name VARCHAR(50))","INSERT INTO {SCHEMA}.Items (Name) VALUES ('plain')","INSERT INTO {SCHEMA}.Items (Name) VALUES ('ŉé日本')","INSERT INTO {SCHEMA}.Items (Name) VALUES ('other')","INSERT INTO {SCHEMA}.""Odd Name"" (Name) VALUES ('d')" {{
  Set tRS=##class(%SQL.Statement).%ExecDirect(,tDDL)
  If tRS.%SQLCODE<0 {{ Write "FAIL:",tDDL,":",tRS.%Message,! Set tSC=0 }}
}}
Write $Select(tSC:"READY",1:"BROKEN")"#
        ),
    );
    assert!(
        r["output"].as_str().unwrap_or("").contains("READY"),
        "fixture tables were not created: {r}"
    );
}

fn drop_tables(mcp: &mut McpSession) {
    exec(
        mcp,
        &format!(
            r#"For tDDL="DROP TABLE {SCHEMA}.Items","DROP TABLE {SCHEMA}.""Odd Name""" {{ Set tRS=##class(%SQL.Statement).%ExecDirect(,tDDL) }}
Write "DROPPED""#
        ),
    );
}

/// IRIS's own lexing, measured: what a literal, an identifier and a comment are.
#[test]
#[ignore = "requires live IRIS (iris-dev-iris) and the built binary"]
fn iris_lexes_the_way_the_validators_do() {
    let Some(_bin) = require_iad_binary() else {
        return;
    };
    let mut mcp = session();

    // `--` and `/* */` inside a literal are text; `\` is an ordinary character; `''` is one quote.
    let r = sql(
        &mut mcp,
        "SELECT 'a--b' AS x, '/*c*/' AS y, 'it''s' AS z, 'back\\' AS w",
    );
    assert_eq!(r["success"], true, "{r}");
    assert_eq!(r["rows"][0]["x"], "a--b", "{r}");
    assert_eq!(r["rows"][0]["y"], "/*c*/", "{r}");
    assert_eq!(r["rows"][0]["z"], "it's", "{r}");
    assert_eq!(r["rows"][0]["w"], "back\\", "{r}");

    // A delimited identifier doubles its quote, and holds `--` and `/*` as text.
    let r = sql(
        &mut mcp,
        "SELECT 1 AS \"a\"\"b\", 2 AS \"x--y\", 3 AS \"p/*q\" /* c1 -- still comment */ -- tail 'open",
    );
    assert_eq!(r["success"], true, "{r}");
    assert_eq!(r["rows"][0]["a\"b"], 1, "{r}");
    assert_eq!(r["rows"][0]["x--y"], 2, "{r}");
    assert_eq!(r["rows"][0]["p/*q"], 3, "{r}");

    // A line comment ends at the newline, and a `/*` inside it opens nothing.
    let r = sql(
        &mut mcp,
        "SELECT 1 AS a -- c1\n, 2 AS b -- /* not a block\n, 3 AS c",
    );
    assert_eq!(r["success"], true, "{r}");
    assert_eq!(r["rows"][0]["c"], 3, "{r}");

    // `5--1` is 5 and a comment.
    let r = sql(&mut mcp, "SELECT 5--1 AS x");
    assert_eq!(r["success"], true, "{r}");
    assert_eq!(r["rows"][0]["HostVar_1"], 5, "{r}");

    // Block comments do not nest: the first `*/` ends one, so the text after it is code.
    let r = sql(&mut mcp, "SELECT 1 AS a /* x /* y */ , 2 AS b */");
    assert_eq!(r["success"], false, "{r}");
    assert!(r["error"].as_str().unwrap_or("").contains("SQLCODE"), "{r}");

    // A literal or comment left open is a syntax error.
    let r = sql(&mut mcp, "SELECT 'a\nb' AS x, 'unterminated");
    assert_eq!(r["success"], false, "{r}");
    let r = sql(&mut mcp, "SELECT 1 AS a /* never closed");
    assert_eq!(r["success"], false, "{r}");

    // Multibyte text passes through unchanged.
    let r = sql(&mut mcp, "SELECT 'héllo→日本😀' AS x, 1 AS \"é\"");
    assert_eq!(r["success"], true, "{r}");
    assert_eq!(r["rows"][0]["x"], "héllo→日本😀", "{r}");
}

/// The read-only check refuses what IRIS would run as a write, and not what IRIS reads as text.
#[test]
#[ignore = "requires live IRIS (iris-dev-iris) and the built binary"]
fn read_mode_follows_the_lexing_it_measured() {
    let Some(_bin) = require_iad_binary() else {
        return;
    };
    let mut mcp = session();
    make_tables(&mut mcp);
    let before = count(&mut mcp, &format!("{SCHEMA}.Items"));

    // The old pre-pass cut the line at the `--` inside the literal, and took `\'` as an escape.
    for hidden in [
        format!("SELECT '--' AS a ; DROP TABLE {SCHEMA}.Items"),
        format!("SELECT 'a\\' ; DROP TABLE {SCHEMA}.Items"),
        format!("SELECT 1 -- /*\nDROP TABLE {SCHEMA}.Items /* */"),
        format!("SELECT 1 /* a /* b */ DROP TABLE {SCHEMA}.Items */"),
    ] {
        let r = sql(&mut mcp, &hidden);
        assert_eq!(r["error_code"], "SQL_WRITE_BLOCKED", "{hidden}: {r}");
        assert_eq!(r["blocked_keyword"], "DROP", "{hidden}: {r}");
    }
    assert_eq!(count(&mut mcp, &format!("{SCHEMA}.Items")), before);

    // Text IRIS reads as a literal or an identifier is not a keyword.
    let r = sql(
        &mut mcp,
        &format!("SELECT Name FROM {SCHEMA}.Items WHERE Name = 'DROP TABLE x -- y' OR Name = 'copy INTO z'"),
    );
    assert_eq!(r["success"], true, "{r}");
    let r = sql(&mut mcp, "SELECT 1 AS \"a\"\" DROP \"\"b\"");
    assert_eq!(r["success"], true, "{r}");
    // Multibyte text, which used to panic the validator on a `SELECT ... INTO` after it.
    let r = sql(&mut mcp, "SELECT 'ŉé日本' AS x INTO :v");
    assert_eq!(r["error_code"], "SQL_WRITE_BLOCKED", "{r}");
    assert_eq!(r["blocked_keyword"], "SELECT INTO", "{r}");
    drop_tables(&mut mcp);
}

/// `mode="write"`: the row count is taken from the statement's own target, and a leading comment
/// does not turn it off.
#[test]
#[ignore = "requires live IRIS (iris-dev-iris) and the built binary"]
fn write_mode_counts_the_rows_of_its_own_target() {
    let Some(_bin) = require_iad_binary() else {
        return;
    };
    let mut mcp = session();
    make_tables(&mut mcp);
    let table = format!("{SCHEMA}.Items");

    // A leading comment, a leading space and a leading newline each used to skip the check.
    for stmt in [
        format!("/* tidy up */ DELETE FROM {table}"),
        format!("  \n DELETE FROM {table}"),
        format!("-- tidy up\nDELETE FROM {table}"),
        format!("DELETE /* FROM {SCHEMA}.\"Odd Name\" */ FROM {table}"),
        format!("/* x */ UPDATE {table} SET Name = 'z'"),
    ] {
        let r = call(
            &mut mcp,
            "iris_query",
            json!({"query": stmt, "mode": "write", "max_rows_affected": 1}),
        );
        assert_eq!(r["error_code"], "ROWS_LIMIT_EXCEEDED", "{stmt}: {r}");
        assert_eq!(count(&mut mcp, &table), 3, "{stmt} must not have run");
    }

    // The WHERE holds a subquery with its own FROM and WHERE; multibyte text in a literal.
    let r = call(
        &mut mcp,
        "iris_query",
        json!({
            "query": format!("DELETE FROM {table} WHERE Name IN (SELECT Name FROM {table} WHERE Name = 'ŉé日本')"),
            "mode": "write",
            "max_rows_affected": 1
        }),
    );
    assert_eq!(r["success"], true, "{r}");
    assert_eq!(r["rows_affected"], 1, "{r}");
    assert!(r.get("rows_check_skipped").is_none(), "the check ran: {r}");
    assert_eq!(count(&mut mcp, &table), 2);

    // A delimited table name, and a `WHERE` nested in a `SET` subquery.
    let r = call(
        &mut mcp,
        "iris_query",
        json!({
            "query": format!("UPDATE {SCHEMA}.\"Odd Name\" SET Name = (SELECT MAX(Name) FROM {table} WHERE Name = 'plain') WHERE Name = 'd'"),
            "mode": "write",
            "max_rows_affected": 1
        }),
    );
    assert_eq!(r["success"], true, "{r}");
    assert_eq!(r["rows_affected"], 1, "{r}");
    drop_tables(&mut mcp);
}

/// `iris_table_info` reads `Schema."Delimited Name"`. (IRIS refuses a dot inside a table name, so
/// the split at unquoted dots only is pinned in the unit tests; the live part is that a delimited
/// name survives being unquoted for the lookup and quoted again for the row count.)
#[test]
#[ignore = "requires live IRIS (iris-dev-iris) and the built binary"]
fn table_info_reads_a_delimited_table_name() {
    let Some(_bin) = require_iad_binary() else {
        return;
    };
    let mut mcp = session();
    make_tables(&mut mcp);

    let quoted = format!("{SCHEMA}.\"Odd Name\"");
    let r = call(
        &mut mcp,
        "iris_table_info",
        json!({"table": quoted, "include_row_count": true}),
    );
    assert_eq!(r["success"], true, "the table was not found: {r}");
    assert_eq!(r["result"]["table"], quoted.as_str(), "{r}");
    assert_eq!(r["result"]["row_count"], 1, "{r}");

    let plain = format!("{SCHEMA}.Items");
    let r = call(
        &mut mcp,
        "iris_table_info",
        json!({"table": plain, "include_row_count": true}),
    );
    assert_eq!(r["success"], true, "{r}");
    assert_eq!(r["result"]["row_count"], 3, "{r}");

    // A name that does not exist still says so.
    let r = call(
        &mut mcp,
        "iris_table_info",
        json!({"table": format!("{SCHEMA}.\"No Such\"")}),
    );
    assert_eq!(r["success"], false, "{r}");
    drop_tables(&mut mcp);
}

/// `body_where` still filters, and refuses each way out of its parentheses.
#[test]
#[ignore = "requires live IRIS (iris-dev-iris) and the built binary"]
fn body_where_filters_and_refuses_what_leaves_the_expression() {
    let Some(_bin) = require_iad_binary() else {
        return;
    };
    let fixture = seed_interop_fixture();
    let mut mcp = session();
    let body = |where_: &str| {
        json!({
            "what": "messages",
            "source": fixture.source,
            "body_class": fixture.body_class,
            "body_where": where_,
            "body_select": ["StringValue"],
            "limit": 3
        })
    };

    // A legitimate filter with a parenthesised OR returns rows.
    let r = call(
        &mut mcp,
        "iris_interop_query",
        body("StringValue LIKE 'IAD fixture body%' AND (StringValue IS NOT NULL OR 1 < 0)"),
    );
    assert_eq!(r["success"], true, "{r}");
    assert!(
        r["messages"].as_array().is_some_and(|m| !m.is_empty()),
        "the filter returned nothing: {r}"
    );

    // Text that looks like a comment or a separator inside a literal is just text: it reaches IRIS
    // and matches nothing.
    let r = call(
        &mut mcp,
        "iris_interop_query",
        body("StringValue = 'x;--y /* z'"),
    );
    assert_eq!(r["success"], true, "{r}");
    assert_eq!(r["count"], 0, "{r}");

    // Each way out of the expression is refused before IRIS sees it.
    for bad in [
        "1=1) UNION SELECT Name FROM Security.Users WHERE (1=1",
        "StringValue = 'x') UNION ALL SELECT 1 FROM Ens.MessageHeader WHERE ('1'='1",
        "1=1; DROP TABLE x",
        "1=1 -- rest",
        "StringValue IN (SELECT Name FROM Security.Users)",
        "(StringValue IS NOT NULL",
        "StringValue = 'x\\' UNION SELECT 1 --",
    ] {
        let r = call(&mut mcp, "iris_interop_query", body(bad));
        assert_eq!(r["error_code"], "INVALID_PARAMS", "{bad}: {r}");
    }
}

/// Body mode is a bulk-data call: refused under the default policy, admitted when the server
/// allows data reads. The header listing is not.
#[test]
#[ignore = "requires live IRIS (iris-dev-iris) and the built binary"]
fn body_mode_sits_under_the_bulk_data_gate() {
    let Some(_bin) = require_iad_binary() else {
        return;
    };
    let fixture = seed_interop_fixture();
    // No operator file: `dataPolicy` is its default, `block`.
    let mut closed = McpSession::start(&live_env());
    for args in [
        json!({"what": "messages", "body_class": fixture.body_class}),
        json!({"what": "messages", "body_class": fixture.body_class, "body_where": "1=1"}),
        json!({"what": "messages", "body_class": fixture.body_class, "body_select": ["StringValue"]}),
    ] {
        let r = call(&mut closed, "iris_interop_query", args.clone());
        assert_eq!(r["error_code"], "DATA_POLICY_BLOCKED", "{args}: {r}");
    }
    let r = call(
        &mut closed,
        "iris_interop_query",
        json!({"what": "messages", "source": fixture.source, "limit": 2}),
    );
    assert_eq!(r["success"], true, "headers are not a body read: {r}");

    let mut open = session();
    let r = call(
        &mut open,
        "iris_interop_query",
        json!({"what": "messages", "source": fixture.source, "body_class": fixture.body_class, "limit": 2}),
    );
    assert_eq!(r["success"], true, "dataPolicy=allow admits body mode: {r}");
}

/// T213: read mode sends SELECT, WITH and EXPLAIN, and nothing else. IRIS's EXPLAIN plans a
/// statement without running it (measured: `EXPLAIN DELETE` left the row, `EXPLAIN CREATE TABLE`
/// is SQLCODE -481), which is why it is on the list.
#[test]
#[ignore = "requires live IRIS (iris-dev-iris) and the built binary"]
fn read_mode_sends_select_with_and_explain_only() {
    let Some(_bin) = require_iad_binary() else {
        return;
    };
    let mut mcp = session();
    for ok in [
        "SELECT 1 AS a",
        "WITH c AS (SELECT 1 AS a) SELECT a FROM c",
        "EXPLAIN SELECT 1 AS a",
        "-- lead\n/* lead */ SELECT 1 AS a;",
    ] {
        let r = sql(&mut mcp, ok);
        assert_eq!(r["success"], true, "{ok}: {r}");
    }
    for mode in ["read", "Read", " READ "] {
        for bad in [
            "CALL %SYSTEM.SQL.Functions_DATEADD('d',1,'2026-01-01')",
            "TUNE TABLE IadT141.Nothing",
            "SELECT 1 AS a; SELECT 2 AS b",
        ] {
            let r = call(&mut mcp, "iris_query", json!({"query": bad, "mode": mode}));
            assert_eq!(r["error_code"], "SQL_WRITE_BLOCKED", "{mode} {bad}: {r}");
        }
    }
}

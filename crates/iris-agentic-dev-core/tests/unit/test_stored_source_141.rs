//! Spec 141 US3 follow-ups (T165, T166, T168): the stored-source walk and the DDL scan.
//!
//! The walk is pure logic over a `read` function, so these tests pin what live IRIS cannot show:
//! how many times a document is read, that a cycle ends, and what each outcome of a read does. The
//! proof that IRIS compiles nothing when the walk refuses is `test_compile_gate_141_live`.

use iris_agentic_dev_core::policy::code_edit_gate::{
    check_compile_time_code_mode, check_sql_code_edit, included_documents,
};
use iris_agentic_dev_core::tools::stored_check::{
    check_documents, compile_list_arg, list_code, parse_read, read_code, split_targets,
    wildcard_regex, Outcome, Seed, Stored,
};
use std::cell::RefCell;
use std::collections::HashMap;

// ── included_documents ───────────────────────────────────────────────────

#[test]
fn a_class_names_its_includes_in_every_spelling() {
    let cls = "Include MyInc\nInclude (%occStatus, Other.Inc)\nIncludeGenerator Gen\n\nClass My.X\n{\n}\n";
    assert_eq!(
        included_documents(cls, "My.X.cls"),
        ["MyInc.inc", "%occStatus.inc", "Other.Inc.inc", "Gen.inc"]
    );
}

#[test]
fn an_include_directive_in_a_method_body_or_routine_counts() {
    let cls = "Class My.X\n{\n\nClassMethod M()\n{\n    #include InBody\n    Quit\n}\n\n}\n";
    assert_eq!(included_documents(cls, "My.X.cls"), ["InBody.inc"]);
    let mac = "ROUTINE R\nR ;\n#include %occInclude\n#Include Second\n Quit\n";
    assert_eq!(
        included_documents(mac, "R.mac"),
        ["%occInclude.inc", "Second.inc"]
    );
    assert_eq!(
        included_documents("#include FromInc\n", "Top.inc"),
        ["FromInc.inc"]
    );
}

#[test]
fn includes_in_comments_and_strings_are_not_includes() {
    let cls = "/// Include InDoc\n// #include InLine\nClass My.X\n{\n\nClassMethod M()\n{\n    Set x = \"#include InString\"\n    ; #include InSemi\n    Quit\n}\n\n}\n";
    let got = included_documents(cls, "My.X.cls");
    assert!(got.is_empty(), "{got:?}");
}

#[test]
fn an_include_named_twice_is_listed_once_whatever_its_case() {
    let cls = "Include MyInc\nInclude (myinc, Other)\n\nClass My.X\n{\n}\n";
    assert_eq!(
        included_documents(cls, "My.X.cls"),
        ["MyInc.inc", "Other.inc"]
    );
}

#[test]
fn class_xml_names_includes_in_its_include_code_element() {
    let xml = "<?xml version=\"1.0\"?>\n<Export>\n<Class name=\"My.X\">\n<IncludeCode>One,Two</IncludeCode>\n</Class>\n</Export>\n";
    assert_eq!(included_documents(xml, "My.X.cls"), ["One.inc", "Two.inc"]);
}

#[test]
fn a_document_that_is_not_source_has_no_includes() {
    assert!(included_documents("Include X\n", "My.X.int").is_empty());
}

// ── targets and wildcards ────────────────────────────────────────────────

#[test]
fn a_comma_list_is_split_and_trimmed() {
    assert_eq!(
        split_targets(" A.B , C.cls,, D.* "),
        ["A.B", "C.cls", "D.*"]
    );
}

#[test]
fn a_wildcard_matches_by_package_and_ignores_case() {
    let re = wildcard_regex("IadGate.*");
    assert!(re.is_match("IadGate.CExec.cls"));
    assert!(re.is_match("iadgate.cexec.cls"));
    assert!(!re.is_match("IadGateX.Other.cls"));
    let any = wildcard_regex("IadGate.C*.cls");
    assert!(any.is_match("IadGate.CExec.cls"));
    assert!(!any.is_match("IadGate.Probe.cls"));
}

#[test]
fn a_pattern_with_regex_metacharacters_does_not_match_everything() {
    // The old expansion fell back to `.*` when the pattern was not a valid regex, which listed
    // every class in the namespace.
    for pattern in ["A(b*", "A[b*", "A+*", "A|B*", "*)"] {
        let re = wildcard_regex(pattern);
        assert!(!re.is_match("Unrelated.Class.cls"), "{pattern} matched");
    }
    assert!(wildcard_regex("A.B+*").is_match("A.B+C.cls"));
}

// ── the walk ─────────────────────────────────────────────────────────────

const DANGEROUS: &str = "ROUTINE Bad [Type=INC]\n#execute Do ##class(My.Probe).Mark(\"inc\")\n";

struct Store {
    docs: HashMap<&'static str, Stored>,
    reads: RefCell<Vec<String>>,
}

impl Store {
    fn new(docs: Vec<(&'static str, Stored)>) -> Self {
        Self {
            docs: docs.into_iter().collect(),
            reads: RefCell::new(Vec::new()),
        }
    }

    async fn read(&self, name: String) -> Stored {
        self.reads.borrow_mut().push(name.clone());
        match self.docs.get(name.as_str()) {
            Some(Stored::Found(s)) => Stored::Found(s.clone()),
            Some(Stored::Unreadable(s)) => Stored::Unreadable(s.clone()),
            Some(Stored::Transport(s)) => Stored::Transport(s.clone()),
            Some(Stored::Missing) | None => Stored::Missing,
        }
    }
}

fn found(src: &str) -> Stored {
    Stored::Found(src.to_string())
}

fn refusal(outcome: Outcome) -> serde_json::Value {
    match outcome {
        Outcome::Refused(v) => v,
        other => panic!("expected a refusal, got {other:?}"),
    }
}

#[tokio::test]
async fn a_dangerous_include_two_levels_down_is_found_and_named() {
    let store = Store::new(vec![
        ("My.Top.cls", found("Include A\n\nClass My.Top\n{\n}\n")),
        ("A.inc", found("#include B\n")),
        ("B.inc", found(DANGEROUS)),
    ]);
    let v =
        refusal(check_documents(|n| store.read(n), vec![Seed::Stored("My.Top.cls".into())]).await);
    assert_eq!(v["error_code"], "COMPILE_TIME_EXEC_BLOCKED", "{v}");
    assert_eq!(v["document"], "B.inc", "{v}");
    assert_eq!(v["included_by"], "A.inc", "{v}");
}

#[tokio::test]
async fn each_document_is_read_once_and_a_cycle_ends() {
    let store = Store::new(vec![
        ("My.Top.cls", found("Include A, B\n\nClass My.Top\n{\n}\n")),
        ("A.inc", found("#include B\n#include A\n")),
        ("B.inc", found("#include A\n")),
    ]);
    let outcome = check_documents(
        |n| store.read(n),
        vec![
            Seed::Stored("My.Top.cls".into()),
            Seed::Stored("my.top.cls".into()),
        ],
    )
    .await;
    assert!(matches!(outcome, Outcome::Clear), "{outcome:?}");
    let mut reads = store.reads.borrow().clone();
    reads.sort();
    assert_eq!(reads, ["A.inc", "B.inc", "My.Top.cls"], "each exactly once");
}

#[tokio::test]
async fn system_includes_are_not_followed() {
    // `%` includes carry `##expression` in their `#define`s by design.
    let store = Store::new(vec![
        (
            "My.Top.cls",
            found("Include (%sySystem, Own)\n\nClass My.Top\n{\n}\n"),
        ),
        ("%sySystem.inc", found(DANGEROUS)),
        ("Own.inc", found("#define Answer 42\n")),
    ]);
    let outcome = check_documents(|n| store.read(n), vec![Seed::Stored("My.Top.cls".into())]).await;
    assert!(matches!(outcome, Outcome::Clear), "{outcome:?}");
    let mut reads = store.reads.borrow().clone();
    reads.sort();
    assert_eq!(reads, ["My.Top.cls", "Own.inc"]);
}

#[tokio::test]
async fn a_missing_target_or_include_is_left_to_the_compile() {
    let store = Store::new(vec![(
        "My.Top.cls",
        found("Include Gone\n\nClass My.Top\n{\n}\n"),
    )]);
    let outcome = check_documents(
        |n| store.read(n),
        vec![
            Seed::Stored("My.Top.cls".into()),
            Seed::Stored("My.Nope.cls".into()),
        ],
    )
    .await;
    assert!(matches!(outcome, Outcome::Clear), "{outcome:?}");
}

#[tokio::test]
async fn a_target_that_cannot_be_read_is_refused() {
    let store = Store::new(vec![("My.Top.cls", Stored::Unreadable("HTTP 500".into()))]);
    let v =
        refusal(check_documents(|n| store.read(n), vec![Seed::Stored("My.Top.cls".into())]).await);
    assert_eq!(v["error_code"], "STORED_SOURCE_UNREADABLE", "{v}");
    assert_eq!(v["success"], false, "{v}");
    assert_eq!(v["document"], "My.Top.cls", "{v}");
    assert!(v["reason"].as_str().unwrap().contains("HTTP 500"), "{v}");
}

#[tokio::test]
async fn an_include_that_cannot_be_read_is_refused_and_names_its_includer() {
    let store = Store::new(vec![
        ("My.Top.cls", found("Include A\n\nClass My.Top\n{\n}\n")),
        ("A.inc", Stored::Unreadable("HTTP 403".into())),
    ]);
    let v =
        refusal(check_documents(|n| store.read(n), vec![Seed::Stored("My.Top.cls".into())]).await);
    assert_eq!(v["error_code"], "STORED_SOURCE_UNREADABLE", "{v}");
    assert_eq!(v["document"], "A.inc", "{v}");
    assert_eq!(v["included_by"], "My.Top.cls", "{v}");
}

#[tokio::test]
async fn a_transport_failure_is_reported_as_one_so_the_caller_can_say_why() {
    let store = Store::new(vec![(
        "My.Top.cls",
        Stored::Transport("CONTAINER_UNREACHABLE: no such container".into()),
    )]);
    let outcome = check_documents(|n| store.read(n), vec![Seed::Stored("My.Top.cls".into())]).await;
    match outcome {
        Outcome::Transport(msg) => assert!(msg.starts_with("CONTAINER_UNREACHABLE"), "{msg}"),
        other => panic!("expected Transport, got {other:?}"),
    }
}

#[tokio::test]
async fn content_already_in_hand_is_checked_and_its_includes_followed() {
    let store = Store::new(vec![("A.inc", found(DANGEROUS))]);
    let local = "Include A\n\nClass My.Local\n{\n}\n";
    let v = refusal(
        check_documents(
            |n| store.read(n),
            vec![Seed::Content {
                doc: "My.Local.cls".into(),
                content: local.into(),
            }],
        )
        .await,
    );
    assert_eq!(v["document"], "A.inc", "{v}");
    assert_eq!(
        store.reads.borrow().as_slice(),
        ["A.inc"],
        "the local text is not read back from the server"
    );
}

#[tokio::test]
async fn clean_documents_are_clear() {
    let store = Store::new(vec![
        ("My.Top.cls", found("Include A\n\nClass My.Top\n{\n}\n")),
        ("A.inc", found("#define Answer 42\n")),
    ]);
    let outcome = check_documents(|n| store.read(n), vec![Seed::Stored("My.Top.cls".into())]).await;
    assert!(matches!(outcome, Outcome::Clear), "{outcome:?}");
}

// ── T168: DDL that creates code ──────────────────────────────────────────

fn ddl_refused(sql: &str) -> serde_json::Value {
    check_sql_code_edit(sql, "srv").unwrap_or_else(|| panic!("should be refused: {sql}"))
}

/// Asserts the DDL gate refuses `sql`.
macro_rules! assert_ddl_refused {
    ($sql:expr $(,)?) => {
        assert!(
            check_sql_code_edit($sql, "srv").is_some(),
            "should be refused: {}",
            $sql
        )
    };
}

/// Asserts the DDL gate lets `sql` through.
macro_rules! assert_ddl_allowed {
    ($sql:expr $(,)?) => {
        if let Some(v) = check_sql_code_edit($sql, "srv") {
            panic!("should be allowed: {}\n{v}", $sql);
        }
    };
}

#[test]
fn a_procedure_body_with_a_compile_time_directive_is_refused() {
    let v = ddl_refused(
        "CREATE PROCEDURE IadGate.Pr() LANGUAGE OBJECTSCRIPT { Set x = ##expression(##class(IadGate.Probe).Mark(\"ddl\")) Quit }",
    );
    assert_eq!(v["error_code"], "COMPILE_TIME_EXEC_BLOCKED", "{v}");
    assert_eq!(v["success"], false, "{v}");
}

#[test]
fn every_code_creating_statement_kind_is_checked() {
    for head in [
        "CREATE PROCEDURE p() LANGUAGE OBJECTSCRIPT",
        "create or replace function f() returns int language objectscript",
        "CREATE METHOD m() LANGUAGE OBJECTSCRIPT",
        "CREATE QUERY q() LANGUAGE OBJECTSCRIPT",
        "CREATE TRIGGER t AFTER INSERT ON s.t LANGUAGE OBJECTSCRIPT",
        "CREATE /* c */ PROCEDURE p() LANGUAGE OBJECTSCRIPT",
        "CREATE\n  PROCEDURE p() LANGUAGE OBJECTSCRIPT",
    ] {
        for directive in [
            "#execute Do ^X",
            "Set x = ##function(1)",
            "Set x = ##EXPRESSION(1)",
        ] {
            let sql = format!("{head} {{ {directive} Quit }}");
            assert_ddl_refused!(&sql);
        }
    }
}

#[test]
fn a_computed_column_in_create_table_is_checked_too() {
    assert_ddl_refused!(
        "CREATE TABLE t (id INT, c VARCHAR(10) COMPUTECODE { Set {c} = ##expression(1) } COMPUTEONCHANGE id)",
    );
}

#[test]
fn a_directive_after_an_earlier_statement_is_found() {
    assert_ddl_refused!(
        "INSERT INTO t VALUES (1); CREATE PROCEDURE p() LANGUAGE OBJECTSCRIPT { #execute Do ^X }",
    );
}

#[test]
fn a_procedure_without_a_compile_time_construct_is_not_refused() {
    for sql in [
        "CREATE PROCEDURE IadGate.Pr() LANGUAGE OBJECTSCRIPT { Set x = 1 Quit }",
        "CREATE PROCEDURE p() LANGUAGE SQL BEGIN SELECT 1 END",
        "CREATE OR REPLACE FUNCTION f() RETURNS INT LANGUAGE OBJECTSCRIPT { Quit 1 }",
        // A runtime directive is allowed, as it is in a class.
        "CREATE PROCEDURE p() LANGUAGE OBJECTSCRIPT { Set x = 1 ; ##expression(1) is only a comment\n Quit }",
        "CREATE PROCEDURE p() LANGUAGE OBJECTSCRIPT { Set s = \"##expression(1)\" Quit }",
    ] {
        assert_ddl_allowed!(sql);
    }
}

#[test]
fn a_directive_after_a_sql_comment_marker_is_still_found() {
    // `--` is two unary minuses in ObjectScript, so what follows it runs.
    assert_ddl_refused!(
        "CREATE PROCEDURE p() LANGUAGE OBJECTSCRIPT { Set x = 1 -- ##expression(1)\n Quit }",
    );
}

#[test]
fn data_that_mentions_a_directive_is_not_ddl() {
    for sql in [
        "INSERT INTO notes VALUES ('use ##expression(1) at compile time')",
        "UPDATE notes SET body = '#execute Do ^X' WHERE id = 1",
        "SELECT * FROM notes WHERE body = '##function(1)'",
    ] {
        assert_ddl_allowed!(sql);
    }
}

#[test]
fn the_document_gate_still_agrees_with_the_ddl_gate_on_what_a_directive_is() {
    // The same text, as a class method body and as a procedure body, is refused by both.
    let body = "Set x = ##expression(1)";
    let cls = format!("Class My.X\n{{\n\nClassMethod M()\n{{\n    {body}\n}}\n\n}}\n");
    assert!(check_compile_time_code_mode(&cls, "My.X.cls").is_some());
    assert_ddl_refused!(&format!(
        "CREATE PROCEDURE p() LANGUAGE OBJECTSCRIPT {{ {body} }}"
    ));
}

// ── the exec reply ───────────────────────────────────────────────────────

#[test]
fn an_exec_reply_with_the_source_between_markers_is_found() {
    let reply = "USER>\n\n~IAD-BEGIN~\nClass A.B\n{\n}\n~IAD-END~\n\nUSER>\n";
    match parse_read(reply) {
        Stored::Found(src) => assert_eq!(src, "Class A.B\n{\n}"),
        other => panic!("{other:?}"),
    }
    match parse_read("~IAD-BEGIN~\r\nline one\r\n~IAD-END~\r\n") {
        Stored::Found(src) => assert_eq!(src, "line one"),
        other => panic!("{other:?}"),
    }
}

#[test]
fn markers_inside_the_source_cannot_change_the_verdict() {
    // A document that quotes the markers is still read whole, and cannot pass for a missing one.
    let reply = "~IAD-BEGIN~\n~IAD-MISSING~\n~IAD-END~ in a comment\n#execute Do ^X\n~IAD-END~\n";
    match parse_read(reply) {
        Stored::Found(src) => assert!(src.contains("#execute"), "{src}"),
        other => panic!("{other:?}"),
    }
}

#[test]
fn an_exec_reply_says_missing_only_for_a_missing_document() {
    assert!(matches!(
        parse_read("USER>\n~IAD-MISSING~\n"),
        Stored::Missing
    ));
}

#[test]
fn an_exec_reply_with_an_error_or_no_markers_is_unreadable() {
    match parse_read("~IAD-ERROR~ ERROR #5001: access denied\n") {
        Stored::Unreadable(r) => assert!(r.contains("access denied"), "{r}"),
        other => panic!("{other:?}"),
    }
    assert!(matches!(parse_read("<PROTECT>\n"), Stored::Unreadable(_)));
    assert!(matches!(parse_read(""), Stored::Unreadable(_)));
    assert!(matches!(
        parse_read("~IAD-BEGIN~\nhalf of the source"),
        Stored::Unreadable(_)
    ));
}

/// T173. `$SYSTEM.OBJ.CompileList` reads documents with their extension; a bare name is a class.
#[test]
fn a_compile_target_becomes_a_list_of_documents() {
    assert_eq!(
        compile_list_arg(" My.Class , My.Other.cls,R.mac, I.inc,,My.*, P.*.cls"),
        "My.Class.cls,My.Other.cls,R.mac,I.inc,My.*.cls,P.*.cls"
    );
}

// ── the docker-exec code carries a name as data, never as structure ──────

/// `code` with the inside of every string literal blanked, so what is left is the structure
/// ObjectScript would parse. A doubled quote stays inside the literal, as it does in IRIS.
fn outside_literals(code: &str) -> String {
    let mut out = String::new();
    let mut in_str = false;
    let mut chars = code.chars().peekable();
    while let Some(c) = chars.next() {
        match (in_str, c) {
            (false, '"') => {
                in_str = true;
                out.push('"');
            }
            (true, '"') if chars.peek() == Some(&'"') => {
                chars.next();
            }
            (true, '"') => {
                in_str = false;
                out.push('"');
            }
            (true, _) => {}
            (false, _) => out.push(c),
        }
    }
    assert!(!in_str, "a literal was left open in:\n{code}");
    out
}

/// Names and patterns that try to close the literal, concatenate, call out, or break the line.
const HOSTILE: [&str; 6] = [
    "A\" Kill ^X Set q=\"",
    "A\"_$ZF(-1,\"id\")_\"",
    "A) Do $ClassMethod(\"%SYSTEM.OBJ\",\"Delete\",\"B\") (",
    "A\"_$C(75,105,108,108)_\"",
    "A\\\" Kill ^X ;",
    "A\"\"\" Kill ^X",
];

macro_rules! assert_same_structure {
    ($hostile:expr, $benign:expr) => {
        assert_eq!(
            outside_literals(&$hostile),
            outside_literals(&$benign),
            "a hostile name changed the code's structure:\n{}",
            $hostile
        )
    };
}

#[test]
fn a_hostile_document_name_does_not_change_the_read_code() {
    for name in HOSTILE {
        let doc = format!("{name}.cls");
        assert_same_structure!(read_code(&doc), read_code("A.cls"));
        let mac = format!("{name}.mac");
        assert_same_structure!(read_code(&mac), read_code("A.mac"));
    }
}

#[test]
fn a_hostile_wildcard_pattern_does_not_change_the_list_code() {
    for name in HOSTILE {
        assert_same_structure!(list_code(&format!("{name}*")), list_code("A*"));
    }
}

#[test]
fn a_newline_in_a_name_stays_on_one_line_and_is_rebuilt_by_char() {
    for code in [read_code("A\nKill ^X.cls"), list_code("A\r\nKill ^X*")] {
        assert!(!code.contains(['\n', '\r']), "raw line break in:\n{code:?}");
        assert!(code.contains("$CHAR("), "no $CHAR splice in:\n{code}");
    }
}

#[test]
fn the_name_is_carried_as_one_quoted_expression() {
    assert!(read_code("A\"B.cls").starts_with("Set d=\"A\"\"B.cls\" "));
    assert!(list_code("A\"B*").contains("rs.Execute(\"A\"\"B*\",1,1,1,1,0,0)"));
}

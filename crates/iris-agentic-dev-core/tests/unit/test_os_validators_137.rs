//! 137: every caller-supplied name that is spliced into ObjectScript, SQL or a URL goes through a
//! validator first. These lists pin what each one accepts (names IRIS really uses) and refuses (text
//! that would close a literal or a call and run something else).

use iris_agentic_dev_core::objectscript::{
    os_str_expr, validate_class_name, validate_global_name, validate_method_name,
    validate_namespace, validate_routine_name, validate_sql_identifier, validate_timestamp,
};

/// Text that turns a splice into code, whichever quoting the site uses.
const CRAFTED: &[&str] = &[
    "X\" Kill ^Y Set z=\"",
    "X) Kill ^Y Set q=$Get(^X",
    "X' OR 1=1 --",
    "X Kill ^Y",
    "X\nKill ^Y",
    "X,Y",
    "@X",
    "X;Y",
    "X{Y}",
    "X$Y",
    "",
];

fn assert_accepts(name: &str, f: fn(&str) -> Result<(), serde_json::Value>, ok: &[&str]) {
    assert!(!ok.is_empty());
    for v in ok {
        assert!(f(v).is_ok(), "{name} must accept {v:?}: {:?}", f(v));
    }
}

fn assert_refuses(
    name: &str,
    f: fn(&str) -> Result<(), serde_json::Value>,
    code: &str,
    bad: &[&str],
) {
    assert!(!bad.is_empty());
    for v in bad {
        let err = f(v).expect_err(&format!("{name} must refuse {v:?}"));
        assert_eq!(err["error_code"], code, "{name} on {v:?}: {err}");
        assert_eq!(err["success"], false);
        assert!(
            err["message"].as_str().is_some_and(|m| !m.is_empty()),
            "{err}"
        );
    }
}

#[test]
fn class_names() {
    assert_accepts(
        "class",
        validate_class_name,
        &[
            "MyApp.Patient",
            "%Library.String",
            "Ens.Director",
            "%ZEN.Component.page",
            "HS.FHIR.DTL.vR4.Model.Resource.Patient",
            "A",
            "IADTest136.IntakeRule",
        ],
    );
    assert_refuses("class", validate_class_name, "INVALID_CLASS_NAME", CRAFTED);
    assert_refuses(
        "class",
        validate_class_name,
        "INVALID_CLASS_NAME",
        &["1A.B", ".A", "A.", "A..B", "%%A", "A.%B", "A.1B"],
    );
}

#[test]
fn routine_and_document_names() {
    assert_accepts(
        "routine",
        validate_routine_name,
        &[
            "MyRoutine",
            "MyRoutine.mac",
            "%SYS.Something.int",
            "IAD.Probe.BP.1.int",
            "MyApp.Patient.cls",
            "MyApp.Macros.inc",
            "MyApp.Flow.bpl",
            "MyApp.Map.dtl",
            "%ZSTART",
        ],
    );
    assert_refuses(
        "routine",
        validate_routine_name,
        "INVALID_ROUTINE_NAME",
        CRAFTED,
    );
    assert_refuses(
        "routine",
        validate_routine_name,
        "INVALID_ROUTINE_NAME",
        &["A..mac", ".mac", "A.mac.", "A/B.mac"],
    );
}

#[test]
fn namespaces() {
    assert_accepts(
        "namespace",
        validate_namespace,
        &[
            "USER",
            "%SYS",
            "BENCHMARK",
            "HSLIB",
            "FHIR-SERVER",
            "my_ns",
            "Ns2",
        ],
    );
    assert_refuses(
        "namespace",
        validate_namespace,
        "INVALID_NAMESPACE",
        CRAFTED,
    );
    assert_refuses(
        "namespace",
        validate_namespace,
        "INVALID_NAMESPACE",
        &["1USER", "US ER", "USER'", "a.b", "^USER"],
    );
}

#[test]
fn method_names() {
    assert_accepts(
        "method",
        validate_method_name,
        &[
            "OnRequest",
            "%New",
            "%OpenId",
            "GetProductionStatus",
            "Test1",
        ],
    );
    assert_refuses(
        "method",
        validate_method_name,
        "INVALID_METHOD_NAME",
        CRAFTED,
    );
    assert_refuses(
        "method",
        validate_method_name,
        "INVALID_METHOD_NAME",
        &["A.B", "1A", "%%A"],
    );
}

#[test]
fn sql_identifiers() {
    assert_accepts(
        "sql",
        validate_sql_identifier,
        &[
            "Ens_Rule.RuleSet",
            "Sample.Person",
            "%Dictionary.CompiledClass",
            "INFORMATION_SCHEMA.TABLES",
            "Person",
            "\"My Schema\".\"My Table\"",
            "SQLUser.Bench_Q2",
        ],
    );
    assert_refuses(
        "sql",
        validate_sql_identifier,
        "INVALID_SQL_IDENTIFIER",
        CRAFTED,
    );
    assert_refuses(
        "sql",
        validate_sql_identifier,
        "INVALID_SQL_IDENTIFIER",
        &["a..c", "\"a\"\"b\"", "a b", "a.", ".a"],
    );
}

#[test]
fn timestamps() {
    assert_accepts(
        "ts",
        validate_timestamp,
        &[
            "2026-10-17",
            "2026-10-17 09:30:00",
            "2026-10-17 09:30:00.123",
        ],
    );
    assert_refuses("ts", validate_timestamp, "INVALID_TIMESTAMP", CRAFTED);
    assert_refuses(
        "ts",
        validate_timestamp,
        "INVALID_TIMESTAMP",
        &[
            "2026-10-17\" Kill ^Y",
            "yesterday",
            "2026-1-7",
            "2026-10-17 9:30",
        ],
    );
}

#[test]
fn global_names_still_validate_from_their_new_home() {
    assert!(validate_global_name("IrisDevTest.G146").is_ok());
    assert_eq!(
        validate_global_name("X) Kill ^Y").unwrap_err()["error_code"],
        "INVALID_GLOBAL_NAME"
    );
}

#[test]
fn os_str_expr_quotes_what_it_splices() {
    assert_eq!(os_str_expr(""), "\"\"");
    assert_eq!(os_str_expr("a\"b"), "\"a\"\"b\"");
    let with_newline = os_str_expr("a\nb");
    assert!(with_newline.contains("$CHAR(10)"), "{with_newline}");
    assert!(!with_newline.contains('\n'), "{with_newline}");
}

/// Evaluates an expression made only of ObjectScript string literals, `_` and `$CHAR(n,...)`.
/// Anything else (a command, a call, an operator) is a parse failure: that is the property a splice
/// needs, since the payload must stay data.
fn eval_string_expr(e: &str) -> Option<String> {
    let b: Vec<char> = e.chars().collect();
    let (mut i, mut out, mut want_term) = (0, String::new(), true);
    while i < b.len() {
        if want_term {
            if b[i] == '"' {
                i += 1;
                loop {
                    let c = *b.get(i)?;
                    if c == '"' {
                        if b.get(i + 1) == Some(&'"') {
                            out.push('"');
                            i += 2;
                            continue;
                        }
                        i += 1;
                        break;
                    }
                    out.push(c);
                    i += 1;
                }
            } else if e[i..].starts_with("$CHAR(") {
                let close = e[i..].find(')')? + i;
                for n in e[i + 6..close].split(',') {
                    out.push(char::from_u32(n.trim().parse().ok()?)?);
                }
                i = close + 1;
            } else {
                return None;
            }
            want_term = false;
        } else {
            if b[i] != '_' {
                return None;
            }
            i += 1;
            want_term = true;
        }
    }
    (!want_term).then_some(out)
}

/// `os_lit_inner` goes between the quotes a site already has: `"{}"` with it inside is an
/// expression of literals and `$CHAR` only, equal to the input, whatever the input holds.
#[test]
fn os_lit_inner_fits_inside_existing_quotes() {
    use iris_agentic_dev_core::objectscript::os_lit_inner;
    let cases = [
        "plain",
        "",
        "a\"b",
        "X\" Kill ^Y Set z=\"",
        "X) Kill ^Y Set q=$Get(^X",
        "line1\nline2",
        "\tlead",
        "trail\r",
    ];
    for s in cases {
        let expr = format!("\"{}\"", os_lit_inner(s));
        assert!(!expr.contains('\n') && !expr.contains('\r'), "{expr}");
        assert_eq!(eval_string_expr(&expr).as_deref(), Some(s), "{expr}");
    }
    assert_eq!(
        eval_string_expr("\"a\" Kill ^Y"),
        None,
        "the checker must reject code"
    );
    // Printable text keeps the hand-written look: quotes doubled, nothing else.
    assert_eq!(os_lit_inner("say \"hi\""), "say \"\"hi\"\"");
}

/// `iris_interop_query` `body_where` is a WHERE fragment spliced into a SELECT. It gets the same
/// read-only check `iris_query` applies, and no comment that could hide the rest of the query.
#[test]
fn body_where_is_held_to_the_iris_query_rules() {
    use iris_agentic_dev_core::tools::interop::check_body_where;
    for ok in [
        "r.Status = 'Open'",
        "r.Amount > 10 AND r.Region IN ('N','S')",
    ] {
        assert!(check_body_where(ok).is_ok(), "{ok}");
    }
    for bad in [
        "1=1; DELETE FROM Ens.MessageHeader",
        "1=1) OR (1=1 --",
        "1=1 /* hide */",
        "ID IN (SELECT ID FROM Foo) OR 1=1 UNION DELETE FROM X",
        "1=1 OR ID=(SELECT 1 FROM X WHERE 1=(UPDATE X SET A=1))",
    ] {
        let e = check_body_where(bad).expect_err(bad);
        assert_eq!(e["error_code"], "INVALID_PARAMS", "{bad}: {e}");
    }
}

/// B4: the literal-Kill check looked only at the first argument, so `Kill x,^Y` went through.
#[test]
fn literal_kill_check_reads_every_argument() {
    use iris_agentic_dev_core::tools::write_gate::contains_global_kill;
    for code in [
        "Kill x,^Y",
        "Kill x, ^Y",
        "kill a,b,^Y(1)",
        "k x,^Y",
        "Kill:1 x,^Y",
        "Set a=1 Kill x,^Y Write 1",
        "Kill ^Y",
    ] {
        assert!(contains_global_kill(code), "{code}");
    }
    for code in [
        "Kill x,y",
        "Kill (x),y",
        "Kill x Write \",^Y\"",
        "Do check^MyRtn",
        "Set x=$$lock^MyLocks",
    ] {
        assert!(!contains_global_kill(code), "{code}");
    }
}

/// B4: an argument's quotes and parentheses do not end it, and a global inside one is a subscript
/// or a string, not a global being killed.
#[test]
fn literal_kill_check_reads_past_strings_and_subscripts() {
    use iris_agentic_dev_core::tools::write_gate::contains_global_kill;
    for code in [
        "Kill a(\"x,y\"),^Y",
        "Kill a(1,2),^Y",
        "Kill a(b(1),\"p q\"),^Y",
        "Kill x,\t^Y",
    ] {
        assert!(contains_global_kill(code), "{code}");
    }
    for code in [
        "Kill a(\"^Y\")",
        "Kill a(^Y)",
        "Kill a(1,^Y)",
        "Kill x,y Set ^Z=1",
        "Kill a(\"x y\") Write ^Y",
    ] {
        assert!(!contains_global_kill(code), "{code}");
    }
}

/// B1: the ssh route built a remote shell command from the container and namespace with no
/// quoting, outside every IRIS gate. A host starting with `-` would be read by ssh as an option.
#[test]
fn ssh_remote_command_is_built_from_checked_parts() {
    use iris_agentic_dev_core::iris::connection::build_ssh_remote_command;
    assert_eq!(
        build_ssh_remote_command("iris-dev-iris", "USER", "build-host").unwrap(),
        "docker exec -i 'iris-dev-iris' iris session IRIS -U 'USER'"
    );
    assert!(build_ssh_remote_command("iris_1.a", "%SYS", "user@host.example").is_ok());
    for (c, ns, host) in [
        ("x; touch /tmp/iad137", "USER", "h"),
        ("x$(id)", "USER", "h"),
        ("iris", "USER; id", "h"),
        ("iris", "USER", "-oProxyCommand=id"),
        ("-x", "USER", "h"),
        ("", "USER", "h"),
    ] {
        assert!(
            build_ssh_remote_command(c, ns, host).is_err(),
            "{c:?} {ns:?} {host:?}"
        );
    }
}

/// B2: iris_test put its pattern into RunTest's literal with a backslash escape, and into
/// `##class(<pattern>).Run()` bare.
#[test]
fn test_run_code_keeps_the_pattern_data() {
    use iris_agentic_dev_core::tools::build_test_run_code;
    let crafted = "X\") Kill ^IrisDevTest.S137 Set z=(\"";
    for test_production in [false, true] {
        let code = build_test_run_code(crafted, "/verbose=1", "tok", test_production);
        assert!(!code.contains("##class(X\")"), "{code}");
        assert!(code.contains("X\"\")"), "the quote must be doubled: {code}");
        assert!(!code.contains("\\\""), "no backslash escape: {code}");
    }
    let dir = build_test_run_code("Pkg/Dir\" Kill ^Y Set z=\"", "/v", "t", false);
    assert!(!dir.contains("Dir\" Kill"), "{dir}");
    let ok = build_test_run_code("MyApp.Tests.Prod", "/v", "t", true);
    assert!(ok.contains("##class(MyApp.Tests.Prod).Run()"), "{ok}");
}

const CRAFTED_QUOTE: &str = "X\" Kill ^IrisDevTest.S137 Set z=\"";

/// iris_coverage put `test_path` into its RunTest literal unescaped; the 137 sweep checked its
/// `classes` and `package` but not this (spec 140 triage, TEST_DIAG-21). A newline would also have
/// started a new terminal line on the docker path.
#[test]
fn coverage_run_code_keeps_the_test_path_data() {
    use iris_agentic_dev_core::tools::coverage::build_coverage_run_code;
    let routines = ["Demo.A.1".to_string()];
    let code = build_coverage_run_code(&routines, CRAFTED_QUOTE, "USER");
    assert!(!code.contains("X\" Kill"), "{code}");
    assert!(
        code.contains("X\"\" Kill"),
        "the quote must be doubled: {code}"
    );
    let code = build_coverage_run_code(&routines, "X\nKill ^IrisDevTest.S137", "USER");
    assert!(
        !code
            .lines()
            .any(|l| l.trim_start().starts_with("Kill ^IrisDevTest")),
        "a newline must not start a statement: {code}"
    );
    let ok = build_coverage_run_code(&routines, "MyApp.Tests", "USER");
    assert!(
        ok.contains("RunTest(\"MyApp.Tests\",\"/noload/nodelete\")"),
        "{ok}"
    );
}

/// B2/B3: write- and destructive-tier tools whose splices reach past their own operation.
#[test]
fn write_tier_builders_keep_parameters_data() {
    use iris_agentic_dev_core::tools::admin_tools::{
        build_mirror_add_async_code, build_namespace_create_code,
    };
    use iris_agentic_dev_core::tools::interop::build_production_start_code;
    let codes = [
        build_production_start_code(CRAFTED_QUOTE),
        build_mirror_add_async_code(CRAFTED_QUOTE, CRAFTED_QUOTE, 1972, CRAFTED_QUOTE, 2),
        build_namespace_create_code("NS1", CRAFTED_QUOTE),
    ];
    for code in codes {
        assert!(!code.contains("X\" Kill"), "{code}");
        assert!(
            code.contains("X\"\" Kill"),
            "the quote must be doubled: {code}"
        );
    }
    assert!(build_production_start_code("Demo.Production").contains("\"Demo.Production\""));
}

/// B2: iris_execute_method spliced class and method bare into `##class(<class>).<method>(...)`
/// behind a three-character denylist, and its arguments with doubled quotes but raw newlines.
#[tokio::test]
async fn execute_method_checks_names_before_any_call() {
    use iris_agentic_dev_core::iris::connection::{DiscoverySource, IrisConnection};
    use iris_agentic_dev_core::tools::doc::handle_iris_execute_method;
    use iris_agentic_dev_core::tools::IrisExecuteMethodParams;
    // Port 1 answers nothing: a refusal must come before any request is made.
    let iris = IrisConnection::new(
        "http://127.0.0.1:1".to_string(),
        "USER",
        "u".to_string(),
        "p".to_string(),
        DiscoverySource::EnvVar,
    );
    let client = reqwest::Client::new();
    let run = |class: &str, method: &str| IrisExecuteMethodParams {
        class: class.to_string(),
        method: method.to_string(),
        args: vec![],
        namespace: None,
        server: None,
    };
    for (class, method, code) in [
        (
            "%SYSTEM.OBJ).Delete(\"X\") Set z=##class(%SYSTEM.OBJ",
            "Load",
            "INVALID_CLASS_NAME",
        ),
        (
            "%SYSTEM.Version",
            "GetVersion() Kill ^Y Set z=1",
            "INVALID_METHOD_NAME",
        ),
        (
            "%SYSTEM.Version",
            "GetVersion()\nKill ^Y",
            "INVALID_METHOD_NAME",
        ),
    ] {
        let r = handle_iris_execute_method(&iris, &client, &run(class, method))
            .await
            .expect("Ok");
        let text = r.content.first().unwrap().as_text().unwrap().text.clone();
        let v: serde_json::Value = serde_json::from_str(&text).unwrap();
        assert_eq!(v["error_code"], code, "{class} {method}: {v}");
    }
}

/// UNSURE, iris_query mode=count: the table was spliced as an identifier unchecked, and the query
/// wrapped as a subquery without the read-only check the other modes apply.
#[test]
fn count_query_is_checked_like_a_read() {
    use iris_agentic_dev_core::tools::count_query_checked;
    assert_eq!(
        count_query_checked(Some("Sample.Person"), None).unwrap(),
        "SELECT COUNT(*) FROM Sample.Person"
    );
    assert!(count_query_checked(None, Some("SELECT ID FROM Sample.Person")).is_ok());
    for (t, q) in [
        (Some("Sample.Person WHERE 1=(SELECT 1)"), None),
        (Some("X; DROP TABLE Y"), None),
        (None, Some("SELECT 1) t; DELETE FROM Sample.Person --")),
        (None, Some("DELETE FROM Sample.Person")),
    ] {
        let e = count_query_checked(t, q).expect_err(&format!("{t:?} {q:?}"));
        assert!(
            [
                "INVALID_SQL_IDENTIFIER",
                "SQL_WRITE_BLOCKED",
                "INVALID_PARAMS"
            ]
            .contains(&e["error_code"].as_str().unwrap_or("")),
            "{e}"
        );
    }
}

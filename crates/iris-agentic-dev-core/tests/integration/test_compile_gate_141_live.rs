//! Spec 141 US3: the compile-time gate against live IRIS.
//!
//! Each construct below is first put and compiled straight over Atelier, with no iad in the way, to
//! learn whether this build runs code at compile time for it. The probe class
//! `IadGate.Probe.Mark(tag)` sets `^IadGateProbe(tag)` to `$USERNAME`, so a set sentinel is the proof
//! that the construct ran. The iad cases then send the same source through `iris_doc put`
//! (with and without `compile`) and `iris_compile` of a local file, and assert a refusal with
//! `isError` and a sentinel that stays unset.
//!
//! Requires `iris-dev-iris`: `IRIS_HOST=localhost IRIS_WEB_PORT=52780 IRIS_USERNAME=_SYSTEM
//! IRIS_PASSWORD=SYS IRIS_NAMESPACE=USER`. Run serial.

use iris_agentic_dev_core::iris::connection::{
    is_generator_error, CompileResult, DiscoverySource, IrisConnection,
};
use iris_agentic_dev_core::testing::{live_env, require_iad_binary, McpSession};

const NS: &str = "USER";

struct Harness {
    conn: IrisConnection,
    client: reqwest::Client,
}

impl Harness {
    fn new() -> Self {
        let host = std::env::var("IRIS_HOST").unwrap_or_default();
        assert!(
            !host.is_empty(),
            "IRIS_HOST unset. These tests measure what live IRIS runs at compile time; without \
             IRIS they assert nothing. Set IRIS_HOST=localhost IRIS_WEB_PORT=52780 \
             IRIS_USERNAME=_SYSTEM IRIS_PASSWORD=SYS"
        );
        let port = std::env::var("IRIS_WEB_PORT").unwrap_or_else(|_| "52780".into());
        let user = std::env::var("IRIS_USERNAME").unwrap_or_else(|_| "_SYSTEM".into());
        let pass = std::env::var("IRIS_PASSWORD").unwrap_or_else(|_| "SYS".into());
        Self {
            conn: IrisConnection::new(
                format!("http://{host}:{port}"),
                NS,
                user,
                pass,
                DiscoverySource::EnvVar,
            ),
            client: reqwest::Client::new(),
        }
    }

    /// Atelier PUT, no iad gate in the way. Returns the HTTP status and body.
    async fn put(&self, doc: &str, src: &str) -> (u16, String) {
        let url = self.conn.versioned_ns_url(
            NS,
            &format!("/doc/{}?ignoreConflict=1", urlencoding::encode(doc)),
        );
        let lines: Vec<&str> = src.lines().collect();
        let resp = self
            .client
            .put(&url)
            .basic_auth(&self.conn.username, Some(&self.conn.password))
            .json(&serde_json::json!({"enc": false, "content": lines}))
            .send()
            .await
            .expect("PUT must reach IRIS");
        let status = resp.status().as_u16();
        (status, resp.text().await.unwrap_or_default())
    }

    async fn compile(&self, doc: &str) -> CompileResult {
        self.conn
            .compile_document(doc, NS, "cuk", &self.client)
            .await
            .expect("compile request must reach IRIS")
    }

    async fn run(&self, code: &str) -> String {
        let out = self
            .conn
            .execute_via_generator(code, NS, &self.client)
            .await
            .expect("execute request must reach IRIS");
        assert!(!is_generator_error(&out), "IRIS run failed: {out}");
        out.trim().to_string()
    }

    /// The sentinel for `tag`: `None` when unset, the `$USERNAME` that set it otherwise.
    async fn marker(&self, tag: &str) -> Option<String> {
        let out = self
            .run(&format!(
                " Write \"~[\",$Select($Data(^IadGateProbe(\"{tag}\")):\"set:\"_^IadGateProbe(\"{tag}\"),1:\"unset\"),\"]~\""
            ))
            .await;
        let start = out
            .rfind("~[")
            .unwrap_or_else(|| panic!("no marker in {out}"))
            + 2;
        let end = out[start..]
            .find("]~")
            .unwrap_or_else(|| panic!("no end in {out}"));
        let v = &out[start..start + end];
        v.strip_prefix("set:").map(str::to_string)
    }

    async fn exists(&self, doc: &str) -> bool {
        let url = self
            .conn
            .versioned_ns_url(NS, &format!("/doc/{}", urlencoding::encode(doc)));
        let resp = self
            .client
            .get(&url)
            .basic_auth(&self.conn.username, Some(&self.conn.password))
            .send()
            .await
            .expect("GET must reach IRIS");
        resp.status().is_success()
    }

    /// Kill the sentinels and every `IadGate*` document. Run before and after each test, so a run
    /// that died half way does not decide the next one.
    async fn clean(&self) {
        self.run(
            " Kill ^IadGateProbe\n \
             Do $system.OBJ.DeletePackage(\"IadGate\",\"-d\")\n \
             For r=\"IadGateR.mac\",\"IadGateM.mac\",\"IadGateI.inc\",\"IadGateJ.inc\",\"IadGateK.inc\",\"IadGateE.mac\",\"IadGateZ.mac\",\"IadZsProbe.mac\",\"IadZsProbe.int\" { If ##class(%Routine).Exists(r) Do ##class(%Routine).Delete(r) }",
        )
        .await;
        // The DDL case leaves a procedure class behind in the `IadGate` package, deleted above.
        // Rows written by hand into the routine-source globals are not owned by %Routine.
        self.run(
            " Kill ^rMAC(\"IadGateM\"),^rMAC(\"IadGate\"),^rINC(\"IadGateI\"),^rINC(\"IadGateJ\"),^rINC(\"IadGateK\"),^ROUTINE(\"IadZsProbe\"),^rOBJ(\"IadZsProbe\")",
        )
        .await;
    }

    /// Put and compile the helper classes every case relies on.
    async fn install_probe(&self) {
        for (doc, src) in [("IadGate.Probe.cls", PROBE), ("IadGate.Proj.cls", PROJ)] {
            let (status, body) = self.put(doc, src).await;
            assert!((200..300).contains(&status), "PUT {doc}: {status} {body}");
            let r = self.compile(doc).await;
            assert!(r.success(), "{doc} must compile: {:?}", r.errors);
        }
    }
}

const PROBE: &str = r#"Class IadGate.Probe
{

ClassMethod Mark(pTag As %String) As %String
{
    Set ^IadGateProbe(pTag) = $USERNAME
    Quit 1
}

ClassMethod MarkSql(pTag As %String) As %String [ SqlProc ]
{
    Set ^IadGateProbe(pTag) = $USERNAME
    Quit 1
}

}
"#;

const PROJ: &str = r#"Class IadGate.Proj Extends %Projection.AbstractProjection
{

ClassMethod CreateProjection(pClass As %String, ByRef pParams, pModified As %String, pQStruct) As %Status
{
    Do ##class(IadGate.Probe).Mark("proj")
    Quit $$$OK
}

}
"#;

/// One construct. `setup` documents are put and compiled directly first (they are clean). The
/// subject `doc` is what the baseline and the iad cases both send; `iad` replaces it for the iad
/// cases when the dangerous document is not the one that carries the sentinel (an include file).
struct Case {
    tag: &'static str,
    doc: &'static str,
    src: &'static str,
    setup: &'static [(&'static str, &'static str)],
    /// Measured on 2026.2: does this construct set its sentinel at compile time?
    runs: bool,
    iad: Option<(&'static str, &'static str)>,
}

// Raw strings, not `\n\` continuations: a backslash-newline eats the next line's indentation, which
// pushes `Quit` into column 1, where ObjectScript reads it as a label.

const EXEC: &str = r#"Class IadGate.CExec
{

ClassMethod M() As %Status
{
    #execute Do ##class(IadGate.Probe).Mark("exec")
    Quit $$$OK
}

}
"#;

const EXPRESSION_DIRECTIVE: &str = r#"Class IadGate.CExpr
{

ClassMethod M() As %Status
{
    Set x = ##expression(##class(IadGate.Probe).Mark("expr"))
    Quit $$$OK
}

}
"#;

const FUNCTION_DIRECTIVE: &str = r#"Class IadGate.CFunc
{

ClassMethod M() As %Status
{
    Set x = ##function(##class(IadGate.Probe).Mark("func"))
    Quit $$$OK
}

}
"#;

const IF_DIRECTIVE: &str = r#"Class IadGate.CIf
{

ClassMethod M() As %Status
{
    #if ##class(IadGate.Probe).Mark("if")
    Set x = 1
    #else
    Set x = 2
    #endif
    Quit $$$OK
}

}
"#;

const SQL_DIRECTIVE: &str = r#"Class IadGate.CSql
{

ClassMethod M() As %Status
{
    ##sql(SELECT IadGate.Probe_MarkSql('sql') INTO :x)
    Quit $$$OK
}

}
"#;

const BRACED_PARAM: &str = r#"Class IadGate.CParam
{

Parameter P = {##class(IadGate.Probe).Mark("param")};

}
"#;

const PROJECTION: &str = r#"Class IadGate.CProj
{

Projection P As IadGate.Proj;

}
"#;

const QUOTED_GENERATOR: &str = r#"Class IadGate.CQGen
{

ClassMethod G() As %Status [ CodeMode = "objectgenerator" ]
{
    Do ##class(IadGate.Probe).Mark("qgen")
    Do %code.WriteLine(" Quit 1")
    Quit $$$OK
}

}
"#;

const OBJECT_GENERATOR: &str = r#"Class IadGate.CGen
{

ClassMethod G() As %Status [ CodeMode = objectgenerator ]
{
    Do ##class(IadGate.Probe).Mark("ogen")
    Do %code.WriteLine(" Quit 1")
    Quit $$$OK
}

}
"#;

const LEGACY_GENERATOR: &str = r#"Class IadGate.CLegacy
{

ClassMethod G() As %Status [ CodeMode = generator ]
{
    Do ##class(IadGate.Probe).Mark("gen")
    $$$GENERATE(" Quit 1")
    Quit $$$OK
}

}
"#;

const XML_EXPORT: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<Export generator="IRIS" version="26">
<Class name="IadGate.CXml">
<TimeCreated>66000,1</TimeCreated>
<Method name="G">
<ClassMethod>1</ClassMethod>
<CodeMode>objectgenerator</CodeMode>
<ReturnType>%Status</ReturnType>
<Implementation><![CDATA[
    Do ##class(IadGate.Probe).Mark("xml")
    Do %code.WriteLine(" Quit 1")
    Quit $$$OK
]]></Implementation>
</Method>
</Class>
</Export>
"#;

/// `expression` and `call` run nothing at compile time: the sentinel is set by the first call only.
const EXPRESSION_MODE: &str = r#"Class IadGate.CExprMode
{

ClassMethod E() As %String [ CodeMode = expression ]
{
##class(IadGate.Probe).Mark("exprmode")
}

}
"#;

const CALL_MODE: &str = r#"Class IadGate.CCallMode
{

ClassMethod C() As %String [ CodeMode = call ]
{
Entry^IadGateNoSuchRoutine()
}

}
"#;

const MAC_EXEC: &str = "ROUTINE IadGateR\nIadGateR ;\n    #execute Do ##class(IadGate.Probe).Mark(\"mac\")\n    Quit\n";

const MAC_EXPRESSION: &str = "ROUTINE IadGateE\nIadGateE ;\n    Set x = ##expression(##class(IadGate.Probe).Mark(\"macexpr\"))\n    Quit\n";

const INC_EXEC: &str =
    "ROUTINE IadGateI [Type=INC]\n#execute Do ##class(IadGate.Probe).Mark(\"inc\")\n";

const INC_USER: &str = r#"Include IadGateI

Class IadGate.CInc
{

ClassMethod M() As %Status
{
    Quit $$$OK
}

}
"#;

/// Every construct the baseline measures, in the order of tasks.md T075. The tag is the sentinel
/// the construct sets. `runs` is what the baseline measured on `iris-dev-iris` (2026.2).
const CASES: &[Case] = &[
    Case {
        tag: "exec",
        doc: "IadGate.CExec.cls",
        src: EXEC,
        setup: &[],
        runs: true,
        iad: None,
    },
    Case {
        tag: "expr",
        doc: "IadGate.CExpr.cls",
        src: EXPRESSION_DIRECTIVE,
        setup: &[],
        runs: true,
        iad: None,
    },
    Case {
        tag: "func",
        doc: "IadGate.CFunc.cls",
        src: FUNCTION_DIRECTIVE,
        setup: &[],
        runs: true,
        iad: None,
    },
    Case {
        tag: "if",
        doc: "IadGate.CIf.cls",
        src: IF_DIRECTIVE,
        setup: &[],
        runs: true,
        iad: None,
    },
    // `##sql(...)` is embedded SQL run at call time. It is measured, and not gated.
    Case {
        tag: "sql",
        doc: "IadGate.CSql.cls",
        src: SQL_DIRECTIVE,
        setup: &[],
        runs: false,
        iad: None,
    },
    Case {
        tag: "param",
        doc: "IadGate.CParam.cls",
        src: BRACED_PARAM,
        setup: &[],
        runs: true,
        iad: None,
    },
    Case {
        tag: "proj",
        doc: "IadGate.CProj.cls",
        src: PROJECTION,
        setup: &[],
        runs: true,
        iad: None,
    },
    // IRIS refuses to parse a quoted CodeMode value, so nothing runs. The gate refuses it anyway.
    Case {
        tag: "qgen",
        doc: "IadGate.CQGen.cls",
        src: QUOTED_GENERATOR,
        setup: &[],
        runs: false,
        iad: None,
    },
    Case {
        tag: "ogen",
        doc: "IadGate.CGen.cls",
        src: OBJECT_GENERATOR,
        setup: &[],
        runs: true,
        iad: None,
    },
    Case {
        tag: "gen",
        doc: "IadGate.CLegacy.cls",
        src: LEGACY_GENERATOR,
        setup: &[],
        runs: true,
        iad: None,
    },
    Case {
        tag: "xml",
        doc: "IadGate.CXml.cls",
        src: XML_EXPORT,
        setup: &[],
        runs: true,
        iad: None,
    },
    Case {
        tag: "mac",
        doc: "IadGateR.mac",
        src: MAC_EXEC,
        setup: &[],
        runs: true,
        iad: None,
    },
    Case {
        tag: "macexpr",
        doc: "IadGateE.mac",
        src: MAC_EXPRESSION,
        setup: &[],
        runs: true,
        iad: None,
    },
    // The class is clean; the include it names carries the directive.
    Case {
        tag: "inc",
        doc: "IadGate.CInc.cls",
        src: INC_USER,
        setup: &[("IadGateI.inc", INC_EXEC)],
        runs: true,
        iad: Some(("IadGateI.inc", INC_EXEC)),
    },
];

/// Put `case` straight over Atelier, compile it, and report the sentinel and the compile outcome.
/// Asserts the sentinel agrees with `case.runs`, so a build that changes the answer fails here
/// instead of leaving the iad cases testing the wrong set.
async fn baseline(h: &Harness, case: &Case) -> String {
    for (doc, src) in case.setup {
        let (status, body) = h.put(doc, src).await;
        assert!(
            (200..300).contains(&status),
            "setup PUT {doc}: {status} {body}"
        );
    }
    let (status, body) = h.put(case.doc, case.src).await;
    let compiled = if (200..300).contains(&status) {
        let r = h.compile(case.doc).await;
        format!("compile_ok={} errors={:?}", r.success(), r.errors)
    } else {
        format!(
            "put_failed={status} {}",
            body.chars().take(200).collect::<String>()
        )
    };
    let marker = h.marker(case.tag).await;
    assert_eq!(
        marker.is_some(),
        case.runs,
        "{}: sentinel {marker:?} but the table says runs={} — {compiled}",
        case.tag,
        case.runs
    );
    format!(
        "{:<8} {:<22} marker={:<12} {}",
        case.tag,
        case.doc,
        marker.as_deref().unwrap_or("unset"),
        compiled
    )
}

/// T075. Measure, per construct, whether this build runs the code at compile time. The table is
/// printed so it can be copied into research.md; the assertions pin it, so a build that changes
/// the answer fails here rather than leaving the iad cases testing the wrong set.
#[tokio::test]
#[ignore]
async fn baseline_direct_atelier_records_what_runs_at_compile_time() {
    let h = Harness::new();
    h.clean().await;
    h.install_probe().await;

    let mut table = Vec::new();
    for case in CASES {
        table.push(baseline(&h, case).await);
        h.clean().await;
        h.install_probe().await;
    }
    h.clean().await;
    assert_eq!(table.len(), 14, "every construct must be measured");
    eprintln!("R3 baseline\n{}", table.join("\n"));
}

/// T075, the accepted modes and the DDL case. `expression` and `call` set no sentinel until the
/// method is called; `CREATE PROCEDURE` with `##expression` in its body is measured for the record
/// (it reaches IRIS through `iris_query`, which is not this story's gate).
#[tokio::test]
#[ignore]
async fn baseline_direct_atelier_expression_call_and_ddl() {
    let h = Harness::new();
    h.clean().await;
    h.install_probe().await;

    let (status, body) = h.put("IadGate.CExprMode.cls", EXPRESSION_MODE).await;
    assert!((200..300).contains(&status), "PUT: {status} {body}");
    let r = h.compile("IadGate.CExprMode.cls").await;
    let expr_ok = r.success();
    let expr_after_compile = h.marker("exprmode").await;
    h.run(" Write ##class(IadGate.CExprMode).E()").await;
    let expr_after_call = h.marker("exprmode").await;

    let (status, body) = h.put("IadGate.CCallMode.cls", CALL_MODE).await;
    assert!((200..300).contains(&status), "PUT: {status} {body}");
    let call = h.compile("IadGate.CCallMode.cls").await;

    let ddl = r#"CREATE PROCEDURE IadGate.Pr() LANGUAGE OBJECTSCRIPT { Set x = ##expression(##class(IadGate.Probe).Mark("ddl")) Quit }"#;
    let ddl_out = h
        .run(&format!(
            " Set rs=##class(%SQL.Statement).%ExecDirect(,\"{}\") Write \"sqlcode=\",rs.%SQLCODE,\" \",rs.%Message",
            ddl.replace('"', "\"\"")
        ))
        .await;
    let ddl_marker = h.marker("ddl").await;

    assert!(expr_ok, "CodeMode = expression must compile");
    assert_eq!(
        expr_after_compile, None,
        "expression must run nothing at compile time"
    );
    assert_eq!(
        expr_after_call.as_deref(),
        Some("_SYSTEM"),
        "expression runs at call time"
    );
    assert!(
        call.success(),
        "CodeMode = call must compile: {:?}",
        call.errors
    );
    assert!(
        ddl_marker.is_some(),
        "CREATE PROCEDURE ran ##expression: {ddl_out}"
    );
    eprintln!(
        "R3 baseline (accepted modes, DDL)\n\
         expression: compile_ok={expr_ok} after_compile={expr_after_compile:?} after_call={expr_after_call:?}\n\
         call:       compile_ok={} errors={:?}\n\
         ddl:        marker={ddl_marker:?} out={ddl_out}",
        call.success(),
        call.errors
    );
    h.clean().await;
}

// ── The iad cases ────────────────────────────────────────────────────────────────────────────

/// `iris_doc` put is write-classified and delete is destructive; both tiers open, or the write gate
/// answers before the compile-time scan runs and the tests pass for the wrong reason.
fn open_gates() -> Vec<(String, String)> {
    let mut env = live_env();
    env.push(("IRIS_WRITE_TOOLS_ENABLED".to_string(), "1".to_string()));
    env.push((
        "IRIS_DESTRUCTIVE_TOOLS_ENABLED".to_string(),
        "1".to_string(),
    ));
    env
}

fn is_error(answer: &serde_json::Value) -> bool {
    answer
        .pointer("/result/isError")
        .and_then(serde_json::Value::as_bool)
        == Some(true)
}

/// The tool's own JSON: `structuredContent` when present, else the first text block parsed.
fn body(answer: &serde_json::Value) -> serde_json::Value {
    let result = answer
        .get("result")
        .unwrap_or_else(|| panic!("no result in {answer}"));
    if let Some(sc) = result.get("structuredContent") {
        return sc.clone();
    }
    let text = result
        .pointer("/content/0/text")
        .and_then(serde_json::Value::as_str)
        .unwrap_or_else(|| panic!("no content in {answer}"));
    serde_json::from_str(text).unwrap_or_else(|e| panic!("content was not JSON ({e}): {text}"))
}

/// Why `answer` is not a refusal by the compile-time gate, or `None` when it is one: an error
/// result (FR-017) carrying `COMPILE_TIME_EXEC_BLOCKED`.
fn not_a_compile_gate_refusal(answer: &serde_json::Value) -> Option<String> {
    if !is_error(answer) {
        return Some(format!("not an isError result: {}", short(answer)));
    }
    let code = body(answer)["error_code"]
        .as_str()
        .unwrap_or("")
        .to_string();
    if code != "COMPILE_TIME_EXEC_BLOCKED" {
        return Some(format!("error_code {code:?}: {}", short(answer)));
    }
    None
}

fn short(v: &serde_json::Value) -> String {
    v.to_string().chars().take(240).collect()
}

fn iris_doc_put(mcp: &mut McpSession, doc: &str, src: &str, compile: bool) -> serde_json::Value {
    mcp.call(
        "iris_doc",
        &serde_json::json!({"mode": "put", "name": doc, "content": src, "compile": compile}),
    )
}

/// The cases the gate must refuse: everything that ran, and the quoted generator that IRIS itself
/// rejects (refused anyway; the stricter reading).
fn refused_cases() -> Vec<&'static Case> {
    let list: Vec<&Case> = CASES.iter().filter(|c| c.runs || c.tag == "qgen").collect();
    assert_eq!(
        list.len(),
        13,
        "the refusal list drifted from the baseline table"
    );
    list
}

/// What a case sends through iad: the case's own document, or the override.
fn iad_subject(case: &Case) -> (&'static str, &'static str) {
    case.iad.unwrap_or((case.doc, case.src))
}

/// T076, `iris_doc put`. FR-012, FR-017, SC-005. Each construct that ran in the baseline is refused
/// with `isError`, with and without `compile`, the document is not stored and the sentinel stays
/// unset. Red until the gate knows the construct: today's gate sees `CodeMode` only, and a miss
/// here is a document stored and compiled as the primary identity.
#[test]
#[ignore]
fn constructs_that_run_at_compile_time_are_refused_by_iris_doc_put() {
    let Some(_bin) = require_iad_binary() else {
        return;
    };
    let rt = tokio::runtime::Runtime::new().unwrap();
    let h = Harness::new();
    rt.block_on(async {
        h.clean().await;
        h.install_probe().await;
    });
    let mut mcp = McpSession::start(&open_gates());
    let mut failures = Vec::new();
    for case in refused_cases() {
        let (doc, src) = iad_subject(case);
        for compile in [false, true] {
            let answer = iris_doc_put(&mut mcp, doc, src, compile);
            if let Some(why) = not_a_compile_gate_refusal(&answer) {
                failures.push(format!("{} (compile={compile}): {why}", case.tag));
            }
            let (stored, marker) =
                rt.block_on(async { (h.exists(doc).await, h.marker(case.tag).await) });
            if stored {
                failures.push(format!(
                    "{} (compile={compile}): the document was stored",
                    case.tag
                ));
            }
            if marker.is_some() {
                failures.push(format!(
                    "{} (compile={compile}): sentinel {marker:?} is set",
                    case.tag
                ));
            }
            rt.block_on(async {
                h.clean().await;
                h.install_probe().await;
            });
        }
    }
    rt.block_on(h.clean());
    assert!(
        failures.is_empty(),
        "compile-time constructs got through:\n{}",
        failures.join("\n")
    );
}

/// T076, `iris_compile` of a local file: the second route into the same compile.
#[test]
#[ignore]
fn constructs_that_run_at_compile_time_are_refused_by_iris_compile_of_a_local_file() {
    let Some(_bin) = require_iad_binary() else {
        return;
    };
    let rt = tokio::runtime::Runtime::new().unwrap();
    let h = Harness::new();
    rt.block_on(async {
        h.clean().await;
        h.install_probe().await;
    });
    let dir = std::env::temp_dir().join(format!("iadgate141-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    // The files are compiled from the project directory, which is the server's working one.
    let mut mcp = McpSession::start_in(Some(&dir), &open_gates());
    let mut failures = Vec::new();
    for case in refused_cases() {
        let (doc, src) = iad_subject(case);
        let path = dir.join(doc);
        std::fs::write(&path, src).unwrap();
        let answer = mcp.call(
            "iris_compile",
            &serde_json::json!({"target": path.to_str().unwrap()}),
        );
        if let Some(why) = not_a_compile_gate_refusal(&answer) {
            failures.push(format!("{}: {why}", case.tag));
        }
        let (stored, marker) =
            rt.block_on(async { (h.exists(doc).await, h.marker(case.tag).await) });
        if stored {
            failures.push(format!("{}: the document was uploaded", case.tag));
        }
        if marker.is_some() {
            failures.push(format!("{}: sentinel {marker:?} is set", case.tag));
        }
        rt.block_on(async {
            h.clean().await;
            h.install_probe().await;
        });
    }
    let _ = std::fs::remove_dir_all(&dir);
    rt.block_on(h.clean());
    assert!(
        failures.is_empty(),
        "compile-time constructs got through:\n{}",
        failures.join("\n")
    );
}

/// T078, FR-014: class XML under a `.cls` name is checked like UDL.
#[test]
#[ignore]
fn class_xml_with_an_object_generator_under_a_cls_name_is_refused() {
    let Some(_bin) = require_iad_binary() else {
        return;
    };
    let rt = tokio::runtime::Runtime::new().unwrap();
    let h = Harness::new();
    rt.block_on(async {
        h.clean().await;
        h.install_probe().await;
    });
    let mut mcp = McpSession::start(&open_gates());
    let answer = iris_doc_put(&mut mcp, "IadGate.CXml.cls", XML_EXPORT, true);
    let verdict = not_a_compile_gate_refusal(&answer);
    let stored = rt.block_on(h.exists("IadGate.CXml.cls"));
    let marker = rt.block_on(h.marker("xml"));
    rt.block_on(h.clean());
    assert_eq!(verdict, None, "XML export must be refused");
    assert!(!stored, "the XML class must not be stored");
    assert_eq!(marker, None, "the XML generator must not have run");
}

/// A routine written without Atelier: source in `^rMAC` and nothing else, as `X.mac` (G1, H5).
const ROUTINE_ONLY: &str = " Set r=##class(%Routine).%New(\"IadGateM.mac\")\n \
     Do r.WriteLine(\"IadGateM ;\")\n \
     Do r.WriteLine(\" #execute Do ##class(IadGate.Probe).Mark(\"\"rmac\"\")\")\n \
     Do r.WriteLine(\" Quit\")\n \
     Do r.Save()";

/// T077, FR-015: a document already on the server is checked before `iris_compile` compiles it by
/// name. Each is stored straight over Atelier (or `%Routine`), compiled through iad, and refused with
/// the sentinel unset. The control at the end compiles the same document directly and shows the
/// sentinel does get set, so the unset sentinel is the refusal's doing and not a dead document.
#[test]
#[ignore]
fn iris_compile_by_name_checks_the_stored_source_first() {
    let Some(_bin) = require_iad_binary() else {
        return;
    };
    let rt = tokio::runtime::Runtime::new().unwrap();
    let h = Harness::new();
    let mut mcp = McpSession::start(&open_gates());
    let mut failures = Vec::new();
    // (sentinel, document, how it is stored)
    for (tag, doc, via_routine) in [
        ("ogen", "IadGate.CGen.cls", false),
        ("exec", "IadGate.CExec.cls", false),
        ("mac", "IadGateR.mac", false),
        ("rmac", "IadGateM.mac", true),
    ] {
        rt.block_on(async {
            h.clean().await;
            h.install_probe().await;
            if via_routine {
                h.run(ROUTINE_ONLY).await;
            } else {
                let case = CASES.iter().find(|c| c.tag == tag).unwrap();
                let (status, body) = h.put(doc, case.src).await;
                assert!((200..300).contains(&status), "PUT {doc}: {status} {body}");
            }
            assert!(
                h.exists(doc).await,
                "{doc} must be stored before iad compiles it"
            );
        });
        let answer = mcp.call("iris_compile", &serde_json::json!({"target": doc}));
        if let Some(why) = not_a_compile_gate_refusal(&answer) {
            failures.push(format!("{tag}: {why}"));
        }
        let after_iad = rt.block_on(h.marker(tag));
        if after_iad.is_some() {
            failures.push(format!(
                "{tag}: sentinel {after_iad:?} set by iad's compile"
            ));
        }
        let control = rt.block_on(async {
            h.compile(doc).await;
            h.marker(tag).await
        });
        if control.is_none() {
            failures.push(format!(
                "{tag}: control compile set no sentinel — the document was dead"
            ));
        }
    }
    rt.block_on(h.clean());
    assert!(
        failures.is_empty(),
        "stored documents compiled unchecked:\n{}",
        failures.join("\n")
    );
}

/// T079, FR-016: `CodeMode = expression` and `call` compile through iad, run nothing at compile
/// time, and `expression` runs at call time. The trusted-file switch half is Foundation's.
#[test]
#[ignore]
fn expression_and_call_modes_are_accepted_and_compile() {
    let Some(_bin) = require_iad_binary() else {
        return;
    };
    let rt = tokio::runtime::Runtime::new().unwrap();
    let h = Harness::new();
    rt.block_on(async {
        h.clean().await;
        h.install_probe().await;
    });
    let mut mcp = McpSession::start(&open_gates());
    let mut failures = Vec::new();
    for (doc, src) in [
        ("IadGate.CExprMode.cls", EXPRESSION_MODE),
        ("IadGate.CCallMode.cls", CALL_MODE),
    ] {
        let answer = iris_doc_put(&mut mcp, doc, src, true);
        let b = body(&answer);
        if is_error(&answer) || b["success"] != serde_json::Value::Bool(true) {
            failures.push(format!(
                "{doc}: put+compile refused or failed: {}",
                short(&answer)
            ));
        }
    }
    // Fail on the refusal before calling the method: it does not exist when the put was refused.
    if !failures.is_empty() {
        rt.block_on(h.clean());
        panic!("accepted modes were refused:\n{}", failures.join("\n"));
    }
    let at_compile = rt.block_on(h.marker("exprmode"));
    let at_call = rt.block_on(async {
        h.run(" Write ##class(IadGate.CExprMode).E()").await;
        h.marker("exprmode").await
    });
    rt.block_on(h.clean());
    assert_eq!(
        at_compile, None,
        "expression must run nothing at compile time"
    );
    assert!(at_call.is_some(), "expression must run when called");
}

// ── The code-edit block (T080) ───────────────────────────────────────────────────────────────

/// A code string, and the `class`/`method` pair `iris_execute_method` sends for the same call when
/// there is one.
struct EditCase {
    label: &'static str,
    code: &'static str,
    method: Option<(&'static str, &'static str)>,
}

const BLOCKED_EDITS: &[EditCase] = &[
    EditCase {
        label: "%Routine alias",
        code: "Set r=##class(%Routine).%New(\"IadGateZ.mac\") Do r.Write(\" Quit\") Do r.Save()",
        method: Some(("%Routine", "%New")),
    },
    EditCase {
        label: "UnCompile",
        code: "Do $SYSTEM.OBJ.UnCompile(\"IadGate.NoSuch\")",
        method: Some(("%SYSTEM.OBJ", "UnCompile")),
    },
    EditCase {
        label: "ZInsert+ZSave",
        code: "ZInsert \" Quit 42\" ZSave IadZsProbe",
        method: None,
    },
    EditCase {
        label: "ZSave inside Xecute",
        code: "Xecute \"ZInsert \"\" Quit 42\"\" ZSave IadZsProbe\"",
        method: None,
    },
    EditCase {
        label: "ZLoad",
        code: "ZLoad IadZsProbe",
        method: None,
    },
    EditCase {
        label: "^rMAC",
        code: "Set ^rMAC(\"IadGate\",0)=1",
        method: None,
    },
    EditCase {
        label: "^rINC",
        code: "Set ^rINC(\"IadGateI\",0)=1",
        method: None,
    },
    EditCase {
        label: "$Text of a %apiOBJ entry point",
        code: "Write $Text(Load^%apiOBJ)",
        method: None,
    },
    EditCase {
        label: "%apiOBJ Load entry point",
        code: "Do Load^%apiOBJ(\"/nonexistent/iadgate.xml\",\"-d\")",
        method: None,
    },
];

/// Neither reads nor writes code: must go through.
const ALLOWED_CODE: &[(&str, &str)] = &[
    ("production config read", "Write $Data(^Ens.Config.ItemD)"),
    (
        "code global named in a comment",
        "Write 1 ; never touch ^oddDEF from here",
    ),
];

fn error_code_of(answer: &serde_json::Value) -> String {
    body(answer)["error_code"]
        .as_str()
        .unwrap_or("")
        .to_string()
}

fn blocked_by_code_edit(answer: &serde_json::Value) -> Option<String> {
    if !is_error(answer) || error_code_of(answer) != "CODE_EDIT_BLOCKED" {
        return Some(short(answer));
    }
    None
}

/// T080, FR-019, FR-017: the code-edit block, through all three arbitrary-execution tools.
#[test]
#[ignore]
fn code_edit_block_covers_routines_buffers_and_apiobj_through_every_tool() {
    let Some(_bin) = require_iad_binary() else {
        return;
    };
    let rt = tokio::runtime::Runtime::new().unwrap();
    let h = Harness::new();
    rt.block_on(h.clean());
    let mut mcp = McpSession::start(&open_gates());
    let opened = mcp.call("iris_ws_open", &serde_json::json!({"namespace": "USER"}));
    let token = body(&opened)["session"]
        .as_str()
        .unwrap_or_else(|| {
            panic!(
                "iris_ws_open gave no session on iris-dev-iris: {}",
                short(&opened)
            )
        })
        .to_string();

    let mut failures = Vec::new();
    for case in BLOCKED_EDITS {
        let via_execute = mcp.call("iris_execute", &serde_json::json!({"code": case.code}));
        if let Some(why) = blocked_by_code_edit(&via_execute) {
            failures.push(format!("iris_execute / {}: {why}", case.label));
        }
        let via_ws = mcp.call(
            "iris_ws_exec",
            &serde_json::json!({"session": token, "code": case.code}),
        );
        if let Some(why) = blocked_by_code_edit(&via_ws) {
            failures.push(format!("iris_ws_exec / {}: {why}", case.label));
        }
        if let Some((class, method)) = case.method {
            let via_method = mcp.call(
                "iris_execute_method",
                &serde_json::json!({"class": class, "method": method, "args": ["IadGate.NoSuch"]}),
            );
            if let Some(why) = blocked_by_code_edit(&via_method) {
                failures.push(format!("iris_execute_method / {}: {why}", case.label));
            }
        }
    }
    for (label, code) in ALLOWED_CODE {
        let via_execute = mcp.call("iris_execute", &serde_json::json!({"code": code}));
        if is_error(&via_execute) || error_code_of(&via_execute) == "CODE_EDIT_BLOCKED" {
            failures.push(format!(
                "iris_execute / allowed {label}: {}",
                short(&via_execute)
            ));
        }
        let via_ws = mcp.call(
            "iris_ws_exec",
            &serde_json::json!({"session": token, "code": code}),
        );
        if error_code_of(&via_ws) == "CODE_EDIT_BLOCKED" {
            failures.push(format!(
                "iris_ws_exec / allowed {label}: {}",
                short(&via_ws)
            ));
        }
    }
    let _ = mcp.call("iris_ws_close", &serde_json::json!({"session": token}));

    // A refused edit that ran anyway leaves these behind.
    let leftovers = rt.block_on(h.run(
        " Write \"~[\",##class(%Routine).Exists(\"IadZsProbe.int\"),##class(%Routine).Exists(\"IadGateZ.mac\"),$Data(^rMAC(\"IadGate\")),$Data(^rINC(\"IadGateI\")),\"]~\"",
    ));
    rt.block_on(h.clean());
    assert!(
        failures.is_empty(),
        "code-edit block gaps:\n{}",
        failures.join("\n")
    );
    assert!(
        leftovers.contains("~[0000]~"),
        "a refused edit ran anyway: {leftovers}"
    );
}

// ── US3 follow-ups: T165 (fail closed), T166 (includes), T167 (docker-only), T168 (DDL) ──────

/// A forwarder in front of `iris-dev-iris` that answers some requests itself, so that a condition
/// IRIS will not produce on demand (a document that exists and cannot be read) can be set up while
/// the compile that follows still reaches the real server. A `GET` whose path contains a rule's text
/// gets that rule's status with no body; everything else is passed through untouched.
struct FaultProxy {
    port: u16,
}

fn fault_proxy(rules: &[(&'static str, u16)]) -> FaultProxy {
    use std::io::{Read, Write};
    use std::net::{TcpListener, TcpStream};

    fn serve(
        mut client: TcpStream,
        upstream: &str,
        rules: &[(&'static str, u16)],
    ) -> std::io::Result<()> {
        let mut buf = Vec::new();
        let mut chunk = [0u8; 8192];
        let head_end = loop {
            let n = client.read(&mut chunk)?;
            if n == 0 {
                return Ok(());
            }
            buf.extend_from_slice(&chunk[..n]);
            if let Some(at) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
                break at + 4;
            }
        };
        let head = String::from_utf8_lossy(&buf[..head_end]).to_string();
        let mut lines = head.split("\r\n");
        let request_line = lines.next().unwrap_or("").to_string();
        let mut parts = request_line.split(' ');
        let (method, path) = (parts.next().unwrap_or(""), parts.next().unwrap_or(""));
        let mut content_length = 0usize;
        let mut forwarded = vec![request_line.clone()];
        for line in lines.filter(|l| !l.is_empty()) {
            let lower = line.to_ascii_lowercase();
            if let Some(v) = lower.strip_prefix("content-length:") {
                content_length = v.trim().parse().unwrap_or(0);
            }
            if !lower.starts_with("connection:") && !lower.starts_with("proxy-connection:") {
                forwarded.push(line.to_string());
            }
        }
        if method == "GET" {
            if let Some((_, status)) = rules.iter().find(|(text, _)| path.contains(text)) {
                let reply = format!(
                    "HTTP/1.1 {status} Injected\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                );
                return client.write_all(reply.as_bytes());
            }
        }
        while buf.len() < head_end + content_length {
            let n = client.read(&mut chunk)?;
            if n == 0 {
                break;
            }
            buf.extend_from_slice(&chunk[..n]);
        }
        let mut up = TcpStream::connect(upstream)?;
        forwarded.push("Connection: close".to_string());
        up.write_all(format!("{}\r\n\r\n", forwarded.join("\r\n")).as_bytes())?;
        up.write_all(&buf[head_end..])?;
        std::io::copy(&mut up, &mut client)?;
        Ok(())
    }

    let host = std::env::var("IRIS_HOST").unwrap_or_else(|_| "localhost".into());
    let web = std::env::var("IRIS_WEB_PORT").unwrap_or_else(|_| "52780".into());
    let upstream = format!("{host}:{web}");
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind the fault proxy");
    let port = listener.local_addr().unwrap().port();
    let rules = rules.to_vec();
    std::thread::spawn(move || {
        for conn in listener.incoming().flatten() {
            let (upstream, rules) = (upstream.clone(), rules.clone());
            std::thread::spawn(move || {
                let _ = serve(conn, &upstream, &rules);
            });
        }
    });
    FaultProxy { port }
}

/// A session whose only IRIS is `proxy`. The workspace file is written in a temp directory
/// because a `.iris-agentic-dev.toml` further up the tree would name the real server and win over
/// the environment.
fn session_through(proxy: &FaultProxy) -> (McpSession, tempfile::TempDir) {
    let dir = tempfile::tempdir().expect("tempdir");
    std::fs::write(
        dir.path().join(".iris-agentic-dev.toml"),
        format!(
            "host = \"127.0.0.1\"\nweb_port = {}\nnamespace = \"USER\"\n",
            proxy.port
        ),
    )
    .unwrap();
    let mut env = open_gates();
    env.retain(|(k, _)| !["IRIS_WEB_PORT", "IRIS_CONTAINER", "IRIS_HOST"].contains(&k.as_str()));
    let mcp = McpSession::start_in(Some(dir.path()), &env);
    (mcp, dir)
}

/// A session on a `docker_only` connection: no Atelier, every call goes through `docker exec`.
fn docker_only_session() -> (McpSession, tempfile::TempDir) {
    let container = iris_agentic_dev_core::testing::live_container();
    let dir = tempfile::tempdir().expect("tempdir");
    std::fs::write(
        dir.path().join(".iris-agentic-dev.toml"),
        format!("docker_only = true\ncontainer = \"{container}\"\nnamespace = \"USER\"\n"),
    )
    .unwrap();
    let env: Vec<(String, String)> = [
        ("IRIS_CONTAINER", container.as_str()),
        ("IRIS_USERNAME", "_SYSTEM"),
        ("IRIS_PASSWORD", "SYS"),
        ("IRIS_WRITE_TOOLS_ENABLED", "1"),
        ("IRIS_DESTRUCTIVE_TOOLS_ENABLED", "1"),
    ]
    .iter()
    .map(|(k, v)| (k.to_string(), v.to_string()))
    .collect();
    let mcp = McpSession::start_in(Some(dir.path()), &env);
    (mcp, dir)
}

fn put_direct(rt: &tokio::runtime::Runtime, h: &Harness, doc: &str, src: &str) {
    let (status, body) = rt.block_on(h.put(doc, src));
    assert!((200..300).contains(&status), "PUT {doc}: {status} {body}");
}

fn fresh(rt: &tokio::runtime::Runtime, h: &Harness) {
    rt.block_on(async {
        h.clean().await;
        h.install_probe().await;
    });
}

fn case(tag: &str) -> &'static Case {
    CASES
        .iter()
        .find(|c| c.tag == tag)
        .unwrap_or_else(|| panic!("no case {tag}"))
}

fn error_code_is(answer: &serde_json::Value, want: &str) -> Option<String> {
    if !is_error(answer) {
        return Some(format!("not an isError result: {}", short(answer)));
    }
    let got = error_code_of(answer);
    (got != want).then(|| format!("error_code {got:?}, wanted {want}: {}", short(answer)))
}

/// An include that runs code in its includer, and the includer.
const INC_CHAIN_JUMP: &str = "ROUTINE IadGateJ [Type=INC]\n#include IadGateI\n";

const CLASS_INCLUDING_J: &str = r#"Include IadGateJ

Class IadGate.CIncJ
{

ClassMethod M() As %Status
{
    Quit $$$OK
}

}
"#;

const CLEAN_INC: &str = "ROUTINE IadGateK [Type=INC]\n#define IadGateAnswer 42\n";

const CLASS_INCLUDING_K: &str = r#"Include IadGateK

Class IadGate.CIncK
{

ClassMethod M() As %Integer
{
    Quit $$$IadGateAnswer
}

}
"#;

const CLASS_INCLUDING_LIBRARY: &str = r#"Include (%occStatus, %occErrors, Ensemble)

Class IadGate.CIncLib
{

ClassMethod M() As %Status
{
    Quit $$$OK
}

}
"#;

const DDL_WITH_DIRECTIVE: &str = r#"CREATE PROCEDURE IadGate.Pr() LANGUAGE OBJECTSCRIPT { Set x = ##expression(##class(IadGate.Probe).Mark("ddl")) Quit }"#;

/// T165. A stored target whose source cannot be read is refused whatever the reason, and the
/// compile never runs: the generator class would set its sentinel if it did.
#[test]
#[ignore]
fn iris_compile_refuses_a_target_whose_stored_source_cannot_be_read() {
    let Some(_bin) = require_iad_binary() else {
        return;
    };
    let rt = tokio::runtime::Runtime::new().unwrap();
    let h = Harness::new();
    let mut failures = Vec::new();
    for status in [500u16, 403, 401] {
        fresh(&rt, &h);
        put_direct(&rt, &h, "IadGate.CGen.cls", case("ogen").src);
        let proxy = fault_proxy(&[("/doc/", status)]);
        let (mut mcp, _dir) = session_through(&proxy);
        let answer = mcp.call(
            "iris_compile",
            &serde_json::json!({"target": "IadGate.CGen.cls"}),
        );
        if let Some(why) = error_code_is(&answer, "STORED_SOURCE_UNREADABLE") {
            failures.push(format!("HTTP {status}: {why}"));
        }
        if let Some(who) = rt.block_on(h.marker("ogen")) {
            failures.push(format!(
                "HTTP {status}: the compile ran, sentinel set by {who}"
            ));
        }
        let control = rt.block_on(async {
            h.compile("IadGate.CGen.cls").await;
            h.marker("ogen").await
        });
        if control.is_none() {
            failures.push(format!(
                "HTTP {status}: control compile set no sentinel — dead document"
            ));
        }
    }
    rt.block_on(h.clean());
    assert!(
        failures.is_empty(),
        "unreadable source compiled:\n{}",
        failures.join("\n")
    );
}

/// T165, the other half: a document that does not exist is left to the compiler. Refusing it as
/// unreadable would hide the compiler's own "does not exist" behind a permissions message.
#[test]
#[ignore]
fn iris_compile_leaves_a_missing_target_to_the_compiler() {
    let Some(_bin) = require_iad_binary() else {
        return;
    };
    let mut mcp = McpSession::start(&open_gates());
    let answer = mcp.call(
        "iris_compile",
        &serde_json::json!({"target": "IadGate.NoSuchClass"}),
    );
    let code = error_code_of(&answer);
    assert_ne!(code, "STORED_SOURCE_UNREADABLE", "{}", short(&answer));
    assert_ne!(code, "COMPILE_TIME_EXEC_BLOCKED", "{}", short(&answer));
}

/// T165. A wildcard or package target is expanded and each member checked. The member that
/// carries a generator is named; the compile does not run.
#[test]
#[ignore]
fn iris_compile_checks_every_member_of_a_wildcard_or_package_target() {
    let Some(_bin) = require_iad_binary() else {
        return;
    };
    let rt = tokio::runtime::Runtime::new().unwrap();
    let h = Harness::new();
    fresh(&rt, &h);
    put_direct(
        &rt,
        &h,
        "IadGate.CClean.cls",
        "Class IadGate.CClean\n{\n}\n",
    );
    put_direct(&rt, &h, "IadGate.CGen.cls", case("ogen").src);
    let mut mcp = McpSession::start(&open_gates());
    let mut failures = Vec::new();
    for target in [
        "IadGate.C*",
        "IadGate.*",
        "IadGate.*.cls",
        "IadGate.CG*.cls",
    ] {
        let answer = mcp.call("iris_compile", &serde_json::json!({"target": target}));
        if let Some(why) = error_code_is(&answer, "COMPILE_TIME_EXEC_BLOCKED") {
            failures.push(format!("{target}: {why}"));
        } else if body(&answer)["document"] != "IadGate.CGen.cls" {
            failures.push(format!(
                "{target}: the refusal did not name the member: {}",
                short(&answer)
            ));
        }
        if let Some(who) = rt.block_on(h.marker("ogen")) {
            failures.push(format!("{target}: sentinel set by {who}"));
        }
    }
    // A pattern that is not a valid regex used to become `.*` and select every class.
    let answer = mcp.call("iris_compile", &serde_json::json!({"target": "IadGate(*"}));
    if let Some(why) = error_code_is(&answer, "NOT_FOUND") {
        failures.push(format!("IadGate(*: {why}"));
    }
    rt.block_on(h.clean());
    assert!(
        failures.is_empty(),
        "wildcard members compiled unchecked:\n{}",
        failures.join("\n")
    );
}

/// T165. A wildcard whose members cannot be listed is refused: IRIS would expand the pattern
/// itself and the members would never be read.
#[test]
#[ignore]
fn iris_compile_refuses_a_wildcard_it_cannot_list() {
    let Some(_bin) = require_iad_binary() else {
        return;
    };
    let rt = tokio::runtime::Runtime::new().unwrap();
    let h = Harness::new();
    fresh(&rt, &h);
    put_direct(&rt, &h, "IadGate.CGen.cls", case("ogen").src);
    let proxy = fault_proxy(&[("/docnames/", 500)]);
    let (mut mcp, _dir) = session_through(&proxy);
    let answer = mcp.call("iris_compile", &serde_json::json!({"target": "IadGate.C*"}));
    let verdict = error_code_is(&answer, "STORED_SOURCE_UNREADABLE");
    let marker = rt.block_on(h.marker("ogen"));
    rt.block_on(h.clean());
    assert_eq!(verdict, None);
    assert_eq!(marker, None, "the generator ran");
}

/// T166. The include a stored class names is checked, directly and through another include. The
/// directive in the `.inc` runs inside the including class when it compiles (measured by the
/// "inc" baseline); the control compile shows that.
#[test]
#[ignore]
fn iris_compile_checks_the_includes_of_a_stored_class() {
    let Some(_bin) = require_iad_binary() else {
        return;
    };
    let rt = tokio::runtime::Runtime::new().unwrap();
    let h = Harness::new();
    let mut mcp = McpSession::start(&open_gates());
    let mut failures = Vec::new();
    // (class, its source, the include that is named, the include it hangs from)
    for (class, src, document, by) in [
        (
            "IadGate.CInc.cls",
            INC_USER,
            "IadGateI.inc",
            "IadGate.CInc.cls",
        ),
        (
            "IadGate.CIncJ.cls",
            CLASS_INCLUDING_J,
            "IadGateI.inc",
            "IadGateJ.inc",
        ),
    ] {
        fresh(&rt, &h);
        put_direct(&rt, &h, "IadGateI.inc", INC_EXEC);
        put_direct(&rt, &h, "IadGateJ.inc", INC_CHAIN_JUMP);
        put_direct(&rt, &h, class, src);
        let answer = mcp.call("iris_compile", &serde_json::json!({"target": class}));
        if let Some(why) = error_code_is(&answer, "COMPILE_TIME_EXEC_BLOCKED") {
            failures.push(format!("{class}: {why}"));
        } else {
            let b = body(&answer);
            if b["document"] != document || b["included_by"] != by {
                failures.push(format!(
                    "{class}: wrong document or includer: {}",
                    short(&answer)
                ));
            }
        }
        if let Some(who) = rt.block_on(h.marker("inc")) {
            failures.push(format!("{class}: the include ran, sentinel set by {who}"));
        }
        let control = rt.block_on(async {
            h.compile(class).await;
            h.marker("inc").await
        });
        if control.is_none() {
            failures.push(format!(
                "{class}: control compile set no sentinel — dead include"
            ));
        }
    }
    rt.block_on(h.clean());
    assert!(
        failures.is_empty(),
        "stored includes compiled unchecked:\n{}",
        failures.join("\n")
    );
}

/// T166, the twins: a clean include chain and the library includes (`%occStatus`, `%occErrors`,
/// `Ensemble`, each of which includes more) compile through iad. A check that refused them would
/// make every class that includes a library include uncompilable.
#[test]
#[ignore]
fn iris_compile_accepts_clean_includes_and_library_includes() {
    let Some(_bin) = require_iad_binary() else {
        return;
    };
    let rt = tokio::runtime::Runtime::new().unwrap();
    let h = Harness::new();
    fresh(&rt, &h);
    put_direct(&rt, &h, "IadGateK.inc", CLEAN_INC);
    put_direct(&rt, &h, "IadGate.CIncK.cls", CLASS_INCLUDING_K);
    put_direct(&rt, &h, "IadGate.CIncLib.cls", CLASS_INCLUDING_LIBRARY);
    let mut mcp = McpSession::start(&open_gates());
    let mut failures = Vec::new();
    for class in ["IadGate.CIncK.cls", "IadGate.CIncLib.cls"] {
        let answer = mcp.call("iris_compile", &serde_json::json!({"target": class}));
        if is_error(&answer) || body(&answer)["success"] != serde_json::Value::Bool(true) {
            failures.push(format!("{class}: {}", short(&answer)));
        }
    }
    rt.block_on(h.clean());
    assert!(
        failures.is_empty(),
        "clean includes were refused:\n{}",
        failures.join("\n")
    );
}

/// T166 on the local-file route: the file is clean, the include it names is stored and dangerous.
#[test]
#[ignore]
fn iris_compile_of_a_local_file_checks_the_stored_includes_it_names() {
    let Some(_bin) = require_iad_binary() else {
        return;
    };
    let rt = tokio::runtime::Runtime::new().unwrap();
    let h = Harness::new();
    fresh(&rt, &h);
    put_direct(&rt, &h, "IadGateI.inc", INC_EXEC);
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("IadGate.CInc.cls");
    std::fs::write(&path, INC_USER).unwrap();
    let mut mcp = McpSession::start_in(Some(dir.path()), &open_gates());
    let answer = mcp.call(
        "iris_compile",
        &serde_json::json!({"target": path.to_str().unwrap()}),
    );
    let verdict = error_code_is(&answer, "COMPILE_TIME_EXEC_BLOCKED");
    let (stored, marker) =
        rt.block_on(async { (h.exists("IadGate.CInc.cls").await, h.marker("inc").await) });
    rt.block_on(h.clean());
    assert_eq!(verdict, None);
    assert!(!stored, "the file was uploaded");
    assert_eq!(marker, None, "the include ran");
}

/// T167. The docker-only path reads the stored source too: a stored generator, a stored include,
/// and a wildcard member are each refused with the sentinel unset, and a clean class compiles.
#[test]
#[ignore]
fn docker_only_iris_compile_checks_the_stored_source() {
    let Some(_bin) = require_iad_binary() else {
        return;
    };
    let rt = tokio::runtime::Runtime::new().unwrap();
    let h = Harness::new();
    let (mut mcp, _dir) = docker_only_session();
    let mut failures = Vec::new();

    // (target, what is stored, the sentinel, the document the refusal names)
    for (target, tag, document) in [
        ("IadGate.CGen.cls", "ogen", "IadGate.CGen.cls"),
        ("IadGate.CGen", "ogen", "IadGate.CGen.cls"),
        ("IadGate.CInc.cls", "inc", "IadGateI.inc"),
        ("IadGate.CG*", "ogen", "IadGate.CGen.cls"),
        (
            "IadGate.CClean.cls,IadGate.CGen.cls",
            "ogen",
            "IadGate.CGen.cls",
        ),
    ] {
        fresh(&rt, &h);
        put_direct(&rt, &h, "IadGate.CGen.cls", case("ogen").src);
        put_direct(
            &rt,
            &h,
            "IadGate.CClean.cls",
            "Class IadGate.CClean\n{\n}\n",
        );
        put_direct(&rt, &h, "IadGateI.inc", INC_EXEC);
        put_direct(&rt, &h, "IadGate.CInc.cls", INC_USER);
        // The probe compiled before its sentinel was cleared by `fresh`; the sentinels are clean.
        let answer = mcp.call("iris_compile", &serde_json::json!({"target": target}));
        if let Some(why) = error_code_is(&answer, "COMPILE_TIME_EXEC_BLOCKED") {
            failures.push(format!("{target}: {why}"));
        } else if body(&answer)["document"] != document {
            failures.push(format!("{target}: wrong document: {}", short(&answer)));
        }
        if let Some(who) = rt.block_on(h.marker(tag)) {
            failures.push(format!("{target}: sentinel {tag} set by {who}"));
        }
    }

    fresh(&rt, &h);
    put_direct(
        &rt,
        &h,
        "IadGate.CClean.cls",
        "Class IadGate.CClean\n{\n}\n",
    );
    let answer = mcp.call(
        "iris_compile",
        &serde_json::json!({"target": "IadGate.CClean"}),
    );
    let b = body(&answer);
    if is_error(&answer)
        || b["success"] != serde_json::Value::Bool(true)
        || b["method"] != "docker_exec"
    {
        failures.push(format!(
            "clean class did not compile through docker exec: {}",
            short(&answer)
        ));
    }
    // A target that does not exist is the compiler's to report.
    let answer = mcp.call(
        "iris_compile",
        &serde_json::json!({"target": "IadGate.NoSuchClass"}),
    );
    let code = error_code_of(&answer);
    if code == "STORED_SOURCE_UNREADABLE" || code == "COMPILE_TIME_EXEC_BLOCKED" {
        failures.push(format!("missing target refused: {}", short(&answer)));
    }
    rt.block_on(h.clean());
    assert!(
        failures.is_empty(),
        "docker-only compile unchecked:\n{}",
        failures.join("\n")
    );
}

/// T168. DDL that creates code is scanned like a class: `##expression` in a `CREATE PROCEDURE`
/// body runs when the statement executes (the baseline's DDL case), so each way the statement can
/// reach `iris_query` is refused with the sentinel unset and no procedure class stored.
#[test]
#[ignore]
fn iris_query_ddl_with_a_compile_time_directive_in_its_body_is_refused() {
    let Some(_bin) = require_iad_binary() else {
        return;
    };
    let rt = tokio::runtime::Runtime::new().unwrap();
    let h = Harness::new();
    let mut mcp = McpSession::start(&open_gates());
    let mut failures = Vec::new();
    for (mode, force) in [("read", true), ("write", false), ("write", true)] {
        fresh(&rt, &h);
        let answer = mcp.call(
            "iris_query",
            &serde_json::json!({"query": DDL_WITH_DIRECTIVE, "mode": mode, "force": force, "namespace": "USER"}),
        );
        if let Some(why) = error_code_is(&answer, "COMPILE_TIME_EXEC_BLOCKED") {
            failures.push(format!("mode={mode} force={force}: {why}"));
        }
        let (marker, stored) =
            rt.block_on(async { (h.marker("ddl").await, h.exists("IadGate.Pr.cls").await) });
        if let Some(who) = marker {
            failures.push(format!(
                "mode={mode} force={force}: the body ran, sentinel set by {who}"
            ));
        }
        if stored {
            failures.push(format!(
                "mode={mode} force={force}: the procedure class was created"
            ));
        }
    }
    rt.block_on(h.clean());
    assert!(
        failures.is_empty(),
        "DDL with a directive got through:\n{}",
        failures.join("\n")
    );
}

/// T168, the twin: a procedure with a plain body is not refused by the code gate.
#[test]
#[ignore]
fn iris_query_ddl_without_a_compile_time_construct_is_not_refused_by_the_code_gate() {
    let Some(_bin) = require_iad_binary() else {
        return;
    };
    let rt = tokio::runtime::Runtime::new().unwrap();
    let h = Harness::new();
    fresh(&rt, &h);
    let mut mcp = McpSession::start(&open_gates());
    let answer = mcp.call(
        "iris_query",
        &serde_json::json!({
            "query": "CREATE PROCEDURE IadGate.Pr() LANGUAGE OBJECTSCRIPT { Set x = 1 Quit }",
            "mode": "read", "force": true, "namespace": "USER"
        }),
    );
    let code = error_code_of(&answer);
    rt.block_on(h.clean());
    assert_ne!(code, "COMPILE_TIME_EXEC_BLOCKED", "{}", short(&answer));
}

/// T172. `iris_doc put` with `compile=true` checks the stored includes the document names, as
/// `iris_compile` of a local file does: the directive in a stored `.inc` runs inside the including
/// class when it compiles. The refusal comes before the upload, so nothing is stored and the
/// sentinel stays unset. Without `compile` the document is stored and nothing compiles. A clean
/// include chain compiles through the same call.
#[test]
#[ignore]
fn iris_doc_put_with_compile_checks_the_stored_includes_it_names() {
    let Some(_bin) = require_iad_binary() else {
        return;
    };
    let rt = tokio::runtime::Runtime::new().unwrap();
    let h = Harness::new();
    let mut mcp = McpSession::start(&open_gates());
    let mut failures = Vec::new();
    // (class, its source, the include that is named, the include it hangs from)
    for (class, src, document, by) in [
        (
            "IadGate.CInc.cls",
            INC_USER,
            "IadGateI.inc",
            "IadGate.CInc.cls",
        ),
        (
            "IadGate.CIncJ.cls",
            CLASS_INCLUDING_J,
            "IadGateI.inc",
            "IadGateJ.inc",
        ),
    ] {
        fresh(&rt, &h);
        put_direct(&rt, &h, "IadGateI.inc", INC_EXEC);
        put_direct(&rt, &h, "IadGateJ.inc", INC_CHAIN_JUMP);
        let answer = iris_doc_put(&mut mcp, class, src, true);
        if let Some(why) = error_code_is(&answer, "COMPILE_TIME_EXEC_BLOCKED") {
            failures.push(format!("{class}: {why}"));
        } else {
            let b = body(&answer);
            if b["document"] != document || b["included_by"] != by {
                failures.push(format!(
                    "{class}: wrong document or includer: {}",
                    short(&answer)
                ));
            }
        }
        let (stored, marker) =
            rt.block_on(async { (h.exists(class).await, h.marker("inc").await) });
        if stored {
            failures.push(format!("{class}: stored despite the refusal"));
        }
        if let Some(who) = marker {
            failures.push(format!("{class}: the include ran, sentinel set by {who}"));
        }

        // The same document without `compile` is stored, and nothing compiles. Clear the sentinel
        // first, so a set one is this call's.
        rt.block_on(h.run(" Kill ^IadGateProbe"));
        let answer = iris_doc_put(&mut mcp, class, src, false);
        if is_error(&answer) {
            failures.push(format!(
                "{class}: put without compile refused: {}",
                short(&answer)
            ));
        }
        let (stored, marker) =
            rt.block_on(async { (h.exists(class).await, h.marker("inc").await) });
        if !stored {
            failures.push(format!("{class}: put without compile did not store it"));
        }
        if let Some(who) = marker {
            failures.push(format!(
                "{class}: put without compile ran the include ({who})"
            ));
        }
    }

    // The twin: a clean include chain compiles through `iris_doc put`.
    fresh(&rt, &h);
    put_direct(&rt, &h, "IadGateK.inc", CLEAN_INC);
    let answer = iris_doc_put(&mut mcp, "IadGate.CIncK.cls", CLASS_INCLUDING_K, true);
    if is_error(&answer) || body(&answer)["success"] != serde_json::Value::Bool(true) {
        failures.push(format!("clean include chain refused: {}", short(&answer)));
    }
    rt.block_on(h.clean());
    assert!(
        failures.is_empty(),
        "iris_doc put compiled stored includes unchecked:\n{}",
        failures.join("\n")
    );
}

/// T173. Docker-only `iris_compile` hands IRIS the name form it reads: `$SYSTEM.OBJ.Compile` takes
/// class names and answered `X.cls` with ERROR #5351 ("Class 'X.cls' does not exist"), and it
/// takes no routine at all. Every target form that compiles over Atelier compiles here: a class
/// with and without `.cls`, a routine, a wildcard, and a list that mixes them.
#[test]
#[ignore]
fn docker_only_iris_compile_accepts_every_target_form() {
    let Some(_bin) = require_iad_binary() else {
        return;
    };
    let rt = tokio::runtime::Runtime::new().unwrap();
    let h = Harness::new();
    fresh(&rt, &h);
    put_direct(
        &rt,
        &h,
        "IadGate.CClean.cls",
        "Class IadGate.CClean\n{\n}\n",
    );
    put_direct(
        &rt,
        &h,
        "IadGateM.mac",
        "ROUTINE IadGateM\nIadGateM ;\n    Quit\n",
    );
    let (mut mcp, _dir) = docker_only_session();
    let mut failures = Vec::new();
    for target in [
        "IadGate.CClean.cls",
        "IadGate.CClean",
        "IadGateM.mac",
        "IadGate.CC*.cls",
        "IadGate.CC*",
        "IadGate.CClean.cls, IadGateM.mac",
    ] {
        let answer = mcp.call("iris_compile", &serde_json::json!({"target": target}));
        let b = body(&answer);
        if is_error(&answer) || b["success"] != serde_json::Value::Bool(true) {
            failures.push(format!("{target}: {}", short(&answer)));
        } else if b["method"] != "docker_exec" {
            failures.push(format!(
                "{target}: did not go through docker exec: {}",
                short(&answer)
            ));
        }
    }
    // A target that is not there is still the compiler's to report, as a failure.
    let answer = mcp.call(
        "iris_compile",
        &serde_json::json!({"target": "IadGate.NoSuchClass.cls"}),
    );
    if body(&answer)["success"] == serde_json::Value::Bool(true) {
        failures.push(format!("a missing class compiled: {}", short(&answer)));
    }
    rt.block_on(h.clean());
    assert!(
        failures.is_empty(),
        "docker-only compile target forms:\n{}",
        failures.join("\n")
    );
}

// ── The switch (T089, R4a D9) ────────────────────────────────────────────────────────────────

/// A generator that writes a body and nothing else. The gate refuses it by construct, not by what
/// it does; with the gate off it must put, compile and run.
const HARMLESS_GENERATOR: &str = r#"Class IadGate.Switch141
{

ClassMethod G() As %Integer [ CodeMode = objectgenerator ]
{
    Do %code.WriteLine("    Quit 141")
    Quit $$$OK
}

}
"#;

fn gate_off_env() -> Vec<(String, String)> {
    let mut env = open_gates();
    env.push(("IAD_COMPILE_TIME_GATE".to_string(), "0".to_string()));
    env
}

/// Gate off for the default server: the put and the compile of a stored class go through, and each
/// response says the gate was off and where that came from. With the gate on, the same put is
/// refused and the response carries no such note.
#[test]
#[ignore]
fn a_switched_off_gate_lets_a_generator_through_and_says_so() {
    let Some(_bin) = require_iad_binary() else {
        return;
    };
    let rt = tokio::runtime::Runtime::new().unwrap();
    let h = Harness::new();
    rt.block_on(h.clean());

    // On: refused, no note.
    let mut on = McpSession::start(&open_gates());
    let refused = iris_doc_put(&mut on, "IadGate.Switch141.cls", HARMLESS_GENERATOR, true);
    assert_eq!(
        not_a_compile_gate_refusal(&refused),
        None,
        "gate on must refuse the generator"
    );
    assert!(
        body(&refused).get("compile_time_gate").is_none(),
        "a refusal does not carry the off note: {}",
        short(&refused)
    );
    assert!(!rt.block_on(h.exists("IadGate.Switch141.cls")));
    drop(on);

    // Off: put + compile succeeds, says so.
    let mut off = McpSession::start(&gate_off_env());
    let put = iris_doc_put(&mut off, "IadGate.Switch141.cls", HARMLESS_GENERATOR, true);
    let b = body(&put);
    assert!(!is_error(&put), "gate off, put refused: {}", short(&put));
    assert_eq!(b["success"], true, "{}", short(&put));
    assert_eq!(b["compile_time_gate"], "off", "{}", short(&put));
    assert!(
        b["compile_time_gate_source"]
            .as_str()
            .is_some_and(|s| s.starts_with("launch")),
        "the note names the launch setting: {}",
        short(&put)
    );

    // The stored class compiles by name with the gate off, and the compile says so too.
    let compiled = off.call(
        "iris_compile",
        &serde_json::json!({"target": "IadGate.Switch141.cls"}),
    );
    let cb = body(&compiled);
    assert!(!is_error(&compiled), "{}", short(&compiled));
    assert_eq!(cb["success"], true, "{}", short(&compiled));
    assert_eq!(cb["compile_time_gate"], "off", "{}", short(&compiled));
    drop(off);

    // The same stored class, compiled with the gate back on, is refused: the stored-source check
    // still reads what is on the server.
    let mut on_again = McpSession::start(&open_gates());
    let again = on_again.call(
        "iris_compile",
        &serde_json::json!({"target": "IadGate.Switch141.cls"}),
    );
    assert_eq!(
        not_a_compile_gate_refusal(&again),
        None,
        "{}",
        short(&again)
    );
    drop(on_again);
    rt.block_on(h.clean());
}

//! Spec 130: live proof for each content-skill claim.
//!
//! Every test shows iris-dev-iris doing what the new or corrected skill text says. The wording
//! guards are in `tests/unit/test_content_skills_130.rs`; the reproductions are in
//! `specs/130-content-skills/research.md`. Each test builds its own `IadLive130.*` objects and drops
//! them at the end. Nothing here calls embedded Python.
//!
//! Run with:
//!   IRIS_HOST=localhost IRIS_WEB_PORT=52780 IRIS_USERNAME=_SYSTEM IRIS_PASSWORD=SYS \
//!   cargo test --features testing --test integration test_content_skills_130_live -- \
//!     --ignored --test-threads=1

use iris_agentic_dev_core::iris::connection::{
    is_generator_error, CompileResult, DiscoverySource, IrisConnection,
};

const NS: &str = "USER";

/// The connection, or a panic naming what to set. `IAD_ALLOW_SKIP=1` opts into skipping.
fn conn() -> Option<(IrisConnection, reqwest::Client)> {
    let host = std::env::var("IRIS_HOST").unwrap_or_default();
    if host.is_empty() {
        if std::env::var("IAD_ALLOW_SKIP").is_ok() {
            eprintln!("SKIP (IAD_ALLOW_SKIP set): IRIS_HOST unset");
            return None;
        }
        panic!(
            "IRIS_HOST unset. This test proves a skill claim against live IRIS; without IRIS it \
             asserts nothing, so it fails instead of passing quietly.\n\
             Set: IRIS_HOST=localhost IRIS_WEB_PORT=52780 IRIS_USERNAME=_SYSTEM IRIS_PASSWORD=SYS\n\
             Or opt into skipping deliberately: IAD_ALLOW_SKIP=1"
        );
    }
    let port = std::env::var("IRIS_WEB_PORT").unwrap_or_else(|_| "52780".into());
    let user = std::env::var("IRIS_USERNAME").unwrap_or_else(|_| "_SYSTEM".into());
    let pass = std::env::var("IRIS_PASSWORD").unwrap_or_else(|_| "SYS".into());
    let conn = IrisConnection::new(
        format!("http://{host}:{port}"),
        NS,
        user,
        pass,
        DiscoverySource::EnvVar,
    );
    Some((conn, reqwest::Client::new()))
}

/// Atelier PUT; returns the HTTP status and body so a test can assert on a refusal.
async fn put_raw(
    c: &IrisConnection,
    client: &reqwest::Client,
    doc: &str,
    src: &str,
) -> (u16, String) {
    let url = c.versioned_ns_url(
        NS,
        &format!("/doc/{}?ignoreConflict=1", urlencoding::encode(doc)),
    );
    let lines: Vec<&str> = src.lines().collect();
    let resp = client
        .put(&url)
        .basic_auth(&c.username, Some(&c.password))
        .json(&serde_json::json!({"enc": false, "content": lines}))
        .send()
        .await
        .expect("PUT must reach IRIS");
    let status = resp.status().as_u16();
    (status, resp.text().await.unwrap_or_default())
}

async fn put_and_compile(
    c: &IrisConnection,
    client: &reqwest::Client,
    doc: &str,
    src: &str,
) -> CompileResult {
    let (status, body) = put_raw(c, client, doc, src).await;
    assert!(
        (200..300).contains(&status),
        "PUT {doc}: HTTP {status} {body}"
    );
    compile(c, client, doc).await
}

async fn compile(c: &IrisConnection, client: &reqwest::Client, doc: &str) -> CompileResult {
    c.compile_document(doc, NS, "cuk", client)
        .await
        .expect("compile request must reach IRIS")
}

fn compile_text(r: &CompileResult) -> String {
    format!("{}\n{}", r.errors.join("\n"), r.console.join("\n"))
}

/// Run ObjectScript and return its output; a generator error fails the test.
async fn run(c: &IrisConnection, client: &reqwest::Client, code: &str) -> String {
    let out = c
        .execute_via_generator(code, NS, client)
        .await
        .expect("execute request must reach IRIS");
    assert!(!is_generator_error(&out), "IRIS run failed: {out}");
    out.trim().to_string()
}

/// The text between `~[` and `]~` in `out`, so RunTest's own output does not get in the way.
fn marked(out: &str) -> String {
    let start = out
        .rfind("~[")
        .unwrap_or_else(|| panic!("no ~[ marker in {out}"));
    let end = out[start..]
        .find("]~")
        .unwrap_or_else(|| panic!("no ]~ marker in {out}"));
    out[start + 2..start + end].to_string()
}

async fn drop_classes(c: &IrisConnection, client: &reqwest::Client, names: &[&str]) {
    for n in names {
        let _ = c
            .execute_via_generator(
                &format!(" Do $system.OBJ.Delete(\"{n}\",\"e-d\")"),
                NS,
                client,
            )
            .await;
    }
}

/// The plan text `iris_query(mode="explain")` returns: `EXPLAIN` through `/action/query`.
async fn explain(c: &IrisConnection, client: &reqwest::Client, sql: &str) -> String {
    let url = c.versioned_ns_url(NS, "/action/query");
    let body: serde_json::Value = client
        .post(&url)
        .basic_auth(&c.username, Some(&c.password))
        .json(&serde_json::json!({"query": format!("EXPLAIN {sql}")}))
        .send()
        .await
        .expect("EXPLAIN must reach IRIS")
        .json()
        .await
        .expect("EXPLAIN body is JSON");
    body["result"]["content"][0]["Plan"]
        .as_str()
        .unwrap_or_else(|| panic!("no Plan in {body}"))
        .to_string()
}

async fn sql_count(c: &IrisConnection, client: &reqwest::Client, sql: &str) -> String {
    run(
        c,
        client,
        &format!(
            " Set rs=##class(%SQL.Statement).%ExecDirect(,\"{}\")\n \
             If rs.%SQLCODE<0 {{ Write \"SQLCODE=\",rs.%SQLCODE Quit }}\n \
             Do rs.%Next() Write rs.%GetData(1)",
            sql.replace('"', "\"\"")
        ),
    )
    .await
}

// ── R1 ───────────────────────────────────────────────────────────────────────────────────────

const RATIO: &str = "Class IadLive130.Ratio Extends %Persistent
{

Property Amount As %Integer;

}";

/// R1. A divide by zero in the select list surfaces at fetch time: `%SQLCODE` is 0 after execute,
/// `%Next(.tSC)` returns 0 with SQLCODE -400 in `tSC`, and `%SQLCODE` is -400 after the loop.
/// `%Prepare` of a missing table returns an error status and does not throw.
#[tokio::test]
#[ignore]
async fn sql_statement_reports_fetch_errors_only_at_fetch() {
    let Some((c, client)) = conn() else { return };
    drop_classes(&c, &client, &["IadLive130.Ratio"]).await;
    let r = put_and_compile(&c, &client, "IadLive130.Ratio.cls", RATIO).await;
    assert!(r.success(), "Ratio: {}", compile_text(&r));
    run(
        &c,
        &client,
        " Do ##class(IadLive130.Ratio).%KillExtent() Set o=##class(IadLive130.Ratio).%New(), o.Amount=10 Do o.%Save()",
    )
    .await;

    let out = run(
        &c,
        &client,
        " Set rs=##class(%SQL.Statement).%ExecDirect(,\"SELECT Amount/? AS r FROM IadLive130.Ratio\",0)\n \
         Set afterExec=rs.%SQLCODE, n=0\n \
         While rs.%Next(.tSC) { Set n=n+1 }\n \
         Write \"~[\",afterExec,\"|\",n,\"|\",$Select($$$ISERR(tSC):$System.Status.GetErrorText(tSC),1:\"OK\"),\"|\",rs.%SQLCODE,\"]~\"",
    )
    .await;
    let parts: Vec<String> = marked(&out).split('|').map(str::to_string).collect();
    assert_eq!(parts[0], "0", "%SQLCODE after execute: {out}");
    assert_eq!(parts[1], "0", "rows read: {out}");
    assert!(
        parts[2].contains("-400"),
        "%Next(.tSC) must carry SQLCODE -400: {out}"
    );
    assert_eq!(parts[3], "-400", "%SQLCODE after the loop: {out}");

    let prep = run(
        &c,
        &client,
        " Set st=##class(%SQL.Statement).%New() Set tSC=st.%Prepare(\"SELECT * FROM IadLive130.NoSuch\")\n \
         Write \"~[\",$Select($$$ISERR(tSC):$System.Status.GetErrorText(tSC),1:\"OK\"),\"]~\"",
    )
    .await;
    assert!(
        marked(&prep).contains("-30"),
        "%Prepare must return SQLCODE -30 as a status: {prep}"
    );

    drop_classes(&c, &client, &["IadLive130.Ratio"]).await;
}

// ── R2 ───────────────────────────────────────────────────────────────────────────────────────

const NSCLS: &str = r#"Class IadLive130.Ns
{

ClassMethod WithNew() As %String
{
    New $NAMESPACE
    Set $NAMESPACE = "%SYS"
    Quit $NAMESPACE
}

ClassMethod WithoutNew() As %String
{
    Set $NAMESPACE = "%SYS"
    Quit $NAMESPACE
}

}"#;

/// R2. `New $NAMESPACE` gives the caller its namespace back when the method exits; without it the
/// caller is left in `%SYS`.
#[tokio::test]
#[ignore]
async fn new_namespace_restores_the_callers_namespace() {
    let Some((c, client)) = conn() else { return };
    let r = put_and_compile(&c, &client, "IadLive130.Ns.cls", NSCLS).await;
    assert!(r.success(), "Ns: {}", compile_text(&r));
    let with = run(
        &c,
        &client,
        " Set a=##class(IadLive130.Ns).WithNew() Write \"~[\",a,\",\",$NAMESPACE,\"]~\"",
    )
    .await;
    assert_eq!(marked(&with), "%SYS,USER");
    let without = run(
        &c,
        &client,
        " Set a=##class(IadLive130.Ns).WithoutNew() Set b=$NAMESPACE Set $NAMESPACE=\"USER\" Write \"~[\",b,\"]~\"",
    )
    .await;
    assert_eq!(marked(&without), "%SYS");
    drop_classes(&c, &client, &["IadLive130.Ns"]).await;
}

// ── R3 ───────────────────────────────────────────────────────────────────────────────────────

const MISNAMED: &str = "Class IadLive130.Misnamed Extends %UnitTest.TestCase
{

Method TestOk()
{
    Do $$$AssertEquals(1+1, 2, \"one plus one\")
}

Method CheckAdd()
{
    Do $$$AssertEquals(1+1, 3, \"never runs\")
}

}";

const ONLY_MISNAMED: &str = "Class IadLive130.OnlyMisnamed Extends %UnitTest.TestCase
{

Method CheckAdd()
{
    Do $$$AssertEquals(1+1, 3, \"never runs\")
}

}";

/// Run one compiled class with `DebugRunTestCase`, then list the methods it recorded and whether
/// any assertion failed, as `methods|failed`.
///
/// This used `RunTest(":<class>","/noload/nodelete")` with `^UnitTestRoot` at `/tmp/`. That still
/// walks every directory under the root and runs what it finds there, so on the CI container, whose
/// `/tmp/httest` holds `IrisDevRunTest`, it ran that suite and never this class.
/// `DebugRunTestCase` passes `/norecursive` and runs the named class on 2025.3 and 2026.2 alike.
/// The manager runs in a JOB (`run_in_job`), as `iris_test` does: when `messages.log` grows during
/// a run, `%UnitTest.Manager` leaves `$IO` on `$PRINCIPAL`, which in this CSP process is the
/// gateway socket. That failed CI's full e2e run twice on 2026-10-21; restoring `$IO` afterwards
/// covers only a line that lands after the last test.
async fn run_unit_test(c: &IrisConnection, client: &reqwest::Client, class: &str) -> String {
    let out = run(
        c,
        client,
        &format!(
            " Set had=$Data(^UnitTestRoot)#2, old=$Get(^UnitTestRoot), ^UnitTestRoot=\"/tmp/\"\n \
             {}\n \
             If had Set ^UnitTestRoot=old\n Else  Kill ^UnitTestRoot\n \
             Set idx=$Order(^UnitTest.Result(\"\"),-1), ref=$Name(^UnitTest.Result(idx)), m=\"\", bad=0\n \
             For {{ Set ref=$Query(@ref) Quit:ref=\"\"  Quit:$QSubscript(ref,1)'=idx  If $QLength(ref)=4 {{ Set m=m_$QSubscript(ref,4)_\",\" Set:$ListGet(@ref,1)=0 bad=1 }} }}\n \
             Write \"~[\",m,\"|\",bad,\"]~\"",
            iris_agentic_dev_core::tools::run_in_job(&format!(
                "##class(%UnitTest.Manager).DebugRunTestCase(\"\",\"{class}\")"
            ))
        ),
    )
    .await;
    marked(&out)
}

/// R3. Only methods whose names start with `Test` run. A class whose one assertion is wrong but
/// sits in `CheckAdd` records no failure, and a class with no `Test*` method records nothing.
#[tokio::test]
#[ignore]
async fn only_test_prefixed_methods_run() {
    let Some((c, client)) = conn() else { return };
    for (doc, src) in [
        ("IadLive130.Misnamed.cls", MISNAMED),
        ("IadLive130.OnlyMisnamed.cls", ONLY_MISNAMED),
    ] {
        let r = put_and_compile(&c, &client, doc, src).await;
        assert!(r.success(), "{doc}: {}", compile_text(&r));
    }
    let got = run_unit_test(&c, &client, "IadLive130.Misnamed").await;
    assert!(got.contains("TestOk,"), "TestOk must run: {got}");
    assert!(!got.contains("CheckAdd"), "CheckAdd must not run: {got}");
    assert!(got.ends_with("|0"), "no failure is recorded: {got}");

    let none = run_unit_test(&c, &client, "IadLive130.OnlyMisnamed").await;
    assert_eq!(
        none, "|0",
        "a class with no Test* method runs nothing: {none}"
    );
    drop_classes(
        &c,
        &client,
        &["IadLive130.Misnamed", "IadLive130.OnlyMisnamed"],
    )
    .await;
}

// ── R4 ───────────────────────────────────────────────────────────────────────────────────────

const CLEAN: &str = r#"Class IadLive130.CleanCompile
{

ClassMethod Total(pA As %Integer, pB As %Integer) As %Integer
{
    Set tSum = pA + pB
    Quit tSun
}

ClassMethod Dyn() As %String
{
    Quit $CLASSMETHOD("IadLive130.CleanCompile", "Nope")
}

}"#;

const LITERAL: &str = r#"Class IadLive130.Literal
{

ClassMethod Call() As %String
{
    Quit ..Nope()
}

}"#;

/// R4. A typo'd local and a dynamic call to a missing method compile clean and fail at runtime.
/// A literal `..Nope()` is caught at compile time.
#[tokio::test]
#[ignore]
async fn a_clean_compile_can_still_fail_at_runtime() {
    let Some((c, client)) = conn() else { return };
    let r = put_and_compile(&c, &client, "IadLive130.CleanCompile.cls", CLEAN).await;
    assert!(
        r.success(),
        "CleanCompile must compile: {}",
        compile_text(&r)
    );
    let out = run(
        &c,
        &client,
        " Set a=\"\",b=\"\"\n \
         Try { Set a=##class(IadLive130.CleanCompile).Total(2,3) } Catch e { Set a=e.Name }\n \
         Try { Set b=##class(IadLive130.CleanCompile).Dyn() } Catch e { Set b=e.Name }\n \
         Write \"~[\",a,\"|\",b,\"]~\"",
    )
    .await;
    assert_eq!(marked(&out), "<UNDEFINED>|<METHOD DOES NOT EXIST>");

    let lit = put_and_compile(&c, &client, "IadLive130.Literal.cls", LITERAL).await;
    assert!(!lit.success(), "..Nope() must not compile");
    assert!(
        compile_text(&lit).contains("Nope"),
        "the compile error names the method: {}",
        compile_text(&lit)
    );
    drop_classes(
        &c,
        &client,
        &["IadLive130.CleanCompile", "IadLive130.Literal"],
    )
    .await;
}

// ── R5 ───────────────────────────────────────────────────────────────────────────────────────

const ORDERS_V1: &str = "Class IadLive130.Orders Extends %Persistent [ DdlAllowed ]
{

Property Customer As %String;

Property Status As %String;

}";

const ORDERS_V2: &str = "Class IadLive130.Orders Extends %Persistent [ DdlAllowed ]
{

Property Customer As %String;

Property Status As %String;

Index StatusIdx On Status;

Index CustIdx On Customer;

}";

/// R5. Plans and indexes: no index reads the master map; DDL `CREATE INDEX` builds at once and the
/// plan reads the index map; an index added in the class is empty until `%BuildIndices`; `INSERT
/// %NOINDEX` leaves it stale and `WHERE %NOINDEX` shows the true count; after `TUNE TABLE` a value
/// on 98% of rows reads the master map on purpose.
#[tokio::test]
#[ignore]
async fn class_added_index_is_empty_until_built() {
    let Some((c, client)) = conn() else { return };
    drop_classes(&c, &client, &["IadLive130.Orders"]).await;
    let r = put_and_compile(&c, &client, "IadLive130.Orders.cls", ORDERS_V1).await;
    assert!(r.success(), "Orders v1: {}", compile_text(&r));
    run(
        &c,
        &client,
        " For i=1:1:5000 { Set o=##class(IadLive130.Orders).%New(), o.Customer=\"C\"_(i#500), o.Status=$Select(i#50=0:\"OPEN\",1:\"DONE\") Do o.%Save() }",
    )
    .await;

    let open = "SELECT Customer FROM IadLive130.Orders WHERE Status = 'OPEN'";
    let plan = explain(&c, &client, open).await;
    assert!(plan.contains("Read master map"), "no index: {plan}");

    let ddl = run(
        &c,
        &client,
        " Set rs=##class(%SQL.Statement).%ExecDirect(,\"CREATE INDEX StatusIdx ON IadLive130.Orders (Status)\") Write \"~[\",rs.%SQLCODE,\"]~\"",
    )
    .await;
    assert_eq!(marked(&ddl), "0", "CREATE INDEX: {ddl}");
    let plan = explain(&c, &client, open).await;
    assert!(
        plan.contains("Read index map IadLive130.Orders.StatusIdx"),
        "DDL index is used: {plan}"
    );
    assert_eq!(
        sql_count(
            &c,
            &client,
            "SELECT COUNT(*) FROM IadLive130.Orders WHERE Status = 'OPEN'"
        )
        .await,
        "100",
        "DDL built StatusIdx"
    );

    let r = put_and_compile(&c, &client, "IadLive130.Orders.cls", ORDERS_V2).await;
    assert!(r.success(), "Orders v2: {}", compile_text(&r));
    let by_index = "SELECT COUNT(*) FROM IadLive130.Orders WHERE Customer = 'C1'";
    let by_scan = "SELECT COUNT(*) FROM IadLive130.Orders WHERE %NOINDEX Customer = 'C1'";
    assert_eq!(
        sql_count(&c, &client, by_index).await,
        "0",
        "unbuilt CustIdx"
    );
    assert_eq!(sql_count(&c, &client, by_scan).await, "10");

    let built = run(
        &c,
        &client,
        " Set sc=##class(IadLive130.Orders).%BuildIndices($ListBuild(\"CustIdx\")) Write \"~[\",+sc,\"]~\"",
    )
    .await;
    assert_eq!(marked(&built), "1");
    assert_eq!(
        sql_count(&c, &client, by_index).await,
        "10",
        "built CustIdx"
    );

    run(
        &c,
        &client,
        " Set rs=##class(%SQL.Statement).%ExecDirect(,\"INSERT %NOINDEX INTO IadLive130.Orders (Customer, Status) VALUES ('ZNEW', 'OPEN')\")",
    )
    .await;
    assert_eq!(
        sql_count(
            &c,
            &client,
            "SELECT COUNT(*) FROM IadLive130.Orders WHERE Customer = 'ZNEW'"
        )
        .await,
        "0",
        "INSERT %NOINDEX leaves CustIdx stale"
    );
    assert_eq!(
        sql_count(
            &c,
            &client,
            "SELECT COUNT(*) FROM IadLive130.Orders WHERE %NOINDEX Customer = 'ZNEW'"
        )
        .await,
        "1"
    );

    let tune = run(
        &c,
        &client,
        " Set rs=##class(%SQL.Statement).%ExecDirect(,\"TUNE TABLE IadLive130.Orders\") Write \"~[\",rs.%SQLCODE,\"]~\"",
    )
    .await;
    assert_eq!(marked(&tune), "0", "TUNE TABLE: {tune}");
    let done = explain(
        &c,
        &client,
        "SELECT Customer FROM IadLive130.Orders WHERE Status = 'DONE'",
    )
    .await;
    assert!(
        done.contains("Read master map") && !done.contains("StatusIdx"),
        "an outlier value reads the master map: {done}"
    );
    drop_classes(&c, &client, &["IadLive130.Orders"]).await;
}

const LOADER_SALES: &str = "Class IadLive130.Sales Extends %Persistent
{

Property Region As %String;

Index RegionIdx On Region;

}";

/// A loader in SKILL-13's shape: empty the extent, then `INSERT %NOINDEX` 500 rows over 50 regions.
/// `build` adds the `%BuildIndices` call the corrected skill says belongs in the loader. The shape
/// matters: with 40 rows over 2 values, the statistics gathered at the build make a master-map scan
/// cheaper, so `COUNT(*)` reads the data and hides the empty index.
fn loader_class(build: bool) -> String {
    let tail = if build {
        "Quit ##class(IadLive130.Sales).%BuildIndices($ListBuild(\"RegionIdx\"))"
    } else {
        "Quit $$$OK"
    };
    format!(
        "Class IadLive130.SalesLoad Extends %RegisteredObject
{{

ClassMethod Run() As %Status
{{
    Do ##class(IadLive130.Sales).%KillExtent()
    Set tStmt = ##class(%SQL.Statement).%New()
    Set tSC = tStmt.%Prepare(\"INSERT %NOINDEX INTO IadLive130.Sales (Region) VALUES (?)\")
    If $$$ISERR(tSC) {{ Quit tSC }}
    For i = 1:1:500 {{
        Set tRs = tStmt.%Execute(\"R\"_(i#50))
        If tRs.%SQLCODE < 0 {{ Quit }}
    }}
    {tail}
}}

}}"
    )
}

/// Round 3, SKILL-13. A `%BuildIndices` run by hand fixes the count only until the loader runs
/// again; a loader that calls it after its `INSERT %NOINDEX` stays right on every run.
#[tokio::test]
#[ignore]
async fn a_rebuild_by_hand_lasts_until_the_next_noindex_load() {
    let Some((c, client)) = conn() else { return };
    let classes = ["IadLive130.SalesLoad", "IadLive130.Sales"];
    drop_classes(&c, &client, &classes).await;
    let r = put_and_compile(&c, &client, "IadLive130.Sales.cls", LOADER_SALES).await;
    assert!(r.success(), "Sales: {}", compile_text(&r));
    let r = put_and_compile(
        &c,
        &client,
        "IadLive130.SalesLoad.cls",
        &loader_class(false),
    )
    .await;
    assert!(r.success(), "SalesLoad: {}", compile_text(&r));

    let load = " Set sc=##class(IadLive130.SalesLoad).Run() Write \"~[\",+sc,\"]~\"";
    // The index entries for R1, read from the index global. Not a query: once a build has left
    // statistics behind, the planner may read the master map, and then both `COUNT(*)` and a row
    // fetch report 10 over an empty index. The test is about what the index holds.
    let eu = " Set n=0,id=\"\" For { Set id=$Order(^IadLive130.SalesI(\"RegionIdx\",\" R1\",id)) Quit:id=\"\"  Set n=n+1 } Write \"~[\",n,\"]~\"";
    assert_eq!(marked(&run(&c, &client, load).await), "1");
    assert_eq!(marked(&run(&c, &client, eu).await), "0", "%NOINDEX load");

    let built = run(
        &c,
        &client,
        " Set sc=##class(IadLive130.Sales).%BuildIndices($ListBuild(\"RegionIdx\")) Write \"~[\",+sc,\"]~\"",
    )
    .await;
    assert_eq!(marked(&built), "1");
    assert_eq!(marked(&run(&c, &client, eu).await), "10", "rebuilt by hand");
    assert_eq!(marked(&run(&c, &client, load).await), "1");
    assert_eq!(
        marked(&run(&c, &client, eu).await),
        "0",
        "the next load undoes a rebuild by hand"
    );

    let r = put_and_compile(&c, &client, "IadLive130.SalesLoad.cls", &loader_class(true)).await;
    assert!(r.success(), "SalesLoad with build: {}", compile_text(&r));
    for pass in 1..=2 {
        assert_eq!(marked(&run(&c, &client, load).await), "1");
        assert_eq!(
            marked(&run(&c, &client, eu).await),
            "10",
            "loader that builds, run {pass}"
        );
    }
    drop_classes(&c, &client, &classes).await;
}

// ── R6 ───────────────────────────────────────────────────────────────────────────────────────

const EMPTY_PROD: &str = "Class IadLive130.EmptyProd Extends Ens.Production
{

XData ProductionDefinition
{
<Production Name=\"IadLive130.EmptyProd\">
</Production>
}

}";

/// R6. `GetProductionStatus` reports 1 and the name while running, 2 and "" when stopped. A
/// second start is `ErrProductionAlreadyRunning`. `GetProductionState` does not exist.
/// `GetActiveProductionName` still names the production after it stops.
#[tokio::test]
#[ignore]
async fn director_status_calls_match_the_skill() {
    let Some((c, client)) = conn() else { return };
    // `CleanProduction` clears a production an earlier test left suspended (state 3), which a force
    // stop does not.
    let _ = c
        .execute_via_generator(
            " Do ##class(Ens.Director).StopProduction(10,1)\n Do ##class(Ens.Director).CleanProduction()",
            NS,
            &client,
        )
        .await;
    let r = put_and_compile(&c, &client, "IadLive130.EmptyProd.cls", EMPTY_PROD).await;
    assert!(r.success(), "EmptyProd: {}", compile_text(&r));
    let out = run(
        &c,
        &client,
        " Set before=+$Order(^Ens.MessageHeaderD(\"\"),-1)\n \
         Set sc=##class(Ens.Director).StartProduction(\"IadLive130.EmptyProd\") Set s1=+sc\n \
         Set sc=##class(Ens.Director).GetProductionStatus(.n1,.st1)\n \
         Set sc=##class(Ens.Director).StartProduction(\"IadLive130.EmptyProd\") Set again=$System.Status.GetErrorCodes(sc)_\" \"_$System.Status.GetErrorText(sc)\n \
         Set running=##class(Ens.Director).IsProductionRunning(\"IadLive130.EmptyProd\")\n \
         For waitn=1:1:60 { Set rs=##class(%SQL.Statement).%ExecDirect(,\"SELECT COUNT(*) AS n, SUM(CASE WHEN Status IN (1,2,3) THEN 1 ELSE 0 END) AS pend FROM Ens.MessageHeader WHERE ID > ? AND TargetConfigName = 'Ens.ScheduleHandler'\",before) Do rs.%Next() Quit:(rs.n>0)&&(+rs.pend=0)  Hang 0.5 }\n \
         Set sc=##class(Ens.Director).StopProduction(60,0) Set s2=+sc,stoperr=$Select(sc:\"\",1:$System.Status.GetErrorText(sc))\n \
         Set hist=\"\" For i=1:1:20 { Set sc=##class(Ens.Director).GetProductionStatus(.n2,.st2) Set hist=hist_st2 Quit:st2'=3  Hang 0.5 }\n \
         Set active=##class(Ens.Director).GetActiveProductionName()\n \
         Try { Do $CLASSMETHOD(\"Ens.Director\",\"GetProductionState\") Set gps=\"exists\" } Catch e { Set gps=e.Name }\n \
         Write \"~[\",s1,\"|\",n1,\"|\",st1,\"|\",running,\"|\",s2,\"|\",n2,\"|\",st2,\"|\",active,\"|\",gps,\"|\",again,\"|\",stoperr,\"|\",hist,\"|\",waitn,\"]~\"",
    )
    .await;
    // A start sends Ens.ScheduleService a request to Ens.ScheduleHandler, whose OnRequest calls
    // UpdateProduction and so needs the runtime lock. StopProduction holds that lock while it waits
    // for the jobs to end; if it gets there first, the handler waits 30 s, dies with
    // ErrCanNotAcquireRuntimeLock, and the stop leaves the production suspended (state 3). On the CI
    // runner the handler was still pending when the stop came, every run since 2026-10-07. So the
    // stop waits until that request has completed (`waitn` is the polls it took, at 0.5 s each),
    // and the status is still polled for up to 10 s; `hist` is every state it read.
    let p: Vec<String> = marked(&out).split('|').map(str::to_string).collect();
    assert_eq!(p[0], "1", "start: {out}");
    assert_eq!(p[1], "IadLive130.EmptyProd", "running name: {out}");
    assert_eq!(p[2], "1", "running state: {out}");
    assert_eq!(p[3], "1", "IsProductionRunning: {out}");
    assert_eq!(p[4], "1", "stop: {out}");
    assert_eq!(p[5], "", "stopped name: {out}");
    assert_eq!(p[6], "2", "stopped state: {out}");
    assert_eq!(
        p[7], "IadLive130.EmptyProd",
        "GetActiveProductionName after stop: {out}"
    );
    assert_eq!(p[8], "<METHOD DOES NOT EXIST>", "GetProductionState: {out}");
    assert!(
        p[9].contains("ErrProductionAlreadyRunning") || p[9].contains("already running"),
        "second start: {out}"
    );
    drop_classes(&c, &client, &["IadLive130.EmptyProd"]).await;
}

// ── R7 ───────────────────────────────────────────────────────────────────────────────────────

const XML_EXPORT: &str = r#"<?xml version="1.0" encoding="UTF8"?>
<Export generator="IRIS" version="26">
<Class name="IadLive130.FromXml">
<Super>%RegisteredObject</Super>
<Method name="Hello">
<ClassMethod>1</ClassMethod>
<ReturnType>%String</ReturnType>
<Implementation><![CDATA[    Quit "hi"
]]></Implementation>
</Method>
</Class>
</Export>"#;

/// R7. Atelier refuses a `.xml` document name with #16006. Put under its `.cls` name, an export whose
/// XML declaration is on its own line is imported, compiles and reads back as UDL. The same export on
/// one line is refused with #16021 in `result.status`, while `status.errors` stays empty.
#[tokio::test]
#[ignore]
async fn xml_export_imports_under_its_cls_name() {
    let Some((c, client)) = conn() else { return };
    drop_classes(&c, &client, &["IadLive130.FromXml"]).await;
    let (status, body) = put_raw(&c, &client, "IadLive130.FromXml.xml", XML_EXPORT).await;
    assert_eq!(status, 400, "a .xml name is refused: {body}");
    assert!(body.contains("16006"), "#16006: {body}");

    let one_line: String = XML_EXPORT.lines().collect::<Vec<_>>().join("");
    let (_, body) = put_raw(&c, &client, "IadLive130.FromXml.cls", &one_line).await;
    let v: serde_json::Value = serde_json::from_str(&body).expect("PUT body is JSON");
    assert!(
        v["result"]["status"]
            .as_str()
            .unwrap_or("")
            .contains("16021"),
        "one-line export: #16021 in result.status: {body}"
    );
    assert_eq!(
        v["status"]["errors"].as_array().map(Vec::len),
        Some(0),
        "and status.errors is empty: {body}"
    );

    let r = put_and_compile(&c, &client, "IadLive130.FromXml.cls", XML_EXPORT).await;
    assert!(r.success(), "multi-line export: {}", compile_text(&r));
    let out = run(
        &c,
        &client,
        " Write \"~[\",##class(IadLive130.FromXml).Hello(),\"]~\"",
    )
    .await;
    assert_eq!(marked(&out), "hi");

    let url = c.versioned_ns_url(NS, "/doc/IadLive130.FromXml.cls");
    let got: serde_json::Value = client
        .get(&url)
        .basic_auth(&c.username, Some(&c.password))
        .send()
        .await
        .expect("GET must reach IRIS")
        .json()
        .await
        .expect("GET body is JSON");
    assert_eq!(
        got["result"]["content"][0].as_str(),
        Some("Class IadLive130.FromXml Extends %RegisteredObject"),
        "reads back as UDL: {got}"
    );
    drop_classes(&c, &client, &["IadLive130.FromXml"]).await;
}

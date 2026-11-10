//! Spec 127: live proof for each corrected skill claim.
//!
//! Every test here shows iris-dev-iris doing what the corrected skill text says. The wording guard
//! for the same item is in `tests/unit/test_skill_facts_127.rs`. Reproductions and the IRIS build
//! they ran on are in `specs/127-skill-fact-fixes/research.md`.
//!
//! Nothing here calls embedded Python. On iris-dev-iris a Python call can segfault the process and
//! leave the routine cache poisoned until IRIS restarts (research.md, item 9).
//!
//! Run with:
//!   IRIS_HOST=localhost IRIS_WEB_PORT=52780 IRIS_USERNAME=_SYSTEM IRIS_PASSWORD=SYS \
//!   cargo test --features testing --test integration test_skill_facts_127_live -- \
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

/// Save a document (`Test127.X.cls`, `ROU127.mac`) by Atelier PUT.
async fn put_doc(c: &IrisConnection, client: &reqwest::Client, doc: &str, src: &str) {
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
    assert!(
        resp.status().is_success(),
        "PUT {doc}: HTTP {}",
        resp.status()
    );
}

async fn compile(c: &IrisConnection, client: &reqwest::Client, doc: &str) -> CompileResult {
    c.compile_document(doc, NS, "cuk", client)
        .await
        .expect("compile request must reach IRIS")
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

/// Cleanup: delete classes with `e`, so no Test127 rows outlive the test.
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

async fn count(c: &IrisConnection, client: &reqwest::Client, table: &str) -> String {
    run(
        c,
        client,
        &format!(
            " Set rs=##class(%SQL.Statement).%ExecDirect(,\"SELECT COUNT(*) AS n FROM {table}\")\n \
             If rs.%SQLCODE<0 {{ Write \"SQLCODE=\",rs.%SQLCODE Quit }}\n \
             Do rs.%Next() Write rs.%Get(\"n\")"
        ),
    )
    .await
}

const EXT: &str = "Class Test127.Ext Extends %Persistent
{

Property Name As %String;

}";

/// US1. Compile and Load with the flags the skill recommends keep the rows. `Delete` with `e`
/// drops the extent, which is what `ShowFlags` says `e` means.
#[tokio::test]
#[ignore]
async fn e_flag_deletes_the_extent_and_recommended_flags_keep_rows() {
    let Some((c, client)) = conn() else { return };
    drop_classes(&c, &client, &["Test127.Ext"]).await;
    put_doc(&c, &client, "Test127.Ext.cls", EXT).await;
    let r = compile(&c, &client, "Test127.Ext.cls").await;
    assert!(r.success(), "Test127.Ext must compile: {:?}", r.errors);

    run(
        &c,
        &client,
        " For i=1:1:3 { Set o=##class(Test127.Ext).%New(), o.Name=i Set sc=o.%Save() If 'sc { Write \"save failed\" Quit } }",
    )
    .await;
    assert_eq!(count(&c, &client, "Test127.Ext").await, "3");

    // Recompile with the recommended flags: rows stay.
    let r = compile(&c, &client, "Test127.Ext.cls").await;
    assert!(r.success(), "recompile: {:?}", r.errors);
    assert_eq!(
        count(&c, &client, "Test127.Ext").await,
        "3",
        "recompile lost rows"
    );

    // Export, delete without `e`, load with `ck`: rows stay.
    let out = run(
        &c,
        &client,
        " Set f=\"/tmp/test127_ext.xml\"\n \
         Set sc=$system.OBJ.Export(\"Test127.Ext.cls\",f,\"-d\") If 'sc { Write \"export failed\" Quit }\n \
         Set sc=$system.OBJ.Delete(\"Test127.Ext\",\"-d\") If 'sc { Write \"delete failed\" Quit }\n \
         Set sc=$system.OBJ.Load(f,\"ck-d\") If 'sc { Write \"load failed\" Quit }\n \
         Write \"ok\"",
    )
    .await;
    assert_eq!(out, "ok");
    assert_eq!(
        count(&c, &client, "Test127.Ext").await,
        "3",
        "Delete without e lost rows"
    );

    // Delete with `e`, load again: the extent is gone.
    let out = run(
        &c,
        &client,
        " Set f=\"/tmp/test127_ext.xml\"\n \
         Set sc=$system.OBJ.Delete(\"Test127.Ext\",\"e-d\") If 'sc { Write \"delete failed\" Quit }\n \
         Set sc=$system.OBJ.Load(f,\"ck-d\") If 'sc { Write \"load failed\" Quit }\n \
         Write \"ok\"",
    )
    .await;
    assert_eq!(out, "ok");
    assert_eq!(
        count(&c, &client, "Test127.Ext").await,
        "0",
        "Delete with e kept rows"
    );

    drop_classes(&c, &client, &["Test127.Ext"]).await;
}

/// All compile output in one string, for asserting on an error number.
fn compile_text(r: &CompileResult) -> String {
    format!("{}\n{}", r.errors.join("\n"), r.console.join("\n"))
}

async fn put_and_compile(
    c: &IrisConnection,
    client: &reqwest::Client,
    doc: &str,
    src: &str,
) -> CompileResult {
    put_doc(c, client, doc, src).await;
    compile(c, client, doc).await
}

const PC: &str = r#"Class Test127.PC
{

ClassMethod Shared() As %Integer
{
    Set arr("a")="",arr("b")="",key="",n=0
    For {
        Set key=$Order(arr(key))  Quit:key=""
        Set n=n+1
    }
    Quit n
}

ClassMethod RetShared() As %Integer
{
    Set arr("a")="",key="",n=0
    For {
        Set key=$Order(arr(key))
        Return n  Quit:key=""
    }
    Quit -1
}

ClassMethod QuitInLoop() As %Integer
{
    For i=1:1:3 {
        Quit 5
    }
    Quit 0
}

ClassMethod NewNs() As %String
{
    New $Namespace
    Set $Namespace="%SYS"
    Quit $Namespace
}

ClassMethod Inner()
{
    TROLLBACK
}

ClassMethod Outer() As %String
{
    TSTART
    Set before=$TLevel
    TSTART
    Do ..Inner()
    Set after=$TLevel
    TROLLBACK:$TLevel
    Quit before_","_after
}

ClassMethod InnerOneLevel()
{
    Set entry=$TLevel
    TSTART
    TROLLBACK:$TLevel>entry 1
}

ClassMethod OuterOneLevel() As %String
{
    TSTART
    Set before=$TLevel
    Do ..InnerOneLevel()
    Set after=$TLevel
    TROLLBACK:$TLevel
    Quit before_","_after
}

ClassMethod Exec() As %String
{
    Set sql="SELECT Name FROM %Dictionary.ClassDefinition WHERE Name = ? OR Name = ?"
    Set stmt=##class(%SQL.Statement).%New()
    Set sc=stmt.%Prepare(sql) If $$$ISERR(sc) Quit "prepare failed"
    Set args=2,args(1)="Test127.PC",args(2)="Test127.Ext"
    Set rs=stmt.%Execute(args...)
    Set n=0 While rs.%Next() { Set n=n+1 }
    Quit n_":"_rs.%SQLCODE
}

}"#;

/// US2.1. A postfix `Quit:key=""` sharing a line compiles and runs. The space form is the one that
/// fails, and with #1054, not #5559.
#[tokio::test]
#[ignore]
async fn postconditional_quit_shares_a_line_and_the_space_form_is_1054() {
    let Some((c, client)) = conn() else { return };
    let r = put_and_compile(&c, &client, "Test127.PC.cls", PC).await;
    assert!(r.success(), "Test127.PC must compile: {}", compile_text(&r));
    assert_eq!(
        run(
            &c,
            &client,
            " Write ##class(Test127.PC).Shared(),\",\",##class(Test127.PC).RetShared()"
        )
        .await,
        "2,0"
    );

    let spaced = r#"Class Test127.PCSpace
{

ClassMethod Spaced() As %Integer
{
    Set arr("a")="",key="",n=0
    For {
        Set key=$Order(arr(key))
        Quit:key = ""
        Set n=n+1
    }
    Quit n
}

}"#;
    let r = put_and_compile(&c, &client, "Test127.PCSpace.cls", spaced).await;
    let text = compile_text(&r);
    assert!(!r.success(), "the spaced postconditional must not compile");
    assert!(text.contains("#1054"), "expected #1054, got: {text}");
    assert!(
        !text.contains("#5559"),
        "the spaced form is not #5559: {text}"
    );
    drop_classes(&c, &client, &["Test127.PC", "Test127.PCSpace"]).await;
}

/// US2.2. `Quit 5` in a `For` block compiles and fails at runtime with `<COMMAND>`. Inside `Try` it
/// is a compile error.
#[tokio::test]
#[ignore]
async fn quit_value_in_loop_is_runtime_command_and_in_try_is_1043() {
    let Some((c, client)) = conn() else { return };
    let r = put_and_compile(&c, &client, "Test127.PC.cls", PC).await;
    assert!(r.success(), "Test127.PC must compile: {}", compile_text(&r));
    let out = run(
        &c,
        &client,
        " Try { Write ##class(Test127.PC).QuitInLoop() } Catch e { Write e.Name }",
    )
    .await;
    assert_eq!(out, "<COMMAND>");

    let qtry = "Class Test127.QTry
{

ClassMethod T() As %Integer
{
    Try {
        Quit 5
    } Catch e { }
    Quit 0
}

}";
    let r = put_and_compile(&c, &client, "Test127.QTry.cls", qtry).await;
    let text = compile_text(&r);
    assert!(
        !r.success(),
        "Quit with a value inside Try must not compile"
    );
    assert!(text.contains("#1043"), "expected #1043, got: {text}");
    drop_classes(&c, &client, &["Test127.PC", "Test127.QTry"]).await;
}

/// US2.3. A `.mac` routine can use `Try/Catch` and `Return`.
#[tokio::test]
#[ignore]
async fn mac_routine_try_catch_and_return_work() {
    let Some((c, client)) = conn() else { return };
    let src = "ROUTINE ROU127
 Quit
Go() PUBLIC {
    Try {
        Set x=1/0
    } Catch e {
        Return \"caught:\"_e.Name
    }
    Return \"none\"
}";
    let r = put_and_compile(&c, &client, "ROU127.mac", src).await;
    assert!(r.success(), "ROU127 must compile: {}", compile_text(&r));
    assert_eq!(
        run(&c, &client, " Write $$Go^ROU127()").await,
        "caught:<DIVIDE>"
    );
    let _ = c
        .execute_via_generator(" Do ##class(%Routine).Delete(\"ROU127.mac\")", NS, &client)
        .await;
}

/// US2.4. Both list forms build the same list; appending with `_$LB()` is the fast one.
#[tokio::test]
#[ignore]
async fn list_concat_is_fast_and_list_star_plus_one_is_slow() {
    let Some((c, client)) = conn() else { return };
    let out = run(
        &c,
        &client,
        " Set n=20000\n \
         Set t=$ZH,a=\"\" For i=1:1:n { Set a=a_$LB(i) } Set ta=$ZH-t\n \
         Set t=$ZH,b=\"\" For i=1:1:n { Set $LIST(b,*+1)=i } Set tb=$ZH-t\n \
         Write (a=b),\",\",$LL(a),\",\",(ta*5<tb)",
    )
    .await;
    assert_eq!(
        out, "1,20000,1",
        "want same list, 20000 items, concat at least 5x faster"
    );
}

/// US2.5. Two-level classes are queried by their dotted name; the underscore form is -30. Deeper
/// packages turn the inner dots into underscores.
#[tokio::test]
#[ignore]
async fn sql_table_names_follow_the_last_dot_rule() {
    let Some((c, client)) = conn() else { return };
    let r = put_and_compile(&c, &client, "Test127.Ext.cls", EXT).await;
    assert!(r.success(), "{}", compile_text(&r));
    let deep = "Class Test127.Sub.Deep Extends %Persistent
{

Property Name As %String;

}";
    let r = put_and_compile(&c, &client, "Test127.Sub.Deep.cls", deep).await;
    assert!(r.success(), "{}", compile_text(&r));

    assert_eq!(count(&c, &client, "Test127.Ext").await, "0");
    assert_eq!(count(&c, &client, "Test127_Ext").await, "SQLCODE=-30");
    assert_eq!(count(&c, &client, "Test127_Sub.Deep").await, "0");
    assert_eq!(count(&c, &client, "Test127.Sub.Deep").await, "SQLCODE=-30");
    drop_classes(&c, &client, &["Test127.Ext", "Test127.Sub.Deep"]).await;
}

/// US2.6. `%Execute(args...)` with an array of values returns rows and raises nothing.
#[tokio::test]
#[ignore]
async fn execute_with_variadic_args_returns_rows() {
    let Some((c, client)) = conn() else { return };
    let r = put_and_compile(&c, &client, "Test127.Ext.cls", EXT).await;
    assert!(r.success(), "{}", compile_text(&r));
    let r = put_and_compile(&c, &client, "Test127.PC.cls", PC).await;
    assert!(r.success(), "{}", compile_text(&r));
    assert_eq!(
        run(&c, &client, " Write ##class(Test127.PC).Exec()").await,
        "2:100"
    );
    drop_classes(&c, &client, &["Test127.PC", "Test127.Ext"]).await;
}

/// US2.7. `New $Namespace` compiles and restores the caller's namespace. `New x` in a procedure
/// block is #1038.
#[tokio::test]
#[ignore]
async fn new_namespace_works_and_new_plain_variable_is_1038() {
    let Some((c, client)) = conn() else { return };
    let r = put_and_compile(&c, &client, "Test127.PC.cls", PC).await;
    assert!(r.success(), "{}", compile_text(&r));
    assert_eq!(
        run(
            &c,
            &client,
            " Write ##class(Test127.PC).NewNs(),\",\",$Namespace"
        )
        .await,
        "%SYS,USER"
    );

    let newx = "Class Test127.NewX
{

ClassMethod T() As %Integer [ ProcedureBlock = 1 ]
{
    New x
    Set x=1
    Quit x
}

}";
    let r = put_and_compile(&c, &client, "Test127.NewX.cls", newx).await;
    let text = compile_text(&r);
    assert!(!r.success(), "New x in a procedure block must not compile");
    assert!(text.contains("#1038"), "expected #1038, got: {text}");
    drop_classes(&c, &client, &["Test127.PC", "Test127.NewX"]).await;
}

/// US2.8. A callee's bare `TROLLBACK` ends the caller's transaction too. Rolling back one level,
/// only when the callee opened one, leaves the caller's level alone.
#[tokio::test]
#[ignore]
async fn bare_trollback_kills_callers_level_and_one_level_does_not() {
    let Some((c, client)) = conn() else { return };
    let r = put_and_compile(&c, &client, "Test127.PC.cls", PC).await;
    assert!(r.success(), "{}", compile_text(&r));
    assert_eq!(
        run(&c, &client, " Write ##class(Test127.PC).Outer()").await,
        "1,0"
    );
    assert_eq!(
        run(&c, &client, " Write ##class(Test127.PC).OuterOneLevel()").await,
        "1,1"
    );
    drop_classes(&c, &client, &["Test127.PC"]).await;
}

const PROD_CLASSES: &[(&str, &str)] = &[
    (
        "Test127.PMsg.cls",
        "Class Test127.PMsg Extends Ens.Request
{

Property N As %Integer;

}",
    ),
    (
        "Test127.SlowOp.cls",
        "Class Test127.SlowOp Extends Ens.BusinessOperation
{

Method OnMessage(pRequest As Test127.PMsg, Output pResponse As Ens.Response) As %Status
{
    Hang 1
    Set ^Test127.Done(pRequest.N) = $Increment(^Test127.Done)
    Quit $$$OK
}

}",
    ),
    (
        "Test127.Svc.cls",
        "Class Test127.Svc Extends Ens.BusinessService
{

Method OnProcessInput(pInput As %RegisteredObject, Output pOutput As %RegisteredObject) As %Status
{
    Quit ..SendRequestAsync(\"Test127.SlowOp\", pInput)
}

}",
    ),
    (
        "Test127.Prod.cls",
        "Class Test127.Prod Extends Ens.Production
{

XData ProductionDefinition
{
<Production Name=\"Test127.Prod\">
  <Item Name=\"Test127.Svc\" ClassName=\"Test127.Svc\" PoolSize=\"0\" Enabled=\"true\"/>
  <Item Name=\"Test127.SlowOp\" ClassName=\"Test127.SlowOp\" PoolSize=\"1\" Enabled=\"true\"/>
</Production>
}

}",
    ),
];

/// Send six one-second messages, wait until one is done, stop with `stop_args`, and report
/// `done,queued` right after the stop. The count has a unary `+` because on 2025.3 `GetCount` returns
/// "" for a stopped production's empty queue, which read as "not a count: 6," in CI.
async fn send_six_then_stop(
    c: &IrisConnection,
    client: &reqwest::Client,
    stop_args: &str,
) -> String {
    run(
        c,
        client,
        &format!(
            " Kill ^Test127.Done\n \
             Set sc=##class(Ens.Director).CreateBusinessService(\"Test127.Svc\",.svc) If 'sc {{ Write \"svc failed\" Quit }}\n \
             For i=1:1:6 {{ Set m=##class(Test127.PMsg).%New(), m.N=i Set sc=svc.ProcessInput(m) }}\n \
             Kill svc\n \
             Hang 1.5\n \
             Set sc=##class(Ens.Director).StopProduction({stop_args}) If 'sc {{ Write \"stop failed\" Quit }}\n \
             Write $Get(^Test127.Done,0),\",\",+##class(Ens.Queue).GetCount(\"Test127.SlowOp\")"
        ),
    )
    .await
}

/// Start the production, give the queue time to drain, and report `runs,distinct,queued`.
async fn start_and_drain(c: &IrisConnection, client: &reqwest::Client) -> String {
    run(
        c,
        client,
        " Set sc=##class(Ens.Director).StartProduction(\"Test127.Prod\") If 'sc { Write \"start failed\" Quit }\n \
         For w=1:1:20 { Quit:##class(Ens.Queue).GetCount(\"Test127.SlowOp\")=0  Hang 0.5 }\n \
         Hang 1.5\n \
         Set k=\"\",d=0 For { Set k=$Order(^Test127.Done(k)) Quit:k=\"\"  Set d=d+1 }\n \
         Write $Get(^Test127.Done,0),\",\",d,\",\",+##class(Ens.Queue).GetCount(\"Test127.SlowOp\")",
    )
    .await
}

fn done_plus_queued(pair: &str) -> i64 {
    pair.split(',')
        .map(|n| {
            n.parse::<i64>()
                .unwrap_or_else(|_| panic!("not a count: {pair}"))
        })
        .sum()
}

/// US2.10. Stopping a production loses nothing. A graceful stop finishes the message in hand and
/// leaves the rest queued; a force stop puts the interrupted message back on the queue, so it runs
/// again after the restart. Every message is delivered once the production starts again.
#[tokio::test]
#[ignore]
async fn production_stop_keeps_queued_messages_and_force_stop_requeues() {
    let Some((c, client)) = conn() else { return };
    // A production an earlier test left troubled (state 3) survives a force stop and refuses the
    // next start; on the 2025.3 CI runner `IadLive130.EmptyProd` did. `CleanProduction` resets it.
    let _ = c
        .execute_via_generator(
            " Do ##class(Ens.Director).StopProduction(10,1)\n Do ##class(Ens.Director).CleanProduction()",
            NS,
            &client,
        )
        .await;
    for (doc, src) in PROD_CLASSES {
        let r = put_and_compile(&c, &client, doc, src).await;
        assert!(r.success(), "{doc}: {}", compile_text(&r));
    }
    let started = run(
        &c,
        &client,
        " Set sc=##class(Ens.Director).StartProduction(\"Test127.Prod\") Write +sc Write:'sc \" \",$System.Status.GetErrorText(sc)",
    )
    .await;
    assert!(
        started.starts_with('1'),
        "Test127.Prod must start: {started}"
    );

    let graceful = send_six_then_stop(&c, &client, "2,0").await;
    assert_eq!(
        done_plus_queued(&graceful),
        6,
        "graceful stop lost messages (done,queued = {graceful})"
    );
    assert_eq!(start_and_drain(&c, &client).await, "6,6,0");

    let forced = send_six_then_stop(&c, &client, "0,1").await;
    assert_eq!(
        done_plus_queued(&forced),
        6,
        "force stop lost messages (done,queued = {forced})"
    );
    assert_eq!(start_and_drain(&c, &client).await, "6,6,0");

    let _ = c
        .execute_via_generator(
            " Do ##class(Ens.Director).StopProduction(10,1) Kill ^Test127.Done",
            NS,
            &client,
        )
        .await;
    drop_classes(
        &c,
        &client,
        &[
            "Test127.Prod",
            "Test127.Svc",
            "Test127.SlowOp",
            "Test127.PMsg",
        ],
    )
    .await;
}

/// US3.2. A hand-written `DataLocation` on an `Ens.Request` subclass is rejected by the compiler
/// with #5477. The skill said runtime and then showed a hand-edited Storage block as the fix.
#[tokio::test]
#[ignore]
async fn ens_request_custom_datalocation_is_5477_at_compile() {
    let Some((c, client)) = conn() else { return };
    let msg = "Class Test127.Msg Extends Ens.Request
{

Property MyField As %String;

Storage Default
{
<Data name=\"MsgDefaultData\">
<Subscript>\"Msg\"</Subscript>
<Value name=\"1\">
<Value>MyField</Value>
</Value>
</Data>
<DataLocation>^Test127.MsgD</DataLocation>
<DefaultData>MsgDefaultData</DefaultData>
<Type>%Storage.Persistent</Type>
}

}";
    let r = put_and_compile(&c, &client, "Test127.Msg.cls", msg).await;
    let text = compile_text(&r);
    assert!(!r.success(), "a custom DataLocation must not compile");
    assert!(text.contains("#5477"), "expected #5477, got: {text}");
    assert!(
        text.contains("^Ens.MessageBodyD"),
        "the error names the required location: {text}"
    );

    // Without a Storage block the compiler writes one, and the class compiles.
    let plain = "Class Test127.Msg Extends Ens.Request
{

Property MyField As %String;

}";
    let r = put_and_compile(&c, &client, "Test127.Msg.cls", plain).await;
    assert!(r.success(), "no Storage block: {}", compile_text(&r));
    drop_classes(&c, &client, &["Test127.Msg"]).await;
}

/// US3.2. There is no 31-character global-name limit to design around: a long class name
/// compiles, IRIS hashes the global name, and rows save.
#[tokio::test]
#[ignore]
async fn long_class_name_compiles_and_saves() {
    let Some((c, client)) = conn() else { return };
    let name = "Test127.VeryLongPackageName.Data.PatientRecordHistory";
    let src = format!("Class {name} Extends %Persistent\n{{\n\nProperty Name As %String;\n\n}}");
    let r = put_and_compile(&c, &client, &format!("{name}.cls"), &src).await;
    assert!(r.success(), "{}", compile_text(&r));
    let out = run(
        &c,
        &client,
        &format!(
            " Set o=##class({name}).%New(), o.Name=\"x\" Set sc=o.%Save() If 'sc {{ Write \"save failed\" Quit }}\n \
             Set loc=$Get(^oddCOM(\"{name}\",\"s\",\"Default\",22))\n \
             Write +sc,\",\",$Select(loc=\"\":\"none\",1:$Length(loc)<=31)"
        ),
    )
    .await;
    assert_eq!(out, "1,1", "save and a short hashed DataLocation");
    drop_classes(&c, &client, &[name]).await;
}

/// 130 FR-019. The class the sql-patterns review ran its probes on. It stays on iris-dev-iris
/// (130 clarifications); the tests put it again so they do not depend on what is there.
const SQLCODE_CLASS: &str = r#"Class Test130.SqlCode Extends %Persistent
{

Property Code As %String;

Property Label As %String;

ClassMethod Seed() As %Status
{
    Set tSC = $$$OK
    Do ..%KillExtent()
    Set o = ..%New(), o.Code = "A", o.Label = "Alpha"
    Set tSC = o.%Save()
    Quit tSC
}

/// Section 3 WRONG form, verbatim shape
ClassMethod Bare(pCode As %String) As %String
{
    Set result = ""
    &sql(SELECT Label INTO :result FROM Test130.SqlCode WHERE Code = :pCode)
    If SQLCODE { Quit "NOT FOUND" }
    Quit result_" (SQLCODE="_SQLCODE_")"
}

/// Section 3 WRONG form against a missing table
ClassMethod Missing(pCode As %String) As %String
{
    Set result = ""
    &sql(SELECT Label INTO :result FROM Test130_SqlCode WHERE Code = :pCode)
    If SQLCODE { Quit "NOT FOUND (SQLCODE="_SQLCODE_")" }
    Quit result
}

/// Section 5 old form: a negative SQLCODE returns "", the same as no row
ClassMethod Swallow(pCode As %String, pDiv As %Integer) As %String
{
    Set tName = "", tRatio = ""
    &sql(SELECT Label, 1/:pDiv INTO :tName, :tRatio FROM Test130.SqlCode WHERE Code = :pCode)
    If SQLCODE = 100 { Quit "" }
    If SQLCODE < 0 { Quit "" }
    Quit tName
}

/// Section 5 corrected form: a negative SQLCODE throws
ClassMethod Surface(pCode As %String, pDiv As %Integer) As %String
{
    Set tName = "", tRatio = ""
    &sql(SELECT Label, 1/:pDiv INTO :tName, :tRatio FROM Test130.SqlCode WHERE Code = :pCode)
    If SQLCODE = 100 { Quit "" }
    If SQLCODE < 0 { Throw ##class(%Exception.SQL).CreateFromSQLCODE(SQLCODE, %msg) }
    Quit tName
}

/// Section 9 WRONG form: COUNT(*) INTO an undefined variable
ClassMethod CountUndefined() As %String
{
    Kill tCount
    &sql(SELECT COUNT(*) INTO :tCount FROM Test130.SqlCode WHERE Code = 'none')
    Quit "SQLCODE="_SQLCODE_" defined="_$Data(tCount)_" value=["_$Get(tCount,"<undef>")_"]"
}

/// Section 9 on a no-row SELECT: is the INTO variable touched?
ClassMethod NoRowInto() As %String
{
    Set tLabel = "untouched"
    &sql(SELECT Label INTO :tLabel FROM Test130.SqlCode WHERE Code = 'none')
    Quit "SQLCODE="_SQLCODE_" value=["_tLabel_"]"
}

/// -114 probe, background half: hold row 1's lock until told to let go
ClassMethod HoldLock()
{
    Lock +^Test130.SqlCodeD(1)
    Set ^Test130Lk("held") = 1
    For i = 1:1:300 { Quit:$Get(^Test130Lk("release"))  Hang 0.1 }
    Lock -^Test130.SqlCodeD(1)
    Kill ^Test130Lk("held")
}

/// -114 probe, reading half: a READ COMMITTED read of the locked row
ClassMethod ReadLocked(pCode As %String) As %String
{
    Do $SYSTEM.SQL.Util.SetOption("ProcessLockTimeout", 1)
    &sql(SET TRANSACTION ISOLATION LEVEL READ COMMITTED)
    Set tLabel = "untouched"
    &sql(SELECT Label INTO :tLabel FROM Test130.SqlCode WHERE Code = :pCode)
    Quit "SQLCODE="_SQLCODE_" value=["_tLabel_"]"
}

}"#;

async fn sqlcode_class(c: &IrisConnection, client: &reqwest::Client) {
    put_doc(c, client, "Test130.SqlCode.cls", SQLCODE_CLASS).await;
    let r = compile(c, client, "Test130.SqlCode.cls").await;
    assert!(r.success(), "Test130.SqlCode must compile: {:?}", r.errors);
    let seeded = run(
        c,
        client,
        " Write $System.Status.IsOK(##class(Test130.SqlCode).Seed())",
    )
    .await;
    assert_eq!(seeded, "1", "Test130.SqlCode.Seed failed");
}

/// sql-3. `If SQLCODE` is false on 0, so a found row comes back. It is true on 100 and on -30,
/// so a missing table reads as "NOT FOUND".
#[tokio::test]
#[ignore]
async fn if_sqlcode_lumps_no_row_with_an_error() {
    let Some((c, client)) = conn() else { return };
    sqlcode_class(&c, &client).await;
    let found = run(&c, &client, " Write ##class(Test130.SqlCode).Bare(\"A\")").await;
    assert_eq!(found, "Alpha (SQLCODE=0)", "a found row must come back");
    let none = run(&c, &client, " Write ##class(Test130.SqlCode).Bare(\"Z\")").await;
    assert_eq!(none, "NOT FOUND");
    let missing = run(
        &c,
        &client,
        " Write ##class(Test130.SqlCode).Missing(\"A\")",
    )
    .await;
    assert_eq!(
        missing, "NOT FOUND (SQLCODE=-30)",
        "a missing table must read as not found under `If SQLCODE`"
    );
}

/// sql-5. `If SQLCODE < 0 { Quit "" }` turns a -400 into the no-row answer. Throwing from
/// `CreateFromSQLCODE` keeps the code.
#[tokio::test]
#[ignore]
async fn a_negative_sqlcode_is_surfaced_not_returned_empty() {
    let Some((c, client)) = conn() else { return };
    sqlcode_class(&c, &client).await;
    let swallowed = run(
        &c,
        &client,
        " Write \"[\",##class(Test130.SqlCode).Swallow(\"A\",0),\"]\"",
    )
    .await;
    let no_row = run(
        &c,
        &client,
        " Write \"[\",##class(Test130.SqlCode).Swallow(\"Z\",1),\"]\"",
    )
    .await;
    assert_eq!(swallowed, "[]", "the old form returns \"\" on -400");
    assert_eq!(swallowed, no_row, "and that is the no-row answer");

    let surfaced = run(
        &c,
        &client,
        " Try { Write ##class(Test130.SqlCode).Surface(\"A\",0) } \
         Catch e { Write \"caught \",e.Code }",
    )
    .await;
    assert_eq!(
        surfaced, "caught -400",
        "the corrected form must throw with the SQLCODE"
    );
    let ok = run(
        &c,
        &client,
        " Write ##class(Test130.SqlCode).Surface(\"A\",1)",
    )
    .await;
    assert_eq!(ok, "Alpha");
}

/// sql-9. `COUNT(*) INTO` with an undefined variable sets it to 0. A no-row SELECT sets the INTO
/// variable to "" rather than leaving it alone.
#[tokio::test]
#[ignore]
async fn count_into_sets_zero_and_no_row_sets_empty() {
    let Some((c, client)) = conn() else { return };
    sqlcode_class(&c, &client).await;
    let count = run(
        &c,
        &client,
        " Write ##class(Test130.SqlCode).CountUndefined()",
    )
    .await;
    assert_eq!(count, "SQLCODE=0 defined=1 value=[0]");
    let no_row = run(&c, &client, " Write ##class(Test130.SqlCode).NoRowInto()").await;
    assert_eq!(no_row, "SQLCODE=100 value=[]");
}

/// sql-114. Under READ COMMITTED a read of a row another process holds times out with -114. The
/// timeout and isolation level are set per process, so nothing instance-wide changes.
#[tokio::test]
#[ignore]
async fn a_read_committed_read_of_a_locked_row_is_minus_114() {
    let Some((c, client)) = conn() else { return };
    sqlcode_class(&c, &client).await;
    run(
        &c,
        &client,
        " Kill ^Test130Lk Job ##class(Test130.SqlCode).HoldLock() \
         For i=1:1:50 { Quit:$Get(^Test130Lk(\"held\"))  Hang 0.1 } Write $Get(^Test130Lk(\"held\"))",
    )
    .await;
    let read = run(
        &c,
        &client,
        " Write ##class(Test130.SqlCode).ReadLocked(\"A\")",
    )
    .await;
    run(
        &c,
        &client,
        " Set ^Test130Lk(\"release\")=1 For i=1:1:50 { Quit:'$Data(^Test130Lk(\"held\"))  Hang 0.1 } \
         Kill ^Test130Lk Write 1",
    )
    .await;
    // The INTO variable is set in some runs and empty in others (empty in three full-suite runs,
    // set in every run on its own), so the skill promises neither. It is never left untouched.
    assert!(
        read == "SQLCODE=-114 value=[Alpha]" || read == "SQLCODE=-114 value=[]",
        "expected -114 with the row or an empty value, got {read}"
    );
    let unlocked = run(
        &c,
        &client,
        " Write ##class(Test130.SqlCode).ReadLocked(\"A\")",
    )
    .await;
    assert_eq!(unlocked, "SQLCODE=0 value=[Alpha]");
}

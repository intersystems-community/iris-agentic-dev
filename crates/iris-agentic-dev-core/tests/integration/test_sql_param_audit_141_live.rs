//! Spec 141 T092 (T-R4b-L02): does IRIS's SQL statement audit record a bound parameter value?
//!
//! The answer decides how `execute_via_generator_with_args` hands a password to IRIS. If a
//! statement audit event holds the parameter, a SQL `?` is no better than splicing the password
//! into class source, and the value has to travel in a request header instead.
//!
//! **Verdict (2026-10-23, `intersystemsdc/iris-community:2026.2`): the audit records it.** With
//! the `%System/%SQL` events on, a value bound to `?` in `SELECT <SqlProc>(?)` appears in a
//! `%SYS.Audit` row, so a SQL parameter keeps a password out of class source and puts it in the
//! audit database instead. `execute_via_generator_with_args` therefore sends the arguments in the
//! `X-IAD-Args` request header. This test pins both halves:
//!
//! - a control with a **literal** in the statement text, and a bound parameter, are both recorded,
//!   so an audit that never fired cannot make the header look safe;
//! - the same call with the value in `X-IAD-Args`, read by the SqlProc from
//!   `%request.CgiEnvs("HTTP_X_IAD_ARGS")`, reaches the proc and leaves no audit row.
//!
//! Run, serialised behind the shared lock:
//!
//! ```text
//! lockf -k /tmp/iad-141-iris.lock cargo test --features testing --test integration \
//!     sql_param_audit_141 -- --ignored --test-threads=1
//! ```

use iris_agentic_dev_core::testing::ThrowawayIris;

const SENTINEL: &str = "Zq9-SENTINEL-pw";
const HEADER_SENTINEL: &str = "Zq9-SENTINEL-hdr";
const CONTROL: &str = "Zq9-CONTROL-literal";
const CLASS: &str = "IrisDevTmp.T092Probe";

fn atelier(iris: &ThrowawayIris, path: &str) -> String {
    format!(
        "http://{}:{}/api/atelier/v1/USER{}",
        iris.host, iris.web_port, path
    )
}

#[tokio::test]
#[ignore]
async fn sql_parameter_is_audited_and_the_header_is_not() {
    let Some(iris) = ThrowawayIris::start() else {
        return;
    };
    let client = reqwest::Client::new();

    // 1. Every `%System/%SQL` event on (Dynamic, XDBC and Embedded statements, DDL, DML, query,
    //    utility): which leaf event the Atelier query endpoint raises depends on the statement.
    let enable = iris.session(
        "%SYS",
        "Set s(\"AuditEnabled\")=1\n\
         Write \"SYS:\",##class(Security.System).Modify(\"SYSTEM\",.s),!\n\
         Set rs=##class(%SQL.Statement).%ExecDirect(,\"SELECT Name FROM Security.Events WHERE Source='%System' AND Type='%SQL'\")\n\
         Set n=0,bad=0\n\
         While rs.%Next() { Set p(\"Enabled\")=1 Set:##class(Security.Events).Modify(\"%System\",\"%SQL\",rs.%GetData(1),.p)'=1 bad=bad+1 Set n=n+1 }\n\
         Write \"EVENTS:\",n,\" BAD:\",bad,!",
    );
    assert!(
        enable.contains("SYS:1") && enable.contains("BAD:0") && !enable.contains("EVENTS:0 "),
        "audit was not enabled: {enable}"
    );

    // 2. A generator-shaped class: one SqlProc taking a long string, as D2 specifies.
    let class = [
        format!("Class {CLASS} [ Final ]"),
        "{".into(),
        "ClassMethod Execute(pArgs As %String(MAXLEN=\"\") = \"\") As %String [ SqlProc ]".into(),
        "{".into(),
        "  Quit \"got:\"_$Length(pArgs)".into(),
        "}".into(),
        "ClassMethod FromHeader() As %String [ SqlProc ]".into(),
        "{".into(),
        "  Set tRaw = $Get(%request.CgiEnvs(\"HTTP_X_IAD_ARGS\"))".into(),
        "  Set tArgs = [].%FromJSON($System.Encryption.Base64Decode(tRaw))".into(),
        "  Quit \"got:\"_$Length(tArgs.%Get(0))".into(),
        "}".into(),
        "}".into(),
    ];
    let put = client
        .put(atelier(&iris, &format!("/doc/{CLASS}.cls")))
        .basic_auth(iris.username, Some(iris.password))
        .json(&serde_json::json!({"enc": false, "content": class}))
        .send()
        .await
        .expect("PUT doc");
    assert!(put.status().is_success(), "PUT doc: {}", put.status());
    let compile = client
        .post(atelier(&iris, "/action/compile?flags=cuk"))
        .basic_auth(iris.username, Some(iris.password))
        .json(&serde_json::json!([format!("{CLASS}.cls")]))
        .send()
        .await
        .expect("compile");
    assert!(
        compile.status().is_success(),
        "compile: {}",
        compile.status()
    );

    // 3. The control (literal in the text) and the statement under test (bound parameter).
    let query = |body: serde_json::Value| {
        let client = client.clone();
        let url = atelier(&iris, "/action/query");
        async move {
            let resp = client
                .post(url)
                .basic_auth("_SYSTEM", Some("SYS"))
                .json(&body)
                .send()
                .await
                .expect("query");
            assert!(resp.status().is_success(), "query: {}", resp.status());
            resp.text().await.unwrap()
        }
    };
    let control = query(serde_json::json!({
        "query": format!("SELECT '{CONTROL}' AS result")
    }))
    .await;
    assert!(control.contains(CONTROL), "control query failed: {control}");
    let bound = query(serde_json::json!({
        "query": format!("SELECT {CLASS}_Execute(?) AS result"),
        "parameters": [SENTINEL]
    }))
    .await;
    assert!(
        bound.contains(&format!("got:{}", SENTINEL.len())),
        "the SqlProc did not receive the parameter: {bound}"
    );

    // The same call with the value in the header, as a base64 JSON array.
    let header_value = {
        use base64::Engine as _;
        base64::engine::general_purpose::STANDARD
            .encode(serde_json::to_vec(&[HEADER_SENTINEL]).unwrap())
    };
    let resp = client
        .post(atelier(&iris, "/action/query"))
        .basic_auth(iris.username, Some(iris.password))
        .header("X-IAD-Args", &header_value)
        .json(&serde_json::json!({"query": format!("SELECT {CLASS}_FromHeader() AS result")}))
        .send()
        .await
        .expect("header query");
    let via_header = resp.text().await.unwrap();
    assert!(
        via_header.contains(&format!("got:{}", HEADER_SENTINEL.len())),
        "the SqlProc did not read X-IAD-Args: {via_header}"
    );

    // 4. Count audit rows that hold each value, anywhere in the row.
    // One line per statement: `iris session` runs each input line on its own, so a loop body
    // split across lines would not be a loop body.
    let scan = iris.session(
        "%SYS",
        &format!(
            "Set (nc,np,nh,nb,nrows)=0 Set rs=##class(%SQL.Statement).%ExecDirect(,\"SELECT * FROM %SYS.Audit\") \
             While rs.%Next() {{ Set nrows=nrows+1,row=\"\" For i=1:1:rs.%ResultColumnCount {{ Set row=row_\"|\"_rs.%GetData(i) }} \
             Set:row[\"{CONTROL}\" nc=nc+1 Set:row[\"{SENTINEL}\" np=np+1 Set:row[\"{HEADER_SENTINEL}\" nh=nh+1 Set:row[\"{header_value}\" nb=nb+1 }} \
             Write \"AUDIT rows=\",nrows,\" control=\",nc,\" param=\",np,\" header=\",nh,\" b64=\",nb,!"
        ),
    );
    let line = scan
        .lines()
        .find(|l| l.starts_with("AUDIT "))
        .unwrap_or_else(|| panic!("no audit scan result: {scan}"));
    let count = |key: &str| -> u32 {
        line.split(&format!("{key}="))
            .nth(1)
            .and_then(|t| t.split_whitespace().next())
            .and_then(|n| n.parse().ok())
            .unwrap_or_else(|| panic!("cannot read {key} from {line:?}"))
    };
    assert!(
        count("control") >= 1,
        "statement auditing recorded no literal, so it proves nothing about parameters: {line}"
    );
    assert!(
        count("param") >= 1,
        "the bound parameter is no longer audited; X-IAD-Args may be unnecessary, re-decide D2: {line}"
    );
    assert_eq!(
        count("header"),
        0,
        "the X-IAD-Args value reached %SYS.Audit: {line}"
    );
    assert_eq!(
        count("b64"),
        0,
        "the encoded X-IAD-Args value reached %SYS.Audit: {line}"
    );
}

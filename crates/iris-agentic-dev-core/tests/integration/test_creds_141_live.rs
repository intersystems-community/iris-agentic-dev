//! Spec 141 FR-020, live (T-R4b-L01 and L03): a password never reaches IRIS storage as source.
//!
//! On the throwaway container a stub source-control class records what IRIS hands its constructor
//! and every document it is asked to save. The connection account's password is a sentinel. The
//! tools that used to splice a password into a generated class (source-control status, `iris_doc`
//! put, `iris_production_diff`, `iris_credential_manage`, `iris_admin` create user and clear-flag)
//! and a generator call forced to fail all run as that account. Afterwards the sentinels are
//! absent from `^IadR4bSeen`, `^ERRORS` and the journal, the stub received an empty password, and
//! the created credential and users work.
//!
//! The journal is the strong check: a password spliced into class source is journaled as a SET of
//! `^oddDEF`. A control generator call with a literal in its source proves the journal search
//! finds source text at all.
//!
//! Run, serialised behind the shared lock:
//!
//! ```text
//! lockf -k /tmp/iad-141-iris.lock cargo test --features testing --test integration \
//!     creds_141 -- --ignored --test-threads=1
//! ```

use iris_agentic_dev_core::elicitation::{CheckoutCache, ElicitationStore};
use iris_agentic_dev_core::iris::connection::{
    DiscoverySource, GeneratorCall, IrisConnection, Secret,
};
use iris_agentic_dev_core::testing::EnvGuard;
use iris_agentic_dev_core::testing::ThrowawayIris;
use iris_agentic_dev_core::tools::admin::{
    admin_clear_password_change_flag_impl, admin_create_user_impl,
};
use iris_agentic_dev_core::tools::interop::{
    handle_iris_production_diff, interop_credential_manage_impl, CredentialManageParams,
    ProductionDiffParams,
};
use iris_agentic_dev_core::tools::scm::{handle_iris_source_control, ScmParams};
use iris_agentic_dev_core::tools::{IrisTools, Toolset};
use serde_json::{json, Value};

const ACCOUNT: &str = "iad141acct";
const ACCOUNT_PW: &str = "Zq9-SENTINEL-pw";
const CRED_PW: &str = "Zq9-SENTINEL-cred";
const NEW_USER_PW: &str = "Zq9-SENTINEL-newuser";
const FLAG_OLD_PW: &str = "Zq9-SENTINEL-old";
const FLAG_NEW_PW: &str = "Zq9-SENTINEL-new";
const CONTROL: &str = "Zq9-CONTROL-literal";

/// A source-control class that records the login IRIS makes on it and which documents it is asked to
/// save. It never records a password, only its length.
const STUB_SC: &[&str] = &[
    "Class Iad.R4bSC Extends %Studio.Extension.Base",
    "{",
    "Method Login(Name As %String, Password As %String) As %Status",
    "{",
    "  Set n = $Increment(^IadR4bSeen(\"create\"))",
    "  Set ^IadR4bSeen(\"create\", n) = $ListBuild($Get(Name), $Length($Get(Password)))",
    "  Quit $$$OK",
    "}",
    "Method GetStatus(InternalName As %String, ByRef IsInSourceControl As %Boolean, ByRef Editable As %Boolean, ByRef IsCheckedOut As %Boolean, ByRef UserCheckedOut As %String) As %Status",
    "{",
    "  Set IsInSourceControl = 1, Editable = 1, IsCheckedOut = 1, UserCheckedOut = ..Username",
    "  Quit $$$OK",
    "}",
    "Method OnAfterSave(InternalName As %String, Object As %RegisteredObject = {$$$NULLOREF}) As %Status",
    "{",
    "  Set n = $Increment(^IadR4bSeen(\"saved\"))",
    "  Set ^IadR4bSeen(\"saved\", n) = InternalName",
    "  Set tText = \"\"",
    "  Try { Set tSC = ##class(%Compiler.UDL.TextServices).GetTextAsString($Namespace, $Piece(InternalName, \".\", 1, *-1), .tText) } Catch e { Set tText = \"\" }",
    "  Set ^IadR4bSeen(\"source\", n) = tText",
    "  Quit $$$OK",
    "}",
    "}",
];

fn url(iris: &ThrowawayIris) -> String {
    format!("http://{}:{}", iris.host, iris.web_port)
}

fn conn(iris: &ThrowawayIris, user: &str, pw: &str) -> IrisConnection {
    IrisConnection::new(url(iris), "USER", user, pw, DiscoverySource::EnvVar)
}

fn body<E: std::fmt::Debug>(r: Result<rmcp::model::CallToolResult, E>) -> Value {
    let text = r.unwrap().content[0].as_text().unwrap().text.clone();
    serde_json::from_str(&text).unwrap_or_else(|e| panic!("not JSON ({e}): {text}"))
}

/// Put, compile and install the stub as `namespace`'s source-control class.
async fn install_stub(iris: &ThrowawayIris, client: &reqwest::Client) {
    let put = client
        .put(format!(
            "{}/api/atelier/v1/USER/doc/Iad.R4bSC.cls",
            url(iris)
        ))
        .basic_auth(iris.username, Some(iris.password))
        .json(&json!({"enc": false, "content": STUB_SC}))
        .send()
        .await
        .expect("PUT stub");
    assert!(put.status().is_success(), "PUT stub: {}", put.status());
    let compile = client
        .post(format!(
            "{}/api/atelier/v1/USER/action/compile?flags=cuk",
            url(iris)
        ))
        .basic_auth(iris.username, Some(iris.password))
        .json(&json!(["Iad.R4bSC.cls"]))
        .send()
        .await
        .expect("compile stub");
    let result: Value = compile.json().await.unwrap();
    assert!(
        result["status"]["errors"]
            .as_array()
            .is_some_and(|e| e.is_empty()),
        "stub did not compile: {result}"
    );
    let set = iris.session(
        "USER",
        "Write \"SET:\",##class(%Studio.SourceControl.Interface).SourceControlClassSet(\"Iad.R4bSC\",\"USER\"),!",
    );
    assert!(set.contains("SET:1"), "stub not installed: {set}");
}

/// How many nodes of `global` (subscripts or values) contain `needle`, in `ns`.
fn global_hits(iris: &ThrowawayIris, ns: &str, global: &str, needle: &str) -> u32 {
    let global = global.replace('"', "\"\"");
    let out = iris.session(
        ns,
        &format!(
            "Set (n,hit)=0 Set r=\"{global}\" For  Set r=$Query(@r) Quit:r=\"\"  Set n=n+1 Set:((r_\"|\"_$Get(@r))[\"{needle}\") hit=hit+1\nWrite \"HITS:\",hit,\" NODES:\",n,!"
        ),
    );
    out.lines()
        .find_map(|l| l.strip_prefix("HITS:"))
        .and_then(|l| l.split_whitespace().next())
        .and_then(|n| n.parse().ok())
        .unwrap_or_else(|| panic!("cannot read {global} scan: {out}"))
}

/// Global references of the journal records that hold `needle`, leaving out those of
/// `allowed_globals` (the needle's own destination, if it has one).
///
/// Searched from the container's terminal, not through `journal_search`: that tool splices its
/// search term into the generator class it runs, so searching for a sentinel journals the sentinel
/// and every search would find the previous one.
fn journal_refs(iris: &ThrowawayIris, needle: &str, allowed_globals: &[&str]) -> Vec<String> {
    let out = iris.session(
        "%SYS",
        &format!(
            "Set f=##class(%SYS.Journal.System).GetCurrentFile() Set first=f.FirstRecordGet() Set rs=##class(%ResultSet).%New(\"%SYS.Journal.File:Search\") Set sc=rs.Execute(\"{needle}\",f.Name,first.Address,0) Write \"EXEC:\",sc,!\n\
             While rs.Next() {{ Set rec=f.GetRecordAt(rs.GetData(1)) Set g=\"\" Set:rec.%IsA(\"%SYS.Journal.SetKillRecord\") g=rec.GlobalReference Write \"REC|\",g,! }}"
        ),
    );
    assert!(out.contains("EXEC:1"), "journal search did not run: {out}");
    out.lines()
        .filter_map(|l| l.strip_prefix("REC|"))
        .filter(|g| !allowed_globals.iter().any(|a| g.contains(a)))
        .map(str::to_string)
        .collect()
}

fn scm_params(v: Value) -> ScmParams {
    serde_json::from_value(v).expect("ScmParams")
}

#[tokio::test]
#[ignore]
async fn no_sentinel_reaches_storage_and_the_created_credential_and_users_work() {
    let mut env_guard = EnvGuard::new();
    let Some(iris) = ThrowawayIris::start() else {
        return;
    };
    env_guard.set("IRIS_ADMIN_TOOLS", "1");
    let client = reqwest::Client::new();
    let sys = conn(&iris, iris.username, iris.password);
    install_stub(&iris, &client).await;

    // The connection account: its password is the sentinel, and it can do everything.
    let made = body(
        admin_create_user_impl(
            Some(&sys),
            ACCOUNT,
            ACCOUNT_PW,
            Some("L01 account"),
            Some("%All"),
        )
        .await,
    );
    assert_eq!(made["success"], true, "account not created: {made}");
    let acct = conn(&iris, ACCOUNT, ACCOUNT_PW);
    acct.query("SELECT 1 AS x", vec![], "USER", &client)
        .await
        .expect("the account the sentinel belongs to must be able to connect");

    // Control: a literal in generator source is journaled, so the journal search can see source.
    let control = acct
        .execute_via_generator(&format!("Write \"{CONTROL}\""), "USER", &client)
        .await
        .unwrap();
    assert!(control.contains(CONTROL), "{control}");

    // Source-control status and menu: the old builders spliced the account's password.
    let store = ElicitationStore::new();
    let cache = CheckoutCache::new();
    let status = body(
        handle_iris_source_control(
            &acct,
            &client,
            scm_params(
                json!({"action": "status", "document": "Iad141.Probe.cls", "namespace": "USER"}),
            ),
            &store,
            &cache,
        )
        .await,
    );
    assert_eq!(status["success"], true, "status: {status}");

    // `iris_doc` put runs the pre-write source-control probe.
    let p = json!({
        "mode": "put", "name": "Iad141.Probe.cls", "namespace": "USER",
        "content": "Class Iad141.Probe\n{\nClassMethod Run() As %String\n{\n  Quit 1\n}\n}\n"
    });
    let tools = IrisTools::for_test_gates_open(Some(acct.clone()), Toolset::Merged).unwrap();
    let put = body(tools.call_gated("iris_doc", p).await);
    assert_eq!(put["success"], true, "doc put: {put}");

    // Production diff builds its own source-control probe. The production need not exist: the
    // probe is generated and run before that is known.
    let diff_params: ProductionDiffParams =
        serde_json::from_value(json!({"production": "Iad141.NoProd", "namespace": "USER"}))
            .unwrap();
    let diff = body(handle_iris_production_diff(Some(&acct), &diff_params).await);
    assert!(
        diff["error_code"] == "NO_SCM"
            || diff["success"] == true
            || diff["error_code"] == "PRODUCTION_NOT_FOUND",
        "production diff did not reach its source-control probe: {diff}"
    );

    // Credential create: the password goes in the header, and the stored credential is right.
    let cred_params: CredentialManageParams = serde_json::from_value(json!({
        "action": "create", "id": "iad141cred", "username": "svc", "password": CRED_PW,
        "namespace": "USER"
    }))
    .unwrap();
    let cred = body(interop_credential_manage_impl(Some(&acct), cred_params).await);
    assert_eq!(cred["success"], true, "credential create: {cred}");
    let stored = acct
        .execute_via_generator(
            "Write ##class(Ens.Config.Credentials).GetValue(\"iad141cred\",\"Password\")",
            "USER",
            &client,
        )
        .await
        .unwrap();
    assert_eq!(
        stored.trim(),
        CRED_PW,
        "the credential must hold the password"
    );

    // Admin create user: the new user can connect.
    let newu = body(
        admin_create_user_impl(Some(&acct), "iad141new", NEW_USER_PW, None, Some("%All")).await,
    );
    assert_eq!(newu["success"], true, "create user: {newu}");
    conn(&iris, "iad141new", NEW_USER_PW)
        .query("SELECT 1 AS x", vec![], "USER", &client)
        .await
        .expect("the user created with a header-borne password must be able to connect");

    // Admin clear flag: old and new password both travel in the header.
    let flagged = iris.session(
        "%SYS",
        &format!(
            "Set p(\"Password\")=\"{FLAG_OLD_PW}\",p(\"ChangePassword\")=1,p(\"Roles\")=\"%All\"\nWrite \"MADE:\",##class(Security.Users).Create(\"iad141flag\",.p),!"
        ),
    );
    assert!(flagged.contains("MADE:1"), "{flagged}");
    let cleared = body(
        admin_clear_password_change_flag_impl(Some(&acct), "iad141flag", FLAG_OLD_PW, FLAG_NEW_PW)
            .await,
    );
    assert_eq!(cleared["success"], true, "clear flag: {cleared}");
    conn(&iris, "iad141flag", FLAG_NEW_PW)
        .query("SELECT 1 AS x", vec![], "USER", &client)
        .await
        .expect("the user whose flag was cleared must connect with the new password");

    // A generator call forced to fail, with an argument in flight.
    let failing = GeneratorCall {
        code: "Set tP=tIadArgs.%Get(0)\nSet x=1/0".into(),
        args: vec![Secret::new(CRED_PW)],
    };
    let failed = acct
        .execute_via_generator_with_args(&failing, "USER", &client)
        .await
        .unwrap();
    assert!(
        failed.contains("ERROR"),
        "the call must have failed: {failed}"
    );
    assert!(
        !failed.contains(CRED_PW),
        "the error text leaked the argument: {failed}"
    );

    // The source-control stub saw the account with an empty password, every time.
    let creates = iris.session(
        "USER",
        "Set (n,bad,user)=0 Set r=\"^IadR4bSeen(\"\"create\"\")\" Set k=\"\" For  Set k=$Order(^IadR4bSeen(\"create\",k),1,v) Quit:k=\"\"  Set n=n+1 Set:$ListGet(v,2)'=0 bad=bad+1 Set:$ListGet(v,1)=\"iad141acct\" user=user+1\nWrite \"CREATES:\",n,\" BADPW:\",bad,\" ACCT:\",user,!",
    );
    let field = |key: &str| -> u32 {
        creates
            .split(key)
            .nth(1)
            .and_then(|t| t.split_whitespace().next())
            .and_then(|n| n.parse().ok())
            .unwrap_or_else(|| panic!("cannot read {key} from {creates}"))
    };
    assert!(
        field("CREATES:") >= 3,
        "the hook was created too few times: {creates}"
    );
    assert_eq!(
        field("BADPW:"),
        0,
        "source control received a password: {creates}"
    );
    assert!(
        field("ACCT:") >= 3,
        "source control did not get the account name: {creates}"
    );

    // The hook saw class source (scratch classes and the document), so its copy could have held a
    // password and does not.
    let saw_source = global_hits(&iris, "USER", "^IadR4bSeen(\"source\")", "ClassMethod");
    assert!(
        saw_source >= 1,
        "the stub never read any source: nothing was checked"
    );

    // Nothing the account's password or the other sentinels touched leaves a trace.
    for needle in [ACCOUNT_PW, CRED_PW, NEW_USER_PW, FLAG_OLD_PW, FLAG_NEW_PW] {
        assert_eq!(
            global_hits(&iris, "USER", "^IadR4bSeen", needle),
            0,
            "{needle} in ^IadR4bSeen"
        );
        for ns in ["USER", "%SYS"] {
            assert_eq!(
                global_hits(&iris, ns, "^ERRORS", needle),
                0,
                "{needle} in ^ERRORS ({ns})"
            );
        }
    }
    std::thread::sleep(std::time::Duration::from_secs(3));
    assert!(
        !journal_refs(&iris, CONTROL, &[]).is_empty(),
        "the journal search cannot see generator source, so its silence proves nothing"
    );
    for needle in [ACCOUNT_PW, NEW_USER_PW, FLAG_OLD_PW, FLAG_NEW_PW] {
        let hits = journal_refs(&iris, needle, &[]);
        assert!(hits.is_empty(), "{needle} in the journal: {hits:?}");
    }
    // The credential's password is stored by design; it must be in no other record.
    let hits = journal_refs(
        &iris,
        CRED_PW,
        &["Ens.Config.Credentials", "Ens.SecondaryData.Password"],
    );
    assert!(
        hits.is_empty(),
        "{CRED_PW} journaled somewhere other than the credential: {hits:?}"
    );
}

#[tokio::test]
#[ignore]
async fn source_control_still_works_with_an_empty_password() {
    let Some(iris) = ThrowawayIris::start() else {
        return;
    };
    let client = reqwest::Client::new();
    let acct = conn(&iris, iris.username, iris.password);
    install_stub(&iris, &client).await;
    let store = ElicitationStore::new();
    let cache = CheckoutCache::new();

    for action in ["status", "menu"] {
        let out = body(
            handle_iris_source_control(
                &acct,
                &client,
                scm_params(
                    json!({"action": action, "document": "Iad141.Probe.cls", "namespace": "USER"}),
                ),
                &store,
                &cache,
            )
            .await,
        );
        assert_eq!(out["success"], true, "{action}: {out}");
        assert_ne!(out["error_code"], "SCM_UNAVAILABLE", "{action}: {out}");
    }

    // A write into the namespace triggers the hook's save callback, as before.
    let p = json!({
        "mode": "put", "name": "Iad141.Hooked.cls", "namespace": "USER",
        "content": "Class Iad141.Hooked\n{\n}\n"
    });
    let tools = IrisTools::for_test_gates_open(Some(acct.clone()), Toolset::Merged).unwrap();
    let put = body(tools.call_gated("iris_doc", p).await);
    assert_eq!(put["success"], true, "doc put: {put}");
    assert!(
        global_hits(&iris, "USER", "^IadR4bSeen(\"saved\")", "Iad141.Hooked") >= 1,
        "the hook's OnAfterSave never saw the document"
    );

    // And it was created for the session's user with no password.
    let creates = iris.session(
        "USER",
        "Set (n,bad)=0 Set k=\"\" For  Set k=$Order(^IadR4bSeen(\"create\",k),1,v) Quit:k=\"\"  Set n=n+1 Set:($ListGet(v,2)'=0)||($ListGet(v,1)'=\"_SYSTEM\") bad=bad+1\nWrite \"CREATES:\",n,\" BAD:\",bad,!",
    );
    assert!(creates.contains("BAD:0"), "{creates}");
    assert!(
        !creates.contains("CREATES:0 "),
        "the hook was never created: {creates}"
    );
}

// ── T-R4b-L04: identification on real instances ─────────────────────────────────────────────

/// A TCP relay to `target` that keeps every byte the client sent, so a test can see what a client
/// said to a real server.
fn relay_to(target: String) -> (u16, std::sync::Arc<std::sync::Mutex<Vec<u8>>>) {
    use std::io::{Read, Write};
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind relay");
    let port = listener.local_addr().unwrap().port();
    let sent = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let log = sent.clone();
    std::thread::spawn(move || {
        for client in listener.incoming().flatten() {
            let log = log.clone();
            let target = target.clone();
            std::thread::spawn(move || {
                let Ok(server) = std::net::TcpStream::connect(target.as_str()) else {
                    return;
                };
                let (mut c_read, mut s_write) =
                    (client.try_clone().unwrap(), server.try_clone().unwrap());
                let (mut s_read, mut c_write) = (server.try_clone().unwrap(), client);
                std::thread::spawn(move || {
                    let mut buf = [0u8; 4096];
                    while let Ok(n) = c_read.read(&mut buf) {
                        if n == 0 || s_write.write_all(&buf[..n]).is_err() {
                            break;
                        }
                        log.lock().unwrap().extend_from_slice(&buf[..n]);
                    }
                });
                let mut buf = [0u8; 4096];
                while let Ok(n) = s_read.read(&mut buf) {
                    if n == 0 || c_write.write_all(&buf[..n]).is_err() {
                        break;
                    }
                }
                let _ = server.shutdown(std::net::Shutdown::Both);
            });
        }
    });
    (port, sent)
}

#[tokio::test]
#[ignore]
async fn discovery_identifies_the_private_web_server_before_it_sends_a_credential() {
    use iris_agentic_dev_core::iris::discovery::{
        identify_listener, probe_atelier, Identification, Identified,
    };
    use iris_agentic_dev_core::iris::http::{IdentifyClient, TlsSetting};

    let live = iris_agentic_dev_core::testing::LiveTarget::from_env();
    let (port, sent) = relay_to(format!("{}:{}", live.host, live.web_port));
    let client = IdentifyClient::new(&TlsSetting::default(), std::time::Duration::from_secs(5))
        .expect("identify client");
    let found = identify_listener(&client, &format!("http://127.0.0.1:{port}")).await;
    assert_eq!(
        found,
        Identification::Iris(Identified::PortalCookie),
        "IRIS answers 401 Basic at /api/atelier/, so it is the Portal login page that shows it is IRIS"
    );
    let said = String::from_utf8_lossy(&sent.lock().unwrap()).to_ascii_lowercase();
    assert!(said.contains("get /csp/sys/utilhome.csp"), "{said}");
    assert!(
        !said.contains("authorization:"),
        "identification sent a credential: {said}"
    );

    // The whole probe: the identification requests carry no credential, then one request does.
    sent.lock().unwrap().clear();
    let _ = probe_atelier(
        "127.0.0.1",
        port,
        "iad141probe",
        "Zq9-SENTINEL-pw",
        "USER",
        5000,
    )
    .await;
    let said = String::from_utf8_lossy(&sent.lock().unwrap()).to_ascii_lowercase();
    let first_auth = said.find("authorization:").unwrap_or_else(|| {
        panic!("an identified listener should get the credentialed probe: {said}")
    });
    let ident = said
        .find("get /csp/sys/utilhome.csp")
        .unwrap_or_else(|| panic!("no identification request: {said}"));
    assert!(
        ident < first_auth,
        "the credential went before the identification: {said}"
    );
    assert_eq!(said.matches("authorization:").count(), 1, "{said}");
}

#[tokio::test]
#[ignore]
async fn discovery_identifies_a_gateway_fronted_instance_by_its_cookie_when_one_is_running() {
    use iris_agentic_dev_core::iris::discovery::{identify_listener, Identification};
    use iris_agentic_dev_core::iris::http::{IdentifyClient, TlsSetting};

    // The note names `iad-aihub-iris` (web port 52781, behind a gateway sidecar) "when running".
    let running = std::process::Command::new("docker")
        .args([
            "ps",
            "--filter",
            "name=^iad-aihub-iris$",
            "--format",
            "{{.Names}}",
        ])
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim() == "iad-aihub-iris")
        .unwrap_or(false);
    if !running {
        eprintln!("iad-aihub-iris is not running: the gateway-fronted case is not exercised");
        return;
    }
    let client = IdentifyClient::new(&TlsSetting::default(), std::time::Duration::from_secs(5))
        .expect("identify client");
    let found = identify_listener(&client, "http://127.0.0.1:52781").await;
    assert!(matches!(found, Identification::Iris(_)), "{found:?}");
}

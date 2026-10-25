//! T-R2-L01: what the Atelier path needs from a service account (spec 141, R2 section 5), on a
//! throwaway container.
//!
//! - **Case A**: the account README describes (no `%Development`) cannot log in to the Atelier
//!   API. The call is refused as `SERVICE_ACCOUNT_HTTP_DENIED` and does not fall back to the
//!   container, though the container is reachable.
//! - **Case B**: the layout that works. The agent's namespace has the scratch database as its
//!   default routines database, and the application's packages are mapped in from a code database
//!   the account can only read.
//! - **Negative**: the first layout R2 proposed (read-only default code database, only
//!   `IrisDevTmp` mapped to scratch) puts the class but cannot compile it: `ERROR #302`.
//!
//! `lockf -k /tmp/iad-141-iris.lock cargo test --features testing --test integration service_account_http_141 -- --ignored --test-threads=1`

use crate::support_identity_141::{Account, Container};
use iris_agentic_dev_core::iris::connection::{DiscoverySource, IrisConnection};
use iris_agentic_dev_core::testing::EnvGuard;
use iris_agentic_dev_core::testing::{ThrowawayIris, THROWAWAY_IRIS_NAME};
use iris_agentic_dev_core::tools::exec::{
    ExecContext, ExecRoute, IdentitySource, RunOpts, ERR_SERVICE_ACCOUNT_HTTP_DENIED,
};
use iris_agentic_dev_core::tools::IrisTools;
use std::sync::Arc;
use std::time::Duration;

const CASE_B_USER: &str = "iadsvc";
const CASE_B_PW: &str = "Pw141svc";
const CASE_A_USER: &str = "iadsvc_doc";
const CASE_A_PW: &str = "Pw141doc";

const LAYOUT: &str = "\
Do ##class(%File).CreateDirectoryChain(\"/usr/irissys/mgr/t141\")
For n=\"APPCODE\",\"APPDATA\",\"IADSCRATCH\" Set d=\"/usr/irissys/mgr/t141/\"_n Do ##class(%File).CreateDirectoryChain(d) Write n,\" create=\",##class(SYS.Database).CreateDatabase(d),! Kill p Set p(\"Directory\")=d Write n,\" cfg=\",##class(Config.Databases).Create(n,.p),!
For n=\"APPCODE\",\"APPDATA\",\"IADSCRATCH\" Do ##class(Security.Resources).Create(\"%DB_\"_n,\"t141\",\"\")
For n=\"APPCODE\",\"APPDATA\",\"IADSCRATCH\" Set db=##class(SYS.Database).%OpenId(\"/usr/irissys/mgr/t141/\"_n_\"/\") Set db.ResourceName=\"%DB_\"_n Write n,\" res=\",db.%Save(),!
Kill p Set p(\"Globals\")=\"APPDATA\",p(\"Routines\")=\"IADSCRATCH\" Write \"nsB=\",##class(Config.Namespaces).Create(\"T141B\",.p),!
Kill p Set p(\"Database\")=\"APPCODE\" Write \"mapB1=\",##class(Config.MapPackages).Create(\"T141B\",\"App\",.p),!
Kill p Set p(\"Database\")=\"APPCODE\" Write \"mapB2=\",##class(Config.MapRoutines).Create(\"T141B\",\"App*\",.p),!
Kill p Set p(\"Globals\")=\"APPDATA\",p(\"Routines\")=\"APPCODE\" Write \"nsA=\",##class(Config.Namespaces).Create(\"T141\",.p),!
Kill p Set p(\"Database\")=\"IADSCRATCH\" Write \"mapA1=\",##class(Config.MapPackages).Create(\"T141\",\"IrisDevTmp\",.p),!
Kill p Set p(\"Database\")=\"IADSCRATCH\" Write \"mapA2=\",##class(Config.MapRoutines).Create(\"T141\",\"IrisDevTmp*\",.p),!";

fn url(iris: &ThrowawayIris) -> String {
    format!("http://{}:{}", iris.host, iris.web_port)
}

/// The agent's namespace and the databases behind it, plus the two accounts.
fn build_layout(c: &Container) -> (Account, Account) {
    let out = c.session("%SYS", LAYOUT);
    for key in ["nsB=1", "mapB1=1", "mapB2=1", "nsA=1", "mapA1=1", "mapA2=1"] {
        assert!(out.contains(key), "layout step {key} failed:\n{out}");
    }
    // The application's code, written by the primary, before the account exists.
    let seed = c.session(
        "T141B",
        "Set cd=##class(%Dictionary.ClassDefinition).%New(\"App.Seed\") Write \"seed=\",cd.%Save(),!\nSet sc=$SYSTEM.OBJ.Compile(\"App.Seed\",\"ckq\") Write \"compile=\",sc,!",
    );
    assert!(
        seed.contains("seed=1") && seed.contains("compile=1"),
        "{seed}"
    );
    let svc = Account::create(
        c,
        CASE_B_USER,
        CASE_B_PW,
        "%DB_APPCODE:R,%DB_APPDATA:RW,%DB_IADSCRATCH:RW,%Development:U",
    );
    let doc = Account::create(c, CASE_A_USER, CASE_A_PW, "%DB_USER:R");
    (svc, doc)
}

async fn context(
    iris: &ThrowawayIris,
    ns: &str,
    user: &str,
    password: &str,
    route: ExecRoute,
) -> ExecContext {
    let mut conn =
        IrisConnection::new(url(iris), ns, user, password, DiscoverySource::ExplicitFlag);
    conn.probe().await;
    conn.member.expect_user = Some(user.to_string());
    ExecContext {
        ssh_strict: false,
        conn: Arc::new(conn),
        client: Arc::new(
            IrisConnection::http_client_for_identity(&format!("l01-{user}"), &Default::default())
                .unwrap(),
        ),
        expected: Some(user.to_string()),
        source: IdentitySource::ServiceAccountEnv,
        server: "default".into(),
        route,
        opt_in: None,
        named_server: false,
        env_container_set: false,
    }
}

fn opts() -> RunOpts<'static> {
    RunOpts::with_terminal(Duration::from_secs(120), "IrisDevRun")
}

async fn case_a_the_readme_account(iris: &ThrowawayIris, c: &Container) -> String {
    c.session("USER", "Kill ^IadT141A");

    // The container is reachable, so a fallback would succeed. It must not be taken.
    let cx = context(
        iris,
        "USER",
        CASE_A_USER,
        CASE_A_PW,
        ExecRoute::HttpThenDocker {
            container: c.name.clone(),
            ssh_host: None,
        },
    )
    .await;
    let err = cx
        .run("Set ^IadT141A=$USERNAME", "USER", &opts())
        .await
        .expect_err("an account without %Development cannot log in to /api/atelier");
    assert_eq!(err.code, ERR_SERVICE_ACCOUNT_HTTP_DENIED, "{err}");
    assert!(err.message.contains("%Development:USE"), "{}", err.message);
    assert_eq!(
        err.identity.as_ref().map(|i| i.user.as_str()),
        Some(CASE_A_USER)
    );
    assert_eq!(
        c.global("USER", "^IadT141A"),
        "UNSET",
        "the code ran, so the container leg was taken"
    );

    // check_config says the same without sending code.
    let tools = IrisTools::new(Some(IrisConnection::new(
        url(iris),
        "USER",
        "_SYSTEM",
        "SYS",
        DiscoverySource::ExplicitFlag,
    )))
    .unwrap();
    let _u = EnvGuard::with("IRIS_SERVICE_USERNAME", CASE_A_USER);
    let _p = EnvGuard::with("IRIS_SERVICE_PASSWORD", CASE_A_PW);
    let login = tools.service_login_probe().await.expect("a probe answer");
    assert!(login.starts_with("denied (HTTP 401)"), "{login}");
    login
}

async fn case_b_the_mapped_layout(iris: &ThrowawayIris, c: &Container) -> String {
    let cx = context(iris, "T141B", CASE_B_USER, CASE_B_PW, ExecRoute::Http).await;

    let count_index = || {
        let out = c.session(
            "%SYS",
            "Set n=0,x=\"\" For  Set x=$Order(^|\"^^/usr/irissys/mgr/t141/APPCODE/\"|rINDEXCLASS(x)) Quit:x=\"\"  Set n=n+1\nWrite \"N=\",n,!",
        );
        out.lines()
            .find_map(|l| l.trim().strip_prefix("N=").map(str::to_string))
            .unwrap_or_else(|| panic!("no N= in {out}"))
    };
    let before = count_index();

    // 1. Who ran, and with which roles.
    let out = cx
        .run("Write $USERNAME,\"|\",$ROLES", "T141B", &opts())
        .await
        .expect("the recommended layout must run code");
    assert!(out.output.trim().starts_with("iadsvc|"), "{}", out.output);
    assert!(!out.output.contains("%All"), "{}", out.output);
    assert!(out.identity.verified, "{:?}", out.identity);
    assert_eq!(out.identity.user, CASE_B_USER);
    assert_eq!(out.identity.path.as_str(), "atelier");
    let ran_as = out.identity.user.clone();

    // 2. A class in a mapped application package cannot be saved.
    let out = cx
        .run(
            "Set cd=##class(%Dictionary.ClassDefinition).%New(\"App.T141\") Write $SYSTEM.Status.GetErrorText(cd.%Save())",
            "T141B",
            &opts(),
        )
        .await
        .unwrap();
    assert!(out.output.contains("PROTECT"), "{}", out.output);
    let exists = c.session(
        "T141B",
        "Write \"EX=\",##class(%Dictionary.ClassDefinition).%ExistsId(\"App.T141\"),!",
    );
    assert!(
        exists.contains("EX=0"),
        "the refused save left a class: {exists}"
    );

    // 3. Nothing is left behind: no scratch class, nothing new in the code database's index.
    let left = c.session(
        "T141B",
        "Set n=0,x=\"IrisDevTmp.\" For  Set x=$Order(^oddDEF(x)) Quit:x=\"\"  Quit:$Extract(x,1,11)'=\"IrisDevTmp.\"  Set n=n+1\nWrite \"LEFT=\",n,!",
    );
    assert!(left.contains("LEFT=0"), "scratch classes remain: {left}");
    assert_eq!(
        before,
        count_index(),
        "the compile wrote into the code database"
    );
    ran_as
}

async fn negative_the_first_proposed_layout(iris: &ThrowawayIris) -> String {
    let cx = context(iris, "T141", CASE_B_USER, CASE_B_PW, ExecRoute::Http).await;
    let err = cx
        .run("Write 1", "T141", &opts())
        .await
        .expect_err("the compile needs write access to the default routines database");
    // With only a package map the compile stops at ERROR #302. With a routine map as well it passes,
    // and the procedure the generator calls is never registered: the SQL catalog (`^rINDEXSQL`)
    // lives in the read-only default database, so the query answers SQLCODE -359.
    assert!(
        err.message.contains("read-only")
            || err.message.contains("#302")
            || err.message.contains("-359"),
        "{err}"
    );
    err.message
}

#[tokio::test]
#[ignore]
async fn the_http_path_needs_development_use_and_a_writable_scratch_database() {
    let Some(iris) = ThrowawayIris::start() else {
        return;
    };
    let c = Container::named(THROWAWAY_IRIS_NAME);
    c.wait_stable();
    let (_svc, _doc) = build_layout(&c);
    let login = case_a_the_readme_account(&iris, &c).await;
    let ran_as = case_b_the_mapped_layout(&iris, &c).await;
    let negative = negative_the_first_proposed_layout(&iris).await;
    assert!(login.contains("denied"), "{login}");
    assert_eq!(ran_as, CASE_B_USER);
    assert!(!negative.is_empty());
}

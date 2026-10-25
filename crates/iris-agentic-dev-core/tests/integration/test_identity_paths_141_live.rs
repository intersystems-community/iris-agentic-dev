//! T-R2-L03 to L08 (spec 141, R2): every path that runs the agent's code names the identity it ran
//! as, and that identity is the service account. Each test reads who ran from IRIS (`$USERNAME`,
//! a sentinel global), not from the response alone.
//!
//! Against the live server (`LiveTarget`: `iris-dev-iris` by default); accounts are created by the test and dropped by it. Serial:
//! `lockf -k /tmp/iad-141-iris.lock cargo test --features testing --test integration identity_paths_141 -- --ignored --test-threads=1`

use crate::support_identity_141::{Account, Container};
use iris_agentic_dev_core::iris::config_sources::{ConfigSnapshot, LoadInputs, CONFIG_FILE_NAME};
use iris_agentic_dev_core::iris::connection::{DiscoverySource, IrisConnection};
use iris_agentic_dev_core::iris::connection_pool::load_pool;
use iris_agentic_dev_core::skills::SkillRegistry;
use iris_agentic_dev_core::testing::{EnvGuard, LiveTarget};
use iris_agentic_dev_core::tools::exec::{
    ExecContext, ExecRoute, IdentitySource, OptInSource, RunOpts, ERR_SERVICE_ACCOUNT_LOGIN_FAILED,
};
use iris_agentic_dev_core::tools::{IrisTools, Toolset};
use serde_json::{json, Value};
use std::sync::Arc;
use std::time::Duration;

const ACCOUNT_RESOURCES: &str = "%DB_USER:RW,%Development:U";
/// Read access only: a write is refused with `<PROTECT>`.
const READ_ONLY_RESOURCES: &str = "%DB_USER:R,%Development:U";

/// The primary connection: the server `Container::live()` sets up, reached over HTTP.
fn dev_conn() -> IrisConnection {
    let t = LiveTarget::from_env();
    IrisConnection::new(
        t.base_url(),
        "USER",
        &t.username,
        &t.password,
        DiscoverySource::ExplicitFlag,
    )
}

struct Rig {
    tools: IrisTools,
    _dir: tempfile::TempDir,
}

/// Tools on the live server under an operator file that opens the gates and the data policy.
async fn rig() -> Rig {
    let dir = tempfile::tempdir().unwrap();
    let root = std::fs::canonicalize(dir.path()).unwrap();
    let home = root.join("home");
    let proj = root.join("proj");
    std::fs::create_dir_all(&home).unwrap();
    std::fs::create_dir_all(&proj).unwrap();
    std::fs::write(
        home.join(CONFIG_FILE_NAME),
        "write_tools_enabled = true\ndestructive_tools_enabled = true\n\n\
         [policy.default]\nmcpTemplate = \"dev\"\nwriteTools = true\ndestructiveTools = true\ndataPolicy = \"allow\"\n\n\
         [policy._env]\nmcpTemplate = \"dev\"\nwriteTools = true\ndestructiveTools = true\ndataPolicy = \"allow\"\n\n\
         [policy.dev2]\nmcpTemplate = \"dev\"\nwriteTools = true\ndestructiveTools = true\ndataPolicy = \"allow\"\n",
    )
    .unwrap();
    let inputs = LoadInputs {
        cwd: proj,
        home: Some(home),
        ..Default::default()
    };
    let snapshot = ConfigSnapshot::load(&inputs);
    let declared = snapshot.declared_gates();
    let mut conn = dev_conn();
    conn.probe().await;
    let tools = IrisTools::with_registry_and_toolset(
        Some(conn),
        SkillRegistry::new(),
        Toolset::Merged,
        None,
        None,
        true,
        declared,
    )
    .unwrap();
    tools.set_config(inputs, snapshot);
    Rig { tools, _dir: dir }
}

async fn call(r: &Rig, tool: &str, args: Value) -> Value {
    match r.tools.call_gated(tool, args).await {
        Ok(res) => res
            .structured_content
            .clone()
            .or_else(|| {
                res.content
                    .iter()
                    .find_map(|c| c.as_text())
                    .and_then(|t| serde_json::from_str(&t.text).ok())
            })
            .unwrap_or(json!({})),
        Err(e) => json!({"error": e}),
    }
}

fn opts() -> RunOpts<'static> {
    RunOpts::with_terminal(Duration::from_secs(120), "IrisDevRun")
}

// ── L03: the forced HTTP-to-docker fallback ───────────────────────────────────

#[tokio::test]
#[ignore]
async fn l03_a_fallback_to_the_container_runs_as_the_service_account() {
    let c = Container::live();
    let user = "iadT141L03";
    let _acct = Account::create(&c, user, "Pw141L03x", ACCOUNT_RESOURCES);
    c.session("USER", "Kill ^IadT141L03");
    // The web port is closed and the container is reachable: HTTP fails before the code runs.
    let conn = IrisConnection::new(
        "http://127.0.0.1:39998",
        "USER",
        user,
        "Pw141L03x",
        DiscoverySource::ExplicitFlag,
    );
    let cx = ExecContext {
        ssh_strict: false,
        conn: Arc::new(conn),
        client: Arc::new(
            IrisConnection::http_client_for_identity("l03", &Default::default()).unwrap(),
        ),
        expected: Some(user.into()),
        source: IdentitySource::ServiceAccountEnv,
        server: "default".into(),
        route: ExecRoute::HttpThenDocker {
            container: c.name.clone(),
            ssh_host: None,
        },
        opt_in: None,
        named_server: false,
        env_container_set: true,
    };
    let out = cx
        .run("Set ^IadT141L03=$USERNAME", "USER", &opts())
        .await
        .expect("the fallback must run the code as the service account");
    assert_eq!(
        out.identity.path.as_str(),
        "docker_exec_local",
        "{:?}",
        out.identity
    );
    assert!(out.identity.verified);
    assert!(out.identity.user.eq_ignore_ascii_case(user));
    assert!(
        c.global("USER", "^IadT141L03").eq_ignore_ascii_case(user),
        "the global holds {:?}, not the service account",
        c.global("USER", "^IadT141L03")
    );
    c.session("USER", "Kill ^IadT141L03");
}

// ── L04: the WebSocket terminal ───────────────────────────────────────────────

#[tokio::test]
#[ignore]
async fn l04_the_websocket_terminal_logs_in_as_the_service_account_and_notices_a_change() {
    let c = Container::live();
    let user = "iadT141L04";
    let pw = "Pw141L04x";
    let _acct = Account::create(&c, user, pw, READ_ONLY_RESOURCES);
    c.session("USER", "Kill ^IadT141L04");
    let r = rig().await;
    let _u = EnvGuard::with("IRIS_SERVICE_USERNAME", user);
    let _p = EnvGuard::with("IRIS_SERVICE_PASSWORD", pw);

    let open = call(&r, "iris_ws_open", json!({})).await;
    let session = open["session"]
        .as_str()
        .unwrap_or_else(|| panic!("no session: {open}"))
        .to_string();
    assert!(
        open["identity"]["user"]
            .as_str()
            .unwrap()
            .eq_ignore_ascii_case(user),
        "{open}"
    );
    assert_eq!(open["identity"]["verified"], json!(true), "{open}");
    assert_eq!(open["identity"]["path"], json!("ws_terminal"), "{open}");

    let who = call(
        &r,
        "iris_ws_exec",
        json!({"session": session, "code": "Write \"WHO=\",$USERNAME,!"}),
    )
    .await;
    assert!(
        who["output"]
            .as_str()
            .unwrap_or("")
            .contains(&format!("WHO={user}")),
        "{who}"
    );

    let save = call(
        &r,
        "iris_ws_exec",
        json!({"session": session, "code": "Set ^IadT141L04=1"}),
    )
    .await;
    assert!(
        save["output"].as_str().unwrap_or("").contains("PROTECT"),
        "{save}"
    );

    // The operator removes the account: the open session is refused, not reused as the primary.
    drop(_u);
    drop(_p);
    let again = call(
        &r,
        "iris_ws_exec",
        json!({"session": session, "code": "Write 1"}),
    )
    .await;
    assert_eq!(
        again["error_code"],
        json!("SESSION_IDENTITY_CHANGED"),
        "{again}"
    );
    let gone = call(
        &r,
        "iris_ws_exec",
        json!({"session": session, "code": "Write 1"}),
    )
    .await;
    assert_ne!(
        gone["success"],
        json!(true),
        "the session must be closed: {gone}"
    );
}

// ── L05: named servers with their own account ─────────────────────────────────

#[tokio::test]
#[ignore]
async fn l05_a_named_server_runs_every_tool_as_its_own_service_account() {
    let c = Container::live();
    let own = "iadT141L05";
    let own_pw = "Pw141L05x";
    let env_user = "iadT141L05e";
    let _own = Account::create(&c, own, own_pw, ACCOUNT_RESOURCES);
    let _env = Account::create(&c, env_user, "Pw141L05y", ACCOUNT_RESOURCES);
    let table = c.session(
        "USER",
        "Do $SYSTEM.SQL.Execute(\"DROP TABLE IadT141.Who\")\nSet rs=$SYSTEM.SQL.Execute(\"CREATE TABLE IadT141.Who (U VARCHAR(100))\") Write \"T=\",rs.%SQLCODE,!",
    );
    assert!(table.contains("T=0"), "{table}");
    let grant = c.session(
        "USER",
        &format!(
            "Set rs=$SYSTEM.SQL.Execute(\"GRANT INSERT,SELECT ON IadT141.Who TO {own}Rl\") Write \"GRANT=\",rs.%SQLCODE,!"
        ),
    );
    assert!(grant.contains("GRANT=0"), "{grant}");

    // servers.json under a temp HOME, read through load_pool.
    let home = tempfile::tempdir().unwrap();
    let cfg_dir = home.path().join(".config").join("iris-agentic-dev");
    std::fs::create_dir_all(&cfg_dir).unwrap();
    let target = LiveTarget::from_env();
    let web_port = target.web_port.to_string();
    let (host, user, pass) = (&target.host, &target.username, &target.password);
    std::fs::write(
        cfg_dir.join("servers.json"),
        format!(
            r#"{{"version":1,"servers":{{"dev2":{{"host":"{host}","port":{web_port},"namespace":"USER","username":"{user}","password":"{pass}","password_endpoint":"http://{host}:{web_port}","service_username":"{own}","service_password":"{own_pw}"}}}}}}"#
        ),
    )
    .unwrap();
    let _h = EnvGuard::with("HOME", home.path().to_str().unwrap());
    let _hh = EnvGuard::with("IRIS_HOST", host);
    let _hp = EnvGuard::with("IRIS_WEB_PORT", &web_port);
    let _eu = EnvGuard::with("IRIS_SERVICE_USERNAME", env_user);
    let _ep = EnvGuard::with("IRIS_SERVICE_PASSWORD", "Pw141L05y");

    let r = rig().await;
    *r.tools.pool.write().unwrap() = Arc::new(load_pool(None));

    let who = call(
        &r,
        "iris_execute",
        json!({"code": "Write $USERNAME", "server": "dev2"}),
    )
    .await;
    assert_eq!(who["output"].as_str().map(str::trim), Some(own), "{who}");
    assert_eq!(
        who["identity"]["source"],
        json!("service_account_server"),
        "{who}"
    );
    assert_eq!(who["identity"]["verified"], json!(true), "{who}");

    let env = call(
        &r,
        "iris_execute",
        json!({"code": "Write $USERNAME", "server": "_env"}),
    )
    .await;
    assert_eq!(
        env["output"].as_str().map(str::trim),
        Some(env_user),
        "{env}"
    );
    assert_eq!(
        env["identity"]["source"],
        json!("service_account_env"),
        "{env}"
    );

    let method = call(
        &r,
        "iris_execute_method",
        json!({"class": "%SYSTEM.Process", "method": "UserName", "server": "dev2"}),
    )
    .await;
    assert!(method.to_string().contains(own), "{method}");

    let insert = call(
        &r,
        "iris_query",
        json!({"mode": "write", "query": "INSERT INTO IadT141.Who (U) SELECT $USERNAME", "server": "dev2"}),
    )
    .await;
    assert_eq!(insert["success"], json!(true), "{insert}");
    let row = c.session("USER", "Set rs=$SYSTEM.SQL.Execute(\"SELECT U FROM IadT141.Who\") Do rs.%Next() Write \"ROW=\",rs.U,!");
    assert!(row.contains(&format!("ROW={own}")), "{row}");

    let set = call(
        &r,
        "iris_global",
        json!({"action": "set", "global_name": "IadT141L05", "value": "x", "server": "dev2", "acknowledgePhi": true}),
    )
    .await;
    assert_eq!(set["success"], json!(true), "{set}");
    assert_eq!(
        set["identity"]["user"].as_str().map(str::to_lowercase),
        Some(own.to_lowercase()),
        "{set}"
    );

    c.session(
        "USER",
        "Kill ^IadT141L05\nDo $SYSTEM.SQL.Execute(\"DROP TABLE IadT141.Who\")",
    );
}

// ── L08: the opt-in ───────────────────────────────────────────────────────────

fn docker_only_cx(c: &Container, user: &str, opt_in: Option<OptInSource>) -> ExecContext {
    let conn = IrisConnection::new(
        "http://127.0.0.1:1",
        "USER",
        "_SYSTEM",
        "SYS",
        DiscoverySource::ExplicitFlag,
    );
    ExecContext {
        ssh_strict: false,
        conn: Arc::new(conn),
        client: Arc::new(
            IrisConnection::http_client_for_identity("l08", &Default::default()).unwrap(),
        ),
        expected: Some(user.into()),
        source: IdentitySource::ServiceAccountEnv,
        server: "default".into(),
        route: ExecRoute::DockerOnly {
            container: c.name.clone(),
            ssh_host: None,
        },
        opt_in,
        named_server: false,
        env_container_set: true,
    }
}

#[tokio::test]
#[ignore]
async fn l08_the_opt_in_runs_as_the_os_user_only_when_the_guard_cannot_work() {
    let c = Container::live();
    let user = "iadT141L08";
    let acct = Account::create(&c, user, "Pw141L08x", ACCOUNT_RESOURCES);
    acct.disable();

    // Without the opt-in: refused, and nothing ran.
    c.session("USER", "Kill ^IadT141L08");
    let err = docker_only_cx(&c, user, None)
        .run("Set ^IadT141L08=$USERNAME", "USER", &opts())
        .await
        .expect_err("a disabled account must not run code");
    assert_eq!(err.code, ERR_SERVICE_ACCOUNT_LOGIN_FAILED, "{err}");
    // The opt-in is operator-only: the refusal says what failed, never how to opt in (FR-039).
    assert!(
        !err.message.contains("IAD_EXEC_AS_OS_USER") && !err.message.contains("exec_as_os_user"),
        "{}",
        err.message
    );
    assert_eq!(c.global("USER", "^IadT141L08"), "UNSET");

    // With it: the OS user runs, and the response says so.
    let out = docker_only_cx(&c, user, Some(OptInSource::Env))
        .run("Set ^IadT141L08=$USERNAME", "USER", &opts())
        .await
        .expect("the opt-in runs as the container's OS user");
    assert_eq!(
        out.identity.source,
        IdentitySource::OsUserOptIn,
        "{:?}",
        out.identity
    );
    let held = c.global("USER", "^IadT141L08");
    assert_ne!(held, "UNSET");
    assert!(
        !held.eq_ignore_ascii_case(user),
        "the disabled account cannot have run: {held}"
    );
    assert!(
        out.identity.user.eq_ignore_ascii_case(&held),
        "{:?} vs {held}",
        out.identity
    );
    c.session("USER", "Kill ^IadT141L08");
}

// ── L06: global_kill, iris_test, iris_coverage ────────────────────────────────

/// The test class, as lines of a file `RunTest` reads from `^UnitTestRoot`.
const TEST_CLASS_LINES: &[&str] = &[
    "Class IadT141.Tests.L06Test Extends %UnitTest.TestCase",
    "{",
    "Method TestWho()",
    "{",
    "    Set ^IadT141L06($Get(^IadT141L06(\"mode\"),\"test\"))=$USERNAME",
    "    Do $$$AssertTrue(1,\"ran\")",
    "}",
    "}",
];

const TEST_DIR: &str = "IadT141L06";

/// Put the class on disk under `^UnitTestRoot/IadT141L06/` and compile it, as the primary:
/// `RunTest` lists a directory, and `/noload` then expects the class to be compiled already.
fn deploy_test_class(c: &Container) {
    let writes: String = TEST_CLASS_LINES
        .iter()
        .map(|l| {
            format!(
                "Write {},!",
                iris_agentic_dev_core::objectscript::os_str_expr(l)
            )
        })
        .collect::<Vec<_>>()
        .join(" ");
    let out = c.session(
        "USER",
        &format!(
            "Set ^UnitTestRoot=\"/tmp/httest/\"\n\
             Do ##class(%File).CreateDirectoryChain(\"/tmp/httest/{TEST_DIR}/\")\n\
             Set f=\"/tmp/httest/{TEST_DIR}/L06Test.cls\" Open f:\"NWS\" Use f {writes} Close f\n\
             Set sc=$SYSTEM.OBJ.Load(f,\"ckq\") Write \"LOAD=\",sc,!"
        ),
    );
    assert!(
        out.contains("LOAD=1"),
        "could not deploy the test class: {out}"
    );
}

fn remove_test_class(c: &Container) {
    c.session(
        "USER",
        &format!(
            "Do ##class(%Dictionary.ClassDefinition).%DeleteId(\"IadT141.Tests.L06Test\")\n\
             Do ##class(%File).RemoveDirectoryTree(\"/tmp/httest/{TEST_DIR}/\")\n\
             Kill ^IadT141L06"
        ),
    );
}

#[tokio::test]
#[ignore]
async fn l06_global_kill_is_refused_by_iris_for_an_account_that_cannot_write() {
    let c = Container::live();
    let user = "iadT141L06r";
    let pw = "Pw141L06r";
    // Read access to the namespace's databases, and no write access to IRISSYS, where a
    // %-global lives.
    let _acct = Account::create(&c, user, pw, ACCOUNT_RESOURCES);
    c.session("USER", "Set ^%IadT141Ro=1");
    let r = rig().await;
    let _u = EnvGuard::with("IRIS_SERVICE_USERNAME", user);
    let _p = EnvGuard::with("IRIS_SERVICE_PASSWORD", pw);

    let preview = call(&r, "global_preview", json!({"global": "^%IadT141Ro"})).await;
    let token = preview["confirm_token"]
        .as_str()
        .unwrap_or_else(|| panic!("no confirm_token: {preview}"))
        .to_string();
    let kill = call(
        &r,
        "global_kill",
        json!({"global": "^%IadT141Ro", "confirm_token": token}),
    )
    .await;
    assert!(
        kill.to_string().contains("PROTECT"),
        "IRIS refused nothing, or the refusal is not the account's: {kill}"
    );
    assert_eq!(
        kill["identity"]["user"].as_str().map(str::to_lowercase),
        Some(user.to_lowercase()),
        "{kill}"
    );
    assert_eq!(
        c.global("USER", "^%IadT141Ro"),
        "1",
        "the global was killed by an account that cannot write it"
    );
    c.session("USER", "Kill ^%IadT141Ro");
}

#[tokio::test]
#[ignore]
async fn l06_iris_test_runs_the_tests_as_the_service_account_with_and_without_a_container() {
    let c = Container::live();
    let user = "iadT141L06w";
    let pw = "Pw141L06w";
    let _acct = Account::create(&c, user, pw, ACCOUNT_RESOURCES);
    deploy_test_class(&c);
    c.session("USER", "Kill ^IadT141L06");

    let _u = EnvGuard::with("IRIS_SERVICE_USERNAME", user);
    let _p = EnvGuard::with("IRIS_SERVICE_PASSWORD", pw);

    // Without IRIS_CONTAINER: over HTTP. Unset on purpose: an earlier test in this binary once left
    // it set, and this half then ran through the container (141 T184).
    let no_container = EnvGuard::without("IRIS_CONTAINER");
    let r = rig().await;
    let http = call(&r, "iris_test", json!({"pattern": TEST_DIR})).await;
    assert_eq!(http["success"], json!(true), "{http}");
    assert!(
        c.global("USER", "^IadT141L06(\"test\")")
            .eq_ignore_ascii_case(user),
        "HTTP: the test ran as {:?}; response {http}",
        c.global("USER", "^IadT141L06(\"test\")")
    );
    assert_eq!(http["identity"]["path"], json!("atelier"), "{http}");

    // With IRIS_CONTAINER: the container, as the account through the guard.
    c.session("USER", "Set ^IadT141L06(\"mode\")=\"docker\"");
    drop(no_container);
    let _c = EnvGuard::with("IRIS_CONTAINER", &c.name);
    let r2 = rig().await;
    let docker = call(&r2, "iris_test", json!({"pattern": TEST_DIR})).await;
    assert_eq!(docker["success"], json!(true), "{docker}");
    assert!(
        c.global("USER", "^IadT141L06(\"docker\")")
            .eq_ignore_ascii_case(user),
        "docker: the test ran as {:?}; response {docker}",
        c.global("USER", "^IadT141L06(\"docker\")")
    );
    assert_eq!(
        docker["identity"]["path"],
        json!("docker_exec_local"),
        "{docker}"
    );

    remove_test_class(&c);
}

/// What `%Monitor.System.LineByLine` needs of an account is measured here, not assumed: the
/// monitor's state is system-wide, in IRISSYS.
#[tokio::test]
#[ignore]
async fn l06_iris_coverage_run_names_the_privilege_the_monitor_needs() {
    let c = Container::live();
    let user = "iadT141L06c";
    let pw = "Pw141L06c";
    let acct = Account::create(&c, user, pw, ACCOUNT_RESOURCES);
    deploy_test_class(&c);
    let r = rig().await;

    // `check` is iad's own code and runs as the connection's user.
    let check = call(&r, "iris_coverage", json!({"mode": "check"})).await;
    assert!(
        check["identity"].is_null(),
        "check runs iad's own code: {check}"
    );
    assert_eq!(
        check["ok"],
        json!(true),
        "the line monitor is not set up on this container (bbsiz), so a run cannot be measured: {check}"
    );

    let _u = EnvGuard::with("IRIS_SERVICE_USERNAME", user);
    let _p = EnvGuard::with("IRIS_SERVICE_PASSWORD", pw);
    let args = json!({"mode": "run", "test_path": TEST_DIR, "classes": ["IadT141.Tests.L06Test"]});

    // An ordinary service account cannot start the monitor, and the answer says so.
    let denied = call(&r, "iris_coverage", args.clone()).await;
    assert_eq!(
        denied["error_code"],
        json!("COVERAGE_PRIVILEGE_DENIED"),
        "{denied}"
    );
    assert!(
        denied["error"].as_str().unwrap().contains("PROTECT"),
        "{denied}"
    );
    assert!(
        denied["identity"]["user"]
            .as_str()
            .unwrap()
            .eq_ignore_ascii_case(user),
        "{denied}"
    );

    // With write access to IRISSYS it runs, as the account.
    acct.grant("%DB_IRISSYS:RW");
    c.session(
        "USER",
        "Kill ^IadT141L06\nSet ^IadT141L06(\"mode\")=\"cov\"",
    );
    let run = call(&r, "iris_coverage", args).await;
    assert_eq!(run["success"], json!(true), "{run}");
    assert!(
        c.global("USER", "^IadT141L06(\"cov\")")
            .eq_ignore_ascii_case(user),
        "the coverage run executed as {:?}",
        c.global("USER", "^IadT141L06(\"cov\")")
    );
    drop(_u);
    drop(_p);
    remove_test_class(&c);
}

// ── L07: the cookie crossover ─────────────────────────────────────────────────

#[tokio::test]
#[ignore]
async fn l07_a_session_cookie_opened_as_the_primary_does_not_decide_who_the_service_account_is() {
    let c = Container::live();
    let user = "iadT141L07";
    let pw = "Pw141L07x";
    let _acct = Account::create(&c, user, pw, ACCOUNT_RESOURCES);
    let r = rig().await;
    let _u = EnvGuard::with("IRIS_SERVICE_USERNAME", user);
    let _p = EnvGuard::with("IRIS_SERVICE_PASSWORD", pw);

    // 1. A call as the primary opens a CSP session in the shared jar.
    let primary_before = call(&r, "iris_query", json!({"query": "SELECT USER AS U"})).await;
    assert_eq!(
        primary_before["rows"][0]["U"],
        json!(LiveTarget::from_env().username),
        "{primary_before}"
    );

    // 2. The service account's call is not answered from that session.
    let svc = call(&r, "iris_execute", json!({"code": "Write $USERNAME"})).await;
    assert!(
        svc["output"]
            .as_str()
            .unwrap_or("")
            .trim()
            .eq_ignore_ascii_case(user),
        "{svc}"
    );
    assert_eq!(svc["identity"]["verified"], json!(true), "{svc}");

    // 3. And it did not replace the primary's: the primary is still the primary.
    let primary_after = call(&r, "iris_query", json!({"query": "SELECT USER AS U"})).await;
    assert_eq!(
        primary_after["rows"][0]["U"],
        json!(LiveTarget::from_env().username),
        "{primary_after}"
    );
}

// ── L09: a terminal that asks for a password ──────────────────────────────────

#[tokio::test]
#[ignore]
async fn l09_a_password_only_terminal_is_reported_and_runs_nothing() {
    use iris_agentic_dev_core::iris::connection::TerminalTarget;
    use iris_agentic_dev_core::testing::{ThrowawayIris, THROWAWAY_IRIS_NAME};
    use iris_agentic_dev_core::tools::exec::MarkerError;

    let Some(iris) = ThrowawayIris::start() else {
        return;
    };
    let c = Container::named(THROWAWAY_IRIS_NAME);
    c.wait_stable();
    // Password authentication only (32), so `iris session` asks who is there.
    let set = c.session(
        "%SYS",
        "Set p(\"AutheEnabled\")=32 Write \"SET=\",##class(Security.Services).Modify(\"%Service_Terminal\",.p),!",
    );
    assert!(set.contains("SET=1"), "{set}");

    // Without this check the test would pass on a terminal that never asked.
    let mut probe = std::process::Command::new("docker")
        .args([
            "exec", "-i", &c.name, "iris", "session", "IRIS", "-U", "USER",
        ])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .spawn()
        .unwrap();
    {
        use std::io::Write;
        let _ = probe
            .stdin
            .take()
            .unwrap()
            .write_all(b"nobody\nnothing\nhalt\n");
    }
    let asked = String::from_utf8_lossy(&probe.wait_with_output().unwrap().stdout).to_string();
    assert!(
        asked.contains("Username:"),
        "the terminal did not ask for a user name, so the setup does not test the case: {asked}"
    );

    let conn = IrisConnection::new(
        format!("http://{}:{}", iris.host, iris.web_port),
        "USER",
        "_SYSTEM",
        "SYS",
        DiscoverySource::ExplicitFlag,
    );
    let err = conn
        .execute_terminal(
            "Set ^IadT141L09=1",
            "USER",
            &TerminalTarget {
                container: c.name.clone(),
                ssh_host: None,
                login: Some("iadsvc".into()),
                ssh_strict: false,
            },
        )
        .await
        .expect_err("a terminal that asks for a password cannot be driven");
    let marker = err.downcast_ref::<MarkerError>().expect("a MarkerError");
    assert_eq!(marker.code(), "DOCKER_TERMINAL_LOGIN_REQUIRED", "{marker}");

    // Look at the global through the same terminal, logging in with the credentials this time.
    // The container is removed with `iris`, so the terminal is not put back.
    let mut look = std::process::Command::new("docker")
        .args([
            "exec", "-i", &c.name, "iris", "session", "IRIS", "-U", "USER",
        ])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .spawn()
        .unwrap();
    {
        use std::io::Write;
        let _ = look
            .stdin
            .take()
            .unwrap()
            .write_all(b"_SYSTEM\nSYS\nWrite \"G=\",$Get(^IadT141L09,\"UNSET\"),!\nhalt\n");
    }
    let seen = String::from_utf8_lossy(&look.wait_with_output().unwrap().stdout).to_string();
    assert!(
        seen.contains("G=UNSET"),
        "the agent's code ran in a terminal that never logged in, or the check could not log in: {seen}"
    );
}

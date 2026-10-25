//! Spec 141 R5 C16, live half: the durable telemetry record is kept in the local file. The IRIS
//! global gains a node only when the operator opted in (`IAD_TELEMETRY_SINK=iris`, or
//! `telemetry_sink = "iris"` in the operator file) and the call's server has its writes on, is not
//! strict and is not a subject server.
//!
//! Against iris-dev-iris. Each case makes three `iris_search` calls (a read, like `iris_info`)
//! through the gated pipeline, waits for the durable writes to finish, then asks IRIS whether
//! `^IRISDEV("telemetry", <session>)` exists. The control case, the opt-in with writes on, shows
//! the global gains a node, so an empty global in the other cases is the sink's doing and not a
//! write that never ran.
//!
//! Run: `IRIS_HOST=localhost IRIS_WEB_PORT=52780 cargo test --features testing --test integration
//! test_telemetry_sink_141 -- --include-ignored --test-threads=1`

use iris_agentic_dev_core::iris::connection::{DiscoverySource, IrisConnection};
use iris_agentic_dev_core::telemetry::{read_durable, DURABLE_WRITES};
use iris_agentic_dev_core::testing::EnvGuard;
use iris_agentic_dev_core::tools::write_gate::DeclaredGates;
use iris_agentic_dev_core::tools::{telemetry_config_dir, IrisTools, Toolset};
use std::sync::Arc;
use std::time::Duration;

fn conn() -> IrisConnection {
    let host = std::env::var("IRIS_HOST").unwrap_or_else(|_| "localhost".into());
    let port = std::env::var("IRIS_WEB_PORT").unwrap_or_else(|_| "52780".into());
    let user = std::env::var("IRIS_USERNAME").unwrap_or_else(|_| "_SYSTEM".into());
    let pass = std::env::var("IRIS_PASSWORD").unwrap_or_else(|_| "SYS".into());
    IrisConnection::new(
        format!("http://{host}:{port}"),
        "USER",
        user,
        pass,
        DiscoverySource::EnvVar,
    )
}

fn declared(writes: bool) -> DeclaredGates {
    DeclaredGates {
        write_tools_enabled: Some(writes),
        destructive_tools_enabled: Some(false),
        ..Default::default()
    }
}

/// What one run of three calls left behind.
struct Outcome {
    /// IRIS holds `^IRISDEV("telemetry", <session>)`.
    in_iris: bool,
    /// Records in the local file.
    local: usize,
    /// Records `read_durable` returns with the IRIS sink handed to it too: a call both sinks hold
    /// counts once.
    merged: usize,
}

/// Three calls, then what each sink holds. The node and the local file are removed before
/// returning. `opt_in` sets `IAD_TELEMETRY_SINK=iris`; `strict` sets `IAD_READ_ONLY=strict`.
async fn run(writes: bool, opt_in: bool, strict: bool) -> Outcome {
    let mut env_guard = EnvGuard::new();
    // Safety: the integration target runs single-threaded (`--test-threads=1`).
    env_guard.unset("IAD_TELEMETRY_SINK").unset("IAD_READ_ONLY");
    if opt_in {
        env_guard.set("IAD_TELEMETRY_SINK", "iris");
    }
    if strict {
        env_guard.set("IAD_READ_ONLY", "strict");
    }
    let tools = IrisTools::for_test_with_gates(Some(conn()), Toolset::Merged, declared(writes))
        .expect("tools build");
    for _ in 0..3 {
        let answer = tools
            .call_for_test(
                "iris_search",
                serde_json::json!({"query": "zzz141", "documents": ["Nope141.*.cls"], "namespace": "USER"}),
            )
            .await
            .expect("iris_search answers");
        assert_ne!(
            answer.is_error,
            Some(true),
            "iris_search must reach IRIS for this test to mean anything: {answer:?}"
        );
    }
    assert!(
        DURABLE_WRITES.flush(Duration::from_secs(20)).await,
        "the durable writes did not finish"
    );
    let sid = tools.session.id;
    let out = outcome(sid).await;
    env_guard.unset("IAD_TELEMETRY_SINK").unset("IAD_READ_ONLY");
    out
}

/// Read both sinks for `sid`, then tear down: the node (when there is one) and the local file.
async fn outcome(sid: uuid::Uuid) -> Outcome {
    let probe = conn();
    let client = IrisConnection::http_client().unwrap();
    let out = probe
        .execute_via_generator(
            &format!("write $DATA(^IRISDEV(\"telemetry\",\"{sid}\"))"),
            "USER",
            &client,
        )
        .await
        .expect("the probe runs");
    let in_iris = out.trim() != "0";
    let dir = telemetry_config_dir();
    let local = read_durable(Some(sid), None, &client, &dir).await.len();
    let merged = read_durable(Some(sid), Some(Arc::new(conn())), &client, &dir)
        .await
        .len();
    probe
        .execute_via_generator(
            &format!("kill ^IRISDEV(\"telemetry\",\"{sid}\")"),
            "USER",
            &client,
        )
        .await
        .expect("teardown runs");
    let _ = std::fs::remove_file(dir.join("telemetry").join(format!("{sid}.jsonl")));
    Outcome {
        in_iris,
        local,
        merged,
    }
}

/// Set in the child that [`run_in_child_without_operator_gates`] starts, so it runs the case itself.
const WRITES_OFF_CHILD: &str = "IAD_T141_TELEMETRY_WRITES_OFF_CHILD";

/// Re-run one test of this binary in a child process with the operator's gate variables removed,
/// and assert it passed. The child keeps the connection variables, so it reaches the same server.
fn run_in_child_without_operator_gates(test: &str) {
    let out = std::process::Command::new(std::env::current_exe().expect("this test binary"))
        .args(["--exact", test, "--include-ignored", "--test-threads=1"])
        .env(WRITES_OFF_CHILD, "1")
        .env_remove("IRIS_WRITE_TOOLS_ENABLED")
        .env_remove("IRIS_DESTRUCTIVE_TOOLS_ENABLED")
        .env_remove("IRIS_ALLOW_PROD")
        .output()
        .expect("must start the child test process");
    assert!(
        String::from_utf8_lossy(&out.stdout).contains("test result: ok. 1 passed"),
        "the child did not pass {test}:\n{}\n{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
}

#[tokio::test]
#[ignore = "live IRIS on iris-dev-iris"]
async fn by_default_with_writes_on_the_global_gains_no_node_and_the_records_stay_local() {
    let o = run(true, false, false).await;
    assert!(
        !o.in_iris,
        "default settings: ^IRISDEV(\"telemetry\", sid) must stay undefined on a writable server"
    );
    assert_eq!(
        o.local, 3,
        "default settings: the three calls are in the local file"
    );
    assert_eq!(
        o.merged, 3,
        "reading with the IRIS sink handed over finds the same three"
    );
}

#[tokio::test]
#[ignore = "live IRIS on iris-dev-iris"]
async fn with_the_opt_in_and_writes_on_the_global_gains_a_node_and_the_file_has_the_records_too() {
    let o = run(true, true, false).await;
    assert!(
        o.in_iris,
        "opt-in, writes on: the IRIS sink should have written"
    );
    assert_eq!(o.local, 3, "opt-in: the local file is written as well");
    assert_eq!(
        o.merged, 3,
        "a call both sinks hold is returned once, not twice"
    );
}

#[tokio::test]
#[ignore = "live IRIS on iris-dev-iris"]
async fn with_the_opt_in_and_writes_off_the_global_gains_no_node_and_the_records_stay_local() {
    // The default connection's writes are off only when the operator's launch settings do not turn
    // them on: `IRIS_WRITE_TOOLS_ENABLED=1` outranks the declaration under test, and the process
    // reads it once, at its first gate resolution. The CI job exports it, so the case runs in a
    // child process of this binary that starts without the operator's gate variables.
    if std::env::var_os(WRITES_OFF_CHILD).is_none() {
        run_in_child_without_operator_gates(
            "test_telemetry_sink_141_live::\
             with_the_opt_in_and_writes_off_the_global_gains_no_node_and_the_records_stay_local",
        );
        return;
    }
    let o = run(false, true, false).await;
    assert!(
        !o.in_iris,
        "opt-in, writes off: ^IRISDEV(\"telemetry\", sid) must stay undefined"
    );
    assert_eq!(
        o.local, 3,
        "writes off: the three calls are in the local file"
    );
}

#[tokio::test]
#[ignore = "live IRIS on iris-dev-iris"]
async fn with_the_opt_in_under_strict_read_only_the_global_gains_no_node() {
    let o = run(true, true, true).await;
    assert!(
        !o.in_iris,
        "strict: ^IRISDEV(\"telemetry\", sid) must stay undefined"
    );
    assert_eq!(o.local, 3, "strict: the three calls are in the local file");
}

// ── T208: the sink writes to the server the call named, and only where it may ────────────────────
//
// The default connection is a port nothing answers on, with its writes on. Before T208 the sink
// wrote to the default connection whatever server the call named, so a call on `live` left no node
// on `live` (the write went to the dead port and was dropped). Every case here opts in in the
// operator file, except the one that shows the opt-in is what matters.

mod named_server {
    use super::*;
    use iris_agentic_dev_core::iris::config_sources::{
        ConfigSnapshot, LoadInputs, CONFIG_FILE_NAME,
    };
    use iris_agentic_dev_core::iris::connection_pool::ConnectionPool;
    use iris_agentic_dev_core::skills::SkillRegistry;

    fn dev_host() -> String {
        std::env::var("IRIS_HOST").unwrap_or_else(|_| "localhost".into())
    }

    async fn live_conn() -> IrisConnection {
        let mut c = conn();
        c.probe().await;
        c
    }

    async fn dead_conn() -> IrisConnection {
        let mut c = IrisConnection::new(
            "http://127.0.0.1:1",
            "USER",
            "_SYSTEM",
            "SYS",
            DiscoverySource::ExplicitFlag,
        );
        c.probe().await;
        c
    }

    /// Three `iris_search` calls with `server: "live"`, then what each sink holds. Cleans up after
    /// itself. The launch settings carry no sink choice: the operator file says it.
    async fn run(operator: &str) -> Outcome {
        let mut env_guard = EnvGuard::new();
        env_guard.unset("IAD_TELEMETRY_SINK").unset("IAD_READ_ONLY");
        let dir = tempfile::tempdir().unwrap();
        let root = std::fs::canonicalize(dir.path()).unwrap();
        let (home, proj) = (root.join("home"), root.join("proj"));
        std::fs::create_dir_all(&home).unwrap();
        std::fs::create_dir_all(&proj).unwrap();
        std::fs::write(home.join(CONFIG_FILE_NAME), operator).unwrap();
        let inputs = LoadInputs {
            cwd: proj,
            home: Some(home),
            ..Default::default()
        };
        let snapshot = ConfigSnapshot::load(&inputs);
        let declared = snapshot.declared_gates();
        let tools = IrisTools::with_registry_and_toolset(
            Some(dead_conn().await),
            SkillRegistry::new(),
            Toolset::Merged,
            None,
            None,
            true,
            declared,
        )
        .unwrap();
        tools.set_config(inputs, snapshot);
        let mut pool = ConnectionPool::builder();
        pool.add("dev".to_string(), dead_conn().await, true);
        pool.add("live".to_string(), live_conn().await, false);
        *tools.pool.write().unwrap() = Arc::new(pool.build());

        for _ in 0..3 {
            let answer = tools
                .call_gated(
                    "iris_search",
                    serde_json::json!({"query": "zzz141", "documents": ["Nope141.*.cls"], "namespace": "USER", "server": "live"}),
                )
                .await
                .expect("iris_search answers");
            assert_ne!(
                answer.is_error,
                Some(true),
                "iris_search on `live` must reach IRIS for this test to mean anything: {answer:?}"
            );
        }
        assert!(
            DURABLE_WRITES.flush(Duration::from_secs(20)).await,
            "the durable writes did not finish"
        );
        outcome(tools.session.id).await
    }

    const OPT_IN: &str = "telemetry_sink = \"iris\"\n";

    #[tokio::test]
    #[ignore = "live IRIS on iris-dev-iris"]
    async fn a_call_on_a_named_server_records_to_that_server_with_the_opt_in() {
        let o = run(&format!(
            "{OPT_IN}write_tools_enabled = true\n[policy.live]\nwriteTools = true\n"
        ))
        .await;
        assert!(o.in_iris, "the named server must hold the session's node");
        assert_eq!(o.local, 3, "the local file is written as well");
        assert_eq!(o.merged, 3, "a call both sinks hold is returned once");
    }

    #[tokio::test]
    #[ignore = "live IRIS on iris-dev-iris"]
    async fn a_named_server_with_writes_on_gets_no_node_without_the_opt_in() {
        let o = run("write_tools_enabled = true\n[policy.live]\nwriteTools = true\n").await;
        assert!(!o.in_iris, "no opt-in: no node on `live`");
        assert_eq!(o.local, 3, "the three calls are in the local file");
    }

    #[tokio::test]
    #[ignore = "live IRIS on iris-dev-iris"]
    async fn a_named_server_with_writes_off_gets_no_node_though_the_default_has_writes_on() {
        let o = run(&format!(
            "{OPT_IN}write_tools_enabled = true\n[policy.live]\nwriteTools = false\n"
        ))
        .await;
        assert!(!o.in_iris, "writes off on `live`: no node");
        assert_eq!(o.local, 3, "the three calls are in the local file");
    }

    #[tokio::test]
    #[ignore = "live IRIS on iris-dev-iris"]
    async fn under_strict_read_only_a_named_server_gets_no_node() {
        let o = run(&format!(
            "{OPT_IN}read_only = \"strict\"\nwrite_tools_enabled = true\n[policy.live]\nwriteTools = true\n"
        ))
        .await;
        assert!(!o.in_iris, "strict: no node on `live`");
        assert_eq!(o.local, 3, "the three calls are in the local file");
    }

    #[tokio::test]
    #[ignore = "live IRIS on iris-dev-iris"]
    async fn a_subject_server_gets_no_node() {
        let operator = format!(
            "{OPT_IN}mode = \"operate\"\nwrite_tools_enabled = true\n[policy.live]\nwriteTools = true\n\
             [instance.live]\nhost = \"{}\"\nrole = \"subject\"\n",
            dev_host()
        );
        let o = run(&operator).await;
        assert!(!o.in_iris, "a server under change control: no node");
        assert_eq!(o.local, 3, "the three calls are in the local file");
    }
}

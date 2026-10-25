//! Layer 3 for spec 141 FR-001 to FR-006, R1 section 6.3 and T-R4a-L01, L04: the gates of the
//! server a call names, against a real IRIS.
//!
//! `iris-dev-iris` is registered twice, as `dev` and as `prod` with `mcpTemplate = "live"`. A write
//! that names `prod` is refused and leaves nothing behind. The Live case needs an instance whose
//! `^%SYS("SystemMode")` is `LIVE`; that is only ever the throwaway container, never
//! `iris-dev-iris`.
//!
//! Serial, `#[ignore]`: run with
//! `lockf -k /tmp/iad-141-iris.lock cargo test --features testing --test integration target_gates_141 -- --ignored --test-threads=1`.

use iris_agentic_dev_core::iris::config_sources::{ConfigSnapshot, LoadInputs, CONFIG_FILE_NAME};
use iris_agentic_dev_core::iris::connection::{DiscoverySource, IrisConnection};
use iris_agentic_dev_core::iris::connection_pool::ConnectionPool;
use iris_agentic_dev_core::skills::SkillRegistry;
use iris_agentic_dev_core::testing::ThrowawayIris;
use iris_agentic_dev_core::tools::{IrisTools, Toolset};
use serde_json::{json, Value};
use std::sync::Arc;

fn dev_url() -> String {
    let host = std::env::var("IRIS_HOST").unwrap_or_else(|_| "localhost".into());
    let port = std::env::var("IRIS_WEB_PORT").unwrap_or_else(|_| "52780".into());
    format!("http://{host}:{port}")
}

async fn live_conn(url: &str) -> IrisConnection {
    let mut c = IrisConnection::new(url, "USER", "_SYSTEM", "SYS", DiscoverySource::ExplicitFlag);
    c.probe().await;
    c
}

struct Rig {
    tools: IrisTools,
    _dir: tempfile::TempDir,
}

/// A default connection to `iris-dev-iris` and a pool of `members` (name, url), under the operator
/// file `operator` and, when given, the untrusted project file `project`.
async fn rig(operator: &str, project: Option<&str>, members: &[(&str, String)]) -> Rig {
    let dir = tempfile::tempdir().unwrap();
    let root = std::fs::canonicalize(dir.path()).unwrap();
    let home = root.join("home");
    let proj = root.join("proj");
    std::fs::create_dir_all(&home).unwrap();
    std::fs::create_dir_all(&proj).unwrap();
    std::fs::write(home.join(CONFIG_FILE_NAME), operator).unwrap();
    if let Some(p) = project {
        std::fs::write(proj.join(CONFIG_FILE_NAME), p).unwrap();
    }
    let inputs = LoadInputs {
        cwd: proj,
        home: Some(home),
        ..Default::default()
    };
    let snapshot = ConfigSnapshot::load(&inputs);
    let declared = snapshot.declared_gates();
    let tools = IrisTools::with_registry_and_toolset(
        Some(live_conn(&dev_url()).await),
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
    pool.add("dev".to_string(), live_conn(&dev_url()).await, true);
    for (name, url) in members {
        pool.add(name.to_string(), live_conn(url).await, false);
    }
    *tools.pool.write().unwrap() = Arc::new(pool.build());
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

fn code(b: &Value) -> &str {
    b["error_code"].as_str().unwrap_or("")
}

const SENTINEL: &str = "Iad141.Sentinel.cls";
const SENTINEL_BODY: &str = "Class Iad141.Sentinel { }\n";

const PROD_LIVE_WRITABLE: &str = "[policy.prod]\nmcpTemplate = \"live\"\nwriteTools = true\n";

#[tokio::test]
#[ignore = "requires live IRIS (iris-dev-iris)"]
async fn a_sentinel_class_put_with_server_prod_is_refused_and_absent() {
    let r = rig(PROD_LIVE_WRITABLE, None, &[("prod", dev_url())]).await;
    // Clean slate.
    let _ = call(&r, "iris_doc", json!({"mode": "delete", "name": SENTINEL})).await;

    let refused = call(
        &r,
        "iris_doc",
        json!({"mode": "put", "name": SENTINEL, "content": SENTINEL_BODY, "server": "prod"}),
    )
    .await;
    assert_eq!(code(&refused), "ENV_GATE_BLOCKED", "{refused}");
    assert!(refused.to_string().contains("prod"), "{refused}");

    let absent = call(&r, "iris_doc", json!({"mode": "get", "name": SENTINEL})).await;
    assert_ne!(
        absent["success"],
        json!(true),
        "the refused put must leave no class behind: {absent}"
    );

    // The same call with no server runs: dev has no live template.
    let ok = call(
        &r,
        "iris_doc",
        json!({"mode": "put", "name": SENTINEL, "content": SENTINEL_BODY}),
    )
    .await;
    assert_eq!(ok["success"], json!(true), "{ok}");
    let _ = call(&r, "iris_doc", json!({"mode": "delete", "name": SENTINEL})).await;
}

/// Set `^%SYS("SystemMode")` on the throwaway container, through the container's own terminal.
fn set_throwaway_mode(tw: &ThrowawayIris, mode: &str) {
    let out = tw.session(
        "%SYS",
        &format!(
            "Set ^%SYS(\"SystemMode\")=\"{mode}\"\nWrite \"MODE-SET:\",^%SYS(\"SystemMode\"),!"
        ),
    );
    assert!(
        out.contains(&format!("MODE-SET:{mode}")),
        "could not set SystemMode: {out}"
    );
}

fn throwaway_url(t: &ThrowawayIris) -> String {
    format!("http://{}:{}", t.host, t.web_port)
}

// T-R4a-L01 and R1 section 6.3: a member on a LIVE instance.
#[tokio::test]
#[ignore = "requires Docker: starts and removes the throwaway IRIS iad-throwaway-141"]
async fn a_live_member_is_write_closed_and_an_undeclared_template_follows_its_system_mode() {
    let Some(tw) = ThrowawayIris::start() else {
        return;
    };
    set_throwaway_mode(&tw, "LIVE");

    // 1. No declaration: the probe reads LIVE, the write gate closes, and says where it came from.
    let r = rig("", None, &[("tw", throwaway_url(&tw))]).await;
    let b = call(
        &r,
        "iris_execute",
        json!({"code": "Write 1", "server": "tw"}),
    )
    .await;
    assert_eq!(code(&b), "WRITE_TOOLS_DISABLED", "{b}");
    assert!(b.to_string().contains("inferred_system_mode"), "{b}");
    assert!(b.to_string().contains("tw"), "{b}");

    // 2. Writes declared, no template anywhere: the system mode picks `live`, which refuses Execute.
    let r = rig(
        "[policy.tw]\nwriteTools = true\n",
        None,
        &[("tw", throwaway_url(&tw))],
    )
    .await;
    let b = call(
        &r,
        "iris_execute",
        json!({"code": "Write 1", "server": "tw"}),
    )
    .await;
    assert_eq!(code(&b), "ENV_GATE_BLOCKED", "{b}");
    assert_eq!(b["template"], json!("live"), "{b}");
    assert_eq!(
        b["refusal"]["server"], "tw",
        "the refusal is judged on the member, not the default: {b}"
    );

    set_throwaway_mode(&tw, "");
}

#[tokio::test]
#[ignore = "requires live IRIS (iris-dev-iris)"]
async fn iris_message_body_on_prod_stays_blocked_whatever_the_caller_sends() {
    let fixture = iris_agentic_dev_core::testing::seed_interop_fixture();
    let r = rig(
        "[policy.default]\ndataPolicy = \"allow\"\n[policy.prod]\ndataPolicy = \"block\"\n",
        None,
        &[("prod", dev_url())],
    )
    .await;
    let id = call(
        &r,
        "iris_query",
        json!({"query": format!(
            "SELECT TOP 1 ID FROM Ens.MessageHeader WHERE SourceConfigName = '{}'",
            fixture.source
        )}),
    )
    .await;
    let message_id = id["rows"][0]["ID"]
        .to_string()
        .trim_matches('"')
        .to_string();

    let blocked = call(
        &r,
        "iris_message_body",
        json!({"message_id": message_id, "server": "prod", "dataPolicy": "allow", "acknowledgePhi": true}),
    )
    .await;
    assert_eq!(code(&blocked), "DATA_POLICY_BLOCKED", "{blocked}");
    assert!(blocked.to_string().contains("prod"), "{blocked}");

    let body = call(
        &r,
        "iris_message_body",
        json!({"message_id": message_id, "dataPolicy": "allow", "acknowledgePhi": true}),
    )
    .await;
    assert_ne!(
        code(&body),
        "DATA_POLICY_BLOCKED",
        "the default allows data reads: {body}"
    );
}

#[tokio::test]
#[ignore = "requires live IRIS (iris-dev-iris)"]
async fn iris_test_on_a_test_template_member_adds_no_unit_test_result_row() {
    let r = rig(
        "[policy.prod]\nmcpTemplate = \"test\"\nwriteTools = true\n",
        None,
        &[("prod", dev_url())],
    )
    .await;
    let count = |r: &Rig| {
        let r = &r.tools;
        let r = r.clone();
        async move {
            let res = r
                .call_gated(
                    "iris_query",
                    json!({"query": "SELECT COUNT(*) AS N FROM %UnitTest_Result.TestSuite"}),
                )
                .await
                .unwrap();
            let b = res.structured_content.unwrap_or(json!({}));
            b["rows"][0]["N"].as_i64().unwrap_or(-1)
        }
    };
    let before = count(&r).await;
    let b = call(
        &r,
        "iris_test",
        json!({"pattern": "Iad141.NoSuchTests", "server": "prod"}),
    )
    .await;
    assert_eq!(code(&b), "ENV_GATE_BLOCKED", "{b}");
    assert_eq!(
        count(&r).await,
        before,
        "a refused iris_test must not run anything"
    );
}

// T-R4a-L04
#[tokio::test]
#[ignore = "requires live IRIS (iris-dev-iris)"]
async fn an_untrusted_data_policy_allow_does_not_open_journal_search() {
    let r = rig("", Some("[policy.default]\ndataPolicy = \"allow\"\n"), &[]).await;
    let b = call(&r, "journal_search", json!({"global_pattern": "^Iad141*"})).await;
    assert_eq!(code(&b), "DATA_POLICY_BLOCKED", "{b}");
    assert_eq!(b["refusal"]["kind"], "untrusted_setting", "{b}");
}

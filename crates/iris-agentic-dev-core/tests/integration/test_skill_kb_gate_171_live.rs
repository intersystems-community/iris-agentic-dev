//! Layer 3 for R2, against `iris-dev-iris`: under a `live` template or strict read-only the skill
//! and knowledge-base tools do not put a scratch class on the server.
//!
//! The default connection goes through a forwarding socket that counts the requests naming an
//! `IrisDevTmp` document, the scratch package. It forwards every byte to the real server and
//! changes nothing; it exists because the class is deleted before the call returns, so nothing on
//! the server shows that one was there. The control runs the same call under the `dev` template
//! and has to put at least one.
//!
//! Serial, `#[ignore]`: run with
//! `lockf -k /tmp/iad-141-iris.lock cargo test --features testing --test integration skill_kb_gate_171 -- --ignored --test-threads=1`.

use iris_agentic_dev_core::iris::config_sources::{ConfigSnapshot, LoadInputs, CONFIG_FILE_NAME};
use iris_agentic_dev_core::iris::connection::{DiscoverySource, IrisConnection};
use iris_agentic_dev_core::skills::SkillRegistry;
use iris_agentic_dev_core::testing::{EnvGuard, LiveTarget};
use iris_agentic_dev_core::tools::{IrisTools, Toolset};
use serde_json::{json, Value};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

/// A socket on 127.0.0.1 that forwards to `target` and counts the client-to-server chunks that
/// name a scratch document.
struct CountingProxy {
    port: u16,
    scratch_requests: Arc<AtomicUsize>,
}

impl CountingProxy {
    async fn start(target: String) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let counter = Arc::new(AtomicUsize::new(0));
        let seen = counter.clone();
        tokio::spawn(async move {
            loop {
                let Ok((client, _)) = listener.accept().await else {
                    return;
                };
                let target = target.clone();
                let seen = seen.clone();
                tokio::spawn(async move {
                    let Ok(server) = TcpStream::connect(&target).await else {
                        return;
                    };
                    let (mut cr, mut cw) = client.into_split();
                    let (mut sr, mut sw) = server.into_split();
                    let up = async move {
                        let mut buf = vec![0u8; 16 * 1024];
                        loop {
                            let n = match cr.read(&mut buf).await {
                                Ok(0) | Err(_) => break,
                                Ok(n) => n,
                            };
                            let chunk = String::from_utf8_lossy(&buf[..n]);
                            seen.fetch_add(chunk.matches("IrisDevTmp").count(), Ordering::SeqCst);
                            if sw.write_all(&buf[..n]).await.is_err() {
                                break;
                            }
                        }
                        let _ = sw.shutdown().await;
                    };
                    let down = async move {
                        let _ = tokio::io::copy(&mut sr, &mut cw).await;
                        let _ = cw.shutdown().await;
                    };
                    tokio::join!(up, down);
                });
            }
        });
        CountingProxy {
            port,
            scratch_requests: counter,
        }
    }

    fn count(&self) -> usize {
        self.scratch_requests.load(Ordering::SeqCst)
    }
}

struct Rig {
    tools: IrisTools,
    proxy: CountingProxy,
    _dir: tempfile::TempDir,
}

async fn rig(operator: &str) -> Rig {
    let live = LiveTarget::from_env();
    live.assert_running();
    let proxy = CountingProxy::start(format!("{}:{}", live.host, live.web_port)).await;
    let dir = tempfile::tempdir().unwrap();
    let root = std::fs::canonicalize(dir.path()).unwrap();
    let home = root.join("home");
    let proj = root.join("proj");
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
    let mut conn = IrisConnection::new(
        format!("http://127.0.0.1:{}", proxy.port),
        &live.namespace,
        &live.username,
        &live.password,
        DiscoverySource::ExplicitFlag,
    );
    conn.probe().await;
    let tools = IrisTools::with_registry_and_toolset(
        Some(conn),
        SkillRegistry::new(),
        Toolset::Baseline,
        None,
        None,
        false,
        declared,
    )
    .unwrap();
    tools.set_config(inputs, snapshot);
    Rig {
        tools,
        proxy,
        _dir: dir,
    }
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

const OPEN: &str = "write_tools_enabled = true\ndestructive_tools_enabled = true\n";

/// Every call of the family that would put a scratch class on the server, and the terminal ones.
fn family() -> Vec<(&'static str, Value)> {
    vec![
        ("skill", json!({"action": "list"})),
        ("skill", json!({"action": "search", "query": "x"})),
        ("kb", json!({"action": "recall", "query": "x"})),
        ("kb_index", json!({})),
        ("agent_info", json!({"what": "stats"})),
        ("skill_community", json!({"action": "list"})),
        ("skill_list", json!({})),
        ("skill_describe", json!({"name": "x"})),
        ("skill_search", json!({"query": "x"})),
    ]
}

#[tokio::test]
#[ignore = "requires live IRIS (iris-dev-iris)"]
async fn the_control_puts_a_scratch_class_under_the_dev_template() {
    let r = rig(OPEN).await;
    let before = r.proxy.count();
    // `agent_info stats` counts `^SKILLS` through a scratch class. (`skill list` would do as a
    // control but for a separate defect: its generated ObjectScript escapes quotes with
    // backslashes, so the class does not compile and the call fails after the PUT.)
    let b = call(&r, "agent_info", json!({"what": "stats"})).await;
    assert_eq!(b["success"], true, "{b}");
    assert!(
        r.proxy.count() > before,
        "the control compiled no scratch class, so the counting below proves nothing: {b}"
    );
}

#[tokio::test]
#[ignore = "requires live IRIS (iris-dev-iris)"]
async fn skill_list_under_a_live_template_compiles_nothing() {
    // `skill_list` reads the container's terminal when a container is named; name the real one so
    // the control below is the path an operator has.
    let live = LiveTarget::from_env();
    let _container = EnvGuard::with("IRIS_CONTAINER", &live.container);
    let r = rig(&format!("{OPEN}[policy.default]\nmcpTemplate = \"live\"\n")).await;
    let before = r.proxy.count();
    for (tool, args) in family() {
        let b = call(&r, tool, args.clone()).await;
        assert_eq!(b["error_code"], "ENV_GATE_BLOCKED", "{tool} {args}: {b}");
        assert_eq!(b["blocked_category"], "execute", "{tool}: {b}");
    }
    assert_eq!(
        r.proxy.count(),
        before,
        "a refused call put a scratch class on the server"
    );
}

#[tokio::test]
#[ignore = "requires live IRIS (iris-dev-iris)"]
async fn the_skill_tools_under_strict_put_nothing_on_the_server() {
    let r = rig(&format!("{OPEN}read_only = \"strict\"\n")).await;
    let before = r.proxy.count();
    for (tool, args) in [
        ("skill", json!({"action": "list"})),
        ("kb", json!({"action": "recall", "query": "x"})),
        ("kb_index", json!({})),
        ("agent_info", json!({"what": "stats"})),
        ("skill_community", json!({"action": "list"})),
    ] {
        let b = call(&r, tool, args.clone()).await;
        assert_eq!(b["error_code"], "READ_ONLY_STRICT", "{tool} {args}: {b}");
    }
    // The terminal-only readers stay, and they compile nothing either.
    for (tool, args) in [
        ("skill_list", json!({})),
        ("skill_search", json!({"query": "x"})),
    ] {
        let b = call(&r, tool, args.clone()).await;
        assert_ne!(b["error_code"], "READ_ONLY_STRICT", "{tool}: {b}");
        assert!(b["count"].as_u64().is_some(), "{tool}: {b}");
    }
    assert_eq!(
        r.proxy.count(),
        before,
        "strict let a scratch class reach the server"
    );
}

#[tokio::test]
#[ignore = "requires live IRIS (iris-dev-iris)"]
async fn a_narrow_allow_keeps_them_off_the_server_too() {
    let r = rig(&format!("{OPEN}[policy.default]\nallow = [\"query\"]\n")).await;
    let before = r.proxy.count();
    for (tool, args) in family() {
        let b = call(&r, tool, args.clone()).await;
        assert_eq!(b["error_code"], "POLICY_GATE", "{tool} {args}: {b}");
    }
    assert_eq!(r.proxy.count(), before);
}

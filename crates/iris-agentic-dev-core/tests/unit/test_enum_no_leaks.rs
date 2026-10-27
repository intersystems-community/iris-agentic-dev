//! No tool reaches IRIS past its gate (E2).
//!
//! Every registered tool, in every toolset, is called through the MCP tail of the real pipeline
//! with minimal valid arguments, under three settings that each close a different gate:
//!
//! - `read_only = "strict"`;
//! - a `live` template that allows only `search`, writes closed;
//! - writes and destructive writes closed.
//!
//! The "IRIS" is a TCP listener on loopback that counts connections and answers each with an empty 404. It stands for no IRIS behaviour (the pattern of `test_probe_server_offline`): what is
//! measured is whether iad opened a connection. The rule that comes out is one sentence:
//!
//! > A call may reach the server only if the table says the setting permits it.
//!
//! A `Local` row is a claim that the tool touches no IRIS, so it may never connect. An `Iris` row
//! may connect only for a read (writes are closed in all three settings), not as a scratch-class
//! tool under strict, and only in a category the narrow `allow` lists. A tool that opens a
//! connection when it should have been refused fails by name, and so does one the pipeline let
//! through that the table said it would refuse.
//!
//! The arguments come from the advertised schema (required fields, by type), not from a list of
//! the tools: a tool added tomorrow is called tomorrow. The skip list below is the only
//! hand-kept list, and it may only shrink.

use crate::support_gate_141::{conn, McpClient};
use iris_agentic_dev_core::iris::config_sources::{ConfigSnapshot, LoadInputs, CONFIG_FILE_NAME};
use iris_agentic_dev_core::iris::connection::SystemMode;
use iris_agentic_dev_core::iris::connection_pool::ConnectionPool;
use iris_agentic_dev_core::iris::workspace_config::ToolCategory;
use iris_agentic_dev_core::skills::SkillRegistry;
use iris_agentic_dev_core::tools::write_gate::{
    classify, effective_category, tool_class, TargetKind, TargetSource, WriteClass, CLASSIFICATION,
};
use iris_agentic_dev_core::tools::{IrisTools, Toolset};
use serde_json::{json, Map, Value};
use std::collections::BTreeSet;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

/// Tools the sweep does not call, with the reason. This list may only shrink: a new entry is a
/// tool nobody checks, and a reviewer should see that in this file.
const SKIPPED: &[(&str, &str)] = &[(
    "iris_doc_search",
    "searches the public documentation site over the Internet, which a unit run must not do",
)];

/// The most entries `SKIPPED` may hold. Lower it when an entry goes; raising it is a review event.
const SKIP_BUDGET: usize = 1;

#[derive(Clone, Copy, Debug)]
enum Setting {
    Strict,
    LiveNarrowAllow,
    WritesOff,
}

impl Setting {
    const ALL: [Setting; 3] = [
        Setting::Strict,
        Setting::LiveNarrowAllow,
        Setting::WritesOff,
    ];

    /// The operator file. Writes are closed in all three, so a write tool that is not refused by
    /// the setting under test is refused by the write gate, and no sweep call can change anything.
    fn toml(self) -> &'static str {
        match self {
            Setting::Strict => {
                "read_only = \"strict\"\nwrite_tools_enabled = false\ndestructive_tools_enabled = false\n"
            }
            Setting::LiveNarrowAllow => concat!(
                "write_tools_enabled = false\ndestructive_tools_enabled = false\n",
                "[policy.default]\nmcpTemplate = \"live\"\nallow = [\"search\"]\n"
            ),
            Setting::WritesOff => "write_tools_enabled = false\ndestructive_tools_enabled = false\n",
        }
    }
}

/// A loopback listener that counts connections and drops them.
struct Counter {
    port: u16,
    seen: Arc<AtomicUsize>,
}

impl Counter {
    async fn bind() -> Counter {
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let port = listener.local_addr().unwrap().port();
        let seen = Arc::new(AtomicUsize::new(0));
        let count = seen.clone();
        tokio::spawn(async move {
            while let Ok((mut sock, _)) = listener.accept().await {
                count.fetch_add(1, Ordering::SeqCst);
                // One 404 and a close. A bare drop makes the generator path retry for fifteen
                // seconds, and a 404 is not retried. It says nothing about IRIS either way.
                tokio::spawn(async move {
                    let mut buf = [0u8; 8192];
                    let _ = tokio::time::timeout(
                        std::time::Duration::from_millis(200),
                        sock.read(&mut buf),
                    )
                    .await;
                    let _ = sock
                        .write_all(
                            b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
                        )
                        .await;
                });
            }
        });
        Counter { port, seen }
    }

    fn connections(&self) -> usize {
        self.seen.load(Ordering::SeqCst)
    }
}

struct Sweep {
    client: McpClient,
    counter: Counter,
    _dir: tempfile::TempDir,
}

/// An `IrisTools` whose default connection is the listener, configured by an operator file in a
/// temporary home, and an MCP client in front of it.
async fn sweep(setting: Setting, toolset: Toolset) -> (Sweep, IrisTools) {
    let counter = Counter::bind().await;
    let url = format!("http://127.0.0.1:{}", counter.port);
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path().join("home");
    let cwd = dir.path().join("proj");
    std::fs::create_dir_all(&home).unwrap();
    std::fs::create_dir_all(&cwd).unwrap();
    std::fs::write(home.join(CONFIG_FILE_NAME), setting.toml()).unwrap();
    let inputs = LoadInputs {
        cwd: std::fs::canonicalize(&cwd).unwrap(),
        home: Some(std::fs::canonicalize(&home).unwrap()),
        ..Default::default()
    };
    let snapshot = ConfigSnapshot::load(&inputs);
    let declared = snapshot.declared_gates();
    let tools = IrisTools::with_registry_and_toolset(
        Some(conn(&url, SystemMode::Development)),
        SkillRegistry::new(),
        toolset,
        None,
        None,
        false, // no_skills: the skill tools are part of what is swept
        declared,
    )
    .unwrap();
    tools.set_config(inputs, snapshot);
    let mut pool = ConnectionPool::builder();
    pool.add("dev".to_string(), conn(&url, SystemMode::Development), true);
    *tools.pool.write().unwrap() = Arc::new(pool.build());
    let client = McpClient::start(tools.clone()).await;
    (
        Sweep {
            client,
            counter,
            _dir: dir,
        },
        tools,
    )
}

// ── arguments from the schema ───────────────────────────────────────────────────────────────────

fn schema_type(schema: &Value) -> Option<String> {
    match &schema["type"] {
        Value::String(t) => Some(t.clone()),
        Value::Array(ts) => ts
            .iter()
            .filter_map(|t| t.as_str())
            .find(|t| *t != "null")
            .map(str::to_string),
        _ => schema["anyOf"]
            .as_array()
            .and_then(|alts| alts.iter().filter_map(schema_type).find(|t| t != "null")),
    }
}

fn enum_values(schema: &Value) -> Vec<Value> {
    if let Some(e) = schema["enum"].as_array() {
        return e.clone();
    }
    schema["anyOf"]
        .as_array()
        .map(|alts| alts.iter().flat_map(enum_values).collect())
        .unwrap_or_default()
}

/// A value of the property's type: its first enum value, else `1`/`false`/`[]`/`{}`, else a string
/// that is a legal name for the properties that have a grammar.
fn default_value(name: &str, schema: &Value) -> Value {
    if let Some(first) = enum_values(schema).into_iter().next() {
        return first;
    }
    match schema_type(schema).as_deref() {
        Some("integer") | Some("number") => json!(schema["minimum"].as_i64().unwrap_or(1).max(1)),
        Some("boolean") => json!(false),
        Some("array") => json!([]),
        Some("object") => json!({}),
        _ => match name {
            "namespace" | "from_namespace" | "to_namespace" => json!("USER"),
            "host" => json!("127.0.0.1"),
            "url" | "base_url" => json!("http://127.0.0.1:1"),
            _ => json!("x"),
        },
    }
}

/// The required properties of a tool, each given a value of its type.
fn required_args(input_schema: &Value) -> Map<String, Value> {
    let props = input_schema["properties"].as_object();
    let mut args = Map::new();
    for key in input_schema["required"].as_array().into_iter().flatten() {
        let Some(key) = key.as_str() else { continue };
        let schema = props.and_then(|p| p.get(key)).unwrap_or(&Value::Null);
        args.insert(key.to_string(), default_value(key, schema));
    }
    args
}

/// Every call worth making on a tool: the required arguments alone, and once per action the
/// table gives a class of its own. The action goes in whichever of `action`, `mode` the tool
/// advertises, so each tier of a dispatcher meets its gate.
fn calls_for(tool: &str, input_schema: &Value) -> Vec<Map<String, Value>> {
    let mut base = required_args(input_schema);
    // A tool that names the server to judge in an argument of its own (`iris_test_server name=`)
    // has nothing to judge until it is told which; the registered one is the default.
    if let Some(TargetSource::Name(arg)) = tool_class(tool).map(|r| r.target) {
        base.insert(arg.to_string(), json!("dev"));
    }
    let mut calls = vec![base.clone()];
    let props = input_schema["properties"].as_object();
    let key = ["action", "mode"]
        .into_iter()
        .find(|k| props.is_some_and(|p| p.contains_key(*k)));
    if let (Some(key), Some(row)) = (key, tool_class(tool)) {
        for (action, _) in row.actions {
            let mut args = base.clone();
            args.insert(key.to_string(), json!(action));
            // The required `action` already took its first enum value in the base call.
            if !calls.contains(&args) {
                calls.push(args);
            }
        }
    }
    calls
}

// ── what the table permits ──────────────────────────────────────────────────────────────────────

/// Whether the table lets this call reach the server under the setting.
fn may_connect(setting: Setting, tool: &str, args: &Map<String, Value>) -> bool {
    let Some(row) = tool_class(tool) else {
        return false;
    };
    if row.kind == TargetKind::Local {
        return false;
    }
    let class = classify(tool, Some(args)).unwrap_or(WriteClass::Write);
    if class != WriteClass::ReadOnly {
        return false;
    }
    match setting {
        Setting::Strict => !row.scratch,
        Setting::LiveNarrowAllow => {
            let categories = effective_category(tool, Some(args), class);
            !categories.is_empty() && categories.iter().all(|c| *c == ToolCategory::Search)
        }
        Setting::WritesOff => true,
    }
}

/// Codes a gate refuses with. A call the table says is refused must end in one of them: any other
/// error means the call never reached the gate and the sweep checked nothing.
const REFUSALS: &[&str] = &[
    "WRITE_TOOLS_DISABLED",
    "DESTRUCTIVE_TOOLS_DISABLED",
    "DESTRUCTIVE_REQUIRES_WRITES",
    "READ_ONLY_STRICT",
    "READ_ONLY_STRICT_WITH_AUDIT",
    "POLICY_GATE",
    "ENV_GATE_BLOCKED",
    "DATA_POLICY_BLOCKED",
];

struct Outcome {
    failures: Vec<String>,
    calls: usize,
    refused: usize,
    connected: usize,
}

async fn run(setting: Setting, toolset: Toolset) -> Outcome {
    let (mut s, tools) = sweep(setting, toolset).await;
    let skipped: BTreeSet<&str> = SKIPPED.iter().map(|(t, _)| *t).collect();
    let mut out = Outcome {
        failures: Vec::new(),
        calls: 0,
        refused: 0,
        connected: 0,
    };
    for entry in tools.tool_catalogue() {
        if skipped.contains(entry.name.as_str()) {
            continue;
        }
        for args in calls_for(&entry.name, &entry.input_schema) {
            let before = s.counter.connections();
            let t0 = std::time::Instant::now();
            if std::env::var_os("IAD_ENUM_TRACE").is_some() {
                eprintln!(
                    "call {setting:?} {toolset:?} {} {}",
                    entry.name,
                    Value::Object(args.clone())
                );
            }
            let body = s
                .client
                .call(&entry.name, Value::Object(args.clone()))
                .await;
            if std::env::var_os("IAD_ENUM_TRACE").is_some() {
                eprintln!("  -> {:?} {}", t0.elapsed(), short(&body));
            }
            // A connection the handler opened is counted before its call returns; give a task it
            // spawned and did not await a turn to land.
            tokio::task::yield_now().await;
            tokio::task::yield_now().await;
            let connected = s.counter.connections() - before;
            let code = body["error_code"].as_str().unwrap_or("");
            let allowed = may_connect(setting, &entry.name, &args);
            let what = format!(
                "{setting:?} {toolset:?} {} {}",
                entry.name,
                Value::Object(args.clone())
            );
            out.calls += 1;
            if connected > 0 {
                out.connected += 1;
                if !allowed {
                    let kind = tool_class(&entry.name)
                        .map(|r| format!("{:?}", r.kind))
                        .unwrap_or_default();
                    out.failures.push(format!(
                        "{what}: opened {connected} connection(s) to the server; the table says \
                         this call ({kind}) may not (answer: {})",
                        short(&body)
                    ));
                }
            }
            if !allowed {
                if REFUSALS.contains(&code) {
                    out.refused += 1;
                } else if connected == 0
                    && tool_class(&entry.name).map(|r| r.kind) == Some(TargetKind::Iris)
                {
                    // An Iris call the table refuses has to be refused by a gate, not by an
                    // argument check or a handler that happened to fail before connecting.
                    out.failures.push(format!(
                        "{what}: the table says a gate refuses this call and it ended in {code:?} \
                         (answer: {})",
                        short(&body)
                    ));
                }
            }
        }
    }
    out
}

fn short(body: &Value) -> String {
    let s = body.to_string();
    s.chars().take(240).collect()
}

async fn assert_clean(toolset: Toolset) {
    let mut failures = Vec::new();
    let (mut calls, mut refused, mut connected) = (0, 0, 0);
    for setting in Setting::ALL {
        let out = run(setting, toolset).await;
        failures.extend(out.failures);
        calls += out.calls;
        refused += out.refused;
        connected += out.connected;
    }
    // The sweep has to have exercised both sides, or a broken rig would pass it.
    assert!(
        calls > 150,
        "only {calls} calls were made: the sweep did not run"
    );
    assert!(refused > 50, "only {refused} calls were refused by a gate");
    assert!(
        connected > 0,
        "no call reached the listener: it is not the default connection, so a leak could not show"
    );
    assert!(
        failures.is_empty(),
        "{} call(s) crossed a gate or lied about their class:\n  {}",
        failures.len(),
        failures.join("\n  ")
    );
}

#[tokio::test]
async fn no_baseline_tool_reaches_the_server_past_its_gate() {
    assert_clean(Toolset::Baseline).await;
}

#[tokio::test]
async fn no_merged_tool_reaches_the_server_past_its_gate() {
    assert_clean(Toolset::Merged).await;
}

// ── the skip list ───────────────────────────────────────────────────────────────────────────────

#[test]
fn the_skip_list_only_shrinks() {
    assert!(
        SKIPPED.len() <= SKIP_BUDGET,
        "SKIPPED holds {} tools and SKIP_BUDGET is {SKIP_BUDGET}. A tool the sweep does not call \
         is a tool nobody checks: give it arguments the sweep can send, or argue for it in review.",
        SKIPPED.len()
    );
    let registered: BTreeSet<String> = [Toolset::Baseline, Toolset::Merged, Toolset::Nostub]
        .into_iter()
        .flat_map(|t| {
            IrisTools::new_with_toolset(None, t)
                .unwrap()
                .registered_tool_names()
        })
        .collect();
    for (tool, reason) in SKIPPED {
        assert!(
            registered.contains(*tool),
            "{tool} is skipped and not registered"
        );
        assert!(reason.len() >= 20, "{tool} is skipped without a reason");
        assert!(
            CLASSIFICATION.iter().any(|r| r.tool == *tool),
            "{tool} is skipped and has no row"
        );
    }
}

/// The control: a read in a permitted category does reach the listener, so "no connection" in the
/// sweep means the pipeline stopped the call and not that the listener is unplugged.
#[tokio::test]
async fn a_permitted_read_does_reach_the_listener() {
    let (mut s, _tools) = sweep(Setting::LiveNarrowAllow, Toolset::Merged).await;
    let before = s.counter.connections();
    let _ = s
        .client
        .call(
            "iris_search",
            json!({"query": "x", "documents": ["Pkg.*.cls"]}),
        )
        .await;
    tokio::task::yield_now().await;
    assert!(
        s.counter.connections() > before,
        "iris_search is permitted by allow = [\"search\"] and did not connect"
    );
    // And a read outside `allow` does not.
    let before = s.counter.connections();
    let body = s
        .client
        .call("iris_query", json!({"query": "SELECT 1"}))
        .await;
    tokio::task::yield_now().await;
    assert_eq!(s.counter.connections(), before, "{body}");
    assert_eq!(body["error_code"], "POLICY_GATE", "{body}");
}

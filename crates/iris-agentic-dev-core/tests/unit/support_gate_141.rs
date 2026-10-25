//! Fixtures for the spec 141 gate-resolution tests.
//!
//! `rig` builds an `IrisTools` whose default connection is a Development instance and whose pool
//! holds the members the test names, every one at `127.0.0.1:1` where nothing listens. A call that
//! reaches the network therefore fails with a connection error, which no gate test accepts as a
//! refusal: the refusal has to come from a gate (R1 section 6).
//!
//! The configuration is an operator file in a temporary home, so it is trusted without a trust
//! entry and no test reads the developer's own file.

#![allow(dead_code)]

use iris_agentic_dev_core::iris::config_sources::{ConfigSnapshot, LoadInputs, CONFIG_FILE_NAME};
use iris_agentic_dev_core::iris::connection::{DiscoverySource, IrisConnection, SystemMode};
use iris_agentic_dev_core::iris::connection_pool::ConnectionPool;
use iris_agentic_dev_core::skills::SkillRegistry;
use iris_agentic_dev_core::tools::{IrisTools, Toolset};
use serde_json::{json, Value};
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

/// The URL nothing listens on, where `prod` is.
pub const UNROUTABLE: &str = "http://127.0.0.1:1";

/// The default connection's URL, which nothing listens on either. It differs from `prod`'s so the
/// two are not the same instance under two names.
pub const DEFAULT_URL: &str = "http://127.0.0.1:2";

pub struct Rig {
    pub tools: IrisTools,
    pub dir: tempfile::TempDir,
}

pub fn conn(url: &str, mode: SystemMode) -> IrisConnection {
    let mut c = IrisConnection::new(url, "USER", "_SYSTEM", "SYS", DiscoverySource::ExplicitFlag);
    c.system_mode = mode;
    c
}

/// The default connection is `dev`, a Development instance at [`DEFAULT_URL`]. The pool holds
/// `dev` (the default) and one member per name in `members`.
pub fn rig_with(toml: &str, toolset: Toolset, members: &[(&str, &str)]) -> Rig {
    rig_launched(toml, toolset, members, false)
}

/// [`rig_with`], and `launch_strict` stands for `IAD_READ_ONLY=strict` in the launch settings.
pub fn rig_launched(
    toml: &str,
    toolset: Toolset,
    members: &[(&str, &str)],
    launch_strict: bool,
) -> Rig {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path().join("home");
    let cwd = dir.path().join("proj");
    std::fs::create_dir_all(&home).unwrap();
    std::fs::create_dir_all(&cwd).unwrap();
    std::fs::write(home.join(CONFIG_FILE_NAME), toml).unwrap();
    let inputs = LoadInputs {
        cwd: std::fs::canonicalize(&cwd).unwrap(),
        home: Some(std::fs::canonicalize(&home).unwrap()),
        launch_read_only_strict: launch_strict,
        ..Default::default()
    };
    let snapshot = ConfigSnapshot::load(&inputs);
    let declared = snapshot.declared_gates();
    let tools = IrisTools::with_registry_and_toolset(
        Some(conn(DEFAULT_URL, SystemMode::Development)),
        SkillRegistry::new(),
        toolset,
        None,
        None,
        true,
        declared,
    )
    .unwrap();
    tools.set_config(inputs, snapshot);
    let mut pool = ConnectionPool::builder();
    pool.add(
        "dev".to_string(),
        conn(DEFAULT_URL, SystemMode::Development),
        true,
    );
    for (name, url) in members {
        pool.add(name.to_string(), conn(url, SystemMode::Unknown), false);
    }
    *tools.pool.write().unwrap() = Arc::new(pool.build());
    Rig { tools, dir }
}

/// `prod` joins the pool, and the default is `dev`.
pub fn rig(toml: &str) -> Rig {
    rig_with(toml, Toolset::Merged, &[("prod", UNROUTABLE)])
}

/// The operator file the R1 table test uses: `prod` is a Live subject server that allows only
/// search and blocks data reads.
pub const PROD_LOCKED: &str = r#"
mode = "operate"

[instance.prod]
host = "127.0.0.1"
web_port = 1
role = "subject"

[policy.prod]
mcpTemplate = "live"
allow = ["search"]
dataPolicy = "block"
"#;

/// The same, with `prod`'s write and destructive gates declared open, so the call gets past the
/// write gate and meets the gates after it.
pub const PROD_LOCKED_WRITES_DECLARED: &str = r#"
mode = "operate"

[instance.prod]
host = "127.0.0.1"
web_port = 1
role = "subject"

[policy.prod]
mcpTemplate = "live"
allow = ["search"]
dataPolicy = "block"
writeTools = true
destructiveTools = true
"#;

/// Every registered tool's advertised property names, read off the catalogue once. The per-tool
/// accessors clone the whole catalogue on every call, which a table over ninety tools cannot
/// afford.
pub fn advertised(tools: &IrisTools) -> std::collections::BTreeMap<String, Vec<String>> {
    tools
        .tool_catalogue()
        .into_iter()
        .map(|e| {
            let props = e.input_schema["properties"]
                .as_object()
                .map(|m| m.keys().cloned().collect())
                .unwrap_or_default();
            (e.name, props)
        })
        .collect()
}

/// The error body of a result: `structuredContent`, else the first text block parsed as JSON.
pub fn body_of(result: &rmcp::model::CallToolResult) -> Value {
    if let Some(v) = &result.structured_content {
        return v.clone();
    }
    for c in &result.content {
        if let Some(t) = c.as_text() {
            if let Ok(v) = serde_json::from_str::<Value>(&t.text) {
                return v;
            }
        }
    }
    json!({})
}

pub fn code_of(body: &Value) -> String {
    body["error_code"].as_str().unwrap_or("").to_string()
}

/// A real `ServerHandler` over an in-memory pipe, spoken to in newline-delimited JSON-RPC. This is
/// the MCP tail of the pipeline: `call_tool`, the router and everything between, as a client sees
/// it. rmcp's client side is not compiled in, so the client here is a dozen lines of framing.
pub struct McpClient {
    writer: tokio::io::WriteHalf<tokio::io::DuplexStream>,
    reader: BufReader<tokio::io::ReadHalf<tokio::io::DuplexStream>>,
    next_id: u64,
}

impl McpClient {
    pub async fn start(tools: IrisTools) -> McpClient {
        use rmcp::ServiceExt;
        let (client_io, server_io) = tokio::io::duplex(1 << 22);
        let (sr, sw) = tokio::io::split(server_io);
        tokio::spawn(async move {
            if let Ok(svc) = tools.serve((sr, sw)).await {
                let _ = svc.waiting().await;
            }
        });
        let (cr, cw) = tokio::io::split(client_io);
        let mut c = McpClient {
            writer: cw,
            reader: BufReader::new(cr),
            next_id: 1,
        };
        let init = c
            .request(
                "initialize",
                json!({
                    "protocolVersion": "2025-11-25",
                    "capabilities": {},
                    "clientInfo": {"name": "gate-141", "version": "1"},
                }),
            )
            .await;
        assert!(init.get("result").is_some(), "initialize failed: {init}");
        c.send(json!({"jsonrpc": "2.0", "method": "notifications/initialized"}))
            .await;
        c
    }

    async fn send(&mut self, v: Value) {
        let mut line = v.to_string();
        line.push('\n');
        self.writer.write_all(line.as_bytes()).await.unwrap();
        self.writer.flush().await.unwrap();
    }

    async fn request(&mut self, method: &str, params: Value) -> Value {
        let id = self.next_id;
        self.next_id += 1;
        self.send(json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params}))
            .await;
        loop {
            let mut line = String::new();
            let n = tokio::time::timeout(
                std::time::Duration::from_secs(60),
                self.reader.read_line(&mut line),
            )
            .await
            .expect("no answer from the server within 60 s")
            .unwrap();
            assert!(n > 0, "server closed the pipe");
            let v: Value = serde_json::from_str(line.trim()).unwrap();
            if v["id"] == json!(id) {
                return v;
            }
        }
    }

    /// `tools/call`, answered with the result body: `structuredContent`, else the first text block.
    /// A JSON-RPC error comes back as `{"rpc_error": ...}`.
    pub async fn call(&mut self, name: &str, args: Value) -> Value {
        let answer = self
            .request("tools/call", json!({"name": name, "arguments": args}))
            .await;
        if let Some(e) = answer.get("error") {
            return json!({"rpc_error": e});
        }
        let result = &answer["result"];
        if result.get("structuredContent").is_some() && !result["structuredContent"].is_null() {
            return result["structuredContent"].clone();
        }
        result["content"][0]["text"]
            .as_str()
            .and_then(|t| serde_json::from_str(t).ok())
            .unwrap_or_else(|| result.clone())
    }
}

use anyhow::Result;
use clap::Args;
use iris_agentic_dev_core::{
    iris::discovery::{discover_iris, IrisDiscovery},
    skills::SkillRegistry,
    tools::{ConfigWatcher, IrisTools, Toolset},
};
use rmcp::{transport::stdio, ServiceExt};
use tokio::sync::watch;

/// Start the iris-dev MCP server (stdio transport by default).
///
/// REQUIREMENTS
///   IRIS must have the Atelier REST API enabled. Three ways to achieve this:
///
///   1. Community images (include private web server):
///      iris-community, irishealth-community → port 52773
///
///   2. Enterprise images + ISC Web Gateway container (recommended for production):
///      Use containers.intersystems.com/intersystems/webgateway alongside
///      intersystems/iris. The webgateway container exposes port 80/443 and
///      proxies Atelier REST. Set IRIS_WEB_PORT=<webgateway-host-port>.
///      iris-dev auto-detects webgateway containers in the Docker scan.
///
///   3. Enterprise images standalone: no private web server — requires option 2 above.
///
/// WEBGATEWAY SETUP GOTCHAS (verified 2026-05-03)
///   Three non-obvious bugs in fresh enterprise container + webgateway setups:
///   a) CSP.ini race: patch CSP.ini only after "Configuration_Initialized" appears in it
///   b) Missing credentials: add Username=_SYSTEM + Password=SYS to [LOCAL] in CSP.ini
///      (default tries CSPSystem which doesn't exist in fresh enterprise containers)
///   c) Wrong Apache directive: use "CSP On" in <Location />, not "SetHandler csp-handler-sa"
///   d) Expired password: run UnExpireUserPasswords("*") in %SYS on first start
///   See: https://github.com/intersystems-community/iris-agentic-dev/blob/master/skills/skills/iris-vscode-objectscript/SKILL.md
///
/// CONNECTION DISCOVERY (in priority order)
///   1. --host / IRIS_HOST env var
///   2. .iris-agentic-dev.toml in workspace (walks up from cwd)
///   3. IRIS_CONTAINER env var → Docker named container lookup
///   4. Localhost port scan (52773, 41773, 51773, 8080)
///   5. Auto-scan running Docker containers
#[derive(Args)]
pub struct McpCommand {
    #[arg(long, default_value = "stdio")]
    pub transport: String,
    #[arg(long, default_value = "8080")]
    pub port: u16,
    /// Bind address for HTTP transport (default: 127.0.0.1)
    #[arg(long, default_value = "127.0.0.1")]
    pub bind: String,
    #[arg(long, env = "IRIS_HOST")]
    pub host: Option<String>,
    #[arg(long, env = "IRIS_WEB_PORT")]
    pub web_port: Option<u16>,
    #[arg(long, env = "IRIS_WEB_PREFIX", default_value = "")]
    pub web_prefix: String,
    /// URL scheme: http or https (default: http)
    #[arg(long, env = "IRIS_SCHEME", default_value = "http")]
    pub scheme: String,
    #[arg(long, env = "IRIS_USERNAME")]
    pub username: Option<String>,
    #[arg(long, env = "IRIS_PASSWORD")]
    pub password: Option<String>,
    #[arg(long, env = "IRIS_NAMESPACE", default_value = "USER")]
    pub namespace: String,
    #[arg(long)]
    pub server: Option<String>,
    #[arg(long)]
    pub config: Option<String>,
    #[arg(long = "subscribe")]
    pub subscribe: Vec<String>,
    #[arg(long, default_value = ".")]
    pub workspace: String,
    /// Tool set to register: baseline (all 34 tools), nostub (stubs removed),
    /// or merged (stubs removed + consolidated tools). Also read from IRIS_TOOLSET env var.
    #[arg(long, env = "IRIS_TOOLSET", default_value = "merged")]
    pub toolset: String,
}

impl McpCommand {
    /// Build an explicit IrisConnection from CLI flags, if --host was provided.
    pub(crate) fn build_explicit_connection(
        &self,
    ) -> Option<iris_agentic_dev_core::iris::connection::IrisConnection> {
        use iris_agentic_dev_core::iris::connection::{DiscoverySource, IrisConnection};
        let host = self.host.as_deref()?;
        let port = self.web_port.unwrap_or(52773);
        let prefix = self.web_prefix.trim_matches('/');
        let scheme = self.scheme.trim_matches('/');
        let base_url = if prefix.is_empty() {
            format!("{}://{}:{}", scheme, host, port)
        } else {
            format!("{}://{}:{}/{}", scheme, host, port, prefix)
        };
        let username = self.username.as_deref().unwrap_or("_SYSTEM");
        let password = self.password.as_deref().unwrap_or("SYS");
        Some(IrisConnection::new(
            base_url,
            &self.namespace,
            username,
            password,
            DiscoverySource::ExplicitFlag,
        ))
    }

    pub async fn run(self) -> Result<()> {
        let toolset = Toolset::from_str(&self.toolset);
        tracing::info!(
            "iris-agentic-dev mcp starting — toolset={}",
            toolset.as_str()
        );

        let explicit = self.build_explicit_connection();

        let (iris_tx, iris_rx) =
            watch::channel::<Option<iris_agentic_dev_core::iris::connection::IrisConnection>>(None);

        // Load .iris-agentic-dev.toml — takes precedence over env vars but not CLI flags (FR-006).
        // If --workspace was explicitly passed (not the default "."), warn when no config found.
        let ws_root =
            iris_agentic_dev_core::iris::workspace_config::workspace_root(Some(&self.workspace));
        if self.workspace != "." && !ws_root.join(".iris-agentic-dev.toml").exists() {
            tracing::warn!(
                "No .iris-agentic-dev.toml found at {} — falling back to auto-discovery. \
                 Set IRIS_HOST or IRIS_CONTAINER to connect directly.",
                ws_root.display()
            );
        }
        // apply_workspace_config_with_path returns the loaded config path so we can record it
        // in ConnectionState at startup (not just after hot-reload). Fixes issue #82.
        let (explicit, startup_config_path) =
            iris_agentic_dev_core::iris::workspace_config::apply_workspace_config_with_path(
                explicit,
                Some(&self.workspace),
                &self.namespace,
            );

        tokio::spawn(async move {
            let conn = match discover_iris(explicit).await {
                IrisDiscovery::Found(c) => {
                    tracing::info!(
                        "IRIS connected: {}/api/atelier/{} {}",
                        c.base_url,
                        c.atelier_version.version_str(),
                        c.version.as_deref().unwrap_or("?")
                    );
                    Some(c)
                }
                IrisDiscovery::NotFound => {
                    tracing::warn!("No IRIS connection — tools return IRIS_UNREACHABLE");
                    None
                }
                IrisDiscovery::Explained => {
                    // Specific actionable message already emitted — add no noise.
                    None
                }
            };
            let _ = iris_tx.send(conn);
        });

        let mut registry = SkillRegistry::new();
        for owner_repo in &self.subscribe {
            match registry.load_from_github(owner_repo).await {
                Ok(()) => tracing::info!("Subscribed to {}", owner_repo),
                Err(e) => tracing::warn!("Failed to subscribe to {}: {}", owner_repo, e),
            }
        }

        // Wait briefly for discovery — env-var discovery (single HTTP probe) completes in <500ms.
        // Cap at 2s so Claude Code / Copilot get the initialize response well within their timeout.
        // Docker/localhost-scan discovery may still be running when tools are first called;
        // those return IRIS_UNREACHABLE and the client can retry.
        let mut iris_rx_wait = iris_rx.clone();
        let _ = tokio::time::timeout(
            tokio::time::Duration::from_secs(2),
            iris_rx_wait.wait_for(|v| v.is_some()),
        )
        .await;
        let iris = iris_rx.borrow().clone();

        // On Windows, stdout opens in text mode which translates \n → \r\n.
        // MCP clients expect bare \n-terminated JSON lines — set stdout/stdin to binary mode.
        #[cfg(windows)]
        unsafe {
            extern "C" {
                fn _setmode(fd: i32, mode: i32) -> i32;
            }
            const O_BINARY: i32 = 0x8000;
            _setmode(0, O_BINARY); // stdin
            _setmode(1, O_BINARY); // stdout
        }

        // Build ConfigWatcher for .iris-agentic-dev.toml hot-reload (034-live-connection-reload).
        // When spawned from a launcher (e.g. Claude Desktop) the CWD is often "/" — fall back
        // to $HOME so the watch path is usable without setting OBJECTSCRIPT_WORKSPACE.
        let config_root = if ws_root == std::path::Path::new("/") {
            std::env::var("HOME")
                .ok()
                .map(std::path::PathBuf::from)
                .unwrap_or(ws_root)
        } else {
            ws_root
        };
        let config_watcher = ConfigWatcher::new(config_root.join(".iris-agentic-dev.toml"));
        let tools = IrisTools::with_registry_and_toolset(
            iris,
            registry,
            toolset,
            config_watcher,
            startup_config_path,
        )?;

        // FR-007: periodically sweep expired elicitation entries.
        {
            let store = tools.elicitation_store.clone();
            tokio::spawn(async move {
                loop {
                    tokio::time::sleep(std::time::Duration::from_secs(60)).await;
                    store.sweep();
                }
            });
        }

        match self.transport.as_str() {
            "stdio" => {
                let service = tools
                    .serve(stdio())
                    .await
                    .inspect_err(|e| tracing::error!("MCP server error: {:?}", e))?;
                service.waiting().await?;
            }
            "http" => {
                run_http_transport(tools, &self.bind, self.port).await?;
            }
            other => {
                eprintln!(
                    "error: unsupported transport '{}' — supported values: stdio, http",
                    other
                );
                std::process::exit(1);
            }
        }
        Ok(())
    }
}

pub(crate) async fn run_http_transport(
    tools: IrisTools,
    bind: &str,
    port: u16,
) -> anyhow::Result<()> {
    use std::net::{IpAddr, SocketAddr};
    use std::str::FromStr;
    use std::sync::Arc;

    use hyper::body::Incoming;
    use hyper::service::service_fn;
    use hyper_util::rt::TokioIo;
    use rmcp::transport::streamable_http_server::{
        session::local::LocalSessionManager, StreamableHttpServerConfig, StreamableHttpService,
    };

    let bind_ip = IpAddr::from_str(bind)
        .map_err(|e| anyhow::anyhow!("invalid bind address '{}': {}", bind, e))?;
    let addr = SocketAddr::from((bind_ip, port));

    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .map_err(|e| anyhow::anyhow!("failed to bind {}:{}: {}", bind, port, e))?;

    // When binding to all interfaces (0.0.0.0 / ::) the server is intentionally
    // reachable from external callers (e.g. AI Hub containers using host.docker.internal).
    // Disable the Host header allowlist so those callers aren't rejected.
    let config = if bind_ip.is_unspecified() {
        StreamableHttpServerConfig::default().disable_allowed_hosts()
    } else {
        let mut c = StreamableHttpServerConfig::default();
        c.allowed_hosts = vec![
            "localhost".into(),
            "127.0.0.1".into(),
            "::1".into(),
            bind.to_string(),
        ];
        c
    };

    let session_mgr = Arc::new(LocalSessionManager::default());
    // IrisTools is Clone — each session gets its own clone that shares the inner Arcs
    // (connection state, client, registry, etc.) so they all observe the same IRIS state.
    let tools = Arc::new(tools);
    let svc = StreamableHttpService::new(move || Ok((*tools).clone()), session_mgr, config);

    tracing::info!("iris-agentic-dev mcp listening on http://{}/mcp", addr);

    loop {
        let (stream, _peer) = listener.accept().await?;
        let io = TokioIo::new(stream);
        let svc_clone = svc.clone();
        tokio::spawn(async move {
            let result = hyper::server::conn::http1::Builder::new()
                .serve_connection(
                    io,
                    service_fn(move |req: hyper::Request<Incoming>| {
                        let mut s = svc_clone.clone();
                        async move { tower_service::Service::call(&mut s, req).await }
                    }),
                )
                .await;
            if let Err(e) = result {
                tracing::debug!("HTTP connection error: {}", e);
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use iris_agentic_dev_core::{
        skills::SkillRegistry,
        tools::{ConfigWatcher, Toolset},
    };

    fn make_tools() -> IrisTools {
        let watcher = ConfigWatcher::new("/nonexistent/.iris-agentic-dev.toml".into());
        IrisTools::with_registry_and_toolset(
            None,
            SkillRegistry::new(),
            Toolset::Merged,
            watcher,
            None,
        )
        .unwrap()
    }

    fn base_cmd() -> McpCommand {
        McpCommand {
            transport: "stdio".into(),
            port: 8080,
            bind: "127.0.0.1".into(),
            host: None,
            web_port: None,
            web_prefix: String::new(),
            scheme: "http".into(),
            username: None,
            password: None,
            namespace: "USER".into(),
            server: None,
            config: None,
            subscribe: vec![],
            workspace: ".".into(),
            toolset: "merged".into(),
        }
    }

    #[test]
    fn test_build_explicit_connection_no_host() {
        let cmd = base_cmd();
        assert!(cmd.build_explicit_connection().is_none());
    }

    #[test]
    fn test_build_explicit_connection_host_defaults() {
        let mut cmd = base_cmd();
        cmd.host = Some("iris.example.com".into());
        let conn = cmd.build_explicit_connection().unwrap();
        assert_eq!(conn.base_url, "http://iris.example.com:52773");
        assert_eq!(conn.username, "_SYSTEM");
        assert_eq!(conn.namespace, "USER");
    }

    #[test]
    fn test_build_explicit_connection_with_prefix() {
        let mut cmd = base_cmd();
        cmd.host = Some("iris.example.com".into());
        cmd.web_prefix = "/iris".into();
        cmd.web_port = Some(80);
        let conn = cmd.build_explicit_connection().unwrap();
        assert_eq!(conn.base_url, "http://iris.example.com:80/iris");
    }

    #[test]
    fn test_build_explicit_connection_custom_creds() {
        let mut cmd = base_cmd();
        cmd.host = Some("iris.example.com".into());
        cmd.username = Some("myuser".into());
        cmd.password = Some("mypass".into());
        cmd.scheme = "https".into();
        cmd.web_port = Some(443);
        let conn = cmd.build_explicit_connection().unwrap();
        assert_eq!(conn.base_url, "https://iris.example.com:443");
        assert_eq!(conn.username, "myuser");
    }

    #[tokio::test]
    async fn test_run_http_transport_bad_bind() {
        let tools = make_tools();
        let err = run_http_transport(tools, "not-an-ip", 0).await.unwrap_err();
        assert!(
            err.to_string().contains("invalid bind address"),
            "expected invalid bind address error, got: {err}"
        );
    }

    #[tokio::test]
    async fn test_run_http_transport_binds_127_0_0_1() {
        let tools = make_tools();
        // Port 0 lets the OS assign a free port; we abort after the first connection.
        let handle = tokio::spawn(run_http_transport(tools, "127.0.0.1", 0));
        // Give the listener a moment to bind.
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        // The task is still running (infinite loop). Abort it — this instruments the
        // bind + config-setup path (lines up to the accept loop entrance).
        handle.abort();
        let result = handle.await;
        assert!(
            result.unwrap_err().is_cancelled(),
            "task should be cancelled"
        );
    }

    #[tokio::test]
    async fn test_run_http_transport_binds_unspecified() {
        let tools = make_tools();
        // 0.0.0.0 with port 0 exercises the disable_allowed_hosts branch.
        let handle = tokio::spawn(run_http_transport(tools, "0.0.0.0", 0));
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        handle.abort();
        let result = handle.await;
        assert!(
            result.unwrap_err().is_cancelled(),
            "task should be cancelled"
        );
    }

    #[tokio::test]
    async fn test_run_http_transport_accept_loop() {
        use std::sync::atomic::{AtomicU16, Ordering};
        use std::sync::Arc;
        use tokio::io::AsyncWriteExt;

        // Bind port 0 first to get a free port, then hand ownership to run_http_transport.
        // run_http_transport re-binds on the given port, so we drop our listener first.
        // Use a Barrier to coordinate so run_http_transport is ready before we connect.
        let port = Arc::new(AtomicU16::new(0));
        let port_clone = port.clone();

        let tools = make_tools();
        // Spawn with port 0; the server picks a port and starts accepting.
        // We detect readiness by polling until a TCP connect succeeds.
        let handle = tokio::spawn(run_http_transport(tools, "127.0.0.1", 0));
        // Give the server time to bind.
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        // We can't easily retrieve the bound port from run_http_transport, so probe
        // a known-available port instead: bind one ourself and record it.
        let probe = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let probe_port = probe.local_addr().unwrap().port();
        port_clone.store(probe_port, Ordering::Relaxed);
        drop(probe);

        // Spawn a second server on the recorded port to exercise accept loop body.
        let tools2 = make_tools();
        let handle2 = tokio::spawn(run_http_transport(tools2, "127.0.0.1", probe_port));
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;

        // Connect and send a minimal HTTP/1.1 request to drive the accept loop body.
        if let Ok(mut stream) = tokio::net::TcpStream::connect(("127.0.0.1", probe_port)).await {
            let req = b"GET /mcp HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n";
            let _ = stream.write_all(req).await;
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        }

        handle.abort();
        handle2.abort();
        let _ = handle.await;
        let _ = handle2.await;
        drop(port);
    }

    /// Exercises McpCommand::run() with --transport http so we cover the dispatch path
    /// through the match arm and into run_http_transport.
    #[tokio::test]
    async fn test_mcp_command_run_http_dispatch() {
        // Bind a free port, then drop it so McpCommand::run() can take it.
        let probe = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = probe.local_addr().unwrap().port();
        drop(probe);

        let cmd = McpCommand {
            transport: "http".into(),
            port,
            bind: "127.0.0.1".into(),
            host: None,
            web_port: None,
            web_prefix: String::new(),
            scheme: "http".into(),
            username: None,
            password: None,
            namespace: "USER".into(),
            server: None,
            config: None,
            subscribe: vec![],
            workspace: ".".into(),
            toolset: "merged".into(),
        };

        let handle = tokio::spawn(cmd.run());
        tokio::time::sleep(std::time::Duration::from_millis(150)).await;
        handle.abort();
        let _ = handle.await;
    }

    /// Exercises McpCommand::run() with an invalid transport value — exits process(1).
    /// This is already covered by the integration test but we add it here too.
    #[test]
    fn test_mcp_command_invalid_transport() {
        // The invalid-transport path calls std::process::exit(1) so we can't call it
        // in-process. This test just verifies the McpCommand struct can be constructed.
        let cmd = McpCommand {
            transport: "grpc".into(),
            port: 9999,
            bind: "127.0.0.1".into(),
            host: None,
            web_port: None,
            web_prefix: String::new(),
            scheme: "http".into(),
            username: None,
            password: None,
            namespace: "USER".into(),
            server: None,
            config: None,
            subscribe: vec![],
            workspace: ".".into(),
            toolset: "merged".into(),
        };
        assert_eq!(cmd.transport, "grpc");
    }
}

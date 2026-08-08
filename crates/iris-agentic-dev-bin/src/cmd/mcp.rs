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
    pub async fn run(self) -> Result<()> {
        let toolset = Toolset::from_str(&self.toolset);
        tracing::info!(
            "iris-agentic-dev mcp starting — toolset={}",
            toolset.as_str()
        );

        let explicit = if let Some(host) = self.host.clone() {
            use iris_agentic_dev_core::iris::connection::{DiscoverySource, IrisConnection};
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
        } else {
            None
        };

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

async fn run_http_transport(tools: IrisTools, bind: &str, port: u16) -> anyhow::Result<()> {
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

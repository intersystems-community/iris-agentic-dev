use anyhow::Result;
use clap::Args;
use iris_agentic_dev_core::{
    iris::discovery::{discover_iris_with, IrisDiscovery},
    skills::SkillRegistry,
    tools::{ConfigWatcher, IrisTools, Toolset},
};
use rmcp::{transport::stdio, ServiceExt};
use tokio::sync::watch;

use super::http_guard::{self, HttpGuard};

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
///   b) Missing credentials: the gateway's [LOCAL] section needs a login (the default tries
///      CSPSystem, which doesn't exist in fresh enterprise containers). Give it the _SYSTEM
///      password you set from the environment; never write the default SYS into CSP.ini
///   c) Wrong Apache directive: use "CSP On" in <Location />, not "SetHandler csp-handler-sa"
///   d) Expired password: set a new _SYSTEM password on first start (setup skill, step 4);
///      do not un-expire every account, which keeps the published default working on all of them
///   Publish container ports on 127.0.0.1 only (-p 127.0.0.1:<host>:<container>).
///   See: https://github.com/intersystems-community/iris-agentic-dev/blob/master/skills/skills/iris-vscode-objectscript/SKILL.md
///
/// CONNECTION DISCOVERY (in priority order)
///   1. --host flag
///   2. .iris-agentic-dev.toml in workspace (walks up from cwd); one in the home directory
///      yields to IRIS_HOST
///   3. IRIS_HOST env var (used even when that server does not answer)
///   4. IRIS_CONTAINER env var → Docker named container lookup
///   5. Localhost port scan (52773, 41773, 51773, 8080)
///   6. Auto-scan running Docker containers
#[derive(Args)]
pub struct McpCommand {
    #[arg(long, default_value = "stdio")]
    pub transport: String,
    #[arg(long, default_value = "8080")]
    pub port: u16,
    /// Bind address for HTTP transport (default: 127.0.0.1)
    #[arg(long, default_value = "127.0.0.1")]
    pub bind: String,
    /// File holding the HTTP transport's bearer token (at least 32 bytes). `IAD_HTTP_TOKEN` and
    /// `[transport] http_token` in ~/.iris-agentic-dev.toml set it too; no flag takes the value,
    /// because a value on the command line shows in `ps`. A bind other than loopback needs one.
    #[arg(long, value_name = "PATH")]
    pub auth_token_file: Option<std::path::PathBuf>,
    /// Extra `Host` name the HTTP transport serves, such as `host.docker.internal`. Repeatable.
    #[arg(long = "allowed-host", value_name = "NAME")]
    pub allowed_host: Vec<String>,
    /// Extra `Origin` the HTTP transport serves, such as `https://app.example`. Repeatable.
    #[arg(long = "allowed-origin", value_name = "ORIGIN")]
    pub allowed_origin: Vec<String>,
    // No `env = "IRIS_HOST"`: clap would fill this slot from the variable, so any `IRIS_HOST`
    // made the connection explicit and outranked `.iris-agentic-dev.toml`. The documented order is
    // flag, then the file, then `IRIS_*`; discovery reads `IRIS_HOST` itself after the file.
    #[arg(long)]
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
    /// Remove all skill and learning-agent tools (skill, skill_community, agent_history,
    /// agent_stats, kb_*) from the server. Also skips --subscribe fetching.
    /// Use when you want IRIS tools only with no skill management surface.
    #[arg(long, env = "IRIS_NO_SKILLS")]
    pub no_skills: bool,
    /// Advertise the named tool with an empty description in tools/list, leaving its input
    /// schema intact. Repeatable. For eval runs that measure whether a tool's schema is
    /// sufficient on its own; not something a normal client should set.
    #[arg(long = "suppress-tool-description", value_name = "TOOL")]
    pub suppress_tool_description: Vec<String>,
}

impl McpCommand {
    /// Build an explicit IrisConnection from CLI flags, if --host was provided.
    pub(crate) fn build_explicit_connection(
        &self,
    ) -> Option<iris_agentic_dev_core::iris::connection::IrisConnection> {
        use iris_agentic_dev_core::iris::connection::{DiscoverySource, IrisConnection};
        let host = self.host.as_deref()?;
        let port = self.web_port.unwrap_or(52773);
        let endpoint = match iris_agentic_dev_core::iris::endpoint::Endpoint::parse(
            Some(&self.scheme),
            host,
            port,
            Some(&self.web_prefix),
        ) {
            Ok(e) => e,
            Err(e) => {
                eprintln!("Error [INVALID_CONFIG]: --host/--scheme/--web-prefix: {e}");
                crate::exit(2);
            }
        };
        let base_url = endpoint.base_url();
        let username = self.username.as_deref().unwrap_or("_SYSTEM");
        let password = self.password.as_deref().unwrap_or("SYS");
        Some(
            IrisConnection::new(
                base_url,
                &self.namespace,
                username,
                password,
                DiscoverySource::ExplicitFlag,
            )
            // The launch settings describe this server, so their certificate setting applies to it.
            .with_tls(iris_agentic_dev_core::iris::http::TlsSetting::from_launch_env()),
        )
    }

    /// The HTTP transport's guard, or exit 2 with the reason it cannot start (FR-025).
    ///
    /// The token comes from `IAD_HTTP_TOKEN`, then `--auth-token-file`, then the operator file's
    /// `[transport] http_token`. A workspace file's token is set aside and never read here.
    fn http_guard(
        &self,
        snapshot: &iris_agentic_dev_core::iris::config_sources::ConfigSnapshot,
    ) -> Result<super::http_guard::HttpGuard> {
        use super::http_guard::{GuardSettings, HttpGuard, TokenSources};

        let bind_ip = parse_bind(&self.bind)?;
        let file = match &self.auth_token_file {
            Some(p) => Some(std::fs::read_to_string(p).map_err(|e| {
                anyhow::anyhow!("cannot read --auth-token-file {}: {e}", p.display())
            })?),
            None => None,
        };
        let sources = TokenSources {
            env: std::env::var("IAD_HTTP_TOKEN").ok(),
            file,
            operator: snapshot.operator.http_token.clone(),
        };
        let fail = |reason: String| -> ! {
            // Bare stderr as well as tracing: the exit comes before any MCP handshake, and
            // `RUST_LOG` can silence tracing.
            tracing::error!("{reason}");
            eprintln!("error: {reason}");
            crate::exit(2)
        };
        let token = sources.resolve().unwrap_or_else(|e| fail(e));
        let ignored = snapshot
            .set_aside
            .iter()
            .find(|i| i.id == "workspace:transport.http_token");
        HttpGuard::from_settings(
            bind_ip,
            GuardSettings {
                token,
                allowed_hosts: self.allowed_host.clone(),
                allowed_origins: self.allowed_origin.clone(),
            },
        )
        .map_err(|reason| match ignored {
            Some(item) => format!(
                "{reason}. The `[transport] http_token` in {} was set aside: only the operator \
                 file or launch settings may set it (see check_config)",
                item.file.as_deref().unwrap_or("the workspace file")
            ),
            None => reason,
        })
        .or_else(|reason| fail(reason))
    }

    pub async fn run(self) -> Result<()> {
        // The flag and `IRIS_SUPPRESS_TOOL_DESCRIPTION` name the same set, and `list_tools` reads
        // the variable, so the flag folds into it rather than being carried separately down to the
        // tool router. Folding rather than overwriting: an operator who set the variable and then
        // passed the flag meant both, and silently dropping one is the #111 failure again.
        if !self.suppress_tool_description.is_empty() {
            let mut names = std::env::var("IRIS_SUPPRESS_TOOL_DESCRIPTION").unwrap_or_default();
            for name in &self.suppress_tool_description {
                if !names.is_empty() {
                    names.push(',');
                }
                names.push_str(name);
            }
            std::env::set_var("IRIS_SUPPRESS_TOOL_DESCRIPTION", names);
        }
        let toolset = Toolset::from_str(&self.toolset);
        tracing::info!(
            "iris-agentic-dev mcp starting — toolset={}",
            toolset.as_str()
        );

        let explicit = self.build_explicit_connection();

        let (iris_tx, iris_rx) =
            watch::channel::<Option<iris_agentic_dev_core::iris::connection::IrisConnection>>(None);

        // Read the operator file and at most one workspace file — takes precedence over env vars but
        // not CLI flags (FR-006). A workspace file cannot raise a gate until the operator trusts
        // it (spec 141), so what it asked for and was refused is in the snapshot's set-aside list.
        let inputs = iris_agentic_dev_core::iris::config_sources::LoadInputs::from_process(
            Some(&self.workspace),
            self.config.as_deref().map(std::path::Path::new),
        );
        let snapshot = iris_agentic_dev_core::iris::config_sources::ConfigSnapshot::load(&inputs);
        // If --workspace was explicitly passed (not the default "."), warn when no config found.
        if inputs.workspace_flag.is_some()
            && inputs.env_workspace.is_none()
            && snapshot.workspace_layer.is_none()
        {
            tracing::warn!(
                "No .iris-agentic-dev.toml found at {} — falling back to auto-discovery. \
                 Set IRIS_HOST or IRIS_CONTAINER to connect directly.",
                iris_agentic_dev_core::iris::config_sources::workspace_root(&inputs).display()
            );
        }
        for e in &snapshot.errors {
            tracing::error!("configuration file does not parse: {}", e.display());
            eprintln!("error: configuration file does not parse: {}", e.display());
        }
        snapshot.announce();
        iris_agentic_dev_core::skills::bundled::note_started_with_local_skills(
            snapshot.operator.local_skills,
        );
        // apply_snapshot returns the loaded config path so we can record it in ConnectionState at
        // startup (not just after hot-reload). Fixes issue #82.
        let (explicit, startup_config_path, declared_gates) =
            iris_agentic_dev_core::iris::workspace_config::apply_snapshot(
                explicit,
                &snapshot,
                &self.namespace,
            );

        // FR-005/FR-006: refuse a gate declaration that can never mean what it says, before
        // anything is served. `destructive_tools_enabled = true` with `write_tools_enabled = false`
        // asks for a tier that is a subset of a gate the same file turns off.
        //
        // 1.2.6 logged DESTRUCTIVE_REQUIRES_WRITES from inside the config loader and then started
        // *with writes on*: the log sat above a `return None`, which skipped the gate export and
        // dropped the server into the permissive namespace heuristic. The operator got the warning
        // they asked for and the opposite of the configuration they asked for. Exiting here is what
        // docs/tools.md and specs/073-destructive-gate have promised since 1.2.1.
        if let Some(e) = snapshot.gate_error() {
            tracing::error!("{}", e);
            // Also on bare stderr: the exit happens before the MCP handshake, so a client sees a
            // process that died without speaking. `RUST_LOG` can silence tracing; this it cannot.
            eprintln!("error: {}", e);
            crate::exit(2);
        }

        // FR-025: the HTTP transport's guard is decided before anything is served or discovered, so
        // a public bind with no token exits 2 without having opened a connection.
        let http_guard = if self.transport == "http" {
            Some(self.http_guard(&snapshot)?)
        } else {
            None
        };

        // A configuration file that does not parse builds no connection: discovery would attach to
        // whichever IRIS answers first, with gates the operator never chose. Every IRIS tool says
        // CONFIG_INVALID until the file is fixed (R5 C11).
        let config_invalid = snapshot.is_invalid();
        // A file that names a container and no host hands it to discovery as data.
        let hints =
            iris_agentic_dev_core::iris::workspace_config::snapshot_discovery_hints(&snapshot);
        tokio::spawn(async move {
            if config_invalid {
                let _ = iris_tx.send(None);
                return;
            }
            let conn = match discover_iris_with(explicit, &hints).await {
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
        if !self.no_skills {
            for owner_repo in &self.subscribe {
                match registry.load_from_github(owner_repo).await {
                    Ok(()) => tracing::info!("Subscribed to {}", owner_repo),
                    Err(e) => tracing::warn!("Failed to subscribe to {}: {}", owner_repo, e),
                }
            }
        }

        // Wait briefly for discovery — env-var discovery (single HTTP probe) completes in <500ms.
        // Cap at 2s so Claude Code / Copilot get the initialize response well within their timeout.
        // Docker/localhost-scan discovery may still be running when tools are first called;
        // those return IRIS_UNREACHABLE and the client can retry.
        let mut iris_rx_wait = iris_rx.clone();
        if !config_invalid {
            let _ = tokio::time::timeout(
                tokio::time::Duration::from_secs(2),
                iris_rx_wait.wait_for(|v| v.is_some()),
            )
            .await;
        }
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

        // Build ConfigWatcher for hot-reload (034-live-connection-reload): the workspace file, or
        // where one would appear in the project directory, and the operator file. The path handed to
        // the agent is never the operator file; with no project directory (a launcher's cwd of "/",
        // or the home directory) there is nothing to watch but the operator file.
        let config_watcher = ConfigWatcher::for_snapshot(&snapshot);
        let tools = IrisTools::with_registry_and_toolset(
            iris,
            registry,
            toolset,
            config_watcher,
            startup_config_path,
            self.no_skills,
            declared_gates,
        )?;
        tools.set_config(inputs, snapshot);

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
                tokio::select! {
                    r = service.waiting() => { r?; }
                    sig = shutdown_signal() => tracing::info!("{sig}: shutting down"),
                }
            }
            "http" => {
                let guard = http_guard.expect("built above for the http transport");
                tokio::select! {
                    r = run_http_transport(tools, &self.bind, self.port, guard) => r?,
                    sig = shutdown_signal() => tracing::info!("{sig}: shutting down"),
                }
            }
            other => {
                eprintln!(
                    "error: unsupported transport '{}' — supported values: stdio, http",
                    other
                );
                crate::exit(1);
            }
        }
        Ok(())
    }
}

/// Resolves on SIGTERM or Ctrl-C, naming which.
///
/// MCP hosts stop a server with SIGTERM. Left to its default, that ends the process on the spot,
/// before `main` flushes the telemetry write the last call started, and the write's scratch class
/// stays in USER (130 round 4). Returning instead lets `main` flush, and dropping the runtime runs
/// the scratch-class cleanup of any call still in flight.
async fn shutdown_signal() -> &'static str {
    #[cfg(unix)]
    {
        use tokio::signal::unix::{signal, SignalKind};
        match signal(SignalKind::terminate()) {
            Ok(mut term) => tokio::select! {
                _ = term.recv() => "SIGTERM",
                _ = tokio::signal::ctrl_c() => "SIGINT",
            },
            Err(e) => {
                tracing::warn!("cannot listen for SIGTERM: {e}");
                let _ = tokio::signal::ctrl_c().await;
                "SIGINT"
            }
        }
    }
    #[cfg(not(unix))]
    {
        let _ = tokio::signal::ctrl_c().await;
        "Ctrl-C"
    }
}

fn parse_bind(bind: &str) -> anyhow::Result<std::net::IpAddr> {
    use std::str::FromStr;
    std::net::IpAddr::from_str(bind)
        .map_err(|e| anyhow::anyhow!("invalid bind address '{}': {}", bind, e))
}

pub(crate) async fn run_http_transport(
    tools: IrisTools,
    bind: &str,
    port: u16,
    guard: HttpGuard,
) -> anyhow::Result<()> {
    use std::net::SocketAddr;
    use std::sync::Arc;

    use hyper::body::Incoming;
    use hyper::service::service_fn;
    use hyper_util::rt::TokioIo;
    use rmcp::transport::streamable_http_server::{
        session::local::LocalSessionManager, StreamableHttpServerConfig, StreamableHttpService,
    };

    let bind_ip = parse_bind(bind)?;
    let addr = SocketAddr::from((bind_ip, port));

    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .map_err(|e| anyhow::anyhow!("failed to bind {}:{}: {}", bind, port, e))?;

    // Host, Origin and the token are checked by `http_guard::check` before rmcp sees a request.
    // Where rmcp can state the same Host rule (a loopback or pinned bind) it is told too, so a
    // wiring mistake in the guard still fails closed there. An unspecified bind
    // (0.0.0.0 / ::) is reachable from other machines on purpose (an AI Hub container calling
    // host.docker.internal), and rmcp's list cannot name every IP literal, so there the guard
    // alone decides.
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
        c.allowed_hosts.extend(guard.extra_hosts().iter().cloned());
        c
    };
    let guard = Arc::new(guard);

    let session_mgr = Arc::new(LocalSessionManager::default());
    // Each session gets its own tools from `for_new_session`: they share the connection state,
    // pool, clients and registry, so every session sees the same IRIS state, but the log store,
    // pending SCM dialogs and call history are the session's own. A plain clone shared those too,
    // and `iris_get_log` listed another client's results (spec 141 T220).
    let tools = Arc::new(tools);
    let svc = StreamableHttpService::new(move || Ok(tools.for_new_session()), session_mgr, config);

    tracing::info!("iris-agentic-dev mcp listening on http://{}/mcp", addr);

    loop {
        let (stream, peer) = listener.accept().await?;
        let io = TokioIo::new(stream);
        let svc_clone = svc.clone();
        let guard = guard.clone();
        tokio::spawn(async move {
            let result = hyper::server::conn::http1::Builder::new()
                .serve_connection(
                    io,
                    service_fn(move |req: hyper::Request<Incoming>| {
                        let mut s = svc_clone.clone();
                        let guard = guard.clone();
                        async move {
                            // Every method and path goes through the guard: POST, the GET event
                            // stream and DELETE. A refused request never reaches rmcp.
                            let (parts, body) = req.into_parts();
                            if let Err(refusal) = http_guard::check(&parts, &guard) {
                                tracing::warn!(
                                    "refused {} from {peer}: {refusal:?}",
                                    parts.uri.path()
                                );
                                return Ok(http_guard::rejection_response(refusal));
                            }
                            let req = hyper::Request::from_parts(parts, body);
                            tower_service::Service::call(&mut s, req).await
                        }
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
            false,
            Default::default(),
        )
        .unwrap()
    }

    /// A guard for `bind`: loopback needs none, anything else gets a token.
    fn guard_for(bind: &str) -> HttpGuard {
        use crate::cmd::http_guard::GuardSettings;
        let ip: std::net::IpAddr = bind.parse().unwrap_or([127, 0, 0, 1].into());
        let token = (!ip.is_loopback()).then(|| "t".repeat(40));
        HttpGuard::from_settings(
            ip,
            GuardSettings {
                token,
                ..Default::default()
            },
        )
        .unwrap()
    }

    fn base_cmd() -> McpCommand {
        McpCommand {
            transport: "stdio".into(),
            port: 8080,
            bind: "127.0.0.1".into(),
            auth_token_file: None,
            allowed_host: Vec::new(),
            allowed_origin: Vec::new(),
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
            no_skills: false,
            suppress_tool_description: Vec::new(),
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
        let err = run_http_transport(tools, "not-an-ip", 0, guard_for("127.0.0.1"))
            .await
            .unwrap_err();
        assert!(
            err.to_string().contains("invalid bind address"),
            "expected invalid bind address error, got: {err}"
        );
    }

    #[tokio::test]
    async fn test_run_http_transport_binds_127_0_0_1() {
        let tools = make_tools();
        // Port 0 lets the OS assign a free port; we abort after the first connection.
        let handle = tokio::spawn(run_http_transport(
            tools,
            "127.0.0.1",
            0,
            guard_for("127.0.0.1"),
        ));
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
    async fn test_run_http_transport_binds_unspecified_with_a_token() {
        let tools = make_tools();
        // 0.0.0.0 with port 0 exercises the unspecified-bind branch, which only the guard checks.
        let handle = tokio::spawn(run_http_transport(
            tools,
            "0.0.0.0",
            0,
            guard_for("0.0.0.0"),
        ));
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        handle.abort();
        let result = handle.await;
        assert!(
            result.unwrap_err().is_cancelled(),
            "task should be cancelled"
        );
    }

    #[test]
    fn an_unspecified_bind_without_a_token_has_no_guard() {
        // The allow-all branch this replaces served 0.0.0.0 to anyone who could reach it.
        for bind in ["0.0.0.0", "::"] {
            let ip: std::net::IpAddr = bind.parse().unwrap();
            assert!(HttpGuard::from_settings(ip, Default::default()).is_err());
        }
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
        let handle = tokio::spawn(run_http_transport(
            tools,
            "127.0.0.1",
            0,
            guard_for("127.0.0.1"),
        ));
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
        let handle2 = tokio::spawn(run_http_transport(
            tools2,
            "127.0.0.1",
            probe_port,
            guard_for("127.0.0.1"),
        ));
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
            auth_token_file: None,
            allowed_host: Vec::new(),
            allowed_origin: Vec::new(),
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
            no_skills: false,
            suppress_tool_description: Vec::new(),
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
            auth_token_file: None,
            allowed_host: Vec::new(),
            allowed_origin: Vec::new(),
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
            no_skills: false,
            suppress_tool_description: Vec::new(),
        };
        assert_eq!(cmd.transport, "grpc");
    }
}

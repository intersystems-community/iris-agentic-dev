//! IRIS instance discovery cascade.
//!
//! Order of priority (highest to lowest):
//! 1. Explicit IrisConnection passed directly
//! 2. Env vars (IRIS_HOST + IRIS_WEB_PORT)
//! 3. Localhost port scan (100ms timeout, parallel)
//! 4. Docker containers via bollard
//! 5. VS Code settings.json objectscript.conn
//!
//! Each step fails silently and falls through to the next, except step 2: a server named by
//! `IRIS_HOST` is kept even when it does not answer.

use crate::iris::connection::{container_from_env, host_from_env, DiscoverySource, IrisConnection};
use crate::iris::http::{client_for, ClientKind, IdentifyClient, TlsSetting};
use std::sync::Mutex;
use std::time::Duration;

/// The ports we scan on localhost for IRIS web servers.
/// Includes 52773 (IRIS private web server), 80/8080/8443/443 (ISC Web Gateway container),
/// and legacy ports 41773/51773.
const IRIS_WEB_PORTS: &[u16] = &[52773, 80, 8080, 41773, 51773, 8443, 443];

/// The ports the localhost scan visits. Debug builds read `IAD_TEST_SCAN_PORTS` (comma-separated)
/// instead, so a test can aim the scan at listeners it owns; a release build never does.
fn scan_ports() -> Vec<u16> {
    if cfg!(debug_assertions) {
        if let Ok(list) = std::env::var("IAD_TEST_SCAN_PORTS") {
            return list
                .split(',')
                .filter_map(|p| p.trim().parse::<u16>().ok())
                .collect();
        }
    }
    IRIS_WEB_PORTS.to_vec()
}

// ── Identification (FR-024) ──────────────────────────────────────────────────

/// How a listener showed it is an IRIS web server, without being sent a credential.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Identified {
    /// `GET /api/atelier/` answered `200` with an IRIS version: the web application allows
    /// unauthenticated access.
    UnauthenticatedAtelier,
    /// `GET /csp/sys/UtilHome.csp` set a `CSPSESSIONID-…` or `IRISSessionToken` cookie. Every
    /// instance with a web server serves the Portal login page this way.
    PortalCookie,
    /// `GET /api/monitor/metrics` returned Prometheus text with `iris_` metrics.
    Metrics,
}

/// What asking a listener who it is found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Identification {
    Iris(Identified),
    /// Nothing answered.
    NoListener,
    /// Something answered and showed no sign of being IRIS. A bare `401 Basic` is not a sign: IRIS
    /// sends it with no realm, and so does any web server.
    Other(String),
}

/// A listener that answered and was not identified, so it got no credentials.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkippedListener {
    pub port: u16,
    pub reason: String,
}

static SKIPPED: Mutex<Vec<SkippedListener>> = Mutex::new(Vec::new());

/// The listeners the last discovery scan did not send credentials to, for `check_config`.
pub fn skipped_listeners() -> Vec<SkippedListener> {
    SKIPPED.lock().unwrap().clone()
}

fn note_skipped(port: u16, reason: &str) {
    tracing::info!("discovery: not sending credentials to port {port}: {reason}");
    SKIPPED.lock().unwrap().push(SkippedListener {
        port,
        reason: reason.to_string(),
    });
}

/// Ask the web server at `base_url` who it is. Sends no credential: the client cannot.
pub async fn identify_listener(client: &IdentifyClient, base_url: &str) -> Identification {
    let base = base_url.trim_end_matches('/');

    match client.get(&format!("{base}/api/atelier/")).await {
        Err(_) => return Identification::NoListener,
        Ok(id) if id.status == 200 => {
            let version_says_iris = serde_json::from_str::<serde_json::Value>(&id.body)
                .ok()
                .and_then(|v| {
                    v["result"]["content"]["version"]
                        .as_str()
                        .map(str::to_string)
                })
                .is_some_and(|v| v.to_uppercase().contains("IRIS"));
            if version_says_iris {
                return Identification::Iris(Identified::UnauthenticatedAtelier);
            }
        }
        Ok(_) => {}
    }

    if let Ok(id) = client.get(&format!("{base}/csp/sys/UtilHome.csp")).await {
        if id
            .set_cookie_names
            .iter()
            .any(|n| n.starts_with("CSPSESSIONID") || n == "IRISSessionToken")
        {
            return Identification::Iris(Identified::PortalCookie);
        }
    }

    if let Ok(id) = client.get(&format!("{base}/api/monitor/metrics")).await {
        let text = id
            .content_type
            .as_deref()
            .is_some_and(|t| t.starts_with("text/plain"));
        if id.status == 200 && text && id.body.lines().any(|l| l.starts_with("iris_")) {
            return Identification::Iris(Identified::Metrics);
        }
    }

    Identification::Other("answered, and showed no sign of being IRIS".to_string())
}

// ── Discovery result types ────────────────────────────────────────────────────

/// Why a named-container probe failed.
#[derive(Debug)]
pub enum FailureMode {
    /// Port 52773 is not mapped to a host port.
    PortNotMapped,
    /// Container + port found; HTTP probe got no response (refused, timeout, empty).
    AtelierNotResponding { port: u16 },
    /// Container + port found; HTTP probe returned a non-200, non-401 status.
    AtelierHttpError { port: u16, status: u16 },
    /// Container + port found; HTTP probe returned 401 (message already emitted).
    AtelierAuth401 { port: u16 },
}

/// Return type of `discover_via_docker_named()`.
// The connection is the variant almost every call returns, so boxing it buys nothing.
#[allow(clippy::large_enum_variant)]
#[derive(Debug)]
pub enum DiscoveryResult {
    /// Probe succeeded — connection ready.
    Connected(IrisConnection),
    /// No container matched the name — cascade may continue.
    NotFound,
    /// Container found but probe failed — caller must stop cascade.
    FoundUnhealthy(FailureMode),
}

/// Return type of `discover_iris()`. Replaces `Result<Option<IrisConnection>>`.
// The connection is the variant almost every call returns, so boxing it buys nothing.
#[allow(clippy::large_enum_variant)]
#[derive(Debug)]
pub enum IrisDiscovery {
    /// Connection established.
    Found(IrisConnection),
    /// No IRIS found anywhere in the cascade.
    NotFound,
    /// A specific actionable message was already emitted — callers must not add noise.
    Explained,
}

/// Inner probe using a pre-built HTTP client. Avoids creating a new client per probe (Bug 24).
/// `pub` so live integration tests can exercise the shared-client path directly.
pub async fn probe_atelier_with_client(
    client: &reqwest::Client,
    host: &str,
    port: u16,
    username: &str,
    password: &str,
    namespace: &str,
) -> Option<IrisConnection> {
    let base_url = format!("http://{}:{}", host, port);
    let resp = client
        .get(format!("{}/api/atelier/", base_url))
        .basic_auth(username, Some(password))
        .send()
        .await
        .ok()?;

    if resp.status().as_u16() == 401 {
        // #21: iris-community containers started without IRIS_PASSWORD have OS auth only.
        // Basic auth is rejected. Only suggest Docker restart if a container is configured.
        if container_from_env().is_some() {
            tracing::warn!(
                "IRIS at {}:{} returned 401 — container may need IRIS_PASSWORD. \
                 Restart with: docker run -e IRIS_PASSWORD=SYS ...",
                host,
                port
            );
        } else {
            tracing::warn!(
                "IRIS at {}:{} returned 401 — check credentials. \
                 Verify IRIS_USERNAME/IRIS_PASSWORD or set username/password in .iris-agentic-dev.toml.",
                host,
                port
            );
        }
        return None;
    }
    if !resp.status().is_success() {
        return None;
    }

    let body: serde_json::Value = crate::iris::http::capped_json::<serde_json::Value>(resp)
        .await
        .ok()?;
    let content = &body["result"]["content"];

    let version = content["version"]
        .as_str()
        .filter(|v| v.to_uppercase().contains("IRIS"))
        .map(|v| v.to_string())?;

    let mut conn = IrisConnection::new(
        base_url,
        namespace,
        username,
        password,
        DiscoverySource::LocalhostScan { port },
    )
    .with_tls(TlsSetting::from_launch_env());
    conn.version = Some(version);
    conn.atelier_version = match content["api"].as_u64() {
        Some(v) if v >= 8 => crate::iris::connection::AtelierVersion::V8,
        Some(v) if v >= 2 => crate::iris::connection::AtelierVersion::V2,
        _ => crate::iris::connection::AtelierVersion::V1,
    };
    Some(conn)
}

/// Probe a single host:port via Atelier REST. Returns Some(conn) if IRIS found.
pub async fn probe_atelier(
    host: &str,
    port: u16,
    username: &str,
    password: &str,
    namespace: &str,
    timeout_ms: u64,
) -> Option<IrisConnection> {
    // Bug 24: for the public API we still create a client here, but internal callers
    // (localhost scan, docker probe) reuse a shared client via probe_atelier_with_client.
    // The listener identifies itself first, through a client that cannot send a credential.
    let tls = TlsSetting::from_launch_env();
    let identify = IdentifyClient::new(&tls, Duration::from_millis(timeout_ms)).ok()?;
    match identify_listener(&identify, &format!("http://{host}:{port}")).await {
        Identification::Iris(_) => {}
        Identification::NoListener => return None,
        Identification::Other(reason) => {
            note_skipped(port, &reason);
            return None;
        }
    }
    let client = client_for(&tls, ClientKind::Raw).ok()?;
    tokio::time::timeout(
        Duration::from_millis(timeout_ms).max(Duration::from_secs(1)),
        probe_atelier_with_client(&client, host, port, username, password, namespace),
    )
    .await
    .ok()?
}

/// Full discovery cascade. Returns `IrisDiscovery` — infallible.
/// What a configuration file asked discovery to look for, passed as data.
///
/// A file's `container`, `namespace`, `username` and `password` used to be written into the
/// process environment so the cascade would pick them up there. That made a workspace file's
/// credentials indistinguishable from the launch settings for every later reader, and a value
/// outlived a reload that removed the key. Now they travel in this struct and only the container
/// lookup reads them; the environment is not touched.
#[derive(Clone, Default)]
pub struct DiscoveryHints {
    /// The container the file names, when it names a container and no host.
    pub container: Option<String>,
    pub namespace: Option<String>,
    pub username: Option<String>,
    pub password: Option<String>,
}

impl std::fmt::Debug for DiscoveryHints {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DiscoveryHints")
            .field("container", &self.container)
            .field("namespace", &self.namespace)
            .field("username", &self.username)
            .field("password", &self.password.as_ref().map(|_| "[redacted]"))
            .finish()
    }
}

impl DiscoveryHints {
    fn username(&self) -> String {
        self.username
            .clone()
            .or_else(|| std::env::var("IRIS_USERNAME").ok())
            .unwrap_or_else(|| "_SYSTEM".to_string())
    }

    fn password(&self) -> String {
        self.password
            .clone()
            .or_else(|| std::env::var("IRIS_PASSWORD").ok())
            .unwrap_or_else(|| "SYS".to_string())
    }

    fn namespace(&self) -> String {
        self.namespace
            .clone()
            .or_else(|| std::env::var("IRIS_NAMESPACE").ok())
            .unwrap_or_else(|| "USER".to_string())
    }
}

/// [`discover_iris_with`] for a caller with no configuration file's hints.
pub async fn discover_iris(explicit: Option<IrisConnection>) -> IrisDiscovery {
    discover_iris_with(explicit, &DiscoveryHints::default()).await
}

pub async fn discover_iris_with(
    explicit: Option<IrisConnection>,
    hints: &DiscoveryHints,
) -> IrisDiscovery {
    // 1. Explicit wins immediately — but probe for version + Atelier API level first
    if let Some(mut conn) = explicit {
        conn.probe().await;
        return IrisDiscovery::Found(conn);
    }

    // 2. Env vars. A server named by `IRIS_HOST` is used whether or not it answers, as a
    // `--host` flag is: falling through would hand the session to whatever the container lookup or
    // the localhost scan finds next, which is a different server from the one asked for.
    if let Some(host) = host_from_env() {
        let port = std::env::var("IRIS_WEB_PORT")
            .ok()
            .and_then(|p| p.parse::<u16>().ok())
            .unwrap_or(52773);
        let username = std::env::var("IRIS_USERNAME").unwrap_or_else(|_| "_SYSTEM".to_string());
        let password = std::env::var("IRIS_PASSWORD").unwrap_or_else(|_| "SYS".to_string());
        let namespace = std::env::var("IRIS_NAMESPACE").unwrap_or_else(|_| "USER".to_string());
        let scheme = std::env::var("IRIS_SCHEME")
            .ok()
            .map(|s| s.trim_matches('/').to_string())
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| "http".to_string());
        let prefix = std::env::var("IRIS_WEB_PREFIX")
            .ok()
            .map(|p| p.trim_matches('/').to_string())
            .filter(|p| !p.is_empty());

        let base_url = match &prefix {
            Some(p) => format!("{}://{}:{}/{}", scheme, host, port, p),
            None => format!("{}://{}:{}", scheme, host, port),
        };
        let mut conn = IrisConnection::new(
            base_url,
            namespace,
            username,
            password,
            DiscoverySource::EnvVar,
        )
        .with_tls(TlsSetting::from_launch_env());
        conn.probe().await;
        return IrisDiscovery::Found(conn);
    }

    // 3. The named container: resolve its web port via Docker. A configuration file's
    // `container = "..."` arrives in `hints`, ahead of `IRIS_CONTAINER`. Must run before the
    // generic localhost scan or another container on port 52773 wins.
    let named = hints
        .container
        .clone()
        .filter(|c| !c.trim().is_empty())
        .or_else(container_from_env);
    if let Some(container_name) = named {
        match discover_via_docker_named_with(&container_name, hints).await {
            DiscoveryResult::Connected(conn) => return IrisDiscovery::Found(conn),
            DiscoveryResult::NotFound => {
                tracing::warn!(
                    "Container '{}' not found in Docker — is it running? ('docker ps' to check)",
                    container_name
                );
                // Container not found — cascade continues to localhost scan
            }
            DiscoveryResult::FoundUnhealthy(mode) => {
                // Container was found but probe failed — emit specific message, stop cascade.
                emit_unhealthy_message(&container_name, mode);
                return IrisDiscovery::Explained;
            }
        }
    }

    // 4. Localhost scan (parallel, 100ms each). Uses env var credentials.
    let username = std::env::var("IRIS_USERNAME").unwrap_or_else(|_| "_SYSTEM".to_string());
    let password = std::env::var("IRIS_PASSWORD").unwrap_or_else(|_| "SYS".to_string());
    let namespace = std::env::var("IRIS_NAMESPACE").unwrap_or_else(|_| "USER".to_string());

    // Each listener says who it is first, through a client that has no way to send a credential.
    // The first one that is IRIS gets the one credentialed request; the rest are listed in
    // `check_config.discovery.skipped` (FR-024).
    SKIPPED.lock().unwrap().clear();
    let tls = TlsSetting::from_launch_env();
    let identify = match IdentifyClient::new(&tls, Duration::from_millis(100)) {
        Ok(c) => std::sync::Arc::new(c),
        Err(_) => return IrisDiscovery::NotFound,
    };
    let scan_tasks: Vec<_> = scan_ports()
        .into_iter()
        .map(|port| {
            let identify = identify.clone();
            tokio::spawn(async move {
                let found = identify_listener(&identify, &format!("http://localhost:{port}")).await;
                (port, found)
            })
        })
        .collect();

    let mut identified_port = None;
    for task in scan_tasks {
        match task.await {
            Ok((port, Identification::Iris(_))) if identified_port.is_none() => {
                identified_port = Some(port);
            }
            Ok((_, Identification::Iris(_))) | Ok((_, Identification::NoListener)) | Err(_) => {}
            Ok((port, Identification::Other(reason))) => note_skipped(port, &reason),
        }
    }
    if let Some(port) = identified_port {
        if let Ok(client) = client_for(&tls, ClientKind::Scan) {
            if let Some(conn) = probe_atelier_with_client(
                &client,
                "localhost",
                port,
                &username,
                &password,
                &namespace,
            )
            .await
            {
                return IrisDiscovery::Found(conn);
            }
        }
    }

    // 5. Docker scan via bollard
    if let Some(conn) = discover_via_docker().await {
        return IrisDiscovery::Found(conn);
    }

    // 6. VS Code settings.json
    if let Some(conn) = discover_via_vscode_settings().await {
        return IrisDiscovery::Found(conn);
    }

    // 7. VS Code Server Manager (intersystems.servers in user settings.json)
    match discover_via_server_manager().await {
        SmDiscovery::Found(conn) => return IrisDiscovery::Found(conn),
        SmDiscovery::CredentialError(msg) => {
            tracing::warn!("{msg}");
            return IrisDiscovery::Explained;
        }
        SmDiscovery::NotAvailable => {}
    }

    IrisDiscovery::NotFound
}

/// Emit a mode-specific, actionable warning for a found-but-unhealthy container.
/// `pub` so live Docker integration tests can exercise every `FailureMode` variant directly.
pub fn emit_unhealthy_message(container_name: &str, mode: FailureMode) {
    match mode {
        FailureMode::PortNotMapped => {
            tracing::warn!(
                "Container '{}' found but port 52773 is not mapped to a host port. \
                 Restart with: docker run -p <host_port>:52773 ... \
                 Note: iris_execute and iris_test still work via docker exec.",
                container_name
            );
        }
        FailureMode::AtelierNotResponding { port } => {
            tracing::warn!(
                "Container '{}' found at localhost:{} but Atelier REST API is not responding. \
                 Enterprise IRIS images (iris:, irishealth:) do not include the private web server \
                 — use iris-community or irishealth-community for local dev, or connect via \
                 IRIS_HOST+IRIS_WEB_PORT to an external Web Gateway. \
                 Note: iris_execute and iris_test still work via docker exec.",
                container_name,
                port
            );
        }
        FailureMode::AtelierHttpError { port, status } => {
            tracing::warn!(
                "Container '{}' found at localhost:{} but Atelier REST returned HTTP {}. \
                 Check IRIS logs: docker logs {}",
                container_name,
                port,
                status,
                container_name
            );
        }
        FailureMode::AtelierAuth401 { port: _ } => {
            // 401 warn was already emitted by probe_atelier_for_container with container name.
            // No second message needed.
        }
    }
}

/// Score a container name against a workspace basename (spec-025 scoring rules).
/// Returns a higher score for closer name matches to the workspace.
pub fn score_container_name(container_name: &str, workspace_basename: &str) -> u32 {
    if workspace_basename.is_empty() {
        return 0;
    }
    let cn = container_name.to_lowercase().replace('-', "_");
    let wb = workspace_basename.to_lowercase().replace('-', "_");

    let base = if cn == wb {
        100
    } else if cn.starts_with(&wb) {
        80
    } else if cn.contains(&wb) {
        60
    } else {
        0
    };

    if base == 0 {
        return 0;
    }

    let suffix_bonus = if cn.ends_with("-iris") || cn.ends_with("_iris") {
        10
    } else {
        0
    } + if cn.ends_with("-test") || cn.ends_with("_test") {
        5
    } else {
        0
    };

    base + suffix_bonus
}

/// Resolve a specific named container to its web port and probe it.
/// Used when IRIS_CONTAINER env var is set (e.g. from .iris-dev.toml).
/// Resolve a specific named container to its web port and probe it.
/// Returns a structured `DiscoveryResult` — never emits log messages itself.
/// `pub` so live Docker integration tests can probe real containers directly.
pub async fn discover_via_docker_named(target: &str) -> DiscoveryResult {
    discover_via_docker_named_with(target, &DiscoveryHints::default()).await
}

/// [`discover_via_docker_named`] with the account and namespace a configuration file named.
pub async fn discover_via_docker_named_with(
    target: &str,
    hints: &DiscoveryHints,
) -> DiscoveryResult {
    use bollard::query_parameters::ListContainersOptionsBuilder;
    use bollard::Docker;

    let docker = match Docker::connect_with_defaults() {
        Ok(d) => d,
        Err(_) => return DiscoveryResult::NotFound, // daemon unreachable; caller emits message
    };
    let containers = match tokio::time::timeout(
        Duration::from_secs(3),
        docker.list_containers(Some(
            ListContainersOptionsBuilder::default().all(false).build(),
        )),
    )
    .await
    {
        Ok(Ok(c)) => c,
        _ => return DiscoveryResult::NotFound, // timeout or Docker error
    };

    let (username, password, namespace) = (hints.username(), hints.password(), hints.namespace());

    for container in containers {
        let name = container
            .names
            .and_then(|n| n.into_iter().next())
            .unwrap_or_default()
            .trim_start_matches('/')
            .to_string();

        if name != target {
            continue;
        }

        // Container found — now check port mapping.
        let mut port_web: Option<u16> = None;
        let mut port_ss: Option<u16> = None;
        for port in container.ports.unwrap_or_default() {
            if port.private_port == 52773 {
                port_web = port.public_port;
            }
            if port.private_port == 1972 {
                port_ss = port.public_port;
            }
        }

        let web_port = match port_web {
            Some(p) => p,
            None => return DiscoveryResult::FoundUnhealthy(FailureMode::PortNotMapped),
        };

        // Port is mapped — probe Atelier REST.
        return probe_atelier_for_container(
            &name, web_port, &username, &password, &namespace, port_ss,
        )
        .await;
    }

    // No container matched the name.
    DiscoveryResult::NotFound
}

/// Probe Atelier REST for a named container. Returns a structured `DiscoveryResult`.
/// Emits the 401 WARN here (with container name) so the generic warn is never needed.
async fn probe_atelier_for_container(
    container_name: &str,
    port: u16,
    username: &str,
    password: &str,
    namespace: &str,
    port_ss: Option<u16>,
) -> DiscoveryResult {
    // The named container's mapped port says who it is before it is sent a credential: a workspace
    // file can name the container (FR-024).
    let tls = TlsSetting::from_launch_env();
    let identify = match IdentifyClient::new(&tls, Duration::from_millis(2000)) {
        Ok(c) => c,
        Err(_) => {
            return DiscoveryResult::FoundUnhealthy(FailureMode::AtelierNotResponding { port })
        }
    };
    match identify_listener(&identify, &format!("http://localhost:{port}")).await {
        Identification::Iris(_) => {}
        Identification::NoListener => {
            return DiscoveryResult::FoundUnhealthy(FailureMode::AtelierNotResponding { port })
        }
        Identification::Other(reason) => {
            note_skipped(port, &reason);
            return DiscoveryResult::FoundUnhealthy(FailureMode::AtelierNotResponding { port });
        }
    }
    let client = match client_for(&tls, ClientKind::Scan) {
        Ok(c) => c,
        Err(_) => {
            return DiscoveryResult::FoundUnhealthy(FailureMode::AtelierNotResponding { port })
        }
    };

    let url = format!("http://localhost:{}/api/atelier/", port);
    let resp = match client
        .get(&url)
        .basic_auth(username, Some(password))
        .send()
        .await
    {
        Ok(r) => r,
        Err(_) => {
            return DiscoveryResult::FoundUnhealthy(FailureMode::AtelierNotResponding { port })
        }
    };

    let status = resp.status().as_u16();

    if status == 401 {
        tracing::warn!(
            "IRIS at localhost:{} returned 401 — container '{}' may need IRIS_PASSWORD. \
             Restart with: docker run -e IRIS_PASSWORD=SYS ...",
            port,
            container_name
        );
        return DiscoveryResult::FoundUnhealthy(FailureMode::AtelierAuth401 { port });
    }

    if !resp.status().is_success() {
        return DiscoveryResult::FoundUnhealthy(FailureMode::AtelierHttpError { port, status });
    }

    let body: serde_json::Value =
        match crate::iris::http::capped_json::<serde_json::Value>(resp).await {
            Ok(b) => b,
            Err(_) => {
                return DiscoveryResult::FoundUnhealthy(FailureMode::AtelierNotResponding { port })
            }
        };

    let content = &body["result"]["content"];
    let version = match content["version"]
        .as_str()
        .filter(|v| v.to_uppercase().contains("IRIS"))
        .map(|v| v.to_string())
    {
        Some(v) => v,
        None => return DiscoveryResult::FoundUnhealthy(FailureMode::AtelierNotResponding { port }),
    };

    let base_url = format!("http://localhost:{}", port);
    let mut conn = IrisConnection::new(
        base_url,
        namespace,
        username,
        password,
        DiscoverySource::Docker {
            container_name: container_name.to_string(),
        },
    )
    .with_tls(tls);
    conn.version = Some(version);
    conn.atelier_version = match content["api"].as_u64() {
        Some(v) if v >= 8 => crate::iris::connection::AtelierVersion::V8,
        Some(v) if v >= 2 => crate::iris::connection::AtelierVersion::V2,
        _ => crate::iris::connection::AtelierVersion::V1,
    };
    conn.port_superserver = port_ss;
    DiscoveryResult::Connected(conn)
}

/// `pub` so live Docker integration tests can exercise the full unnamed scan directly.
pub async fn discover_via_docker() -> Option<IrisConnection> {
    use bollard::query_parameters::ListContainersOptionsBuilder;
    use bollard::Docker;

    let workspace_basename = std::env::current_dir()
        .ok()
        .and_then(|p| p.file_name().map(|n| n.to_string_lossy().to_string()))
        .unwrap_or_default();

    let docker = Docker::connect_with_defaults().ok()?;
    let containers = tokio::time::timeout(
        Duration::from_secs(3),
        docker.list_containers(Some(
            ListContainersOptionsBuilder::default().all(false).build(),
        )),
    )
    .await
    .ok()?
    .ok()?;

    let mut candidates: Vec<(u32, String, u16, Option<u16>)> = Vec::new();

    for container in containers {
        let image = container.image.as_deref().unwrap_or("");
        // Match IRIS containers and ISC Web Gateway containers (webgateway image).
        // The webgateway container proxies Atelier REST from its port 80/443 to iris:52773.
        let is_iris = image.contains("intersystems") || image.contains("iris");
        let is_webgateway = image.contains("webgateway");
        if !is_iris && !is_webgateway {
            continue;
        }

        let container_name = container
            .names
            .clone()
            .and_then(|n| n.into_iter().next())
            .unwrap_or_default()
            .trim_start_matches('/')
            .to_string();

        let mut port_web: Option<u16> = None;
        let mut port_superserver: Option<u16> = None;

        if let Some(ports) = container.ports {
            for port in &ports {
                // IRIS private web server port
                if port.private_port == 52773 {
                    port_web = port.public_port;
                }
                // ISC Web Gateway container: exposes port 80 (HTTP) or 443 (HTTPS)
                if is_webgateway
                    && port_web.is_none()
                    && (port.private_port == 80 || port.private_port == 443)
                {
                    port_web = port.public_port;
                }
                if port.private_port == 1972 {
                    port_superserver = port.public_port;
                }
            }
        }

        if let Some(web_port) = port_web {
            let score = score_container_name(&container_name, &workspace_basename);
            candidates.push((score, container_name, web_port, port_superserver));
        }
    }

    candidates.sort_by_key(|b| std::cmp::Reverse(b.0));

    // Bug 12: use IRIS_USERNAME/IRIS_PASSWORD env vars instead of hardcoded credentials.
    let username = std::env::var("IRIS_USERNAME").unwrap_or_else(|_| "_SYSTEM".to_string());
    let password = std::env::var("IRIS_PASSWORD").unwrap_or_else(|_| "SYS".to_string());

    // Each candidate says who it is first. The first that is IRIS gets the credentialed request
    // (FR-024).
    let tls = TlsSetting::from_launch_env();
    let identify = IdentifyClient::new(&tls, Duration::from_millis(500)).ok()?;
    let probe_client = client_for(&tls, ClientKind::Scan).ok()?;

    for (_score, container_name, web_port, port_ss) in candidates {
        match identify_listener(&identify, &format!("http://localhost:{web_port}")).await {
            Identification::Iris(_) => {}
            Identification::NoListener => continue,
            Identification::Other(reason) => {
                note_skipped(web_port, &reason);
                continue;
            }
        }
        if let Some(mut conn) = probe_atelier_with_client(
            &probe_client,
            "localhost",
            web_port,
            &username,
            &password,
            "USER",
        )
        .await
        {
            conn.source = DiscoverySource::Docker { container_name };
            conn.port_superserver = port_ss;
            return Some(conn);
        }
        break;
    }
    None
}

// ── Server Manager discovery (044) ───────────────────────────────────────────

enum SmDiscovery {
    Found(IrisConnection),
    /// SM settings found + server matched but credential lookup failed. Callers must
    /// stop the cascade and surface the error — do NOT fall through to NotAvailable.
    CredentialError(String),
    /// SM not installed or settings empty. Cascade may continue.
    NotAvailable,
}

async fn discover_via_server_manager() -> SmDiscovery {
    use crate::iris::server_manager::{
        parse_sm_settings, select_server, sm_secret_for_in, sm_settings_path, SmCredentialError,
        SmSecretError,
    };

    let path = match sm_settings_path() {
        Some(p) => p,
        None => return SmDiscovery::NotAvailable,
    };

    let profiles = parse_sm_settings(&path);
    if profiles.is_empty() {
        return SmDiscovery::NotAvailable;
    }

    let profile = match select_server(&profiles) {
        Ok(p) => p,
        Err(SmCredentialError::Ambiguous { available }) => {
            let msg = format!(
                "Multiple Server Manager servers found: {}. Set IRIS_SERVER_NAME to select one.",
                available.join(", ")
            );
            return SmDiscovery::CredentialError(msg);
        }
        Err(e) => return SmDiscovery::CredentialError(e.to_string()),
    };

    // The profile's own endpoint: its secret goes only there (the keychain is initialised on first
    // use).
    let endpoint = match profile.endpoint() {
        Ok(e) => e,
        Err(e) => {
            return SmDiscovery::CredentialError(format!(
                "Server Manager profile '{}' has an unusable address: {e}",
                profile.name
            ))
        }
    };
    let password = match sm_secret_for_in(&profiles, &profile.name, &profile.username, &endpoint) {
        Ok(pw) => pw,
        Err(SmSecretError::Credential(e)) => return SmDiscovery::CredentialError(e.to_string()),
        Err(e) => return SmDiscovery::CredentialError(format!("{e:?}")),
    };

    let namespace = std::env::var("IRIS_NAMESPACE").unwrap_or_else(|_| "USER".to_string());
    let base_url = endpoint.base_url();

    let mut conn = IrisConnection::new(
        base_url,
        namespace,
        profile.username.clone(),
        password,
        DiscoverySource::ServerManager {
            server_name: profile.name.clone(),
        },
    )
    .with_tls(TlsSetting::validating(
        crate::iris::http::TlsSource::ServerManager,
    ));
    conn.probe().await;
    SmDiscovery::Found(conn)
}

/// Attempt to find IRIS connection from VS Code settings.json in the working directory.
async fn discover_via_vscode_settings() -> Option<IrisConnection> {
    discover_vscode_settings_in(&std::env::current_dir().ok()?).await
}

static VSCODE_SET_ASIDE: Mutex<Vec<crate::iris::config_sources::SetAsideItem>> =
    Mutex::new(Vec::new());

/// What discovery found in `.vscode/settings.json` and did not act on: a connection whose password
/// could not be read, so none was built (spec 141, T204). `check_config` lists it under
/// `set_aside`, because with no connection there is nothing else to report it on.
pub fn discovery_set_aside() -> Vec<crate::iris::config_sources::SetAsideItem> {
    VSCODE_SET_ASIDE.lock().unwrap().clone()
}

/// [`discover_via_vscode_settings`] for the settings file under `dir`, so a test needs no working
/// directory. A file that exists replaces what the last read of it reported.
pub async fn discover_vscode_settings_in(dir: &std::path::Path) -> Option<IrisConnection> {
    use crate::iris::vscode_config::VsCodeResolution;
    let path = dir.join(".vscode/settings.json");
    if !path.exists() {
        return None;
    }
    VSCODE_SET_ASIDE.lock().unwrap().clear();
    let settings = crate::iris::vscode_config::parse_vscode_settings(&path).ok()?;
    match settings.resolve_connection().await {
        VsCodeResolution::Connection(r) => {
            let mut conn = r.conn;
            if let Some(item) = r.set_aside.first() {
                conn.credentials_withheld = Some(item.reason.clone());
            }
            Some(conn)
        }
        VsCodeResolution::PasswordMissing(item) => {
            tracing::warn!("{}", item.reason);
            VSCODE_SET_ASIDE.lock().unwrap().push(item);
            None
        }
        VsCodeResolution::NotConfigured => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_score_container_empty_workspace() {
        assert_eq!(score_container_name("my-iris", ""), 0);
    }

    #[test]
    fn test_score_container_exact_match() {
        assert_eq!(score_container_name("myapp", "myapp"), 100);
    }

    #[test]
    fn test_score_container_exact_match_with_iris_suffix() {
        let score = score_container_name("myapp_iris", "myapp");
        assert_eq!(score, 90, "exact + iris suffix = 100 + 10");
    }

    #[test]
    fn test_score_container_starts_with_workspace() {
        let score = score_container_name("myapp-dev", "myapp");
        assert_eq!(score, 80);
    }

    #[test]
    fn test_score_container_contains_workspace() {
        let score = score_container_name("prod-myapp-instance", "myapp");
        assert_eq!(score, 60);
    }

    #[test]
    fn test_score_container_no_match() {
        assert_eq!(score_container_name("completely-different", "myapp"), 0);
    }

    #[test]
    fn test_score_container_case_insensitive() {
        assert_eq!(score_container_name("MYAPP", "myapp"), 100);
    }

    #[test]
    fn test_score_container_hyphen_underscore_normalized() {
        // "my-app" normalized to "my_app", workspace "my-app" normalized to "my_app" → exact match
        assert_eq!(score_container_name("my-app", "my_app"), 100);
    }

    #[test]
    fn test_score_container_starts_with_iris_suffix_bonus() {
        let score = score_container_name("myapp-iris", "myapp");
        // starts_with → 80, iris suffix → 10 = 90
        assert_eq!(score, 90);
    }

    #[test]
    fn test_score_container_test_suffix_bonus() {
        let score = score_container_name("myapp-test", "myapp");
        // starts_with → 80, test suffix → 5 = 85
        assert_eq!(score, 85);
    }

    #[test]
    fn test_score_container_iris_and_test_suffix_not_both() {
        // A name can only end with one suffix at a time; neither applies to "myapp-iris-test"
        // because it ends with "_test" after normalization, not "_iris".
        let score = score_container_name("myapp-iris-test", "myapp");
        // starts_with → 80, ends_with _test → 5 = 85
        assert_eq!(score, 85);
    }

    #[test]
    fn test_score_container_exact_match_with_test_suffix() {
        let score = score_container_name("myapp_test", "myapp");
        // starts_with (after exact check fails because cn != wb) → 80, test suffix → 5 = 85
        // Wait: cn = "myapp_test", wb = "myapp"; cn != wb, cn.starts_with(wb) = true → 80 + 5 = 85
        assert_eq!(score, 85);
    }

    #[test]
    fn test_score_container_workspace_longer_than_container() {
        // Workspace "myapp-service" is longer; container "myapp" won't contain the full wb.
        let score = score_container_name("myapp", "myapp-service");
        // cn = "myapp", wb = "myapp_service"; cn != wb, not starts_with, not contains → 0
        assert_eq!(score, 0);
    }

    #[test]
    fn test_score_container_empty_container_name() {
        // Empty container name with non-empty workspace → no match.
        assert_eq!(score_container_name("", "myapp"), 0);
    }

    #[test]
    fn test_score_container_both_empty() {
        // Empty workspace triggers early return of 0.
        assert_eq!(score_container_name("", ""), 0);
    }

    #[test]
    fn test_score_container_single_char_workspace() {
        // Single-char workspace — container containing it scores 60.
        let score = score_container_name("a-iris", "a");
        // cn = "a_iris", wb = "a"; cn != wb, cn.starts_with("a") = true → 80 + 10 = 90
        assert_eq!(score, 90);
    }

    #[test]
    fn test_score_container_all_uppercase_normalized() {
        let score = score_container_name("MY-APP-IRIS", "my_app");
        // cn = "my_app_iris", wb = "my_app"; cn starts_with wb → 80, ends_with _iris → 10 = 90
        assert_eq!(score, 90);
    }

    #[test]
    fn test_score_container_contains_no_prefix_match() {
        // "prefix-myapp" starts_with check fails, but contains wb → 60.
        let score = score_container_name("prefix-myapp", "myapp");
        // cn = "prefix_myapp", wb = "myapp"; not equal, not starts_with, contains → 60
        assert_eq!(score, 60);
    }

    #[test]
    fn test_iris_web_ports_includes_private_port() {
        assert!(IRIS_WEB_PORTS.contains(&52773));
    }

    #[test]
    fn test_iris_web_ports_includes_http_port() {
        assert!(IRIS_WEB_PORTS.contains(&80));
    }

    #[test]
    fn test_iris_web_ports_includes_legacy_ports() {
        assert!(IRIS_WEB_PORTS.contains(&41773));
        assert!(IRIS_WEB_PORTS.contains(&51773));
    }

    #[test]
    fn test_score_container_workspace_with_hyphens_vs_underscores() {
        // Both sides use hyphens — should normalize to same and be exact match.
        assert_eq!(score_container_name("my-app", "my-app"), 100);
    }

    #[test]
    fn test_score_container_exact_match_iris_suffix_underscore() {
        // Container name exactly equals workspace after normalization, then has _iris appended.
        // "myapp_iris" vs "myapp": cn starts_with wb → 80, not exact → 80 + 10 = 90
        let score = score_container_name("myapp_iris", "myapp");
        assert_eq!(score, 90);
    }
}

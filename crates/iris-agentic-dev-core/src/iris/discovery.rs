//! IRIS instance discovery cascade.
//!
//! Order of priority (highest to lowest):
//! 1. Explicit IrisConnection passed directly
//! 2. Env vars (IRIS_HOST + IRIS_WEB_PORT)
//! 3. Localhost port scan (100ms timeout, parallel)
//! 4. Docker containers via bollard
//! 5. VS Code settings.json objectscript.conn
//!
//! Each step fails silently and falls through to the next.

use crate::iris::connection::{DiscoverySource, IrisConnection};
use std::time::Duration;

/// The ports we scan on localhost for IRIS web servers.
/// Includes 52773 (IRIS private web server), 80/8080/8443/443 (ISC Web Gateway container),
/// and legacy ports 41773/51773.
const IRIS_WEB_PORTS: &[u16] = &[52773, 80, 8080, 41773, 51773, 8443, 443];

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
async fn probe_atelier_with_client(
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
        if std::env::var("IRIS_CONTAINER").is_ok() {
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

    let body: serde_json::Value = resp.json().await.ok()?;
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
    );
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
    let client = reqwest::Client::builder()
        .timeout(Duration::from_millis(timeout_ms))
        .danger_accept_invalid_certs(true)
        .build()
        .ok()?;
    probe_atelier_with_client(&client, host, port, username, password, namespace).await
}

/// Full discovery cascade. Returns `IrisDiscovery` — infallible.
pub async fn discover_iris(explicit: Option<IrisConnection>) -> IrisDiscovery {
    // 1. Explicit wins immediately — but probe for version + Atelier API level first
    if let Some(mut conn) = explicit {
        conn.probe().await;
        return IrisDiscovery::Found(conn);
    }

    // 2. Env vars
    if let Ok(host) = std::env::var("IRIS_HOST") {
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

        // When scheme or prefix is set, build base_url directly — probe_atelier
        // hardcodes http:// and doesn't support prefixes.
        if scheme != "http" || prefix.is_some() {
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
            );
            conn.probe().await;
            return IrisDiscovery::Found(conn);
        }

        if let Some(mut conn) =
            probe_atelier(&host, port, &username, &password, &namespace, 5000).await
        {
            conn.source = DiscoverySource::EnvVar;
            return IrisDiscovery::Found(conn);
        }
    }

    // 3. IRIS_CONTAINER: resolve the named container's web port via Docker.
    // workspace_config sets this when container = "..." in .iris-dev.toml.
    // Must run before the generic localhost scan or another container on port 52773 wins.
    if let Ok(container_name) = std::env::var("IRIS_CONTAINER") {
        if !container_name.is_empty() {
            match discover_via_docker_named(&container_name).await {
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
    }

    // 4. Localhost scan (parallel, 100ms each). Uses env var credentials.
    let username = std::env::var("IRIS_USERNAME").unwrap_or_else(|_| "_SYSTEM".to_string());
    let password = std::env::var("IRIS_PASSWORD").unwrap_or_else(|_| "SYS".to_string());
    let namespace = std::env::var("IRIS_NAMESPACE").unwrap_or_else(|_| "USER".to_string());

    let scan_client = match reqwest::Client::builder()
        .timeout(Duration::from_millis(100))
        .danger_accept_invalid_certs(true)
        .build()
    {
        Ok(c) => std::sync::Arc::new(c),
        Err(_) => return IrisDiscovery::NotFound,
    };

    let scan_tasks: Vec<_> = IRIS_WEB_PORTS
        .iter()
        .map(|&port| {
            let client = scan_client.clone();
            let u = username.clone();
            let p = password.clone();
            let ns = namespace.clone();
            tokio::spawn(async move {
                probe_atelier_with_client(&client, "localhost", port, &u, &p, &ns).await
            })
        })
        .collect();

    for task in scan_tasks {
        if let Ok(Some(conn)) = task.await {
            return IrisDiscovery::Found(conn);
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
fn emit_unhealthy_message(container_name: &str, mode: FailureMode) {
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
async fn discover_via_docker_named(target: &str) -> DiscoveryResult {
    use bollard::container::ListContainersOptions;
    use bollard::Docker;

    let docker = match Docker::connect_with_defaults() {
        Ok(d) => d,
        Err(_) => return DiscoveryResult::NotFound, // daemon unreachable; caller emits message
    };
    let containers = match tokio::time::timeout(
        Duration::from_secs(3),
        docker.list_containers(Some(ListContainersOptions::<String> {
            all: false,
            ..Default::default()
        })),
    )
    .await
    {
        Ok(Ok(c)) => c,
        _ => return DiscoveryResult::NotFound, // timeout or Docker error
    };

    let username = std::env::var("IRIS_USERNAME").unwrap_or_else(|_| "_SYSTEM".to_string());
    let password = std::env::var("IRIS_PASSWORD").unwrap_or_else(|_| "SYS".to_string());
    let namespace = std::env::var("IRIS_NAMESPACE").unwrap_or_else(|_| "USER".to_string());

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
    let client = match reqwest::Client::builder()
        .timeout(Duration::from_millis(2000))
        .danger_accept_invalid_certs(true)
        .build()
    {
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

    let body: serde_json::Value = match resp.json().await {
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
    );
    conn.version = Some(version);
    conn.atelier_version = match content["api"].as_u64() {
        Some(v) if v >= 8 => crate::iris::connection::AtelierVersion::V8,
        Some(v) if v >= 2 => crate::iris::connection::AtelierVersion::V2,
        _ => crate::iris::connection::AtelierVersion::V1,
    };
    conn.port_superserver = port_ss;
    DiscoveryResult::Connected(conn)
}

async fn discover_via_docker() -> Option<IrisConnection> {
    use bollard::container::ListContainersOptions;
    use bollard::Docker;

    let workspace_basename = std::env::current_dir()
        .ok()
        .and_then(|p| p.file_name().map(|n| n.to_string_lossy().to_string()))
        .unwrap_or_default();

    let docker = Docker::connect_with_defaults().ok()?;
    let containers = tokio::time::timeout(
        Duration::from_secs(3),
        docker.list_containers(Some(ListContainersOptions::<String> {
            all: false,
            ..Default::default()
        })),
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

    // Bug 24: create a single shared HTTP client for all docker probes.
    let probe_client = match reqwest::Client::builder()
        .timeout(Duration::from_millis(500))
        .danger_accept_invalid_certs(true)
        .build()
    {
        Ok(c) => c,
        Err(_) => return None,
    };

    for (_score, container_name, web_port, port_ss) in candidates {
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
        init_platform_keystore, parse_sm_settings, resolve_credential, select_server,
        sm_settings_path, SmCredentialError,
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

    // Initialize platform keystore (once-only via Once guard).
    init_platform_keystore();

    let password = match resolve_credential(&profile.name, &profile.username) {
        Ok(pw) => pw,
        Err(e) => return SmDiscovery::CredentialError(e.to_string()),
    };

    let namespace = std::env::var("IRIS_NAMESPACE").unwrap_or_else(|_| "USER".to_string());
    let base_url = match &profile.path_prefix {
        Some(prefix) => format!(
            "{}://{}:{}/{}",
            profile.scheme, profile.host, profile.port, prefix
        ),
        None => format!("{}://{}:{}", profile.scheme, profile.host, profile.port),
    };

    let mut conn = IrisConnection::new(
        base_url,
        namespace,
        profile.username.clone(),
        password,
        DiscoverySource::ServerManager {
            server_name: profile.name.clone(),
        },
    );
    conn.probe().await;
    SmDiscovery::Found(conn)
}

/// Attempt to find IRIS connection from VS Code settings.json in common locations.
async fn discover_via_vscode_settings() -> Option<IrisConnection> {
    let candidates = [std::env::current_dir().ok()?.join(".vscode/settings.json")];

    for path in &candidates {
        if !path.exists() {
            continue;
        }
        if let Ok(settings) = crate::iris::vscode_config::parse_vscode_settings(path) {
            if let Some(conn) = settings.to_iris_connection().await {
                return Some(conn);
            }
        }
    }
    None
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

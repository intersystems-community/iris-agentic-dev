//! Per-workspace IRIS connection config via `.iris-agentic-dev.toml`.
//!
//! Priority order: CLI flags > .iris-agentic-dev.toml > env vars > auto-discovery.

use crate::iris::connection::{DiscoverySource, IrisConnection};
use serde::Deserialize;
use std::path::PathBuf;

/// Parsed contents of `.iris-agentic-dev.toml`. All fields are optional.
#[derive(Debug, Deserialize, Default, Clone)]
pub struct WorkspaceConfig {
    pub container: Option<String>,
    pub namespace: Option<String>,
    pub host: Option<String>,
    pub web_port: Option<u16>,
    /// URL path prefix for the IRIS web gateway, e.g. "irisaicore" when the
    /// Atelier API is served at http://host:port/irisaicore/api/atelier/...
    /// Corresponds to intersystems.servers[x].webServer.pathPrefix in VS Code settings.
    pub web_prefix: Option<String>,
    /// URL scheme: "http" or "https". Defaults to "http".
    /// Set to "https" for TLS-protected IRIS web gateways.
    pub scheme: Option<String>,
    pub username: Option<String>,
    pub password: Option<String>,
    /// When true, skip HTTP/Atelier REST and use docker exec exclusively.
    /// Use for containers without a web server (e.g. community IRIS with no web gateway).
    /// Requires IRIS_CONTAINER to be set or container= in config.
    #[serde(default)]
    pub docker_only: bool,
}

/// Resolve the workspace root path.
/// Priority: OBJECTSCRIPT_WORKSPACE env var > workspace_path arg > walk up from cwd.
///
/// When no explicit path is given, walks up from current_dir() looking for .iris-agentic-dev.toml
/// (git-style discovery). This ensures the config is found even when the MCP server is
/// launched from a parent directory (e.g. by an IDE that sets cwd to the home directory).
pub fn workspace_root(workspace_path: Option<&str>) -> PathBuf {
    if let Ok(ws) = std::env::var("OBJECTSCRIPT_WORKSPACE") {
        if !ws.is_empty() {
            return PathBuf::from(ws);
        }
    }
    if let Some(p) = workspace_path {
        if !p.is_empty() && p != "." {
            return PathBuf::from(p);
        }
    }
    // Walk up from current directory looking for .iris-agentic-dev.toml
    // (fall back to legacy .iris-dev.toml for backward compatibility)
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let mut dir = cwd.as_path();
    let mut legacy_root: Option<PathBuf> = None;
    loop {
        if dir.join(".iris-agentic-dev.toml").exists() {
            return dir.to_path_buf();
        }
        if legacy_root.is_none() && dir.join(".iris-dev.toml").exists() {
            legacy_root = Some(dir.to_path_buf());
        }
        match dir.parent() {
            Some(parent) => dir = parent,
            None => break,
        }
    }
    // If no .iris-agentic-dev.toml found but legacy .iris-dev.toml exists, use that dir.
    legacy_root.unwrap_or(cwd)
}

/// Load `.iris-agentic-dev.toml` from the resolved workspace root.
/// Returns `None` if the file does not exist (not an error).
/// Logs a warning and returns `None` on parse errors — never panics.
pub fn load_workspace_config(workspace_path: Option<&str>) -> Option<WorkspaceConfig> {
    let root = workspace_root(workspace_path);
    // Prefer new config name; fall back to legacy .iris-dev.toml for backward compatibility.
    let config_path = if root.join(".iris-agentic-dev.toml").exists() {
        root.join(".iris-agentic-dev.toml")
    } else if root.join(".iris-dev.toml").exists() {
        tracing::debug!(
            "Using legacy .iris-dev.toml (consider renaming to .iris-agentic-dev.toml)"
        );
        root.join(".iris-dev.toml")
    } else {
        return None;
    };

    match std::fs::read_to_string(&config_path) {
        Err(e) => {
            tracing::warn!(
                "Could not read .iris-agentic-dev.toml at {}: {}",
                config_path.display(),
                e
            );
            None
        }
        Ok(contents) => match toml::from_str::<WorkspaceConfig>(&contents) {
            Ok(cfg) => {
                tracing::debug!(
                    "Loaded .iris-agentic-dev.toml from {}",
                    config_path.display()
                );
                Some(cfg)
            }
            Err(e) => {
                tracing::warn!(
                    "Could not parse .iris-agentic-dev.toml at {}: {}",
                    config_path.display(),
                    e
                );
                None
            }
        },
    }
}

/// Apply workspace config to set up the connection environment.
///
/// If `host` is specified: returns `Some(IrisConnection)` that will be passed directly
/// to `discover_iris()` as the explicit override.
///
/// If `container` is specified (but not host): sets `IRIS_CONTAINER` (and optionally
/// `IRIS_NAMESPACE`, `IRIS_USERNAME`, `IRIS_PASSWORD`) so the standard discovery cascade
/// picks up the container. Returns `None` to let discovery proceed normally.
///
/// If neither is specified: returns `None` — no connection info in the config.
pub fn workspace_config_to_connection(
    cfg: &WorkspaceConfig,
    namespace_default: &str,
) -> Option<IrisConnection> {
    // host + web_port → explicit HTTP/HTTPS connection (highest priority, no docker needed)
    if let Some(ref host) = cfg.host {
        let port = cfg.web_port.unwrap_or(52773);
        let scheme = cfg
            .scheme
            .clone()
            .or_else(|| std::env::var("IRIS_SCHEME").ok())
            .map(|s| s.trim_matches('/').to_string())
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| "http".to_string());
        let prefix = cfg
            .web_prefix
            .clone()
            .or_else(|| std::env::var("IRIS_WEB_PREFIX").ok())
            .map(|p| p.trim_matches('/').to_string())
            .filter(|p| !p.is_empty());
        let base_url = match prefix {
            Some(p) => format!("{}://{}:{}/{}", scheme, host, port, p),
            None => format!("{}://{}:{}", scheme, host, port),
        };
        let namespace = cfg
            .namespace
            .clone()
            .or_else(|| std::env::var("IRIS_NAMESPACE").ok())
            .unwrap_or_else(|| namespace_default.to_string());
        let username = cfg
            .username
            .clone()
            .or_else(|| std::env::var("IRIS_USERNAME").ok())
            .unwrap_or_else(|| "_SYSTEM".to_string());
        let password = cfg
            .password
            .clone()
            .or_else(|| std::env::var("IRIS_PASSWORD").ok())
            .unwrap_or_else(|| "SYS".to_string());
        // If container is also specified alongside host, update IRIS_CONTAINER so docker
        // exec tools (iris_execute fallback, iris_test, etc.) target the right container.
        if let Some(ref container) = cfg.container {
            std::env::set_var("IRIS_CONTAINER", container);
        }
        return Some(IrisConnection::new(
            base_url,
            namespace,
            username,
            password,
            DiscoverySource::EnvVar,
        ));
    }

    // container → inject into env so discover_iris() docker step picks it up
    if let Some(ref container) = cfg.container {
        std::env::set_var("IRIS_CONTAINER", container);
        let ns = cfg
            .namespace
            .clone()
            .or_else(|| std::env::var("IRIS_NAMESPACE").ok())
            .unwrap_or_else(|| namespace_default.to_string());
        let username = cfg
            .username
            .clone()
            .or_else(|| std::env::var("IRIS_USERNAME").ok())
            .unwrap_or_else(|| "_SYSTEM".to_string());
        let password = cfg
            .password
            .clone()
            .or_else(|| std::env::var("IRIS_PASSWORD").ok())
            .unwrap_or_else(|| "SYS".to_string());
        if let Some(ref ns_val) = cfg.namespace {
            std::env::set_var("IRIS_NAMESPACE", ns_val);
        }
        if let Some(ref user) = cfg.username {
            std::env::set_var("IRIS_USERNAME", user);
        }
        if let Some(ref pass) = cfg.password {
            std::env::set_var("IRIS_PASSWORD", pass);
        }
        if cfg.docker_only {
            // docker_only=true: skip HTTP entirely, use docker exec for all operations.
            // Return a connection with an unreachable URL — HTTP calls will fail fast,
            // triggering the docker exec fallback in iris_execute/iris_compile etc.
            return Some(IrisConnection::new(
                "http://127.0.0.1:1",
                ns,
                username,
                password,
                DiscoverySource::Docker {
                    container_name: container.clone(),
                },
            ));
        }
        return None; // discover_iris() will find the container via IRIS_CONTAINER
    }

    None
}

/// Apply workspace config to an existing explicit connection override.
///
/// If `explicit` is already set (from CLI flags), returns it unchanged.
/// Otherwise loads `.iris-agentic-dev.toml` from `workspace_path` and applies it:
/// - `host` config → returns `Some(IrisConnection)`
/// - `container` config → sets `IRIS_CONTAINER` env var, returns `None`
/// - no config / no relevant fields → returns `None`
pub fn apply_workspace_config(
    explicit: Option<IrisConnection>,
    workspace_path: Option<&str>,
    namespace: &str,
) -> Option<IrisConnection> {
    if explicit.is_some() {
        return explicit;
    }
    let cfg = load_workspace_config(workspace_path)?;
    explicit.or_else(|| workspace_config_to_connection(&cfg, namespace))
}

/// Generate starter `.iris-agentic-dev.toml` content with inline comments.
/// Used by `iris-dev init` and `check_config`.
pub fn generate_toml_content(container: &str, namespace: &str) -> String {
    format!(
        r#"# iris-agentic-dev workspace configuration
# Commit this file to share connection settings with your team.

# ── Native IRIS (no Docker) — Windows IIS or Linux Apache ──────────────────
# For IRIS installed directly on the host (not in Docker), uncomment and set:
# host = "localhost"
# web_port = 80       # IIS default (IRIS 2024.1+); use 52773 for pre-2024.1 Private Web Server
# web_prefix = ""     # URL path prefix, e.g. "iris" when Atelier is at /iris/api/atelier/
# namespace = "USER"
# scheme = "http"     # Use "https" for TLS-protected IRIS web gateways

# ── Docker container ────────────────────────────────────────────────────────
# For IRIS running in Docker, uncomment and set container name:
# NOTE: iris-agentic-dev requires the IRIS Atelier REST API. Three supported configurations:
#   1. Community images (iris-community, irishealth-community) — include private web server on port 52773
#   2. Enterprise + ISC Web Gateway container (intersystems/webgateway) — iris-agentic-dev auto-detects it
#   3. Enterprise standalone (intersystems/iris) — NOT supported, no Atelier REST available
#
# container = "{container}"

# Default IRIS namespace
namespace = "{namespace}"

# Credentials (optional)
# Use IRIS_USERNAME / IRIS_PASSWORD env vars instead of committing credentials.
# username = "_SYSTEM"
# password = "..."  # not recommended in committed files
"#
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use std::sync::Mutex;

    // Serialize tests that mutate global env vars to avoid parallel interference.
    static ENV_LOCK: Mutex<()> = Mutex::new(());

    // ── workspace_root tests ─────────────────────────────────────────────────

    #[test]
    fn workspace_root_env_var_overrides_all() {
        let _guard = ENV_LOCK.lock().unwrap();
        let dir = tempfile::tempdir().unwrap();
        std::env::set_var("OBJECTSCRIPT_WORKSPACE", dir.path().to_str().unwrap());
        let root = workspace_root(None);
        std::env::remove_var("OBJECTSCRIPT_WORKSPACE");
        assert_eq!(root, dir.path());
    }

    #[test]
    fn workspace_root_env_overrides_arg() {
        let _guard = ENV_LOCK.lock().unwrap();
        // When env is set, it takes priority over the arg
        let env_dir = tempfile::tempdir().unwrap();
        let arg_dir = tempfile::tempdir().unwrap();
        std::env::set_var("OBJECTSCRIPT_WORKSPACE", env_dir.path().to_str().unwrap());
        let root = workspace_root(Some(arg_dir.path().to_str().unwrap()));
        std::env::remove_var("OBJECTSCRIPT_WORKSPACE");
        assert_eq!(root, env_dir.path(), "env var must win over arg");
    }

    #[test]
    fn workspace_root_empty_arg_falls_through() {
        let _guard = ENV_LOCK.lock().unwrap();
        std::env::remove_var("OBJECTSCRIPT_WORKSPACE");
        let root = workspace_root(Some(""));
        // Falls through to cwd-walk — just check it doesn't panic and returns something
        assert!(root.is_absolute() || root.to_str() == Some("."));
    }

    #[test]
    fn workspace_root_dot_arg_falls_through() {
        let _guard = ENV_LOCK.lock().unwrap();
        std::env::remove_var("OBJECTSCRIPT_WORKSPACE");
        let root = workspace_root(Some("."));
        assert!(root.is_absolute() || root.to_str() == Some("."));
    }

    // ── load_workspace_config tests ──────────────────────────────────────────

    #[test]
    fn load_config_missing_file_returns_none() {
        let _guard = ENV_LOCK.lock().unwrap();
        std::env::remove_var("OBJECTSCRIPT_WORKSPACE");
        let dir = tempfile::tempdir().unwrap();
        std::env::set_var("OBJECTSCRIPT_WORKSPACE", dir.path().to_str().unwrap());
        let result = load_workspace_config(None);
        std::env::remove_var("OBJECTSCRIPT_WORKSPACE");
        assert!(result.is_none());
    }

    #[test]
    fn load_config_valid_toml_returns_some() {
        let _guard = ENV_LOCK.lock().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let mut f = std::fs::File::create(dir.path().join(".iris-agentic-dev.toml")).unwrap();
        writeln!(
            f,
            "host = \"localhost\"\nweb_port = 52773\nnamespace = \"USER\""
        )
        .unwrap();
        std::env::set_var("OBJECTSCRIPT_WORKSPACE", dir.path().to_str().unwrap());
        let result = load_workspace_config(None);
        std::env::remove_var("OBJECTSCRIPT_WORKSPACE");
        let cfg = result.expect("should load config");
        assert_eq!(cfg.host.as_deref(), Some("localhost"));
        assert_eq!(cfg.web_port, Some(52773));
    }

    #[test]
    fn load_config_invalid_toml_returns_none() {
        let _guard = ENV_LOCK.lock().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let mut f = std::fs::File::create(dir.path().join(".iris-agentic-dev.toml")).unwrap();
        writeln!(f, "this is not valid toml ={{{{").unwrap();
        std::env::set_var("OBJECTSCRIPT_WORKSPACE", dir.path().to_str().unwrap());
        let result = load_workspace_config(None);
        std::env::remove_var("OBJECTSCRIPT_WORKSPACE");
        assert!(result.is_none());
    }

    #[test]
    fn load_config_legacy_iris_dev_toml() {
        let _guard = ENV_LOCK.lock().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let mut f = std::fs::File::create(dir.path().join(".iris-dev.toml")).unwrap();
        writeln!(f, "host = \"myhost\"\nweb_port = 52773").unwrap();
        std::env::set_var("OBJECTSCRIPT_WORKSPACE", dir.path().to_str().unwrap());
        let result = load_workspace_config(None);
        std::env::remove_var("OBJECTSCRIPT_WORKSPACE");
        // Legacy file may or may not be picked up depending on priority; at minimum no panic
        let _ = result;
    }

    // ── workspace_config_to_connection tests ─────────────────────────────────

    #[test]
    fn config_with_host_returns_connection() {
        let cfg = WorkspaceConfig {
            host: Some("localhost".to_string()),
            web_port: Some(52773),
            namespace: Some("USER".to_string()),
            container: None,
            username: None,
            password: None,
            scheme: None,
            web_prefix: None,
            docker_only: false,
        };
        let conn = workspace_config_to_connection(&cfg, "USER");
        assert!(conn.is_some(), "host config should produce connection");
    }

    #[test]
    fn config_with_container_only_returns_none_and_sets_env() {
        let _guard = ENV_LOCK.lock().unwrap();
        std::env::remove_var("IRIS_CONTAINER");
        let cfg = WorkspaceConfig {
            host: None,
            web_port: None,
            namespace: Some("MYNS".to_string()),
            container: Some("my-iris-container".to_string()),
            username: None,
            password: None,
            scheme: None,
            web_prefix: None,
            docker_only: false,
        };
        let conn = workspace_config_to_connection(&cfg, "USER");
        let container_env = std::env::var("IRIS_CONTAINER").ok();
        std::env::remove_var("IRIS_CONTAINER");
        assert!(conn.is_none(), "container-only config should return None");
        assert_eq!(container_env.as_deref(), Some("my-iris-container"));
    }

    #[test]
    fn config_empty_returns_none() {
        let cfg = WorkspaceConfig {
            host: None,
            web_port: None,
            namespace: None,
            container: None,
            username: None,
            password: None,
            scheme: None,
            web_prefix: None,
            docker_only: false,
        };
        let conn = workspace_config_to_connection(&cfg, "USER");
        assert!(conn.is_none());
    }

    #[test]
    fn workspace_root_arg_used_when_no_env() {
        let _guard = ENV_LOCK.lock().unwrap();
        std::env::remove_var("OBJECTSCRIPT_WORKSPACE");
        let dir = tempfile::tempdir().unwrap();
        let root = workspace_root(Some(dir.path().to_str().unwrap()));
        assert_eq!(root, dir.path());
    }

    #[test]
    fn workspace_root_legacy_toml_fallback() {
        let _guard = ENV_LOCK.lock().unwrap();
        std::env::remove_var("OBJECTSCRIPT_WORKSPACE");
        let dir = tempfile::tempdir().unwrap();
        std::fs::File::create(dir.path().join(".iris-dev.toml")).unwrap();
        std::env::set_var("OBJECTSCRIPT_WORKSPACE", dir.path().to_str().unwrap());
        // load_workspace_config will walk and find .iris-dev.toml
        let result = load_workspace_config(None);
        std::env::remove_var("OBJECTSCRIPT_WORKSPACE");
        // Legacy file loads fine (may be empty/default, doesn't error)
        let _ = result;
    }

    #[test]
    fn config_with_host_and_path_prefix_builds_prefixed_url() {
        let cfg = WorkspaceConfig {
            host: Some("my-host".to_string()),
            web_port: Some(80),
            namespace: Some("USER".to_string()),
            container: None,
            username: None,
            password: None,
            scheme: Some("http".to_string()),
            web_prefix: Some("iriscore".to_string()),
            docker_only: false,
        };
        let conn = workspace_config_to_connection(&cfg, "USER").unwrap();
        assert!(
            conn.base_url.contains("iriscore"),
            "URL should include path prefix: {}",
            conn.base_url
        );
    }

    #[test]
    fn config_with_host_and_container_sets_iris_container_env() {
        let _guard = ENV_LOCK.lock().unwrap();
        std::env::remove_var("IRIS_CONTAINER");
        let cfg = WorkspaceConfig {
            host: Some("localhost".to_string()),
            web_port: Some(52773),
            namespace: None,
            container: Some("my-iris-container".to_string()),
            username: None,
            password: None,
            scheme: None,
            web_prefix: None,
            docker_only: false,
        };
        let conn = workspace_config_to_connection(&cfg, "USER");
        let container_env = std::env::var("IRIS_CONTAINER").ok();
        std::env::remove_var("IRIS_CONTAINER");
        assert!(conn.is_some(), "host present → connection returned");
        assert_eq!(
            container_env.as_deref(),
            Some("my-iris-container"),
            "IRIS_CONTAINER must be set when container+host both specified"
        );
    }

    #[test]
    fn config_container_with_credentials_sets_env_vars() {
        let _guard = ENV_LOCK.lock().unwrap();
        std::env::remove_var("IRIS_CONTAINER");
        std::env::remove_var("IRIS_USERNAME");
        std::env::remove_var("IRIS_PASSWORD");
        std::env::remove_var("IRIS_NAMESPACE");
        let cfg = WorkspaceConfig {
            host: None,
            web_port: None,
            namespace: Some("MYNAMESPACE".to_string()),
            container: Some("cred-container".to_string()),
            username: Some("myuser".to_string()),
            password: Some("mypass".to_string()),
            scheme: None,
            web_prefix: None,
            docker_only: false,
        };
        let conn = workspace_config_to_connection(&cfg, "USER");
        let ns_env = std::env::var("IRIS_NAMESPACE").ok();
        let user_env = std::env::var("IRIS_USERNAME").ok();
        let pass_env = std::env::var("IRIS_PASSWORD").ok();
        std::env::remove_var("IRIS_CONTAINER");
        std::env::remove_var("IRIS_USERNAME");
        std::env::remove_var("IRIS_PASSWORD");
        std::env::remove_var("IRIS_NAMESPACE");
        assert!(conn.is_none(), "container-only → no connection");
        assert_eq!(ns_env.as_deref(), Some("MYNAMESPACE"));
        assert_eq!(user_env.as_deref(), Some("myuser"));
        assert_eq!(pass_env.as_deref(), Some("mypass"));
    }

    #[test]
    fn config_docker_only_returns_unreachable_connection() {
        let _guard = ENV_LOCK.lock().unwrap();
        let cfg = WorkspaceConfig {
            host: None,
            web_port: None,
            namespace: Some("USER".to_string()),
            container: Some("docker-only-iris".to_string()),
            username: None,
            password: None,
            scheme: None,
            web_prefix: None,
            docker_only: true,
        };
        let conn = workspace_config_to_connection(&cfg, "USER");
        std::env::remove_var("IRIS_CONTAINER");
        assert!(
            conn.is_some(),
            "docker_only=true should return a connection (with unreachable URL)"
        );
        let c = conn.unwrap();
        assert!(
            c.base_url.contains("127.0.0.1:1"),
            "docker_only URL must be unreachable: {}",
            c.base_url
        );
    }

    #[test]
    fn apply_workspace_config_returns_explicit_unchanged() {
        use crate::iris::connection::DiscoverySource;
        let explicit = IrisConnection::new(
            "http://explicit-host:52773",
            "EXPLICIT",
            "_SYSTEM",
            "SYS",
            DiscoverySource::EnvVar,
        );
        let result = apply_workspace_config(Some(explicit.clone()), None, "USER");
        assert!(result.is_some());
        assert_eq!(result.unwrap().base_url, explicit.base_url);
    }

    #[test]
    fn apply_workspace_config_loads_config_when_no_explicit() {
        let _guard = ENV_LOCK.lock().unwrap();
        std::env::remove_var("OBJECTSCRIPT_WORKSPACE");
        let dir = tempfile::tempdir().unwrap();
        let mut f = std::fs::File::create(dir.path().join(".iris-agentic-dev.toml")).unwrap();
        use std::io::Write;
        writeln!(f, "host = \"config-host\"\nweb_port = 52773").unwrap();
        std::env::set_var("OBJECTSCRIPT_WORKSPACE", dir.path().to_str().unwrap());
        let result = apply_workspace_config(None, None, "USER");
        std::env::remove_var("OBJECTSCRIPT_WORKSPACE");
        assert!(result.is_some(), "should load config and build connection");
        let c = result.unwrap();
        assert!(c.base_url.contains("config-host"));
    }

    // ── generate_toml_content tests ──────────────────────────────────────────

    #[test]
    fn toml_template_native_section_before_container() {
        let content = generate_toml_content("my-iris", "USER");
        let native_pos = content
            .find("Native IRIS")
            .expect("template must contain 'Native IRIS' section header");
        let container_pos = content
            .find("Docker container")
            .expect("template must contain 'Docker container' section header");
        assert!(
            native_pos < container_pos,
            "Native IRIS section must appear before Docker container section"
        );
    }

    #[test]
    fn toml_template_has_port_80_comment() {
        let content = generate_toml_content("my-iris", "USER");
        assert!(
            content.contains("web_port = 80"),
            "template must document port 80 as IIS default"
        );
    }

    #[test]
    fn toml_template_both_sections_commented_by_default() {
        let content = generate_toml_content("my-iris", "USER");
        // Neither host nor container should be active (uncommented) assignments
        for line in content.lines() {
            let trimmed = line.trim();
            if trimmed.starts_with('#') || trimmed.is_empty() {
                continue;
            }
            assert!(
                !trimmed.starts_with("host =") && !trimmed.starts_with("container ="),
                "host and container must be commented out in default template, found: {trimmed}"
            );
        }
    }
}

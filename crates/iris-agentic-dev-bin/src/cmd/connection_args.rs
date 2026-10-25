use clap::Args;
use iris_agentic_dev_core::iris::{
    connection::{DiscoverySource, IrisConnection},
    discovery::{discover_iris_with, IrisDiscovery},
    workspace_config::{apply_workspace_config, workspace_discovery_hints},
};

/// Shared IRIS connection flags reused by all CLI subcommands. They are `global`, so a command with
/// an action (`doc put`) takes them after the action as well as before it.
/// Precedence (highest to lowest):
///   1. Explicit CLI flags (--host, --port, --namespace, ...)
///   2. iris-dev.toml workspace config
///   3. Environment variables (IRIS_HOST, IRIS_WEB_PORT, IRIS_CONTAINER, ...)
///   4. Auto-discovery cascade (localhost scan → Docker scan → VS Code settings)
#[derive(Args, Clone)]
pub struct ConnectionArgs {
    /// IRIS web hostname (overrides discovery)
    // No `env = "IRIS_HOST"`: clap would fill this slot from the variable, so any `IRIS_HOST`
    // made the connection explicit and outranked `.iris-agentic-dev.toml`. The documented order is
    // flag, then the file, then `IRIS_*`; discovery reads `IRIS_HOST` itself after the file.
    #[arg(global = true, long)]
    pub host: Option<String>,

    /// IRIS web port
    #[arg(global = true, long, env = "IRIS_WEB_PORT", default_value = "52773")]
    pub web_port: u16,

    /// URL path prefix for IIS-fronted instances (e.g. healthshareucr)
    #[arg(global = true, long, env = "IRIS_WEB_PREFIX", default_value = "")]
    pub web_prefix: String,

    /// URL scheme: http or https
    #[arg(global = true, long, env = "IRIS_SCHEME", default_value = "http")]
    pub scheme: String,

    /// IRIS namespace
    #[arg(
        global = true,
        long,
        short = 'n',
        env = "IRIS_NAMESPACE",
        default_value = "USER"
    )]
    pub namespace: String,

    /// IRIS username
    #[arg(global = true, long, short = 'u', env = "IRIS_USERNAME")]
    pub username: Option<String>,

    /// IRIS password
    #[arg(global = true, long, short = 'p', env = "IRIS_PASSWORD")]
    pub password: Option<String>,

    /// Named Docker container for IRIS (overrides auto-discovery)
    #[arg(global = true, long, env = "IRIS_CONTAINER")]
    pub container: Option<String>,
}

impl ConnectionArgs {
    /// Resolve this `ConnectionArgs` into a live `IrisConnection`.
    /// Runs the same discovery cascade as the MCP server.
    /// Exits the process (printing to stderr) on connection failure.
    pub async fn resolve(self) -> anyhow::Result<IrisConnection> {
        let explicit = self.host.as_ref().map(|host| {
            let endpoint = match iris_agentic_dev_core::iris::endpoint::Endpoint::parse(
                Some(&self.scheme),
                host,
                self.web_port,
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
            IrisConnection::new(
                base_url,
                &self.namespace,
                username,
                password,
                DiscoverySource::ExplicitFlag,
            )
            // The launch settings describe this server, so their certificate setting applies.
            .with_tls(iris_agentic_dev_core::iris::http::TlsSetting::from_launch_env())
        });

        // Apply workspace config — sits between CLI flags and env/auto-discovery
        let ws_path = std::env::var("OBJECTSCRIPT_WORKSPACE").ok();
        let explicit = apply_workspace_config(explicit, ws_path.as_deref(), &self.namespace);
        let hints = workspace_discovery_hints(ws_path.as_deref());

        match discover_iris_with(explicit, &hints).await {
            IrisDiscovery::Found(c) => Ok(c),
            IrisDiscovery::NotFound => {
                anyhow::bail!(
                    "No IRIS connection found — set IRIS_HOST or run `iris-agentic-dev mcp` for auto-discovery"
                );
            }
            IrisDiscovery::Explained => {
                crate::exit(1);
            }
        }
    }

    /// Build the explicit base_url without running discovery (for testing).
    #[cfg(test)]
    fn explicit_base_url(&self) -> Option<String> {
        self.host.as_ref().map(|host| {
            let scheme = self.scheme.trim_matches('/');
            let prefix = self.web_prefix.trim_matches('/');
            if prefix.is_empty() {
                format!("{}://{}:{}", scheme, host, self.web_port)
            } else {
                format!("{}://{}:{}/{}", scheme, host, self.web_port, prefix)
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(host: &str, port: u16, scheme: &str, prefix: &str) -> ConnectionArgs {
        ConnectionArgs {
            host: Some(host.to_string()),
            web_port: port,
            web_prefix: prefix.to_string(),
            scheme: scheme.to_string(),
            namespace: "USER".to_string(),
            username: None,
            password: None,
            container: None,
        }
    }

    #[test]
    fn no_prefix_builds_plain_url() {
        let a = args("myhost", 52773, "http", "");
        assert_eq!(
            a.explicit_base_url(),
            Some("http://myhost:52773".to_string())
        );
    }

    #[test]
    fn prefix_appended_to_url() {
        let a = args("myhost", 80, "http", "healthshareucr");
        assert_eq!(
            a.explicit_base_url(),
            Some("http://myhost:80/healthshareucr".to_string())
        );
    }

    #[test]
    fn prefix_with_slashes_stripped() {
        let a = args("myhost", 80, "http", "/healthshareucr/");
        assert_eq!(
            a.explicit_base_url(),
            Some("http://myhost:80/healthshareucr".to_string())
        );
    }

    #[test]
    fn https_scheme_applied() {
        let a = args("myhost", 443, "https", "");
        assert_eq!(
            a.explicit_base_url(),
            Some("https://myhost:443".to_string())
        );
    }

    #[test]
    fn no_host_returns_none() {
        let a = ConnectionArgs {
            host: None,
            web_port: 52773,
            web_prefix: String::new(),
            scheme: "http".to_string(),
            namespace: "USER".to_string(),
            username: None,
            password: None,
            container: None,
        };
        assert_eq!(a.explicit_base_url(), None);
    }
}

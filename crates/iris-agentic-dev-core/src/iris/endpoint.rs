//! Where an IRIS web server is: scheme, host, port and path prefix, parsed once.
//!
//! Every source of a server (the workspace file, `[instance.*]`, `servers.json`, Server Manager,
//! `.vscode`, `IRIS_HOST` and `--host`) builds its URL through [`Endpoint::parse`], so a bad scheme
//! is an error for that entry and never a URL, and "is this the same server" is answered in one
//! place ([`Endpoint::same_server`]). Credentials travel by that answer (spec 141, FR-021, FR-023).

use std::fmt;
use std::net::IpAddr;

/// `http` or `https`. Nothing else is a scheme iad will send a request with.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Scheme {
    Http,
    Https,
}

impl Scheme {
    pub fn as_str(&self) -> &'static str {
        match self {
            Scheme::Http => "http",
            Scheme::Https => "https",
        }
    }
}

/// An endpoint that could not be built, and why.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InvalidEndpoint(pub String);

impl fmt::Display for InvalidEndpoint {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for InvalidEndpoint {}

/// The parsed form of `scheme://host:port[/prefix]`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Endpoint {
    scheme: Scheme,
    /// Lowercase, IPv6 literals without brackets.
    host: String,
    port: u16,
    /// No leading or trailing slash; `None` for no prefix.
    prefix: Option<String>,
}

/// `http` or `https`, lowercased, from a configured scheme (surrounding `/` and a trailing `:` are
/// tolerated); `None` for anything else, so a scheme cannot carry the URL to a host the loopback
/// check never read.
pub fn parse_scheme(raw: &str) -> Option<Scheme> {
    let s = raw
        .trim()
        .trim_matches('/')
        .trim_end_matches(':')
        .to_ascii_lowercase();
    match s.as_str() {
        "http" => Some(Scheme::Http),
        "https" => Some(Scheme::Https),
        _ => None,
    }
}

fn bare_host(host: &str) -> &str {
    let h = host.trim();
    h.strip_prefix('[')
        .and_then(|h| h.strip_suffix(']'))
        .unwrap_or(h)
}

fn is_loopback_host(host: &str) -> bool {
    host.eq_ignore_ascii_case("localhost")
        || host.parse::<IpAddr>().is_ok_and(|ip| ip.is_loopback())
}

impl Endpoint {
    /// Build an endpoint. `scheme` of `None` or empty means `http`.
    pub fn parse(
        scheme: Option<&str>,
        host: &str,
        port: u16,
        prefix: Option<&str>,
    ) -> Result<Endpoint, InvalidEndpoint> {
        let scheme = match scheme.map(str::trim).filter(|s| !s.is_empty()) {
            None => Scheme::Http,
            Some(raw) => parse_scheme(raw)
                .ok_or_else(|| InvalidEndpoint(format!("scheme {raw:?} is not http or https")))?,
        };
        let host = bare_host(host);
        let plain = !host.is_empty()
            && !host
                .chars()
                .any(|c| c.is_whitespace() || "/?#@\\[]".contains(c));
        let literal = host.parse::<IpAddr>().is_ok();
        let url_host = if literal && host.contains(':') {
            format!("[{host}]")
        } else {
            host.to_string()
        };
        let parsed = reqwest::Url::parse(&format!("http://{url_host}/")).ok();
        let is_host = parsed.as_ref().is_some_and(|u| {
            u.host().is_some()
                && u.port().is_none()
                && u.username().is_empty()
                && u.path() == "/"
                && u.query().is_none()
        });
        if !(plain || literal) || !is_host {
            return Err(InvalidEndpoint(format!(
                "host {host:?} must be a hostname or IP address, without a scheme, port or path"
            )));
        }
        let prefix = match prefix.map(str::trim) {
            None => None,
            Some(p) => {
                if p.contains("://") || p.chars().any(|c| c.is_whitespace() || "?#\\".contains(c)) {
                    return Err(InvalidEndpoint(format!(
                        "web prefix {p:?} must be a path only, such as \"/hs20261\""
                    )));
                }
                let p = p.trim_matches('/');
                (!p.is_empty()).then(|| p.to_string())
            }
        };
        Ok(Endpoint {
            scheme,
            host: host.to_ascii_lowercase(),
            port,
            prefix,
        })
    }

    /// Parse `scheme://host:port[/prefix]`. A URL with no port takes the scheme's default.
    pub fn from_base_url(url: &str) -> Result<Endpoint, InvalidEndpoint> {
        let u = reqwest::Url::parse(url)
            .map_err(|e| InvalidEndpoint(format!("{url:?} is not a URL: {e}")))?;
        let host = u
            .host_str()
            .ok_or_else(|| InvalidEndpoint(format!("{url:?} has no host")))?;
        let port = u
            .port_or_known_default()
            .ok_or_else(|| InvalidEndpoint(format!("{url:?} has no port")))?;
        let prefix = u.path().trim_matches('/');
        Endpoint::parse(
            Some(u.scheme()),
            host,
            port,
            (!prefix.is_empty()).then_some(prefix),
        )
    }

    pub fn scheme(&self) -> Scheme {
        self.scheme
    }

    pub fn host(&self) -> &str {
        &self.host
    }

    pub fn port(&self) -> u16 {
        self.port
    }

    pub fn prefix(&self) -> Option<&str> {
        self.prefix.as_deref()
    }

    /// `scheme://host:port` plus `/prefix` when there is one. No trailing slash.
    pub fn base_url(&self) -> String {
        let host = if self.host.contains(':') {
            format!("[{}]", self.host)
        } else {
            self.host.clone()
        };
        match &self.prefix {
            Some(p) => format!("{}://{host}:{}/{p}", self.scheme.as_str(), self.port),
            None => format!("{}://{host}:{}", self.scheme.as_str(), self.port),
        }
    }

    /// This machine: `localhost`, or a loopback address.
    pub fn is_loopback(&self) -> bool {
        is_loopback_host(&self.host)
    }

    /// The same web server: scheme, port and prefix equal; host equal without case, and every
    /// loopback name counts as one host.
    pub fn same_server(&self, other: &Endpoint) -> bool {
        self.scheme == other.scheme
            && self.port == other.port
            && self.prefix == other.prefix
            && (self.host == other.host || (self.is_loopback() && other.is_loopback()))
    }
}

impl fmt::Display for Endpoint {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.base_url())
    }
}

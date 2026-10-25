//! The guard in front of the MCP HTTP transport (spec 141 FR-025, contract `http-transport.md`).
//!
//! [`check`] runs on the request head before rmcp sees the request, so a refused request creates no
//! session, runs no tool and opens no IRIS connection. It is a pure function: the table of cases
//! in the unit tests needs no socket.
//!
//! The order is Host, then Origin, then the token. A refusal for a bad Host says nothing about the
//! token, so a probe from a hostile page learns nothing about it.

use std::convert::Infallible;
use std::net::IpAddr;

use http_body_util::{combinators::BoxBody, BodyExt, Full};
use hyper::body::Bytes;
use hyper::http::request::Parts;
use hyper::{header, Response, StatusCode};

/// A token shorter than this is refused at startup. `openssl rand -hex 32` makes 64.
pub const MIN_TOKEN_BYTES: usize = 32;

/// Why [`check`] refused a request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Rejection {
    /// 403: the `Host` header names something this bind does not serve.
    BadHost,
    /// 403: the `Origin` header is cross-site.
    BadOrigin,
    /// 401: the bearer token is missing or wrong.
    Unauthorized,
}

impl Rejection {
    pub fn status(self) -> StatusCode {
        match self {
            Rejection::BadHost | Rejection::BadOrigin => StatusCode::FORBIDDEN,
            Rejection::Unauthorized => StatusCode::UNAUTHORIZED,
        }
    }

    fn reason(self) -> &'static str {
        match self {
            Rejection::BadHost => "Host not allowed\n",
            Rejection::BadOrigin => "Origin not allowed\n",
            Rejection::Unauthorized => "Bearer token required\n",
        }
    }
}

/// The bytes of the token. `Debug` never prints them.
#[derive(Clone)]
struct Secret(Vec<u8>);

impl std::fmt::Debug for Secret {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Secret(<{} bytes>)", self.0.len())
    }
}

/// What the operator supplied. The token is operator-only (FR-035): it never comes from a
/// workspace file, and no flag takes its value, because a value on the command line shows in `ps`.
#[derive(Debug, Clone, Default)]
pub struct GuardSettings {
    pub token: Option<String>,
    /// `--allowed-host`, repeatable.
    pub allowed_hosts: Vec<String>,
    /// `--allowed-origin`, repeatable.
    pub allowed_origins: Vec<String>,
}

/// Where a token may come from, in order: `IAD_HTTP_TOKEN`, the `--auth-token-file` contents, the
/// operator file's `[transport] http_token`.
#[derive(Debug, Clone, Default)]
pub struct TokenSources {
    pub env: Option<String>,
    pub file: Option<String>,
    pub operator: Option<String>,
}

impl TokenSources {
    /// The first source that is set. A source that is set and too short is an error, not a reason to
    /// look at the next one: the operator meant that token.
    pub fn resolve(&self) -> Result<Option<String>, String> {
        let candidates = [
            self.env.as_deref().filter(|s| !s.trim().is_empty()),
            // A file named on the command line is always meant, so an empty one is an error.
            self.file.as_deref(),
            self.operator.as_deref().filter(|s| !s.trim().is_empty()),
        ];
        match candidates.into_iter().flatten().next() {
            None => Ok(None),
            Some(raw) => {
                let token = raw.trim();
                check_length(token)?;
                Ok(Some(token.to_string()))
            }
        }
    }
}

fn check_length(token: &str) -> Result<(), String> {
    if token.len() < MIN_TOKEN_BYTES {
        return Err(format!(
            "the HTTP token is {} bytes; at least {MIN_TOKEN_BYTES} are required. \
             Generate one with: openssl rand -hex 32",
            token.len()
        ));
    }
    Ok(())
}

/// Who may talk to the transport.
#[derive(Debug, Clone)]
pub struct HttpGuard {
    bind: IpAddr,
    token: Option<Secret>,
    /// Lowercased `--allowed-host` names.
    hosts: Vec<String>,
    /// Lowercased `--allowed-origin` values without a trailing slash.
    origins: Vec<String>,
}

impl HttpGuard {
    /// The guard for a bind address, or the reason the server must not start: a token that is too
    /// short, or no token on an address other machines can reach.
    pub fn from_settings(bind: IpAddr, settings: GuardSettings) -> Result<Self, String> {
        let token = match settings.token.as_deref() {
            Some(t) => {
                check_length(t)?;
                Some(Secret(t.as_bytes().to_vec()))
            }
            None => None,
        };
        if token.is_none() && !bind.is_loopback() {
            return Err(format!(
                "refusing to listen on {bind}: other machines can reach that address and no HTTP \
                 token is set. Set one with IAD_HTTP_TOKEN in the environment, \
                 --auth-token-file <path>, or `[transport] http_token` in \
                 ~/.iris-agentic-dev.toml. Generate one with: openssl rand -hex 32"
            ));
        }
        Ok(Self {
            bind,
            token,
            hosts: settings
                .allowed_hosts
                .iter()
                .map(|h| h.trim().to_ascii_lowercase())
                .filter(|h| !h.is_empty())
                .collect(),
            origins: settings
                .allowed_origins
                .iter()
                .map(|o| normalize_origin(o))
                .filter(|o| !o.is_empty())
                .collect(),
        })
    }

    /// The names `--allowed-host` added, for the rmcp host list on binds where rmcp can state one.
    pub fn extra_hosts(&self) -> &[String] {
        &self.hosts
    }

    fn host_allowed(&self, host: &str) -> bool {
        if matches!(host, "localhost" | "127.0.0.1" | "::1") {
            return true;
        }
        if let Ok(ip) = host.parse::<IpAddr>() {
            // An IP literal carries no DNS-rebinding risk: rebinding works through a name the
            // attacker controls. On an unspecified bind any literal is a way in to this machine.
            return self.bind.is_unspecified() || ip == self.bind;
        }
        (self.bind.is_unspecified() && host == "host.docker.internal")
            || self.hosts.iter().any(|h| h == host)
    }
}

fn normalize_origin(o: &str) -> String {
    o.trim().trim_end_matches('/').to_ascii_lowercase()
}

/// The host part of a `Host` header value, lowercased, without the port. `None` when it is empty
/// or malformed (a bare IPv6 address, a port that is not a number).
fn host_name(value: &str) -> Option<String> {
    let value = value.trim().to_ascii_lowercase();
    let (host, port) = if let Some(rest) = value.strip_prefix('[') {
        let (inner, after) = rest.split_once(']')?;
        (inner, after.strip_prefix(':'))
    } else {
        match value.split_once(':') {
            Some((h, p)) => (h, Some(p)),
            None => (value.as_str(), None),
        }
    };
    if host.is_empty() {
        return None;
    }
    if let Some(p) = port {
        if p.is_empty() || !p.bytes().all(|b| b.is_ascii_digit()) {
            return None;
        }
    }
    Some(host.to_string())
}

/// Compare in time that depends only on `expected`'s length. `expected` is a token of 32 bytes or
/// more, so the length leaks nothing worth having, and no byte position leaks.
pub fn token_matches(given: &[u8], expected: &[u8]) -> bool {
    let mut diff = given.len() ^ expected.len();
    for (i, e) in expected.iter().enumerate() {
        diff |= usize::from(given.get(i).copied().unwrap_or(0) ^ e);
    }
    std::hint::black_box(diff) == 0
}

/// Decide whether the request may reach rmcp.
pub fn check(parts: &Parts, guard: &HttpGuard) -> Result<(), Rejection> {
    // Host.
    let host_value = parts
        .headers
        .get(header::HOST)
        .and_then(|v| v.to_str().ok())
        .map(str::to_string)
        .or_else(|| parts.uri.authority().map(|a| a.as_str().to_string()));
    let host_value = host_value.ok_or(Rejection::BadHost)?;
    let host = host_name(&host_value).ok_or(Rejection::BadHost)?;
    if !guard.host_allowed(&host) {
        return Err(Rejection::BadHost);
    }

    // Origin: absent is fine (CLI and SDK clients send none); present must be same-origin with
    // the Host just accepted, or listed.
    if let Some(origin) = parts.headers.get(header::ORIGIN) {
        let origin = origin.to_str().map_err(|_| Rejection::BadOrigin)?;
        let origin = normalize_origin(origin);
        let same_origin = ["http://", "https://"]
            .iter()
            .filter_map(|scheme| origin.strip_prefix(scheme))
            .any(|authority| authority == host_value.to_ascii_lowercase());
        if !(same_origin || guard.origins.contains(&origin)) {
            return Err(Rejection::BadOrigin);
        }
    }

    // Token.
    if let Some(Secret(expected)) = &guard.token {
        let given = parts
            .headers
            .get(header::AUTHORIZATION)
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.split_once(' '))
            .filter(|(scheme, _)| scheme.eq_ignore_ascii_case("bearer"))
            .map(|(_, token)| token.trim())
            .unwrap_or("");
        // Always compared, so a missing header costs the same as a wrong one.
        let matches = token_matches(given.as_bytes(), expected);
        if !matches || given.is_empty() {
            return Err(Rejection::Unauthorized);
        }
    }
    Ok(())
}

/// The response for a refused request, in the body type rmcp's service answers with.
pub fn rejection_response(r: Rejection) -> Response<BoxBody<Bytes, Infallible>> {
    let mut b = Response::builder()
        .status(r.status())
        .header(header::CONTENT_TYPE, "text/plain; charset=utf-8");
    if r == Rejection::Unauthorized {
        b = b.header(
            header::WWW_AUTHENTICATE,
            "Bearer realm=\"iris-agentic-dev\"",
        );
    }
    b.body(Full::new(Bytes::from_static(r.reason().as_bytes())).boxed())
        .expect("a static response is valid")
}

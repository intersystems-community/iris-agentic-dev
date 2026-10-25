//! The one builder for every HTTP client that talks to an IRIS server (spec 141, FR-021, FR-022).
//!
//! Whether a server's certificate is checked is a property of its connection ([`TlsSetting`]), not
//! of the process. [`client_for`] is the only place in the crate where certificate checks can be
//! switched off, and its signature takes the connection's setting, so a caller cannot pass a bare
//! `true`. Nothing writes `IRIS_TLS_VERIFY` or `IRIS_INSECURE` into the environment: those two
//! variables are read once, by [`TlsSetting::from_launch_env`], for the connection the launch
//! settings describe.
//!
//! Clients are built on demand and not cached. Each carries a `User-Agent` that names the MCP peer
//! of the call in scope (`connection::user_agent`), which a cached client would freeze.
//!
//! [`IdentifyClient`] is the other half of FR-024: discovery asks a listener who it is through a
//! type that has no way to attach a credential.

use std::sync::{Arc, OnceLock};
use std::time::Duration;

use super::connection::{caller_mode, tls_insecure, user_agent};
use super::csp_session::CspSessionStore;

/// Where a connection's certificate setting came from, for `check_config`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TlsSource {
    /// Nothing declared it off: certificates are checked.
    Default,
    /// `IRIS_INSECURE` or `IRIS_TLS_VERIFY`, on the connection the launch settings describe.
    LaunchEnv,
    /// `tls_verify` in the operator's file, or in a project file the operator trusted.
    ConfigFile,
    /// `tls_verify` on a `servers.json` entry.
    ServersJson,
    /// A Server Manager profile. Profiles carry no such setting and always validate.
    ServerManager,
}

impl TlsSource {
    pub fn as_str(&self) -> &'static str {
        match self {
            TlsSource::Default => "default",
            TlsSource::LaunchEnv => "launch_env",
            TlsSource::ConfigFile => "config_file",
            TlsSource::ServersJson => "servers_json",
            TlsSource::ServerManager => "server_manager",
        }
    }
}

/// Whether to check a server's certificate, and what said so.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TlsSetting {
    pub verify: bool,
    pub source: TlsSource,
}

impl Default for TlsSetting {
    fn default() -> Self {
        TlsSetting::validating(TlsSource::Default)
    }
}

impl TlsSetting {
    pub fn validating(source: TlsSource) -> Self {
        TlsSetting {
            verify: true,
            source,
        }
    }

    /// A `tls_verify` value some file declared. `None` declares nothing.
    pub fn declared(verify: Option<bool>, source: TlsSource) -> Self {
        match verify {
            Some(verify) => TlsSetting { verify, source },
            None => TlsSetting::default(),
        }
    }

    /// The setting the launch settings give the connection they describe, from the values of
    /// `IRIS_INSECURE` and `IRIS_TLS_VERIFY` (the truth table of `connection::tls_insecure`).
    pub fn from_launch_values(insecure_var: Option<&str>, tls_verify_var: Option<&str>) -> Self {
        if tls_insecure(insecure_var, tls_verify_var) {
            TlsSetting {
                verify: false,
                source: TlsSource::LaunchEnv,
            }
        } else {
            TlsSetting::default()
        }
    }

    /// [`Self::from_launch_values`] applied to the process environment. Only the connection the
    /// launch settings describe (`--host`, `IRIS_HOST`, discovery) may call this.
    pub fn from_launch_env() -> Self {
        let insecure = std::env::var("IRIS_INSECURE").ok();
        let verify = std::env::var("IRIS_TLS_VERIFY").ok();
        Self::from_launch_values(insecure.as_deref(), verify.as_deref())
    }
}

/// What a client is for: its timeout and whether it joins the process's CSP session.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ClientKind {
    /// Tool calls: 30 s, in the shared CSP session.
    Call,
    /// The startup probe: connect in 5 s, answer in 10 s, in the shared CSP session.
    Probe,
    /// `iris_search`'s synchronous request: no client timeout (the request sets its own), no cookies.
    Search,
    /// The one credentialed request discovery makes to a listener that identified itself: 2 s.
    Scan,
    /// Concurrent document reads: 5 s, no cookies.
    Batch,
    /// A request whose raw `Set-Cookie` headers are wanted: no timeout, no cookies.
    Raw,
    /// The logout that ends the shared CSP session at exit: in that session, timeout per request.
    Logout,
    /// A probe of a host the agent named: validates always, no cookies, no redirects.
    AdHoc,
    /// One credentialed request to a connection's own server (the service account's login probe):
    /// that server's certificate setting, no cookies, no redirects, 10 s.
    OwnProbe,
}

fn base_builder(verify: bool) -> reqwest::ClientBuilder {
    reqwest::Client::builder()
        // Identifies agent traffic in Web Gateway / IIS / Apache access logs and to
        // `%request.CgiEnvs("HTTP_USER_AGENT")` (FR-001, FR-009 of the attribution spec).
        .user_agent(user_agent(caller_mode()))
        .danger_accept_invalid_certs(!verify)
        .tcp_keepalive(Duration::from_secs(20))
}

/// A client for `kind` that checks (or not) the certificate as `tls` says. Clients that join a
/// CSP session join the process's shared one.
pub fn client_for(tls: &TlsSetting, kind: ClientKind) -> anyhow::Result<reqwest::Client> {
    client_with_store(tls, kind, super::csp_session::shared_cookie_store())
}

/// [`client_for`] with the CSP session store a cookie-joining `kind` uses: a service identity's own
/// store (spec 141, R2 point 2), or the one a logout must carry to end that store's sessions. A
/// `kind` that holds no cookies ignores `store`.
pub fn client_with_store(
    tls: &TlsSetting,
    kind: ClientKind,
    store: Arc<CspSessionStore>,
) -> anyhow::Result<reqwest::Client> {
    let b = base_builder(tls.verify);
    let b = match kind {
        ClientKind::Call => b.timeout(Duration::from_secs(30)).cookie_provider(store),
        ClientKind::Probe => b
            .connect_timeout(Duration::from_secs(5))
            .timeout(Duration::from_secs(10))
            .cookie_provider(store),
        ClientKind::Scan => b.timeout(Duration::from_secs(2)),
        ClientKind::Search | ClientKind::Raw => b,
        ClientKind::Batch => b.timeout(Duration::from_secs(5)),
        ClientKind::Logout => b.cookie_provider(store),
        ClientKind::AdHoc => base_builder(true)
            .connect_timeout(Duration::from_secs(5))
            .timeout(Duration::from_secs(10))
            .redirect(reqwest::redirect::Policy::none()),
        ClientKind::OwnProbe => b
            .connect_timeout(Duration::from_secs(5))
            .timeout(Duration::from_secs(10))
            .redirect(reqwest::redirect::Policy::none()),
    };
    Ok(b.build()?)
}

/// The tool-call client for a connection whose setting is `tls`, for callers that need a borrow
/// (`IrisTools::http_client`). Two exist for the life of the process, one per answer.
pub fn call_client_ref(tls: &TlsSetting) -> anyhow::Result<&'static reqwest::Client> {
    static CHECKED: OnceLock<reqwest::Client> = OnceLock::new();
    static UNCHECKED: OnceLock<reqwest::Client> = OnceLock::new();
    let cell = if tls.verify { &CHECKED } else { &UNCHECKED };
    if let Some(c) = cell.get() {
        return Ok(c);
    }
    let client = client_for(tls, ClientKind::Call)?;
    Ok(cell.get_or_init(|| client))
}

// ── Identification ──────────────────────────────────────────────────────────────────────────

/// What a listener answered to an identification request. No credential was sent to get it.
#[derive(Debug, Clone, Default)]
pub struct Identity {
    pub status: u16,
    pub content_type: Option<String>,
    /// Names (not values) of the cookies the answer set.
    pub set_cookie_names: Vec<String>,
    /// The first 64 KiB of the body, lossily decoded.
    pub body: String,
}

/// A client that can only ask a listener who it is: `GET`, no credentials, no cookie jar, no
/// redirects. Discovery identifies a listener through this type before any credential is sent to it
/// (spec 141, FR-024), so "no credentials before identification" is a property of the type.
pub struct IdentifyClient(reqwest::Client);

impl IdentifyClient {
    pub fn new(tls: &TlsSetting, timeout: Duration) -> anyhow::Result<Self> {
        Ok(IdentifyClient(
            base_builder(tls.verify)
                .timeout(timeout)
                .redirect(reqwest::redirect::Policy::none())
                .build()?,
        ))
    }

    pub async fn get(&self, url: &str) -> Result<Identity, reqwest::Error> {
        let resp = self.0.get(url).send().await?;
        let status = resp.status().as_u16();
        let content_type = resp
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .map(str::to_string);
        let set_cookie_names = resp
            .headers()
            .get_all(reqwest::header::SET_COOKIE)
            .iter()
            .filter_map(|v| v.to_str().ok())
            .filter_map(|v| v.split(';').next())
            .filter_map(|nv| nv.split('=').next())
            .map(|n| n.trim().to_string())
            .collect();
        let mut body = Vec::new();
        let mut resp = resp;
        while let Some(chunk) = resp.chunk().await.unwrap_or(None) {
            body.extend_from_slice(&chunk);
            if body.len() >= 64 * 1024 {
                body.truncate(64 * 1024);
                break;
            }
        }
        Ok(Identity {
            status,
            content_type,
            set_cookie_names,
            body: String::from_utf8_lossy(&body).into_owned(),
        })
    }
}

// ── The cap on a body read from IRIS ────────────────────────────────────────────────────────────

/// The most bytes iad reads from one IRIS response before it gives up (spec 141 T221).
///
/// A server that answers a request with an endless or enormous body was read to the end into
/// memory. The cap is far above any legitimate answer: the largest measured on a live instance was
/// the USER namespace's document list at 1.6 MB, and a namespace with a hundred thousand documents
/// is about twenty. Raise or lower it with `IAD_MAX_RESPONSE_BYTES`, a launch setting.
pub const DEFAULT_MAX_RESPONSE_BYTES: usize = 128 * 1024 * 1024;

/// The cap in force: `IAD_MAX_RESPONSE_BYTES` when it holds a positive whole number of bytes,
/// otherwise [`DEFAULT_MAX_RESPONSE_BYTES`].
pub fn max_response_bytes() -> usize {
    std::env::var("IAD_MAX_RESPONSE_BYTES")
        .ok()
        .and_then(|v| v.trim().parse::<usize>().ok())
        .filter(|n| *n > 0)
        .unwrap_or(DEFAULT_MAX_RESPONSE_BYTES)
}

/// An IRIS response was bigger than the cap.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResponseTooLarge {
    pub limit: usize,
}

impl std::fmt::Display for ResponseTooLarge {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "RESPONSE_TOO_LARGE: IRIS answered with more than {} bytes, the most iad reads from one \
             response. Narrow the request (a filter, a limit, a smaller document set). The request \
             may already have been carried out. The operator can change the limit with \
             IAD_MAX_RESPONSE_BYTES in the launch settings.",
            self.limit
        )
    }
}

impl std::error::Error for ResponseTooLarge {}

/// Why a body could not be read.
#[derive(Debug)]
pub enum BodyError {
    TooLarge(ResponseTooLarge),
    Transport(reqwest::Error),
    /// The body was read and is not the JSON asked for.
    Json(serde_json::Error),
}

impl std::fmt::Display for BodyError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BodyError::TooLarge(e) => e.fmt(f),
            BodyError::Transport(e) => e.fmt(f),
            BodyError::Json(e) => e.fmt(f),
        }
    }
}

impl std::error::Error for BodyError {}

tokio::task_local! {
    /// The oversize answers met while one tool call ran. The pipeline opens it around the call and
    /// turns a successful answer into `RESPONSE_TOO_LARGE` if anything was recorded, because most
    /// readers fall back to an empty value and would otherwise report success on nothing.
    pub static OVERSIZE: std::sync::Arc<std::sync::Mutex<Option<ResponseTooLarge>>>;
}

fn note_oversize(e: &ResponseTooLarge) {
    let _ = OVERSIZE.try_with(|cell| {
        if let Ok(mut c) = cell.lock() {
            c.get_or_insert_with(|| e.clone());
        }
    });
}

/// The whole body, or [`BodyError::TooLarge`] as soon as it is known to pass the cap: from the
/// declared length before a byte is read, or from the bytes read so far when there is none.
pub async fn capped_bytes(mut resp: reqwest::Response) -> Result<Vec<u8>, BodyError> {
    let limit = max_response_bytes();
    let too_large = || {
        let e = ResponseTooLarge { limit };
        note_oversize(&e);
        BodyError::TooLarge(e)
    };
    if resp.content_length().is_some_and(|n| n > limit as u64) {
        return Err(too_large());
    }
    let mut body = Vec::new();
    while let Some(chunk) = resp.chunk().await.map_err(BodyError::Transport)? {
        if body.len() + chunk.len() > limit {
            return Err(too_large());
        }
        body.extend_from_slice(&chunk);
    }
    Ok(body)
}

/// [`reqwest::Response::text`] with the cap. Invalid UTF-8 is replaced, as `text` does.
pub async fn capped_text(resp: reqwest::Response) -> Result<String, BodyError> {
    let bytes = capped_bytes(resp).await?;
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

/// [`reqwest::Response::json`] with the cap.
pub async fn capped_json<T: serde::de::DeserializeOwned>(
    resp: reqwest::Response,
) -> Result<T, BodyError> {
    let bytes = capped_bytes(resp).await?;
    serde_json::from_slice(&bytes).map_err(BodyError::Json)
}

/// `resp.json().await.unwrap_or_default()` with the cap. A body that is not JSON is `Null`, as
/// before. A body over the cap is an Atelier-shaped error naming the cap, so a handler that reads
/// `status.errors` shows it, and the pipeline reports `RESPONSE_TOO_LARGE` whatever the handler did.
pub async fn json_or_default(resp: reqwest::Response) -> serde_json::Value {
    match capped_json(resp).await {
        Ok(v) => v,
        Err(BodyError::TooLarge(e)) => {
            let msg = e.to_string();
            serde_json::json!({
                "status": {"errors": [{"error": msg}], "summary": msg},
                "result": null,
            })
        }
        Err(_) => serde_json::Value::Null,
    }
}

/// `resp.text().await.unwrap_or_default()` with the cap. A body over the cap reads as the
/// `RESPONSE_TOO_LARGE` message, which is not what IRIS sent and parses as nothing a handler expects.
pub async fn text_or_default(resp: reqwest::Response) -> String {
    match capped_text(resp).await {
        Ok(t) => t,
        Err(BodyError::TooLarge(e)) => e.to_string(),
        Err(_) => String::new(),
    }
}

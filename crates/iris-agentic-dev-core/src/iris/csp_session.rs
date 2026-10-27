//! One CSP cookie store per identity, and the logout that ends their sessions (132 B1, 171 S7).
//!
//! Each `reqwest::Client` built with `.cookie_store(true)` gets a jar of its own, so the probe,
//! `client` and `exec_client` each opened a CSP session on `/api/atelier`, and none was ever ended.
//! A session holds its slot for the web app's timeout (3600 s by default): ten `iad exec` calls
//! left twenty sessions on 139, and two ladder tasks ran its key into `<LICENSE LIMIT EXCEEDED>`.
//!
//! Every client of a connection takes the store of its (endpoint, user) ([`store_for_primary`]), so
//! a process holds one session per web application and account, and two accounts on one host never
//! share a cookie: IRIS reads `CSPSESSIONID` before `Authorization`, so a shared jar would run the
//! second account's call in the first account's session. [`shared_cookie_store`] is left for a
//! client that belongs to no connection. Each store keeps the path of each `CSPSESSIONID-*` cookie
//! IRIS sets, and [`logout_blocking`] sends `?IRISLogout=end` to each one on the way out. IRIS ends
//! the session named by the cookie and answers 401, so the reply is not checked.

use std::collections::BTreeSet;
use std::sync::{Arc, LazyLock, Mutex};
use std::time::Duration;

use reqwest::cookie::{CookieStore, Jar};
use reqwest::header::HeaderValue;
use reqwest::Url;

const SESSION_COOKIE_PREFIX: &str = "CSPSESSIONID";

/// A `reqwest` cookie jar that also records where each CSP session lives.
#[derive(Default)]
pub struct CspSessionStore {
    jar: Jar,
    // `origin + cookie path`, e.g. `http://localhost:52781/api/atelier/`.
    sessions: Mutex<BTreeSet<String>>,
}

impl CspSessionStore {
    pub fn new() -> Self {
        Self::default()
    }

    /// One `?IRISLogout=end` URL per CSP session this store has seen.
    pub fn logout_urls(&self) -> Vec<String> {
        self.sessions
            .lock()
            .unwrap()
            .iter()
            .map(|base| format!("{base}?IRISLogout=end"))
            .collect()
    }

    /// True when this store has seen no CSP session.
    pub fn is_empty(&self) -> bool {
        self.sessions.lock().unwrap().is_empty()
    }

    /// Take over the sessions `other` holds: the cookies it sends to each session's URL, and the
    /// record of where each lives, so this store's logout ends them.
    pub fn adopt(&self, other: &CspSessionStore) {
        let bases: Vec<String> = other.sessions.lock().unwrap().iter().cloned().collect();
        for base in bases {
            let Ok(url) = Url::parse(&base) else { continue };
            let Some(sent) = other.jar.cookies(&url) else {
                continue;
            };
            let Ok(sent) = sent.to_str() else { continue };
            let headers: Vec<HeaderValue> = sent
                .split(';')
                .map(str::trim)
                .filter(|pair| pair.contains('='))
                .filter_map(|pair| {
                    HeaderValue::from_str(&format!("{pair}; Path={}", url.path())).ok()
                })
                .collect();
            self.set_cookies(&mut headers.iter(), &url);
        }
    }

    fn record(&self, header: &HeaderValue, url: &Url) {
        let Ok(text) = header.to_str() else { return };
        let mut parts = text.split(';').map(str::trim);
        // `split` always yields one item, so a header with no name reads as an empty name.
        let name = parts
            .next()
            .unwrap_or_default()
            .split('=')
            .next()
            .unwrap_or_default();
        if !name.starts_with(SESSION_COOKIE_PREFIX) {
            return;
        }
        let path = parts
            .filter_map(|p| p.split_once('='))
            .find(|(k, _)| k.eq_ignore_ascii_case("path"))
            .map(|(_, v)| v.to_string())
            // RFC 6265 5.1.4: with no Path attribute the cookie belongs to the request's directory.
            .unwrap_or_else(|| {
                let p = url.path();
                p[..=p.rfind('/').unwrap_or(0)].to_string()
            });
        let path = if path.ends_with('/') {
            path
        } else {
            format!("{path}/")
        };
        let origin = url.origin().ascii_serialization();
        self.sessions
            .lock()
            .unwrap()
            .insert(format!("{origin}{path}"));
    }
}

impl CookieStore for CspSessionStore {
    fn set_cookies(&self, cookie_headers: &mut dyn Iterator<Item = &HeaderValue>, url: &Url) {
        let headers: Vec<&HeaderValue> = cookie_headers.collect();
        for header in &headers {
            self.record(header, url);
        }
        self.jar.set_cookies(&mut headers.into_iter(), url);
    }

    fn cookies(&self, url: &Url) -> Option<HeaderValue> {
        self.jar.cookies(url)
    }
}

static SHARED: LazyLock<Arc<CspSessionStore>> = LazyLock::new(|| Arc::new(CspSessionStore::new()));

/// One store per service identity (spec 141, R2 point 2). A service account that shared the
/// primary's jar could be answered from the primary's CSP session, because IRIS reads the cookie
/// before the `Authorization` header: the code would then run as the primary while the request
/// named the service account. `$USERNAME` in the result detects that; a jar of its own prevents it.
static IDENTITY_STORES: LazyLock<Mutex<std::collections::BTreeMap<String, Arc<CspSessionStore>>>> =
    LazyLock::new(Default::default);

/// The store of a client that belongs to no connection (and so holds no account's session).
pub fn shared_cookie_store() -> Arc<CspSessionStore> {
    SHARED.clone()
}

/// Origins whose connection has certificate checks off. A logout to one of them uses the same
/// setting as the session it ends, and every other origin is logged out with checks on.
static UNCHECKED_ORIGINS: LazyLock<Mutex<BTreeSet<String>>> =
    LazyLock::new(|| Mutex::new(BTreeSet::new()));

/// Record that the connection at `base_url` runs with certificate checks off.
pub fn note_unchecked_origin(base_url: &str) {
    if let Ok(u) = Url::parse(base_url) {
        UNCHECKED_ORIGINS
            .lock()
            .unwrap()
            .insert(u.origin().ascii_serialization());
    }
}

fn setting_for(url: &str) -> super::http::TlsSetting {
    let unchecked = Url::parse(url).is_ok_and(|u| {
        UNCHECKED_ORIGINS
            .lock()
            .unwrap()
            .contains(&u.origin().ascii_serialization())
    });
    if unchecked {
        super::http::TlsSetting {
            verify: false,
            source: super::http::TlsSource::Default,
        }
    } else {
        super::http::TlsSetting::default()
    }
}

/// The store every client of identity `key` shares, created on first use and registered for
/// [`logout_blocking`]. `key` names the server and the account, so two servers' accounts never
/// share a jar.
pub fn store_for_identity(key: &str) -> Arc<CspSessionStore> {
    IDENTITY_STORES
        .lock()
        .unwrap()
        .entry(key.to_string())
        .or_default()
        .clone()
}

/// The registry key of the primary account `username` at `base_url`: scheme, host, port and prefix
/// as the connection holds them, and the user without regard to case.
pub fn primary_identity_key(base_url: &str, username: &str) -> String {
    format!(
        "primary|{}|{}",
        base_url.trim_end_matches('/').to_ascii_lowercase(),
        username.to_ascii_lowercase()
    )
}

/// The store every client of the account `username` on the server at `base_url` shares (171 S7).
/// Two accounts on one host get two stores, and a service account has a third, so no request
/// carries a session another account opened.
pub fn store_for_primary(base_url: &str, username: &str) -> Arc<CspSessionStore> {
    store_for_identity(&primary_identity_key(base_url, username))
}

/// The `?IRISLogout=end` URL of every CSP session any registered store has seen.
pub fn all_logout_urls() -> Vec<String> {
    let mut urls = SHARED.logout_urls();
    for store in IDENTITY_STORES.lock().unwrap().values() {
        urls.extend(store.logout_urls());
    }
    urls.sort();
    urls.dedup();
    urls
}

/// End every CSP session any store has seen. Waits at most `timeout` in all.
///
/// Runs on a thread of its own with its own runtime, so it can be called from `main` after the
/// runtime has returned and from `crate::exit` inside a tokio task alike.
pub fn logout_blocking(timeout: Duration) {
    // A session ends only when the request carries its cookie, so each store logs out through a
    // client that holds that store.
    let stores: Vec<(Arc<CspSessionStore>, Vec<String>)> = std::iter::once(SHARED.clone())
        .chain(IDENTITY_STORES.lock().unwrap().values().cloned())
        .map(|s| {
            let urls = s.logout_urls();
            (s, urls)
        })
        .filter(|(_, urls)| !urls.is_empty())
        .collect();
    if stores.is_empty() {
        return;
    }
    let (done_tx, done_rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let Ok(rt) = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
        else {
            return;
        };
        rt.block_on(async {
            for (store, urls) in stores {
                // Each URL goes out through a client that holds this store and takes its origin's
                // certificate setting, so a logout checks certificates exactly as the session did.
                let sends = urls.iter().filter_map(|u| {
                    let client = super::http::client_with_store(
                        &setting_for(u),
                        super::http::ClientKind::Logout,
                        store.clone(),
                    )
                    .ok()?;
                    Some(client.get(u).timeout(timeout).send())
                });
                for result in futures_util::future::join_all(sends).await {
                    if let Err(e) = result {
                        tracing::debug!("CSP logout failed: {e}");
                    }
                }
            }
        });
        let _ = done_tx.send(());
    });
    let _ = done_rx.recv_timeout(timeout);
}

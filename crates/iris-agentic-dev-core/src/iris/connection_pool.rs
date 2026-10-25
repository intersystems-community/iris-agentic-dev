//! Multi-instance connection pool for IRIS connections (072-multi-instance-pool).
//!
//! `ConnectionPool` holds a named set of `IrisConnection` instances loaded from
//! all configured sources in priority order (first-wins name dedup).

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use rmcp::ErrorData as McpError;

use crate::iris::connection::{DiscoverySource, IrisConnection, SystemMode};

// ── Constants ─────────────────────────────────────────────────────────────────

const SERVER_NOT_FOUND_PREFIX: &str = "SERVER_NOT_FOUND: no server named '";

/// How long a Development or Test answer stays. A Live answer stays for the life of the pool: the
/// permissive answer expires so a member switched to Live is seen, the restrictive one does not
/// need to.
const MODE_TTL: Duration = Duration::from_secs(10 * 60);

/// The shortest time between two probes of a member that did not answer.
const FAILED_PROBE_BACKOFF: Duration = Duration::from_secs(30);

// ── ConnectionPool ────────────────────────────────────────────────────────────

/// A named pool of IRIS connections, loaded from all configured sources.
pub struct ConnectionPool {
    instances: HashMap<String, Member>,
    default_name: Option<String>,
    /// Source label per connection name. Values: `"iad-native"`, `"vscode"`, `"fleet"`, `"env"`.
    sources: HashMap<String, String>,
    /// What the last probe of each member answered, and when (spec 141, R1 D6). A swapped pool
    /// starts with an empty cache, so a reload forgets every answer.
    modes: std::sync::Mutex<HashMap<String, (SystemMode, Instant)>>,
    /// When each member's last unsuccessful probe ran, so an unreachable member costs one timeout
    /// per [`FAILED_PROBE_BACKOFF`], not one per call.
    failed_probes: std::sync::Mutex<HashMap<String, Instant>>,
    /// Set on a pool that has not been loaded yet: the stand-in `IrisTools` holds until the first
    /// call that needs a pool member (spec 141 T187). Loading reads every member's password, which
    /// opens the OS keychain, and a process that only lists tools or answers a call on the default
    /// connection must not do that.
    pending: Option<PendingLoad>,
}

/// Fills in the keychain secrets of a pool member's connection, and answers the credentials it
/// held back (spec 141, FR-042).
type SecretResolver = Box<
    dyn Fn(&mut IrisConnection) -> Vec<crate::iris::config_sources::SetAsideItem> + Send + Sync,
>;

/// One pool member. Its keychain secrets are read when a request first needs the connection
/// (spec 141 T189), not when the pool loads: loading read every member's password, and on macOS
/// each read can raise its own approval prompt.
struct Member {
    /// The connection without anything from the keychain: address, account name, certificate
    /// setting, service account name, container. What a listing reads.
    shell: Arc<IrisConnection>,
    /// How to fill in the secrets; `None` for a member whose connection was complete when added
    /// (an inline password, an environment variable).
    resolver: Option<SecretResolver>,
    /// The connection with its secrets, and the credentials held back, once the first request
    /// asked for them. Set at most once, then reused.
    resolved: std::sync::OnceLock<ResolvedMember>,
}

struct ResolvedMember {
    conn: Arc<IrisConnection>,
    withheld: Vec<crate::iris::config_sources::SetAsideItem>,
}

impl Member {
    fn complete(conn: IrisConnection) -> Self {
        Self {
            shell: Arc::new(conn),
            resolver: None,
            resolved: std::sync::OnceLock::new(),
        }
    }

    /// The connection a request uses. Reads this member's secrets the first time.
    fn connection(&self) -> Arc<IrisConnection> {
        let Some(resolver) = &self.resolver else {
            return self.shell.clone();
        };
        self.resolved
            .get_or_init(|| {
                let mut conn = (*self.shell).clone();
                let withheld = resolver(&mut conn);
                ResolvedMember {
                    conn: Arc::new(conn),
                    withheld,
                }
            })
            .conn
            .clone()
    }
}

/// What [`load_pool`] needs to build the pool a stand-in stands for.
#[derive(Debug, Clone)]
pub struct PendingLoad {
    /// The `.iris-agentic-dev.toml` whose `[instance.*]` blocks join the pool.
    pub config_file: Option<std::path::PathBuf>,
}

/// The error for a `server` the pool does not hold.
pub fn server_not_found(name: &str) -> McpError {
    McpError::invalid_request(
        format!(
            "{SERVER_NOT_FOUND_PREFIX}{name}' in pool. \
             Use iris_servers to list available instances."
        ),
        None,
    )
}

impl ConnectionPool {
    /// Create an empty pool with no connections and no default.
    pub fn empty() -> Self {
        Self {
            instances: HashMap::new(),
            default_name: None,
            sources: HashMap::new(),
            modes: Default::default(),
            failed_probes: Default::default(),
            pending: None,
        }
    }

    /// An empty stand-in for the pool `load_pool(config_file)` would build. Nothing is read: the
    /// holder swaps in the real pool the first time a member is needed (see
    /// `IrisTools::pool_ref`).
    pub fn unloaded(config_file: Option<std::path::PathBuf>) -> Self {
        Self {
            pending: Some(PendingLoad { config_file }),
            ..Self::empty()
        }
    }

    /// `Some` while this is a stand-in that has not been loaded.
    pub fn pending_load(&self) -> Option<&PendingLoad> {
        self.pending.as_ref()
    }

    /// The credentials held back from members whose secrets have been read: a Server Manager
    /// secret for an entry that no longer points at its profile, a stored password for an endpoint
    /// it was not saved for.
    ///
    /// A member's secrets are read when a request first uses it (T189), so a member nobody has
    /// called yet reports nothing here: deciding what to hold back starts with a keychain read,
    /// and listing the pool must not do one.
    pub fn withheld_credentials(&self) -> Vec<crate::iris::config_sources::SetAsideItem> {
        let mut items: Vec<_> = self
            .instances
            .values()
            .filter_map(|m| m.resolved.get())
            .flat_map(|r| r.withheld.iter().cloned())
            .collect();
        items.sort_by(|a, b| a.id.cmp(&b.id));
        items
    }

    /// The `SystemMode` of member `name`, probed when no answer is cached (spec 141, R1 D6).
    ///
    /// `Unknown` when the member is absent, did not answer, or answered less than
    /// [`FAILED_PROBE_BACKOFF`] ago without success. Live is cached for the life of the pool;
    /// Development, Test and Unset for [`MODE_TTL`]; Unknown not at all.
    pub async fn system_mode(&self, name: &str) -> SystemMode {
        let Some(member) = self.instances.get(name) else {
            return SystemMode::Unknown;
        };
        if let Some((mode, at)) = self.modes.lock().unwrap().get(name).copied() {
            if mode == SystemMode::Live || at.elapsed() < MODE_TTL {
                return mode;
            }
        }
        if self
            .failed_probes
            .lock()
            .unwrap()
            .get(name)
            .is_some_and(|at| at.elapsed() < FAILED_PROBE_BACKOFF)
        {
            return SystemMode::Unknown;
        }
        // The probe logs in: this is a request, so it reads the member's secrets.
        let conn = member.connection();
        let mode = match conn.client(crate::iris::http::ClientKind::Probe) {
            Ok(client) => conn.detect_system_mode(&client).await,
            Err(_) => SystemMode::Unknown,
        };
        if mode == SystemMode::Unknown {
            self.failed_probes
                .lock()
                .unwrap()
                .insert(name.to_string(), Instant::now());
        } else {
            self.failed_probes.lock().unwrap().remove(name);
            self.modes
                .lock()
                .unwrap()
                .insert(name.to_string(), (mode, Instant::now()));
        }
        mode
    }

    /// Pretend a probe of `name` answered `mode`, for a test that needs a cached answer without a
    /// server to give one.
    #[cfg(any(test, feature = "testing"))]
    pub fn record_mode_for_test(&self, name: &str, mode: SystemMode) {
        self.modes
            .lock()
            .unwrap()
            .insert(name.to_string(), (mode, Instant::now()));
    }

    /// Forget every cached system mode and failed probe. A configuration reload calls it, because
    /// the files that decided what to trust about a member may have changed with it.
    pub fn clear_modes(&self) {
        self.modes.lock().unwrap().clear();
        self.failed_probes.lock().unwrap().clear();
    }

    /// The cached answer for `name`, without probing.
    pub fn cached_system_mode(&self, name: &str) -> Option<SystemMode> {
        self.modes.lock().unwrap().get(name).map(|(m, _)| *m)
    }

    /// Return the source label for a named connection, or `"unknown"` if not found.
    ///
    /// Source values: `"iad-native"`, `"vscode"`, `"fleet"`, `"env"`.
    pub fn source_of(&self, name: &str) -> &str {
        self.sources
            .get(name)
            .map(|s| s.as_str())
            .unwrap_or("unknown")
    }

    /// Retrieve a connection by name, or the default connection when `name` is `None`, with its
    /// keychain secrets. For a request. The first call for a member reads that member's secrets
    /// (and no other member's); later calls reuse them. A listing uses [`ConnectionPool::peek`].
    ///
    /// - `get(None)` with no default → `IRIS_UNREACHABLE` error.
    /// - `get(Some("x"))` when `"x"` is absent → `SERVER_NOT_FOUND` error.
    pub fn get(&self, name: Option<&str>) -> Result<Arc<IrisConnection>, McpError> {
        self.member(name).map(Member::connection)
    }

    /// A member's connection without anything from the keychain: its address, account name,
    /// certificate setting, service account name and container; `password` and the service
    /// account's password are blank. Never reads a secret, so a listing can call it for every
    /// member. Errors as [`ConnectionPool::get`] does.
    pub fn peek(&self, name: Option<&str>) -> Result<Arc<IrisConnection>, McpError> {
        self.member(name).map(|m| m.shell.clone())
    }

    fn member(&self, name: Option<&str>) -> Result<&Member, McpError> {
        match name {
            None => {
                // Return default, or error if no default.
                match &self.default_name {
                    Some(default) => self
                        .instances
                        .get(default.as_str())
                        .ok_or_else(|| {
                            McpError::invalid_request(
                                format!(
                                    "IRIS_UNREACHABLE: no IRIS connection configured (default '{default}' missing from pool). \
                                     Set IRIS_HOST or configure servers.json.",
                                ),
                                None,
                            )
                        }),
                    None => {
                        if let Some(member) = self.instances.values().next() {
                            return Ok(member);
                        }
                        Err(McpError::invalid_request(
                            "IRIS_UNREACHABLE: no IRIS connection configured. \
                             Set IRIS_HOST or configure servers.json.",
                            None,
                        ))
                    }
                }
            }
            Some(n) => self.instances.get(n).ok_or_else(|| server_not_found(n)),
        }
    }

    /// Return sorted list of server names in the pool.
    pub fn names(&self) -> Vec<&str> {
        let mut names: Vec<&str> = self.instances.keys().map(|s| s.as_str()).collect();
        names.sort();
        names
    }

    /// Return the name of the default connection, if any.
    pub fn default_name(&self) -> Option<&str> {
        self.default_name.as_deref()
    }

    /// Return the number of connections in the pool.
    pub fn len(&self) -> usize {
        self.instances.len()
    }

    /// Return `true` if the pool contains no connections.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Create a `ConnectionPoolBuilder` to assemble a pool from multiple sources.
    pub fn builder() -> ConnectionPoolBuilder {
        ConnectionPoolBuilder {
            instances: Vec::new(),
            default_name: None,
            sources: HashMap::new(),
        }
    }
}

// ── ConnectionPoolBuilder ─────────────────────────────────────────────────────

/// Assembles a `ConnectionPool` from multiple sources with first-wins name dedup.
pub struct ConnectionPoolBuilder {
    /// Ordered insertion list; first entry for a given name wins.
    instances: Vec<(String, Member)>,
    default_name: Option<String>,
    /// Source label for each entry, keyed by name.
    sources: HashMap<String, String>,
}

impl ConnectionPoolBuilder {
    /// Add a named connection. If `is_default` is true and no default has been set yet,
    /// this entry becomes the default. Duplicate names are silently ignored (first wins).
    /// Source is recorded as `"unknown"`.
    pub fn add(&mut self, name: String, conn: IrisConnection, is_default: bool) {
        self.add_with_source(name, conn, is_default, "unknown");
    }

    /// Add a named connection with an explicit source label.
    ///
    /// Source values used by `load_pool`: `"iad-native"`, `"vscode"`, `"fleet"`, `"env"`.
    /// Duplicate names are silently ignored (first wins).
    pub fn add_with_source(
        &mut self,
        name: String,
        conn: IrisConnection,
        is_default: bool,
        source: &str,
    ) {
        self.push(name, Member::complete(conn), is_default, source);
    }

    /// Add a named connection whose keychain secrets are read when a request first uses it.
    /// `conn` carries no secret; `resolve` fills them in on a copy and answers the credentials it
    /// held back. Otherwise as [`ConnectionPoolBuilder::add_with_source`].
    pub fn add_deferred(
        &mut self,
        name: String,
        conn: IrisConnection,
        is_default: bool,
        source: &str,
        resolve: impl Fn(&mut IrisConnection) -> Vec<crate::iris::config_sources::SetAsideItem>
            + Send
            + Sync
            + 'static,
    ) {
        let member = Member {
            resolver: Some(Box::new(resolve)),
            ..Member::complete(conn)
        };
        self.push(name, member, is_default, source);
    }

    fn push(&mut self, name: String, member: Member, is_default: bool, source: &str) {
        // First-wins: only add if name not already present.
        let already_present = self.instances.iter().any(|(n, _)| n == &name);
        if !already_present {
            if is_default && self.default_name.is_none() {
                self.default_name = Some(name.clone());
            }
            self.sources.insert(name.clone(), source.to_string());
            self.instances.push((name, member));
        }
    }

    /// Consume the builder and produce a `ConnectionPool`.
    pub fn build(self) -> ConnectionPool {
        let instances: HashMap<String, Member> = self.instances.into_iter().collect();
        ConnectionPool {
            instances,
            default_name: self.default_name,
            sources: self.sources,
            modes: Default::default(),
            failed_probes: Default::default(),
            pending: None,
        }
    }
}

// ── Base URL assembly ─────────────────────────────────────────────────────────

/// The path segment a web prefix contributes to a base URL: `""` or `"/<prefix>"`.
///
/// One rule for every source. `servers.json` used to have no prefix at all, so the same deployment
/// resolved to a working URL through a VS Code Server Manager profile and to the gateway root
/// through `iris_add_server` (issue #129). Both branches of `load_pool` call this now, so a second
/// normalisation rule cannot drift in beside the first.
///
/// `None`, `""`, `"/"`, and whitespace all mean no prefix — a bare trailing slash here becomes a
/// doubled slash in the Atelier path, which is the Server Manager bug 089 fixed.
pub fn web_prefix_path_part(prefix: Option<&str>) -> String {
    let trimmed = prefix.unwrap_or_default().trim().trim_matches('/');
    if trimmed.is_empty() {
        String::new()
    } else {
        format!("/{trimmed}")
    }
}

/// The base URL for one `servers.json` entry. An entry whose scheme is not `http` or `https` has
/// no URL: this answers the unreachable sentinel, and [`native_connection`] reports the error.
pub fn native_base_url(entry: &crate::iris::servers_config::ServerEntry) -> String {
    crate::iris::servers_config::entry_endpoint(entry)
        .map(|e| e.base_url())
        .unwrap_or_else(|_| UNREACHABLE_BASE_URL.to_string())
}

/// A base URL nothing answers on, for a configured entry that could not be parsed.
pub const UNREACHABLE_BASE_URL: &str = "http://127.0.0.1:1";

/// A `servers.json` entry as a connection, without its password ([`native_password_in`] finds that).
/// The entry's own `tls_verify` decides its certificate checks.
pub fn native_connection(
    name: &str,
    entry: &crate::iris::servers_config::ServerEntry,
) -> Result<IrisConnection, crate::iris::endpoint::InvalidEndpoint> {
    use crate::iris::http::{TlsSetting, TlsSource};
    let endpoint = crate::iris::servers_config::entry_endpoint(entry)?;
    Ok(IrisConnection::new(
        endpoint.base_url(),
        entry.namespace.clone(),
        entry.username.clone(),
        "",
        DiscoverySource::ServerManager {
            server_name: name.to_string(),
        },
    )
    .with_tls(TlsSetting::declared(
        entry.tls_verify,
        TlsSource::ServersJson,
    )))
}

/// An `[instance.*]` block as a connection. Its own `tls_verify` decides its certificate checks;
/// the loader has already dropped a `false` that came from a file nobody trusted.
pub fn fleet_connection(
    name: &str,
    inst: &crate::iris::workspace_config::InstanceConfig,
) -> Result<IrisConnection, crate::iris::endpoint::InvalidEndpoint> {
    use crate::iris::http::{TlsSetting, TlsSource};
    let _ = name;
    let endpoint = crate::iris::workspace_config::instance_endpoint(inst)?;
    Ok(IrisConnection::new(
        endpoint.base_url(),
        inst.namespace.as_deref().unwrap_or("USER"),
        inst.username.as_deref().unwrap_or("_SYSTEM"),
        inst.password.as_deref().unwrap_or(""),
        DiscoverySource::EnvVar,
    )
    .with_tls(TlsSetting::declared(inst.tls_verify, TlsSource::ConfigFile)))
}

/// A Server Manager profile as a connection. Profiles carry no certificate setting: always checked.
pub fn server_manager_connection(
    profile: &crate::iris::server_manager::ServerManagerProfile,
    password: &str,
) -> Result<IrisConnection, crate::iris::endpoint::InvalidEndpoint> {
    use crate::iris::http::{TlsSetting, TlsSource};
    Ok(IrisConnection::new(
        profile.endpoint()?.base_url(),
        "USER", // SM profiles don't carry a namespace; default to USER
        profile.username.clone(),
        password,
        DiscoverySource::ServerManager {
            server_name: profile.name.clone(),
        },
    )
    .with_tls(TlsSetting::validating(TlsSource::ServerManager)))
}

/// The password for a `servers.json` entry, and the set-aside item when one was held back.
#[derive(Debug, Default)]
pub struct NativePassword {
    pub password: String,
    pub withheld: Option<crate::iris::config_sources::SetAsideItem>,
}

fn withheld_item(
    name: &str,
    id_suffix: &str,
    setting: &str,
    reason: String,
    lift: &str,
) -> crate::iris::config_sources::SetAsideItem {
    crate::iris::config_sources::SetAsideItem {
        id: format!("{name}:{id_suffix}"),
        part: None,
        kind: crate::iris::config_sources::SetAsideKind::CredentialWithheld,
        setting: setting.to_string(),
        server: Some(name.to_string()),
        file: None,
        reason,
        lift: lift.to_string(),
    }
}

/// The password `name`'s `servers.json` entry may use, looking in order at iad's own keychain item
/// for the entry's endpoint, the Server Manager's secret (only for an entry `iris_import_servers`
/// wrote, or a 1.6.x import whose endpoint still equals the profile's), and the plaintext password
/// saved for this endpoint.
pub fn native_password_in(
    name: &str,
    entry: &crate::iris::servers_config::ServerEntry,
    profiles: &[crate::iris::server_manager::ServerManagerProfile],
) -> NativePassword {
    use crate::iris::server_manager::{
        legacy_iad_secret, resolve_iad_credential, sm_secret_for_in, store_iad_credential,
        SmSecretError,
    };
    let Ok(endpoint) = crate::iris::servers_config::entry_endpoint(entry) else {
        return NativePassword::default();
    };

    if let Ok(pw) = resolve_iad_credential(name, &entry.username, &endpoint) {
        return NativePassword {
            password: pw,
            withheld: None,
        };
    }

    // An entry the Server Manager import wrote carries a marker; so does a 1.6.x import, in
    // effect, when a profile of the same name sits at the same endpoint.
    let profile_here = profiles.iter().find(|p| p.name == name);
    let imported = entry.credential.as_deref() == Some("server-manager")
        || (entry.credential.is_none()
            && entry.password.is_none()
            && profile_here
                .and_then(|p| p.endpoint().ok())
                .is_some_and(|pe| pe.same_server(&endpoint)));
    if imported {
        return match sm_secret_for_in(profiles, name, &entry.username, &endpoint) {
            Ok(pw) => NativePassword {
                password: pw,
                withheld: None,
            },
            Err(SmSecretError::Withheld { reason, .. }) => NativePassword {
                password: String::new(),
                withheld: Some(withheld_item(
                    name,
                    "server_manager",
                    "Server Manager keychain secret",
                    format!("not sent to {endpoint}: {reason}"),
                    "make the endpoints match, or give the entry its own password with iris_add_server",
                )),
            },
            Err(_) => NativePassword::default(),
        };
    }

    if let Some(pw) = entry.password.as_deref() {
        return if entry.password_endpoint.as_deref() == Some(endpoint.base_url().as_str()) {
            NativePassword {
                password: pw.to_string(),
                withheld: None,
            }
        } else {
            NativePassword {
                password: String::new(),
                withheld: Some(withheld_item(
                    name,
                    "stored_password",
                    "servers.json password",
                    format!(
                        "the password in servers.json was saved for {}, and this entry is {endpoint}",
                        entry.password_endpoint.as_deref().unwrap_or("no recorded endpoint")
                    ),
                    "run iris_add_server for this name with a password for the new endpoint",
                )),
            }
        };
    }

    // 1.6.x wrote its own passwords under the Server Manager's service. An entry that matches no
    // profile is iad's: copy the item to iad's account and leave the original where it is.
    if profile_here.is_none() {
        if let Some(pw) = legacy_iad_secret(name, &entry.username) {
            if store_iad_credential(name, &entry.username, &endpoint, &pw).is_ok() {
                tracing::info!("copied the keychain password of '{name}' to iad's own service");
            }
            return NativePassword {
                password: pw,
                withheld: None,
            };
        }
    }
    NativePassword::default()
}

/// Give a pool member its own restricted account and container (spec 141, R2).
///
/// The service password is iad's own keychain item for the account at this connection's endpoint
/// (the one place iad keeps a secret), then `plaintext_password`: what the file holds for an
/// account it names, which the caller has already checked is bound to this endpoint. A blank user
/// means the member has no account of its own. The member's `container` is the only container a
/// docker fallback may use for it; `IRIS_CONTAINER` belongs to the default connection.
///
/// [`name_service_account`] then [`fill_service_password`]: the pool runs the first at load and the
/// second when a request first uses the member.
pub fn apply_service_account(
    conn: &mut IrisConnection,
    server: &str,
    service_username: Option<&str>,
    plaintext_password: Option<&str>,
    container: Option<&str>,
) {
    name_service_account(conn, service_username, container);
    fill_service_password(conn, server, plaintext_password);
}

/// The part of [`apply_service_account`] that holds no secret: the account's name and the
/// container. A blank `service_username` leaves the member without an account.
pub fn name_service_account(
    conn: &mut IrisConnection,
    service_username: Option<&str>,
    container: Option<&str>,
) {
    let user = service_username.map(str::trim).filter(|u| !u.is_empty());
    if let Some(user) = user {
        conn.member.service_username = Some(user.to_string());
    }
    conn.member.container = container
        .map(str::trim)
        .filter(|c| !c.is_empty())
        .map(str::to_string);
}

/// The part of [`apply_service_account`] that reads the keychain: the password of the account
/// [`name_service_account`] named, if it named one.
pub fn fill_service_password(
    conn: &mut IrisConnection,
    server: &str,
    plaintext_password: Option<&str>,
) {
    use crate::iris::server_manager;
    let Some(user) = conn.member.service_username.clone() else {
        return;
    };
    let from_keychain = conn
        .endpoint()
        .ok()
        .and_then(|ep| server_manager::resolve_iad_credential(server, &user, &ep).ok());
    conn.member.service_password =
        Some(from_keychain.unwrap_or_else(|| plaintext_password.unwrap_or_default().to_string()));
}

// ── load_pool ─────────────────────────────────────────────────────────────────

/// Load a `ConnectionPool` from all configured sources in priority order.
///
/// Sources (first-wins name dedup across all sources):
/// 1. iad-native `servers.json` (`load_native_config`)
/// 2. VS Code / Cursor Server Manager `settings.json` (`parse_sm_settings`)
/// 3. `[instance.*]` blocks from workspace TOML (`load_fleet_config`)
/// 4. `IRIS_HOST` env var → `"_env"` entry
///
/// Default precedence: `servers.json` `"default"` field; then `"_env"` if present;
/// then implicit first entry (when pool has exactly one entry).
///
/// Never panics — any source that fails to load is silently skipped; an entry whose scheme is not
/// `http` or `https` is skipped with a warning.
pub fn load_pool(config_file: Option<&std::path::Path>) -> ConnectionPool {
    load_pool_from(
        crate::iris::servers_config::load_native_config(),
        crate::iris::server_manager::user_profiles(),
        config_file,
    )
}

/// [`load_pool`] over an already-read `servers.json` and Server Manager profile list, so a test
/// can hand it both as parsed text instead of writing into the process's home directory.
pub fn load_pool_from(
    native_cfg: crate::iris::servers_config::ServersConfig,
    profiles: Vec<crate::iris::server_manager::ServerManagerProfile>,
    config_file: Option<&std::path::Path>,
) -> ConnectionPool {
    // `config_file` is typically the path to `.iris-agentic-dev.toml` (a file).
    // `merged_fleet` accepts either a workspace directory or that file path.
    // The operator file's instances come first and win a name clash; the workspace file's follow.
    let fleet = crate::iris::workspace_config::merged_fleet(config_file.and_then(|p| p.to_str()));
    load_pool_from_fleet(native_cfg, profiles, fleet)
}

/// [`load_pool`] over the fleet a running server already holds. A server that reloads its pool in
/// mid-session must not go back to the files: an edit to the operator file that loosens waits for
/// the next start, and the files may already hold it (spec 141 FR-043).
pub fn load_pool_with_fleet(fleet: crate::iris::workspace_config::FleetConfig) -> ConnectionPool {
    load_pool_from_fleet(
        crate::iris::servers_config::load_native_config(),
        crate::iris::server_manager::user_profiles(),
        fleet,
    )
}

fn load_pool_from_fleet(
    native_cfg: crate::iris::servers_config::ServersConfig,
    profiles: Vec<crate::iris::server_manager::ServerManagerProfile>,
    fleet: crate::iris::workspace_config::FleetConfig,
) -> ConnectionPool {
    use crate::iris::server_manager;

    let mut builder = ConnectionPool::builder();
    let profiles = Arc::new(profiles);

    // ── Source 1: iad-native servers.json ────────────────────────────────────
    let native_default = native_cfg.default.clone();
    for (name, entry) in &native_cfg.servers {
        let is_default = native_default.as_deref() == Some(name.as_str());
        let mut conn = match native_connection(name, entry) {
            Ok(c) => c,
            Err(e) => {
                tracing::warn!("INVALID_CONFIG: servers.json entry '{name}' skipped: {e}");
                continue;
            }
        };
        name_service_account(
            &mut conn,
            entry.service_username.as_deref(),
            entry.container.as_deref(),
        );
        let (member, entry, profiles) = (name.clone(), entry.clone(), profiles.clone());
        builder.add_deferred(name.clone(), conn, is_default, "iad-native", move |conn| {
            let found = native_password_in(&member, &entry, &profiles);
            conn.password = found.password;
            // The service password is bound to the endpoint like the primary one: a plaintext
            // copy saved for another endpoint is not sent here.
            let plaintext_service_password = entry
                .service_password
                .as_deref()
                .filter(|_| entry.password_endpoint.as_deref() == Some(conn.base_url.as_str()));
            fill_service_password(conn, &member, plaintext_service_password);
            found.withheld.into_iter().collect()
        });
    }

    // ── Source 2: VS Code / Cursor Server Manager settings.json ──────────────
    for profile in profiles.iter() {
        let endpoint = match profile.endpoint() {
            Ok(e) => e,
            Err(e) => {
                tracing::warn!(
                    "INVALID_CONFIG: Server Manager profile '{}' skipped: {e}",
                    profile.name
                );
                continue;
            }
        };
        if let Ok(conn) = server_manager_connection(profile, "") {
            let (profile, profiles) = (profile.clone(), profiles.clone());
            builder.add_deferred(profile.name.clone(), conn, false, "vscode", move |conn| {
                conn.password = server_manager::sm_secret_for_in(
                    &profiles,
                    &profile.name,
                    &profile.username,
                    &endpoint,
                )
                .unwrap_or_else(|_| profile.password_deprecated.clone().unwrap_or_default());
                Vec::new()
            });
        }
    }

    // ── Source 3: [instance.*] blocks from the configuration files ───────────
    if fleet.mode.as_deref() == Some("operate") {
        for (name, inst) in &fleet.instance {
            match fleet_connection(name, inst) {
                Ok(mut conn) => {
                    name_service_account(
                        &mut conn,
                        inst.service_username.as_deref(),
                        inst.container.as_deref(),
                    );
                    let (member, plaintext) = (name.clone(), inst.service_password.clone());
                    builder.add_deferred(name.clone(), conn, false, "fleet", move |conn| {
                        fill_service_password(conn, &member, plaintext.as_deref());
                        Vec::new()
                    });
                }
                Err(e) => tracing::warn!("INVALID_CONFIG: instance '{name}' skipped: {e}"),
            }
        }
    }

    // ── Source 4: IRIS_HOST env var → "_env" ─────────────────────────────────
    if let Ok(iris_host) = std::env::var("IRIS_HOST") {
        if !iris_host.is_empty() {
            let port = std::env::var("IRIS_WEB_PORT")
                .ok()
                .and_then(|p| p.parse::<u16>().ok())
                .unwrap_or(52773);
            let scheme = std::env::var("IRIS_SCHEME").ok();
            let prefix = std::env::var("IRIS_WEB_PREFIX").ok();
            match crate::iris::endpoint::Endpoint::parse(
                scheme.as_deref(),
                &iris_host,
                port,
                prefix.as_deref(),
            ) {
                Ok(endpoint) => {
                    let ns = std::env::var("IRIS_NAMESPACE").unwrap_or_else(|_| "USER".to_string());
                    let user =
                        std::env::var("IRIS_USERNAME").unwrap_or_else(|_| "_SYSTEM".to_string());
                    let pw = std::env::var("IRIS_PASSWORD").unwrap_or_default();
                    // The launch settings describe this connection, so their certificate setting
                    // applies to it.
                    let conn = IrisConnection::new(
                        endpoint.base_url(),
                        ns,
                        user,
                        pw,
                        DiscoverySource::EnvVar,
                    )
                    .with_tls(crate::iris::http::TlsSetting::from_launch_env());
                    // "_env" is the default if no native default was set
                    let is_default = builder.default_name.is_none();
                    builder.add_with_source("_env".to_string(), conn, is_default, "env");
                }
                Err(e) => tracing::warn!("INVALID_CONFIG: IRIS_HOST/IRIS_SCHEME: {e}"),
            }
        }
    }

    builder.build()
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn make_conn(base_url: &str) -> IrisConnection {
        IrisConnection::new(base_url, "USER", "_SYSTEM", "", DiscoverySource::EnvVar)
    }

    fn pool_with_one(name: &str, base_url: &str) -> ConnectionPool {
        let mut b = ConnectionPool::builder();
        b.add(name.to_string(), make_conn(base_url), false);
        b.build()
    }

    // T011.1 — empty pool, get(None) returns IRIS_UNREACHABLE
    #[test]
    fn get_none_on_empty_pool_returns_unreachable() {
        let pool = ConnectionPool::empty();
        let err = pool.get(None).unwrap_err();
        let msg: &str = &err.message;
        assert!(
            msg.contains("IRIS_UNREACHABLE"),
            "expected IRIS_UNREACHABLE, got: {msg}"
        );
    }

    // T011.2 — pool with one entry "a", get(Some("x")) returns SERVER_NOT_FOUND
    #[test]
    fn get_named_missing_returns_not_found() {
        let pool = pool_with_one("a", "http://localhost:52773");
        let err = pool.get(Some("x")).unwrap_err();
        let msg: &str = &err.message;
        assert!(
            msg.contains("SERVER_NOT_FOUND"),
            "expected SERVER_NOT_FOUND, got: {msg}"
        );
    }

    // T011.3 — pool with entry "a", get(Some("a")) returns the right connection
    #[test]
    fn get_named_present_returns_connection() {
        let pool = pool_with_one("a", "http://localhost:52773");
        let conn = pool.get(Some("a")).expect("should find 'a'");
        assert_eq!(conn.base_url, "http://localhost:52773");
    }

    // T011.4 — pool with "a" and "b", default="a", get(None) returns "a"
    #[test]
    fn get_none_with_default_returns_default() {
        let mut b = ConnectionPool::builder();
        b.add("a".to_string(), make_conn("http://a:52773"), true);
        b.add("b".to_string(), make_conn("http://b:52773"), false);
        let pool = b.build();
        let conn = pool.get(None).expect("should return default 'a'");
        assert_eq!(conn.base_url, "http://a:52773");
    }

    // T011.5 — pool with 3 entries, len() == 3
    #[test]
    fn len_returns_correct_count() {
        let mut b = ConnectionPool::builder();
        b.add("a".to_string(), make_conn("http://a:52773"), false);
        b.add("b".to_string(), make_conn("http://b:52773"), false);
        b.add("c".to_string(), make_conn("http://c:52773"), false);
        let pool = b.build();
        assert_eq!(pool.len(), 3);
    }

    // T011.5b — default_name set but missing from instances map → IRIS_UNREACHABLE
    #[test]
    fn get_none_with_missing_default_returns_unreachable() {
        // Build a pool where default_name points to a name not in instances.
        // This exercises the Some(default) branch where instances.get() returns None.
        let mut pool = ConnectionPool::empty();
        pool.default_name = Some("missing".to_string());
        let err = pool.get(None).unwrap_err();
        let msg: &str = &err.message;
        assert!(
            msg.contains("IRIS_UNREACHABLE"),
            "expected IRIS_UNREACHABLE, got: {msg}"
        );
        assert!(
            msg.contains("missing"),
            "message should name the missing default, got: {msg}"
        );
    }

    // T011.6 — cascade: first-wins dedup — "dev" added from native source wins over vscode source
    #[test]
    fn cascade_iad_native_wins_over_vscode() {
        let mut b = ConnectionPool::builder();
        // First source (iad-native): "dev" at port 52780
        b.add("dev".to_string(), make_conn("http://localhost:52780"), true);
        // Second source (vscode): "dev" at port 52773 — should be silently dropped
        b.add(
            "dev".to_string(),
            make_conn("http://localhost:52773"),
            false,
        );
        let pool = b.build();
        // Only one "dev" in pool
        assert_eq!(pool.len(), 1);
        let conn = pool.get(Some("dev")).expect("should find 'dev'");
        assert_eq!(
            conn.base_url, "http://localhost:52780",
            "native source should win over vscode source"
        );
    }

    // Fleet [instance.*] must load when callers pass the toml *file* path (not only
    // the parent directory). Regression: load_pool used to pass config_file into
    // load_fleet_config, which joined `.iris-agentic-dev.toml` onto the file path
    // and silently skipped operate-mode instances — so server="<fleet-name>" failed.
    #[test]
    fn load_pool_includes_fleet_instances_from_config_file_path() {
        let dir = tempfile::tempdir().unwrap();
        let cfg = dir.path().join(".iris-agentic-dev.toml");
        std::fs::write(
            &cfg,
            r#"mode = "operate"

[instance.iad-dev]
host = "dev.example.com"
web_port = 443
scheme = "https"
namespace = "DEVNS"
username = "u"
password = "p"

[instance.iad-prod]
host = "prod.example.com"
web_port = 443
scheme = "https"
namespace = "PROD"
role = "subject"
"#,
        )
        .unwrap();

        let pool = load_pool(Some(&cfg));
        assert_eq!(
            pool.source_of("iad-dev"),
            "fleet",
            "iad-dev must come from fleet toml"
        );
        assert_eq!(pool.source_of("iad-prod"), "fleet");
        let dev = pool.get(Some("iad-dev")).expect("iad-dev in pool");
        assert_eq!(dev.base_url, "https://dev.example.com:443");
        assert_eq!(dev.namespace, "DEVNS");
        let prod = pool.get(Some("iad-prod")).expect("iad-prod in pool");
        assert_eq!(prod.base_url, "https://prod.example.com:443");
        assert_eq!(prod.namespace, "PROD");
    }

    #[test]
    fn load_pool_includes_fleet_instances_from_workspace_directory() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join(".iris-agentic-dev.toml"),
            r#"mode = "operate"

[instance.iad-only]
host = "only.example.com"
web_port = 52773
namespace = "USER"
"#,
        )
        .unwrap();

        let pool = load_pool(Some(dir.path()));
        assert_eq!(pool.source_of("iad-only"), "fleet");
        let conn = pool.get(Some("iad-only")).expect("iad-only in pool");
        assert_eq!(conn.base_url, "http://only.example.com:52773");
    }
}

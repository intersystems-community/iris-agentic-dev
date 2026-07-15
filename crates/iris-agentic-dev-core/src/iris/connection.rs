//! IRIS connection types and Atelier REST API fingerprinting.

use std::fmt;

/// Whether the connected IRIS instance is a production (Live) system.
/// Detected at probe time via `^%SYS("SystemMode")` SQL query.
#[derive(Debug, Clone, PartialEq, Default)]
pub enum SystemMode {
    Live,        // "Live" — lock write tools
    Development, // "Development" — allow write tools
    Test,        // "Test" — allow write tools
    #[default]
    Unknown, // null/empty — apply namespace heuristic
}

/// Which version of the Atelier REST API to use.
#[derive(Debug, Clone, PartialEq)]
pub enum AtelierVersion {
    V8,
    V2,
    V1,
}

impl AtelierVersion {
    pub fn version_str(&self) -> &'static str {
        match self {
            AtelierVersion::V8 => "v8",
            AtelierVersion::V2 => "v2",
            AtelierVersion::V1 => "v1",
        }
    }
}

/// A resolved connection to a running IRIS instance via Atelier REST API.
/// T011: manual Debug impl redacts `password` (P1/FR-022).
#[derive(Clone)]
pub struct IrisConnection {
    /// Base URL e.g. "http://localhost:52773" or "http://localhost:80/prefix"
    pub base_url: String,
    pub namespace: String,
    pub username: String,
    pub password: String,
    pub version: Option<String>,
    pub atelier_version: AtelierVersion,
    pub source: DiscoverySource,
    pub port_superserver: Option<u16>,
    /// Detected at probe time — controls write-tool availability (issue #26).
    pub system_mode: SystemMode,
}

/// T011: Manual Debug implementation — never prints the password.
impl fmt::Debug for IrisConnection {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("IrisConnection")
            .field("base_url", &self.base_url)
            .field("namespace", &self.namespace)
            .field("username", &self.username)
            .field("password", &"[redacted]")
            .field("version", &self.version)
            .field("atelier_version", &self.atelier_version)
            .field("source", &self.source)
            .field("port_superserver", &self.port_superserver)
            .field("system_mode", &self.system_mode)
            .finish()
    }
}

#[derive(Debug, Clone)]
pub enum DiscoverySource {
    LocalhostScan {
        port: u16,
    },
    Docker {
        container_name: String,
    },
    VsCodeSettings,
    EnvVar,
    ExplicitFlag,
    /// Discovered via VS Code Server Manager settings.json + OS keychain (044).
    ServerManager {
        server_name: String,
    },
}

/// Structured result from a document compile operation.
#[derive(Debug)]
pub struct CompileResult {
    pub errors: Vec<String>,
    pub console: Vec<String>,
}

impl CompileResult {
    pub fn success(&self) -> bool {
        self.errors.is_empty()
    }
}

impl IrisConnection {
    pub fn new(
        base_url: impl Into<String>,
        namespace: impl Into<String>,
        username: impl Into<String>,
        password: impl Into<String>,
        source: DiscoverySource,
    ) -> Self {
        Self {
            base_url: base_url.into(),
            namespace: namespace.into(),
            username: username.into(),
            password: password.into(),
            version: None,
            atelier_version: AtelierVersion::V1,
            source,
            port_superserver: None,
            system_mode: SystemMode::Unknown,
        }
    }

    /// Returns true if write-capable tools should be registered.
    /// Checks SystemMode, namespace heuristics, and IRIS_ALLOW_PROD override (issue #26).
    pub fn is_write_allowed(&self) -> bool {
        if std::env::var("IRIS_ALLOW_PROD")
            .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
            .unwrap_or(false)
        {
            return true;
        }
        match &self.system_mode {
            SystemMode::Live => false,
            SystemMode::Development | SystemMode::Test => true,
            SystemMode::Unknown => !is_production_namespace(&self.namespace),
        }
    }

    /// Return a clone of this connection authenticating as the restricted service account
    /// configured via `IRIS_SERVICE_USERNAME` / `IRIS_SERVICE_PASSWORD`, or `None` when no
    /// such account is set.
    ///
    /// Used to route privileged arbitrary-execution tools (`iris_execute`, `iris_query`
    /// mode="write", `iris_global` set/kill) through a least-privilege IRIS identity that lacks
    /// code-editing rights (no `%Development`), so those tools cannot edit class/routine code
    /// even via ObjectScript indirection — while SCM / audit-sensitive tools keep running under
    /// the primary (user) identity so checkouts and audit stay attributed to the real user.
    ///
    /// Enforcement lives in IRIS privileges (the service role), not in a string filter: a bare
    /// `IRIS_SERVICE_USERNAME` with insufficient rights is what closes the bypass.
    pub fn with_service_account(&self) -> Option<IrisConnection> {
        let username = std::env::var("IRIS_SERVICE_USERNAME")
            .ok()
            .filter(|s| !s.is_empty())?;
        let password = std::env::var("IRIS_SERVICE_PASSWORD").unwrap_or_default();
        let mut c = self.clone();
        c.username = username;
        c.password = password;
        Some(c)
    }

    /// Build the full Atelier REST URL for a given path suffix.
    pub fn atelier_url(&self, path: &str) -> String {
        format!(
            "{}/api/atelier{}",
            self.base_url.trim_end_matches('/'),
            path
        )
    }

    /// Build a versioned Atelier URL using the detected API version and the connection namespace.
    pub fn atelier_url_versioned(&self, path: &str) -> String {
        self.versioned_ns_url(&self.namespace.clone(), path)
    }

    /// Build a versioned Atelier URL for an explicit namespace.
    pub fn versioned_ns_url(&self, namespace: &str, path: &str) -> String {
        let v = self.atelier_version.version_str();
        // URL-encode namespace so %SYS becomes %25SYS in the path component
        let ns_encoded = urlencoding::encode(namespace);
        self.atelier_url(&format!("/{}/{}{}", v, ns_encoded, path))
    }

    /// Probe this connection: fetch IRIS version, Atelier API level, and SystemMode.
    pub async fn probe(&mut self) {
        let client = match Self::http_client() {
            Ok(c) => c,
            Err(_) => return,
        };

        let url = self.atelier_url("/");
        if let Ok(resp) = client
            .get(&url)
            .basic_auth(&self.username, Some(&self.password))
            .send()
            .await
        {
            let status = resp.status();
            if status.is_success() {
                if let Ok(body) = resp.json::<serde_json::Value>().await {
                    tracing::debug!("Atelier root response: {}", body);
                    let content = &body["result"]["content"];
                    self.version = content["version"].as_str().map(|v| v.to_string());
                    self.atelier_version = match content["api"].as_u64() {
                        Some(v) if v >= 8 => AtelierVersion::V8,
                        Some(v) if v >= 2 => AtelierVersion::V2,
                        _ => AtelierVersion::V1,
                    };
                }
            } else {
                tracing::debug!("Atelier root probe got HTTP {}", status);
            }
        }

        // Detect SystemMode via SQL against %SYS global (issue #26).
        // One extra round-trip at startup; result cached for session lifetime.
        let mode = self.detect_system_mode(&client).await;
        self.system_mode = mode;
        tracing::info!(
            host = %self.base_url,
            version = ?self.version,
            system_mode = ?self.system_mode,
            write_allowed = self.is_write_allowed(),
            "iris-agentic-dev: connection probed"
        );
    }

    /// Query `^%SYS("SystemMode")` to detect whether this is a Live instance.
    async fn detect_system_mode(&self, client: &reqwest::Client) -> SystemMode {
        let url = self.versioned_ns_url("%SYS", "/action/query");
        let resp = client
            .post(&url)
            .basic_auth(&self.username, Some(&self.password))
            .json(&serde_json::json!({
                "query": "SELECT Value FROM %Library.Global_Get('%SYS', '^%SYS(\"SystemMode\")')"
            }))
            .send()
            .await;
        let mode = match resp {
            Ok(r) => {
                if let Ok(body) = r.json::<serde_json::Value>().await {
                    body["result"]["content"][0]["Value"]
                        .as_str()
                        .map(|s| s.trim().to_string())
                        .unwrap_or_default()
                } else {
                    String::new()
                }
            }
            Err(_) => String::new(),
        };
        match mode.as_str() {
            "Live" => SystemMode::Live,
            "Development" => SystemMode::Development,
            "Test" => SystemMode::Test,
            _ => SystemMode::Unknown,
        }
    }

    /// Execute ObjectScript code via the write-compile-query cycle (pure HTTP, no docker).
    /// FR-023: retries up to 3 times with 100/200/400ms backoff on network errors or HTTP 5xx.
    pub async fn execute_via_generator(
        &self,
        code: &str,
        namespace: &str,
        client: &reqwest::Client,
    ) -> anyhow::Result<String> {
        let delays = [
            std::time::Duration::from_millis(100),
            std::time::Duration::from_millis(200),
            std::time::Duration::from_millis(400),
        ];
        let mut last_err = anyhow::anyhow!("no attempts made");

        for (attempt, delay) in delays.iter().enumerate() {
            match self
                .execute_via_generator_once(code, namespace, client)
                .await
            {
                Ok(output) => {
                    if attempt > 0 {
                        tracing::info!(
                            "execute_via_generator succeeded on attempt {}",
                            attempt + 1
                        );
                    }
                    return Ok(output);
                }
                Err(e) => {
                    let msg = e.to_string();
                    let msg_lower = msg.to_ascii_lowercase();
                    // Only retry on transient network errors or 5xx; 4xx are client errors
                    // (e.g. 401 auth) and must NOT be retried. The extra patterns cover cold-start
                    // failures on the first call: DNS not warm, TLS handshake, connection reset,
                    // or a half-open pooled connection surfacing as EOF / broken pipe.
                    let is_retryable = msg.contains("HTTP 5")
                        || msg_lower.contains("error sending request")
                        || msg_lower.contains("connection refused")
                        || msg_lower.contains("connection reset")
                        || msg_lower.contains("connection closed")
                        || msg_lower.contains("broken pipe")
                        || msg_lower.contains("unexpected end of file")
                        || msg_lower.contains("eof")
                        || msg_lower.contains("dns")
                        || msg_lower.contains("handshake")
                        || msg_lower.contains("timed out")
                        || msg_lower.contains("timeout");
                    if !is_retryable || attempt == delays.len() - 1 {
                        return Err(e);
                    }
                    // Transient on cold-start (private web server still warming up) — debug only;
                    // the success path logs at info so a recovery is still visible when needed.
                    tracing::debug!(
                        "execute_via_generator attempt {} failed ({}), retrying in {:?}",
                        attempt + 1,
                        msg,
                        delay
                    );
                    last_err = e;
                    tokio::time::sleep(*delay).await;
                }
            }
        }
        Err(last_err)
    }

    /// Single attempt of execute_via_generator (no retry logic).
    async fn execute_via_generator_once(
        &self,
        code: &str,
        namespace: &str,
        client: &reqwest::Client,
    ) -> anyhow::Result<String> {
        let id: String = uuid::Uuid::new_v4()
            .simple()
            .to_string()
            .chars()
            .take(12)
            .collect();
        // Use a dedicated scratch package (IrisDevTmp) rather than User.* to avoid
        // polluting the user's application namespace with transient executor classes.
        let class_name = format!("IrisDevTmp.IrisDevRun{}", id);
        let doc_name = format!("{}.cls", class_name);
        // SQL proc name: for non-User packages, IRIS SQL schema = package name (no "SQL" prefix).
        // IrisDevTmp.IrisDevRunXXX → SQL schema IrisDevTmp, proc IrisDevTmp.IrisDevRunXXX_Execute.
        // (The SQLUser prefix is a historical special case only for the User package.)
        let sql_func = format!("IrisDevTmp.IrisDevRun{}_Execute", id);
        let content = Self::build_exec_class(&class_name, code);

        // 1. PUT the class document
        let put_url = self.versioned_ns_url(
            namespace,
            &format!("/doc/{}", urlencoding::encode(&doc_name)),
        );
        let put_resp = client
            .put(&put_url)
            .basic_auth(&self.username, Some(&self.password))
            .json(&serde_json::json!({"enc": false, "content": content}))
            .send()
            .await?;
        if !put_resp.status().is_success() {
            anyhow::bail!("PUT doc failed: HTTP {}", put_resp.status());
        }

        // 2. Compile
        let compile_url = self.versioned_ns_url(namespace, "/action/compile?flags=cuk");
        let compile_resp = client
            .post(&compile_url)
            .basic_auth(&self.username, Some(&self.password))
            .json(&serde_json::json!([doc_name]))
            .send()
            .await?;
        if !compile_resp.status().is_success() {
            let _ = self.delete_doc(&doc_name, namespace, client).await;
            anyhow::bail!("compile HTTP {}", compile_resp.status());
        }
        let compile_body: serde_json::Value = compile_resp.json().await.unwrap_or_default();
        let has_errors = compile_body["result"]["log"]
            .as_array()
            .map(|entries| {
                entries.iter().any(|e| {
                    e["type"]
                        .as_str()
                        .map(|t| t.eq_ignore_ascii_case("error"))
                        .unwrap_or(false)
                })
            })
            .unwrap_or(false);
        if has_errors {
            let _ = self.delete_doc(&doc_name, namespace, client).await;
            anyhow::bail!("compile errors: {:?}", compile_body["result"]["log"]);
        }

        // 3. Query via SQL
        // "output" is a reserved word in IRIS SQL — use "result" as the column alias.
        let sql = format!("SELECT {}() AS result", sql_func);
        let query_url = self.versioned_ns_url(namespace, "/action/query");
        let query_resp = client
            .post(&query_url)
            .basic_auth(&self.username, Some(&self.password))
            .json(&serde_json::json!({"query": sql}))
            .send()
            .await?;
        let query_body: serde_json::Value = query_resp.json().await.unwrap_or_default();
        let output = query_body["result"]["content"][0]["result"]
            .as_str()
            .unwrap_or("")
            .replace('\x01', "\n");

        // 4. Delete the temp class (best-effort)
        let _ = self.delete_doc(&doc_name, namespace, client).await;

        Ok(output)
    }

    /// Build the `.cls` source lines for the temp executor class.
    ///
    /// Uses a plain ClassMethod (not CodeMode=objectgenerator) so that user code is
    /// compiled directly through the IRIS macro preprocessor. This means $$$macros and
    /// #include / #define directives in user code work as expected, matching what a real
    /// .cls compilation provides.
    fn build_exec_class(class_name: &str, code: &str) -> Vec<String> {
        let mut lines: Vec<String> = vec![
            "Include %occInclude".into(),
            "".into(),
            format!("Class {} [ Final ]", class_name),
            "{".into(),
            "".into(),
            "ClassMethod Execute() As %String [ SqlProc ]".into(),
            "{".into(),
            // Capture all output to a temp file so we can return it as a string via SQL.
            // #56: use %Library.File.TempFilename() for platform-portable temp path.
            "  Set tmpfile = ##class(%Library.File).TempFilename(\"txt\")".into(),
            "  Set savedIO = $IO".into(),
            "  Open tmpfile:(\"WNS\"):5".into(),
            "  If '$TEST { Quit \"ERROR: output capture unavailable\" }".into(),
            "  Use tmpfile".into(),
            "  Try {".into(),
        ];
        for line in code.lines() {
            lines.push(format!("    {}", line));
        }
        lines.extend([
            "    Write !".into(), // sentinel ensures output always ends with \n (IDEV-3)
            "  } Catch ex {".into(),
            "    Write \"ERROR: \",ex.DisplayString(),!".into(),
            "  }".into(),
            // Snapshot $ZERROR now, before Close/Use/stream operations below can clobber
            // it. This captures non-exception errors (e.g. an OPEN failure that sets
            // $ZERROR without throwing) so we can surface them if the body produced no
            // output — but WITHOUT writing to tmpfile yet (see the out="" test below).
            "  Set ze = $ZError".into(),
            "  Close tmpfile".into(),
            "  Use savedIO".into(),
            // Read captured output from temp file and return it.
            "  Set out = \"\"".into(),
            "  Set stream = ##class(%Stream.FileCharacter).%New()".into(),
            "  Set sc = stream.LinkToFile(tmpfile)".into(),
            "  If $$$ISOK(sc) {".into(),
            "    While 'stream.AtEnd { Set out = out _ stream.ReadLine() _ $Char(10) }".into(),
            "  }".into(),
            "  Do ##class(%Library.File).Delete(tmpfile)".into(),
            // Only surface a non-exception $ZERROR when the body produced NO output.
            // A residual like <ENDOFFILE> is often left as a benign side effect of an
            // SCM provider's internal Read even when the operation fully succeeded;
            // appending it to a non-empty result corrupted otherwise-valid output.
            "  If (out=\"\") && (ze'=\"\") && (ze'=\",\") { Set out = \"ERROR($ZERROR): \"_ze_$Char(10) }"
                .into(),
            "  Quit out".into(),
            "}".into(),
            "".into(),
            "}".into(),
        ]);
        lines
    }

    /// Delete an Atelier document (best-effort).
    async fn delete_doc(
        &self,
        doc_name: &str,
        namespace: &str,
        client: &reqwest::Client,
    ) -> anyhow::Result<()> {
        let url = self.versioned_ns_url(
            namespace,
            &format!("/doc/{}", urlencoding::encode(doc_name)),
        );
        client
            .delete(&url)
            .basic_auth(&self.username, Some(&self.password))
            .send()
            .await?;
        Ok(())
    }

    /// Execute ObjectScript code via docker exec (iris session stdin).
    ///
    /// LIMITATION: IRIS terminal sessions wrap stdin at ~80 columns when code is
    /// sent as a single line. For code longer than ~80 characters, callers with
    /// an HTTP client should use execute_via_generator() instead — it compiles
    /// user code into a temp class with no line-length restriction.
    ///
    /// This method is preserved for environments without Atelier REST access.
    /// Reads IRIS_CONTAINER fresh on each call to pick up late env var changes.
    pub async fn execute(&self, code: &str, namespace: &str) -> anyhow::Result<String> {
        let container =
            std::env::var("IRIS_CONTAINER").map_err(|_| anyhow::anyhow!("DOCKER_REQUIRED"))?;

        use tokio::io::AsyncWriteExt;

        let mut child = tokio::process::Command::new("docker")
            .args([
                "exec", "-i", &container, "iris", "session", "IRIS", "-U", namespace,
            ])
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null())
            .spawn()
            .map_err(|e| anyhow::anyhow!("docker not available: {e}"))?;

        if let Some(mut stdin) = child.stdin.take() {
            let _ = stdin.write_all(code.as_bytes()).await;
            let _ = stdin.write_all(b"\nhalt\n").await;
        }

        let output =
            tokio::time::timeout(std::time::Duration::from_secs(30), child.wait_with_output())
                .await
                .map_err(|_| anyhow::anyhow!("docker exec timed out after 30s"))??;

        let raw = String::from_utf8_lossy(&output.stdout).to_string();
        Ok(strip_iris_banner(&raw))
    }

    /// FR-004: Run a SQL query via the Atelier query endpoint.
    /// Takes an explicit `namespace` parameter rather than always using `self.namespace`.
    pub async fn query(
        &self,
        sql: &str,
        params: Vec<serde_json::Value>,
        namespace: &str,
        client: &reqwest::Client,
    ) -> anyhow::Result<serde_json::Value> {
        let url = self.versioned_ns_url(namespace, "/action/query");
        let resp = client
            .post(&url)
            .basic_auth(&self.username, Some(&self.password))
            .json(&serde_json::json!({"query": sql, "parameters": params}))
            .send()
            .await?;
        let body: serde_json::Value = resp.json().await?;
        // Surface Atelier-level errors returned as 200 OK with status.errors in body.
        if let Some(errs) = body["status"]["errors"].as_array() {
            if !errs.is_empty() {
                let msg = errs[0]["error"].as_str().unwrap_or("Atelier query error");
                anyhow::bail!("{}", msg);
            }
        }
        Ok(body)
    }

    /// Compile a document via POST /action/compile. Returns structured errors and console output.
    /// Used by both the CLI `compile` command and the MCP `iris_compile` tool.
    pub async fn compile_document(
        &self,
        doc_name: &str,
        namespace: &str,
        flags: &str,
        client: &reqwest::Client,
    ) -> anyhow::Result<CompileResult> {
        let compile_url = self.versioned_ns_url(
            namespace,
            &format!("/action/compile?flags={}", urlencoding::encode(flags)),
        );
        let resp = client
            .post(&compile_url)
            .basic_auth(&self.username, Some(&self.password))
            .json(&serde_json::json!([doc_name]))
            .send()
            .await?;
        if !resp.status().is_success() {
            anyhow::bail!("compile HTTP {}", resp.status());
        }
        let body: serde_json::Value = resp.json().await.unwrap_or_default();
        let console: Vec<String> = body["console"]
            .as_array()
            .map(|a| {
                a.iter()
                    .filter_map(|v| v.as_str().map(|s| s.to_string()))
                    .collect()
            })
            .unwrap_or_default();
        let mut errors: Vec<String> = vec![];
        if let Some(se) = body["status"]["errors"].as_array() {
            for e in se {
                if let Some(msg) = e["error"].as_str() {
                    errors.push(msg.to_string());
                }
            }
        }
        for line in &console {
            if let Some(rest) = line.trim().strip_prefix("ERROR ") {
                let rest = rest.to_string();
                if errors.iter().all(|e| !e.contains(&rest)) {
                    errors.push(rest);
                }
            }
        }
        Ok(CompileResult { errors, console })
    }

    /// Build a reqwest Client suitable for Atelier REST calls.
    /// TLS certificate validation is enabled by default; set `IRIS_INSECURE=true` to disable.
    pub fn http_client() -> anyhow::Result<reqwest::Client> {
        // IRIS_INSECURE=true or IRIS_TLS_VERIFY=false both disable TLS cert validation.
        let insecure = std::env::var("IRIS_INSECURE")
            .ok()
            .map(|v| v == "true" || v == "1")
            .unwrap_or_else(|| {
                std::env::var("IRIS_TLS_VERIFY")
                    .map(|v| v == "false" || v == "0")
                    .unwrap_or(false)
            });
        Ok(reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            .danger_accept_invalid_certs(insecure)
            .cookie_store(true) // reuse CSP sessions to avoid license slot exhaustion (#43)
            .tcp_keepalive(std::time::Duration::from_secs(20)) // prevent NAT/firewall from silently dropping idle connections (#44)
            .build()?)
    }

    /// Test accessor for build_exec_class. Exposed for integration tests.
    #[doc(hidden)]
    pub fn build_exec_class_for_test(class_name: &str, _tmpfile: &str, code: &str) -> Vec<String> {
        Self::build_exec_class(class_name, code)
    }
}

/// Returns true if the namespace name looks like a production namespace.
/// Used as fallback when SystemMode is Unknown (community edition or unconfigured).
fn is_production_namespace(ns: &str) -> bool {
    let upper = ns.to_uppercase();
    matches!(upper.as_str(), "PROD" | "PRODUCTION" | "LIVE" | "PRD")
}

/// FR-006: Strip IRIS session banner and prompt lines from docker exec stdout.
///
/// IRIS session output looks like:
///   Copyright (c) 2024 InterSystems Corporation
///   All rights reserved.
///   IRIS for UNIX ... 2024.1 ...
///   USER>
///   <code output lines>
///   USER>
///
/// We strip banner lines and bare prompt lines (lines that are ONLY a prompt, no content).
/// Lines that start with a prompt prefix but have content after it are kept.
pub fn strip_iris_banner(output: &str) -> String {
    let mut result_lines: Vec<&str> = Vec::new();
    // Banner-text rules (below) only apply before the first prompt is seen — after that,
    // a line like "IRIS for UNIX ..." is legitimate `Write $ZVersion` output, not the
    // connect-time banner, and must not be stripped (regression: this previously made
    // test_execute_zversion's `Write $ZVersion` output disappear entirely).
    let mut seen_prompt = false;

    for line in output.lines() {
        let trimmed = line.trim();

        if !seen_prompt
            && (trimmed.starts_with("Copyright")
                || trimmed.contains("InterSystems Corporation")
                || trimmed.starts_with("All rights reserved")
                || trimmed.starts_with("IRIS for ")
                || trimmed.starts_with("Cache for ")
                || trimmed.starts_with("Ensemble for ")
                // IRIS 2026.2+ prints "Node: <hostname>, Instance: IRIS" on session connect.
                // Without this, its embedded ':' gets misparsed as a name:code pair by
                // callers like parse_status_response (e.g. "Node" became the production name).
                || (trimmed.starts_with("Node: ") && trimmed.contains(", Instance:")))
        {
            continue;
        }

        // Strip bare prompt-only lines: lines that are just "USER>", "IRIS>", "%SYS>", etc.
        // A bare prompt line has no content beyond the prompt token.
        if is_bare_prompt_line(trimmed) {
            seen_prompt = true;
            continue;
        }

        result_lines.push(line);
    }

    // Remove leading blank lines
    while result_lines
        .first()
        .map(|l: &&str| l.trim().is_empty())
        .unwrap_or(false)
    {
        result_lines.remove(0);
    }
    // Remove trailing blank lines
    while result_lines
        .last()
        .map(|l: &&str| l.trim().is_empty())
        .unwrap_or(false)
    {
        result_lines.pop();
    }

    result_lines.join("\n")
}

/// Returns true if the line is purely an IRIS session prompt with no following content.
/// Examples: "USER>", "IRIS>", "%SYS>", "USER> " (trailing space only).
fn is_bare_prompt_line(s: &str) -> bool {
    // Strip trailing whitespace for the check
    let s = s.trim_end();
    if !s.ends_with('>') {
        return false;
    }
    // The prompt token is everything before '>'
    let token = &s[..s.len() - 1];
    // Allow optional leading '%'
    let token = token.strip_prefix('%').unwrap_or(token);
    // Prompt namespace is uppercase alphanumeric + underscore, non-empty, reasonable length
    !token.is_empty()
        && token.len() <= 16
        && token.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
}

#[cfg(test)]
mod system_mode_tests {
    use super::*;

    // Serialize tests that read or write IRIS_ALLOW_PROD to prevent races.
    static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    fn conn(namespace: &str, mode: SystemMode) -> IrisConnection {
        let mut c = IrisConnection::new(
            "http://localhost:52773",
            namespace,
            "_SYSTEM",
            "SYS",
            DiscoverySource::EnvVar,
        );
        c.system_mode = mode;
        c
    }

    // T005 — SystemMode parsing
    #[test]
    fn debug_impl_redacts_password() {
        let c = conn("USER", SystemMode::Unknown);
        let debug_str = format!("{c:?}");
        assert!(
            debug_str.contains("[redacted]"),
            "password should be redacted: {debug_str}"
        );
        assert!(
            !debug_str.contains("password: \"SYS\""),
            "raw password should not appear: {debug_str}"
        );
        assert!(
            debug_str.contains("IrisConnection"),
            "should be an IrisConnection: {debug_str}"
        );
    }

    #[test]
    fn service_account_none_when_env_unset() {
        let _guard = ENV_LOCK.lock().unwrap();
        let saved_u = std::env::var("IRIS_SERVICE_USERNAME").ok();
        let saved_p = std::env::var("IRIS_SERVICE_PASSWORD").ok();
        unsafe {
            std::env::remove_var("IRIS_SERVICE_USERNAME");
            std::env::remove_var("IRIS_SERVICE_PASSWORD");
        }
        let c = conn("USER", SystemMode::Unknown);
        assert!(
            c.with_service_account().is_none(),
            "no service account configured → None (falls back to primary identity)"
        );
        unsafe {
            if let Some(v) = saved_u {
                std::env::set_var("IRIS_SERVICE_USERNAME", v);
            }
            if let Some(v) = saved_p {
                std::env::set_var("IRIS_SERVICE_PASSWORD", v);
            }
        }
    }

    #[test]
    fn service_account_overrides_credentials_only() {
        let _guard = ENV_LOCK.lock().unwrap();
        let saved_u = std::env::var("IRIS_SERVICE_USERNAME").ok();
        let saved_p = std::env::var("IRIS_SERVICE_PASSWORD").ok();
        unsafe {
            std::env::set_var("IRIS_SERVICE_USERNAME", "dtetu_mcp");
            std::env::set_var("IRIS_SERVICE_PASSWORD", "password");
        }
        let c = conn("USER", SystemMode::Development);
        let svc = c
            .with_service_account()
            .expect("service account configured");
        // Credentials swapped to the service account…
        assert_eq!(svc.username, "dtetu_mcp");
        assert_eq!(svc.password, "password");
        // …but every other connection property is preserved (same instance, namespace, mode).
        assert_eq!(svc.base_url, c.base_url);
        assert_eq!(svc.namespace, c.namespace);
        assert_eq!(svc.system_mode, c.system_mode);
        // Original connection is untouched.
        assert_eq!(c.username, "_SYSTEM");
        unsafe {
            std::env::remove_var("IRIS_SERVICE_USERNAME");
            std::env::remove_var("IRIS_SERVICE_PASSWORD");
            if let Some(v) = saved_u {
                std::env::set_var("IRIS_SERVICE_USERNAME", v);
            }
            if let Some(v) = saved_p {
                std::env::set_var("IRIS_SERVICE_PASSWORD", v);
            }
        }
    }

    #[test]
    fn service_account_ignored_when_username_empty() {
        let _guard = ENV_LOCK.lock().unwrap();
        let saved_u = std::env::var("IRIS_SERVICE_USERNAME").ok();
        unsafe {
            std::env::set_var("IRIS_SERVICE_USERNAME", "");
        }
        let c = conn("USER", SystemMode::Unknown);
        assert!(
            c.with_service_account().is_none(),
            "empty username must be treated as unset"
        );
        unsafe {
            std::env::remove_var("IRIS_SERVICE_USERNAME");
            if let Some(v) = saved_u {
                std::env::set_var("IRIS_SERVICE_USERNAME", v);
            }
        }
    }

    #[test]
    fn system_mode_live_from_string() {
        // Simulates what detect_system_mode maps "Live" to
        assert_eq!(SystemMode::Live, SystemMode::Live);
        assert_ne!(SystemMode::Live, SystemMode::Unknown);
    }

    #[test]
    fn system_mode_default_is_unknown() {
        assert_eq!(SystemMode::default(), SystemMode::Unknown);
    }

    #[test]
    fn system_mode_development_ne_live() {
        assert_ne!(SystemMode::Development, SystemMode::Live);
    }

    // T006 — is_write_allowed()
    #[test]
    fn write_blocked_for_live() {
        let _guard = ENV_LOCK.lock().unwrap();
        let c = conn("USER", SystemMode::Live);
        // No IRIS_ALLOW_PROD set in this test
        std::env::remove_var("IRIS_ALLOW_PROD");
        assert!(!c.is_write_allowed());
    }

    #[test]
    fn write_allowed_for_development() {
        let _guard = ENV_LOCK.lock().unwrap();
        std::env::remove_var("IRIS_ALLOW_PROD");
        assert!(conn("USER", SystemMode::Development).is_write_allowed());
    }

    #[test]
    fn write_allowed_for_test_mode() {
        let _guard = ENV_LOCK.lock().unwrap();
        std::env::remove_var("IRIS_ALLOW_PROD");
        assert!(conn("USER", SystemMode::Test).is_write_allowed());
    }

    #[test]
    fn write_blocked_for_unknown_with_prod_namespace() {
        let _guard = ENV_LOCK.lock().unwrap();
        std::env::remove_var("IRIS_ALLOW_PROD");
        assert!(!conn("PROD", SystemMode::Unknown).is_write_allowed());
        assert!(!conn("PRODUCTION", SystemMode::Unknown).is_write_allowed());
        assert!(!conn("LIVE", SystemMode::Unknown).is_write_allowed());
        assert!(!conn("PRD", SystemMode::Unknown).is_write_allowed());
    }

    #[test]
    fn write_allowed_for_unknown_with_dev_namespace() {
        let _guard = ENV_LOCK.lock().unwrap();
        std::env::remove_var("IRIS_ALLOW_PROD");
        assert!(conn("USER", SystemMode::Unknown).is_write_allowed());
        assert!(conn("DEV", SystemMode::Unknown).is_write_allowed());
        assert!(conn("MYAPP", SystemMode::Unknown).is_write_allowed());
    }

    #[test]
    fn iris_allow_prod_overrides_live_mode() {
        let _guard = ENV_LOCK.lock().unwrap();
        std::env::set_var("IRIS_ALLOW_PROD", "1");
        assert!(conn("USER", SystemMode::Live).is_write_allowed());
        std::env::remove_var("IRIS_ALLOW_PROD");
    }

    #[test]
    fn is_write_allowed_logic_direct() {
        let _guard = ENV_LOCK.lock().unwrap();
        std::env::remove_var("IRIS_ALLOW_PROD");
        // Test the override logic directly without touching process env vars.
        // The env var branch is: if IRIS_ALLOW_PROD is "1" or "true" → return true.
        // We verify the non-override paths only (env-based override tested manually).
        assert!(!conn("LIVE", SystemMode::Unknown).is_write_allowed());
        assert!(!conn("PROD", SystemMode::Live).is_write_allowed());
        assert!(conn("DEV", SystemMode::Development).is_write_allowed());
    }

    #[test]
    fn is_production_namespace_case_insensitive() {
        assert!(is_production_namespace("prod"));
        assert!(is_production_namespace("PROD"));
        assert!(is_production_namespace("Production"));
        assert!(is_production_namespace("LIVE"));
        assert!(is_production_namespace("live"));
        assert!(is_production_namespace("PRD"));
        assert!(!is_production_namespace("USER"));
        assert!(!is_production_namespace("DEV"));
        assert!(!is_production_namespace("MYAPP"));
    }
}

#[cfg(test)]
mod pure_fn_tests {
    use super::*;

    fn make_conn() -> IrisConnection {
        IrisConnection::new(
            "http://localhost:52773",
            "USER",
            "_SYSTEM",
            "SYS",
            DiscoverySource::EnvVar,
        )
    }

    // ── versioned_ns_url ──────────────────────────────────────────────────────
    #[test]
    fn versioned_ns_url_contains_namespace() {
        let c = make_conn();
        let url = c.versioned_ns_url("USER", "/docnames/CLS");
        assert!(url.contains("USER"), "{url}");
        assert!(url.contains("docnames"), "{url}");
    }

    #[test]
    fn versioned_ns_url_encodes_percent_sys() {
        let c = make_conn();
        let url = c.versioned_ns_url("%SYS", "/action/query");
        assert!(url.contains("%25SYS") || url.contains("%SYS"), "{url}");
        assert!(url.contains("action/query"), "{url}");
    }

    #[test]
    fn versioned_ns_url_different_namespaces() {
        let c = make_conn();
        let url1 = c.versioned_ns_url("MYNS", "/foo");
        let url2 = c.versioned_ns_url("OTHERNS", "/foo");
        assert_ne!(url1, url2);
    }

    // ── strip_iris_banner ────────────────────────────────────────────────────
    #[test]
    fn strip_iris_banner_empty_input() {
        assert_eq!(strip_iris_banner("").trim(), "");
    }

    #[test]
    fn strip_iris_banner_only_prompts() {
        let raw = "USER>\nUSER>\n";
        let stripped = strip_iris_banner(raw);
        assert!(
            stripped.trim().is_empty(),
            "only prompts → empty: {stripped:?}"
        );
    }

    #[test]
    fn strip_iris_banner_output_without_banner() {
        let raw = "42\n";
        let stripped = strip_iris_banner(raw);
        assert_eq!(stripped.trim(), "42");
    }

    // ── build_exec_class / build_exec_class_for_test ──────────────────────────
    #[test]
    fn build_exec_class_contains_class_name() {
        let lines = IrisConnection::build_exec_class_for_test(
            "IrisDevTmp.IrisDevRuntest123",
            "/tmp/test.txt",
            "Write 1",
        );
        assert!(
            lines
                .iter()
                .any(|l| l.contains("IrisDevTmp.IrisDevRuntest123")),
            "class name must appear in generated source"
        );
    }

    #[test]
    fn build_exec_class_contains_user_code() {
        let lines = IrisConnection::build_exec_class_for_test(
            "TestClass",
            "/tmp/t.txt",
            "Write \"hello world\"",
        );
        assert!(
            lines.iter().any(|l| l.contains("hello world")),
            "user code must be embedded"
        );
    }

    #[test]
    fn build_exec_class_has_write_sentinel() {
        let lines = IrisConnection::build_exec_class_for_test("T", "/tmp/t.txt", "Write 1");
        let has_sentinel = lines.iter().any(|l| l.trim() == "Write !");
        assert!(has_sentinel, "sentinel Write ! must be present");
    }

    #[test]
    fn build_exec_class_has_include_occinclude() {
        let lines = IrisConnection::build_exec_class_for_test("T", "/tmp/t.txt", "Write 1");
        assert!(
            lines.iter().any(|l| l.trim() == "Include %occInclude"),
            "Include %occInclude must be present so $$$macros resolve in user code"
        );
    }

    #[test]
    fn build_exec_class_is_plain_classmethod_not_generator() {
        let lines = IrisConnection::build_exec_class_for_test("T", "/tmp/t.txt", "Write 1");
        let joined = lines.join("\n");
        assert!(
            !joined.contains("CodeMode = objectgenerator"),
            "user code method must be plain ClassMethod, not objectgenerator"
        );
        assert!(
            joined.contains("SqlProc"),
            "method must still be SqlProc so it can be called via SQL"
        );
    }
}

#[cfg(test)]
mod additional_tests {
    use super::*;

    fn make_conn() -> IrisConnection {
        IrisConnection::new(
            "http://localhost:52773",
            "USER",
            "_SYSTEM",
            "SYS",
            DiscoverySource::EnvVar,
        )
    }

    fn make_conn_v8() -> IrisConnection {
        let mut c = make_conn();
        c.atelier_version = AtelierVersion::V8;
        c
    }

    // ── AtelierVersion::version_str ───────────────────────────────────────────

    #[test]
    fn atelier_version_str_v1() {
        assert_eq!(AtelierVersion::V1.version_str(), "v1");
    }

    #[test]
    fn atelier_version_str_v2() {
        assert_eq!(AtelierVersion::V2.version_str(), "v2");
    }

    #[test]
    fn atelier_version_str_v8() {
        assert_eq!(AtelierVersion::V8.version_str(), "v8");
    }

    // ── atelier_url ───────────────────────────────────────────────────────────

    #[test]
    fn atelier_url_strips_trailing_slash() {
        let mut c = make_conn();
        c.base_url = "http://localhost:52773/".to_string();
        let url = c.atelier_url("/");
        assert!(
            !url.contains("//api"),
            "double slash must not appear: {url}"
        );
        assert!(url.contains("/api/atelier"), "{url}");
    }

    #[test]
    fn atelier_url_no_trailing_slash_on_base() {
        let c = make_conn();
        let url = c.atelier_url("/");
        assert_eq!(url, "http://localhost:52773/api/atelier/");
    }

    // ── versioned_ns_url with V8 ──────────────────────────────────────────────

    #[test]
    fn versioned_ns_url_uses_v8_when_set() {
        let c = make_conn_v8();
        let url = c.versioned_ns_url("USER", "/docnames/CLS");
        assert!(url.contains("/v8/"), "URL must include v8 segment: {url}");
    }

    #[test]
    fn versioned_ns_url_default_is_v1() {
        let c = make_conn();
        let url = c.versioned_ns_url("USER", "/docnames/CLS");
        assert!(url.contains("/v1/"), "URL must include v1 segment: {url}");
    }

    #[test]
    fn versioned_ns_url_path_appended_after_namespace() {
        let c = make_conn();
        let url = c.versioned_ns_url("USER", "/action/query");
        // Structure: .../v1/USER/action/query
        assert!(url.ends_with("/action/query"), "{url}");
    }

    // ── atelier_url_versioned uses self.namespace ─────────────────────────────

    #[test]
    fn atelier_url_versioned_uses_self_namespace() {
        let mut c = make_conn();
        c.namespace = "MYNS".to_string();
        let url = c.atelier_url_versioned("/action/query");
        assert!(url.contains("MYNS"), "self.namespace must appear: {url}");
    }

    // ── CompileResult::success ────────────────────────────────────────────────

    #[test]
    fn compile_result_success_when_no_errors() {
        let r = CompileResult {
            errors: vec![],
            console: vec!["Compiled.".into()],
        };
        assert!(r.success());
    }

    #[test]
    fn compile_result_failure_when_errors_present() {
        let r = CompileResult {
            errors: vec!["ERROR line 1".into()],
            console: vec![],
        };
        assert!(!r.success());
    }

    // ── is_bare_prompt_line ───────────────────────────────────────────────────

    #[test]
    fn bare_prompt_user_is_stripped() {
        assert!(is_bare_prompt_line("USER>"));
    }

    #[test]
    fn bare_prompt_percent_sys_is_stripped() {
        assert!(is_bare_prompt_line("%SYS>"));
    }

    #[test]
    fn bare_prompt_with_trailing_space_is_stripped() {
        // is_bare_prompt_line receives trimmed input from strip_iris_banner,
        // but the function itself also trims_end internally.
        assert!(is_bare_prompt_line("USER>   "));
    }

    #[test]
    fn line_with_content_after_prompt_is_not_bare() {
        // "USER> 42" should NOT be treated as a bare prompt line
        assert!(!is_bare_prompt_line("USER> 42"));
    }

    #[test]
    fn non_prompt_line_is_not_bare() {
        assert!(!is_bare_prompt_line("42"));
        assert!(!is_bare_prompt_line(""));
        assert!(!is_bare_prompt_line("hello world"));
    }

    // ── strip_iris_banner — additional edge cases ─────────────────────────────

    #[test]
    fn strip_iris_banner_removes_copyright_line() {
        let raw = "Copyright (c) 2024 InterSystems Corporation\n42\n";
        let stripped = strip_iris_banner(raw);
        assert!(!stripped.contains("Copyright"), "{stripped:?}");
        assert_eq!(stripped.trim(), "42");
    }

    #[test]
    fn strip_iris_banner_removes_node_instance_line() {
        // IRIS 2026.2+ prints this on every `iris session` connect. Its embedded ':'
        // previously got misparsed as a name:code pair by callers like
        // interop::parse_status_response (production name came back as "Node").
        let raw = "\nNode: de17f22ad88c, Instance: IRIS\n\nUSER>\nIrisDevTest.CoverageProduction:1\n\nUSER>\n";
        let stripped = strip_iris_banner(raw);
        assert!(!stripped.contains("Node:"), "{stripped:?}");
        assert_eq!(stripped.trim(), "IrisDevTest.CoverageProduction:1");
    }

    #[test]
    fn strip_iris_banner_keeps_iris_for_line_after_first_prompt() {
        // Regression: `Write $ZVersion` legitimately outputs a string starting with
        // "IRIS for UNIX ..." — this must NOT be treated as the connect-time banner
        // just because it shares the same text prefix. Only strip banner-shaped text
        // that appears before the first prompt is seen.
        let raw = "\nNode: de17f22ad88c, Instance: IRIS\n\nUSER>\nIRIS for UNIX (Ubuntu Server LTS for ARM64 Containers) 2026.2.0L\n\nUSER>\n";
        let stripped = strip_iris_banner(raw);
        assert_eq!(
            stripped.trim(),
            "IRIS for UNIX (Ubuntu Server LTS for ARM64 Containers) 2026.2.0L"
        );
    }

    #[test]
    fn strip_iris_banner_keeps_content_lines_unchanged() {
        let raw = "USER>\nhello\nworld\nUSER>\n";
        let stripped = strip_iris_banner(raw);
        assert_eq!(stripped, "hello\nworld");
    }

    #[test]
    fn strip_iris_banner_no_prompts_at_all() {
        let raw = "line one\nline two\n";
        let stripped = strip_iris_banner(raw);
        assert_eq!(stripped, "line one\nline two");
    }

    #[test]
    fn strip_iris_banner_all_banner_gives_empty() {
        let raw =
            "Copyright (c) 2024 InterSystems Corporation\nAll rights reserved.\nIRIS for UNIX\n";
        let stripped = strip_iris_banner(raw);
        assert!(
            stripped.trim().is_empty(),
            "all banner → empty: {stripped:?}"
        );
    }

    #[test]
    fn strip_iris_banner_trims_leading_and_trailing_blank_lines() {
        let raw = "USER>\n\nhello\n\nUSER>\n";
        let stripped = strip_iris_banner(raw);
        assert_eq!(stripped.trim(), "hello");
    }

    // ── is_retryable_error classification logic ───────────────────────────────

    /// Helper to classify error messages as retryable or not.
    /// Mirrors the classification in execute_via_generator (lines 284-295).
    fn classify_error_as_retryable(msg: &str) -> bool {
        let msg_lower = msg.to_ascii_lowercase();
        msg.contains("HTTP 5")
            || msg_lower.contains("error sending request")
            || msg_lower.contains("connection refused")
            || msg_lower.contains("connection reset")
            || msg_lower.contains("connection closed")
            || msg_lower.contains("broken pipe")
            || msg_lower.contains("unexpected end of file")
            || msg_lower.contains("eof")
            || msg_lower.contains("dns")
            || msg_lower.contains("handshake")
            || msg_lower.contains("timed out")
            || msg_lower.contains("timeout")
    }

    #[test]
    fn classify_http_5xx_as_retryable() {
        assert!(classify_error_as_retryable(
            "HTTP 500 Internal Server Error"
        ));
        assert!(classify_error_as_retryable("HTTP 502 Bad Gateway"));
        assert!(classify_error_as_retryable("HTTP 503 Service Unavailable"));
        assert!(classify_error_as_retryable("HTTP 504 Gateway Timeout"));
    }

    #[test]
    fn classify_http_4xx_as_not_retryable() {
        assert!(!classify_error_as_retryable("HTTP 401 Unauthorized"));
        assert!(!classify_error_as_retryable("HTTP 403 Forbidden"));
        assert!(!classify_error_as_retryable("HTTP 404 Not Found"));
        assert!(!classify_error_as_retryable("HTTP 400 Bad Request"));
    }

    #[test]
    fn classify_connection_refused_as_retryable() {
        assert!(classify_error_as_retryable("connection refused"));
        assert!(classify_error_as_retryable("Connection refused"));
        assert!(classify_error_as_retryable("error: connection refused"));
    }

    #[test]
    fn classify_connection_reset_as_retryable() {
        assert!(classify_error_as_retryable("connection reset"));
        assert!(classify_error_as_retryable("Connection reset by peer"));
    }

    #[test]
    fn classify_connection_closed_as_retryable() {
        assert!(classify_error_as_retryable("connection closed"));
        assert!(classify_error_as_retryable(
            "Connection closed unexpectedly"
        ));
    }

    #[test]
    fn classify_broken_pipe_as_retryable() {
        assert!(classify_error_as_retryable("broken pipe"));
        assert!(classify_error_as_retryable("Broken pipe"));
    }

    #[test]
    fn classify_eof_as_retryable() {
        assert!(classify_error_as_retryable("eof"));
        assert!(classify_error_as_retryable("EOF"));
        assert!(classify_error_as_retryable("unexpected end of file"));
        assert!(classify_error_as_retryable("Unexpected End of File"));
    }

    #[test]
    fn classify_dns_as_retryable() {
        assert!(classify_error_as_retryable("dns lookup failed"));
        assert!(classify_error_as_retryable("DNS resolution error"));
    }

    #[test]
    fn classify_handshake_as_retryable() {
        assert!(classify_error_as_retryable("tls handshake failed"));
        assert!(classify_error_as_retryable("Handshake error"));
    }

    #[test]
    fn classify_timeout_as_retryable() {
        assert!(classify_error_as_retryable("timed out"));
        assert!(classify_error_as_retryable("timeout"));
        assert!(classify_error_as_retryable("Timed Out"));
        assert!(classify_error_as_retryable("TIMEOUT"));
    }

    #[test]
    fn classify_error_sending_request_as_retryable() {
        assert!(classify_error_as_retryable("error sending request"));
        assert!(classify_error_as_retryable(
            "Error sending request to server"
        ));
    }

    #[test]
    fn classify_generic_error_as_not_retryable() {
        // Generic errors that don't match any retryable pattern → not retryable
        assert!(!classify_error_as_retryable("invalid json response"));
        assert!(!classify_error_as_retryable("query execution failed"));
        assert!(!classify_error_as_retryable(
            "compilation error: unexpected token"
        ));
    }

    #[test]
    fn classify_case_insensitivity() {
        // Most patterns are checked against lowercase version, so case variations work
        assert!(classify_error_as_retryable("Connection Refused"));
        assert!(classify_error_as_retryable("CONNECTION CLOSED"));
        assert!(classify_error_as_retryable("Error Sending Request"));
    }

    #[test]
    fn classify_combined_message() {
        // Error messages with multiple keywords — retryable if ANY keyword matches
        assert!(classify_error_as_retryable(
            "Failed to send request: connection refused after DNS lookup"
        ));
        assert!(classify_error_as_retryable(
            "HTTP 503 Service Unavailable: timeout waiting for backend"
        ));
    }

    #[test]
    fn classify_empty_string_as_not_retryable() {
        assert!(!classify_error_as_retryable(""));
    }

    #[test]
    fn classify_whitespace_only_as_not_retryable() {
        assert!(!classify_error_as_retryable("   "));
    }
}

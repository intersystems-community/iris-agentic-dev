//! Append-only JSONL audit log for policy-gated tool calls (044-servermanager-discovery).
//!
//! One entry per tool call on any connection that has an active `[policy.<server-name>]` block.
//! Write failures are non-blocking — a warning is emitted and the tool call proceeds normally.
//! PHI-named globals and credential fields are scrubbed before writing (FR-010, 051-phi-policy-env-gates).
//! Payload text (`code`, `query`, …) is reduced to its size unless the operator opts in (141, FR-028).

use std::path::PathBuf;

/// Key-name endings that mark a value as a credential. Compared with case, `_` and `-` removed,
/// so `Password`, `db_password`, `api-key` and `accessToken` all match, and `max_tokens` does not.
const CREDENTIAL_SUFFIXES: &[&str] = &[
    "password",
    "passwd",
    "passphrase",
    "secret",
    "token",
    "apikey",
];

/// Keys whose values are what the caller wrote: code, a query, a value, a file body. They hold the
/// secrets no key-name rule finds, so the log keeps their size and drops the text (FR-028).
const PAYLOAD_KEYS: &[&str] = &["code", "query", "sql", "args", "content", "value"];

fn is_credential_key(key: &str) -> bool {
    let normal: String = key
        .chars()
        .filter(|c| *c != '_' && *c != '-')
        .flat_map(char::to_lowercase)
        .collect();
    CREDENTIAL_SUFFIXES.iter().any(|s| normal.ends_with(s))
}

/// `{"redacted": true, "chars": N, "lines": L}` for a payload value. A non-string is measured as
/// the JSON it serialises to.
fn payload_size(val: &serde_json::Value) -> serde_json::Value {
    let text = match val {
        serde_json::Value::String(s) => s.clone(),
        other => other.to_string(),
    };
    serde_json::json!({
        "redacted": true,
        "chars": text.chars().count(),
        "lines": text.lines().count(),
    })
}

/// [`scrub_params_with`] in the default mode: payloads reduced to their size.
pub fn scrub_params(params: serde_json::Value) -> serde_json::Value {
    scrub_params_with(params, false)
}

/// Scrub a params JSON value before audit logging, at any depth.
///
/// - A key that names a credential (see [`CREDENTIAL_SUFFIXES`]): value replaced with `"[REDACTED]"`.
/// - A payload key (`code`, `query`, `sql`, `args`, `content`, `value`): value replaced with its
///   size, unless `keep_payloads` (the operator's `audit_payloads` opt-in).
/// - `global_name`: replaced with `"[REDACTED-PHI]"` if it matches a PHI name pattern.
/// - Everything else: unchanged.
pub fn scrub_params_with(params: serde_json::Value, keep_payloads: bool) -> serde_json::Value {
    use serde_json::Value;
    match params {
        Value::Object(obj) => {
            let mut out = serde_json::Map::with_capacity(obj.len());
            for (key, val) in obj {
                let scrubbed = if is_credential_key(&key) {
                    Value::String("[REDACTED]".to_string())
                } else if PAYLOAD_KEYS.contains(&key.as_str()) && !keep_payloads {
                    payload_size(&val)
                } else if key == "global_name" || key == "globalName" || key == "global" {
                    match val.as_str() {
                        Some(name)
                            if crate::policy::patterns::matches_any(
                                name.strip_prefix('^').unwrap_or(name),
                                crate::policy::patterns::PHI_NAME_PATTERNS,
                            ) =>
                        {
                            Value::String("[REDACTED-PHI]".to_string())
                        }
                        _ => val,
                    }
                } else {
                    scrub_params_with(val, keep_payloads)
                };
                out.insert(key, scrubbed);
            }
            Value::Object(out)
        }
        Value::Array(items) => Value::Array(
            items
                .into_iter()
                .map(|v| scrub_params_with(v, keep_payloads))
                .collect(),
        ),
        other => other,
    }
}

/// Are full payloads kept? The operator file's `audit_payloads`, or `IAD_AUDIT_PAYLOADS` in the
/// launch environment (`env` is that variable's value). A workspace file cannot set either: its
/// `audit_payloads` is set aside before it reaches `operator`.
pub fn payloads_enabled(
    operator: &crate::iris::config_sources::OperatorSettings,
    env: Option<&str>,
) -> bool {
    operator.audit_payloads
        || env.is_some_and(|v| {
            matches!(
                v.trim().to_ascii_lowercase().as_str(),
                "1" | "true" | "yes" | "on"
            )
        })
}

/// What `check_config` reports about the log: where it is and whether it keeps payload text.
pub fn mode_report(keep_payloads: bool, path: &std::path::Path) -> serde_json::Value {
    serde_json::json!({
        "path": path.to_string_lossy(),
        "payloads": if keep_payloads { "full" } else { "sizes" },
    })
}

/// A single audit log entry. Serialized as one JSONL line.
#[derive(Debug, serde::Serialize)]
pub struct AuditLogEntry {
    /// RFC3339 timestamp.
    pub ts: String,
    /// Tool name, e.g. `"iris_compile"`.
    pub tool: String,
    /// Server Manager server name (or empty string if unknown).
    pub connection: String,
    /// IRIS namespace for this call.
    pub namespace: String,
    /// `"allowed"`, `"blocked"`, or `"error"`.
    pub status: String,
    /// `"policy"` or `"role"` — present only when `status == "blocked"`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub gate: Option<String>,
    /// Present only when blocked by policy — lists the categories that ARE allowed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub allowed_categories: Option<Vec<String>>,
    /// Full input parameters as a JSON object.
    pub params: serde_json::Value,
}

/// Append-only JSONL audit log writer.
pub struct AuditLog {
    path: PathBuf,
    keep_payloads: bool,
}

impl AuditLog {
    /// Create an `AuditLog` writing to `path`, payloads reduced to their size.
    pub fn new(path: PathBuf) -> Self {
        Self {
            path,
            keep_payloads: false,
        }
    }

    /// Keep `code`, `query`, `args`, `content` and `value` in full (the operator-only opt-in).
    /// Credential keys are still redacted.
    pub fn with_payloads(mut self, keep: bool) -> Self {
        self.keep_payloads = keep;
        self
    }

    /// Default audit log path: `~/.iris-agentic-dev/audit.jsonl`.
    pub fn default_path() -> Option<PathBuf> {
        dirs::home_dir().map(|h| h.join(".iris-agentic-dev").join("audit.jsonl"))
    }

    /// Returns `true` if audit logging should fire for the given policy.
    /// Only connections with an explicit `[policy.<server-name>]` block produce entries.
    pub fn should_write(policy: Option<&crate::iris::workspace_config::ConnectionPolicy>) -> bool {
        policy.is_some()
    }

    /// Append `entry` to the JSONL log file.
    ///
    /// Creates the parent directory and file if needed.
    /// Write failures are non-blocking — emits a tracing warning and returns `Ok(())`.
    pub fn write(&self, entry: &AuditLogEntry) -> std::io::Result<()> {
        if let Err(e) = self.write_inner(entry) {
            tracing::warn!("audit log write failed (non-blocking): {e}");
        }
        Ok(())
    }

    fn write_inner(&self, entry: &AuditLogEntry) -> std::io::Result<()> {
        // Build a scrubbed version for serialization — PHI, credentials and payloads dealt with.
        let scrubbed_entry = AuditLogEntry {
            ts: entry.ts.clone(),
            tool: entry.tool.clone(),
            connection: entry.connection.clone(),
            namespace: entry.namespace.clone(),
            status: entry.status.clone(),
            gate: entry.gate.clone(),
            allowed_categories: entry.allowed_categories.clone(),
            params: scrub_params_with(entry.params.clone(), self.keep_payloads),
        };

        let line = serde_json::to_string(&scrubbed_entry).map_err(std::io::Error::other)?;

        // One O_APPEND write per entry. On Linux, concurrent appends from two MCP clients can
        // still interleave between entries; ordering is not guaranteed, every line stays whole.
        // The file is `0600` in a `0700` directory (FR-028).
        crate::iris::private_file::append_private(&self.path, &line)
    }
}

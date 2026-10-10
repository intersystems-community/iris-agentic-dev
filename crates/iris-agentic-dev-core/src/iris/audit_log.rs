//! Append-only JSONL audit log for policy-gated tool calls (044-servermanager-discovery).
//!
//! One entry per tool call on any connection that has an active `[policy.<server-name>]` block.
//! Write failures are non-blocking — a warning is emitted and the tool call proceeds normally.
//! PHI-named globals and credential fields are scrubbed before writing (FR-010, 051-phi-policy-env-gates).

use std::path::PathBuf;

/// Credential/sensitive field names redacted with `[REDACTED]`.
const CREDENTIAL_FIELDS: &[&str] = &[
    "password",
    "token",
    "api_key",
    "secret",
    "access_token",
    "auth_token",
];

/// Scrub PHI global names and credential fields from a params JSON object before audit logging.
///
/// - `global_name` field: replaced with `"[REDACTED-PHI]"` if it matches a PHI name pattern.
/// - Credential fields: replaced with `"[REDACTED]"`.
/// - All other fields: unchanged.
pub fn scrub_params(params: serde_json::Value) -> serde_json::Value {
    let obj = match params.as_object() {
        Some(o) => o,
        None => return params,
    };

    let mut out = serde_json::Map::with_capacity(obj.len());
    for (key, val) in obj {
        let scrubbed = if CREDENTIAL_FIELDS.contains(&key.as_str()) {
            serde_json::Value::String("[REDACTED]".to_string())
        } else if key == "global_name" || key == "globalName" || key == "global" {
            if let Some(name) = val.as_str() {
                let normalized = name.strip_prefix('^').unwrap_or(name);
                if crate::policy::patterns::matches_any(
                    normalized,
                    crate::policy::patterns::PHI_NAME_PATTERNS,
                ) {
                    serde_json::Value::String("[REDACTED-PHI]".to_string())
                } else {
                    val.clone()
                }
            } else {
                val.clone()
            }
        } else {
            val.clone()
        };
        out.insert(key.clone(), scrubbed);
    }
    serde_json::Value::Object(out)
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
}

impl AuditLog {
    /// Create an `AuditLog` writing to `path`.
    pub fn new(path: PathBuf) -> Self {
        Self { path }
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
        use std::io::Write;

        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        // Build a scrubbed version for serialization — PHI and credentials replaced inline.
        let scrubbed_params = scrub_params(entry.params.clone());
        let scrubbed_entry = AuditLogEntry {
            ts: entry.ts.clone(),
            tool: entry.tool.clone(),
            connection: entry.connection.clone(),
            namespace: entry.namespace.clone(),
            status: entry.status.clone(),
            gate: entry.gate.clone(),
            allowed_categories: entry.allowed_categories.clone(),
            params: scrubbed_params,
        };

        let line = serde_json::to_string(&scrubbed_entry).map_err(std::io::Error::other)?;

        // O_APPEND is atomic on macOS and Windows for small writes (< PIPE_BUF).
        // On Linux, concurrent open-append-close from two MCP clients (e.g. Claude Desktop
        // + Cursor) can interleave; entries remain valid JSON lines but ordering may be
        // non-deterministic. This is acceptable for an audit log — correctness over ordering.
        let mut file = std::fs::OpenOptions::new()
            .append(true)
            .create(true)
            .open(&self.path)?;

        writeln!(file, "{}", line)?;
        file.flush()?;
        Ok(())
    }
}

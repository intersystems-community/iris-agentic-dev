//! Append-only JSONL audit log for policy-gated tool calls (044-servermanager-discovery).
//!
//! One entry per tool call on any connection that has an active `[policy.<server-name>]` block.
//! Write failures are non-blocking — a warning is emitted and the tool call proceeds normally.

use std::path::PathBuf;

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

        let line = serde_json::to_string(entry)
            .map_err(std::io::Error::other)?;

        let mut file = std::fs::OpenOptions::new()
            .append(true)
            .create(true)
            .open(&self.path)?;

        writeln!(file, "{}", line)?;
        file.flush()?;
        Ok(())
    }
}

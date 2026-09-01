use anyhow::Result;
use clap::Args;


#[derive(Args)]
pub struct CheckSmCredentialCommand {
    /// Server Manager server name (from intersystems.servers in VS Code settings)
    pub server_name: String,
    /// IRIS username stored with this server
    pub username: String,
    /// Override the path to state.vscdb (for testing; omit to use the VS Code default location)
    #[arg(long)]
    pub db_path: Option<std::path::PathBuf>,
}

impl CheckSmCredentialCommand {
    pub async fn run(self) -> Result<()> {
        #[cfg(not(target_os = "windows"))]
        {
            eprintln!(
                "check-sm-credential is a Windows-only diagnostic.\n\
                 On macOS/Linux, iris-agentic-dev reads Server Manager credentials \
                 from the OS keychain directly. If credentials are missing, reconnect \
                 the server in VS Code (right-click → Reconnect)."
            );
            std::process::exit(1);
        }

        #[cfg(target_os = "windows")]
        {
            let username = self.username.to_lowercase();
            match resolve_vscode_secret_verbose(
                &self.server_name,
                &username,
                self.db_path.as_deref(),
            ) {
                Ok(password) => {
                    println!("OK: resolved password for {}/{username}", self.server_name);
                    println!("Password length: {} chars", password.len());
                }
                Err(e) => {
                    eprintln!("FAIL: {e}");
                    std::process::exit(1);
                }
            }
            Ok(())
        }
    }
}

/// Verbose wrapper around core's `resolve_vscode_secret` that emits diagnostic
/// output at each step — useful for support and field diagnosis.
#[cfg(target_os = "windows")]
fn resolve_vscode_secret_verbose(
    server_name: &str,
    username: &str,
    db_path_override: Option<&std::path::Path>,
) -> Result<String, String> {
    use iris_agentic_dev_core::iris::vscode_payload::{
        decode_payload, hex_preview, DecodedPayload,
    };

    let db_path = match db_path_override {
        Some(p) => p.to_path_buf(),
        None => vscdb_default_path()?,
    };
    eprintln!("state.vscdb: {}", db_path.display());

    let account = format!("credentialProvider:{server_name}/{username}");
    let secret_key = format!(
        r#"secret://{{"extensionId":"intersystems-community.servermanager","key":"{account}"}}"#
    );
    eprintln!("Looking up key: {secret_key}");

    let (stored, sqlite_type) = vscdb_read_verbose(&db_path, &secret_key)?;
    eprintln!(
        "Found stored value ({} bytes, sqlite type {sqlite_type})",
        stored.len()
    );
    eprintln!("Value encoding: {:?}", classify_payload(&stored));
    eprintln!("Value prefix:   {}", hex_preview(&stored, 16));

    match decode_payload(&stored)? {
        DecodedPayload::Dpapi(_) => {
            eprintln!("Payload is DPAPI ciphertext — delegating to core unseal");
        }
        DecodedPayload::SafeStorage(ref envelope) => {
            eprintln!(
                "Payload is a safeStorage AES-GCM envelope ({} bytes) — the AES key \
                 lives in Local State, so DPAPI is applied to the key, not to this value",
                envelope.len()
            );
        }
    }

    // Delegate the actual unseal to core — same path as the MCP server uses.
    iris_agentic_dev_core::iris::server_manager::resolve_vscode_secret(
        server_name,
        &account,
        db_path_override,
    )
}

/// Locate state.vscdb for diagnostic output (mirrors core's private helper).
#[cfg(target_os = "windows")]
fn vscdb_default_path() -> Result<std::path::PathBuf, String> {
    let appdata = std::env::var("APPDATA").map_err(|_| "%APPDATA% not set".to_string())?;
    let path = std::path::PathBuf::from(&appdata)
        .join("Code")
        .join("User")
        .join("globalStorage")
        .join("state.vscdb");
    if path.exists() {
        return Ok(path);
    }
    let cursor = std::path::PathBuf::from(&appdata)
        .join("Cursor")
        .join("User")
        .join("globalStorage")
        .join("state.vscdb");
    if cursor.exists() {
        return Ok(cursor);
    }
    Err(format!(
        "state.vscdb not found at {} (also tried Cursor)",
        path.display()
    ))
}

/// Read one key from state.vscdb for diagnostic output.
#[cfg(target_os = "windows")]
fn vscdb_read_verbose(db_path: &std::path::Path, key: &str) -> Result<(Vec<u8>, String), String> {
    use rusqlite::OptionalExtension;
    let conn =
        rusqlite::Connection::open(db_path).map_err(|e| format!("cannot open state.vscdb: {e}"))?;
    let result: Option<(Vec<u8>, String)> = conn
        .query_row(
            "SELECT value, typeof(value) FROM ItemTable WHERE key = ?1",
            rusqlite::params![key],
            |row| {
                use rusqlite::types::ValueRef;
                let bytes = match row.get_ref(0)? {
                    ValueRef::Blob(b) => b.to_vec(),
                    ValueRef::Text(t) => t.to_vec(),
                    other => {
                        return Err(rusqlite::Error::InvalidColumnType(
                            0,
                            "value".to_string(),
                            other.data_type(),
                        ))
                    }
                };
                Ok((bytes, row.get::<_, String>(1)?))
            },
        )
        .optional()
        .map_err(|e| format!("query failed: {e}"))?;
    result.ok_or_else(|| {
        "key not found in state.vscdb — Server Manager credentials may not be stored yet"
            .to_string()
    })
}

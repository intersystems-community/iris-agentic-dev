use anyhow::Result;
use clap::Args;

#[cfg(target_os = "windows")]
use crate::cmd::vscode_payload::{
    classify_payload, decode_payload, decrypt_safe_storage, hex_preview, parse_local_state_key,
    DecodedPayload,
};

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
            match resolve_vscode_secret(&self.server_name, &username, self.db_path.as_deref()) {
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

/// Resolve one Server Manager password out of VS Code's secret storage.
///
/// Public so the `windows_sm_credential` example shares this implementation
/// instead of keeping a second copy. Two copies is how the original format bug
/// survived: the example still carried the pre-fix logic.
#[cfg(target_os = "windows")]
pub fn resolve_vscode_secret(
    server_name: &str,
    username: &str,
    db_path_override: Option<&std::path::Path>,
) -> Result<String, String> {
    let db_path = match db_path_override {
        Some(p) => p.to_path_buf(),
        None => vscode_state_db_path()?,
    };
    eprintln!("state.vscdb: {}", db_path.display());

    let secret_key = format!(
        r#"secret://{{"extensionId":"intersystems-community.servermanager","key":"credentialProvider:{server_name}/{username}"}}"#
    );
    eprintln!("Looking up key: {secret_key}");

    let (stored, sqlite_type) = read_from_db(&db_path, &secret_key)?;
    eprintln!(
        "Found stored value ({} bytes, sqlite type {sqlite_type})",
        stored.len()
    );

    // VS Code does not store raw DPAPI ciphertext. Identify the wrapper before
    // reaching for any key, so a format mismatch reports itself as a format
    // mismatch instead of masquerading as a user-context problem.
    eprintln!("Value encoding: {:?}", classify_payload(&stored));
    eprintln!("Value prefix:   {}", hex_preview(&stored, 16));

    match decode_payload(&stored)? {
        // Legacy path: some older builds sealed the secret with DPAPI directly.
        DecodedPayload::Dpapi(ciphertext) => {
            eprintln!("Payload is DPAPI ciphertext ({} bytes)", ciphertext.len());
            let plaintext = dpapi_decrypt(&ciphertext, "the stored secret")?;
            String::from_utf8(plaintext).map_err(|e| format!("decrypted bytes are not UTF-8: {e}"))
        }
        // Current path: AES-256-GCM, with the AES key DPAPI-sealed in Local State.
        DecodedPayload::SafeStorage(envelope) => {
            eprintln!(
                "Payload is a safeStorage AES-GCM envelope ({} bytes) — the AES key \
                 lives in Local State, so DPAPI is applied to the key, not to this value",
                envelope.len()
            );

            let local_state = local_state_path(&db_path)?;
            eprintln!("Local State: {}", local_state.display());
            let json = std::fs::read_to_string(&local_state)
                .map_err(|e| format!("cannot read {}: {e}", local_state.display()))?;

            let sealed_key = parse_local_state_key(&json)?;
            eprintln!("Sealed AES key: {} bytes", sealed_key.len());

            let aes_key = dpapi_decrypt(&sealed_key, "the Local State AES key")?;
            eprintln!("Unsealed AES key: {} bytes", aes_key.len());

            decrypt_safe_storage(&envelope, &aes_key)
        }
    }
}

/// `Local State` sits two levels above `globalStorage`, beside the `User`
/// directory: `…\Code\Local State` next to `…\Code\User\globalStorage`.
/// Derived from the database path so Cursor and other forks resolve correctly.
#[cfg(target_os = "windows")]
fn local_state_path(db_path: &std::path::Path) -> Result<std::path::PathBuf, String> {
    let install_root = db_path
        .parent() // globalStorage
        .and_then(|p| p.parent()) // User
        .and_then(|p| p.parent()) // Code
        .ok_or_else(|| {
            format!(
                "cannot locate Local State relative to {} — expected \
                 …\\Code\\User\\globalStorage\\state.vscdb",
                db_path.display()
            )
        })?;
    Ok(install_root.join("Local State"))
}

#[cfg(target_os = "windows")]
fn vscode_state_db_path() -> Result<std::path::PathBuf, String> {
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

#[cfg(target_os = "windows")]
fn read_from_db(db_path: &std::path::Path, key: &str) -> Result<(Vec<u8>, String), String> {
    use rusqlite::OptionalExtension;
    let conn =
        rusqlite::Connection::open(db_path).map_err(|e| format!("cannot open state.vscdb: {e}"))?;

    // VS Code stores the value as BLOB on some versions and as TEXT on others.
    // Accept both, and report which one we got — the storage type is the first
    // clue about how the payload is wrapped.
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

#[cfg(target_os = "windows")]
fn dpapi_decrypt(ciphertext: &[u8], what: &str) -> Result<Vec<u8>, String> {
    use windows::Win32::Security::Cryptography::{CryptUnprotectData, CRYPT_INTEGER_BLOB};

    let mut input = CRYPT_INTEGER_BLOB {
        cbData: ciphertext.len() as u32,
        pbData: ciphertext.as_ptr() as *mut u8,
    };
    let mut output = CRYPT_INTEGER_BLOB {
        cbData: 0,
        pbData: std::ptr::null_mut(),
    };

    let ok = unsafe { CryptUnprotectData(&mut input, None, None, None, None, 0, &mut output) };

    if ok.is_err() {
        // Report the OS error and name what failed. Do not assert a single cause:
        // the caller has already printed the payload encoding, which is the
        // evidence that distinguishes a format problem from a user-context one.
        let err = std::io::Error::last_os_error();
        return Err(format!(
            "CryptUnprotectData failed on {what} ({} bytes): {err}. DPAPI is per-user \
             and per-machine, so the usual causes are: this process running as a \
             different Windows user than VS Code, or the profile having been copied \
             from another machine. Please report the full output of this command.",
            ciphertext.len()
        ));
    }

    let decrypted =
        unsafe { std::slice::from_raw_parts(output.pbData, output.cbData as usize).to_vec() };

    unsafe {
        windows::Win32::Foundation::LocalFree(windows::Win32::Foundation::HLOCAL(
            output.pbData as *mut _,
        ));
    }

    Ok(decrypted)
}

use anyhow::Result;
use clap::Args;

#[derive(Args)]
pub struct CheckSmCredentialCommand {
    /// Server Manager server name (from intersystems.servers in VS Code settings)
    pub server_name: String,
    /// IRIS username stored with this server
    pub username: String,
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
            match resolve_vscode_secret(&self.server_name, &username) {
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

#[cfg(target_os = "windows")]
fn resolve_vscode_secret(server_name: &str, username: &str) -> Result<String, String> {
    let db_path = vscode_state_db_path()?;
    eprintln!("state.vscdb: {}", db_path.display());

    let secret_key = format!(
        r#"secret://{{"extensionId":"intersystems-community.servermanager","key":"credentialProvider:{server_name}/{username}"}}"#
    );
    eprintln!("Looking up key: {secret_key}");

    let encrypted = read_from_db(&db_path, &secret_key)?;
    eprintln!("Found encrypted blob ({} bytes)", encrypted.len());

    let decrypted = dpapi_decrypt(&encrypted)?;
    String::from_utf8(decrypted).map_err(|e| format!("decrypted bytes are not UTF-8: {e}"))
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
fn read_from_db(db_path: &std::path::Path, key: &str) -> Result<Vec<u8>, String> {
    let conn =
        rusqlite::Connection::open(db_path).map_err(|e| format!("cannot open state.vscdb: {e}"))?;

    let result: Option<Vec<u8>> = conn
        .query_row(
            "SELECT value FROM ItemTable WHERE key = ?1",
            rusqlite::params![key],
            |row| row.get(0),
        )
        .optional()
        .map_err(|e| format!("query failed: {e}"))?;

    result.ok_or_else(|| {
        "key not found in state.vscdb — Server Manager credentials may not be stored yet"
            .to_string()
    })
}

#[cfg(target_os = "windows")]
fn dpapi_decrypt(ciphertext: &[u8]) -> Result<Vec<u8>, String> {
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
        return Err(
            "CryptUnprotectData failed — likely running as a different user than VS Code"
                .to_string(),
        );
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

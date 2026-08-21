//! Prototype: read Server Manager credentials from VS Code's state.vscdb on Windows.
//!
//! VS Code's SecretStorage on Windows stores secrets as DPAPI-encrypted blobs in
//! %APPDATA%\Code\User\globalStorage\state.vscdb under keys of the form:
//!   secret://{"extensionId":"intersystems-community.servermanager","key":"credentialProvider:SERVER/USER"}
//!
//! CryptUnprotectData can decrypt them because DPAPI is per-user and the MCP server
//! runs as the same user as VS Code.
//!
//! Usage (Windows only):
//!   cargo run --example windows_sm_credential -- iservice-base _SYSTEM
//!
//! On success prints the password to stdout. On failure explains why.

fn main() {
    #[cfg(not(target_os = "windows"))]
    {
        eprintln!("This example only runs on Windows.");
        eprintln!(
            "On macOS/Linux, iris-agentic-dev reads credentials from the OS keychain directly."
        );
        std::process::exit(1);
    }

    #[cfg(target_os = "windows")]
    windows_main();
}

#[cfg(target_os = "windows")]
fn windows_main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 3 {
        eprintln!("Usage: windows_sm_credential <server-name> <username>");
        eprintln!("Example: windows_sm_credential iservice-base _SYSTEM");
        std::process::exit(1);
    }
    let server_name = &args[1];
    let username = args[2].to_lowercase();

    match resolve_vscode_secret(server_name, &username) {
        Ok(password) => {
            println!("OK: resolved password for {server_name}/{username}");
            println!("Password length: {} chars", password.len());
            // Don't print the actual password — this is a diagnostic tool.
            // Uncomment the next line only for local testing:
            // println!("Password: {password}");
        }
        Err(e) => {
            eprintln!("FAIL: {e}");
            std::process::exit(1);
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
    // Try %APPDATA%\Code\User\globalStorage\state.vscdb
    let appdata = std::env::var("APPDATA").map_err(|_| "%APPDATA% not set".to_string())?;
    let path = std::path::PathBuf::from(appdata)
        .join("Code")
        .join("User")
        .join("globalStorage")
        .join("state.vscdb");
    if path.exists() {
        return Ok(path);
    }
    // Also try Cursor
    let appdata = std::env::var("APPDATA").unwrap();
    let cursor = std::path::PathBuf::from(appdata)
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
    use rusqlite::OptionalExtension;
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
        format!("key not found in state.vscdb — Server Manager credentials may not be stored yet")
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

    let ok = unsafe {
        CryptUnprotectData(
            &mut input,
            None, // no description
            None, // no optional entropy
            None, // reserved
            None, // no prompt struct
            0,    // flags
            &mut output,
        )
    };

    if ok.is_err() {
        return Err(format!(
            "CryptUnprotectData failed — likely running as a different user than VS Code"
        ));
    }

    let decrypted =
        unsafe { std::slice::from_raw_parts(output.pbData, output.cbData as usize).to_vec() };

    // Free the output buffer allocated by Windows
    unsafe {
        windows::Win32::Foundation::LocalFree(windows::Win32::Foundation::HLOCAL(
            output.pbData as *mut _,
        ));
    }

    Ok(decrypted)
}

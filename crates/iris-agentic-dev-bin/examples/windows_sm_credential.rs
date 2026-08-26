//! Read Server Manager credentials from VS Code's `state.vscdb` on Windows.
//!
//! Secrets live in `%APPDATA%\Code\User\globalStorage\state.vscdb` under keys of
//! the form:
//!   secret://{"extensionId":"intersystems-community.servermanager","key":"credentialProvider:SERVER/USER"}
//!
//! The stored value is *not* a DPAPI blob. VS Code writes
//! `JSON.stringify(safeStorage.encryptString(pw))`, so the column holds TEXT like
//! `{"type":"Buffer","data":[…]}`, and the bytes inside are a Chromium OSCrypt
//! envelope: `v10` + 12-byte nonce + AES-256-GCM ciphertext + 16-byte tag. DPAPI
//! protects the *AES key*, which sits in `%APPDATA%\Code\Local State` under
//! `os_crypt.encrypted_key`.
//!
//! So reading a credential is a two-stage unseal: `CryptUnprotectData` on the
//! Local State key, then AES-GCM on the value. Calling `CryptUnprotectData` on
//! the value itself fails for every user on every machine — an earlier revision
//! did that and blamed the user's Windows account for the failure.
//!
//! This example delegates to the same code path as the `check-sm-credential`
//! subcommand rather than reimplementing it; a second copy is what let the old
//! behaviour linger here after the command was fixed.
//!
//! Usage (Windows only):
//!   cargo run --example windows_sm_credential -- iservice-base _SYSTEM

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

    match iris_agentic_dev::cmd::check_sm_credential::resolve_vscode_secret(
        server_name,
        &username,
        None,
    ) {
        Ok(password) => {
            println!("OK: resolved password for {server_name}/{username}");
            // Length only — this is a diagnostic, so the secret stays out of the log.
            println!("Password length: {} chars", password.len());
        }
        Err(e) => {
            eprintln!("FAIL: {e}");
            std::process::exit(1);
        }
    }
}

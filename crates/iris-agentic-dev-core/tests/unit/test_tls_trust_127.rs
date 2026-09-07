//! #127: trust the OS certificate store, and resolve the two TLS env vars in one place.
//!
//! Two defects sit behind that issue. The first is the reqwest feature set: `rustls-tls` bundles
//! webpki roots and never reads the OS trust store, so a gateway serving a cert from a
//! locally-installed CA (mkcert and friends) fails the handshake even though curl and the browser
//! on the same machine accept it. `tokio-tungstenite` in the same manifest already asks for native
//! roots, so the websocket client trusted the cert the Atelier client refused.
//!
//! The second is that `IRIS_INSECURE` and `IRIS_TLS_VERIFY` were resolved by four hand-copied
//! blocks, and two of the copies read only `IRIS_INSECURE`. A user who set `IRIS_TLS_VERIFY=false`
//! got working single `iris_doc` gets and failing batch gets, and failing websockets — the same
//! setting honoured or ignored depending on which code path served the request.

use iris_agentic_dev_core::iris::connection::tls_insecure;

// ── The Cargo feature, which is the actual #127 fix ───────────────────────────
//
// Nothing else in the suite can assert this. Proving native roots behaviourally needs a server
// holding a cert from a CA that is in this machine's trust store and nowhere else, which means
// installing a CA — not something a test may do. So the manifest is the assertion, and this test
// exists to stop a future dependency bump from quietly dropping back to bundled roots.

fn workspace_manifest() -> String {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../Cargo.toml");
    std::fs::read_to_string(path).expect("workspace Cargo.toml must be readable")
}

fn reqwest_line(manifest: &str) -> String {
    manifest
        .lines()
        .find(|l| l.trim_start().starts_with("reqwest = "))
        .expect("workspace Cargo.toml must declare reqwest")
        .to_string()
}

#[test]
fn reqwest_asks_for_native_roots() {
    let line = reqwest_line(&workspace_manifest());
    assert!(
        line.contains("rustls-tls-native-roots"),
        "reqwest must trust the OS certificate store (#127), got: {line}"
    );
}

#[test]
fn reqwest_does_not_also_ask_for_bundled_only_roots() {
    // `rustls-tls` alone is the pre-#127 state. Both features together still work, but leaving
    // the old one in place is how the intent gets lost on the next edit.
    let line = reqwest_line(&workspace_manifest());
    let bundled_only = line.contains("\"rustls-tls\"");
    assert!(
        !bundled_only,
        "the bare `rustls-tls` feature is the bundled-roots build superseded by \
         `rustls-tls-native-roots`; drop it: {line}"
    );
}

#[test]
fn the_websocket_client_still_asks_for_native_roots_too() {
    // The pre-existing half of the asymmetry. If a bump drops this, the two clients disagree
    // about which certs are valid again, just the other way round.
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/Cargo.toml");
    let manifest = std::fs::read_to_string(path).expect("core Cargo.toml must be readable");
    let line = manifest
        .lines()
        .find(|l| l.trim_start().starts_with("tokio-tungstenite = "))
        .expect("core Cargo.toml must declare tokio-tungstenite");
    assert!(
        line.contains("rustls-tls-native-roots"),
        "tokio-tungstenite must keep native roots, got: {line}"
    );
}

// ── One resolver, not four ────────────────────────────────────────────────────

#[test]
fn only_one_source_file_reads_the_tls_env_vars() {
    let src = concat!(env!("CARGO_MANIFEST_DIR"), "/src");
    let mut offenders: Vec<String> = vec![];
    let mut stack = vec![std::path::PathBuf::from(src)];
    while let Some(dir) = stack.pop() {
        for entry in std::fs::read_dir(&dir).expect("src must be readable") {
            let path = entry.expect("dir entry").path();
            if path.is_dir() {
                stack.push(path);
                continue;
            }
            if path.extension().and_then(|e| e.to_str()) != Some("rs") {
                continue;
            }
            let text = std::fs::read_to_string(&path).unwrap_or_default();
            // The read form only. `env::set_var`/`remove_var` on the same names is how
            // `workspace_config.rs` publishes the config's declaration, which is the fix, not the
            // defect. (`env::set_var` does not contain the substring `env::var`.)
            if !text.contains("env::var(\"IRIS_INSECURE\")")
                && !text.contains("env::var(\"IRIS_TLS_VERIFY\")")
            {
                continue;
            }
            let rel = path
                .strip_prefix(concat!(env!("CARGO_MANIFEST_DIR"), "/src"))
                .unwrap_or(&path)
                .to_string_lossy()
                .to_string();
            // `iris/connection.rs` owns the resolver; `testing.rs` only lists the names as env
            // vars a test harness must strip.
            if rel == "iris/connection.rs" || rel == "testing.rs" {
                continue;
            }
            offenders.push(rel);
        }
    }
    assert!(
        offenders.is_empty(),
        "these files resolve the TLS env vars themselves instead of calling \
         `tls_insecure_from_env()`, which is how `IRIS_TLS_VERIFY=false` came to be honoured on \
         some code paths and ignored on others: {offenders:?}"
    );
}

// ── The resolver's truth table ────────────────────────────────────────────────
//
// `tls_insecure` takes the two values as arguments rather than reading the environment, so these
// stay pure: no `set_var`, no lock, and the `unit` target keeps running parallel.

#[test]
fn neither_variable_set_means_validate() {
    assert!(!tls_insecure(None, None));
}

#[test]
fn iris_insecure_turns_validation_off() {
    assert!(tls_insecure(Some("true"), None));
    assert!(tls_insecure(Some("1"), None));
}

#[test]
fn iris_tls_verify_false_turns_validation_off() {
    assert!(tls_insecure(None, Some("false")));
    assert!(tls_insecure(None, Some("0")));
}

#[test]
fn iris_tls_verify_true_keeps_validation_on() {
    assert!(!tls_insecure(None, Some("true")));
    assert!(!tls_insecure(None, Some("1")));
}

#[test]
fn iris_insecure_false_falls_through_to_iris_tls_verify() {
    // `IRIS_INSECURE=false` is not an instruction to validate, it is the absence of one. The
    // second variable still gets to speak — otherwise a shell that exports `IRIS_INSECURE=false`
    // for tidiness would silently override a `tls_verify = false` config.
    assert!(tls_insecure(Some("false"), Some("false")));
    assert!(!tls_insecure(Some("false"), Some("true")));
}

#[test]
fn iris_insecure_wins_when_both_ask_for_the_opposite() {
    // Documented precedence: the blunt override is checked first. Pinned so a refactor cannot
    // reverse it without a failing test.
    assert!(tls_insecure(Some("true"), Some("true")));
}

#[test]
fn an_unrecognised_value_is_not_a_licence_to_skip_validation() {
    // `IRIS_INSECURE=yes` is a typo, not consent. Failing closed here means a handshake error,
    // which is legible; failing open means silently unvalidated TLS, which is not.
    assert!(!tls_insecure(Some("yes"), None));
    assert!(!tls_insecure(Some(""), None));
    assert!(!tls_insecure(None, Some("no")));
}

//! One HTTP client builder, and no handler borrows another server's client (E4).
//!
//! Whether a server's certificate is checked and which CSP session a request rides are properties
//! of the server and the account, decided in `iris/http.rs`. Two ways around that have recurred,
//! each in a place nobody had listed:
//!
//! - a file builds its own `reqwest` client, so it decides certificate checks for itself;
//! - a handler that addresses a server by name uses the client built at startup for the default
//!   server, so a name that says "this server is self-signed" is sent over a client that
//!   validates (or the reverse), and its requests ride the default account's session.
//!
//! This file scans the source for both. It is a scan of text, like `test_endpoint_tls_141`: what it
//! finds is a call site, and a call site that is right gets a line in an allowlist with its reason.
//! The allowlists may only shrink. A new entry is a new way around the builder, and a reviewer
//! should see it here.

use iris_agentic_dev_core::tools::write_gate::{TargetSource, CLASSIFICATION};
use std::path::{Path, PathBuf};

/// Every `.rs` file under both crates' `src`, relative to `crates/`, with everything from the first
/// `#[cfg(test)]` on dropped, and comment lines blanked: a test may build any client it likes.
fn sources() -> Vec<(String, String)> {
    let crates = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
    let mut out = Vec::new();
    let mut stack: Vec<PathBuf> = ["iris-agentic-dev-core", "iris-agentic-dev-bin"]
        .iter()
        .map(|c| crates.join(c).join("src"))
        .collect();
    while let Some(dir) = stack.pop() {
        for entry in std::fs::read_dir(&dir)
            .expect("src must be readable")
            .flatten()
        {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else if path.extension().is_some_and(|e| e == "rs") {
                let rel = path
                    .strip_prefix(&crates)
                    .unwrap()
                    .to_string_lossy()
                    .replace('\\', "/");
                let text = std::fs::read_to_string(&path).unwrap();
                out.push((rel, production_code(&text)));
            }
        }
    }
    assert!(out.len() > 50, "only {} source files found", out.len());
    out
}

fn production_code(text: &str) -> String {
    let end = text.find("#[cfg(test)]").unwrap_or(text.len());
    text[..end]
        .lines()
        .map(|l| {
            if l.trim_start().starts_with("//") {
                ""
            } else {
                l
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

// ── (a) one builder ─────────────────────────────────────────────────────────────────────────────

/// What builds a client, or switches certificate checks, outside the one module.
const BUILDERS: &[&str] = &[
    "Client::new()",
    "Client::builder()",
    "ClientBuilder::new",
    "danger_accept_invalid_certs",
    "reqwest::blocking",
    "reqwest::get(",
    "IrisConnection::http_client()",
];

/// The one module that builds clients for IRIS servers.
const THE_BUILDER: &[&str] = &["iris-agentic-dev-core/src/iris/http.rs"];

/// Files that build a client of their own, each for a host that is not an IRIS server and carries
/// no IRIS credential. This list may only shrink: a file leaves it when it takes
/// `iris::http::external_client`, and a name is added only for a host that is demonstrably not one
/// of ours.
const OWN_CLIENT: &[(&str, &str)] = &[(
    "iris-agentic-dev-core/src/generate.rs",
    "calls the model provider's API with the operator's API key",
)];

fn builder_hits() -> Vec<(String, &'static str)> {
    let mut hits = Vec::new();
    for (rel, text) in sources() {
        for needle in BUILDERS {
            if text.contains(needle) {
                hits.push((rel.clone(), *needle));
            }
        }
    }
    hits
}

#[test]
fn only_the_builder_and_the_listed_files_build_a_client() {
    let mut stray = Vec::new();
    for (rel, needle) in builder_hits() {
        if THE_BUILDER.contains(&rel.as_str()) {
            continue;
        }
        if OWN_CLIENT.iter().any(|(f, _)| *f == rel) {
            continue;
        }
        stray.push(format!("{rel}: {needle}"));
    }
    assert!(
        stray.is_empty(),
        "these files build an HTTP client outside iris/http.rs:\n  {}\n\
         A client for an IRIS server comes from `iris::http` (`client_for`, `call_client_ref`) so \
         the server's own certificate setting and account session decide what it does. A host \
         that is not an IRIS server takes `iris::http::external_client`. If a file truly needs its \
         own, add it to OWN_CLIENT with the host it talks to.",
        stray.join("\n  ")
    );
}

#[test]
fn the_own_client_list_only_shrinks() {
    let hits = builder_hits();
    for (file, reason) in OWN_CLIENT {
        assert!(reason.len() >= 20, "{file} is listed without a reason");
        assert!(
            hits.iter().any(|(rel, _)| rel == file),
            "{file} is in OWN_CLIENT and builds no client now: remove the entry"
        );
    }
    assert!(
        OWN_CLIENT.len() <= 4,
        "OWN_CLIENT grew to {}: every entry is a client that decides TLS for itself",
        OWN_CLIENT.len()
    );
}

// ── (b) the startup client belongs to the default server ────────────────────────────────────────

/// Where the startup client may be read: `(file, enclosing fn, why)`. `self.client` is the client
/// built once, for the default connection. A handler that reaches a server by name takes the
/// client for that server's connection (`conn_client`), and one that acts on the call's target
/// takes `http_client()`, which reads the target in scope.
const STARTUP_CLIENT_USES: &[(&str, &str, &str)] = &[
    (
        "iris-agentic-dev-core/src/tools/mod.rs",
        "http_client",
        "the fallback for code outside any call, where there is no target to read",
    ),
    (
        "iris-agentic-dev-core/src/tools/mod.rs",
        "record",
        "the fallback when the sink's own client cannot be built, in which case nothing is written",
    ),
    (
        "iris-agentic-dev-core/src/tools/mod.rs",
        "iris_admin",
        "the fallback when the call has no connection at all, in which case the action refuses",
    ),
    (
        "iris-agentic-dev-core/src/tools/exec.rs",
        "generate",
        "the Atelier leg of an execution context, which carries the client it was built with",
    ),
];

/// The name of the last `fn` declared at or before `offset`.
fn enclosing_fn(text: &str, offset: usize) -> String {
    let head = &text[..offset];
    let at = head.rfind("fn ").map(|i| i + 3).unwrap_or(0);
    head[at..]
        .chars()
        .take_while(|c| c.is_alphanumeric() || *c == '_')
        .collect()
}

fn startup_client_uses() -> Vec<(String, String)> {
    let mut uses = Vec::new();
    for (rel, text) in sources() {
        if !rel.contains("/src/tools/") {
            continue;
        }
        let mut from = 0;
        while let Some(i) = text[from..].find("self.client") {
            let at = from + i;
            from = at + "self.client".len();
            let next = text[from..].chars().next().unwrap_or(' ');
            if next.is_alphanumeric() || next == '_' {
                continue; // self.client_for, self.clients
            }
            uses.push((rel.clone(), enclosing_fn(&text, at)));
        }
    }
    uses
}

#[test]
fn the_startup_client_is_read_only_where_listed() {
    let mut stray = Vec::new();
    for (rel, func) in startup_client_uses() {
        let listed = STARTUP_CLIENT_USES
            .iter()
            .any(|(f, g, _)| *f == rel && *g == func);
        if !listed {
            stray.push(format!("{rel}: fn {func} reads self.client"));
        }
    }
    assert!(
        stray.is_empty(),
        "these tool code paths use the client built at startup:\n  {}\n\
         It is the default server's client: its certificate setting, its account's CSP session. \
         A handler takes `self.http_client()` for the target in scope, or \
         `self.conn_client(&conn)` for a connection it holds. If this one is right, list it in \
         STARTUP_CLIENT_USES with the reason.",
        stray.join("\n  ")
    );
}

#[test]
fn the_startup_client_list_only_shrinks() {
    let uses = startup_client_uses();
    for (file, func, reason) in STARTUP_CLIENT_USES {
        assert!(reason.len() >= 20, "{func} is listed without a reason");
        assert!(
            uses.iter().any(|(f, g)| f == file && g == func),
            "{file} fn {func} is in STARTUP_CLIENT_USES and no longer reads self.client: remove \
             the entry"
        );
    }
    assert!(
        STARTUP_CLIENT_USES.len() <= 4,
        "STARTUP_CLIENT_USES grew to {}",
        STARTUP_CLIENT_USES.len()
    );
}

#[test]
fn only_the_constructors_build_the_startup_client() {
    let mut calls = Vec::new();
    for (rel, text) in sources() {
        let mut from = 0;
        while let Some(i) = text[from..].find("startup_client(") {
            let at = from + i;
            from = at + 1;
            let func = enclosing_fn(&text, at + "startup_client(".len());
            calls.push((rel.clone(), func));
        }
    }
    let allowed = [
        ("iris-agentic-dev-core/src/tools/mod.rs", "new"),
        (
            "iris-agentic-dev-core/src/tools/mod.rs",
            "with_registry_and_toolset",
        ),
        (
            "iris-agentic-dev-core/src/iris/connection.rs",
            "startup_client",
        ),
    ];
    for (rel, func) in &calls {
        assert!(
            allowed.iter().any(|(f, g)| f == rel && g == func),
            "{rel}: fn {func} builds the startup client. It is built once per IrisTools, for the \
             default server; a per-call client comes from `conn_client`."
        );
    }
    assert!(calls.len() >= 3, "the scan found {calls:?}");
}

/// A tool whose row names a server in an argument of its own (`compare_*`, `iris_test_server`)
/// acts on a server that is not the call's target in scope. `http_client()` would read the wrong
/// one; the handler has to build the client from the connection it resolved.
#[test]
fn a_handler_that_names_its_servers_builds_their_clients() {
    let mods = sources()
        .into_iter()
        .find(|(rel, _)| rel.ends_with("src/tools/mod.rs"))
        .expect("tools/mod.rs")
        .1;
    let mut checked = 0;
    for row in CLASSIFICATION {
        if !matches!(row.target, TargetSource::Args(_) | TargetSource::Name(_)) {
            continue;
        }
        let start = mods
            .find(&format!("async fn {}(", row.tool))
            .unwrap_or_else(|| panic!("no handler `async fn {}` in tools/mod.rs", row.tool));
        let rest = &mods[start + 10..];
        let end = rest
            .find("\n    #[tool(")
            .or_else(|| rest.find("\n    async fn "))
            .or_else(|| rest.find("\n    fn "))
            .unwrap_or(rest.len());
        let body = &rest[..end];
        assert!(
            body.contains("conn_client(") || body.contains("probe_server("),
            "{} addresses servers by name and builds no client from their connections",
            row.tool
        );
        assert!(
            !body.contains("self.client") && !body.contains("http_client()"),
            "{} addresses servers by name and reads the startup or in-scope client",
            row.tool
        );
        checked += 1;
    }
    assert!(checked >= 3, "only {checked} multi-server handlers found");
}

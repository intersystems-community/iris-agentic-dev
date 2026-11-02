//! Every store an HTTP session could share is declared (E6).
//!
//! The HTTP transport builds a session's tools from a prototype, and a plain `clone` shares every
//! `Arc` field. `IrisTools::for_new_session` resets the ones that hold a client's results, tokens or
//! terminals, and `SESSION_FIELDS` says, for every field of the struct, which it is: `PerSession`
//! (reset there) or `Shared` with the reason it is safe. The fix that gave sessions their own
//! `ws_pool`, `confirm_tokens` and `checkout_cache` came after the first one that gave them a log
//! store; each time, the next field nobody had listed was the gap.
//!
//! This reads the struct definition and the constructor off the source and compares them with the
//! table, then checks the live `Arc`s of the public fields, so neither the scan nor the table can
//! drift from what `for_new_session` really does.

use iris_agentic_dev_core::tools::{IrisTools, SessionScope, SESSION_FIELDS};
use regex::Regex;
use std::collections::BTreeSet;
use std::path::Path;
use std::sync::Arc;

fn tools_source() -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/tools/mod.rs");
    std::fs::read_to_string(path).unwrap()
}

/// The field names of `pub struct IrisTools`, at the struct's own indent.
fn struct_fields(src: &str) -> Vec<String> {
    let head = "pub struct IrisTools {";
    let start = src.find(head).expect("IrisTools struct") + head.len();
    let body = &src[start..];
    let body = &body[..body.find("\n}\n").expect("struct end")];
    let field = Regex::new(r"^    (?:pub(?:\([a-z]+\))? )?([a-z_][a-z0-9_]*)\s*:").unwrap();
    body.lines()
        .filter_map(|l| field.captures(l).map(|c| c[1].to_string()))
        .collect()
}

/// The fields `for_new_session_sweeping` assigns on the copy it returns.
fn reset_fields(src: &str) -> BTreeSet<String> {
    let head = "pub fn for_new_session_sweeping(";
    let start = src.find(head).expect("for_new_session_sweeping") + head.len();
    let body = &src[start..];
    let body = &body[..body.find("\n    }\n").expect("fn end")];
    let assign = Regex::new(r"\bsession\.([a-z_][a-z0-9_]*)\s*=[^=]").unwrap();
    assign
        .captures_iter(body)
        .map(|c| c[1].to_string())
        .collect()
}

#[test]
fn every_field_of_the_session_struct_is_declared() {
    let fields = struct_fields(&tools_source());
    assert!(fields.len() >= 15, "the struct scan found {fields:?}");
    let declared: BTreeSet<&str> = SESSION_FIELDS.iter().map(|(n, _)| *n).collect();
    assert_eq!(
        declared.len(),
        SESSION_FIELDS.len(),
        "a field is declared twice"
    );
    let named: BTreeSet<&str> = fields.iter().map(String::as_str).collect();
    let missing: Vec<_> = named.difference(&declared).collect();
    assert!(
        missing.is_empty(),
        "these fields of IrisTools are not in SESSION_FIELDS: {missing:?}. In HTTP mode a clone \
         shares an Arc field between sessions. Declare it PerSession and reset it in \
         `for_new_session_sweeping`, or Shared with the reason it holds nothing of a client's."
    );
    let stale: Vec<_> = declared.difference(&named).collect();
    assert!(
        stale.is_empty(),
        "SESSION_FIELDS names fields IrisTools does not have: {stale:?}"
    );
}

#[test]
fn the_constructor_resets_the_per_session_fields_and_only_those() {
    let reset = reset_fields(&tools_source());
    assert!(reset.len() >= 5, "the constructor scan found {reset:?}");
    let per_session: BTreeSet<String> = SESSION_FIELDS
        .iter()
        .filter(|(_, s)| *s == SessionScope::PerSession)
        .map(|(n, _)| n.to_string())
        .collect();
    let not_reset: Vec<_> = per_session.difference(&reset).collect();
    assert!(
        not_reset.is_empty(),
        "these fields are declared PerSession and `for_new_session_sweeping` does not reset them: \
         {not_reset:?}"
    );
    let reset_but_shared: Vec<_> = reset.difference(&per_session).collect();
    assert!(
        reset_but_shared.is_empty(),
        "`for_new_session_sweeping` resets these fields and SESSION_FIELDS calls them Shared: \
         {reset_but_shared:?}"
    );
}

#[test]
fn every_shared_field_says_why() {
    for (name, scope) in SESSION_FIELDS {
        if let SessionScope::Shared(reason) = scope {
            assert!(
                reason.len() >= 20,
                "{name} is Shared and its reason is {reason:?}"
            );
        }
    }
}

/// The public fields, as the live `Arc`s the constructor leaves: the table and the scan could both
/// be wrong the same way, and this is what a second session would actually see.
#[tokio::test]
async fn the_live_arcs_agree_with_the_table() {
    let proto = IrisTools::new(None).expect("IrisTools::new");
    let session = proto.for_new_session();
    let scope = |name: &str| {
        SESSION_FIELDS
            .iter()
            .find(|(n, _)| *n == name)
            .unwrap_or_else(|| panic!("{name} is not declared"))
            .1
    };
    let check = |name: &str, shared: bool| {
        let per_session = scope(name) == SessionScope::PerSession;
        assert_eq!(
            shared,
            !per_session,
            "{name} is {} by for_new_session and declared {:?}",
            if shared { "shared" } else { "reset" },
            scope(name)
        );
    };
    check(
        "connection",
        Arc::ptr_eq(&proto.connection, &session.connection),
    );
    check(
        "config_watcher",
        Arc::ptr_eq(&proto.config_watcher, &session.config_watcher),
    );
    check("registry", Arc::ptr_eq(&proto.registry, &session.registry));
    check("client", Arc::ptr_eq(&proto.client, &session.client));
    check("history", Arc::ptr_eq(&proto.history, &session.history));
    check(
        "elicitation_store",
        Arc::ptr_eq(&proto.elicitation_store, &session.elicitation_store),
    );
    check(
        "checkout_cache",
        Arc::ptr_eq(&proto.checkout_cache, &session.checkout_cache),
    );
    check(
        "log_store",
        Arc::ptr_eq(&proto.log_store, &session.log_store),
    );
    check(
        "metadata_cache",
        Arc::ptr_eq(&proto.metadata_cache, &session.metadata_cache),
    );
    check("pool", Arc::ptr_eq(&proto.pool, &session.pool));
    check("ws_pool", Arc::ptr_eq(&proto.ws_pool, &session.ws_pool));
    check(
        "confirm_tokens",
        Arc::ptr_eq(&proto.confirm_tokens, &session.confirm_tokens),
    );
}

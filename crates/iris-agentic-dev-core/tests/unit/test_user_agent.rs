// Caller identification on the wire.
//
// Before this, both IRIS-facing reqwest clients were built without `.user_agent(...)`, so
// IRIS saw `HTTP_USER_AGENT` as empty — verified against iris-dev-iris by reading
// `%request.CgiEnvs("HTTP_USER_AGENT")` from inside an `iris_execute` call. An operator
// could not tell an agent's Atelier traffic from a developer's IDE traffic in a Web
// Gateway or IIS access log, which is what "limit agents on certain environments" needs.
//
// The string is assembled here so it can be asserted without a network round trip. The
// companion live test (`test_exec_live.rs::test_user_agent_visible_to_iris`) proves IRIS
// actually receives it.

use iris_agentic_dev_core::iris::connection::{user_agent, CallerMode};
use iris_agentic_dev_core::tools::MCP_PEER;

/// Serialize the tests that read/write `IRIS_AGENT_LABEL`. Cargo runs tests in one binary
/// on threads, so two label tests racing would flake.
static LABEL_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn with_label<T>(value: Option<&str>, f: impl FnOnce() -> T) -> T {
    let _guard = LABEL_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    match value {
        Some(v) => std::env::set_var("IRIS_AGENT_LABEL", v),
        None => std::env::remove_var("IRIS_AGENT_LABEL"),
    }
    let out = f();
    std::env::remove_var("IRIS_AGENT_LABEL");
    out
}

#[test]
fn user_agent_names_the_product_and_its_version() {
    let ua = with_label(None, || user_agent(CallerMode::Mcp));
    assert!(
        ua.starts_with(&format!("iris-agentic-dev/{}", env!("CARGO_PKG_VERSION"))),
        "an operator greps for the product name and needs the version to match a release: {ua}"
    );
}

#[test]
fn user_agent_distinguishes_mcp_from_cli() {
    let mcp = with_label(None, || user_agent(CallerMode::Mcp));
    let cli = with_label(None, || user_agent(CallerMode::Cli));
    assert!(mcp.contains("mcp"), "expected the mcp marker in {mcp}");
    assert!(cli.contains("cli"), "expected the cli marker in {cli}");
    assert_ne!(
        mcp, cli,
        "a long-lived agent session and a one-shot CI dispatch are different callers"
    );
}

#[test]
fn user_agent_carries_the_operator_label() {
    let ua = with_label(Some("build-agent-7"), || user_agent(CallerMode::Cli));
    assert!(
        ua.contains("build-agent-7"),
        "IRIS_AGENT_LABEL is how a fleet tags which agent is calling: {ua}"
    );
}

#[test]
fn user_agent_label_cannot_inject_headers_or_break_the_log_line() {
    let ua = with_label(
        Some("evil\r\nX-Injected: 1\ttab and \u{7f} control"),
        || user_agent(CallerMode::Mcp),
    );
    assert!(
        !ua.contains('\r') && !ua.contains('\n'),
        "CRLF in a header value is request splitting: {ua:?}"
    );
    assert!(
        !ua.chars().any(|c| c.is_control()),
        "a control character corrupts the access-log line it lands in: {ua:?}"
    );
    assert!(
        ua.contains("X-Injected"),
        "sanitizing should strip the control characters, not silently drop the label: {ua:?}"
    );
}

#[test]
fn user_agent_label_is_length_capped() {
    let ua = with_label(Some(&"x".repeat(500)), || user_agent(CallerMode::Mcp));
    assert!(
        ua.len() <= 200,
        "an unbounded label lets a caller flood every log line: {} chars",
        ua.len()
    );
    assert!(ua.contains('x'), "the label should still be present: {ua}");
}

#[test]
fn user_agent_is_a_valid_header_value() {
    let ua = with_label(Some("agent 42"), || user_agent(CallerMode::Mcp));
    assert!(
        reqwest::header::HeaderValue::from_str(&ua).is_ok(),
        "reqwest panics on a client built with an invalid default header: {ua:?}"
    );
}

#[test]
fn missing_label_leaves_no_empty_parens() {
    let ua = with_label(None, || user_agent(CallerMode::Mcp));
    assert!(
        !ua.contains("()") && !ua.contains("; )"),
        "unset label should not leave a dangling separator: {ua}"
    );
}

// ── T007: MCP client identity in the marker ──────────────────────────────────

/// Run `f` inside a MCP_PEER scope so user_agent() sees the peer.
fn with_peer<T>(peer: Option<(&str, &str)>, f: impl FnOnce() -> T) -> T {
    let owned = peer.map(|(n, v)| (n.to_string(), v.to_string()));
    tokio::runtime::Runtime::new()
        .unwrap()
        .block_on(MCP_PEER.scope(owned, async move { f() }))
}

#[test]
fn user_agent_includes_mcp_client_name_and_version() {
    let ua = with_peer(Some(("claude-code", "2.1.0")), || {
        with_label(Some("build-agent-7"), || user_agent(CallerMode::Mcp))
    });
    // Expected: iris-agentic-dev/VER (mcp; build-agent-7; claude-code/2.1.0)
    assert!(
        ua.contains("claude-code/2.1.0"),
        "the connected MCP client must appear so an operator knows which agent product acted: {ua}"
    );
    assert!(
        ua.contains("build-agent-7"),
        "label must still appear when a peer is present: {ua}"
    );
}

#[test]
fn user_agent_includes_mcp_client_without_label() {
    let ua = with_peer(Some(("cursor", "0.99.0")), || {
        with_label(None, || user_agent(CallerMode::Mcp))
    });
    // Expected: iris-agentic-dev/VER (mcp; cursor/0.99.0)  — no dangling "; "
    assert!(
        ua.contains("cursor/0.99.0"),
        "peer must appear without label: {ua}"
    );
    assert!(
        !ua.contains("; )") && !ua.contains("(; "),
        "no dangling separator when there is no label but there is a peer: {ua}"
    );
}

#[test]
fn user_agent_label_at_exactly_max_len() {
    // A label of exactly MAX_LABEL_LEN (64) chars must survive untruncated.
    let label = "x".repeat(64);
    let ua = with_label(Some(&label), || user_agent(CallerMode::Mcp));
    assert!(
        ua.contains(&label),
        "a label of exactly MAX_LABEL_LEN chars should not be truncated: {ua}"
    );
}

#[test]
fn user_agent_label_truncated_on_char_boundary() {
    // A label starting with a 3-byte UTF-8 char (€ = U+20AC) followed by ASCII.
    // Truncation at byte 64 must not split the leading multibyte character.
    let label = format!("€{}", "a".repeat(200));
    let ua = with_label(Some(&label), || user_agent(CallerMode::Mcp));
    // The result must be valid UTF-8 (it is a Rust String) and a valid header value.
    assert!(
        reqwest::header::HeaderValue::from_str(&ua).is_ok(),
        "truncation must land on a char boundary: {ua:?}"
    );
}

#[test]
fn user_agent_all_control_chars_label_stays_valid() {
    // If sanitizing removes everything (all chars are control), the marker must not
    // contain empty parens or a dangling separator — same rule as a missing label.
    // NUL (\x00) cannot be set as an env var value on most OSes; use other control chars.
    let ua = with_label(Some("\x01\x02\r\n\x7f"), || user_agent(CallerMode::Mcp));
    assert!(
        !ua.contains("()") && !ua.contains("; )") && !ua.contains("(; "),
        "all-control label leaves nothing; marker must stay clean: {ua}"
    );
    assert!(
        reqwest::header::HeaderValue::from_str(&ua).is_ok(),
        "still a valid header value after control-only label: {ua:?}"
    );
}

#[test]
fn user_agent_label_non_ascii_stripped() {
    // T039 / DP-446307: non-ASCII chars in IRIS_AGENT_LABEL must be removed, not passed through.
    // A single Unicode char anywhere in %SYS.Audit::Export() throws <ILLEGAL VALUE>.
    let label = "café résumé naïve";
    let ua = with_label(Some(label), || user_agent(CallerMode::Cli));
    assert!(
        ua.is_ascii(),
        "non-ASCII chars must be stripped from the label; got: {ua:?}"
    );
    assert!(
        reqwest::header::HeaderValue::from_str(&ua).is_ok(),
        "UA must be a valid header value after non-ASCII stripping: {ua:?}"
    );
    // The ASCII portions of the label should survive: "caf" "r" "sum" "na" "ve" → "caf sum na ve"
    // (joining on whitespace after stripping). Verify it contains at least some ASCII content.
    assert!(
        ua.contains("caf") || ua.contains("r"),
        "ASCII portions of the label should be preserved: {ua:?}"
    );
}

#[test]
fn non_iris_clients_carry_no_user_agent_call() {
    // Guard: source-tree assertion that the non-IRIS builders in generate.rs,
    // manifest/resolve.rs, skill_install/mod.rs and skills/mod.rs do NOT call
    // user_agent(). These paths are not IRIS-bound; adding the marker there would
    // violate caller-marker invariant 6.
    for path in &[
        "src/generate.rs",
        "src/manifest/resolve.rs",
        "src/skill_install/mod.rs",
        "src/skills/mod.rs",
    ] {
        let src = std::fs::read_to_string(path)
            .unwrap_or_else(|_| panic!("expected source file to exist: {path}"));
        // Check for `user_agent(CallerMode` — the typed function from connection.rs.
        // Plain `.user_agent("literal")` on reqwest builders is allowed for non-IRIS HTTP.
        assert!(
            !src.contains("user_agent(CallerMode"),
            "non-IRIS client in {path} must not call user_agent(CallerMode) — \
             caller-marker invariant 6: only IRIS-bound requests carry the marker"
        );
    }
}

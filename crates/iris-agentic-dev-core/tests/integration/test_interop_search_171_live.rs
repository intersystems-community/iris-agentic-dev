//! S15 against `iris-dev-iris`: `iris_interop_query search_table` returns the indexed body field,
//! so it meets the bulk-data gate like `body_class` does.
//!
//! Run, serialised behind the shared lock:
//!
//! ```text
//! lockf -k /tmp/iad-141-iris.lock cargo test --features testing --test integration \
//!     interop_search_171 -- --ignored --test-threads=1
//! ```

use iris_agentic_dev_core::testing::{
    data_reads_allowed_env, live_env, require_iad_binary, McpSession,
};
use serde_json::{json, Value};

fn call(mcp: &mut McpSession, args: Value) -> Value {
    let answer = mcp.call("iris_interop_query", &args);
    let text = answer
        .pointer("/result/content/0/text")
        .and_then(Value::as_str)
        .unwrap_or_else(|| panic!("no text content in {answer}"));
    serde_json::from_str(text).unwrap_or_else(|e| panic!("not JSON ({e}): {text}"))
}

/// A search over a Search Table that IRIS registers on every instance, so the property exists and
/// the search is legitimate. Nothing in it needs data: an empty result is a successful search.
fn search() -> Value {
    json!({
        "what": "messages",
        "search_table": {
            "prop": "RootElement",
            "value": "IadNoSuchRoot",
            "extent": "EnsLib.XML.SearchTable"
        },
        "limit": 5
    })
}

#[test]
#[ignore = "requires live IRIS (iris-dev-iris) and the built binary"]
fn a_search_table_search_is_refused_unless_data_reads_are_allowed() {
    let Some(_bin) = require_iad_binary() else {
        return;
    };
    // `block` is the default, and `redact` cannot redact a Search Table row.
    for policy in [None, Some("redact")] {
        let mut env = live_env();
        let _dir;
        if let Some(p) = policy {
            let home = tempfile::tempdir().unwrap();
            std::fs::write(
                home.path().join(".iris-agentic-dev.toml"),
                format!("[policy.default]\ndataPolicy = \"{p}\"\n"),
            )
            .unwrap();
            env.push(("HOME".into(), home.path().to_string_lossy().into_owned()));
            _dir = Some(home);
        }
        let mut mcp = McpSession::start(&env);
        let r = call(&mut mcp, search());
        assert_eq!(r["error_code"], "DATA_POLICY_BLOCKED", "{policy:?}: {r}");
        assert_eq!(r["refusal"]["kind"], "operator_limit", "{policy:?}: {r}");
        // The same search with `what` set to logs is no way round it.
        let mut logs = search();
        logs["what"] = json!("logs");
        let r = call(&mut mcp, logs);
        assert_eq!(r["error_code"], "DATA_POLICY_BLOCKED", "{policy:?}: {r}");
        // A header read on the same session is still allowed.
        let r = call(&mut mcp, json!({"what": "messages", "limit": 1}));
        assert_ne!(r["error_code"], "DATA_POLICY_BLOCKED", "{policy:?}: {r}");
    }
}

#[test]
#[ignore = "requires live IRIS (iris-dev-iris) and the built binary"]
fn a_legitimate_search_still_works_under_allow() {
    let Some(_bin) = require_iad_binary() else {
        return;
    };
    let mut env = live_env();
    env.extend(data_reads_allowed_env());
    let mut mcp = McpSession::start(&env);
    let r = call(&mut mcp, search());
    assert_eq!(r["success"], true, "{r}");
    assert!(r.get("error_code").is_none(), "{r}");
}

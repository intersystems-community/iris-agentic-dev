//! 137: no tool parameter runs as ObjectScript.
//!
//! Each case sets a sentinel global, calls a tool through the real binary with write tools off and a
//! parameter shaped to close the literal or call it lands in and kill the sentinel, then checks the
//! sentinel. Where the fix is a validator, the case also names the refusal code. Run:
//!
//! ```bash
//! cargo build -p iris-agentic-dev
//! IRIS_HOST=localhost IRIS_WEB_PORT=52780 IRIS_USERNAME=_SYSTEM IRIS_PASSWORD=SYS \
//!   cargo test --features testing --test integration injection_137 -- --ignored --test-threads=1
//! ```

use iris_agentic_dev_core::iris::connection::{DiscoverySource, IrisConnection};
use iris_agentic_dev_core::testing::{call_tool_with_env, live_env, require_iad_binary};
use serde_json::{json, Value};

const SENTINEL: &str = "^IrisDevTest.S137";

/// Ways out of a splice: a closed string literal, a closed call, a closed SQL literal.
const PAYLOADS: &[&str] = &[
    "X\" Kill ^IrisDevTest.S137 Set z=\"",
    "X) Kill ^IrisDevTest.S137 Set z=$Get(^X",
    "X\") Kill ^IrisDevTest.S137 Set z=$Get(^X(\"",
    "X' Kill ^IrisDevTest.S137 Set z='",
];

fn conn() -> (IrisConnection, reqwest::Client) {
    let host = std::env::var("IRIS_HOST").expect("IRIS_HOST must be set for 137 live tests");
    let port = std::env::var("IRIS_WEB_PORT").unwrap_or_else(|_| "52780".into());
    let user = std::env::var("IRIS_USERNAME").unwrap_or_else(|_| "_SYSTEM".into());
    let pass = std::env::var("IRIS_PASSWORD").unwrap_or_else(|_| "SYS".into());
    (
        IrisConnection::new(
            format!("http://{host}:{port}"),
            "USER",
            user,
            pass,
            DiscoverySource::EnvVar,
        ),
        reqwest::Client::new(),
    )
}

async fn sentinel_set() {
    let (c, client) = conn();
    let code = format!(" Set {SENTINEL}=\"alive\"\n Write \"ok\",$C(10)");
    for ns in ["USER", "%SYS"] {
        let _ = c.execute_via_generator(&code, ns, &client).await;
    }
}

async fn sentinel_alive() -> bool {
    let (c, client) = conn();
    let code = format!(" Write $Get({SENTINEL},\"GONE\"),$C(10)");
    let mut alive = true;
    for ns in ["USER", "%SYS"] {
        let out = c
            .execute_via_generator(&code, ns, &client)
            .await
            .unwrap_or_default();
        alive &= out.contains("alive");
    }
    alive
}

fn fill(v: &Value, payload: &str) -> Value {
    match v {
        Value::String(s) if s == "@P" => Value::String(payload.to_string()),
        Value::Array(a) => Value::Array(a.iter().map(|x| fill(x, payload)).collect()),
        Value::Object(o) => Value::Object(
            o.iter()
                .map(|(k, x)| (k.clone(), fill(x, payload)))
                .collect(),
        ),
        other => other.clone(),
    }
}

fn answer(msg: &Value) -> Value {
    let text = msg["result"]["content"][0]["text"]
        .as_str()
        .unwrap_or_default();
    serde_json::from_str(text).unwrap_or_else(|_| json!({ "raw": msg }))
}

/// Calls `tool` once per payload; the sentinel must survive every call, and when `code` is given,
/// every call must be refused with it.
async fn case(tool: &str, args: Value, code: Option<&str>) {
    case_in(tool, args, code, &live_env()).await;
}

/// [`case`] against a server started with `env`.
async fn case_in(tool: &str, args: Value, code: Option<&str>, env: &[(String, String)]) {
    require_iad_binary().expect("binary");
    let mut failures = Vec::new();
    for p in PAYLOADS {
        sentinel_set().await;
        let got = answer(&call_tool_with_env(tool, &fill(&args, p), env));
        if !sentinel_alive().await {
            failures.push(format!("{p:?} killed the sentinel: {got}"));
        }
        if let Some(code) = code {
            if got["error_code"] != code {
                failures.push(format!("{p:?} expected {code}, got {got}"));
            }
        }
    }
    sentinel_set().await;
    assert!(failures.is_empty(), "{tool}:\n{}", failures.join("\n"));
}

// ── Group A: read-only tools ─────────────────────────────────────────────────

#[tokio::test]
#[ignore]
async fn injection_137_a01_database_stats_db() {
    case("iris_database_stats", json!({"db": "@P"}), None).await;
}

#[tokio::test]
#[ignore]
async fn injection_137_a02_journal_search_start_end() {
    // Since B5 the bulk-PHI gate refuses the tool under the default policy before the timestamps
    // are read; validate_timestamp behind it is for a connection that allows the tool.
    case(
        "journal_search",
        json!({"start": "@P"}),
        Some("DATA_POLICY_BLOCKED"),
    )
    .await;
    case(
        "journal_search",
        json!({"end": "@P"}),
        Some("DATA_POLICY_BLOCKED"),
    )
    .await;
}

#[tokio::test]
#[ignore]
async fn injection_137_a04_debug_map_int_and_source_map() {
    case(
        "iris_debug",
        json!({"action": "map_int", "error_string": "@P"}),
        None,
    )
    .await;
    case(
        "iris_debug",
        json!({"action": "source_map", "class_name": "@P"}),
        Some("INVALID_CLASS_NAME"),
    )
    .await;
}

#[tokio::test]
#[ignore]
async fn injection_137_a06_doc_compiled_name() {
    case(
        "iris_doc",
        json!({"mode": "compiled", "name": "@P"}),
        Some("INVALID_ROUTINE_NAME"),
    )
    .await;
}

#[tokio::test]
#[ignore]
async fn injection_137_a07_coverage_report() {
    case(
        "iris_coverage",
        json!({"mode": "report", "classes": ["@P"]}),
        Some("INVALID_CLASS_NAME"),
    )
    .await;
    case(
        "iris_coverage",
        json!({"mode": "report", "package": "@P"}),
        Some("INVALID_CLASS_NAME"),
    )
    .await;
}

#[tokio::test]
#[ignore]
async fn injection_137_a08_stream_and_hl7_schema() {
    case("stream_inspect", json!({"oid": "@P"}), None).await;
    case("hl7_schema_inspect", json!({"schema": "@P"}), None).await;
    case(
        "hl7_schema_inspect",
        json!({"schema": "2.5", "segment": "@P"}),
        None,
    )
    .await;
}

#[tokio::test]
#[ignore]
async fn injection_137_a09_table_info_and_dispatch() {
    case(
        "iris_table_info",
        json!({"table": "@P"}),
        Some("INVALID_SQL_IDENTIFIER"),
    )
    .await;
    case(
        "resolve_dynamic_dispatch",
        json!({"method_name": "@P"}),
        Some("INVALID_METHOD_NAME"),
    )
    .await;
    case(
        "resolve_dynamic_dispatch",
        json!({"method_name": "OnRequest", "package_prefix": "@P"}),
        Some("INVALID_CLASS_NAME"),
    )
    .await;
}

/// A3: a free WHERE fragment is held to iris_query's read-only rules; nothing here can kill.
#[tokio::test]
#[ignore]
async fn injection_137_a03_interop_body_where() {
    let args =
        json!({"what": "messages", "body_class": "Ens.StringRequest", "body_where": "1=1; @P"});
    // Body mode is a bulk-data call (141 T214): under the default policy the gate answers first.
    case(
        "iris_interop_query",
        args.clone(),
        Some("DATA_POLICY_BLOCKED"),
    )
    .await;
    // On a server that allows data reads, the fragment check is what refuses it.
    let mut env = live_env();
    env.extend(iris_agentic_dev_core::testing::data_reads_allowed_env());
    case_in("iris_interop_query", args, Some("INVALID_PARAMS"), &env).await;
}

/// A12: the journal route through iris_admin. Under the default `dataPolicy = "block"` the bulk-PHI
/// gate refuses it before the timestamps are read; the timestamp check behind it is for a
/// connection that allows the tool.
#[tokio::test]
#[ignore]
async fn injection_137_a12_admin_journal_time_range() {
    case(
        "iris_admin",
        json!({"action": "journal_search", "time_range": {"from": "@P"}}),
        Some("DATA_POLICY_BLOCKED"),
    )
    .await;
}

#[tokio::test]
#[ignore]
async fn injection_137_a10_skills_and_kb() {
    case("skill", json!({"action": "describe", "name": "@P"}), None).await;
    case("skill", json!({"action": "search", "query": "@P"}), None).await;
    case("kb", json!({"action": "recall", "query": "@P"}), None).await;
    case("skill_describe", json!({"name": "@P"}), None).await;
}

#[tokio::test]
#[ignore]
async fn injection_137_a11_mermaid_and_storage() {
    case(
        "mermaid_class",
        json!({"class": "@P"}),
        Some("INVALID_CLASS_NAME"),
    )
    .await;
    case(
        "mermaid_production",
        json!({"production": "@P"}),
        Some("INVALID_CLASS_NAME"),
    )
    .await;
    case(
        "resolve_storage",
        json!({"class": "@P"}),
        Some("INVALID_CLASS_NAME"),
    )
    .await;
}

/// T022: every tool that takes a namespace gets it checked once, before its handler runs.
#[tokio::test]
#[ignore]
async fn injection_137_t022_namespace_on_read_only_tools() {
    case(
        "iris_table_info",
        json!({"table": "Sample.Person", "namespace": "@P"}),
        Some("INVALID_NAMESPACE"),
    )
    .await;
    case(
        "resolve_storage",
        json!({"class": "Sample.Person", "namespace": "@P"}),
        Some("INVALID_NAMESPACE"),
    )
    .await;
    case(
        "iris_coverage",
        json!({"mode": "check", "namespace": "@P"}),
        Some("INVALID_NAMESPACE"),
    )
    .await;
}

// ── Group B: gates that never ran ────────────────────────────────────────────

fn call(tool: &str, args: Value) -> Value {
    require_iad_binary().expect("binary");
    answer(&call_tool_with_env(tool, &args, &live_env()))
}

/// B5: global_preview read ^oddDEF past the system blocklist and a PHI-named global past the PHI
/// gate, because it never called dispatch_gate and the gates only read `global_name`.
#[tokio::test]
#[ignore]
async fn injection_137_b5_global_preview_is_gated() {
    let sys = call("global_preview", json!({"global": "oddDEF", "count": 1}));
    assert_eq!(sys["error_code"], "SYSTEM_BLOCKLIST", "{sys}");
    let phi = call("global_preview", json!({"global": "PAPMI", "count": 1}));
    assert_eq!(phi["error_code"], "PHI_GATE_BLOCKED", "{phi}");
    let plain = call(
        "global_preview",
        json!({"global": "IrisDevTest.S137", "count": 1}),
    );
    assert_eq!(
        plain["success"], true,
        "an ordinary global still previews: {plain}"
    );
}

/// B5: the standalone journal_search returned journal contents under dataPolicy=block, which the
/// bulk-PHI gate refuses on the iris_admin route.
#[tokio::test]
#[ignore]
async fn injection_137_b5_journal_search_is_gated() {
    let r = call("journal_search", json!({"max_entries": 1}));
    assert_eq!(r["error_code"], "DATA_POLICY_BLOCKED", "{r}");
}

// ── Group C: URL text on GET routes ──────────────────────────────────────────

/// C: an include name went into the document URL path unencoded, and a search category into the
/// query string, so `x&sys=true` or `../` could change what the request asked for.
#[tokio::test]
#[ignore]
async fn injection_137_c_url_parts_are_checked() {
    let inc = call(
        "iris_macro",
        json!({"action": "list", "includes": ["../../Other/x"]}),
    );
    assert_eq!(inc["error_code"], "INVALID_ROUTINE_NAME", "{inc}");
    let cat = call(
        "iris_search",
        json!({"query": "Foo", "documents": ["MyPkg.*.cls"], "category": "CLS&sys=true"}),
    );
    assert_eq!(cat["error_code"], "INVALID_PARAMS", "{cat}");
    let ok = call(
        "iris_macro",
        json!({"action": "list", "includes": ["%occStatus"]}),
    );
    assert_ne!(
        ok["error_code"], "INVALID_ROUTINE_NAME",
        "a real include still lists: {ok}"
    );
}

/// Found by the objectscript-splice detector after the inventory: `iris_admin
/// action=database_status` put its name filter into `Execute("...")` unescaped, in a read-only
/// action.
#[tokio::test]
#[ignore]
async fn injection_137_d_admin_database_status_name() {
    case(
        "iris_admin",
        json!({"action": "database_status", "name": "@P"}),
        None,
    )
    .await;
}

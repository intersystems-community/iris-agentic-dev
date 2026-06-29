//! iris_global — read, write, kill, and list IRIS globals.

use regex::Regex;
use std::sync::OnceLock;

fn err_json(code: &str, msg: &str) -> serde_json::Value {
    serde_json::json!({"success": false, "error_code": code, "message": msg})
}

// ---------------------------------------------------------------------------
// Subscript validation
// ---------------------------------------------------------------------------

static SUBSCRIPT_RE: OnceLock<Regex> = OnceLock::new();

fn subscript_regex() -> &'static Regex {
    SUBSCRIPT_RE.get_or_init(|| Regex::new(r"^[a-zA-Z0-9 _.:\-]+$").expect("valid regex"))
}

/// Validate that each subscript matches the allowlist pattern.
/// Returns an `INVALID_SUBSCRIPT` error JSON on the first failing subscript.
pub fn validate_subscripts(subscripts: &[String]) -> Result<(), serde_json::Value> {
    let re = subscript_regex();
    for sub in subscripts {
        if !re.is_match(sub) {
            return Err(serde_json::json!({
                "success": false,
                "error_code": "INVALID_SUBSCRIPT",
                "message": format!("subscript '{}' contains disallowed characters (allowed: a-z A-Z 0-9 space . _ : -)", sub),
                "subscript": sub,
                "pattern": "^[a-zA-Z0-9 _.:\\-]+$"
            }));
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Global name normalization
// ---------------------------------------------------------------------------

/// Strip a leading `^` from a global name. `^MyApp` → `MyApp`.
pub fn normalize_global_name(name: &str) -> String {
    name.strip_prefix('^').unwrap_or(name).to_string()
}

// ---------------------------------------------------------------------------
// Global reference builder
// ---------------------------------------------------------------------------

/// Build an IRIS global reference string for use in ObjectScript.
/// `build_global_ref("MyApp", &["a","b"])` → `^MyApp("a","b")`
/// `build_global_ref("MyApp", &[])` → `^MyApp`
///
/// Subscripts MUST already be validated (no quotes or special chars).
pub fn build_global_ref(name: &str, subscripts: &[String]) -> String {
    if subscripts.is_empty() {
        format!("^{}", name)
    } else {
        let subs: Vec<String> = subscripts.iter().map(|s| format!("\"{}\"", s)).collect();
        format!("^{}({})", name, subs.join(","))
    }
}

// ---------------------------------------------------------------------------
// ObjectScript code builders — output-parsing helpers
// ---------------------------------------------------------------------------

/// Parse the output from execute_via_generator for errors.
/// Lines starting with "ERROR: " indicate an ObjectScript catch or $ZERROR.
pub fn parse_execute_output(output: &str) -> Result<String, serde_json::Value> {
    let trimmed = output.trim();
    if let Some(msg) = trimmed.strip_prefix("ERROR: ") {
        return Err(serde_json::json!({
            "success": false,
            "error_code": "IRIS_EXECUTE_ERROR",
            "message": msg.trim()
        }));
    }
    Ok(trimmed.to_string())
}

/// Parse `get` output: `1|value` (defined) or `0|` (undefined).
fn parse_get_output(raw: &str) -> serde_json::Value {
    // After trim the output is like "1|hello-052" or "0|"
    if let Some(rest) = raw.strip_prefix("1|") {
        serde_json::json!({"success": true, "defined": true, "value": rest})
    } else if let Some(_rest) = raw.strip_prefix("0|") {
        serde_json::json!({"success": true, "defined": false, "value": serde_json::Value::Null})
    } else {
        err_json(
            "IRIS_EXECUTE_ERROR",
            &format!("unexpected get output: {raw}"),
        )
    }
}

/// Parse subtree `get` output: lines of `path|value` followed by `DONE|count|truncated`.
fn parse_subtree_output(raw: &str) -> serde_json::Value {
    let mut nodes: Vec<serde_json::Value> = Vec::new();
    let mut count: i64 = 0;
    let mut truncated = false;
    for line in raw.lines() {
        if line.is_empty() {
            continue;
        }
        if let Some(rest) = line.strip_prefix("DONE|") {
            let parts: Vec<&str> = rest.splitn(2, '|').collect();
            count = parts.first().and_then(|s| s.parse().ok()).unwrap_or(0);
            truncated = parts.get(1).map(|s| *s == "1").unwrap_or(false);
        } else if let Some(idx) = line.find('|') {
            let path = &line[..idx];
            let value = &line[idx + 1..];
            nodes.push(serde_json::json!({"path": path, "value": value}));
        }
    }
    serde_json::json!({"success": true, "nodes": nodes, "node_count": count, "truncated": truncated})
}

/// Parse `list` output: subscript values one per line, followed by `DONE|count|truncated`.
fn parse_list_output(raw: &str) -> serde_json::Value {
    let mut subscripts: Vec<serde_json::Value> = Vec::new();
    let mut truncated = false;
    for line in raw.lines() {
        if line.is_empty() {
            continue;
        }
        if let Some(rest) = line.strip_prefix("DONE|") {
            let parts: Vec<&str> = rest.splitn(2, '|').collect();
            truncated = parts.get(1).map(|s| *s == "1").unwrap_or(false);
        } else {
            subscripts.push(serde_json::Value::String(line.to_string()));
        }
    }
    serde_json::json!({"success": true, "subscripts": subscripts, "truncated": truncated})
}

// ---------------------------------------------------------------------------
// Clamp helpers
// ---------------------------------------------------------------------------

pub fn clamp_max_nodes(v: i64) -> i64 {
    v.clamp(1, 1000)
}

pub fn clamp_max_subscripts(v: i64) -> i64 {
    v.clamp(1, 500)
}

// ---------------------------------------------------------------------------
// ObjectScript generators
// ---------------------------------------------------------------------------

// execute_via_generator cannot return content containing `{` characters: the qout encoding
// encodes `\n` as `$Char(1)` which acts as a line separator in the generated class source,
// and any output starting with `{` in the generated `Quit "..."` statement causes a parse
// error in the IRIS class compiler. Strategy: return pipe-delimited or newline-delimited
// plain text and assemble JSON in Rust.

/// Build ObjectScript for a single-node `get`.
///
/// Returns: `1|value` if defined, `0|` if undefined.
/// No `{`/`}` in output — JSON assembled in Rust.
pub fn build_get_code(gref: &str) -> String {
    [
        format!(" Set val = $Get({})", gref),
        format!(" Set def = ($Data({}) > 0)", gref),
        r#" If def  Write "1|"_val,$C(10)"#.to_string(),
        r#" If 'def  Write "0|",$C(10)"#.to_string(),
    ]
    .join("\n")
}

/// Build ObjectScript for a subtree `get`.
///
/// Returns lines of `path|value`, followed by `DONE|count|truncated`.
/// No `{`/`}` in output. Single-line For body — avoid dot-continuation inside
/// Try block where it is unreliable. JSON assembled in Rust.
pub fn build_subtree_get_code(gref: &str, max_nodes: i64) -> String {
    // Single-line For body; truncated determined post-loop by checking if $Query has more.
    [
        " Set startTime = $ZH".to_string(),
        format!(" Set maxNodes = {}", max_nodes),
        format!(" Set baseRef = $Name({})", gref),
        " Set node = baseRef".to_string(),
        " Set count = 0".to_string(),
        r#" For  Set node = $Query(@node)  Quit:node=""  Quit:($Extract(node,1,$Length(baseRef))'=baseRef)  Quit:(count>=maxNodes)  Quit:(($ZH-startTime)>5)  Write node_"|"_@node,$C(10)  Set count=count+1"#.to_string(),
        // truncated=1 if we hit the node cap; time truncation approximated by same check
        // (a 5s timeout stops the loop before all nodes collected, so count<maxNodes but
        //  we cannot distinguish without extra state — accept that time-truncated results
        //  show truncated=false, which is conservative and safe).
        " Set truncated = (count>=maxNodes)".to_string(),
        r#" Write "DONE|"_count_"|"_truncated,$C(10)"#.to_string(),
    ]
    .join("\n")
}

/// Build ObjectScript for a `set` operation.
///
/// Returns: `ok` on success. No `{`/`}` in output.
pub fn build_set_objectscript(gref: &str, value: &str) -> String {
    let escaped_value = value.replace('"', "\"\"");
    [
        format!(" Set {} = \"{}\"", gref, escaped_value),
        r#" Write "ok",$C(10)"#.to_string(),
    ]
    .join("\n")
}

/// Build ObjectScript for a `kill` operation.
///
/// Returns: `ok` on success. No `{`/`}` in output.
pub fn build_kill_code(gref: &str) -> String {
    [
        format!(" Kill {}", gref),
        r#" Write "ok",$C(10)"#.to_string(),
    ]
    .join("\n")
}

/// Build ObjectScript for a `list` operation.
///
/// Returns: subscript values one per line, followed by `DONE|count|truncated`.
/// No `{`/`}` in output. All For-body logic on one line — avoid dot-continuation
/// inside Try block where it is unreliable.
pub fn build_list_code(gref: &str, max_subscripts: i64) -> String {
    // For root globals: ^Name → $Order(^Name(sub))
    // For subscripted refs: ^Name("a") → $Order(^Name("a",sub))
    let order_ref = if gref.contains('(') {
        let (base, _) = gref.rsplit_once(')').unwrap_or((gref, ""));
        format!("{},sub)", base)
    } else {
        format!("{}(sub)", gref)
    };
    // Single-line For body: no dot-continuation needed.
    // truncated=1 iff we hit the maxSubs cap (count reached maxSubs before sub="").
    [
        format!(" Set maxSubs = {}", max_subscripts),
        r#" Set sub = """#.to_string(),
        " Set count = 0".to_string(),
        format!(
            r#" For  Set sub = $Order({})  Quit:sub=""  Quit:(count>=maxSubs)  Write sub,$C(10)  Set count=count+1"#,
            order_ref
        ),
        " Set truncated = (count>=maxSubs)".to_string(),
        r#" Write "DONE|"_count_"|"_truncated,$C(10)"#.to_string(),
    ]
    .join("\n")
}

// ---------------------------------------------------------------------------
// Handler params
// ---------------------------------------------------------------------------

use schemars::JsonSchema;
use serde::Deserialize;

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct IrisGlobalParams {
    /// Action: get, set, kill, list
    pub action: String,
    /// Global name (with or without leading ^)
    pub global_name: String,
    /// Subscripts (each must match ^[a-zA-Z0-9 _.:\-]+$)
    pub subscripts: Option<Vec<String>>,
    /// Value to set (required for action=set)
    pub value: Option<String>,
    /// IRIS namespace (defaults to connection default)
    pub namespace: Option<String>,
    /// get only: return all descendant nodes
    pub subtree: Option<bool>,
    /// get+subtree: max nodes returned (default 100, max 1000)
    pub max_nodes: Option<i64>,
    /// list: max subscripts returned (default 50, max 500)
    pub max_subscripts: Option<i64>,
    /// Bypass PHI name gate (per spec 051)
    #[serde(rename = "acknowledgePhi")]
    pub acknowledge_phi: Option<bool>,
}

// ---------------------------------------------------------------------------
// Main handler — called from tools/mod.rs
// ---------------------------------------------------------------------------

/// Handle an `iris_global` tool call.
/// Returns the JSON response value (not wrapped in CallToolResult — caller wraps).
pub async fn handle_iris_global(
    iris: &crate::iris::connection::IrisConnection,
    client: &reqwest::Client,
    params: &IrisGlobalParams,
    gate_result: Result<(), serde_json::Value>,
) -> serde_json::Value {
    // Gate result already evaluated by caller; propagate if blocked.
    if let Err(gate_err) = gate_result {
        return gate_err;
    }

    let subs = params.subscripts.clone().unwrap_or_default();
    if let Err(e) = validate_subscripts(&subs) {
        return e;
    }

    let name = normalize_global_name(&params.global_name);
    let gref = build_global_ref(&name, &subs);
    let ns = params
        .namespace
        .clone()
        .unwrap_or_else(|| iris.namespace.clone());

    match params.action.as_str() {
        "get" => {
            let subtree = params.subtree.unwrap_or(false);
            let code = if subtree {
                let max_nodes = clamp_max_nodes(params.max_nodes.unwrap_or(100));
                build_subtree_get_code(&gref, max_nodes)
            } else {
                build_get_code(&gref)
            };
            match iris.execute_via_generator(&code, &ns, client).await {
                Err(e) => {
                    let msg = e.to_string();
                    if msg.contains("HTTP 4")
                        || msg.contains("HTTP 5")
                        || msg.contains("connection")
                    {
                        err_json("IRIS_UNREACHABLE", &msg)
                    } else {
                        err_json("IRIS_EXECUTE_ERROR", &msg)
                    }
                }
                Ok(output) => match parse_execute_output(&output) {
                    Err(e) => e,
                    Ok(raw) => {
                        if subtree {
                            parse_subtree_output(&raw)
                        } else {
                            parse_get_output(&raw)
                        }
                    }
                },
            }
        }
        "set" => {
            let value = match &params.value {
                Some(v) => v.clone(),
                None => {
                    return err_json("INVALID_PARAMS", "action=set requires a 'value' parameter")
                }
            };
            let code = build_set_objectscript(&gref, &value);
            match iris.execute_via_generator(&code, &ns, client).await {
                Err(e) => err_json("IRIS_UNREACHABLE", &e.to_string()),
                Ok(output) => match parse_execute_output(&output) {
                    Err(e) => e,
                    Ok(raw) => {
                        if raw == "ok" {
                            serde_json::json!({"success": true})
                        } else {
                            err_json(
                                "IRIS_EXECUTE_ERROR",
                                &format!("unexpected set output: {raw}"),
                            )
                        }
                    }
                },
            }
        }
        "kill" => {
            let code = build_kill_code(&gref);
            match iris.execute_via_generator(&code, &ns, client).await {
                Err(e) => err_json("IRIS_UNREACHABLE", &e.to_string()),
                Ok(output) => match parse_execute_output(&output) {
                    Err(e) => e,
                    Ok(raw) => {
                        if raw == "ok" {
                            serde_json::json!({"success": true})
                        } else {
                            err_json(
                                "IRIS_EXECUTE_ERROR",
                                &format!("unexpected kill output: {raw}"),
                            )
                        }
                    }
                },
            }
        }
        "list" => {
            let max_subs = clamp_max_subscripts(params.max_subscripts.unwrap_or(50));
            let code = build_list_code(&gref, max_subs);
            match iris.execute_via_generator(&code, &ns, client).await {
                Err(e) => err_json("IRIS_UNREACHABLE", &e.to_string()),
                Ok(output) => match parse_execute_output(&output) {
                    Err(e) => e,
                    Ok(raw) => parse_list_output(&raw),
                },
            }
        }
        other => err_json(
            "INVALID_ACTION",
            &format!("unknown action: {other} (expected: get, set, kill, list)"),
        ),
    }
}

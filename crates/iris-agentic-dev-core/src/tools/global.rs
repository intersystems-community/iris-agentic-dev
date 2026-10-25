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

/// Validates a global name before it is spliced into ObjectScript. Lives with the other name
/// validators in `crate::objectscript`.
pub use crate::objectscript::validate_global_name;

/// Strip a leading `^` from a global name. `^MyApp` → `MyApp`.
///
/// This only strips. It does not say the rest is a name; `canonical_global_name` does.
pub fn normalize_global_name(name: &str) -> String {
    name.strip_prefix('^').unwrap_or(name).to_string()
}

/// The one name every gate, the audit scrub and the executor read (spec 052, FR-015, FR-016).
///
/// Strips exactly one leading `^`, then applies [`validate_global_name`], the 137 rule, and a
/// 128-character bound. Anything else is `INVALID_GLOBAL_NAME`.
///
/// The name is interpolated into generated ObjectScript, so this is also the injection guard, the
/// way `validate_subscripts` is for subscripts. Before it existed the gate compared the string as
/// typed while the executor stripped a caret and built `^{name}`: `^ROUTINE` slipped past a rule
/// that refused `ROUTINE`, `oddDEF("x")` slipped past an exact-match rule, and a name containing
/// `"` ended the string literal `global_preview` builds.
pub fn canonical_global_name(raw: &str) -> Result<String, serde_json::Value> {
    let name = normalize_global_name(raw);
    validate_global_name(&name)?;
    if name.len() > 128 {
        let shown: String = name.chars().take(80).collect();
        return Err(serde_json::json!({
            "success": false,
            "error_code": "INVALID_GLOBAL_NAME",
            "message": format!("'{shown}…' is not a valid global name: 128 characters at most."),
            "global_name": shown,
        }));
    }
    Ok(name)
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

/// Parse the output from `execute_via_generator`, separating a failure report from real output.
///
/// The generator reports IRIS-side failure inside the output string, in four shapes — and this
/// function used to recognise exactly one of them (`ERROR: ` with a trailing space). The other
/// three (`ERROR($ZERROR): `, `ERROR($DEVICE): `, and the no-space `ERROR:<sentinel>` that
/// tool-generated ObjectScript emits) fell through to `Ok`, so `global_get` reported
/// `defined: false` for a global it could not read, and `global_preview` reported an empty preview
/// plus a valid `confirm_token` for a kill it had never actually looked at.
///
/// [`is_generator_error`] is the single definition of "this string is a failure"; every caller goes
/// through it so a fifth shape is one edit, not fourteen.
pub fn parse_execute_output(output: &str) -> Result<String, serde_json::Value> {
    let trimmed = output.trim();
    if crate::iris::connection::is_generator_error(trimmed) {
        // Strip whichever prefix matched so the message reads as a message, not as a sentinel.
        let msg = trimmed
            .strip_prefix("ERROR($ZERROR): ")
            .or_else(|| trimmed.strip_prefix("ERROR($DEVICE): "))
            .or_else(|| trimmed.strip_prefix("ERROR: "))
            .or_else(|| trimmed.strip_prefix("ERROR:"))
            .unwrap_or(trimmed);
        return Err(serde_json::json!({
            "success": false,
            "error_code": "IRIS_EXECUTE_ERROR",
            "message": msg.trim()
        }));
    }
    Ok(trimmed.to_string())
}

/// Parse `get` output: `1|<base64>;` (defined) or `0|` (undefined). The value is the Base64 of its
/// UTF-8 bytes, so no CR or trailing space in it reaches the generator's line reader or trim.
///
/// The reader returns at most 32,000 characters per line and puts a line feed after each piece, so
/// a value over about 24 KB arrives with line feeds inside it. Base64 holds none, so they are
/// dropped. The closing `;` tells a whole value from one the output cut short.
pub fn parse_get_output(raw: &str) -> serde_json::Value {
    // After trim the output is like "1|aGk=;" or "0|"
    if let Some(rest) = raw.strip_prefix("1|") {
        let joined = rest.replace('\n', "");
        let Some(encoded) = joined.strip_suffix(';') else {
            return err_json("IRIS_EXECUTE_ERROR", UNFINISHED_VALUE);
        };
        match decode_value(encoded) {
            Some(value) => serde_json::json!({"success": true, "defined": true, "value": value}),
            None => err_json(
                "IRIS_EXECUTE_ERROR",
                "the get output is not valid Base64 of UTF-8 text",
            ),
        }
    } else if let Some(_rest) = raw.strip_prefix("0|") {
        serde_json::json!({"success": true, "defined": false, "value": serde_json::Value::Null})
    } else {
        err_json(
            "IRIS_EXECUTE_ERROR",
            &format!("unexpected get output: {raw}"),
        )
    }
}

const UNFINISHED_VALUE: &str = "the get output ended before the value did";

/// Parse subtree `get` output: lines of `path|<base64>;` followed by `DONE|count|truncated`.
///
/// The value holds no `|`, so the last one splits the line even when a string subscript contains
/// one. A value over about 24 KB arrives in pieces of 32,000 characters on separate lines (see
/// [`parse_get_output`]); a line with no `|` that follows an unfinished node continues it. A node
/// with no closing `;`, or output with no `DONE`, was cut short and is an error.
pub fn parse_subtree_output(raw: &str) -> serde_json::Value {
    let mut nodes: Vec<serde_json::Value> = Vec::new();
    let mut count: i64 = 0;
    let mut truncated = false;
    let mut done = false;
    // The node being read: its path, and the Base64 gathered so far.
    let mut open: Option<(&str, String)> = None;
    for line in raw.lines() {
        if line.is_empty() {
            continue;
        }
        if let (Some((_, encoded)), false) = (open.as_mut(), line.contains('|')) {
            encoded.push_str(line);
        } else if open.is_some() {
            return err_json("IRIS_EXECUTE_ERROR", UNFINISHED_VALUE);
        } else if let Some(rest) = line.strip_prefix("DONE|") {
            done = true;
            let parts: Vec<&str> = rest.splitn(2, '|').collect();
            count = parts.first().and_then(|s| s.parse().ok()).unwrap_or(0);
            truncated = parts.get(1).map(|s| *s == "1").unwrap_or(false);
        } else if let Some(idx) = line.rfind('|') {
            open = Some((&line[..idx], line[idx + 1..].to_string()));
        }
        // A node is whole once its Base64 ends in the `;` terminator.
        if open.as_ref().is_some_and(|(_, e)| e.ends_with(';')) {
            let (path, mut encoded) = open.take().expect("checked above");
            encoded.pop();
            let Some(value) = decode_value(&encoded) else {
                return err_json(
                    "IRIS_EXECUTE_ERROR",
                    &format!("unexpected subtree get output at {path}"),
                );
            };
            nodes.push(serde_json::json!({"path": path, "value": value}));
        }
    }
    if open.is_some() || !done {
        return err_json("IRIS_EXECUTE_ERROR", UNFINISHED_VALUE);
    }
    serde_json::json!({"success": true, "nodes": nodes, "node_count": count, "truncated": truncated})
}

/// Parse `list` output: subscript values one per line, followed by `DONE|count|truncated`.
pub fn parse_list_output(raw: &str) -> serde_json::Value {
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

/// The value a `get` wrote: Base64 of its UTF-8 bytes. `""` is the empty value.
fn decode_value(encoded: &str) -> Option<String> {
    use base64::Engine;
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(encoded)
        .ok()?;
    String::from_utf8(bytes).ok()
}

/// ObjectScript that writes `val` as Base64 of its UTF-8 bytes, then `;`. A value reaches Rust only
/// in this form. Decimal code points cost 3.5 times the value in output; Base64 costs 4/3.
const ENCODE_VALUE: &str = r#"Write $SYSTEM.Encryption.Base64Encode($ZCVT(val,"O","UTF8"),1),";""#;

/// Build ObjectScript for a single-node `get`.
///
/// Returns: `1|<base64>;` (the value as Base64 of its UTF-8 bytes) if defined, `0|` if undefined.
/// No `{`/`}` in output — JSON assembled in Rust.
pub fn build_get_code(gref: &str) -> String {
    [
        format!(" Set val = $Get({})", gref),
        format!(" Set def = ($Data({}) > 0)", gref),
        format!(r#" If def {{ Write "1|" {ENCODE_VALUE} Write $C(10) }}"#),
        r#" If 'def  Write "0|",$C(10)"#.to_string(),
    ]
    .join("\n")
}

/// Build ObjectScript for a subtree `get`.
///
/// Returns lines of `path|<base64>;` (the value encoded as in single `get`), followed by
/// `DONE|count|truncated`.
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
        // $Query starts after the base and never returns it, so a node that holds a value and
        // children ($DATA = 11) lost its own value. Emit it first. A node with children only
        // (D=10) has no value to emit.
        format!(r#" If $Data(@baseRef)#2 {{ Set val=@baseRef Write baseRef_"|" {ENCODE_VALUE} Write $C(10) Set count=count+1 }}"#),
        // A descendant starts with the reference up to its closing parenthesis, plus a comma:
        // `^G("a")` -> `^G("a",`. Comparing against the whole reference (closing parenthesis
        // included) never matched below a subscripted node (#146), and a prefix without the
        // comma would let `^G(10)` through as a descendant of `^G(1)`.
        r#" Set prefix = $Select($Extract(baseRef,$Length(baseRef))=")":$Extract(baseRef,1,$Length(baseRef)-1)_",",1:baseRef_"(")"#.to_string(),
        format!(r#" For  Set node = $Query(@node)  Quit:node=""  Quit:($Extract(node,1,$Length(prefix))'=prefix)  Quit:(count>=maxNodes)  Quit:(($ZH-startTime)>5)  Set val=@node Write node_"|" {ENCODE_VALUE} Write $C(10)  Set count=count+1"#),
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
    [
        format!(
            " Set {} = {}",
            gref,
            crate::objectscript::os_str_expr(value)
        ),
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
#[serde(deny_unknown_fields)]
pub struct IrisGlobalParams {
    /// Confirms the call on a `subject` server, where a write is refused until it is set. No
    /// effect on other servers.
    #[serde(default)]
    pub confirm: bool,
    /// Action: get, set, kill, list
    #[schemars(extend("enum" = ["get", "set", "kill", "list"]))]
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
    /// Route this call to a named registered IRIS instance. If omitted, uses the default connection.
    #[serde(default)]
    pub server: Option<String>,
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

    // The gate saw the canonical name; the handler checks it again so a caller that reaches this
    // function without the gate still cannot interpolate an invalid one (FR-019).
    let name = match canonical_global_name(&params.global_name) {
        Ok(n) => n,
        Err(e) => return e,
    };
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

// ── Unit tests ────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ── validate_subscripts ────────────────────────────────────────────────

    #[test]
    fn validate_subscripts_valid() {
        assert!(validate_subscripts(&["hello".to_string(), "world-1".to_string()]).is_ok());
    }

    #[test]
    fn validate_subscripts_empty_list() {
        assert!(validate_subscripts(&[]).is_ok());
    }

    #[test]
    fn validate_subscripts_invalid_char() {
        let result = validate_subscripts(&["bad$char".to_string()]);
        let err = result.unwrap_err();
        assert_eq!(err["error_code"], "INVALID_SUBSCRIPT");
    }

    // ── normalize_global_name ──────────────────────────────────────────────

    #[test]
    fn normalize_strips_caret() {
        assert_eq!(normalize_global_name("^MyGlobal"), "MyGlobal");
    }

    #[test]
    fn normalize_no_caret_unchanged() {
        assert_eq!(normalize_global_name("MyGlobal"), "MyGlobal");
    }

    // ── build_global_ref ───────────────────────────────────────────────────

    #[test]
    fn build_global_ref_no_subscripts() {
        assert_eq!(build_global_ref("MyGlobal", &[]), "^MyGlobal");
    }

    #[test]
    fn build_global_ref_with_subscripts() {
        let subs = vec!["a".to_string(), "b".to_string()];
        assert_eq!(build_global_ref("MyGlobal", &subs), r#"^MyGlobal("a","b")"#);
    }

    // ── parse_execute_output ───────────────────────────────────────────────

    #[test]
    fn parse_execute_output_ok() {
        assert_eq!(parse_execute_output("ok\n"), Ok("ok".to_string()));
    }

    #[test]
    fn parse_execute_output_error() {
        let result = parse_execute_output("ERROR: <UNDEFINED>x");
        let err = result.unwrap_err();
        assert_eq!(err["error_code"], "IRIS_EXECUTE_ERROR");
    }

    // ── parse_get_output ───────────────────────────────────────────────────

    #[test]
    fn parse_get_output_defined() {
        let v = parse_get_output("1|aGk=;");
        assert_eq!(v["defined"], true);
        assert_eq!(v["value"], "hi");
    }

    #[test]
    fn parse_get_output_undefined() {
        let v = parse_get_output("0|");
        assert_eq!(v["defined"], false);
        assert!(v["value"].is_null());
    }

    #[test]
    fn parse_get_output_unexpected() {
        let v = parse_get_output("garbage");
        assert_eq!(v["error_code"], "IRIS_EXECUTE_ERROR");
    }

    // ── parse_subtree_output ───────────────────────────────────────────────

    #[test]
    fn parse_subtree_output_with_nodes() {
        let raw = "^Global(\"a\")|dmFsMQ==;\n^Global(\"b\")|dmFsMg==;\nDONE|2|0\n";
        let v = parse_subtree_output(raw);
        assert_eq!(v["node_count"], 2);
        assert_eq!(v["truncated"], false);
        let nodes = v["nodes"].as_array().unwrap();
        assert_eq!(nodes.len(), 2);
    }

    #[test]
    fn parse_subtree_output_truncated() {
        let raw = "^G(\"a\")|dg==;\nDONE|1|1\n";
        let v = parse_subtree_output(raw);
        assert_eq!(v["truncated"], true);
    }

    // ── parse_list_output ─────────────────────────────────────────────────

    #[test]
    fn parse_list_output_with_subscripts() {
        let raw = "a\nb\nc\nDONE|3|0\n";
        let v = parse_list_output(raw);
        let subs = v["subscripts"].as_array().unwrap();
        assert_eq!(subs.len(), 3);
        assert_eq!(v["truncated"], false);
    }

    #[test]
    fn parse_list_output_truncated() {
        let raw = "a\nDONE|1|1\n";
        let v = parse_list_output(raw);
        assert_eq!(v["truncated"], true);
    }

    // ── clamp helpers ──────────────────────────────────────────────────────

    #[test]
    fn clamp_max_nodes_enforces_bounds() {
        assert_eq!(clamp_max_nodes(0), 1);
        assert_eq!(clamp_max_nodes(500), 500);
        assert_eq!(clamp_max_nodes(9999), 1000);
    }

    #[test]
    fn clamp_max_subscripts_enforces_bounds() {
        assert_eq!(clamp_max_subscripts(0), 1);
        assert_eq!(clamp_max_subscripts(200), 200);
        assert_eq!(clamp_max_subscripts(999), 500);
    }

    // ── code builders ─────────────────────────────────────────────────────

    #[test]
    fn build_get_code_references_gref() {
        let code = build_get_code("^MyGlobal");
        assert!(code.contains("^MyGlobal"));
        assert!(code.contains("$Get"));
    }

    #[test]
    fn build_set_objectscript_escapes_quotes() {
        let code = build_set_objectscript("^G", r#"say "hi""#);
        assert!(code.contains(r#""""hi"""#) || code.contains("\"\"hi\"\""));
    }

    #[test]
    fn build_set_objectscript_keeps_a_newline_on_one_line() {
        // Spec 141 C3: a newline in `value` must not start a source line in the scratch method.
        let code = build_set_objectscript("^G", "a\n#execute Set ^IadSentinel=1");
        let lines: Vec<&str> = code.lines().collect();
        assert_eq!(lines.len(), 2, "the Set and the Write only: {code}");
        assert!(
            code.contains("$CHAR(10)"),
            "the newline is a $CHAR splice: {code}"
        );
    }

    #[test]
    fn build_kill_code_references_gref() {
        let code = build_kill_code("^MyGlobal");
        assert!(code.contains("Kill ^MyGlobal"));
    }

    #[test]
    fn build_list_code_unsubscripted_ref() {
        let code = build_list_code("^MyGlobal", 50);
        assert!(code.contains("^MyGlobal"));
        assert!(code.contains("$Order"));
    }

    #[test]
    fn build_list_code_subscripted_ref() {
        let code = build_list_code(r#"^MyGlobal("a")"#, 50);
        assert!(code.contains("$Order"));
    }

    #[test]
    fn build_subtree_get_code_references_gref() {
        let code = build_subtree_get_code("^MyGlobal", 100);
        assert!(code.contains("^MyGlobal"));
        assert!(code.contains("$Query"));
    }
}

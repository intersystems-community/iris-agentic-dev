//! CLI batch/script mode (076-interface-modernization, User Story 3).
//!
//! `iris_ws_open`/`iris_ws_exec`/`iris_ws_close`, `iris_doc`'s SCM-checkout elicitation
//! resume, and `iris_get_log` all hold their state in an in-process pool
//! (`WsSessionPool`/`ElicitationStore`/`LogStore`) owned by one `IrisTools` instance.
//! Two separate CLI invocations always get two separate, empty pools — no flag design
//! can make a token minted in one process resolve in another. The only way to give
//! these three real CLI support is to keep one `IrisTools` instance alive across
//! multiple tool calls in a single process. That's what this is: a short script of
//! `{tool, args}` steps, run in one process, sharing one `IrisTools` — the same
//! `dispatch::call` path `compile`/`exec`/`query`/`doc` already route through (FR-004:
//! this must be a thin loop over that dispatch mechanism, not a fourth
//! parallel-implementation risk alongside the three already found and fixed this
//! session).
//!
//! Later steps often need a value a *prior* step only produced at runtime — a WS
//! session token, an elicitation ID — which nobody authoring the script could know in
//! advance. Args support a placeholder of the form `{{<step-index>.<field>}}` (the
//! entire string value, not embedded in a larger string) that gets resolved against an
//! earlier step's parsed JSON response before dispatch.
//!
//! Example (open a WS session, exec in it twice, close it):
//! ```json
//! [
//!   {"tool": "iris_ws_open", "args": {}},
//!   {"tool": "iris_ws_exec", "args": {"session": "{{0.session}}", "code": "Set x=1"}},
//!   {"tool": "iris_ws_exec", "args": {"session": "{{0.session}}", "code": "Write x"}},
//!   {"tool": "iris_ws_close", "args": {"session": "{{0.session}}"}}
//! ]
//! ```

use anyhow::{Context, Result};
use clap::Args;
use serde::Deserialize;
use std::io::Read;
use std::path::PathBuf;

use super::connection_args::ConnectionArgs;
use super::dispatch;

#[derive(Args)]
pub struct BatchCommand {
    /// Path to a JSON batch script (an array of `{"tool": ..., "args": {...}}` steps).
    /// Reads from stdin if omitted.
    #[arg(long, short = 'f', value_name = "FILE")]
    pub file: Option<PathBuf>,

    #[command(flatten)]
    pub conn: ConnectionArgs,
}

#[derive(Debug, Deserialize)]
struct BatchStep {
    tool: String,
    #[serde(default = "default_args")]
    args: serde_json::Value,
}

fn default_args() -> serde_json::Value {
    serde_json::json!({})
}

impl BatchCommand {
    pub async fn run(self) -> Result<()> {
        let script_text = match &self.file {
            Some(path) => std::fs::read_to_string(path)
                .with_context(|| format!("reading {}", path.display()))?,
            None => {
                let mut buf = String::new();
                std::io::stdin()
                    .read_to_string(&mut buf)
                    .context("reading batch script from stdin")?;
                buf
            }
        };

        let steps: Vec<BatchStep> = serde_json::from_str(&script_text).context(
            "batch script must be a JSON array of {\"tool\": ..., \"args\": {...}} steps",
        )?;

        if steps.is_empty() {
            eprintln!("error: batch script is empty — nothing to run");
            std::process::exit(1);
        }

        let iris = self.conn.resolve().await?;
        let tools = dispatch::build_tools(iris)?;

        // One IrisTools instance for the whole batch — this is the entire point.
        // WsSessionPool/ElicitationStore/LogStore live on `tools` and survive across
        // every step below, unlike across separate CLI invocations.
        let mut history: Vec<serde_json::Value> = Vec::with_capacity(steps.len());

        for (i, step) in steps.iter().enumerate() {
            let args = substitute_placeholders(&step.args, &history)
                .with_context(|| format!("step {i} ({})", step.tool))?;

            match dispatch::call(&tools, &step.tool, args).await {
                Ok(body) => {
                    println!("[{i}] {}: {}", step.tool, body);
                    let failed = dispatch::is_failure(&body);
                    history.push(body);
                    if failed {
                        eprintln!(
                            "error: step {i} ({}) reported failure — stopping batch",
                            step.tool
                        );
                        std::process::exit(1);
                    }
                }
                Err(e) => {
                    eprintln!("error: step {i} ({}) failed: {e}", step.tool);
                    std::process::exit(1);
                }
            }
        }

        Ok(())
    }
}

/// Recursively substitute `{{<step-index>.<field>}}` placeholders found as a whole
/// string value anywhere in `value` (including nested objects/arrays), resolving each
/// against `history` — the parsed responses of steps that have already run.
fn substitute_placeholders(
    value: &serde_json::Value,
    history: &[serde_json::Value],
) -> Result<serde_json::Value> {
    match value {
        serde_json::Value::String(s) => match resolve_placeholder(s, history)? {
            Some(resolved) => Ok(resolved),
            None => Ok(value.clone()),
        },
        serde_json::Value::Array(arr) => {
            let mut out = Vec::with_capacity(arr.len());
            for v in arr {
                out.push(substitute_placeholders(v, history)?);
            }
            Ok(serde_json::Value::Array(out))
        }
        serde_json::Value::Object(obj) => {
            let mut out = serde_json::Map::with_capacity(obj.len());
            for (k, v) in obj {
                out.insert(k.clone(), substitute_placeholders(v, history)?);
            }
            Ok(serde_json::Value::Object(out))
        }
        other => Ok(other.clone()),
    }
}

/// Recognize a whole-string placeholder `{{<step-index>.<field>}}` (e.g. `{{0.session}}`)
/// and resolve it against a prior step's parsed JSON response. Deliberately not a
/// general string-interpolation engine — the placeholder must be the entire string
/// value, so there's no ambiguity about what JSON type the resolved value should be
/// (whatever type the referenced field actually is, not necessarily a string).
/// Returns `Ok(None)` for an ordinary string that isn't a placeholder at all.
fn resolve_placeholder(
    s: &str,
    history: &[serde_json::Value],
) -> Result<Option<serde_json::Value>> {
    let Some(inner) = s
        .strip_prefix("{{")
        .and_then(|rest| rest.strip_suffix("}}"))
    else {
        return Ok(None);
    };
    let (index_str, field) = inner.split_once('.').with_context(|| {
        format!("invalid placeholder {s:?} — expected {{{{<step-index>.<field>}}}}")
    })?;
    let index: usize = index_str
        .trim()
        .parse()
        .with_context(|| format!("invalid step index in placeholder {s:?}"))?;
    let step_result = history.get(index).ok_or_else(|| {
        anyhow::anyhow!(
            "placeholder {s:?} references step {index}, but only {} step(s) have run so far",
            history.len()
        )
    })?;
    let field = field.trim();
    let value = step_result.get(field).ok_or_else(|| {
        anyhow::anyhow!("placeholder {s:?}: step {index}'s response has no field {field:?} (response: {step_result})")
    })?;
    Ok(Some(value.clone()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn history_with(responses: &[serde_json::Value]) -> Vec<serde_json::Value> {
        responses.to_vec()
    }

    #[test]
    fn plain_string_is_unchanged() {
        let history = history_with(&[]);
        let v = serde_json::json!("just a normal string");
        assert_eq!(substitute_placeholders(&v, &history).unwrap(), v);
    }

    #[test]
    fn resolves_top_level_string_field() {
        let history = history_with(&[serde_json::json!({"session": "ws:abc:USER:123"})]);
        let v = serde_json::json!({"session": "{{0.session}}", "code": "Write 1"});
        let resolved = substitute_placeholders(&v, &history).unwrap();
        assert_eq!(resolved["session"], "ws:abc:USER:123");
        assert_eq!(resolved["code"], "Write 1");
    }

    #[test]
    fn resolves_non_string_field_as_its_real_type() {
        let history = history_with(&[serde_json::json!({"count": 42, "success": true})]);
        let v = serde_json::json!({"n": "{{0.count}}", "ok": "{{0.success}}"});
        let resolved = substitute_placeholders(&v, &history).unwrap();
        assert_eq!(resolved["n"], 42);
        assert_eq!(resolved["ok"], true);
    }

    #[test]
    fn resolves_placeholder_nested_in_array() {
        let history = history_with(&[serde_json::json!({"id": "abc-123"})]);
        let v = serde_json::json!({"names": ["{{0.id}}", "literal"]});
        let resolved = substitute_placeholders(&v, &history).unwrap();
        assert_eq!(resolved["names"][0], "abc-123");
        assert_eq!(resolved["names"][1], "literal");
    }

    #[test]
    fn missing_step_index_is_a_clear_error() {
        let history = history_with(&[serde_json::json!({"session": "x"})]);
        let v = serde_json::json!("{{5.session}}");
        let err = substitute_placeholders(&v, &history).unwrap_err();
        assert!(err.to_string().contains("step 5"));
    }

    #[test]
    fn missing_field_is_a_clear_error() {
        let history = history_with(&[serde_json::json!({"session": "x"})]);
        let v = serde_json::json!("{{0.nonexistent}}");
        let err = substitute_placeholders(&v, &history).unwrap_err();
        assert!(err.to_string().contains("nonexistent"));
    }

    #[test]
    fn batch_step_parses_from_json_array() {
        let script = r#"[
            {"tool": "iris_ws_open", "args": {}},
            {"tool": "iris_ws_exec", "args": {"session": "{{0.session}}", "code": "Write 1"}}
        ]"#;
        let steps: Vec<BatchStep> = serde_json::from_str(script).unwrap();
        assert_eq!(steps.len(), 2);
        assert_eq!(steps[0].tool, "iris_ws_open");
        assert_eq!(steps[1].tool, "iris_ws_exec");
    }

    #[test]
    fn batch_step_args_default_to_empty_object_when_omitted() {
        let script = r#"[{"tool": "iris_servers"}]"#;
        let steps: Vec<BatchStep> = serde_json::from_str(script).unwrap();
        assert_eq!(steps[0].args, serde_json::json!({}));
    }
}

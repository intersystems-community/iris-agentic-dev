//! Rejecting parameter names no tool accepts (113 US4, FR-015 through FR-017).
//!
//! Every tool now advertises its parameters, which makes one question answerable that was not
//! answerable before: is this key one the tool knows? The answer is enforced in exactly one place —
//! the `call_tool` override in `mod.rs`, right after the write/destructive gate — for the same
//! reason the gate lives there. A per-handler check is a check the eighty-second tool forgets, and
//! that is precisely how four mutating tools shipped ungated (085).
//!
//! The accepted names come from `ToolRouter::list_all()`, so the list a caller is shown is the list
//! a client was served on `tools/list`. There is no second copy to keep in sync.
//!
//! `#[serde(deny_unknown_fields)]` on the params structs already rejects a stray key — but as a
//! JSON-RPC transport error carrying serde's phrasing, with no `error_code` a client can branch on
//! and no list of what the tool would have accepted. This module is what turns that into an answer.

use std::collections::{BTreeMap, BTreeSet};

/// The error code a caller branches on. `UNKNOWN_PARAMETER` means "I do not know that name";
/// `INVALID_PARAMS` means "I know the name, that value is not permitted" (FR-017).
pub const ERR_UNKNOWN_PARAMETER: &str = "UNKNOWN_PARAMETER";
pub const ERR_INVALID_PARAMS: &str = "INVALID_PARAMS";

/// How rmcp opens the message when a tool's arguments will not deserialize into its params struct
/// (`rmcp-3.1.3/src/handler/server/router/tool.rs:144`).
///
/// Matching on this prefix is what separates "the router could not read the arguments" from an
/// error a handler produced deliberately. Pinned as a constant because it is rmcp's wording, not
/// ours: if an upgrade changes it, `tests/binary/invalid_params.rs` fails rather than the
/// normalization silently going quiet.
const DESERIALIZATION_PREFIX: &str = "failed to deserialize parameters:";

/// Argument keys the tool does not advertise, sorted.
///
/// Sorted rather than in wire order because JSON object order is not meaningful and a message whose
/// key order depends on how the client happened to serialize its map is a message no test can pin.
pub fn unknown_keys(
    arguments: Option<&serde_json::Map<String, serde_json::Value>>,
    accepted: &BTreeSet<String>,
) -> Vec<String> {
    let Some(args) = arguments else {
        return Vec::new();
    };
    let mut unknown: Vec<String> = args
        .keys()
        .filter(|k| !accepted.contains(k.as_str()))
        .cloned()
        .collect();
    unknown.sort();
    unknown
}

/// The refusal text, per `specs/113-typed-tool-schemas/contracts/rejection-response.md`.
///
/// Three sentences: what was rejected, what would have been accepted, and — when a name is one edit
/// away — what the caller probably meant. SC-008 asks that this be enough to fix the call without
/// opening the docs, which is why the accepted list is spelled out in full rather than replaced by
/// a pointer to `docs/tools.md`.
pub fn unknown_parameter_message(
    tool: &str,
    unknown: &[String],
    accepted: &BTreeSet<String>,
) -> String {
    let offending = unknown
        .iter()
        .map(|k| format!("\"{k}\""))
        .collect::<Vec<_>>()
        .join(", ");
    let mut msg = format!("{tool} does not accept {offending}. ");

    if accepted.is_empty() {
        msg.push_str(&format!("{tool} accepts no parameters."));
    } else {
        msg.push_str(&format!(
            "Accepted parameters: {}.",
            accepted
                .iter()
                .map(String::as_str)
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }

    let suggestions: Vec<(&String, String)> = unknown
        .iter()
        .filter_map(|k| nearest(k, accepted).map(|s| (k, s)))
        .collect();
    match suggestions.as_slice() {
        [] => {}
        // One unknown key and one suggestion: the pairing is unambiguous, so say the short thing.
        [(_, only)] if unknown.len() == 1 => {
            msg.push_str(&format!(" Did you mean \"{only}\"?"));
        }
        many => {
            for (key, suggestion) in many {
                msg.push_str(&format!(
                    " Did you mean \"{suggestion}\" instead of \"{key}\"?"
                ));
            }
        }
    }
    msg
}

/// The accepted name one edit from `key`, or `None` when nothing is that close.
///
/// One edit, not two: a guess at distance 2 is as likely to send the caller after the wrong
/// parameter as the right one, and the accepted list is printed in full anyway. Ties resolve to the
/// lexicographically smallest name — `accepted` is a `BTreeSet`, so the first match found is
/// already the smallest.
///
/// Public because tool names get the same treatment as parameter names: a `--schema` request for a
/// name one letter off from `iris_query` suggests it through this function rather than through a
/// second distance metric that could disagree with the one behind `UNKNOWN_PARAMETER`.
pub fn nearest(key: &str, accepted: &BTreeSet<String>) -> Option<String> {
    accepted
        .iter()
        .find(|name| edit_distance_within_one(key, name))
        .cloned()
}

/// True when `a` and `b` are within one Damerau-Levenshtein edit: one insertion, one deletion, one
/// substitution, or one transposition of adjacent characters.
///
/// Transposition has to count as a single edit or the contract's own worked example fails —
/// `namesapce` is two plain Levenshtein edits from `namespace` but one fat-fingered keystroke, and
/// it is the most common way an agent misspells a parameter.
///
/// Compared over `char`s rather than bytes so a multi-byte name cannot be mis-measured, and the
/// early length check keeps this O(n) for the overwhelmingly common "not close at all" case.
fn edit_distance_within_one(a: &str, b: &str) -> bool {
    let (x, y): (Vec<char>, Vec<char>) = (a.chars().collect(), b.chars().collect());
    if x == y {
        // Not a typo — an exact match would not have reached this module.
        return false;
    }
    match x.len().abs_diff(y.len()) {
        0 => {
            // Substitution, or one adjacent transposition.
            let diffs: Vec<usize> = (0..x.len()).filter(|&i| x[i] != y[i]).collect();
            match diffs.as_slice() {
                [_] => true,
                [i, j] if *j == i + 1 => x[*i] == y[*j] && x[*j] == y[*i],
                _ => false,
            }
        }
        1 => {
            // One insertion, seen from whichever side is shorter.
            let (short, long) = if x.len() < y.len() {
                (&x, &y)
            } else {
                (&y, &x)
            };
            let mut si = 0;
            let mut skipped = false;
            for c in long.iter() {
                if si < short.len() && short[si] == *c {
                    si += 1;
                } else if skipped {
                    return false;
                } else {
                    skipped = true;
                }
            }
            true
        }
        _ => false,
    }
}

/// What one tool advertises: the parameter names, and the property schemas behind them.
///
/// The names answer "is this key one I know". The schemas answer "which key was the wrong shape",
/// which serde cannot answer — its deserialization error names the offending value and the expected
/// Rust type but not the field, so `invalid type: string "100", expected u32` leaves a caller with
/// eight parameters to guess between.
#[derive(Clone)]
pub struct AcceptedParams {
    /// Sorted, so the accepted list in a refusal reads the same on every platform. Kept separately
    /// from `properties` because `serde_json::Map` iterates in insertion order when the crate is
    /// built with `preserve_order`, and a sorted list is part of the contract.
    pub names: BTreeSet<String>,
    /// The advertised `properties` object, verbatim.
    pub properties: serde_json::Map<String, serde_json::Value>,
}

/// Every tool's advertised parameters, keyed by tool name.
///
/// Built once from the router and cached, because `list_all()` clones the whole catalog and this
/// runs on every single `tools/call`. Read off the same `input_schema` a client was served, so
/// "accepted" cannot mean one thing to the validator and another to the caller.
///
/// A tool advertising no `properties` key at all maps to an empty set, which refuses every argument.
/// That state is unreachable — `tests/binary/schema_census.rs` asserts zero such tools — but if it
/// ever returns, refusing loudly beats accepting silently.
pub fn accepted_parameters(tools: &[rmcp::model::Tool]) -> BTreeMap<String, AcceptedParams> {
    tools
        .iter()
        .map(|t| {
            let properties = t
                .input_schema
                .get("properties")
                .and_then(|p| p.as_object())
                .cloned()
                .unwrap_or_default();
            let names = properties.keys().cloned().collect();
            (t.name.to_string(), AcceptedParams { names, properties })
        })
        .collect()
}

/// serde's text, when `result` is rmcp's argument-deserialization refusal rather than a handler's
/// own answer.
///
/// rmcp does not surface this as a JSON-RPC error — `into_tool_argument_error` converts it into an
/// `Ok` error result carrying one text block — so the only way to recognize it is by its shape:
/// flagged as an error, no structured content, and serde's wording in the first block. A handler
/// refusal always has structured content, which is what keeps this from catching one by mistake.
pub fn deserialization_failure(result: &rmcp::model::CallToolResult) -> Option<String> {
    if result.is_error != Some(true) || result.structured_content.is_some() {
        return None;
    }
    let text = &result.content.first()?.as_text()?.text;
    text.starts_with(DESERIALIZATION_PREFIX)
        .then(|| text.clone())
}

/// The refusal text for a parameter of the wrong type (FR-017).
///
/// Two sentences where the advertised schema can identify the offending key, one where it cannot.
/// `serde_message` is appended either way rather than paraphrased: it carries the value and the
/// expected Rust type, and rewriting it into something tidier is how detail gets lost.
///
/// Takes `mismatches` already computed rather than the arguments, because the caller has to locate
/// them before the router consumes the request and this way nothing large is cloned to get here.
pub fn invalid_params_message(
    tool: &str,
    mismatches: &[(String, String, String)],
    serde_message: &str,
) -> String {
    if mismatches.is_empty() {
        return format!("{tool}: {serde_message}");
    }
    let located = mismatches
        .iter()
        .map(|(name, expected, got)| {
            format!("parameter \"{name}\" expects {expected}, received {got}.")
        })
        .collect::<Vec<_>>()
        .join(" ");
    format!("{tool}: {located} {serde_message}")
}

/// Arguments whose JSON type disagrees with the type their advertised property declares, as
/// `(name, expected phrase, received phrase)`.
///
/// Only reports what the schema can actually judge. A property with no discoverable `type` is
/// skipped rather than guessed at, and a `null` is left alone — every optional parameter accepts it,
/// and a caller who sent one is not making a type error worth a sentence of its own.
pub fn type_mismatches(
    arguments: Option<&serde_json::Map<String, serde_json::Value>>,
    properties: &serde_json::Map<String, serde_json::Value>,
) -> Vec<(String, String, String)> {
    let Some(args) = arguments else {
        return Vec::new();
    };
    let mut out: Vec<(String, String, String)> = Vec::new();
    for (name, value) in args {
        if value.is_null() {
            continue;
        }
        let Some(prop) = properties.get(name) else {
            continue; // an unknown name is a different refusal, already answered above this one
        };
        let allowed = declared_types(prop);
        if allowed.is_empty() || accepts(&allowed, value) {
            continue;
        }
        out.push((
            name.clone(),
            phrase_list(&allowed),
            article(json_type_name(value)).to_string(),
        ));
    }
    out.sort();
    out
}

/// The JSON type names a property schema permits, `null` excluded.
///
/// Reads `type` (string or array) and recurses into `anyOf`/`oneOf`, because that is where an
/// optional parameter's real type lands after `normalize_schema_openapi3` rewrites `["string",
/// "null"]` into a branch.
fn declared_types(prop: &serde_json::Value) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    match prop.get("type") {
        Some(serde_json::Value::String(s)) => {
            out.insert(s.clone());
        }
        Some(serde_json::Value::Array(arr)) => {
            for v in arr {
                if let Some(s) = v.as_str() {
                    out.insert(s.to_string());
                }
            }
        }
        _ => {}
    }
    for key in ["anyOf", "oneOf"] {
        if let Some(arr) = prop.get(key).and_then(|v| v.as_array()) {
            for branch in arr {
                out.extend(declared_types(branch));
            }
        }
    }
    out.remove("null");
    out
}

/// Whether `value`'s JSON type satisfies one of `allowed`.
///
/// `integer` needs its own arm: JSON has one number type, so a schema saying `integer` and a value
/// of `3` agree even though `json_type_name` calls the value a number.
fn accepts(allowed: &BTreeSet<String>, value: &serde_json::Value) -> bool {
    if value.is_number() {
        return allowed.contains("number")
            || (allowed.contains("integer") && (value.is_i64() || value.is_u64()));
    }
    allowed.contains(json_type_name(value))
}

fn json_type_name(value: &serde_json::Value) -> &'static str {
    match value {
        serde_json::Value::Null => "null",
        serde_json::Value::Bool(_) => "boolean",
        serde_json::Value::Number(_) => "number",
        serde_json::Value::String(_) => "string",
        serde_json::Value::Array(_) => "array",
        serde_json::Value::Object(_) => "object",
    }
}

fn article(type_name: &str) -> String {
    match type_name {
        "integer" | "array" | "object" => format!("an {type_name}"),
        other => format!("a {other}"),
    }
}

fn phrase_list(types: &BTreeSet<String>) -> String {
    types
        .iter()
        .map(|t| article(t))
        .collect::<Vec<_>>()
        .join(" or ")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn set(names: &[&str]) -> BTreeSet<String> {
        names.iter().map(|s| s.to_string()).collect()
    }

    /// `edit_distance_within_one` is the only part of this module a caller cannot see directly, and
    /// the part where an off-by-one turns a helpful suggestion into a misleading one. The four edit
    /// kinds and the two rejections are stated here rather than inferred from message text.
    #[test]
    fn the_four_single_edit_shapes_are_recognized() {
        assert!(edit_distance_within_one("namespac", "namespace")); // deletion
        assert!(edit_distance_within_one("namespacee", "namespace")); // insertion
        assert!(edit_distance_within_one("nomespace", "namespace")); // substitution
        assert!(edit_distance_within_one("namesapce", "namespace")); // transposition
    }

    #[test]
    fn two_edits_and_an_exact_match_are_both_rejected() {
        assert!(!edit_distance_within_one("nomespoce", "namespace"));
        assert!(!edit_distance_within_one("namespace", "namespace"));
        assert!(!edit_distance_within_one("frobnicate", "namespace"));
        // Length differing by two cannot be one edit, whatever the characters are.
        assert!(!edit_distance_within_one("namespaceee", "namespace"));
    }

    /// A non-ASCII name must not be measured in bytes. `é` is two bytes and one char; comparing
    /// bytes would report a two-edit difference and suppress a correct suggestion.
    #[test]
    fn distance_is_measured_in_characters_not_bytes() {
        assert!(edit_distance_within_one("café", "cafe"));
    }

    #[test]
    fn nothing_is_suggested_from_an_empty_accepted_set() {
        assert_eq!(nearest("namespace", &set(&[])), None);
    }

    fn props(json: serde_json::Value) -> serde_json::Map<String, serde_json::Value> {
        json.as_object().expect("an object").clone()
    }

    fn args(json: serde_json::Value) -> serde_json::Map<String, serde_json::Value> {
        json.as_object().expect("an object").clone()
    }

    /// The two shapes the contract calls out by name: a plain type mismatch, and an integer
    /// parameter handed a quoted number — the `query_audit_log {"limit": "100"}` case that used to be
    /// dropped silently and answered with the default.
    #[test]
    fn the_mismatched_parameter_is_named_and_both_types_spelled_out() {
        let schema = props(serde_json::json!({"query": {"type": "string"}}));
        let msg = invalid_params_message(
            "iris_query",
            &type_mismatches(Some(&args(serde_json::json!({"query": 42}))), &schema),
            "failed to deserialize parameters: invalid type: integer 42, expected a string",
        );
        assert!(
            msg.starts_with("iris_query: parameter \"query\" expects a string, received a number."),
            "got: {msg}"
        );
        assert!(
            msg.contains("invalid type: integer 42"),
            "serde's own text has to survive: {msg}"
        );

        let schema = props(serde_json::json!({"limit": {"type": "integer"}}));
        let msg = invalid_params_message(
            "query_audit_log",
            &type_mismatches(Some(&args(serde_json::json!({"limit": "100"}))), &schema),
            "failed to deserialize parameters: invalid type: string \"100\", expected u32",
        );
        assert!(
            msg.contains("parameter \"limit\" expects an integer, received a string."),
            "got: {msg}"
        );
    }

    /// An optional parameter's type lives behind an `anyOf` branch after nullable normalization.
    /// Reading only the top-level `type` would find nothing there and drop the sentence.
    #[test]
    fn a_nullable_parameter_is_judged_by_its_non_null_branch() {
        let schema = props(serde_json::json!({
            "namespace": {"anyOf": [{"type": "string"}, {"type": "null"}]}
        }));
        let found = type_mismatches(Some(&args(serde_json::json!({"namespace": 7}))), &schema);
        assert_eq!(
            found,
            vec![(
                "namespace".to_string(),
                "a string".to_string(),
                "a number".to_string()
            )]
        );
        // And an explicit null is still permitted — every optional parameter accepts one.
        assert!(
            type_mismatches(Some(&args(serde_json::json!({"namespace": null}))), &schema)
                .is_empty()
        );
    }

    #[test]
    fn a_number_satisfies_an_integer_only_when_it_is_whole() {
        let schema = props(serde_json::json!({"limit": {"type": "integer"}}));
        assert!(
            type_mismatches(Some(&args(serde_json::json!({"limit": 100}))), &schema).is_empty()
        );
        let found = type_mismatches(Some(&args(serde_json::json!({"limit": 1.5}))), &schema);
        assert_eq!(found.len(), 1, "a fraction is not an integer: {found:?}");
    }

    /// Nothing is claimed the schema cannot back up. An untyped property and a key the tool does not
    /// advertise both fall through to serde's text alone, which is worse than naming the parameter
    /// but better than naming the wrong one.
    #[test]
    fn an_unjudgeable_argument_leaves_the_message_to_serde() {
        let schema = props(serde_json::json!({"opaque": {"description": "no type here"}}));
        assert!(type_mismatches(Some(&args(serde_json::json!({"opaque": 1}))), &schema).is_empty());
        assert!(type_mismatches(Some(&args(serde_json::json!({"other": 1}))), &schema).is_empty());
        assert_eq!(
            invalid_params_message("t", &[], "failed to deserialize parameters: boom"),
            "t: failed to deserialize parameters: boom"
        );
    }

    /// The prefix match is the whole guard against normalizing a handler's own refusal. A result with
    /// structured content is a handler answer, whatever its text says.
    #[test]
    fn only_rmcp_s_own_deserialization_refusal_is_recognized() {
        use rmcp::model::CallToolResult;
        let serde_text = "failed to deserialize parameters: invalid type: integer 42";
        assert_eq!(
            deserialization_failure(&CallToolResult::error(vec![
                rmcp::model::ContentBlock::text(serde_text)
            ])),
            Some(serde_text.to_string())
        );
        assert_eq!(
            deserialization_failure(&CallToolResult::error(vec![
                rmcp::model::ContentBlock::text("IRIS_UNREACHABLE: no connection")
            ])),
            None
        );
        assert_eq!(
            deserialization_failure(&CallToolResult::structured_error(serde_json::json!({
                "success": false, "error_code": "INVALID_PARAMS", "error": serde_text
            }))),
            None
        );
    }
}

//! Global reference normalization for storage maps (spec 172 US2).

use super::tree::Node;
use super::Action;
use crate::policy::code_edit_gate::CODE_STORAGE_GLOBALS;
use crate::policy::data_policy_gate::check_phi_name_gate;
use crate::policy::effective::EffectivePolicy;
use crate::policy::patterns::matches_any;
use crate::policy::system_blocklist_gate::check_system_blocklist;
use crate::tools::global::canonical_global_name;
use serde::Serialize;
use serde_json::{json, Value};

/// A normalized global reference.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct GlobalRef {
    /// The global name with leading caret (e.g., "^X").
    pub name: String,
    /// Extended reference namespace or directory (e.g., "NS", "^^/db").
    pub extended: Option<String>,
    /// Subscript prefix if present (e.g., "(1,2)").
    pub subscripts_prefix: Option<String>,
}

/// Normalizes a global reference into its canonical form.
///
/// Accepts:
/// - `^X` or `X` → `{name: "^X", extended: None, subscripts_prefix: None}`
/// - `^X(...)` → with subscripts_prefix
/// - `^|"NS"|X` or `^["NS"]X` → with extended
/// - `^|"^^/db"|X` → extended reference to a directory
///
/// Rejects (returns `Err("dynamic")`):
/// - Anything with `@` (indirection)
/// - Anything with `$Name` (dynamic name construction)
pub fn normalize_global(raw: &str) -> Result<GlobalRef, String> {
    // Indirection anywhere, or `$NAME`/`$NA` in any case, makes the global unknowable here.
    if raw.contains('@') || raw.to_ascii_uppercase().contains("$NA") {
        return Err("dynamic".to_string());
    }

    let trimmed = raw.trim();
    let mut working = trimmed.strip_prefix('^').unwrap_or(trimmed);

    // Extended reference: `|"NS"|X` closes on `"|`, `["NS"]X` closes on `"]`.
    let closer = if working.starts_with('|') {
        Some("\"|")
    } else if working.starts_with('[') {
        Some("\"]")
    } else {
        None
    };
    let mut extended = None;
    if let Some(close) = closer {
        // The opening `|"` / `["` is two ASCII bytes, so slicing at 2 is on a char boundary.
        let rest = match working.get(1..2) {
            Some("\"") => &working[2..],
            _ => return Err(format!("unreadable extended reference: {trimmed}")),
        };
        let Some(close_pos) = rest.find(close) else {
            return Err(format!("unreadable extended reference: {trimmed}"));
        };
        extended = Some(rest[..close_pos].to_string());
        working = &rest[close_pos + close.len()..];
    }

    let (base, subscripts_prefix) = match working.find('(') {
        Some(paren_pos) => (
            &working[..paren_pos],
            Some(working[paren_pos..].to_string()),
        ),
        None => (working, None),
    };

    Ok(GlobalRef {
        name: format!("^{base}"),
        extended,
        subscripts_prefix,
    })
}

/// Tags whose value names a global (plan "Gates in the handler" step 1), compared without case.
/// `SqlIdExpression` holds code, so the globals it names are read token by token.
pub(super) const GLOBAL_TAGS: &[&str] = &[
    "Global",
    "IdLocation",
    "DataLocation",
    "IndexLocation",
    "StreamLocation",
    "CounterLocation",
];

/// Checks every global a map or storage keyword names against the global gates (FR-026).
///
/// Each name is normalised the way the shared gate does it ([`canonical_global_name`], so `^X`
/// and `X` are one name), then refused if it is a code-storage global (fixed rule), on the system
/// or the server's blocklist, or PHI-named without `acknowledgePhi`. An extended reference is
/// refused for every action except `read`, `diagram` and `uses`. A reference the dynamic check
/// owns (`@`, `$NAME`) is left to it; any other value that is not a global reference is refused.
///
/// Bodies carry `error_code` only; the dispatcher adds `refusal` (`classify_refusal`).
pub fn check_map_globals(
    nodes: &[Node],
    policy: &EffectivePolicy,
    server: &str,
    action: Action,
    acknowledge_phi: bool,
) -> Option<Value> {
    for (tag, raw) in named_globals(nodes) {
        if let Some(body) = check_global_ref(&tag, &raw, policy, server, action, acknowledge_phi) {
            return Some(body);
        }
    }
    None
}

/// One global the call names, through the rules of [`check_map_globals`]: the same body for a
/// name in a map and for a name the caller hands `propose` as an index global, so no route to a
/// global skips a gate.
fn check_global_ref(
    tag: &str,
    raw: &str,
    policy: &EffectivePolicy,
    server: &str,
    action: Action,
    acknowledge_phi: bool,
) -> Option<Value> {
    let reads_only = matches!(action, Action::Read | Action::Diagram | Action::Uses);
    let r = match normalize_global(raw) {
        Ok(r) => r,
        Err(e) if e == "dynamic" => return None,
        Err(e) => return Some(invalid(tag, raw, "global_reference", &e)),
    };
    if r.extended.is_some() && !reads_only {
        return Some(invalid(
            tag,
            raw,
            "extended_reference",
            "a reference to another namespace or database cannot be written or sampled",
        ));
    }
    let name = match canonical_global_name(&r.name) {
        Ok(n) => n,
        Err(body) => return Some(body),
    };
    let caret_name = format!("^{name}");
    if matches_any(&caret_name, CODE_STORAGE_GLOBALS) {
        return Some(json!({
            "success": false,
            "error_code": "CODE_EDIT_BLOCKED",
            "error": format!(
                "{tag} names {caret_name}, which holds class or routine code; a storage map \
                 cannot be laid over it"
            ),
            "global": caret_name,
            "tag": tag,
        }));
    }
    if let Some(body) = check_system_blocklist(
        &name,
        &policy.global_blocklist.value,
        &policy.kill_allowlist.value,
        false,
        server,
    ) {
        return Some(body);
    }
    check_phi_name_gate(&name, acknowledge_phi, server)
}

/// An index global the caller named for `propose` (`index_globals`), through the same gates as the
/// master: the system and the server's blocklists, the code-storage globals, and the PHI name
/// patterns unless `acknowledge_phi`. `None` lets it through; the body is the refusal.
pub fn check_index_global(
    name: &str,
    policy: &EffectivePolicy,
    server: &str,
    acknowledge_phi: bool,
) -> Option<Value> {
    check_global_ref(
        "index_globals",
        name,
        policy,
        server,
        Action::Propose,
        acknowledge_phi,
    )
}

fn invalid(tag: &str, raw: &str, rule: &str, why: &str) -> Value {
    json!({
        "success": false,
        "error_code": "STORAGE_MAP_INVALID",
        "rule": rule,
        "error": format!("{tag} `{raw}`: {why}"),
        "tag": tag,
    })
}

/// `(tag, reference)` for every global the tree names: the value of each [`GLOBAL_TAGS`] element
/// and each `^name` token in a `SqlIdExpression`, at any depth.
fn named_globals(nodes: &[Node]) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let mut stack: Vec<&Node> = nodes.iter().rev().collect();
    while let Some(node) = stack.pop() {
        if let Some(value) = node
            .value
            .as_deref()
            .map(str::trim)
            .filter(|v| !v.is_empty())
        {
            if GLOBAL_TAGS
                .iter()
                .any(|t| t.eq_ignore_ascii_case(&node.tag))
            {
                out.push((node.tag.clone(), value.to_string()));
                // A global named inside the subscripts (`^X(^Y)`) is checked too.
                if let Some(open) = value.find('(') {
                    for token in global_tokens(&value[open..]) {
                        out.push((node.tag.clone(), token));
                    }
                }
            } else if node.tag.eq_ignore_ascii_case("SqlIdExpression") {
                for token in global_tokens(value) {
                    out.push((node.tag.clone(), token));
                }
            }
        }
        if let Some(children) = &node.children {
            stack.extend(children.iter().rev());
        }
    }
    out
}

/// Every global reference in a piece of ObjectScript: `^name`, `^|"ns"|name` and `^["ns"]name`.
/// A caret not followed by a name (the `"^"` of a `$Piece` delimiter) is skipped. Over-reading
/// is the safe side: a string literal that happens to hold `^X` is checked too.
pub fn global_tokens(code: &str) -> Vec<String> {
    let is_name = |c: char| c.is_ascii_alphanumeric() || c == '%' || c == '.';
    let bytes = code.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] != b'^' {
            i += 1;
            continue;
        }
        let start = i;
        let mut j = i + 1;
        // An extended prefix runs to its closer: `|` for `^|"ns"|`, `]` for `^["ns"]`.
        if let Some(&open) = bytes.get(j).filter(|b| **b == b'|' || **b == b'[') {
            let close = if open == b'|' { b'|' } else { b']' };
            match bytes[j + 1..].iter().position(|b| *b == close) {
                Some(k) => j += k + 2,
                None => {
                    out.push(code[start..].to_string());
                    break;
                }
            }
        }
        let name_start = j;
        while let Some(c) = code[j..].chars().next().filter(|c| is_name(*c)) {
            j += c.len_utf8();
        }
        if j > name_start {
            out.push(code[start..j].to_string());
        }
        i = j.max(i + 1);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn basic_forms() {
        let r = normalize_global("^X").unwrap();
        assert_eq!(r.name, "^X");
        assert!(r.extended.is_none());
        assert!(r.subscripts_prefix.is_none());

        let r = normalize_global("X").unwrap();
        assert_eq!(r.name, "^X");
    }

    #[test]
    fn with_subscripts() {
        let r = normalize_global("^X(1,2)").unwrap();
        assert_eq!(r.name, "^X");
        assert_eq!(r.subscripts_prefix, Some("(1,2)".to_string()));
    }

    #[test]
    fn extended_refs() {
        let r = normalize_global("^|\"NS\"|X").unwrap();
        assert_eq!(r.name, "^X");
        assert_eq!(r.extended, Some("NS".to_string()));

        let r = normalize_global("^[\"NS\"]X").unwrap();
        assert_eq!(r.name, "^X");
        assert_eq!(r.extended, Some("NS".to_string()));
    }

    #[test]
    fn dynamic_rejected() {
        assert!(normalize_global("^@X").is_err());
        assert!(normalize_global("^$Name(x)").is_err());
        assert!(normalize_global("$NAME(^X)").is_err());
        assert!(normalize_global("$na(^X)").is_err());
    }

    #[test]
    fn malformed_extended_is_an_error_not_a_panic() {
        for raw in ["^|\"|X", "^|", "^[", "^|é|X", "^[\"NS", "^|\"NS"] {
            assert!(normalize_global(raw).is_err(), "{raw}");
        }
        let r = normalize_global("^|\"^^/db/\"|X(1)").unwrap();
        assert_eq!(r.extended.as_deref(), Some("^^/db/"));
        assert_eq!(r.name, "^X");
    }
}

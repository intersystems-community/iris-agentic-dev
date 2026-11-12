//! Code extraction and validation for storage maps (spec 172 Phase 5 US8).
//!
//! Map code is agent-supplied ObjectScript that runs at query and filing time (research R6).

use super::tree::Node;
use super::CallShape;
use crate::policy::code_edit_gate;
use crate::policy::effective::EffectivePolicy;
use serde_json::{json, Value};

/// Tags whose values are ObjectScript code (research R6).
///
/// Every element value is treated as code unless its tag is in `LITERAL_TAGS`.
pub const CODE_TAGS: &[&str] = &[
    "Expression",       // Subscript Expression
    "DataAccess",       // Subscript DataAccess
    "NextCode",         // Subscript NextCode
    "StopExpression",   // Subscript StopExpression
    "Code",             // AccessVar Code
    "RetrievalCode",    // Data RetrievalCode
    "RowReference",     // SQLMap RowReference
    "Condition",        // SQLMap Condition
    "Global",           // Extended references are code
    "Invalidcondition", // Subscript Invalidcondition
];

/// Tags whose values are literals, not code.
///
/// Everything else goes through the code gate (fail closed).
pub const LITERAL_TAGS: &[&str] = &[
    "Type",
    "Delimiter",
    "Piece",
    "Node",
    "PopulationType",
    "PopulationPct",
    "BlockCount",
    "Structure",
    "Deferred",
    "ConditionalWithHostVars",
    "Field",
    "AccessType",
];

/// Extracts all code-bearing elements from a storage map tree.
///
/// Returns `(path, code_value)` pairs. An element is code unless its tag is in `LITERAL_TAGS`
/// (fail closed: unknown tags are treated as code). Per FR-027, each value goes through the
/// same code-edit check as `iris_execute`.
pub fn code_elements(nodes: &[Node]) -> Vec<(String, String)> {
    let mut result = Vec::new();
    walk_for_code(nodes, "", &mut result);
    result
}

fn walk_for_code(nodes: &[Node], path_prefix: &str, result: &mut Vec<(String, String)>) {
    for node in nodes {
        let node_path = format!("{path_prefix}/{}", node.tag);
        if let Some(value) = &node.value {
            if !LITERAL_TAGS.contains(&node.tag.as_str()) && !value.trim().is_empty() {
                result.push((node_path.clone(), value.clone()));
            }
        }
        if let Some(children) = &node.children {
            walk_for_code(children, &node_path, result);
        }
    }
}

/// Checks all code-bearing elements in a storage map tree against the code edit gate.
///
/// Per FR-027, map code is treated like class code written through `iris_doc`. Every code-bearing
/// element must pass the same lexical check as `iris_execute` code.
///
/// Returns `Some(error_json)` on the first refusal, `None` if all code passes.
pub fn check_map_code(nodes: &[Node], server: &str) -> Option<Value> {
    code_elements(nodes)
        .into_iter()
        .find_map(|(_, code)| code_edit_gate::check_objectscript_code_edit(&code, server))
}

/// Finds dynamic global references in a storage map (FR-028).
///
/// Dynamic globals are those whose name is only known at run time:
/// - `AccessType Global` (the global name is built when the query runs)
/// - `@` indirection, or `$NAME` / `$NA` (any case), in `DataAccess`, `RowReference`,
///   `SqlIdExpression` or any tag that names a global (`Global` and the storage locations), so
///   every value the global gates skip as dynamic is answered here
///
/// Tags are compared without case. Returns one `"<tag>: <value>"` entry per finding.
pub fn dynamic_globals(nodes: &[Node]) -> Vec<String> {
    let mut out = Vec::new();
    let mut stack: Vec<&Node> = nodes.iter().rev().collect();
    while let Some(node) = stack.pop() {
        if let Some(value) = node.value.as_deref() {
            let tag = node.tag.as_str();
            let dynamic = if tag.eq_ignore_ascii_case("AccessType") {
                value.trim().eq_ignore_ascii_case("global")
            } else if ["DataAccess", "RowReference", "SqlIdExpression"]
                .iter()
                .chain(super::globals::GLOBAL_TAGS)
                .any(|t| t.eq_ignore_ascii_case(tag))
            {
                value.contains('@') || value.to_ascii_uppercase().contains("$NA")
            } else {
                false
            };
            if dynamic {
                out.push(format!("{tag}: {value}"));
            }
        }
        if let Some(children) = &node.children {
            stack.extend(children.iter().rev());
        }
    }
    out
}

/// Returns true if any code-bearing element changed between the two trees.
///
/// Per FR-027, a code change raises the call to Execute category. This compares the code elements
/// extracted from `before` and `after`; a change in a literal (Piece, Delimiter, etc.) returns false.
pub fn code_changed(before: &[Node], after: &[Node]) -> bool {
    code_elements(before) != code_elements(after)
}

/// Runs the storage-map gates in plan order (FR-026, FR-027, FR-028) on a tree a call would
/// write, check or sample.
///
/// 1. [`check_map_globals`](super::globals::check_map_globals): code-storage globals, the system
///    and server blocklists, PHI names, extended references.
/// 2. [`check_map_code`]: the code-edit gate on every code-bearing element.
/// 3. [`dynamic_globals`]: refused unless the server's effective policy allows them.
///
/// The action and `acknowledgePhi` come from `shape` (the one reader, FR-030a). Bodies carry
/// `error_code` only; the dispatcher adds `refusal`.
pub fn gate_tree(
    nodes: &[Node],
    shape: &CallShape,
    policy: &EffectivePolicy,
    server: &str,
) -> Option<Value> {
    if let Some(body) = super::globals::check_map_globals(
        nodes,
        policy,
        server,
        shape.action,
        shape.acknowledge_phi,
    ) {
        return Some(body);
    }
    if let Some(body) = check_map_code(nodes, server) {
        return Some(body);
    }
    let found = dynamic_globals(nodes);
    if !found.is_empty() && !allows_dynamic_globals(policy) {
        return Some(json!({
            "success": false,
            "error_code": "STORAGE_MAP_DYNAMIC_GLOBAL_BLOCKED",
            "error": format!(
                "the map names a global only known at run time ({}), so iad cannot check it \
                 against the global gates",
                found[0]
            ),
            "dynamic_references": found,
        }));
    }
    None
}

/// Whether the server's effective policy allows dynamic map globals (T057; a loosening setting,
/// so an untrusted file cannot turn it on).
fn allows_dynamic_globals(policy: &EffectivePolicy) -> bool {
    policy.allow_dynamic_map_globals.value
}

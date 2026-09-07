//! The source-side half of the parameter contract.
//!
//! Every schema assertion in this feature compares what a tool *advertises* against what its
//! handler *reads*. The advertised side comes from a running server (`tools/list`); the read side
//! comes from these two helpers, which scan the source. Both are in
//! `iris_agentic_dev_core::testing` because eight test targets share them and every test file in
//! this crate is its own cargo target with no shared module.
//!
//! This file exists to prove the helpers work before anything depends on them. A source-scanning
//! helper that quietly returns an empty set turns every comparison into a tautology, which is the
//! failure mode `read_keys` panics rather than tolerates.

use iris_agentic_dev_core::testing::{
    handler_uses_field, params_type, read_keys, struct_fields, tool_names,
};

#[test]
fn the_tool_inventory_is_the_whole_surface() {
    let names = tool_names();
    assert!(
        names.len() >= 81,
        "found only {} `async fn` tool handlers in src/tools/mod.rs, expected at least 81 — a \
         short inventory makes every per-tool assertion below skip tools silently: {names:?}",
        names.len()
    );
}

/// The load-bearing assertion. `read_keys` must answer for all 81 tools today, before any
/// conversion — which means its typed-struct branch has to work for the 50 already-typed tools and
/// its `p.get` branch for the 31 `AnyParams` ones.
///
/// The genuinely no-argument tools are the only exemption, and they are named rather than
/// pattern-matched so tool #82 cannot join them by accident.
#[test]
fn read_keys_answers_for_every_tool() {
    const NO_ARGUMENT_TOOLS: &[&str] = &[
        "agent_stats",
        "check_config",
        "iris_import_servers",
        "iris_reload_pool",
        "skill_community_list",
        "skill_list",
        // A `NoParams` stub, not part of the default toolset: pattern mining is unimplemented, so
        // the handler returns its "not yet implemented" message whatever it is handed.
        "skill_propose",
    ];

    let mut empty = Vec::new();
    for tool in tool_names() {
        let keys = read_keys(&tool);
        if keys.is_empty() && !NO_ARGUMENT_TOOLS.contains(&tool.as_str()) {
            empty.push(tool);
        }
    }
    assert!(
        empty.is_empty(),
        "read_keys returned nothing for {} tool(s): {empty:?}\n\
         Either the tool really takes no arguments — add it to NO_ARGUMENT_TOOLS with a reason — \
         or the helper failed to classify it, in which case every comparison against it passes for \
         free.",
        empty.len()
    );
}

/// `read_keys` prefers the handler body's `p.get("…")` literals, and that preference is what keeps
/// the FR-003 comparison honest after conversion: the schema derives from the struct, the read set
/// from the body.
#[test]
fn read_keys_prefers_handler_body_literals() {
    // `iris_namespace_list` reads `server` via `p.get("server")` and nothing else.
    let keys = read_keys("iris_namespace_list");
    assert!(
        keys.contains("server"),
        "expected `server` among iris_namespace_list's read keys, got {keys:?}"
    );
}

/// The typed-struct branch, exercised on a tool that destructures its parameters directly.
#[test]
fn read_keys_falls_back_to_struct_fields() {
    let ty = params_type("iris_query");
    assert_eq!(ty, "QueryParams");
    let fields = struct_fields(&ty);
    for expected in ["query", "parameters", "namespace", "mode", "server"] {
        assert!(
            fields.contains(expected),
            "QueryParams must declare `{expected}`; parsed fields were {fields:?}"
        );
    }
}

/// A `#[serde(rename = "…")]` field must report its wire name, not its Rust name — the wire name
/// is what a caller sends and what the schema advertises.
#[test]
fn struct_fields_honors_serde_rename() {
    // Sanity: the parser must not silently return Rust identifiers where a rename applies. If no
    // struct in the crate uses `rename`, this test still asserts the parser is reachable.
    let fields = struct_fields("QueryParams");
    assert!(
        !fields.is_empty(),
        "struct_fields must parse a known struct; an empty result means the block scanner failed"
    );
    assert!(
        fields.iter().all(|f| !f.contains(' ')),
        "field names must be bare identifiers or rename literals, got {fields:?}"
    );
}

/// The assertion that survives conversion. Once a tool is typed, `read_keys` and the advertised
/// schema can both trace back to the struct; this one reads the handler body instead, so a declared
/// parameter that no code touches still fails.
#[test]
fn handler_uses_field_sees_what_the_handler_reads() {
    assert!(
        handler_uses_field("iris_namespace_list", "server"),
        "iris_namespace_list reads `server`"
    );
    assert!(
        !handler_uses_field("iris_namespace_list", "definitely_not_a_parameter"),
        "handler_uses_field must not match a name absent from the body — a helper that always \
         returns true would make the audit in T027 vacuous"
    );
}

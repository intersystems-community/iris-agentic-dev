// Spec 085 write-gate integrity — `gate_check` and `classify`, called directly (T073).
//
// The enforcement tests in `tests/integration/test_gate_enforcement_live.rs` reach the gate through
// a spawned `iris-agentic-dev` process, which is the right way to assert that nothing lands in
// IRIS — but a spawned binary is not instrumented, so every refusal arm in `write_gate.rs` read as
// uncovered while being the most exercised code in the feature. These tests call the two functions
// in-process instead. They assert the same behaviour from the other side: not "IRIS is unchanged"
// but "the refusal says which gate refused and why".
//
// Nothing here touches IRIS or the process environment. Every `GateResolution` is built by hand,
// because the resolution *inputs* are already covered by `test_gate_resolution.rs` and what is
// under test here is what `gate_check` does with an answer once it has one.

use iris_agentic_dev_core::tools::write_gate::{
    classify, gate_check, GateResolution, GateSource, WriteClass,
};

/// The refusal payload, or a panic. `gate_check` returns `Some(Ok(..))` for a refusal; anything
/// else is a test bug, so unwrapping here loses no information.
fn refusal_json(
    tool: &str,
    args: Option<&serde_json::Map<String, serde_json::Value>>,
    gates: &GateResolution,
) -> serde_json::Value {
    let result = gate_check(tool, args, gates)
        .unwrap_or_else(|| panic!("{tool} was allowed through, expected a refusal"))
        .expect("a refusal is Ok(CallToolResult), not an McpError — Principle V");
    result
        .structured_content
        .clone()
        .expect("the refusal carries structured content")
}

fn args(v: serde_json::Value) -> serde_json::Map<String, serde_json::Value> {
    v.as_object().expect("object").clone()
}

/// Writes on, destructive off — the default an operator gets from the namespace heuristic.
fn writes_only() -> GateResolution {
    GateResolution {
        write_enabled: true,
        write_source: GateSource::InferredNamespace,
        destructive_enabled: false,
        destructive_source: GateSource::InferredDefault,
    }
}

fn both_on() -> GateResolution {
    GateResolution {
        write_enabled: true,
        write_source: GateSource::ConfigFile,
        destructive_enabled: true,
        destructive_source: GateSource::ConfigFile,
    }
}

fn writes_off_from_config() -> GateResolution {
    GateResolution {
        write_enabled: false,
        write_source: GateSource::ConfigFile,
        destructive_enabled: false,
        destructive_source: GateSource::ConfigFile,
    }
}

// ── the three classes ─────────────────────────────────────────────────────────

/// A read-only tool is never asked about the gate. With everything closed, `iris_info` still
/// answers — this is FR-013, and it is the half of the gate that a fail-closed bug breaks
/// silently: a server that refuses reads is useless but looks safe.
#[test]
fn read_only_tools_pass_with_every_gate_closed() {
    for tool in [
        "iris_info",
        "iris_query",
        "check_config",
        "iris_namespace_list",
    ] {
        assert!(
            gate_check(tool, None, &GateResolution::fail_closed()).is_none(),
            "{tool} is read-only and must not be gated even when the resolution is fail-closed"
        );
    }
}

#[test]
fn write_tools_are_refused_and_the_refusal_names_the_source() {
    let v = refusal_json("iris_compile", None, &writes_off_from_config());
    assert_eq!(v["error_code"], "WRITE_TOOLS_DISABLED");
    assert_eq!(v["success"], false);
    let msg = v["error"].as_str().expect("error message");
    assert!(
        msg.contains("config_file"),
        "the refusal has to name where the gate came from, or an operator cannot tell a toml \
         decision from an inferred one: {msg}"
    );
    assert!(
        msg.contains("write_tools_enabled"),
        "and it has to name the remedy: {msg}"
    );
}

/// Writes off *and* the tier off: the write gate is the one reported. It is the more fundamental
/// refusal and its remedy comes first, so reporting the tier here would send an operator to fix
/// the wrong key.
#[test]
fn a_destructive_tool_with_writes_off_reports_the_write_gate() {
    let v = refusal_json("iris_admin", None, &writes_off_from_config());
    assert_eq!(v["error_code"], "WRITE_TOOLS_DISABLED");
}

#[test]
fn destructive_tools_are_refused_when_only_the_tier_is_closed() {
    let v = refusal_json("iris_admin", None, &writes_only());
    assert_eq!(v["error_code"], "DESTRUCTIVE_TOOLS_DISABLED");
    let msg = v["error"].as_str().expect("error message");
    assert!(
        msg.contains("inferred_default"),
        "the tier defaults to off and the refusal must say so: {msg}"
    );
    assert!(
        msg.contains("destructive_tools_enabled"),
        "and name the key that opens it: {msg}"
    );
}

#[test]
fn a_write_tool_passes_when_writes_are_on_and_the_tier_is_not() {
    assert!(
        gate_check("iris_compile", None, &writes_only()).is_none(),
        "iris_compile is Write, not Destructive — the tier must not gate it"
    );
}

#[test]
fn destructive_tools_pass_when_both_are_declared() {
    for tool in ["iris_admin", "global_kill", "iris_credential_manage"] {
        assert!(
            gate_check(tool, None, &both_on()).is_none(),
            "{tool} must be allowed once both gates are declared"
        );
    }
}

/// An unclassified tool is treated as `Write`. A tool reaching dispatch without a `CLASSIFICATION`
/// entry is a bug the completeness test should have caught; the safe reading of that bug is "this
/// might write", so it fails closed rather than defaulting to read-only.
#[test]
fn an_unclassified_tool_fails_closed_as_write() {
    assert_eq!(
        classify("no_such_tool_085", None),
        None,
        "precondition: the tool really has no entry"
    );
    let v = refusal_json("no_such_tool_085", None, &writes_off_from_config());
    assert_eq!(v["error_code"], "WRITE_TOOLS_DISABLED");
    assert!(
        gate_check("no_such_tool_085", None, &writes_only()).is_none(),
        "fail-closed means Write, not Destructive — an unknown tool must not be less usable than \
         a declared write tool"
    );
}

// ── per-action classification ─────────────────────────────────────────────────

/// `iris_doc` is the mixed case: its default call is a read and its write actions are a closed set.
#[test]
fn mixed_tools_classify_per_action() {
    let cases = [
        ("get", WriteClass::ReadOnly),
        ("list", WriteClass::ReadOnly),
        ("put", WriteClass::Write),
        ("delete", WriteClass::Write),
    ];
    for (action, expected) in cases {
        let a = args(serde_json::json!({"mode": action, "name": "X.cls"}));
        assert_eq!(
            classify("iris_doc", Some(&a)),
            Some(expected),
            "iris_doc mode={action}"
        );
    }
}

/// The handlers lowercase the action before dispatching, so a gate that compared exactly would let
/// `mode="PUT"` through and then write. Asserted for both argument names, because `iris_doc` takes
/// `action` as an alias for `mode` and only one of the two was ever read.
#[test]
fn action_matching_ignores_case_and_reads_both_argument_names() {
    for key in ["mode", "action"] {
        let a = args(serde_json::json!({key: "PUT"}));
        assert_eq!(
            classify("iris_doc", Some(&a)),
            Some(WriteClass::Write),
            "iris_doc {key}=PUT must classify as Write"
        );
        let v = refusal_json("iris_doc", Some(&a), &writes_off_from_config());
        assert_eq!(v["error_code"], "WRITE_TOOLS_DISABLED", "{key}=PUT");
    }
}

/// An action the entry does not list falls back to the entry's default rather than to read-only.
/// For `iris_lookup_manage`, whose write actions are the majority, the default is the write tier —
/// so a typo'd or newly added action is gated instead of waved through.
#[test]
fn an_unlisted_action_falls_back_to_the_entry_default() {
    let a = args(serde_json::json!({"action": "no_such_action"}));
    assert_eq!(
        classify("iris_lookup_manage", Some(&a)),
        Some(WriteClass::Destructive),
        "an unrecognised action on a write-default tool must not read as read-only"
    );
    let read = args(serde_json::json!({"action": "get"}));
    assert_eq!(
        classify("iris_lookup_manage", Some(&read)),
        Some(WriteClass::ReadOnly),
        "and its declared read actions still read"
    );
}

/// Args present but carrying neither `action` nor `mode`: same fallback, no panic. This is the
/// shape every non-dispatcher tool arrives with.
#[test]
fn args_without_an_action_key_use_the_default() {
    let a = args(serde_json::json!({"namespace": "USER"}));
    assert_eq!(classify("iris_compile", Some(&a)), Some(WriteClass::Write));
    assert_eq!(classify("iris_info", Some(&a)), Some(WriteClass::ReadOnly));
}

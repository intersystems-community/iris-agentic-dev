//! Enum contract: the values a tool advertises are the values its code accepts.
//!
//! FR-006 asks for enums on the parameters with fixed value sets. The trap is where the value list
//! comes from. Copying it out of the tool description gives a schema that agrees with the prose and
//! nothing else — and the prose is what this feature exists to stop relying on. So every assertion
//! here compares the advertised `enum` against the string literals the code actually branches on,
//! extracted from source by `handler_match_arms`.
//!
//! Two directions, both load-bearing:
//!
//! * Advertised ⊇ branched — a value the code accepts but the schema omits is a working call a
//!   client will refuse to make.
//! * Advertised ⊆ branched ∪ `complements` — a value the schema offers that no branch accepts is a
//!   call that fails with INVALID_ACTION. Extras must be listed in the table below with a reason, so
//!   "it's in the docs" cannot quietly become the justification again.

use iris_agentic_dev_core::testing::{
    advertised_schemas, advertised_tools_in_toolset, handler_branch_has_default,
    handler_match_arms, require_iad_binary, resolve_property, rust_sources,
};

/// Why a declared value has no branch of its own. Every entry is a claim about the code, and the
/// tests below check the claim rather than trusting it.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Why {
    /// Reached through the match's `_ =>` arm, so it never appears as a literal.
    DefaultArm,
    /// The other half of an inequality: the code tests `!= "yes"` and treats everything else as a
    /// decline, so `"no"` is the value a client should send and the code never names it.
    DeclineHalf,
    /// Never compared anywhere. Only one parameter qualifies and it carries its own assertion.
    NeverCompared,
    /// A filter, not a dispatcher: the value is compared case-insensitively against a string the
    /// code derives from IRIS, so no literal in the call chain names it. `iris_admin.type` is the
    /// only one, and it carries its own assertion.
    DerivedFilter,
}

impl Why {
    /// Whether this reason means the parameter has no branch site at all, as opposed to one branch
    /// site that happens not to name this value.
    fn is_unbranched(self) -> bool {
        matches!(self, Why::NeverCompared | Why::DerivedFilter)
    }
}

/// One row: the tool, the parameter, and the declared values that have no branch literal behind
/// them, each with the reason.
type ContractRow = (&'static str, &'static str, &'static [(&'static str, Why)]);

/// The twenty-nine parameters with a fixed value set, and the values that are declared without a
/// branch literal behind them.
///
/// The first seventeen came with 113. The twelve after them are 114's: the dispatchers whose value
/// sets lived only in prose until the repaired `prose-only-enum` detector could see them.
const CONTRACT: &[ContractRow] = &[
    ("iris_system_performance", "mode", &[]),
    ("iris_interop_query", "what", &[]),
    ("iris_containers", "action", &[]),
    ("iris_production_item", "action", &[]),
    ("iris_business_rule_info", "action", &[]),
    ("iris_credential_manage", "action", &[]),
    ("iris_lookup_manage", "action", &[]),
    ("iris_lookup_transfer", "action", &[]),
    ("iris_admin", "action", &[]),
    ("iris_query", "mode", &[("read", Why::DefaultArm)]),
    ("iris_coverage", "mode", &[]),
    ("iris_doc", "category", &[]),
    ("iris_doc", "compiled_type", &[]),
    (
        "iris_doc",
        "elicitation_answer",
        &[("no", Why::DeclineHalf)],
    ),
    ("iris_test", "test_type", &[]),
    ("iris_generate", "gen_type", &[("class", Why::DefaultArm)]),
    (
        "iris_add_server",
        "scheme",
        &[("http", Why::NeverCompared), ("https", Why::NeverCompared)],
    ),
    // 114: the dispatchers the detector could not read.
    ("iris_doc", "mode", &[]),
    ("iris_info", "what", &[]),
    ("iris_macro", "action", &[]),
    ("iris_debug", "action", &[]),
    ("skill", "action", &[]),
    ("skill_community", "action", &[]),
    ("kb", "action", &[]),
    ("agent_info", "what", &[]),
    ("iris_source_control", "action", &[]),
    ("iris_global", "action", &[]),
    ("iris_production", "action", &[]),
    (
        "iris_admin",
        "type",
        &[("REST", Why::DerivedFilter), ("CSP", Why::DerivedFilter)],
    ),
];

/// `handler_match_arms` for `tool.param`, sorted, as owned strings for a readable assertion.
fn arms(tool: &str, param: &str) -> Vec<String> {
    handler_match_arms(tool, param).into_iter().collect()
}

fn expected(values: &[&str]) -> Vec<String> {
    let mut v: Vec<String> = values.iter().map(|s| s.to_string()).collect();
    v.sort_unstable();
    v
}

// ── the extractor's own canaries ─────────────────────────────────────────────
//
// Every assertion in this file is only as good as the set `handler_match_arms` returns, and the
// eleven dispatchers 114 declares found three ways for it to answer confidently and wrongly. A
// wrong-but-plausible set is worse than an empty one: empty fails the census test above, while a
// partial set silently turns the enum comparison into "the schema offers values nothing branches
// on" and invites the fix of deleting the declaration.

/// `iris_macro` writes four of its five actions as `action @ ("signature" | "location" | …) =>`.
/// The arm pattern required the literal to start the line, so the binding hid four values and the
/// extractor reported a one-value dispatcher.
#[test]
fn an_or_pattern_behind_a_binding_is_still_arms() {
    assert_eq!(
        arms("iris_macro", "action"),
        expected(&["list", "signature", "location", "definition", "expand"])
    );
}

/// `iris_info` matches `p.doc_type` inside its `what == "documents"` arm. Arms were collected from
/// the whole block text, so the inner match's `"ALL"` came back as a value of `what` — a value no
/// client should send, in a set that otherwise looks right.
#[test]
fn a_nested_match_does_not_donate_its_arms() {
    assert_eq!(
        arms("iris_info", "what"),
        expected(&[
            "documents",
            "modified",
            "namespace",
            "metadata",
            "jobs",
            "csp_apps",
            "csp_debug",
            "sa_schema",
        ])
    );
}

/// `iris_global` and `iris_source_control` both hand a `params_json` naming `action` to
/// `dispatch_gate` before dispatching. The gate compares that action against `"kill"` to decide
/// whether a kill allowlist applies, so the walk stopped there and reported the gate's opinion of
/// the parameter as the tool's value set.
#[test]
fn the_policy_gate_is_not_the_dispatcher() {
    assert_eq!(
        arms("iris_global", "action"),
        expected(&["get", "set", "kill", "list"])
    );
    assert_eq!(
        arms("iris_source_control", "action"),
        expected(&["status", "menu", "checkout", "execute"])
    );
}

/// The extractor has to answer for twenty-seven of the twenty-nine, or every comparison below is a
/// comparison against the empty set — which passes for free in one direction and fails
/// uninformatively in the other.
#[test]
fn the_source_scan_finds_a_branch_site_for_every_parameter_that_has_one() {
    let mut silent: Vec<String> = Vec::new();
    for (tool, param, complements) in CONTRACT {
        if !complements.is_empty() && complements.iter().all(|(_, w)| w.is_unbranched()) {
            continue;
        }
        if handler_match_arms(tool, param).is_empty() {
            silent.push(format!("{tool}.{param}"));
        }
    }
    assert!(
        silent.is_empty(),
        "handler_match_arms found no branch literals for {}: {silent:?}\n\
         Either the branch moved out of the call chain the walk follows, or the parameter stopped \
         being a closed set. Both make the enum assertion vacuous.",
        silent.len()
    );
}

/// `iris_add_server.scheme` is the one declared enum with no comparison behind it: the value is
/// stored and interpolated into a base URL. The closed set is the URL scheme itself, so the check is
/// that both declared values appear as schemes the connection layer writes.
#[test]
fn the_one_parameter_with_no_branch_site_is_pinned_another_way() {
    assert!(
        handler_match_arms("iris_add_server", "scheme").is_empty(),
        "`scheme` now has a branch site — move it out of the NeverCompared exemption and let the \
         source scan drive its values"
    );
    let src = rust_sources();
    for value in ["http", "https"] {
        assert!(
            src.contains(&format!(r#"scheme: Some("{value}".to_string())"#)),
            "`{value}` is advertised for iris_add_server.scheme but no server entry is written with \
             it; the exemption claims the closed set comes from the connection layer"
        );
    }
}

/// `iris_admin.type` filters the webapp list. The value it is compared against is not a literal in
/// the source: it is derived from IRIS's `Type` column, `1` meaning REST and `0` meaning CSP, and the
/// caller's filter is matched against that string case-insensitively. So the closed set is the pair
/// of names the derivation can produce, and that derivation is what this pins.
#[test]
fn the_derived_filter_is_pinned_to_the_derivation() {
    assert!(
        handler_match_arms("iris_admin", "type").is_empty(),
        "`type` now has a branch site — move it out of the DerivedFilter exemption and let the \
         source scan drive its values"
    );
    let src = rust_sources();
    for (column, name) in [("Some(1)", "REST"), ("Some(0)", "CSP")] {
        assert!(
            src.contains(&format!(r#"{column} => "{name}""#)),
            "`{name}` is advertised for iris_admin.type but nothing derives it from the webapp \
             `Type` column; the exemption claims the closed set comes from that derivation"
        );
    }
    assert!(
        src.contains("!type_val.eq_ignore_ascii_case(filter)"),
        "the `type` filter is no longer a case-insensitive comparison against the derived name, so \
         the DerivedFilter reason no longer describes the code"
    );
}

/// A value justified as "the default arm" must have a default arm behind it.
#[test]
fn every_default_arm_claim_has_a_default_arm() {
    for (tool, param, complements) in CONTRACT {
        for (value, why) in *complements {
            if *why != Why::DefaultArm {
                continue;
            }
            assert!(
                handler_branch_has_default(tool, param),
                "`{tool}.{param}` declares `{value}` on the grounds that it is the match's catch-all, \
                 but the match on {param} has no `_ =>` arm"
            );
        }
    }
}

#[test]
#[ignore = "spawns the built binary; run with --include-ignored"]
fn the_declared_enum_is_the_set_the_code_accepts() {
    let Some(_bin) = require_iad_binary() else {
        return;
    };
    // The default tier is Merged, which prunes `agent_info` among others. Reading only the default
    // listing is how those tools kept an open parameter set through 113, so read both and take the
    // union: a declared enum has to be there in whichever tier advertises the tool.
    let mut schemas = advertised_tools_in_toolset(Some("baseline"))
        .into_iter()
        .filter_map(|(name, tool)| tool.get("inputSchema").cloned().map(|s| (name, s)))
        .collect::<std::collections::BTreeMap<String, serde_json::Value>>();
    schemas.extend(advertised_schemas());
    let mut problems: Vec<String> = Vec::new();

    for (tool, param, complements) in CONTRACT {
        let schema = schemas
            .get(*tool)
            .unwrap_or_else(|| panic!("`{tool}` must appear in tools/list"));
        let prop = resolve_property(schema, param);
        let Some(declared) = prop.get("enum").and_then(|e| e.as_array()) else {
            problems.push(format!(
                "{tool}.{param}: no `enum` in the advertised schema ({prop})"
            ));
            continue;
        };
        assert!(
            !declared.is_empty(),
            "{tool}.{param} advertises an empty enum, which no value can satisfy"
        );
        let declared: std::collections::BTreeSet<String> = declared
            .iter()
            .map(|v| {
                v.as_str()
                    .unwrap_or_else(|| {
                        panic!("{tool}.{param}: enum values must be strings, got {v}")
                    })
                    .to_string()
            })
            .collect();

        let branched = handler_match_arms(tool, param);
        let allowed_extra: std::collections::BTreeSet<String> =
            complements.iter().map(|(v, _)| v.to_string()).collect();

        let missing: Vec<&String> = branched.difference(&declared).collect();
        if !missing.is_empty() {
            problems.push(format!(
                "{tool}.{param}: the code accepts {missing:?} but the schema does not offer them — \
                 a client following the schema cannot make a call that works"
            ));
        }
        let extra: Vec<String> = declared
            .difference(&branched)
            .filter(|v| !allowed_extra.contains(*v))
            .cloned()
            .collect();
        if !extra.is_empty() {
            problems.push(format!(
                "{tool}.{param}: the schema offers {extra:?}, which nothing in the call chain \
                 branches on. Either the value is dead and must go, or it belongs in CONTRACT's \
                 complement list with a reason"
            ));
        }
    }

    assert!(
        problems.is_empty(),
        "{} enum disagreement(s) between the schema and the code:\n  {}",
        problems.len(),
        problems.join("\n  ")
    );
}

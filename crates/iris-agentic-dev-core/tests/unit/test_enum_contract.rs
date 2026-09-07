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
    advertised_schemas, handler_branch_has_default, handler_match_arms, require_iad_binary,
    resolve_property, rust_sources,
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
}

/// One row: the tool, the parameter, and the declared values that have no branch literal behind
/// them, each with the reason.
type ContractRow = (&'static str, &'static str, &'static [(&'static str, Why)]);

/// The seventeen parameters with a fixed value set, and the values that are declared without a
/// branch literal behind them.
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
];

/// The extractor has to answer for sixteen of the seventeen, or every comparison below is a
/// comparison against the empty set — which passes for free in one direction and fails
/// uninformatively in the other.
#[test]
fn the_source_scan_finds_a_branch_site_for_every_parameter_that_has_one() {
    let mut silent: Vec<String> = Vec::new();
    for (tool, param, complements) in CONTRACT {
        if complements.iter().all(|(_, w)| *w == Why::NeverCompared) && !complements.is_empty() {
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
    let schemas = advertised_schemas();
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

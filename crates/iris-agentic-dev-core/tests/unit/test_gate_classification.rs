// Spec 085 write-gate integrity — the classification table and the tool surface it has to cover.
//
// These are unit tests on purpose: they read the router and the `CLASSIFICATION` table, neither of
// which touches IRIS. Enforcement itself is asserted against a live container in
// `tests/integration/test_gate_enforcement_live.rs`.

use iris_agentic_dev_core::iris::connection::{DiscoverySource, IrisConnection};
use iris_agentic_dev_core::tools::write_gate::{DeclaredGates, WriteClass, CLASSIFICATION};
use iris_agentic_dev_core::tools::{IrisTools, Toolset};

/// A connection object, not a connection. Nothing here dials IRIS — but the constructor's
/// tool-pruning only ran on the `Some(connection)` branch, so passing `None` would make every
/// assertion below pass for free. That is the whole reason this fixture exists.
fn offline_conn() -> IrisConnection {
    IrisConnection::new(
        "http://localhost:52780",
        "USER",
        "_SYSTEM",
        "SYS",
        DiscoverySource::ExplicitFlag,
    )
}

fn tools_with_declared_skills(
    toolset: Toolset,
    no_skills: bool,
    declared: DeclaredGates,
) -> IrisTools {
    IrisTools::with_registry_and_toolset(
        Some(offline_conn()),
        iris_agentic_dev_core::skills::SkillRegistry::new(),
        toolset,
        None,
        None,
        no_skills,
        declared,
    )
    .expect("IrisTools construction must not fail")
}

fn tools_with_declared(toolset: Toolset, declared: DeclaredGates) -> IrisTools {
    tools_with_declared_skills(toolset, true, declared)
}

const WRITES_OFF: DeclaredGates = DeclaredGates {
    write_tools_enabled: Some(false),
    destructive_tools_enabled: None,
};

/// Writes declared off — the state in which the old code stripped tools out of the router.
fn tools_with_writes_off(toolset: Toolset) -> IrisTools {
    tools_with_declared(toolset, WRITES_OFF)
}

/// Every tool surface the binary can serve: three toolsets × skills present or stripped by
/// `--no-skills`. Completeness has to hold over the union, not over one configuration. A tool
/// registered in only one of these is still a tool a caller can invoke — `iris_admin` and
/// `iris_credential_manage` exist only in Merged, the four skill stubs only in Baseline, and the
/// fifteen skill/kb/agent tools disappear under `--no-skills`, which is the mode most of this
/// file's other fixtures happen to use.
fn every_surface() -> Vec<(String, IrisTools)> {
    let mut out = Vec::new();
    for (name, toolset) in [
        ("baseline", Toolset::Baseline),
        ("nostub", Toolset::Nostub),
        ("merged", Toolset::Merged),
    ] {
        for no_skills in [false, true] {
            let label = if no_skills {
                format!("{name}+no-skills")
            } else {
                name.to_string()
            };
            out.push((
                label,
                tools_with_declared_skills(toolset, no_skills, WRITES_OFF),
            ));
        }
    }
    out
}

/// T024. `iris_production_item` and `iris_credential_manage` used to be *removed* from the router
/// when writes were off. That is the failure mode this asserts against, and it is subtle: with the
/// tools absent, the Phase 5 completeness test passes for the wrong reason — it can only check the
/// classification of tools the router actually registered, so a removed tool is a tool nothing
/// verifies. Removal is also invisible to a later reload: the router is built once at startup,
/// while the gate re-resolves on every config change, so an operator who turned writes back on got
/// a gate that said yes and a tool list that had already forgotten the tool existed.
///
/// Visible-but-refusing is the contract. The caller gets `WRITE_TOOLS_DISABLED`, which says *why*;
/// absence says nothing at all.
#[test]
fn write_gated_tools_stay_registered_when_writes_are_off() {
    // Merged is the shipped default (`IRIS_TOOLSET` unset → merged in `mcp.rs`) and the only tier
    // where iris_credential_manage exists at all.
    let names = tools_with_writes_off(Toolset::Merged).registered_tool_names();

    for tool in ["iris_production_item", "iris_credential_manage"] {
        assert!(
            names.contains(tool),
            "{tool} is missing from the router with writes off — it is being removed rather than \
             gated, so the completeness test cannot see it and a reload that turns writes back on \
             will not bring it back. Registered: {} tools.",
            names.len()
        );
    }
}

/// The same assertion for the tier that has to keep working: `iris_production_item` is in Baseline
/// too, and the removal block ran regardless of toolset.
#[test]
fn production_item_stays_registered_in_baseline_with_writes_off() {
    let names = tools_with_writes_off(Toolset::Baseline).registered_tool_names();
    assert!(
        names.contains("iris_production_item"),
        "iris_production_item is missing from the Baseline router with writes off"
    );
}

/// Turning writes *on* must not change the tool list either. If these two counts ever differ, the
/// gate has leaked back into router construction — which is the shape of the defect, not the fix.
#[test]
fn the_advertised_tool_list_does_not_depend_on_the_gate() {
    let off = tools_with_writes_off(Toolset::Merged).registered_tool_names();

    let on = tools_with_declared(
        Toolset::Merged,
        DeclaredGates {
            write_tools_enabled: Some(true),
            destructive_tools_enabled: Some(true),
        },
    )
    .registered_tool_names();

    let only_on: Vec<_> = on.difference(&off).collect();
    let only_off: Vec<_> = off.difference(&on).collect();
    assert!(
        only_on.is_empty() && only_off.is_empty(),
        "the tool list changed with the gate: present only with writes on {only_on:?}, only with \
         writes off {only_off:?}"
    );
}

/// T033 (FR-007, US3 scenario 1). Forward completeness: every registered tool has a
/// `CLASSIFICATION` entry.
///
/// This is the test that makes the gate structural instead of a convention. `gate_check` fails an
/// unclassified tool closed as `Write`, so a missing entry is not an open door — but it is a tool
/// whose refusal nobody chose, which for a read-only tool means a working feature silently stops
/// working the moment writes are off. Either way the classification is a decision someone has to
/// make on purpose, and this is where CI demands it.
///
/// Writes are declared *off* for the fixture because that is the state in which the old code pruned
/// tools out of the router: with a tool absent, this test would pass by never seeing it.
#[test]
fn every_registered_tool_is_classified() {
    for (label, tools) in every_surface() {
        let names = tools.registered_tool_names();
        let mut missing: Vec<&String> = names
            .iter()
            .filter(|n| !CLASSIFICATION.iter().any(|e| e.tool == n.as_str()))
            .collect();
        missing.sort();
        assert!(
            missing.is_empty(),
            "{} tool(s) in the {label} toolset have no write_gate::CLASSIFICATION entry: \
             {missing:?}. Add each one to CLASSIFICATION in \
             crates/iris-agentic-dev-core/src/tools/write_gate.rs — ro() if it only reads, wr() if \
             it can mutate anything, de() for the destructive tier, mixed() if that depends on the \
             action. Until then gate_check fails them closed as Write.",
            missing.len()
        );
    }
}

/// T034. Reverse completeness: every `CLASSIFICATION` entry names a tool some toolset registers.
///
/// A stale entry is the quiet failure mode. `classify` matches on the exact registered name, so
/// renaming a tool without updating the table leaves the old row matching nothing: the rename
/// compiles, the tests that assert *this* tool refuses still pass under whatever name they use, and
/// the renamed tool falls through to the unclassified path. Nothing anywhere says the row went
/// dead. So the union across all three tiers must account for every row.
#[test]
fn every_classification_entry_names_a_registered_tool() {
    let mut registered = std::collections::HashSet::new();
    for (_, tools) in every_surface() {
        registered.extend(tools.registered_tool_names());
    }

    let mut stale: Vec<&str> = CLASSIFICATION
        .iter()
        .map(|e| e.tool)
        .filter(|t| !registered.contains(*t))
        .collect();
    stale.sort_unstable();
    assert!(
        stale.is_empty(),
        "write_gate::CLASSIFICATION has {} entr(ies) that no tool surface registers: {stale:?}. \
         Either the tool was renamed — in which case the entry no longer matches anything and the \
         tool is now unclassified — or it was removed and the row is dead weight. {} distinct tools \
         registered across every toolset/--no-skills combination.",
        stale.len(),
        registered.len()
    );

    // Duplicate rows are the other way a rename goes wrong: `classify` takes the *first* match, so
    // a second entry for the same tool is unreachable and editing it changes nothing.
    let mut seen = std::collections::HashSet::new();
    let dupes: Vec<&str> = CLASSIFICATION
        .iter()
        .map(|e| e.tool)
        .filter(|t| !seen.insert(*t))
        .collect();
    assert!(
        dupes.is_empty(),
        "duplicate CLASSIFICATION entries: {dupes:?}. classify() returns the first match, so the \
         later row is dead code — merge them into one entry."
    );
}

/// T035 (US3 scenario 3). The annotation cross-check, in both directions.
///
/// `read_only_hint` and `destructive_hint` are what a client reads off `tools/list` to decide
/// whether a call needs confirmation; `CLASSIFICATION` is what the server enforces. They are two
/// independent declarations of the same fact, deliberately kept independent — deriving the
/// annotation from the table would make one lie propagate to both, and it is precisely a
/// one-sided lie that shipped: `c641d79` (#94) had to strip `read_only_hint = true` from six
/// mutating tools that advertised themselves read-only for several releases. With both declared by
/// hand, mislabelling a tool takes two edits in two files, and this test names the disagreement.
///
/// Read over every surface, so a tool that only exists in one tier is still checked.
#[test]
fn annotations_agree_with_the_classification() {
    let mut disagreements: Vec<String> = Vec::new();
    let mut checked: std::collections::BTreeMap<String, serde_json::Value> = Default::default();
    for (_, tools) in every_surface() {
        for name in tools.registered_tool_names() {
            if let Some(ann) = tools.tool_annotations(&name) {
                checked.insert(name, ann);
            }
        }
    }

    let mut saw_read_only = 0usize;
    let mut saw_destructive = 0usize;

    for (name, ann) in &checked {
        let name = name.as_str();
        let class = iris_agentic_dev_core::tools::write_gate::classify(name, None);
        let read_only = ann.get("readOnlyHint").and_then(|v| v.as_bool());
        let destructive = ann.get("destructiveHint").and_then(|v| v.as_bool());
        saw_read_only += usize::from(read_only == Some(true));
        saw_destructive += usize::from(destructive == Some(true));

        // readOnlyHint = true is a promise the tool cannot mutate. A tool whose *default* class is
        // ReadOnly but which has write actions (iris_doc, iris_query) is not read-only, so this
        // checks the whole entry rather than the default alone.
        if read_only == Some(true) {
            let entry = CLASSIFICATION.iter().find(|e| e.tool == name);
            let mutating: Vec<&str> = entry
                .map(|e| {
                    e.actions
                        .iter()
                        .filter(|(_, c)| *c != WriteClass::ReadOnly)
                        .map(|(a, _)| *a)
                        .collect()
                })
                .unwrap_or_default();
            if class != Some(WriteClass::ReadOnly) || !mutating.is_empty() {
                disagreements.push(format!(
                    "{name}: annotations say readOnlyHint = true but CLASSIFICATION says \
                     {class:?}{}",
                    if mutating.is_empty() {
                        String::new()
                    } else {
                        format!(" with mutating action(s) {mutating:?}")
                    }
                ));
            }
        }

        // destructiveHint = true is the ☠ marker in docs/tools.md. If the annotation claims it, the
        // destructive tier has to gate it, or the tool warns the caller while the server waves it
        // through on the write gate alone.
        if destructive == Some(true) && class != Some(WriteClass::Destructive) {
            disagreements.push(format!(
                "{name}: annotations say destructiveHint = true but CLASSIFICATION says {class:?} \
                 — the destructive tier does not gate it"
            ));
        }
    }

    assert!(
        disagreements.is_empty(),
        "the router's annotations and write_gate::CLASSIFICATION disagree on {} tool(s). Fix \
         whichever one is wrong — do not derive one from the other, the point is that a \
         mislabelled tool has to be mislabelled twice:\n  {}",
        disagreements.len(),
        disagreements.join("\n  ")
    );

    // A cross-check that reads nothing passes for free, and the ways it could read nothing are all
    // silent: an `annotations` field the router stops populating, a serde rename from camelCase to
    // snake_case, an accessor that returns `None`. Both hint counts are floors well under the
    // present 47/6, so removing a hint from a tool is fine and losing the whole mechanism is not.
    assert!(
        saw_read_only >= 40,
        "only {saw_read_only} tools were seen declaring readOnlyHint = true (expected 40+). Either \
         the annotations stopped reaching the router or `tool_annotations` no longer reads them — \
         this cross-check is now asserting nothing. Keys are camelCase on the wire; a serde rename \
         looks exactly like this."
    );
    assert!(
        saw_destructive >= 5,
        "only {saw_destructive} tools were seen declaring destructiveHint = true (expected 5+); \
         same failure mode as above"
    );
}

// ── 099: fresh container setup actions must classify as Write, not Destructive ──

fn args_with_action(action: &str) -> serde_json::Map<String, serde_json::Value> {
    let mut m = serde_json::Map::new();
    m.insert(
        "action".to_string(),
        serde_json::Value::String(action.to_string()),
    );
    m
}

#[test]
fn test_fresh_container_setup_actions_classify_as_write() {
    for action in &[
        "clear_password_change_flag",
        "unlock_user",
        "fresh_container_setup",
    ] {
        let args = args_with_action(action);
        let class = iris_agentic_dev_core::tools::write_gate::classify("iris_admin", Some(&args));
        assert_eq!(
            class,
            Some(WriteClass::Write),
            "iris_admin action={action} must classify as WriteClass::Write, got {:?}",
            class
        );
    }
}

// ── 097: mirror management actions must classify at correct tiers ──

#[test]
fn test_mirror_add_async_classifies_as_write() {
    let args = args_with_action("mirror_add_async");
    let class = iris_agentic_dev_core::tools::write_gate::classify("iris_admin", Some(&args));
    assert_eq!(
        class,
        Some(WriteClass::Write),
        "iris_admin action=mirror_add_async must classify as WriteClass::Write, got {:?}",
        class
    );
}

#[test]
fn test_mirror_failover_classifies_as_destructive() {
    let args = args_with_action("mirror_failover");
    let class = iris_agentic_dev_core::tools::write_gate::classify("iris_admin", Some(&args));
    assert_eq!(
        class,
        Some(WriteClass::Destructive),
        "iris_admin action=mirror_failover must classify as WriteClass::Destructive, got {:?}",
        class
    );
}

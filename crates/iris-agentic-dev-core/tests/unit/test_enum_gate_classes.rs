//! Every registered tool has a declared gate class (E1).
//!
//! `write_gate::CLASSIFICATION` is the one declared table. A row says what write tier a tool needs,
//! whether it acts on an IRIS server (`Iris`: the template, `allow`, `dataPolicy` and role gates
//! judge it against a target) or on nothing of IRIS's (`Local`: only the write gate applies), and
//! for a `Local` tool why. The registry-to-table completeness in both directions lives in
//! `test_gate_classification`; this file holds the rest of what the declaration has to satisfy, so
//! that a new tool cannot be added without its gate being decided:
//!
//! - a `Local` row carries a reason (the builder will not compile without one);
//! - the set of `Local` tools is closed: [`LOCAL_TOOLS`] below lists them, and adding a name is a
//!   change a reviewer sees in this file;
//! - an `Iris` row maps to a `ToolCategory`, because `allow` and the environment template judge a
//!   call by its category and a tool with none passes both;
//! - the one category exemption (`INTENTIONALLY_UNCATEGORIZED_TOOLS`) names `Local` tools only.
//!
//! Whether a `Local` tool really touches no IRIS is not decided by reading its row: that is the
//! behavioural test in `test_enum_no_leaks`.

use iris_agentic_dev_core::iris::server_manager::{
    tool_to_category_pub, INTENTIONALLY_UNCATEGORIZED_TOOLS,
};
use iris_agentic_dev_core::tools::write_gate::{TargetKind, CLASSIFICATION};
use iris_agentic_dev_core::tools::{IrisTools, Toolset};
use std::collections::BTreeSet;

/// The tools allowed to be `Local`. This list may only shrink: a tool that acts on an IRIS server
/// is `Iris` and is judged against it, and a tool that does not has to be argued for here, in
/// review, with its reason in the table.
///
/// A name leaves when its row becomes `Iris`. Removing a name whose row is still `Local` fails the
/// first test below; adding a `Local` row without adding the name fails it too.
const LOCAL_TOOLS: &[&str] = &[
    "agent_history",
    "agent_stats",
    "check_config",
    "iris_add_server",
    "iris_containers",
    "iris_doc_search",
    "iris_import_servers",
    "iris_list_containers",
    "iris_reload_pool",
    "iris_remove_server",
    "iris_select_container",
    "iris_servers",
    "iris_start_sandbox",
    "iris_symbols_local",
    "kb_recall",
    "skill_community_install",
    "skill_community_list",
    "skill_optimize",
    "skill_propose",
    "skill_share",
];

fn local_rows() -> BTreeSet<&'static str> {
    CLASSIFICATION
        .iter()
        .filter(|r| r.kind == TargetKind::Local)
        .map(|r| r.tool)
        .collect()
}

fn registered() -> BTreeSet<String> {
    let mut all = BTreeSet::new();
    for toolset in [Toolset::Baseline, Toolset::Merged, Toolset::Nostub] {
        all.extend(
            IrisTools::new_with_toolset(None, toolset)
                .expect("IrisTools::new_with_toolset")
                .registered_tool_names(),
        );
    }
    all
}

#[test]
fn the_local_tools_are_a_closed_list() {
    let declared: BTreeSet<&str> = LOCAL_TOOLS.iter().copied().collect();
    assert_eq!(
        declared.len(),
        LOCAL_TOOLS.len(),
        "LOCAL_TOOLS names a tool twice"
    );
    let rows = local_rows();
    let unlisted: Vec<_> = rows.difference(&declared).collect();
    assert!(
        unlisted.is_empty(),
        "CLASSIFICATION marks these tools Local and LOCAL_TOOLS does not list them: {unlisted:?}. \
         A Local tool is judged by the write gate alone: no template, no `allow`, no `dataPolicy`. \
         If the tool reaches an IRIS server, declare it Iris. If it truly reaches none, add it \
         here and give the row a reason."
    );
    let stale: Vec<_> = declared.difference(&rows).collect();
    assert!(
        stale.is_empty(),
        "LOCAL_TOOLS lists these tools and their rows are no longer Local: {stale:?}. Remove \
         them: the list shrinks as rows become Iris."
    );
}

#[test]
fn every_local_row_says_why() {
    for row in CLASSIFICATION {
        match row.kind {
            TargetKind::Local => assert!(
                row.local_reason.trim().len() >= 12,
                "{} is Local and its reason is {:?}: say in a sentence what it touches instead \
                 of IRIS",
                row.tool,
                row.local_reason
            ),
            TargetKind::Iris => assert!(
                row.local_reason.is_empty(),
                "{} is an Iris row and carries the Local reason {:?}",
                row.tool,
                row.local_reason
            ),
        }
    }
}

#[test]
fn every_iris_row_has_a_tool_category() {
    let uncategorized: Vec<_> = CLASSIFICATION
        .iter()
        .filter(|r| r.kind == TargetKind::Iris)
        .filter(|r| tool_to_category_pub(r.tool).is_none())
        .map(|r| r.tool)
        .collect();
    assert!(
        uncategorized.is_empty(),
        "these Iris tools map to no ToolCategory: {uncategorized:?}. `allow` and the environment \
         template judge a call by its category; add an arm to `tool_to_category` \
         (iris/server_manager.rs)."
    );
}

#[test]
fn the_category_exemptions_are_local_tools() {
    let rows = local_rows();
    for name in INTENTIONALLY_UNCATEGORIZED_TOOLS {
        assert!(
            rows.contains(name),
            "{name} is exempt from the category table and is not a Local row: a tool that acts \
             on an IRIS server is judged by its category"
        );
    }
}

#[test]
fn the_local_list_names_registered_tools() {
    let all = registered();
    for name in LOCAL_TOOLS {
        assert!(
            all.contains(*name),
            "LOCAL_TOOLS names {name}, which no toolset registers"
        );
    }
}

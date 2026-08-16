//! Regression test: every real tool must be either categorized (`tool_to_category`) or
//! explicitly, deliberately exempt (`INTENTIONALLY_UNCATEGORIZED_TOOLS`).
//!
//! `check_env_gate` and `policy_gate` both do `tool_to_category(tool_name)?` — a `None`
//! result means "not gated," not "blocked." Before 2026-08 that silently described 55 of
//! the 90 real tools, including two (`iris_ws_exec`, `iris_test`/`iris_coverage`) that
//! could run arbitrary or test code while completely bypassing the documented guarantee
//! that `mcpTemplate = "live"`/`"test"` blocks `Execute`. This test exists so a new tool
//! added later can't silently join that set — it either gets a category, or it goes into
//! `INTENTIONALLY_UNCATEGORIZED_TOOLS` with a comment explaining why, on purpose.

use iris_agentic_dev_core::iris::server_manager::{
    tool_to_category_pub, INTENTIONALLY_UNCATEGORIZED_TOOLS,
};
use iris_agentic_dev_core::tools::{IrisTools, Toolset};

#[test]
fn test_every_real_tool_has_a_category_or_is_exempt() {
    // Union of Baseline and Merged covers all 90 real tools (see registered_tool_names'
    // own doc comment) — Nostub is a strict subset of Baseline, so it adds nothing.
    let baseline = IrisTools::new_with_toolset(None, Toolset::Baseline)
        .expect("IrisTools::new")
        .registered_tool_names();
    let merged = IrisTools::new_with_toolset(None, Toolset::Merged)
        .expect("IrisTools::new")
        .registered_tool_names();

    let mut uncovered: Vec<String> = baseline
        .union(&merged)
        .filter(|name| {
            tool_to_category_pub(name).is_none()
                && !INTENTIONALLY_UNCATEGORIZED_TOOLS.contains(&name.as_str())
        })
        .cloned()
        .collect();
    uncovered.sort();

    assert!(
        uncovered.is_empty(),
        "these tools have no ToolCategory mapping and are not in \
         INTENTIONALLY_UNCATEGORIZED_TOOLS, so they silently bypass both check_env_gate \
         and policy_gate: {uncovered:?}. Add a match arm in tool_to_category (server_manager.rs) \
         or, if the tool genuinely makes no IRIS call and can't violate any policy, add it to \
         INTENTIONALLY_UNCATEGORIZED_TOOLS with a comment explaining why."
    );
}

/// `INTENTIONALLY_UNCATEGORIZED_TOOLS` itself must stay small and real — a name that
/// isn't even a real tool anymore (renamed or removed) shouldn't linger there forever.
#[test]
fn test_intentionally_uncategorized_tools_are_real() {
    let baseline = IrisTools::new_with_toolset(None, Toolset::Baseline)
        .expect("IrisTools::new")
        .registered_tool_names();
    let merged = IrisTools::new_with_toolset(None, Toolset::Merged)
        .expect("IrisTools::new")
        .registered_tool_names();

    for name in INTENTIONALLY_UNCATEGORIZED_TOOLS {
        assert!(
            baseline.contains(*name) || merged.contains(*name),
            "'{name}' is listed in INTENTIONALLY_UNCATEGORIZED_TOOLS but is not a real \
             registered tool in any toolset — remove it or check for a rename."
        );
    }
}

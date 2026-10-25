//! S14: a tool with no category is not "not gated". It gets the strictest reading, and the tools
//! that are exempt are an explicit list that `check_config` reports.

use iris_agentic_dev_core::iris::server_manager::INTENTIONALLY_UNCATEGORIZED_TOOLS;
use iris_agentic_dev_core::iris::workspace_config::{ConnectionPolicy, McpTemplate, ToolCategory};
use iris_agentic_dev_core::testing::gated;
use iris_agentic_dev_core::tools::write_gate::{classify, effective_category, WriteClass};
use iris_agentic_dev_core::tools::{IrisTools, Toolset};
use serde_json::json;

fn policy_allow(cats: &[ToolCategory]) -> ConnectionPolicy {
    ConnectionPolicy {
        server_name: "s".to_string(),
        allow: Some(cats.to_vec()),
        ..ConnectionPolicy::default()
    }
}

#[test]
fn an_unmapped_tool_is_judged_as_execute_and_admin() {
    for class in [
        WriteClass::ReadOnly,
        WriteClass::Write,
        WriteClass::Destructive,
    ] {
        assert_eq!(
            effective_category("a_tool_nobody_categorised", None, class),
            vec![ToolCategory::Execute, ToolCategory::Admin],
            "{class:?}"
        );
    }
}

#[test]
fn an_unmapped_tool_meets_the_environment_template() {
    for template in [McpTemplate::Live, McpTemplate::Test] {
        let r = gated::env("a_tool_nobody_categorised", &template, "s", &json!({}));
        let r = r.unwrap_or_else(|| panic!("{template:?} let an unmapped tool through"));
        assert_eq!(r["error_code"], "ENV_GATE_BLOCKED", "{r}");
    }
    assert!(gated::env(
        "a_tool_nobody_categorised",
        &McpTemplate::Dev,
        "s",
        &json!({})
    )
    .is_none());
}

#[test]
fn an_unmapped_tool_meets_allow() {
    // Every category but the two it needs.
    let r = gated::policy(
        "a_tool_nobody_categorised",
        "s",
        Some(&policy_allow(&[
            ToolCategory::Query,
            ToolCategory::Search,
            ToolCategory::Docs,
        ])),
    )
    .expect("allow = [query, search, docs] let an unmapped tool through");
    assert_eq!(r["error_code"], "POLICY_GATE", "{r}");
    // Execute alone is not enough, and neither is admin alone.
    for only in [ToolCategory::Execute, ToolCategory::Admin] {
        assert!(
            gated::policy(
                "a_tool_nobody_categorised",
                "s",
                Some(&policy_allow(&[only]))
            )
            .is_some(),
            "one of the two categories was enough"
        );
    }
    // An operator who lists both has asked for it.
    assert!(gated::policy(
        "a_tool_nobody_categorised",
        "s",
        Some(&policy_allow(&[ToolCategory::Execute, ToolCategory::Admin]))
    )
    .is_none());
}

#[test]
fn only_the_listed_tools_are_exempt() {
    assert_eq!(INTENTIONALLY_UNCATEGORIZED_TOOLS, &["check_config"]);
    assert!(effective_category("check_config", None, WriteClass::ReadOnly).is_empty());
    assert!(gated::env("check_config", &McpTemplate::Live, "s", &json!({})).is_none());
    assert!(gated::policy(
        "check_config",
        "s",
        Some(&policy_allow(&[ToolCategory::Query]))
    )
    .is_none());
}

/// Every tool any toolset registers has a category, or is on the exemption list. Walks the real
/// routers, so a tool added later is covered without an edit here.
#[test]
fn every_registered_tool_has_a_category_or_is_exempt() {
    let mut names = std::collections::BTreeSet::new();
    for toolset in [Toolset::Baseline, Toolset::Nostub, Toolset::Merged] {
        let tools = IrisTools::new_with_toolset(None, toolset).expect("IrisTools::new");
        names.extend(tools.registered_tool_names());
    }
    assert!(names.len() > 70, "the walk found {} tools", names.len());
    for name in &names {
        let class = classify(name, None).unwrap_or(WriteClass::Write);
        let cats = effective_category(name, None, class);
        let exempt = INTENTIONALLY_UNCATEGORIZED_TOOLS.contains(&name.as_str());
        assert!(
            exempt == cats.is_empty(),
            "{name}: exempt = {exempt}, categories = {cats:?}"
        );
        // Not by the fall-back: a registered tool is mapped on purpose.
        if !exempt {
            assert!(
                iris_agentic_dev_core::iris::server_manager::tool_to_category_pub(name).is_some(),
                "{name} reached only the strict fall-back"
            );
        }
    }
}

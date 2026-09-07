//! The schema-call suite against the listing a running server emits (113 T036b).
//!
//! `test_schema_tasks.rs` checks the suite covers every converted tool. It cannot check that a
//! task's `expect_keys` name real parameters, because a unit test has no schema to compare against.
//! Left unchecked, a typo there fails the tool forever: the model sends the right call, the task
//! demands `globl`, and SC-007 records a schema failure that is really a task bug.

use iris_agentic_dev_core::benchmark::schema_tasks::load_schema_tasks;
use iris_agentic_dev_core::testing::{advertised_tools_in_toolset, require_iad_binary};

#[test]
#[ignore = "spawns the built binary; run with --include-ignored"]
fn every_task_targets_a_tool_that_exists() {
    let Some(_bin) = require_iad_binary() else {
        return;
    };
    let tools = advertised_tools_in_toolset(None);
    let tasks = load_schema_tasks().expect("the embedded schema-call suite must parse");
    let missing: Vec<&str> = tasks
        .iter()
        .map(|t| t.tool.as_str())
        .filter(|t| !tools.contains_key(*t))
        .collect();
    assert!(
        missing.is_empty(),
        "these tasks name tools the server does not advertise, so they can never run: {missing:?}"
    );
}

#[test]
#[ignore = "spawns the built binary; run with --include-ignored"]
fn every_expected_parameter_is_one_the_tool_declares() {
    let Some(_bin) = require_iad_binary() else {
        return;
    };
    let tools = advertised_tools_in_toolset(None);
    let tasks = load_schema_tasks().expect("the embedded schema-call suite must parse");

    let mut wrong: Vec<String> = Vec::new();
    for task in &tasks {
        let Some(tool) = tools.get(&task.tool) else {
            continue; // reported by the test above
        };
        let props = tool
            .get("inputSchema")
            .and_then(|s| s.get("properties"))
            .and_then(|p| p.as_object())
            .unwrap_or_else(|| panic!("`{}` advertises no properties", task.tool));
        for key in &task.expect_keys {
            if !props.contains_key(key) {
                wrong.push(format!(
                    "{}.{key} (declared: {})",
                    task.tool,
                    props
                        .keys()
                        .map(String::as_str)
                        .collect::<Vec<&str>>()
                        .join(", ")
                ));
            }
        }
    }

    assert!(
        wrong.is_empty(),
        "a task expects parameters its tool does not declare, so the model cannot pass it no \
         matter how good the schema is: {wrong:?}"
    );
}

#[test]
#[ignore = "spawns the built binary; run with --include-ignored"]
fn every_task_tool_advertises_a_closed_schema() {
    let Some(_bin) = require_iad_binary() else {
        return;
    };
    let tools = advertised_tools_in_toolset(None);
    let tasks = load_schema_tasks().expect("the embedded schema-call suite must parse");

    // Without `additionalProperties: false` the validator's unknown-key check is decoration: the
    // server would accept the invented parameter, so accepting it here would be the honest answer
    // and SC-007 would be measuring nothing.
    let open: Vec<&str> = tasks
        .iter()
        .filter(|task| {
            tools
                .get(&task.tool)
                .and_then(|t| t.get("inputSchema"))
                .and_then(|s| s.get("additionalProperties"))
                .and_then(|a| a.as_bool())
                != Some(false)
        })
        .map(|task| task.tool.as_str())
        .collect();
    assert!(
        open.is_empty(),
        "these tools still advertise an open parameter set: {open:?}"
    );
}

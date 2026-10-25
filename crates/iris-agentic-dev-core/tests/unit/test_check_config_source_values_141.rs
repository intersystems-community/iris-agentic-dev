//! Spec 141 T188: `check_config`'s description lists the `connection_source` values the code can
//! return, no more and no fewer.
//!
//! It used to say `http|docker|disconnected`. None of the three is ever returned; the field holds
//! how the connection was chosen, and a server with no connection still reports one.

use iris_agentic_dev_core::tools::{ConnectionSource, IrisTools, Toolset};
use std::collections::BTreeSet;

/// Every value `connection_source` can take, as the wire serialises it.
///
/// The `match` has no wildcard arm: adding a variant to `ConnectionSource` stops this file
/// compiling until the new variant is listed here, which is what keeps the list honest.
pub fn every_value_the_code_can_return() -> BTreeSet<String> {
    let all = [
        ConnectionSource::ConfigFile,
        ConnectionSource::EnvVars,
        ConnectionSource::IrisSelectContainer,
        ConnectionSource::AutoDiscovered,
    ];
    for source in &all {
        match source {
            ConnectionSource::ConfigFile
            | ConnectionSource::EnvVars
            | ConnectionSource::IrisSelectContainer
            | ConnectionSource::AutoDiscovered => {}
        }
    }
    all.iter()
        .map(|s| {
            serde_json::to_value(s)
                .unwrap()
                .as_str()
                .expect("a unit variant serialises to a string")
                .to_string()
        })
        .collect()
}

fn check_config_description() -> String {
    let tools = IrisTools::new_with_toolset(None, Toolset::Merged).unwrap();
    tools
        .tool_catalogue()
        .into_iter()
        .find(|e| e.name == "check_config")
        .expect("check_config is in the merged catalogue")
        .description
        .expect("check_config has a description")
}

/// The `a|b|c` list in `connection_source (a|b|c)`.
fn documented_values(description: &str) -> BTreeSet<String> {
    let after = description
        .split("connection_source (")
        .nth(1)
        .expect("the description names connection_source with its values in parentheses");
    let list = after.split(')').next().unwrap();
    list.split('|').map(|v| v.trim().to_string()).collect()
}

#[test]
fn the_description_lists_exactly_the_values_the_code_returns() {
    assert_eq!(
        documented_values(&check_config_description()),
        every_value_the_code_can_return()
    );
}

#[test]
fn the_documented_values_are_the_ones_the_output_schema_allows() {
    let tools = IrisTools::new_with_toolset(None, Toolset::Merged).unwrap();
    let schema = tools
        .tool_output_schema("check_config")
        .expect("check_config declares an output schema")
        .to_string();
    for value in every_value_the_code_can_return() {
        assert!(
            schema.contains(&format!("\"{value}\"")),
            "the output schema does not allow connection_source {value:?}: {schema}"
        );
    }
}

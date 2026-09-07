//! The suppression flag against the listing the server actually serves (113 T036a).
//!
//! A parse test proves the names are read correctly and nothing more. The wiring is the part that
//! has failed before in this repo: `--config` existed, parsed, and was ignored because `self.config`
//! never reached the server (#111). So this spawns the binary, sets the var, and reads `tools/list`.
//!
//! The second assertion is the one that makes SC-007 meaningful: the schema must survive. A flag
//! that blanked the description *and* the properties would make every task fail and look like
//! evidence that the schemas are insufficient.

use iris_agentic_dev_core::testing::{
    advertised_tools_in_toolset_with, advertised_tools_with_flags, require_iad_binary,
    TOOL_UNDER_SUPPRESSION,
};

#[test]
#[ignore = "spawns the built binary; run with --include-ignored"]
fn a_suppressed_tool_loses_its_description_and_keeps_its_schema() {
    let Some(_bin) = require_iad_binary() else {
        return;
    };
    let tools = advertised_tools_in_toolset_with(None, &[TOOL_UNDER_SUPPRESSION]);

    let tool = tools
        .get(TOOL_UNDER_SUPPRESSION)
        .unwrap_or_else(|| panic!("`{TOOL_UNDER_SUPPRESSION}` must appear in tools/list"));

    let description = tool.get("description").and_then(|d| d.as_str());
    assert!(
        description.is_none_or(str::is_empty),
        "`{TOOL_UNDER_SUPPRESSION}` was named for suppression but still advertises a description: \
         {description:?}"
    );

    let props = tool
        .get("inputSchema")
        .and_then(|s| s.get("properties"))
        .and_then(|p| p.as_object())
        .unwrap_or_else(|| {
            panic!("`{TOOL_UNDER_SUPPRESSION}` lost its properties along with its description")
        });
    assert!(
        !props.is_empty(),
        "suppression must leave the schema intact — `{TOOL_UNDER_SUPPRESSION}` advertises no \
         properties, so a failing task would measure the flag rather than the schema"
    );
    // Per-property descriptions are part of the schema, not the tool description. They are what an
    // agent reads once the prose is gone, so blanking them would defeat the whole measurement.
    assert!(
        props
            .values()
            .any(|p| p.get("description").and_then(|d| d.as_str()).is_some()),
        "the properties of `{TOOL_UNDER_SUPPRESSION}` carry no descriptions at all: {props:?}"
    );
}

#[test]
#[ignore = "spawns the built binary; run with --include-ignored"]
fn suppression_touches_only_the_named_tool() {
    let Some(_bin) = require_iad_binary() else {
        return;
    };
    let tools = advertised_tools_in_toolset_with(None, &[TOOL_UNDER_SUPPRESSION]);

    let mut blanked: Vec<&String> = tools
        .iter()
        .filter(|(_, tool)| {
            tool.get("description")
                .and_then(|d| d.as_str())
                .is_none_or(str::is_empty)
        })
        .map(|(name, _)| name)
        .collect();
    blanked.sort();

    assert_eq!(
        blanked,
        vec![&TOOL_UNDER_SUPPRESSION.to_string()],
        "exactly one tool was named for suppression; these came back with no description"
    );
}

#[test]
#[ignore = "spawns the built binary; run with --include-ignored"]
fn the_flag_reaches_the_listing_without_the_env_var() {
    let Some(_bin) = require_iad_binary() else {
        return;
    };
    // Two tools, passed as two occurrences, with nothing in the environment. This is the assertion
    // that a flag parsed into a struct field nobody reads cannot pass.
    let tools = advertised_tools_with_flags(&[
        "--suppress-tool-description",
        TOOL_UNDER_SUPPRESSION,
        "--suppress-tool-description",
        "my_access",
    ]);

    let mut blanked: Vec<&String> = tools
        .iter()
        .filter(|(_, tool)| {
            tool.get("description")
                .and_then(|d| d.as_str())
                .is_none_or(str::is_empty)
        })
        .map(|(name, _)| name)
        .collect();
    blanked.sort();

    let expected = {
        let mut e = vec![TOOL_UNDER_SUPPRESSION.to_string(), "my_access".to_string()];
        e.sort();
        e
    };
    assert_eq!(
        blanked.into_iter().cloned().collect::<Vec<String>>(),
        expected,
        "--suppress-tool-description was passed twice; those two tools and no others should have \
         lost their descriptions"
    );
}

#[test]
#[ignore = "spawns the built binary; run with --include-ignored"]
fn an_unset_variable_suppresses_nothing() {
    let Some(_bin) = require_iad_binary() else {
        return;
    };
    // The control run. Without it, a server that blanks every description would pass the test
    // above and the one before it, and SC-007 would be measured against a listing with no prose
    // anywhere rather than one tool's prose removed.
    let tools = advertised_tools_in_toolset_with(None, &[]);

    let mut blank: Vec<&String> = tools
        .iter()
        .filter(|(_, tool)| {
            tool.get("description")
                .and_then(|d| d.as_str())
                .is_none_or(str::is_empty)
        })
        .map(|(name, _)| name)
        .collect();
    blank.sort();

    assert!(
        blank.is_empty(),
        "with nothing suppressed every tool must describe itself; these do not: {blank:?}"
    );
}

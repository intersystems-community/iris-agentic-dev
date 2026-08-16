use iris_agentic_dev::cmd::tool::{dispatch_map_keys, TOOL_NAMES};
use iris_agentic_dev_core::tools::{IrisTools, Toolset};

#[test]
fn test_dispatch_map_keys_match_registered_tool_names() {
    // Build the Merged toolset (same as MCP server uses) and compare names
    let tools = IrisTools::new_with_toolset(None, Toolset::Merged).unwrap();
    let registered = tools.registered_tool_names();
    let dispatch = dispatch_map_keys();

    let mut missing: Vec<String> = registered
        .iter()
        .filter(|n| !dispatch.contains(n.as_str()))
        .cloned()
        .collect();
    missing.sort();
    let mut extra: Vec<&str> = dispatch
        .iter()
        .copied()
        .filter(|n| !registered.contains(*n))
        .collect();
    extra.sort();

    assert!(
        missing.is_empty(),
        "tools in registered_tool_names() but not in dispatch map: {:?}",
        missing
    );
    assert!(
        extra.is_empty(),
        "tools in dispatch map but not in registered_tool_names(): {:?}",
        extra
    );
}

#[test]
fn test_tool_names_sorted() {
    let sorted: Vec<&str> = {
        let mut v = TOOL_NAMES.to_vec();
        v.sort_unstable();
        v
    };
    assert_eq!(TOOL_NAMES.to_vec(), sorted, "TOOL_NAMES must be sorted");
}

#[test]
fn test_unknown_tool_is_rejected() {
    assert!(
        !dispatch_map_keys().contains("nonexistent_tool_xyz"),
        "unknown tool should not be in dispatch map"
    );
}

#[tokio::test]
async fn test_all_tool_names_dispatch_in_call_for_test() {
    // Regression test for the 043-local-first-sync field report (defect 03): TOOL_NAMES
    // advertised 22 tools that `test_dispatch_map_keys_match_registered_tool_names` above
    // happily approved — because that test only compares TOOL_NAMES against the MCP tool
    // registry, never against what `call_for_test()` actually dispatches. Those 22 had no
    // arm in call_for_test()'s dispatch macro, so `iris-agentic-dev tool <name>` rejected
    // every one of them as "unknown tool" while the MCP stdio transport served them fine.
    //
    // A tool can still fail here for an unrelated reason — missing required params, no
    // IRIS connection — that's expected with no live server and is not what this checks.
    // Only the literal "unknown tool: <name>" message means this dispatcher has no arm
    // for it at all.
    let tools = IrisTools::new_with_toolset(None, Toolset::Merged).unwrap();
    let mut undispatched: Vec<&str> = Vec::new();
    for &name in TOOL_NAMES {
        if let Err(e) = tools.call_for_test(name, serde_json::json!({})).await {
            if e == format!("unknown tool: {name}") {
                undispatched.push(name);
            }
        }
    }
    assert!(
        undispatched.is_empty(),
        "tools in TOOL_NAMES with no dispatch arm in call_for_test(): {:?}",
        undispatched
    );
}

//! Spec 141 FR-004, R1 C3: what the benchmark agent may run.
//!
//! The agent writes `iris-agentic-dev tool <name> --args '<json>'` lines and the benchmark ran each
//! one in a subprocess with the parent's whole environment. Any tool, any `server`, any
//! `confirm`. The allowlist sits in the parent so a refused call never starts a process.
//!
//! `check_invocation` and the refused-argument list decide, and `run_invocations` is the seam a
//! test replaces the subprocess through.

use iris_agentic_dev_core::benchmark::cli_dispatch::{
    build_cli_dispatch_system_prompt, check_invocation, parse_tool_invocations, run_invocations,
    ToolInvocation, CLI_DISPATCH_REFUSED_ARGS, CLI_DISPATCH_TOOLS,
};
use iris_agentic_dev_core::tools::{IrisTools, Toolset};

fn inv(tool: &str, args: &str) -> ToolInvocation {
    ToolInvocation {
        tool_name: tool.to_string(),
        args_json: args.to_string(),
    }
}

#[test]
fn the_four_tools_are_accepted_with_plain_arguments() {
    assert_eq!(
        CLI_DISPATCH_TOOLS.len(),
        4,
        "the benchmark advertises four tools"
    );
    for tool in CLI_DISPATCH_TOOLS {
        let verdict = check_invocation(&inv(tool, r#"{"code":"Write 1"}"#));
        assert!(verdict.is_ok(), "{tool} must be accepted: {verdict:?}");
    }
}

#[test]
fn every_allowed_tool_is_registered_in_the_router() {
    let registered = IrisTools::new_with_toolset(None, Toolset::Merged)
        .unwrap()
        .registered_tool_names();
    assert_eq!(CLI_DISPATCH_TOOLS.len(), 4);
    for tool in CLI_DISPATCH_TOOLS {
        assert!(
            registered.contains(*tool),
            "{tool} is allowed by the benchmark but the router has no such tool"
        );
    }
}

#[test]
fn a_tool_outside_the_four_is_refused() {
    for tool in [
        "global_kill",
        "iris_global",
        "iris_doc",
        "iris_admin",
        "iris_ws_exec",
        "skill_community_install",
    ] {
        assert!(
            check_invocation(&inv(tool, "{}")).is_err(),
            "{tool} is not one of the benchmark's tools"
        );
    }
}

#[test]
fn the_arguments_that_choose_a_server_a_namespace_or_a_confirmation_are_refused_in_every_spelling()
{
    for key in [
        "server",
        "Server",
        "SERVER",
        "namespace",
        "Namespace",
        "confirm",
        "confirmed",
        "Confirm",
        "force_writable",
        "forceWritable",
    ] {
        let args = format!("{{\"code\":\"Write 1\",\"{key}\":\"x\"}}");
        assert!(
            check_invocation(&inv("iris_execute", &args)).is_err(),
            "argument {key:?} must be refused"
        );
    }
    assert_eq!(
        CLI_DISPATCH_REFUSED_ARGS.len(),
        5,
        "the list is the five names of R1 C3"
    );
}

#[test]
fn arguments_that_are_not_a_json_object_are_refused() {
    for args in ["not json", "[1,2]", "\"str\"", "", "{\"code\":"] {
        assert!(
            check_invocation(&inv("iris_execute", args)).is_err(),
            "{args:?} is not a JSON object"
        );
    }
}

#[test]
fn a_refused_invocation_never_reaches_the_runner_and_the_model_sees_a_tool_error() {
    let invocations = parse_tool_invocations(
        "iris-agentic-dev tool global_kill --args '{\"global\":\"^X\"}'\n\
         iris-agentic-dev tool iris_execute --args '{\"code\":\"Write 1\",\"server\":\"prod\"}'\n\
         iris-agentic-dev tool iris_execute --args '{\"code\":\"Write 1\"}'\n",
    );
    assert_eq!(invocations.len(), 3);
    let mut started: Vec<String> = Vec::new();
    let shown = run_invocations(&invocations, &mut |i| {
        started.push(i.tool_name.clone());
        "ran".to_string()
    });
    assert_eq!(
        started,
        vec!["iris_execute".to_string()],
        "only the plain iris_execute call may start a process"
    );
    assert!(shown[0].contains("refused"), "{}", shown[0]);
    assert!(shown[1].contains("refused"), "{}", shown[1]);
    assert!(shown[2].contains("ran"), "{}", shown[2]);
}

#[test]
fn the_prompts_tool_list_is_the_allowlist() {
    let prompt = build_cli_dispatch_system_prompt("");
    let after = prompt
        .split("Available tools:")
        .nth(1)
        .expect("the prompt lists the tools");
    let listed: Vec<String> = after
        .lines()
        .filter_map(|l| l.trim().strip_prefix("- `"))
        .filter_map(|l| l.split('`').next())
        .map(str::to_string)
        .collect();
    let mut want: Vec<String> = CLI_DISPATCH_TOOLS.iter().map(|s| s.to_string()).collect();
    let mut got = listed.clone();
    got.sort();
    want.sort();
    assert_eq!(got, want, "prompt lists {listed:?}");
}

//! R2: the skill and knowledge-base tools that run ObjectScript on the default server meet the
//! default server's gates.
//!
//! They were registered as local tools, so the environment template, `allow` and strict read-only
//! never judged them, although most of them run code on the default server: `skill` (every
//! action), `skill_community`, `kb`, `kb_index` and `agent_info` through a scratch class, and
//! `skill_list`, `skill_describe`, `skill_search` and `skill_forget` through the container's
//! terminal. The tools that read only the registry or the call history stay local.
//!
//! Nothing listens at the rig's default address, so a call that got past every gate would fail
//! with a connection error or answer from the bundled skills, never with a refusal code.

use crate::support_gate_141::*;
use iris_agentic_dev_core::iris::workspace_config::ToolCategory;
use iris_agentic_dev_core::tools::write_gate::{
    classify, effective_category, is_scratch, tool_class, TargetKind, TargetSource, WriteClass,
};
use iris_agentic_dev_core::tools::Toolset;
use serde_json::{json, Value};

/// A tool, the arguments that pass the schema check, and whether it compiles a scratch class on
/// the server (the other tools run their code in the container's terminal).
struct Case {
    tool: &'static str,
    args: Value,
    scratch: bool,
}

fn iris_cases() -> Vec<Case> {
    let c = |tool, args, scratch| Case {
        tool,
        args,
        scratch,
    };
    vec![
        c("skill_list", json!({}), false),
        c("skill_describe", json!({"name": "x"}), false),
        c("skill_search", json!({"query": "x"}), false),
        c("skill_forget", json!({"name": "x"}), false),
        c("kb_index", json!({}), true),
        c("skill", json!({"action": "list"}), true),
        c("skill", json!({"action": "describe", "name": "x"}), true),
        c("skill", json!({"action": "search", "query": "x"}), true),
        c("skill", json!({"action": "forget", "name": "x"}), true),
        c("skill", json!({"action": "propose"}), true),
        c("skill_community", json!({"action": "list"}), true),
        c(
            "skill_community",
            json!({"action": "install", "package": "p"}),
            true,
        ),
        c("kb", json!({"action": "recall", "query": "x"}), true),
        c("kb", json!({"action": "index"}), true),
        c("agent_info", json!({"what": "stats"}), true),
        c("agent_info", json!({"what": "history"}), true),
    ]
}

/// Tools of the family that read the registry or the call history and never call IRIS. The first
/// three are also stubs that answer NOT_IMPLEMENTED.
fn local_cases() -> Vec<Case> {
    let c = |tool, args| Case {
        tool,
        args,
        scratch: false,
    };
    vec![
        c("kb_recall", json!({"query": "x"})),
        c("skill_community_list", json!({})),
        c("agent_history", json!({})),
        c("agent_stats", json!({})),
        c("skill_propose", json!({})),
        c("skill_optimize", json!({"name": "x"})),
        c("skill_share", json!({"name": "x"})),
        c("skill_community_install", json!({"package": "p"})),
    ]
}

const GATES_OPEN: &str = "write_tools_enabled = true\ndestructive_tools_enabled = true\n";

/// The codes a gate refuses with.
fn refused(body: &Value) -> bool {
    matches!(
        body["error_code"].as_str(),
        Some("POLICY_GATE")
            | Some("ENV_GATE_BLOCKED")
            | Some("READ_ONLY_STRICT")
            | Some("WRITE_GATE")
            | Some("DESTRUCTIVE_GATE")
            | Some("WRITE_TOOLS_DISABLED")
            | Some("DESTRUCTIVE_TOOLS_DISABLED")
    )
}

/// Tools the consolidated toolset leaves out: `agent_info` is replaced by `agent_stats` and
/// `agent_history`, and the four stubs are pruned.
const ABSENT_FROM_MERGED: &[&str] = &[
    "agent_info",
    "skill_propose",
    "skill_optimize",
    "skill_share",
    "skill_community_install",
];

/// The call's answer as a body. `None` only where the toolset is documented not to register the
/// tool: any other tool that is missing fails the test, so no case is skipped without a reason.
async fn answer(rig: &Rig, tool: &str, args: &Value) -> Option<Value> {
    let names = rig.tools.registered_tool_names();
    if !names.contains(tool) {
        let merged = names.contains("iris_containers");
        assert!(
            merged && ABSENT_FROM_MERGED.contains(&tool),
            "{tool} is not registered, and the toolset is not documented to leave it out"
        );
        return None;
    }
    match rig.tools.call_gated(tool, args.clone()).await {
        Ok(r) => Some(body_of(&r)),
        // An error that is not a refusal: the call went past the gates and failed later.
        Err(e) => Some(json!({"error": e, "went_past_the_gates": true})),
    }
}

fn rigs(toml: &str) -> Vec<Rig> {
    [Toolset::Baseline, Toolset::Merged]
        .into_iter()
        .map(|t| rig_with_skills(toml, t, &[("prod", UNROUTABLE)], false))
        .collect()
}

// ── the table ───────────────────────────────────────────────────────────────────────────────────

#[test]
fn the_tools_that_run_code_on_the_default_server_are_iris_tools_on_the_default_server() {
    for case in iris_cases() {
        let row = tool_class(case.tool).unwrap_or_else(|| panic!("{} has no row", case.tool));
        assert_eq!(
            row.kind,
            TargetKind::Iris,
            "{} is judged as local",
            case.tool
        );
        assert_eq!(row.target, TargetSource::Default, "{}", case.tool);
        assert_eq!(is_scratch(case.tool), case.scratch, "{}", case.tool);
    }
}

#[test]
fn the_tools_that_never_call_iris_stay_local() {
    for case in local_cases() {
        let row = tool_class(case.tool).unwrap_or_else(|| panic!("{} has no row", case.tool));
        assert_eq!(
            row.kind,
            TargetKind::Local,
            "{} is judged as IRIS",
            case.tool
        );
        assert!(!is_scratch(case.tool), "{}", case.tool);
    }
}

#[test]
fn the_write_tiers_are_what_the_code_path_does() {
    use WriteClass::*;
    for (tool, args, want) in [
        ("skill_list", json!({}), ReadOnly),
        ("skill_describe", json!({"name": "x"}), ReadOnly),
        ("skill_search", json!({"query": "x"}), ReadOnly),
        ("skill_forget", json!({"name": "x"}), Destructive),
        ("kb_index", json!({}), Write),
        ("skill", json!({"action": "list"}), ReadOnly),
        (
            "skill",
            json!({"action": "forget", "name": "x"}),
            Destructive,
        ),
        ("skill", json!({"action": "propose"}), Write),
        (
            "skill_community",
            json!({"action": "install", "package": "p"}),
            Write,
        ),
        ("kb", json!({"action": "recall", "query": "x"}), ReadOnly),
        ("kb", json!({"action": "index"}), Write),
    ] {
        let m = args.as_object().cloned();
        assert_eq!(classify(tool, m.as_ref()), Some(want), "{tool} {args}");
    }
}

/// `allow` names the tool's own category, not `execute`: an operator who lists `skill` and `kb`
/// has asked for them. The environment template is judged as `execute` (see below).
#[test]
fn allow_is_judged_on_the_skill_and_kb_categories() {
    for (tool, cat) in [
        ("skill_list", ToolCategory::Skill),
        ("skill", ToolCategory::Skill),
        ("agent_info", ToolCategory::Skill),
        ("kb", ToolCategory::Kb),
        ("kb_index", ToolCategory::Kb),
    ] {
        assert_eq!(
            effective_category(tool, None, WriteClass::ReadOnly),
            vec![cat],
            "{tool}"
        );
    }
}

// ── the gates ───────────────────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn the_live_and_test_templates_refuse_them() {
    for template in ["live", "test"] {
        let toml = format!("{GATES_OPEN}[policy.default]\nmcpTemplate = \"{template}\"\n");
        for rig in rigs(&toml) {
            for case in iris_cases() {
                let Some(body) = answer(&rig, case.tool, &case.args).await else {
                    continue;
                };
                assert_eq!(
                    body["error_code"], "ENV_GATE_BLOCKED",
                    "{template} {} {}: {body}",
                    case.tool, case.args
                );
                assert_eq!(body["blocked_category"], "execute", "{}", case.tool);
            }
        }
    }
}

#[tokio::test]
async fn a_narrow_allow_refuses_them() {
    let toml = format!("{GATES_OPEN}[policy.default]\nallow = [\"query\", \"search\"]\n");
    for rig in rigs(&toml) {
        for case in iris_cases() {
            let Some(body) = answer(&rig, case.tool, &case.args).await else {
                continue;
            };
            assert_eq!(
                body["error_code"], "POLICY_GATE",
                "{} {}: {body}",
                case.tool, case.args
            );
            assert_eq!(body["refusal"]["kind"], "operator_limit", "{}", case.tool);
            let want = match tool_class(case.tool).map(|_| case.tool) {
                Some("kb" | "kb_index") => "kb",
                _ => "skill",
            };
            assert_eq!(body["blocked_category"], want, "{}: {body}", case.tool);
        }
    }
}

#[tokio::test]
async fn an_operator_who_allows_skill_and_kb_keeps_them() {
    let toml = format!("{GATES_OPEN}[policy.default]\nallow = [\"skill\", \"kb\"]\n");
    for rig in rigs(&toml) {
        for case in iris_cases() {
            let Some(body) = answer(&rig, case.tool, &case.args).await else {
                continue;
            };
            assert!(!refused(&body), "{} {}: {body}", case.tool, case.args);
        }
    }
}

#[tokio::test]
async fn the_dev_template_with_no_allow_keeps_them() {
    // The control: nothing above is refused because the tools stopped working.
    for rig in rigs(GATES_OPEN) {
        for case in iris_cases() {
            let Some(body) = answer(&rig, case.tool, &case.args).await else {
                continue;
            };
            assert!(!refused(&body), "{} {}: {body}", case.tool, case.args);
        }
    }
}

#[tokio::test]
async fn the_default_servers_policy_judges_them_not_another_servers() {
    // `prod` locks itself down and the default is open: the call names no server.
    let toml = format!("{GATES_OPEN}{PROD_LOCKED}");
    for rig in rigs(&toml) {
        for case in iris_cases() {
            let Some(body) = answer(&rig, case.tool, &case.args).await else {
                continue;
            };
            assert!(!refused(&body), "{} {}: {body}", case.tool, case.args);
        }
    }
}

#[tokio::test]
async fn strict_refuses_the_scratch_class_ones_and_leaves_the_terminal_ones() {
    for rig in [Toolset::Baseline, Toolset::Merged].map(|t| {
        rig_with_skills(
            "read_only = \"strict\"\n",
            t,
            &[("prod", UNROUTABLE)],
            false,
        )
    }) {
        for case in iris_cases() {
            let Some(body) = answer(&rig, case.tool, &case.args).await else {
                continue;
            };
            let destructive = matches!(
                (case.tool, case.args["action"].as_str()),
                ("skill_forget", _) | ("skill", Some("forget"))
            );
            let writes = matches!(
                (case.tool, case.args["action"].as_str()),
                ("kb_index", _) | ("skill", Some("propose")) | ("kb", Some("index"))
            ) || (case.tool == "skill_community" && case.args["action"] == "install");
            if case.scratch || destructive || writes {
                assert_eq!(
                    body["error_code"], "READ_ONLY_STRICT",
                    "{} {}: {body}",
                    case.tool, case.args
                );
            } else {
                assert!(
                    !refused(&body),
                    "strict refused {} {}, which compiles nothing: {body}",
                    case.tool,
                    case.args
                );
            }
        }
    }
}

#[tokio::test]
async fn the_write_gate_judges_the_writers() {
    // Writes off: the writers are refused by the write gate and the readers are not.
    let mut judged = 0;
    for rig in rigs("write_tools_enabled = false\n") {
        for (tool, args) in [
            ("kb_index", json!({})),
            ("skill", json!({"action": "propose"})),
            ("kb", json!({"action": "index"})),
            (
                "skill_community",
                json!({"action": "install", "package": "p"}),
            ),
        ] {
            let Some(body) = answer(&rig, tool, &args).await else {
                continue;
            };
            judged += 1;
            assert_eq!(
                body["error_code"], "WRITE_TOOLS_DISABLED",
                "{tool} {args}: {body}"
            );
        }
        for (tool, args) in [
            ("skill_list", json!({})),
            ("skill", json!({"action": "list"})),
            ("kb", json!({"action": "recall", "query": "x"})),
        ] {
            let Some(body) = answer(&rig, tool, &args).await else {
                continue;
            };
            assert!(!refused(&body), "{tool} {args}: {body}");
        }
    }
    assert!(judged >= 4, "only {judged} writer calls were judged");
}

#[tokio::test]
async fn the_local_ones_are_not_gated_by_template_allow_or_strict() {
    for toml in [
        format!("{GATES_OPEN}[policy.default]\nmcpTemplate = \"live\"\n"),
        format!("{GATES_OPEN}[policy.default]\nallow = [\"query\"]\n"),
        "read_only = \"strict\"\n".to_string(),
    ] {
        for rig in rigs(&toml) {
            for case in local_cases() {
                let Some(body) = answer(&rig, case.tool, &case.args).await else {
                    continue;
                };
                let code = body["error_code"].as_str();
                // The write gate still judges the four stubs, which are writes, and strict closes
                // it. That is the only refusal they may meet.
                if code == Some("READ_ONLY_STRICT") {
                    assert_eq!(body["gate"], "write_tools", "{}: {body}", case.tool);
                    assert_eq!(
                        classify(case.tool, None),
                        Some(WriteClass::Write),
                        "{}",
                        case.tool
                    );
                    continue;
                }
                assert!(
                    !matches!(code, Some("POLICY_GATE") | Some("ENV_GATE_BLOCKED")),
                    "{} was gated under {toml:?}: {body}",
                    case.tool
                );
            }
        }
    }
}

/// An operator file that does not parse closes every tool that talks to IRIS. The family runs code
/// on the default server, so it is closed with them; the registry-only tools still answer.
#[tokio::test]
async fn a_file_that_does_not_parse_closes_them_with_the_rest() {
    for rig in rigs("this is = = not toml\n") {
        for case in iris_cases() {
            let Some(body) = answer(&rig, case.tool, &case.args).await else {
                continue;
            };
            assert_eq!(
                body["error_code"], "CONFIG_INVALID",
                "{} {}: {body}",
                case.tool, case.args
            );
        }
        // The two registry-only tools on the tolerant list still answer.
        for (tool, args) in [
            ("kb_recall", json!({"query": "x"})),
            ("skill_community_list", json!({})),
        ] {
            let body = answer(&rig, tool, &args).await.unwrap();
            assert_ne!(body["error_code"], "CONFIG_INVALID", "{tool}: {body}");
        }
    }
}

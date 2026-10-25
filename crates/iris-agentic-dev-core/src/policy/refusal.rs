//! One refusal shape for every gate (spec 141, R4a D7, FR-039).
//!
//! A gate keeps returning its own JSON body. The pipeline calls [`classify_refusal`], which adds
//! `refusal {kind, server, lift, set_aside}` and leaves every other field where it was. Only the
//! pipeline knows where a setting came from, so only it can say whether an untrusted file's value
//! would have let the call through.

use crate::iris::config_sources::SetAsideItem;
use serde_json::{json, Value};
use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RefusalKind {
    AcknowledgementRequired,
    OperatorLimit,
    UntrustedSetting,
    FixedRule,
}

impl RefusalKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            RefusalKind::AcknowledgementRequired => "acknowledgement_required",
            RefusalKind::OperatorLimit => "operator_limit",
            RefusalKind::UntrustedSetting => "untrusted_setting",
            RefusalKind::FixedRule => "fixed_rule",
        }
    }
}

/// Every `error_code` that is a refusal and therefore carries `refusal`.
///
/// Input errors (`UNKNOWN_PARAMETER`, `SERVER_NOT_FOUND`, `CONFIG_INVALID`), capability errors and
/// every IRIS error are not refusals and are not here.
pub const REFUSAL_CODES: &[&str] = &[
    "WRITE_TOOLS_DISABLED",
    "DESTRUCTIVE_TOOLS_DISABLED",
    "ENV_GATE_BLOCKED",
    "POLICY_GATE",
    "DATA_POLICY_BLOCKED",
    "PHI_POLICY_BLOCKED",
    "PHI_GATE_BLOCKED",
    "PHI_ACK_REQUIRED",
    "SYSTEM_BLOCKLIST",
    "CODE_EDIT_BLOCKED",
    "COMPILE_TIME_EXEC_BLOCKED",
    "COMPILE_SUPERCLASS_STALE",
    "SUBJECT_CONFIRM_REQUIRED",
    "SUBJECT_SCM_WRITE_BLOCKED",
    "SQL_WRITE_BLOCKED",
    "CONFIRM_REQUIRED",
    "CONFIRMATION_REQUIRED",
    "ADMIN_WRITE_DISABLED",
    "CHECKIN_BLOCKED",
    "SCM_ADMIN_BLOCKED",
    "SCM_ACTION_UNPARSEABLE",
    "SCM_ACTION_UNKNOWN",
    "SCM_NEEDS_UI",
    "SCM_NEEDS_INPUT",
    "SERVICE_ACCOUNT_WITHHELD",
    "SERVICE_ACCOUNT_UNCONFIGURED_FOR_SERVER",
    "SERVICE_ACCOUNT_HTTP_DENIED",
    "SERVICE_ACCOUNT_LOGIN_FAILED",
    "SERVICE_ACCOUNT_IDENTITY_MISMATCH",
    "SERVICE_ACCOUNT_TERMINAL_DENIED",
    "SESSION_IDENTITY_CHANGED",
    "DOCKER_TERMINAL_LOGIN_REQUIRED",
    "NO_DOCKER_ROUTE_FOR_SERVER",
    "SSH_HOST_KEY_MISMATCH",
    "NAME_CONFLICT",
    "READ_ONLY_STRICT",
    "BODY_NOT_REDACTABLE",
    "STORED_SOURCE_UNREADABLE",
    "HOST_PATH_BLOCKED",
];

/// The setting behind a refusal of this code, when it is a configurable limit.
enum Lift {
    /// A policy part the refusal can be traced to in the set-aside list.
    Part {
        part: &'static str,
        setting: fn(&str) -> String,
    },
    /// A launch setting or a fixed place outside the policy parts.
    Launch(&'static str),
}

fn policy_key(key: &'static str) -> impl Fn(&str) -> String {
    move |server| format!("[policy.{server}] {key}")
}

fn setting_template(s: &str) -> String {
    policy_key("mcpTemplate")(s)
}
fn setting_allow(s: &str) -> String {
    policy_key("allow")(s)
}
fn setting_data(s: &str) -> String {
    policy_key("dataPolicy")(s)
}
fn setting_blocklist(s: &str) -> String {
    policy_key("globalBlocklist")(s)
}
fn setting_write(s: &str) -> String {
    if s == "default" {
        "write_tools_enabled".to_string()
    } else {
        policy_key("writeTools")(s)
    }
}
fn setting_destructive(s: &str) -> String {
    if s == "default" {
        "destructive_tools_enabled".to_string()
    } else {
        policy_key("destructiveTools")(s)
    }
}
fn setting_compile_gate(s: &str) -> String {
    if s == "default" {
        "compile_time_gate".to_string()
    } else {
        policy_key("compileTimeGate")(s)
    }
}

/// The kind of a code that is not decided by provenance.
enum Fixed {
    Acknowledge(&'static str),
    Rule,
    Configurable(Lift),
}

fn describe(code: &str, body: &Value) -> Option<Fixed> {
    use Fixed::*;
    Some(match code {
        "PHI_GATE_BLOCKED" | "PHI_ACK_REQUIRED" => Acknowledge("acknowledgePhi"),
        "SUBJECT_CONFIRM_REQUIRED" | "CONFIRM_REQUIRED" | "CONFIRMATION_REQUIRED" => {
            Acknowledge("confirm")
        }
        "CODE_EDIT_BLOCKED"
        | "SUBJECT_SCM_WRITE_BLOCKED"
        | "SQL_WRITE_BLOCKED"
        | "COMPILE_SUPERCLASS_STALE"
        | "SCM_ACTION_UNPARSEABLE"
        | "SCM_ACTION_UNKNOWN"
        | "SCM_NEEDS_UI"
        | "SCM_NEEDS_INPUT"
        | "SSH_HOST_KEY_MISMATCH"
        | "STORED_SOURCE_UNREADABLE"
        | "HOST_PATH_BLOCKED" => Rule,
        "SYSTEM_BLOCKLIST" => {
            if body["custom_pattern"].as_bool() == Some(true) {
                Configurable(Lift::Part {
                    part: "blocklist",
                    setting: setting_blocklist,
                })
            } else {
                Rule
            }
        }
        "WRITE_TOOLS_DISABLED" => Configurable(Lift::Part {
            part: "write_tools",
            setting: setting_write,
        }),
        "DESTRUCTIVE_TOOLS_DISABLED" => Configurable(Lift::Part {
            part: "destructive_tools",
            setting: setting_destructive,
        }),
        "ENV_GATE_BLOCKED" => Configurable(Lift::Part {
            part: "mcp_template",
            setting: setting_template,
        }),
        "POLICY_GATE" => Configurable(Lift::Part {
            part: "allow",
            setting: setting_allow,
        }),
        "DATA_POLICY_BLOCKED" | "PHI_POLICY_BLOCKED" | "BODY_NOT_REDACTABLE" => {
            Configurable(Lift::Part {
                part: "data_policy",
                setting: setting_data,
            })
        }
        "COMPILE_TIME_EXEC_BLOCKED" => Configurable(Lift::Part {
            part: "compile_gate",
            setting: setting_compile_gate,
        }),
        "ADMIN_WRITE_DISABLED" => Configurable(Lift::Launch("IRIS_ADMIN_TOOLS")),
        "CHECKIN_BLOCKED" => Configurable(Lift::Launch("IRIS_SCM_ALLOW_CHECKIN")),
        "SCM_ADMIN_BLOCKED" => Configurable(Lift::Launch("IRIS_SCM_ALLOW_DISCONNECT")),
        "READ_ONLY_STRICT" => Configurable(Lift::Launch("read_only")),
        "NAME_CONFLICT" => Configurable(Lift::Launch("server name")),
        c if c.starts_with("SERVICE_ACCOUNT_")
            || c == "SESSION_IDENTITY_CHANGED"
            || c == "DOCKER_TERMINAL_LOGIN_REQUIRED"
            || c == "NO_DOCKER_ROUTE_FOR_SERVER" =>
        {
            Configurable(Lift::Launch("service_username"))
        }
        _ => return None,
    })
}

/// Would the value an untrusted file set aside have let this call through?
fn would_permit(code: &str, body: &Value, item: &SetAsideItem) -> bool {
    let value = item
        .setting
        .split_once(" = ")
        .map(|x| x.1)
        .unwrap_or("")
        .trim();
    let unquoted = value.trim_matches('"');
    match code {
        "WRITE_TOOLS_DISABLED" | "DESTRUCTIVE_TOOLS_DISABLED" => value == "true",
        "COMPILE_TIME_EXEC_BLOCKED" => value == "false",
        "ENV_GATE_BLOCKED" => match unquoted {
            "dev" => true,
            "test" => !matches!(
                body["blocked_category"].as_str(),
                Some("execute") | Some("compile")
            ),
            _ => false,
        },
        "POLICY_GATE" => body["blocked_category"]
            .as_str()
            .is_some_and(|c| value.contains(&format!("\"{c}\""))),
        "DATA_POLICY_BLOCKED" | "PHI_POLICY_BLOCKED" => unquoted == "allow",
        _ => false,
    }
}

/// Add `refusal` to a gate body. A body whose `error_code` is not a refusal comes back unchanged.
pub fn classify_refusal(mut body: Value, server: &str, set_aside: &[SetAsideItem]) -> Value {
    let Some(code) = body["error_code"].as_str().map(str::to_string) else {
        return body;
    };
    if !REFUSAL_CODES.contains(&code.as_str()) {
        return body;
    }
    let Some(fixed) = describe(&code, &body) else {
        return body;
    };

    // `lift` names the setting and nothing else. The file and the edit that lifts a limit are the
    // operator's to know: they go to stderr and `trust --show` through [`refusal_remedy`]
    // (FR-039, 2026-10-24). An agent that is handed them makes the edit itself.
    let (kind, lift, set_aside_id) = match fixed {
        Fixed::Acknowledge(arg) => (
            RefusalKind::AcknowledgementRequired,
            Some(json!({ "argument": arg })),
            None,
        ),
        Fixed::Rule => (RefusalKind::FixedRule, None, None),
        Fixed::Configurable(Lift::Launch(setting)) => (
            RefusalKind::OperatorLimit,
            Some(json!({ "setting": setting })),
            None,
        ),
        Fixed::Configurable(Lift::Part { part, setting }) => {
            let setting_text = setting(server);
            let item = set_aside.iter().find(|i| {
                i.server.as_deref() == Some(server)
                    && i.part == Some(part)
                    && would_permit(&code, &body, i)
            });
            match item {
                Some(i) => (
                    RefusalKind::UntrustedSetting,
                    Some(json!({ "setting": setting_text })),
                    Some(i.id.clone()),
                ),
                None => (
                    RefusalKind::OperatorLimit,
                    Some(json!({ "setting": setting_text })),
                    None,
                ),
            }
        }
    };

    let mut refusal = json!({
        "kind": kind.as_str(),
        "server": server,
        "lift": lift.unwrap_or(Value::Null),
    });
    if let Some(id) = set_aside_id {
        refusal["set_aside"] = json!(id);
    }

    let obj = body.as_object_mut().expect("a gate body is an object");
    obj.entry("success").or_insert(json!(false));
    if !obj.contains_key("error") {
        let text = obj
            .get("message")
            .and_then(|m| m.as_str())
            .unwrap_or(&code)
            .to_string();
        obj.insert("error".to_string(), json!(text));
    }
    obj.insert("refusal".to_string(), refusal);
    body
}

// ── The operator's half: what to change to lift a refusal ──────────────────────────────────────

/// What the operator needs to lift a refusal, for stderr and `iris-agentic-dev trust --show`.
/// Never part of a result the agent reads.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Remedy {
    /// Identifies the setting on its server, so stderr says it once per process.
    pub key: String,
    pub line: String,
}

/// The assignment that lifts a refusal with this code, and a launch-setting remedy where the
/// limit is not a file setting. `None` for codes nobody lifts from configuration.
fn edit_for(code: &str, server: &str, body: &Value) -> Option<String> {
    let default = server == "default";
    let policy = |key: &str, value: &str| format!("[policy.{server}] {key} = {value}");
    Some(match code {
        "WRITE_TOOLS_DISABLED" if default => "write_tools_enabled = true".to_string(),
        "WRITE_TOOLS_DISABLED" => policy("writeTools", "true"),
        "DESTRUCTIVE_TOOLS_DISABLED" if default => "destructive_tools_enabled = true".to_string(),
        "DESTRUCTIVE_TOOLS_DISABLED" => policy("destructiveTools", "true"),
        "COMPILE_TIME_EXEC_BLOCKED" if default => "compile_time_gate = false".to_string(),
        "COMPILE_TIME_EXEC_BLOCKED" => policy("compileTimeGate", "false"),
        "ENV_GATE_BLOCKED" => policy("mcpTemplate", "\"dev\" (or \"test\")"),
        "POLICY_GATE" => policy(
            "allow",
            &format!(
                "[..., \"{}\"]",
                body["blocked_category"].as_str().unwrap_or("<category>")
            ),
        ),
        "DATA_POLICY_BLOCKED" | "PHI_POLICY_BLOCKED" | "BODY_NOT_REDACTABLE" => {
            policy("dataPolicy", "\"redact\" (or \"allow\")")
        }
        "SYSTEM_BLOCKLIST" => format!(
            "remove the matching pattern from [policy.{server}] globalBlocklist, or for a kill \
             add it to dataPolicyKillAllowlist"
        ),
        "ADMIN_WRITE_DISABLED" => "set IRIS_ADMIN_TOOLS=1 in the environment iris-agentic-dev is \
                                    launched with"
            .to_string(),
        "CHECKIN_BLOCKED" => "set IRIS_SCM_ALLOW_CHECKIN=1 in the environment iris-agentic-dev is \
                              launched with"
            .to_string(),
        "SCM_ADMIN_BLOCKED" => "set IRIS_SCM_ALLOW_DISCONNECT=1 in the environment \
                                iris-agentic-dev is launched with"
            .to_string(),
        "READ_ONLY_STRICT" => {
            "remove read_only = \"strict\" from the operator file, or unset IAD_READ_ONLY"
                .to_string()
        }
        "NAME_CONFLICT" => "choose a name that no Server Manager profile uses".to_string(),
        c if c.starts_with("SERVICE_ACCOUNT_")
            || c == "SESSION_IDENTITY_CHANGED"
            || c == "DOCKER_TERMINAL_LOGIN_REQUIRED"
            || c == "NO_DOCKER_ROUTE_FOR_SERVER" =>
        {
            format!(
                "give server {server} a service_username, or opt in to the container's OS user \
                 with exec_as_os_user = true"
            )
        }
        _ => return None,
    })
}

fn operator_path_text(operator_file: Option<&Path>) -> String {
    operator_file
        .map(|p| p.display().to_string())
        .unwrap_or_else(|| "~/.iris-agentic-dev.toml".to_string())
}

/// The sentence that tells the operator how to lift a configurable limit.
fn operator_sentence(edit: &str, operator_file: Option<&Path>) -> String {
    format!(
        "to lift it, put `{edit}` in {} (the operator file) and restart iris-agentic-dev; an \
         edit that loosens a setting applies at the next start. A project file counts after \
         you run `iris-agentic-dev trust <file>` in a terminal and restart",
        operator_path_text(operator_file)
    )
}

/// The remedy for a refusal [`classify_refusal`] has classified, or `None` when the operator has
/// nothing to do (an acknowledgement, a fixed rule, a body that is not a refusal).
pub fn refusal_remedy(
    refused: &Value,
    operator_file: Option<&Path>,
    set_aside: &[SetAsideItem],
) -> Option<Remedy> {
    let code = refused["error_code"].as_str()?;
    let r = refused.get("refusal")?;
    let server = r["server"].as_str()?;
    match r["kind"].as_str()? {
        "operator_limit" => {
            let setting = r["lift"]["setting"].as_str()?;
            let edit = edit_for(code, server, refused)?;
            Some(Remedy {
                key: format!("{server}:{setting}"),
                line: format!(
                    "iris-agentic-dev: {code} on server {server} ({setting}): {}",
                    operator_sentence(&edit, operator_file)
                ),
            })
        }
        "untrusted_setting" => {
            let setting = r["lift"]["setting"].as_str()?;
            let item = set_aside
                .iter()
                .find(|i| Some(i.id.as_str()) == r["set_aside"].as_str())?;
            Some(Remedy {
                key: format!("{server}:{setting}"),
                line: format!(
                    "iris-agentic-dev: {code} on server {server} ({setting}): {} in {} is set \
                     aside; {}",
                    item.setting,
                    item.file.as_deref().unwrap_or("a project file"),
                    item.lift
                ),
            })
        }
        _ => None,
    }
}

/// Write a remedy to stderr, once per setting per process. Plain `eprintln!`: `RUST_LOG` can
/// silence tracing, and the operator should not have to know that to learn why a call was refused.
pub fn announce_remedy(remedy: &Remedy) {
    static SAID: std::sync::Mutex<std::collections::BTreeSet<String>> =
        std::sync::Mutex::new(std::collections::BTreeSet::new());
    if SAID.lock().unwrap().insert(remedy.key.clone()) {
        eprintln!("{}", remedy.line);
    }
}

/// Every configurable limit and the edit that lifts it, for `iris-agentic-dev trust --show`.
pub fn remedy_catalogue(operator_file: Option<&Path>) -> Vec<String> {
    const CODES: &[&str] = &[
        "WRITE_TOOLS_DISABLED",
        "DESTRUCTIVE_TOOLS_DISABLED",
        "COMPILE_TIME_EXEC_BLOCKED",
        "ENV_GATE_BLOCKED",
        "POLICY_GATE",
        "DATA_POLICY_BLOCKED",
        "SYSTEM_BLOCKLIST",
        "READ_ONLY_STRICT",
        "ADMIN_WRITE_DISABLED",
        "CHECKIN_BLOCKED",
        "SCM_ADMIN_BLOCKED",
        "SERVICE_ACCOUNT_UNCONFIGURED_FOR_SERVER",
    ];
    let body = Value::Null;
    let mut out = Vec::new();
    for code in CODES {
        for server in ["default", "<server>"] {
            let Some(edit) = edit_for(code, server, &body) else {
                continue;
            };
            let line = format!("{code:<42} {server:<9} {edit}");
            if !out.contains(&line) {
                out.push(line);
            }
        }
    }
    out.push(format!(
        "(put these in {}, then restart iris-agentic-dev)",
        operator_path_text(operator_file)
    ));
    out
}

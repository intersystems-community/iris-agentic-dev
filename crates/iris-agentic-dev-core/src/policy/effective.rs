//! Effective policy: the operator file and the workspace file combined part by part
//! (spec 141, R4a D5, D6, D9).
//!
//! Pure functions. An untrusted or lapsed file only tightens; a trusted file's part replaces the
//! operator file's; blocklists and `irisAudit` always combine.

use crate::iris::config_sources::{trust_command_hint, SetAsideItem, SetAsideKind, TrustState};
use crate::iris::connection::SystemMode;
use crate::iris::workspace_config::{
    parse_role, ConnectionPolicy, ConnectionRole, DataPolicy, McpTemplate, ToolCategory,
};
use std::path::{Path, PathBuf};

/// Where a policy part's value came from. Serialises as the `from` enum of
/// `contracts/check-config.schema.json`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PartSource {
    Launch,
    OperatorFile,
    TrustedFile,
    WorkspaceFile,
    SystemMode,
    StrictDefault,
}

impl PartSource {
    pub fn as_str(&self) -> &'static str {
        match self {
            PartSource::Launch => "launch",
            PartSource::OperatorFile => "operator_file",
            PartSource::TrustedFile => "trusted_file",
            PartSource::WorkspaceFile => "workspace_file",
            PartSource::SystemMode => "system_mode",
            PartSource::StrictDefault => "strict_default",
        }
    }
}

/// One part of an effective policy with its provenance.
#[derive(Debug, Clone, PartialEq)]
pub struct Part<T> {
    pub value: T,
    pub from: PartSource,
    /// Further sources when the part combines files (blocklists, `irisAudit`).
    pub also_from: Vec<PartSource>,
    pub file: Option<PathBuf>,
    /// Ids of the set-aside items for this part.
    pub set_aside: Vec<String>,
}

impl<T> Part<T> {
    pub fn new(value: T, from: PartSource) -> Self {
        Self {
            value,
            from,
            also_from: Vec::new(),
            file: None,
            set_aside: Vec::new(),
        }
    }

    /// `{value, from, file?, set_aside?}` as `check_config` reports it.
    pub fn to_json(&self, value: serde_json::Value) -> serde_json::Value {
        let from = if self.also_from.is_empty() {
            serde_json::json!(self.from.as_str())
        } else {
            let mut all = vec![self.from.as_str()];
            all.extend(self.also_from.iter().map(|f| f.as_str()));
            serde_json::json!(all)
        };
        let mut o = serde_json::json!({ "value": value, "from": from });
        if let Some(f) = &self.file {
            o["file"] = serde_json::json!(f.to_string_lossy());
        }
        if !self.set_aside.is_empty() {
            o["set_aside"] = serde_json::json!(self.set_aside);
        }
        o
    }
}

/// How a server's address was chosen, which decides whether the operator's loosening follows it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Binding {
    OperatorSide,
    WorkspaceChosen { may_receive_credentials: bool },
    TrustedFileChosen,
}

impl Binding {
    fn withholds_operator_loosening(&self) -> bool {
        matches!(
            self,
            Binding::WorkspaceChosen {
                may_receive_credentials: false
            }
        )
    }
}

/// The values launch settings give, which outrank every file.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct LaunchPolicy {
    pub write_tools: Option<bool>,
    pub destructive_tools: Option<bool>,
    /// `Some(true)` is on, `Some(false)` is off (default server only).
    pub compile_gate: Option<bool>,
}

impl LaunchPolicy {
    /// `IAD_COMPILE_TIME_GATE`: `0`/`off` is off, `1`/`on` is on, anything else declares nothing.
    pub fn compile_gate_from_env_value(raw: &str) -> Option<bool> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "0" | "off" => Some(false),
            "1" | "on" => Some(true),
            _ => None,
        }
    }
}

/// A value a file declared, with the key it was declared under and the line it canonically reads.
#[derive(Debug, Clone, PartialEq)]
pub struct Declared<T> {
    pub value: T,
    /// The dotted key as written, e.g. `policy.prod.mcpTemplate`.
    pub key: String,
    /// `key = value`, for the set-aside list.
    pub text: String,
}

impl<T> Declared<T> {
    fn new(value: T, key: String, rendered: String) -> Self {
        let text = format!("{key} = {rendered}");
        Self { value, key, text }
    }
}

/// What one file declares about one server, parsed field by field.
///
/// Parsing is lenient and fails closed: an unknown template reads as `live`, an unknown data policy
/// as `block`, an unknown category is dropped from `allow`, an unknown role is a `subject`.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct PolicyLayer {
    pub mcp_template: Option<Declared<McpTemplate>>,
    pub allow: Option<Declared<Vec<ToolCategory>>>,
    pub data_policy: Option<Declared<DataPolicy>>,
    pub global_blocklist: Vec<String>,
    pub kill_allowlist: Option<Declared<Vec<String>>>,
    pub audit: bool,
    pub write_tools: Option<Declared<bool>>,
    pub destructive_tools: Option<Declared<bool>>,
    pub role: Option<Declared<ConnectionRole>>,
    /// `false` is the gate switched off.
    pub compile_gate: Option<Declared<bool>>,
}

fn q(s: &str) -> String {
    toml::Value::String(s.to_string()).to_string()
}

fn str_list(items: &[String]) -> String {
    format!(
        "[{}]",
        items.iter().map(|s| q(s)).collect::<Vec<_>>().join(", ")
    )
}

fn strings(v: &toml::Value) -> Option<Vec<String>> {
    Some(
        v.as_array()?
            .iter()
            .filter_map(|x| x.as_str().map(|s| s.trim().to_string()))
            .filter(|s| !s.is_empty())
            .collect(),
    )
}

impl PolicyLayer {
    /// Read the parts of `server` out of a file's raw table: `[policy.<server>]`,
    /// `[instance.<server>].role`, and for the default server the top-level gates.
    pub fn from_table(table: &toml::Table, server: &str, is_default: bool) -> PolicyLayer {
        let mut l = PolicyLayer::default();
        let pol = table
            .get("policy")
            .and_then(|p| p.get(server))
            .and_then(|p| p.as_table());
        let key = |k: &str| format!("policy.{server}.{k}");

        if let Some(p) = pol {
            if let Some(s) = p.get("mcpTemplate").and_then(|v| v.as_str()) {
                let t = match s.trim().to_ascii_lowercase().as_str() {
                    "dev" => McpTemplate::Dev,
                    "test" => McpTemplate::Test,
                    _ => McpTemplate::Live,
                };
                let name = template_name(&t);
                l.mcp_template = Some(Declared::new(t, key("mcpTemplate"), q(name)));
            }
            if let Some(v) = p.get("allow") {
                if let Some(items) = strings(v) {
                    let mut cats: Vec<ToolCategory> = Vec::new();
                    for s in &items {
                        if let Ok(c) = s.to_ascii_lowercase().parse::<ToolCategory>() {
                            if !cats.contains(&c) {
                                cats.push(c);
                            }
                        }
                    }
                    let names: Vec<String> = {
                        let mut n: Vec<String> =
                            cats.iter().map(|c| c.as_str().to_string()).collect();
                        n.sort();
                        n
                    };
                    l.allow = Some(Declared::new(cats, key("allow"), str_list(&names)));
                }
            }
            if let Some(s) = p.get("dataPolicy").and_then(|v| v.as_str()) {
                let d = match s.trim().to_ascii_lowercase().as_str() {
                    "allow" => DataPolicy::Allow,
                    "redact" => DataPolicy::Redact,
                    _ => DataPolicy::Block,
                };
                let name = data_policy_name(&d);
                l.data_policy = Some(Declared::new(d, key("dataPolicy"), q(name)));
            }
            if let Some(v) = p.get("globalBlocklist").and_then(strings) {
                l.global_blocklist = v;
            }
            if let Some(mut v) = p.get("dataPolicyKillAllowlist").and_then(strings) {
                v.sort();
                v.dedup();
                let text = str_list(&v);
                l.kill_allowlist = Some(Declared::new(v, key("dataPolicyKillAllowlist"), text));
            }
            l.audit = p
                .get("irisAudit")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            let flag = |k: &str| {
                p.get(k)
                    .and_then(|v| v.as_bool())
                    .map(|b| Declared::new(b, key(k), b.to_string()))
            };
            l.write_tools = flag("writeTools");
            l.destructive_tools = flag("destructiveTools");
            l.compile_gate = flag("compileTimeGate");
        }

        if is_default {
            // The top-level keys are the default server's row. When both spellings are present the
            // stricter one counts.
            let top = |k: &str| {
                table
                    .get(k)
                    .and_then(|v| v.as_bool())
                    .map(|b| Declared::new(b, k.to_string(), b.to_string()))
            };
            let combine = |a: Option<Declared<bool>>,
                           b: Option<Declared<bool>>,
                           strict_value: bool|
             -> Option<Declared<bool>> {
                match (a, b) {
                    (Some(a), Some(b)) => Some(if a.value == strict_value || a.value == b.value {
                        a
                    } else {
                        b
                    }),
                    (a, None) => a,
                    (None, b) => b,
                }
            };
            l.write_tools = combine(top("write_tools_enabled"), l.write_tools.take(), true);
            l.destructive_tools = combine(
                top("destructive_tools_enabled"),
                l.destructive_tools.take(),
                true,
            );
            // For the gate switch the strict value is `true` (on).
            l.compile_gate = combine(top("compile_time_gate"), l.compile_gate.take(), false);
        }

        if let Some(r) = table
            .get("instance")
            .and_then(|i| i.get(server))
            .and_then(|i| i.get("role"))
            .and_then(|r| r.as_str())
        {
            let (role, _) = parse_role(r);
            l.role = Some(Declared::new(
                role,
                format!("instance.{server}.role"),
                q(role.as_str()),
            ));
        }
        l
    }
}

fn template_name(t: &McpTemplate) -> &'static str {
    match t {
        McpTemplate::Dev => "dev",
        McpTemplate::Test => "test",
        McpTemplate::Live => "live",
    }
}

fn data_policy_name(d: &DataPolicy) -> &'static str {
    match d {
        DataPolicy::Block => "block",
        DataPolicy::Redact => "redact",
        DataPolicy::Allow => "allow",
    }
}

fn template_rank(t: &McpTemplate) -> u8 {
    match t {
        McpTemplate::Dev => 0,
        McpTemplate::Test => 1,
        McpTemplate::Live => 2,
    }
}

fn data_rank(d: &DataPolicy) -> u8 {
    match d {
        DataPolicy::Allow => 0,
        DataPolicy::Redact => 1,
        DataPolicy::Block => 2,
    }
}

/// Everything [`effective_policy`] reads.
pub struct PolicyInputs<'a> {
    pub server: &'a str,
    pub is_default: bool,
    pub operator: Option<&'a PolicyLayer>,
    pub workspace: Option<&'a PolicyLayer>,
    pub trust: &'a TrustState,
    pub binding: Binding,
    pub launch: LaunchPolicy,
    /// The template the target server's system mode implies, when that is known (R4a D6). An
    /// untrusted file's `mcpTemplate` is compared with it when the operator file declares none: a
    /// value at least as strict applies, a looser one is set aside. `None` compares with `live`,
    /// the strictest, which is the answer before any connection has been probed.
    pub system_template: Option<McpTemplate>,
    pub operator_file: Option<&'a Path>,
    pub workspace_file: Option<&'a Path>,
}

/// A server's policy after layering.
#[derive(Debug, Clone, PartialEq)]
pub struct EffectivePolicy {
    pub server: String,
    /// `None` means no file declared one: the system mode decides.
    pub mcp_template: Part<Option<McpTemplate>>,
    /// `None` means all categories.
    pub allow: Part<Option<Vec<ToolCategory>>>,
    pub data_policy: Part<DataPolicy>,
    pub global_blocklist: Part<Vec<String>>,
    pub kill_allowlist: Part<Vec<String>>,
    pub audit: Part<bool>,
    /// `None` means undeclared: the system mode decides.
    pub write_tools: Part<Option<bool>>,
    pub destructive_tools: Part<Option<bool>>,
    pub role: Part<ConnectionRole>,
    /// `true` is on.
    pub compile_gate: Part<bool>,
    pub set_aside: Vec<SetAsideItem>,
}

impl Default for EffectivePolicy {
    fn default() -> Self {
        let d = PartSource::StrictDefault;
        Self {
            server: String::new(),
            mcp_template: Part::new(None, PartSource::SystemMode),
            allow: Part::new(None, d),
            data_policy: Part::new(DataPolicy::Block, d),
            global_blocklist: Part::new(Vec::new(), d),
            kill_allowlist: Part::new(Vec::new(), d),
            audit: Part::new(false, d),
            write_tools: Part::new(None, PartSource::SystemMode),
            destructive_tools: Part::new(None, d),
            role: Part::new(ConnectionRole::default(), d),
            compile_gate: Part::new(true, d),
            set_aside: Vec::new(),
        }
    }
}

impl EffectivePolicy {
    /// The gate-facing form. An undeclared template stays `None`; the caller resolves it from the
    /// connection's system mode with [`implied_template`].
    pub fn to_connection_policy(&self) -> ConnectionPolicy {
        ConnectionPolicy {
            server_name: self.server.clone(),
            allow: self.allow.value.clone(),
            mcp_template: self.mcp_template.value.clone(),
            data_policy: Some(self.data_policy.value.clone()),
            global_blocklist: self.global_blocklist.value.clone(),
            data_policy_kill_allowlist: self.kill_allowlist.value.clone(),
            iris_audit: self.audit.value,
        }
    }

    /// The `effective_policy` entry of `check_config` for this server.
    pub fn to_json(&self) -> serde_json::Value {
        let cats = |c: &Option<Vec<ToolCategory>>| match c {
            Some(v) => serde_json::json!(v.iter().map(|c| c.as_str()).collect::<Vec<_>>()),
            None => serde_json::Value::Null,
        };
        serde_json::json!({
            "mcpTemplate": self.mcp_template.to_json(serde_json::json!(
                self.mcp_template.value.as_ref().map(template_name))),
            "allow": self.allow.to_json(cats(&self.allow.value)),
            "dataPolicy": self.data_policy.to_json(serde_json::json!(data_policy_name(&self.data_policy.value))),
            "globalBlocklist": self.global_blocklist.to_json(serde_json::json!(self.global_blocklist.value)),
            "irisAudit": self.audit.to_json(serde_json::json!(self.audit.value)),
            "writeTools": self.write_tools.to_json(serde_json::json!(self.write_tools.value)),
            "destructiveTools": self.destructive_tools.to_json(serde_json::json!(self.destructive_tools.value)),
            "role": self.role.to_json(serde_json::json!(self.role.value.as_str())),
            "compileTimeGate": self.compile_gate.to_json(serde_json::json!(
                if self.compile_gate.value { "on" } else { "off" })),
            "dataPolicyKillAllowlist": self.kill_allowlist.to_json(serde_json::json!(self.kill_allowlist.value)),
        })
    }
}

/// Why a looser value of an untrusted or lapsed file is not applied.
fn untrusted_reason(trust: &TrustState) -> String {
    match trust {
        TrustState::Lapsed { changed } => {
            let parts: Vec<String> = changed
                .iter()
                .map(|c| {
                    format!(
                        "{} changed from {} to {}",
                        c.setting,
                        c.was.as_deref().unwrap_or("(absent)"),
                        c.now.as_deref().unwrap_or("(absent)")
                    )
                })
                .collect();
            format!("trust lapsed: {}", parts.join("; "))
        }
        _ => "the file is not trusted".to_string(),
    }
}

struct Ctx<'a> {
    inp: &'a PolicyInputs<'a>,
    items: Vec<SetAsideItem>,
}

impl Ctx<'_> {
    fn trusted(&self) -> bool {
        self.inp.trust.is_trusted()
    }

    fn ws_file(&self) -> Option<PathBuf> {
        self.inp.workspace_file.map(|p| p.to_path_buf())
    }

    fn op_file(&self) -> Option<PathBuf> {
        self.inp.operator_file.map(|p| p.to_path_buf())
    }

    /// A workspace value that was not applied. Returns the item's id.
    fn set_aside_ws(&mut self, part: &'static str, d_key: &str, text: &str) -> String {
        let id = format!("workspace:{d_key}");
        if !self.items.iter().any(|i| i.id == id) {
            let ws = self.ws_file();
            self.items.push(SetAsideItem {
                id: id.clone(),
                part: Some(part),
                kind: SetAsideKind::UntrustedSetting,
                setting: text.to_string(),
                server: Some(self.inp.server.to_string()),
                file: ws.as_ref().map(|p| p.to_string_lossy().into_owned()),
                reason: untrusted_reason(self.inp.trust),
                lift: trust_command_hint(ws.as_deref()),
            });
        }
        id
    }

    /// An operator-side value that does not follow a server the workspace file chose.
    fn set_aside_bound(&mut self, part: &'static str, d_key: &str, text: &str) -> String {
        let id = format!("{}:{d_key}", self.inp.server);
        if !self.items.iter().any(|i| i.id == id) {
            let ws = self.ws_file();
            self.items.push(SetAsideItem {
                id: id.clone(),
                part: Some(part),
                kind: SetAsideKind::UntrustedSetting,
                setting: text.to_string(),
                server: Some(self.inp.server.to_string()),
                file: self.op_file().map(|p| p.to_string_lossy().into_owned()),
                reason: "the workspace file chose this server, so a looser setting from the \
                         operator's settings or launch settings does not apply to it"
                    .to_string(),
                lift: format!(
                    "{}, or set IRIS_HOST to this server",
                    trust_command_hint(ws.as_deref())
                ),
            });
        }
        id
    }
}

/// The merged value of one scalar part. `None` when neither layer applies.
struct Merged<T> {
    value: T,
    from: PartSource,
    file: Option<PathBuf>,
    set_aside: Vec<String>,
}

/// Merge one scalar part. `rank` orders values strictest-last; `absent_rank` is the rank the
/// untrusted file's value must reach when the operator file declares nothing.
fn merge_scalar<T: Clone + PartialEq>(
    cx: &mut Ctx,
    part: &'static str,
    op: Option<&Declared<T>>,
    ws: Option<&Declared<T>>,
    rank: impl Fn(&T) -> u8,
    absent_rank: u8,
) -> Option<Merged<T>> {
    let from_op = |o: &Declared<T>, cx: &Ctx, aside: Vec<String>| Merged {
        value: o.value.clone(),
        from: PartSource::OperatorFile,
        file: cx.op_file(),
        set_aside: aside,
    };
    match (op, ws) {
        (None, None) => None,
        (Some(o), None) => Some(from_op(o, cx, vec![])),
        (_, Some(w)) if cx.trusted() => Some(Merged {
            value: w.value.clone(),
            from: PartSource::TrustedFile,
            file: cx.ws_file(),
            set_aside: vec![],
        }),
        (o, Some(w)) => {
            let base = o.map(|o| rank(&o.value)).unwrap_or(absent_rank);
            if rank(&w.value) >= base {
                if let Some(o) = o.filter(|o| o.value == w.value) {
                    return Some(from_op(o, cx, vec![]));
                }
                Some(Merged {
                    value: w.value.clone(),
                    from: PartSource::WorkspaceFile,
                    file: cx.ws_file(),
                    set_aside: vec![],
                })
            } else {
                let id = cx.set_aside_ws(part, &w.key, &w.text);
                match o {
                    Some(o) => Some(from_op(o, cx, vec![id])),
                    // Nothing applies. The caller reads `StrictDefault` as "leave the part at its
                    // default" and keeps only the item id.
                    None => Some(Merged {
                        value: w.value.clone(),
                        from: PartSource::StrictDefault,
                        file: None,
                        set_aside: vec![id],
                    }),
                }
            }
        }
    }
}

/// The operator layer with the values that must not follow a workspace-chosen server removed.
fn strip_operator_loosening(cx: &mut Ctx, op: &PolicyLayer) -> PolicyLayer {
    let mut l = op.clone();
    if let Some(d) = l.mcp_template.as_ref() {
        if d.value != McpTemplate::Live {
            let (k, t) = (d.key.clone(), d.text.clone());
            cx.set_aside_bound("mcp_template", &k, &t);
            l.mcp_template = None;
        }
    }
    if let Some(d) = l.data_policy.as_ref() {
        if d.value != DataPolicy::Block {
            let (k, t) = (d.key.clone(), d.text.clone());
            cx.set_aside_bound("data_policy", &k, &t);
            l.data_policy = None;
        }
    }
    if let Some(d) = l.kill_allowlist.as_ref() {
        if !d.value.is_empty() {
            let (k, t) = (d.key.clone(), d.text.clone());
            cx.set_aside_bound("kill_allowlist", &k, &t);
            l.kill_allowlist = None;
        }
    }
    if let Some(d) = l.write_tools.as_ref() {
        if d.value {
            let (k, t) = (d.key.clone(), d.text.clone());
            cx.set_aside_bound("write_tools", &k, &t);
            l.write_tools = None;
        }
    }
    if let Some(d) = l.destructive_tools.as_ref() {
        if d.value {
            let (k, t) = (d.key.clone(), d.text.clone());
            cx.set_aside_bound("destructive_tools", &k, &t);
            l.destructive_tools = None;
        }
    }
    if let Some(d) = l.compile_gate.as_ref() {
        if !d.value {
            let (k, t) = (d.key.clone(), d.text.clone());
            cx.set_aside_bound("compile_gate", &k, &t);
            l.compile_gate = None;
        }
    }
    l
}

/// Combine the layers for one server.
pub fn effective_policy(inp: &PolicyInputs) -> EffectivePolicy {
    let mut cx = Ctx {
        inp,
        items: Vec::new(),
    };
    let mut out = EffectivePolicy {
        server: inp.server.to_string(),
        ..EffectivePolicy::default()
    };

    let op_owned;
    let op: Option<&PolicyLayer> = match inp.operator {
        Some(l) if inp.binding.withholds_operator_loosening() => {
            op_owned = strip_operator_loosening(&mut cx, l);
            Some(&op_owned)
        }
        other => other,
    };
    let ws = inp.workspace;
    let empty = PolicyLayer::default();
    let (o, w) = (op.unwrap_or(&empty), ws.unwrap_or(&empty));
    let trusted = cx.trusted();
    let ws_src = if trusted {
        PartSource::TrustedFile
    } else {
        PartSource::WorkspaceFile
    };

    // mcpTemplate
    if let Some(m) = merge_scalar(
        &mut cx,
        "mcp_template",
        o.mcp_template.as_ref(),
        w.mcp_template.as_ref(),
        template_rank,
        inp.system_template.as_ref().map(template_rank).unwrap_or(2),
    ) {
        out.mcp_template.set_aside = m.set_aside;
        if m.from != PartSource::StrictDefault {
            out.mcp_template.value = Some(m.value);
            out.mcp_template.from = m.from;
            out.mcp_template.file = m.file;
        }
    }

    // allow: an intersection until the file is trusted; no list means all categories.
    match (o.allow.as_ref(), w.allow.as_ref()) {
        (None, None) => {}
        (Some(a), None) => {
            out.allow.value = Some(a.value.clone());
            out.allow.from = PartSource::OperatorFile;
            out.allow.file = cx.op_file();
        }
        (_, Some(f)) if trusted => {
            out.allow.value = Some(f.value.clone());
            out.allow.from = PartSource::TrustedFile;
            out.allow.file = cx.ws_file();
        }
        (None, Some(f)) => {
            out.allow.value = Some(f.value.clone());
            out.allow.from = PartSource::WorkspaceFile;
            out.allow.file = cx.ws_file();
        }
        (Some(a), Some(f)) => {
            let kept: Vec<ToolCategory> = a
                .value
                .iter()
                .filter(|c| f.value.contains(c))
                .cloned()
                .collect();
            let added = f.value.iter().any(|c| !a.value.contains(c));
            if added {
                let id = cx.set_aside_ws("allow", &f.key, &f.text);
                out.allow.set_aside.push(id);
            }
            let narrowed = kept.len() != a.value.len();
            out.allow.from = if narrowed {
                PartSource::WorkspaceFile
            } else {
                PartSource::OperatorFile
            };
            out.allow.file = Some(if narrowed {
                cx.ws_file().unwrap_or_default()
            } else {
                cx.op_file().unwrap_or_default()
            })
            .filter(|p| !p.as_os_str().is_empty());
            out.allow.value = Some(kept);
        }
    }

    // dataPolicy
    if let Some(m) = merge_scalar(
        &mut cx,
        "data_policy",
        o.data_policy.as_ref(),
        w.data_policy.as_ref(),
        data_rank,
        2,
    ) {
        out.data_policy.set_aside = m.set_aside;
        if m.from != PartSource::StrictDefault {
            out.data_policy.value = m.value;
            out.data_policy.from = m.from;
            out.data_policy.file = m.file;
        }
    }

    // Blocklists and audit combine whatever the trust state.
    {
        let mut all: Vec<String> = o.global_blocklist.clone();
        all.extend(w.global_blocklist.iter().cloned());
        all.sort();
        all.dedup();
        if !all.is_empty() {
            out.global_blocklist.value = all;
            let (from, also) = match (o.global_blocklist.is_empty(), w.global_blocklist.is_empty())
            {
                (false, true) => (PartSource::OperatorFile, vec![]),
                (true, false) => (ws_src, vec![]),
                _ => (PartSource::OperatorFile, vec![ws_src]),
            };
            out.global_blocklist.from = from;
            out.global_blocklist.also_from = also;
        }
        if o.audit || w.audit {
            out.audit.value = true;
            out.audit.from = if o.audit {
                PartSource::OperatorFile
            } else {
                ws_src
            };
            if o.audit && w.audit {
                out.audit.also_from = vec![ws_src];
            }
        }
    }

    // The kill allowlist: the operator's, plus the file's once it is trusted.
    {
        let mut v: Vec<String> = o
            .kill_allowlist
            .as_ref()
            .map(|d| d.value.clone())
            .unwrap_or_default();
        let from_op = !v.is_empty();
        if let Some(f) = w.kill_allowlist.as_ref().filter(|f| !f.value.is_empty()) {
            if trusted {
                v.extend(f.value.iter().cloned());
                out.kill_allowlist.also_from = if from_op { vec![ws_src] } else { vec![] };
                out.kill_allowlist.from = if from_op {
                    PartSource::OperatorFile
                } else {
                    PartSource::TrustedFile
                };
                out.kill_allowlist.file = cx.ws_file();
            } else {
                let id = cx.set_aside_ws("kill_allowlist", &f.key, &f.text);
                out.kill_allowlist.set_aside.push(id);
            }
        }
        if from_op && out.kill_allowlist.from == PartSource::StrictDefault {
            out.kill_allowlist.from = PartSource::OperatorFile;
            out.kill_allowlist.file = cx.op_file();
        }
        v.sort();
        v.dedup();
        out.kill_allowlist.value = v;
    }

    // writeTools / destructiveTools: false beats true until the file is trusted.
    for (part, is_write, op_d, ws_d, launch) in [
        (
            "write_tools",
            true,
            o.write_tools.as_ref(),
            w.write_tools.as_ref(),
            inp.launch.write_tools,
        ),
        (
            "destructive_tools",
            false,
            o.destructive_tools.as_ref(),
            w.destructive_tools.as_ref(),
            inp.launch.destructive_tools,
        ),
    ] {
        let target = if is_write {
            &mut out.write_tools
        } else {
            &mut out.destructive_tools
        };
        let merged = merge_scalar(&mut cx, part, op_d, ws_d, |b| u8::from(!*b), 1);
        if let Some(m) = merged {
            target.set_aside = m.set_aside;
            if m.from != PartSource::StrictDefault {
                target.value = Some(m.value);
                target.from = m.from;
                target.file = m.file;
            }
        }
        if let Some(l) = launch {
            if l && inp.binding.withholds_operator_loosening() {
                let key = format!(
                    "launch.{}",
                    if is_write {
                        "write_tools_enabled"
                    } else {
                        "destructive_tools_enabled"
                    }
                );
                let text = format!("{key} = true");
                cx.set_aside_bound(part, &key, &text);
            } else {
                target.value = Some(l);
                target.from = PartSource::Launch;
                target.file = None;
            }
        }
    }

    // role
    if let Some(m) = merge_scalar(
        &mut cx,
        "role",
        o.role.as_ref(),
        w.role.as_ref(),
        |r| u8::from(*r == ConnectionRole::Subject),
        0,
    ) {
        out.role.set_aside = m.set_aside;
        if m.from != PartSource::StrictDefault {
            out.role.value = m.value;
            out.role.from = m.from;
            out.role.file = m.file;
        }
    }

    // The compile-time gate: on is strict.
    if let Some(m) = merge_scalar(
        &mut cx,
        "compile_gate",
        o.compile_gate.as_ref(),
        w.compile_gate.as_ref(),
        |on| u8::from(*on),
        1,
    ) {
        out.compile_gate.set_aside = m.set_aside;
        if m.from != PartSource::StrictDefault {
            out.compile_gate.value = m.value;
            out.compile_gate.from = m.from;
            out.compile_gate.file = m.file;
        }
    }
    match inp.launch.compile_gate {
        Some(true) => {
            out.compile_gate.value = true;
            out.compile_gate.from = PartSource::Launch;
            out.compile_gate.file = None;
        }
        Some(false) if inp.is_default => {
            if inp.binding.withholds_operator_loosening() {
                cx.set_aside_bound(
                    "compile_gate",
                    "launch.compile_time_gate",
                    "launch.compile_time_gate = false",
                );
            } else {
                out.compile_gate.value = false;
                out.compile_gate.from = PartSource::Launch;
                out.compile_gate.file = None;
            }
        }
        _ => {}
    }

    // Each part names its own items; the policy lists all of them, in a stable order.
    for item in &cx.items {
        let ids = match item.part {
            Some("mcp_template") => &mut out.mcp_template.set_aside,
            Some("allow") => &mut out.allow.set_aside,
            Some("data_policy") => &mut out.data_policy.set_aside,
            Some("kill_allowlist") => &mut out.kill_allowlist.set_aside,
            Some("write_tools") => &mut out.write_tools.set_aside,
            Some("destructive_tools") => &mut out.destructive_tools.set_aside,
            Some("role") => &mut out.role.set_aside,
            Some("compile_gate") => &mut out.compile_gate.set_aside,
            _ => continue,
        };
        if !ids.contains(&item.id) {
            ids.push(item.id.clone());
        }
    }
    out.set_aside = cx.items;
    out
}

/// The template a system mode implies when no file declares one (R4a D6).
///
/// `Live` and `Failover` read as live, `Test` as test, `Development` and `Unset` (a fresh
/// instance) as dev. An instance whose mode could not be read (`Unknown`) is judged by the
/// namespace for the default server and as live for a pool member, which has no connection of its
/// own to infer from.
pub fn implied_template(
    mode: SystemMode,
    is_default: bool,
    namespace: &str,
) -> (McpTemplate, PartSource) {
    let t = match mode {
        SystemMode::Live => McpTemplate::Live,
        SystemMode::Test => McpTemplate::Test,
        SystemMode::Development | SystemMode::Unset => McpTemplate::Dev,
        SystemMode::Unknown => {
            if !is_default || crate::tools::write_gate::is_production_namespace(namespace) {
                McpTemplate::Live
            } else {
                McpTemplate::Dev
            }
        }
    };
    (t, PartSource::SystemMode)
}

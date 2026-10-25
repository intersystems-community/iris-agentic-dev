//! A loosening edit to the operator file waits for the next start (spec 141 FR-043, T193).
//!
//! An agent with a file-write tool can write the operator file. If every reload applied what the
//! file says, the agent could raise a gate, drop `read_only = "strict"` or add a `[[trust]]` entry
//! and have it in force on the next call. So during a session an edit applies at once only where it
//! tightens; [`hold_loosening`] puts every loosening value back to what the server runs on and says
//! what it held. The operator restarts iad to take those edits up.
//!
//! Pure functions over parsed tables: the caller keeps the table the server runs on (the held
//! result of the last load) and passes it back with the file as it reads now. The first load of a
//! process has nothing to hold against.
//!
//! Absence is judged by what it implies. A gate that is absent is decided by the system mode, so
//! removing `writeTools = false` may open writes; the destructive tier, the compile-time gate and
//! the data policy have fixed defaults, so removing their strict value changes nothing.

use crate::iris::workspace_config::{ConnectionRole, DataPolicy, McpTemplate};
use crate::policy::effective::{Declared, PolicyLayer};
use std::collections::BTreeSet;
use toml::{Table, Value};

/// A loosening setting the file holds now and the server does not run on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Held {
    /// `operator:<dotted key>`, stable across reloads.
    pub id: String,
    /// The edit as the file reads now, or `<setting> removed`.
    pub setting: String,
}

impl Held {
    /// The one line iad writes to stderr when it holds this edit back.
    pub fn stderr_line(&self, operator_file: Option<&std::path::Path>) -> String {
        format!(
            "iris-agentic-dev: {} in {} loosens a running server and is held until \
             iris-agentic-dev restarts; edits that tighten apply at once",
            self.setting,
            operator_file.map_or_else(
                || "the operator file".to_string(),
                |p| p.display().to_string()
            ),
        )
    }
}

type Segs = Vec<String>;

fn segs(parts: &[&str]) -> Segs {
    parts.iter().map(|s| s.to_string()).collect()
}

fn get<'a>(t: &'a Table, path: &[String]) -> Option<&'a Value> {
    let (first, rest) = path.split_first()?;
    let mut cur = t.get(first)?;
    for p in rest {
        cur = cur.get(p)?;
    }
    Some(cur)
}

/// Put `value` at `path`, or remove the key (and any table that empties) when `None`.
fn set(t: &mut Table, path: &[String], value: Option<Value>) {
    let Some((first, rest)) = path.split_first() else {
        return;
    };
    if rest.is_empty() {
        match value {
            Some(v) => {
                t.insert(first.clone(), v);
            }
            None => {
                t.remove(first);
            }
        }
        return;
    }
    if value.is_none() && !matches!(t.get(first), Some(Value::Table(_))) {
        return;
    }
    let child = t
        .entry(first.clone())
        .or_insert_with(|| Value::Table(Table::new()));
    if let Value::Table(c) = child {
        set(c, rest, value);
        if c.is_empty() {
            t.remove(first);
        }
    }
}

fn dotted(path: &[String]) -> String {
    path.join(".")
}

struct Hold<'a> {
    applied: &'a Table,
    out: Table,
    held: Vec<Held>,
}

impl Hold<'_> {
    /// Put every path back to what the server runs on, and record `setting` under `id_key`.
    fn hold(&mut self, id_key: &str, setting: String, paths: &[Segs]) {
        for p in paths {
            let was = get(self.applied, p).cloned();
            set(&mut self.out, p, was);
        }
        let id = format!("operator:{id_key}");
        if !self.held.iter().any(|h| h.id == id) {
            self.held.push(Held { id, setting });
        }
    }

    /// The text for a part: the new declaration when there is one, else the applied one removed.
    fn hold_part<T>(
        &mut self,
        now: Option<&Declared<T>>,
        was: Option<&Declared<T>>,
        paths: &[Segs],
    ) {
        let (key, setting) = match (now, was) {
            (Some(n), _) => (n.key.clone(), n.text.clone()),
            (None, Some(w)) => (w.key.clone(), format!("{} removed", w.text)),
            (None, None) => return,
        };
        self.hold(&key, setting, paths);
    }
}

fn template_rank(d: Option<&Declared<McpTemplate>>, is_new: bool) -> u8 {
    match d.map(|d| &d.value) {
        Some(McpTemplate::Dev) => 0,
        Some(McpTemplate::Test) => 1,
        Some(McpTemplate::Live) => 2,
        // The system mode decides: as the new value it may be dev, as the old one it may be live.
        None => {
            if is_new {
                0
            } else {
                2
            }
        }
    }
}

fn write_rank(d: Option<&Declared<bool>>, is_new: bool) -> u8 {
    match d.map(|d| d.value) {
        Some(false) => 2,
        Some(true) => 0,
        None => {
            if is_new {
                0
            } else {
                2
            }
        }
    }
}

fn data_rank(d: Option<&Declared<DataPolicy>>) -> u8 {
    match d.map(|d| &d.value) {
        Some(DataPolicy::Allow) => 0,
        Some(DataPolicy::Redact) => 1,
        Some(DataPolicy::Block) | None => 2,
    }
}

fn str_at(t: &Table, path: &[String]) -> Option<String> {
    get(t, path)
        .and_then(Value::as_str)
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

fn bool_at(t: &Table, path: &[String]) -> Option<bool> {
    get(t, path).and_then(Value::as_bool)
}

fn server_names(a: &Table, b: &Table) -> Vec<String> {
    let mut names: BTreeSet<String> = BTreeSet::new();
    for t in [a, b] {
        for section in ["policy", "instance"] {
            if let Some(s) = t.get(section).and_then(Value::as_table) {
                names.extend(s.keys().cloned());
            }
        }
    }
    names.remove("default");
    std::iter::once("default".to_string())
        .chain(names)
        .collect()
}

fn policy_parts(h: &mut Hold, incoming: &Table, server: &str) {
    let is_default = server == "default";
    let was = PolicyLayer::from_table(h.applied, server, is_default);
    let now = PolicyLayer::from_table(incoming, server, is_default);
    let pol = |k: &str| segs(&["policy", server, k]);
    let with_top = |k: &str, top: &str| {
        let mut v = vec![pol(k)];
        if is_default {
            v.push(segs(&[top]));
        }
        v
    };

    if (now.mcp_template.is_some() || was.mcp_template.is_some())
        && template_rank(now.mcp_template.as_ref(), true)
            < template_rank(was.mcp_template.as_ref(), false)
    {
        h.hold_part(
            now.mcp_template.as_ref(),
            was.mcp_template.as_ref(),
            &[pol("mcpTemplate")],
        );
    }
    if (now.write_tools.is_some() || was.write_tools.is_some())
        && write_rank(now.write_tools.as_ref(), true) < write_rank(was.write_tools.as_ref(), false)
    {
        h.hold_part(
            now.write_tools.as_ref(),
            was.write_tools.as_ref(),
            &with_top("writeTools", "write_tools_enabled"),
        );
    }
    // The destructive tier is off until declared, so an absent value is `false`.
    let destructive_open = |d: &Option<Declared<bool>>| d.as_ref().is_some_and(|d| d.value);
    if destructive_open(&now.destructive_tools) && !destructive_open(&was.destructive_tools) {
        h.hold_part(
            now.destructive_tools.as_ref(),
            was.destructive_tools.as_ref(),
            &with_top("destructiveTools", "destructive_tools_enabled"),
        );
    }
    // The compile-time gate is on until a file switches it off.
    let gate_off = |d: &Option<Declared<bool>>| d.as_ref().is_some_and(|d| !d.value);
    if gate_off(&now.compile_gate) && !gate_off(&was.compile_gate) {
        h.hold_part(
            now.compile_gate.as_ref(),
            was.compile_gate.as_ref(),
            &with_top("compileTimeGate", "compile_time_gate"),
        );
    }
    if data_rank(now.data_policy.as_ref()) < data_rank(was.data_policy.as_ref()) {
        h.hold_part(
            now.data_policy.as_ref(),
            was.data_policy.as_ref(),
            &[pol("dataPolicy")],
        );
    }
    // No list means every category.
    if let Some(w) = &was.allow {
        let wider = match &now.allow {
            None => true,
            Some(n) => n.value.iter().any(|c| !w.value.contains(c)),
        };
        if wider {
            h.hold_part(now.allow.as_ref(), was.allow.as_ref(), &[pol("allow")]);
        }
    }
    if was
        .global_blocklist
        .iter()
        .any(|p| !now.global_blocklist.contains(p))
    {
        let setting = format!(
            "policy.{server}.globalBlocklist = {}",
            Value::Array(
                now.global_blocklist
                    .iter()
                    .map(|s| Value::String(s.clone()))
                    .collect()
            )
        );
        h.hold(
            &format!("policy.{server}.globalBlocklist"),
            setting,
            &[pol("globalBlocklist")],
        );
    }
    let was_kill: &[String] = was.kill_allowlist.as_ref().map_or(&[], |d| &d.value);
    if now
        .kill_allowlist
        .as_ref()
        .is_some_and(|n| n.value.iter().any(|p| !was_kill.contains(p)))
    {
        h.hold_part(
            now.kill_allowlist.as_ref(),
            was.kill_allowlist.as_ref(),
            &[pol("dataPolicyKillAllowlist")],
        );
    }
    if was.audit && !now.audit {
        h.hold(
            &format!("policy.{server}.irisAudit"),
            format!("policy.{server}.irisAudit = false"),
            &[pol("irisAudit")],
        );
    }
    let subject = |d: &Option<Declared<ConnectionRole>>| {
        d.as_ref()
            .is_some_and(|d| d.value == ConnectionRole::Subject)
    };
    if subject(&was.role) && !subject(&now.role) {
        h.hold_part(
            now.role.as_ref(),
            was.role.as_ref(),
            &[segs(&["instance", server, "role"])],
        );
    }
}

/// `tls_verify = false`, `ssh_host` and `service_username` of the default server (`prefix` empty)
/// or of `[instance.<name>]`.
fn connection_parts(h: &mut Hold, incoming: &Table, prefix: &[&str]) {
    let at = |k: &str| {
        let mut p = segs(prefix);
        p.push(k.to_string());
        p
    };
    let key = |k: &str| dotted(&at(k));

    let tls = at("tls_verify");
    if bool_at(incoming, &tls) == Some(false) && bool_at(h.applied, &tls) != Some(false) {
        h.hold(
            &key("tls_verify"),
            format!("{} = false", key("tls_verify")),
            &[tls],
        );
    }
    for k in ["ssh_host", "service_username"] {
        let p = at(k);
        let now = str_at(incoming, &p);
        if now.is_some() && now != str_at(h.applied, &p) {
            let mut paths = vec![p];
            if k == "service_username" {
                paths.push(at("service_password"));
            }
            h.hold(
                &key(k),
                format!("{} = {}", key(k), Value::String(now.unwrap_or_default())),
                &paths,
            );
        }
    }
}

fn opt_ins(h: &mut Hold, incoming: &Table, servers: &[String]) {
    for k in ["exec_as_os_user", "local_skills", "audit_payloads"] {
        let p = segs(&[k]);
        if bool_at(incoming, &p) == Some(true) && bool_at(h.applied, &p) != Some(true) {
            h.hold(k, format!("{k} = true"), &[p]);
        }
    }
    // The IRIS telemetry sink compiles a class per call; the file default is local.
    let sink = |t: &Table| {
        str_at(t, &segs(&["telemetry_sink"])).is_some_and(|v| v.eq_ignore_ascii_case("iris"))
    };
    if sink(incoming) && !sink(h.applied) {
        h.hold(
            "telemetry_sink",
            "telemetry_sink = \"iris\"".to_string(),
            &[segs(&["telemetry_sink"])],
        );
    }
    for s in servers {
        let p = segs(&["policy", s, "execAsOsUser"]);
        if bool_at(incoming, &p) == Some(true) && bool_at(h.applied, &p) != Some(true) {
            let key = dotted(&p);
            h.hold(&key, format!("{key} = true"), &[p]);
        }
    }
    let token = segs(&["transport", "http_token"]);
    if str_at(h.applied, &token).is_some() && str_at(incoming, &token).is_none() {
        h.hold(
            "transport.http_token",
            "transport.http_token removed".to_string(),
            &[token],
        );
    }
    let strict = |t: &Table| {
        t.get("read_only")
            .and_then(Value::as_str)
            .is_some_and(|s| s.trim().eq_ignore_ascii_case("strict"))
    };
    if strict(h.applied) && !strict(incoming) {
        let setting = match incoming.get("read_only") {
            Some(v) => format!("read_only = {v}"),
            None => "read_only = \"strict\" removed".to_string(),
        };
        h.hold("read_only", setting, &[segs(&["read_only"])]);
    }
}

/// A `[[trust]]` entry the server does not run on is held; one removed applies. An entry for a
/// path the server already trusts, with other settings, is a replacement: the one in force stays.
fn trust_entries(h: &mut Hold, incoming: &Table) {
    let entries = |t: &Table| -> Vec<Table> {
        t.get("trust")
            .and_then(Value::as_array)
            .map(|a| a.iter().filter_map(|e| e.as_table().cloned()).collect())
            .unwrap_or_default()
    };
    let canonical = |e: &Table| -> Option<(String, i64, Vec<String>)> {
        let path = e.get("path")?.as_str()?.to_string();
        let format = e.get("format").and_then(Value::as_integer).unwrap_or(0);
        let mut settings: Vec<String> = e
            .get("settings")
            .and_then(Value::as_array)
            .map(|s| {
                s.iter()
                    .filter_map(|x| x.as_str().map(str::to_string))
                    .collect()
            })
            .unwrap_or_default();
        settings.sort();
        settings.dedup();
        Some((path, format, settings))
    };
    let was = entries(h.applied);
    let mut kept: Vec<Value> = Vec::new();
    let mut changed = false;
    for e in entries(incoming) {
        let Some(c) = canonical(&e) else {
            // Without a path an entry trusts nothing.
            kept.push(Value::Table(e));
            continue;
        };
        let in_force = was
            .iter()
            .find(|w| canonical(w).is_some_and(|wc| wc.0 == c.0));
        match in_force {
            Some(w) if canonical(w).as_ref() == Some(&c) => kept.push(Value::Table(e)),
            other => {
                changed = true;
                if let Some(w) = other {
                    if !kept.iter().any(|k| k.as_table() == Some(w)) {
                        kept.push(Value::Table(w.clone()));
                    }
                }
                let id = format!("trust:{}", c.0);
                h.held.push(Held {
                    id: format!("operator:{id}"),
                    setting: format!("[[trust]] entry for {}", c.0),
                });
            }
        }
    }
    if changed {
        if kept.is_empty() {
            h.out.remove("trust");
        } else {
            h.out.insert("trust".to_string(), Value::Array(kept));
        }
    }
}

/// `incoming` (the operator file as it reads now) with every loosening value put back to what the
/// server runs on (`applied`), and what was put back.
pub fn hold_loosening(applied: &Table, incoming: Table) -> (Table, Vec<Held>) {
    let servers = server_names(applied, &incoming);
    let mut h = Hold {
        applied,
        out: incoming.clone(),
        held: Vec::new(),
    };
    for s in &servers {
        policy_parts(&mut h, &incoming, s);
        if s == "default" {
            connection_parts(&mut h, &incoming, &[]);
        } else if incoming.get("instance").and_then(|i| i.get(s)).is_some() {
            connection_parts(&mut h, &incoming, &["instance", s]);
        }
    }
    opt_ins(&mut h, &incoming, &servers);
    trust_entries(&mut h, &incoming);
    (h.out, h.held)
}

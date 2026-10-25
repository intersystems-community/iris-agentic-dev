//! Spec 141 FR-030b: the policy a call is judged by, combined part by part.
//!
//! Every case parses two TOML strings (the operator file's and the workspace file's) and runs the
//! pure `effective_policy`. A untrusted or lapsed file only tightens; a trusted file's part
//! replaces the operator file's; blocklists and `irisAudit` always combine.

use iris_agentic_dev_core::iris::config_sources::{
    Change, ConfigSnapshot, LoadInputs, SetAsideKind, TrustState, CONFIG_FILE_NAME,
};
use iris_agentic_dev_core::iris::connection::SystemMode;
use iris_agentic_dev_core::iris::workspace_config::{
    ConnectionRole, DataPolicy, McpTemplate, ToolCategory,
};
use iris_agentic_dev_core::policy::effective::{
    effective_policy, Binding, EffectivePolicy, LaunchPolicy, PartSource, PolicyInputs, PolicyLayer,
};
use iris_agentic_dev_core::policy::system_blocklist_gate::check_system_blocklist;
use iris_agentic_dev_core::tools::write_gate::{resolve_declared, GateSource, OperatorEnvGates};
use std::path::{Path, PathBuf};

#[derive(Clone, Copy, Debug)]
enum T {
    Trusted,
    Untrusted,
    Lapsed,
}

fn state(t: T) -> TrustState {
    match t {
        T::Trusted => TrustState::Trusted,
        T::Untrusted => TrustState::Untrusted,
        T::Lapsed => TrustState::Lapsed {
            changed: vec![Change {
                setting: "x".into(),
                was: None,
                now: Some("1".into()),
            }],
        },
    }
}

fn layer(toml_src: &str, server: &str, is_default: bool) -> PolicyLayer {
    let t: toml::Table = toml_src.parse().expect("test TOML parses");
    PolicyLayer::from_table(&t, server, is_default)
}

fn eff_with(
    server: &str,
    is_default: bool,
    op: &str,
    ws: &str,
    t: T,
    launch: LaunchPolicy,
) -> EffectivePolicy {
    eff_on(server, is_default, op, ws, t, launch, None)
}

/// [`eff_with`] on a server whose system mode implies `system_template`.
fn eff_on(
    server: &str,
    is_default: bool,
    op: &str,
    ws: &str,
    t: T,
    launch: LaunchPolicy,
    system_template: Option<McpTemplate>,
) -> EffectivePolicy {
    let op_layer = (!op.is_empty()).then(|| layer(op, server, is_default));
    let ws_layer = (!ws.is_empty()).then(|| layer(ws, server, is_default));
    let st = state(t);
    effective_policy(&PolicyInputs {
        server,
        is_default,
        operator: op_layer.as_ref(),
        workspace: ws_layer.as_ref(),
        trust: &st,
        binding: Binding::OperatorSide,
        launch,
        system_template,
        operator_file: Some(Path::new("/home/op/.iris-agentic-dev.toml")),
        workspace_file: Some(Path::new("/proj/.iris-agentic-dev.toml")),
    })
}

fn eff(op: &str, ws: &str, t: T) -> EffectivePolicy {
    eff_with("default", true, op, ws, t, LaunchPolicy::default())
}

fn pol(key: &str, value: &str) -> String {
    format!("[policy.default]\n{key} = {value}\n")
}

fn ids(p: &EffectivePolicy) -> Vec<&str> {
    p.set_aside.iter().map(|i| i.id.as_str()).collect()
}

// T-R4a-U13: the D5 table, one block per part.

#[test]
fn mcp_template_takes_the_stricter_unless_the_file_is_trusted() {
    // (operator, workspace, trust, expected value, expected source, set-aside ids)
    #[allow(clippy::type_complexity)]
    let rows: Vec<(&str, &str, T, Option<McpTemplate>, PartSource, Vec<&str>)> = vec![
        ("", "", T::Untrusted, None, PartSource::SystemMode, vec![]),
        (
            "dev",
            "live",
            T::Untrusted,
            Some(McpTemplate::Live),
            PartSource::WorkspaceFile,
            vec![],
        ),
        (
            "dev",
            "test",
            T::Untrusted,
            Some(McpTemplate::Test),
            PartSource::WorkspaceFile,
            vec![],
        ),
        (
            "live",
            "dev",
            T::Untrusted,
            Some(McpTemplate::Live),
            PartSource::OperatorFile,
            vec!["workspace:policy.default.mcpTemplate"],
        ),
        (
            "live",
            "dev",
            T::Lapsed,
            Some(McpTemplate::Live),
            PartSource::OperatorFile,
            vec!["workspace:policy.default.mcpTemplate"],
        ),
        (
            "live",
            "dev",
            T::Trusted,
            Some(McpTemplate::Dev),
            PartSource::TrustedFile,
            vec![],
        ),
        (
            "",
            "dev",
            T::Untrusted,
            None,
            PartSource::SystemMode,
            vec!["workspace:policy.default.mcpTemplate"],
        ),
        (
            "",
            "live",
            T::Untrusted,
            Some(McpTemplate::Live),
            PartSource::WorkspaceFile,
            vec![],
        ),
        (
            "live",
            "",
            T::Untrusted,
            Some(McpTemplate::Live),
            PartSource::OperatorFile,
            vec![],
        ),
    ];
    assert!(rows.len() >= 9);
    for (op, ws, t, want, from, aside) in rows {
        let o = if op.is_empty() {
            String::new()
        } else {
            pol("mcpTemplate", &format!("{op:?}"))
        };
        let w = if ws.is_empty() {
            String::new()
        } else {
            pol("mcpTemplate", &format!("{ws:?}"))
        };
        let p = eff(&o, &w, t);
        let label = format!("op={op:?} ws={ws:?} {t:?}");
        assert_eq!(p.mcp_template.value, want, "{label}");
        assert_eq!(p.mcp_template.from, from, "{label}");
        assert_eq!(ids(&p), aside, "{label}");
        assert_eq!(
            p.mcp_template.set_aside, aside,
            "{label}: the part lists its own items"
        );
    }
}

#[test]
fn allow_is_an_intersection_until_the_file_is_trusted() {
    let q = |cats: &str| pol("allow", cats);
    // untrusted: intersection, the category only the file adds is set aside
    let p = eff(
        &q(r#"["query", "search"]"#),
        &q(r#"["query", "compile"]"#),
        T::Untrusted,
    );
    assert_eq!(p.allow.value, Some(vec![ToolCategory::Query]));
    assert_eq!(ids(&p), vec!["workspace:policy.default.allow"]);

    // trusted: the file's list replaces the operator's
    let p = eff(
        &q(r#"["query"]"#),
        &q(r#"["query", "compile"]"#),
        T::Trusted,
    );
    assert_eq!(
        p.allow.value,
        Some(vec![ToolCategory::Query, ToolCategory::Compile])
    );
    assert_eq!(p.allow.from, PartSource::TrustedFile);
    assert!(p.set_aside.is_empty());

    // nobody declared: all categories; a file that narrows applies even untrusted
    assert_eq!(eff("", "", T::Untrusted).allow.value, None);
    let p = eff("", &q(r#"["search"]"#), T::Untrusted);
    assert_eq!(p.allow.value, Some(vec![ToolCategory::Search]));
    assert_eq!(p.allow.from, PartSource::WorkspaceFile);
    assert!(p.set_aside.is_empty(), "a narrowing list loosens nothing");

    // operator only
    let p = eff(&q(r#"["query"]"#), "", T::Untrusted);
    assert_eq!(p.allow.value, Some(vec![ToolCategory::Query]));
    assert_eq!(p.allow.from, PartSource::OperatorFile);
}

#[test]
fn data_policy_takes_the_stricter_unless_the_file_is_trusted() {
    let dp = |v: &str| pol("dataPolicy", &format!("{v:?}"));
    let rows: Vec<(&str, &str, T, DataPolicy, PartSource, usize)> = vec![
        (
            "allow",
            "redact",
            T::Untrusted,
            DataPolicy::Redact,
            PartSource::WorkspaceFile,
            0,
        ),
        (
            "block",
            "allow",
            T::Untrusted,
            DataPolicy::Block,
            PartSource::OperatorFile,
            1,
        ),
        (
            "block",
            "allow",
            T::Lapsed,
            DataPolicy::Block,
            PartSource::OperatorFile,
            1,
        ),
        (
            "block",
            "allow",
            T::Trusted,
            DataPolicy::Allow,
            PartSource::TrustedFile,
            0,
        ),
        (
            "",
            "allow",
            T::Untrusted,
            DataPolicy::Block,
            PartSource::StrictDefault,
            1,
        ),
        (
            "",
            "redact",
            T::Untrusted,
            DataPolicy::Block,
            PartSource::StrictDefault,
            1,
        ),
        (
            "",
            "allow",
            T::Trusted,
            DataPolicy::Allow,
            PartSource::TrustedFile,
            0,
        ),
        (
            "redact",
            "",
            T::Untrusted,
            DataPolicy::Redact,
            PartSource::OperatorFile,
            0,
        ),
        (
            "",
            "",
            T::Untrusted,
            DataPolicy::Block,
            PartSource::StrictDefault,
            0,
        ),
    ];
    for (op, ws, t, want, from, n_aside) in rows {
        let o = if op.is_empty() { String::new() } else { dp(op) };
        let w = if ws.is_empty() { String::new() } else { dp(ws) };
        let p = eff(&o, &w, t);
        let label = format!("op={op:?} ws={ws:?} {t:?}");
        assert_eq!(p.data_policy.value, want, "{label}");
        assert_eq!(p.data_policy.from, from, "{label}");
        assert_eq!(p.set_aside.len(), n_aside, "{label}: {:?}", ids(&p));
    }
}

#[test]
fn blocklists_and_audit_always_combine() {
    for t in [T::Untrusted, T::Lapsed, T::Trusted] {
        let p = eff(
            "[policy.default]\nglobalBlocklist = [\"^A*\"]\nirisAudit = false\n",
            "[policy.default]\nglobalBlocklist = [\"^B*\", \"^A*\"]\nirisAudit = true\n",
            t,
        );
        assert_eq!(p.global_blocklist.value, vec!["^A*", "^B*"], "{t:?}");
        assert!(p.audit.value, "{t:?}: audit on if either is on");
        assert!(
            p.set_aside.is_empty(),
            "{t:?}: combining is never a set-aside"
        );

        let p = eff(
            "[policy.default]\nirisAudit = true\n",
            "[policy.default]\nirisAudit = false\n",
            t,
        );
        assert!(
            p.audit.value,
            "{t:?}: a file cannot turn the operator's audit off"
        );
    }
}

#[test]
fn the_kill_allowlist_is_set_aside_whole_until_the_file_is_trusted() {
    let op = "[policy.default]\ndataPolicyKillAllowlist = [\"^Z*\"]\n";
    let ws = "[policy.default]\ndataPolicyKillAllowlist = [\"^Y*\"]\n";
    let p = eff(op, ws, T::Untrusted);
    assert_eq!(p.kill_allowlist.value, vec!["^Z*"]);
    assert_eq!(
        ids(&p),
        vec!["workspace:policy.default.dataPolicyKillAllowlist"]
    );

    let p = eff(op, ws, T::Trusted);
    assert_eq!(
        p.kill_allowlist.value,
        vec!["^Y*", "^Z*"],
        "operator's union file's"
    );

    let p = eff("", ws, T::Untrusted);
    assert!(p.kill_allowlist.value.is_empty());
}

#[test]
fn write_and_destructive_tools_false_beats_true_unless_the_file_is_trusted() {
    // (operator, workspace, trust, want, from, aside ids)
    #[allow(clippy::type_complexity)]
    let rows: Vec<(&str, &str, T, Option<bool>, PartSource, Vec<&str>)> = vec![
        (
            "true",
            "false",
            T::Untrusted,
            Some(false),
            PartSource::WorkspaceFile,
            vec![],
        ),
        (
            "false",
            "true",
            T::Untrusted,
            Some(false),
            PartSource::OperatorFile,
            vec!["workspace:policy.default.writeTools"],
        ),
        (
            "",
            "true",
            T::Untrusted,
            None,
            PartSource::SystemMode,
            vec!["workspace:policy.default.writeTools"],
        ),
        (
            "",
            "true",
            T::Lapsed,
            None,
            PartSource::SystemMode,
            vec!["workspace:policy.default.writeTools"],
        ),
        (
            "",
            "true",
            T::Trusted,
            Some(true),
            PartSource::TrustedFile,
            vec![],
        ),
        (
            "false",
            "true",
            T::Trusted,
            Some(true),
            PartSource::TrustedFile,
            vec![],
        ),
        (
            "true",
            "",
            T::Untrusted,
            Some(true),
            PartSource::OperatorFile,
            vec![],
        ),
        (
            "",
            "false",
            T::Untrusted,
            Some(false),
            PartSource::WorkspaceFile,
            vec![],
        ),
    ];
    for (op, ws, t, want, from, aside) in rows {
        let o = if op.is_empty() {
            String::new()
        } else {
            pol("writeTools", op)
        };
        let w = if ws.is_empty() {
            String::new()
        } else {
            pol("writeTools", ws)
        };
        let p = eff(&o, &w, t);
        let label = format!("op={op:?} ws={ws:?} {t:?}");
        assert_eq!(p.write_tools.value, want, "{label}");
        assert_eq!(p.write_tools.from, from, "{label}");
        assert_eq!(ids(&p), aside, "{label}");
    }

    // The destructive gate follows the same rule.
    let p = eff(
        &pol("destructiveTools", "false"),
        &pol("destructiveTools", "true"),
        T::Untrusted,
    );
    assert_eq!(p.destructive_tools.value, Some(false));
    assert_eq!(ids(&p), vec!["workspace:policy.default.destructiveTools"]);
    let p = eff("", &pol("destructiveTools", "true"), T::Trusted);
    assert_eq!(p.destructive_tools.value, Some(true));
}

#[test]
fn the_top_level_gates_are_the_default_servers_row() {
    let p = eff("", "write_tools_enabled = true\n", T::Untrusted);
    assert_eq!(p.write_tools.value, None);
    assert_eq!(ids(&p), vec!["workspace:write_tools_enabled"]);
    assert_eq!(p.set_aside[0].setting, "write_tools_enabled = true");

    let p = eff("", "destructive_tools_enabled = true\n", T::Trusted);
    assert_eq!(p.destructive_tools.value, Some(true));

    // A pool member has no top-level gates: `write_tools_enabled` is the default server's only.
    let p = eff_with(
        "prod",
        false,
        "",
        "write_tools_enabled = true\n",
        T::Trusted,
        LaunchPolicy::default(),
    );
    assert_eq!(p.write_tools.value, None);
}

#[test]
fn roles_take_the_stricter_unless_the_file_is_trusted() {
    let role = |v: &str| format!("[instance.prod]\nrole = \"{v}\"\n");
    let go = |op: &str, ws: &str, t: T| {
        eff_with(
            "prod",
            false,
            &if op.is_empty() {
                String::new()
            } else {
                role(op)
            },
            &if ws.is_empty() {
                String::new()
            } else {
                role(ws)
            },
            t,
            LaunchPolicy::default(),
        )
    };
    let p = go("subject", "workbench", T::Untrusted);
    assert_eq!(p.role.value, ConnectionRole::Subject);
    assert_eq!(p.role.from, PartSource::OperatorFile);
    assert_eq!(ids(&p), vec!["workspace:instance.prod.role"]);

    let p = go("subject", "workbench", T::Trusted);
    assert_eq!(p.role.value, ConnectionRole::Workbench);
    assert_eq!(p.role.from, PartSource::TrustedFile);

    let p = go("", "subject", T::Untrusted);
    assert_eq!(p.role.value, ConnectionRole::Subject);
    assert_eq!(p.role.from, PartSource::WorkspaceFile);

    // The default is a workbench, so an untrusted `workbench` loosens nothing.
    let p = go("", "workbench", T::Untrusted);
    assert_eq!(p.role.value, ConnectionRole::Workbench);
    assert!(p.set_aside.is_empty());
    assert_eq!(
        go("", "", T::Untrusted).role.from,
        PartSource::StrictDefault
    );
}

// The kill allowlist exempts custom blocklist patterns only, never the built-in list.
#[test]
fn the_kill_allowlist_cannot_lift_the_built_in_system_blocklist() {
    let allow = vec!["^oddDEF".to_string(), "^Custom*".to_string()];
    let custom = vec!["^Custom*".to_string()];

    let built_in = check_system_blocklist("^oddDEF", &[], &allow, true, "default")
        .expect("a kill allowlist must not lift the built-in blocklist");
    assert_eq!(built_in["error_code"], "SYSTEM_BLOCKLIST");

    assert!(
        check_system_blocklist("^CustomThing", &custom, &allow, true, "default").is_none(),
        "a custom pattern the operator allowlisted for kills is exempt"
    );
    let not_kill = check_system_blocklist("^CustomThing", &custom, &allow, false, "default");
    assert!(not_kill.is_some(), "the exemption is for kills only");
}

// ── Snapshot-level cases: binding and gate precedence ───────────────────────────────────────

fn load(
    op: &str,
    ws: &str,
    mutate: impl FnOnce(&mut LoadInputs),
) -> (ConfigSnapshot, tempfile::TempDir, PathBuf) {
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path().join("home");
    let proj = tmp.path().join("proj");
    std::fs::create_dir_all(&home).unwrap();
    std::fs::create_dir_all(&proj).unwrap();
    if !op.is_empty() {
        std::fs::write(home.join(CONFIG_FILE_NAME), op).unwrap();
    }
    let ws_path = proj.join(CONFIG_FILE_NAME);
    std::fs::write(&ws_path, ws).unwrap();
    let mut inputs = LoadInputs {
        cwd: std::fs::canonicalize(&proj).unwrap(),
        home: Some(std::fs::canonicalize(&home).unwrap()),
        ..Default::default()
    };
    mutate(&mut inputs);
    (ConfigSnapshot::load(&inputs), tmp, ws_path)
}

// T-R4a-U14: binding rule 1
#[test]
fn an_operator_side_server_name_wins_over_a_workspace_instance() {
    let (snap, _tmp, ws_path) = load(
        "[instance.prod]\nhost = \"op.internal\"\nweb_port = 443\n",
        "[instance.prod]\nhost = \"evil.example\"\nweb_port = 80\n[instance.extra]\nhost = \"extra.example\"\n",
        |_| {},
    );
    assert_eq!(
        snap.fleet.instance["prod"].host.as_deref(),
        Some("op.internal"),
        "the operator's address wins"
    );
    assert!(
        snap.fleet.instance.contains_key("extra"),
        "a new name from the workspace file is kept"
    );
    let shadow: Vec<_> = snap
        .set_aside
        .iter()
        .filter(|i| i.kind == SetAsideKind::ShadowedServer)
        .collect();
    assert_eq!(shadow.len(), 1, "{:?}", snap.set_aside);
    assert_eq!(shadow[0].server.as_deref(), Some("prod"));
    assert_eq!(
        shadow[0].file.as_deref(),
        Some(std::fs::canonicalize(&ws_path).unwrap().to_str().unwrap())
    );
    assert!(shadow[0].reason.contains("redefines server prod"));
}

// T-R4a-U15: binding rule 2
#[test]
fn operator_loosening_does_not_follow_an_untrusted_file_to_a_remote_host() {
    let op = "write_tools_enabled = true\ndestructive_tools_enabled = true\n";

    let (snap, _t, _p) = load(op, "host = \"prod.internal\"\nweb_port = 443\n", |_| {});
    let p = snap.effective_policy("default");
    assert_ne!(
        p.destructive_tools.value,
        Some(true),
        "the destructive tier must not open on a host the repository chose"
    );
    assert_ne!(p.write_tools.value, Some(true));
    let item = snap
        .set_aside
        .iter()
        .find(|i| i.id == "default:destructive_tools_enabled")
        .unwrap_or_else(|| panic!("no destructive item: {:?}", snap.set_aside));
    assert!(
        item.reason.contains("the workspace file chose this server"),
        "{}",
        item.reason
    );
    assert!(snap
        .set_aside
        .iter()
        .any(|i| i.id == "default:write_tools_enabled"));

    // The same operator file and a loopback host: the settings apply.
    let (snap, _t, _p) = load(op, "host = \"localhost\"\nweb_port = 52780\n", |_| {});
    let p = snap.effective_policy("default");
    assert_eq!(p.destructive_tools.value, Some(true));
    assert_eq!(p.write_tools.value, Some(true));
    assert_eq!(p.write_tools.from, PartSource::OperatorFile);

    // IRIS_HOST names the server the operator's own credentials already go to.
    let (snap, _t, _p) = load(op, "host = \"prod.internal\"\nweb_port = 443\n", |i| {
        i.host_env = Some("prod.internal".to_string());
        i.launch_endpoint = Some(
            iris_agentic_dev_core::iris::endpoint::Endpoint::parse(
                None,
                "prod.internal",
                443,
                None,
            )
            .unwrap(),
        );
    });
    assert_eq!(
        snap.effective_policy("default").destructive_tools.value,
        Some(true)
    );

    // A trusted file's choice of address counts as the operator's.
    let ws = "host = \"prod.internal\"\nweb_port = 443\nwrite_tools_enabled = true\n";
    let (_, tmp0, ws_path) = load(op, ws, |_| {});
    let canon = std::fs::canonicalize(&ws_path).unwrap();
    std::fs::write(
        tmp0.path().join("home").join(CONFIG_FILE_NAME),
        format!(
            "{op}[[trust]]\npath = {:?}\nformat = 1\nsettings = ['connection = \"http://prod.internal:443\"', 'write_tools_enabled = true']\n",
            canon.to_string_lossy()
        ),
    )
    .unwrap();
    let snap = ConfigSnapshot::load(&LoadInputs {
        cwd: canon.parent().unwrap().to_path_buf(),
        home: Some(std::fs::canonicalize(tmp0.path().join("home")).unwrap()),
        ..Default::default()
    });
    assert!(snap.trust().is_trusted(), "{:?}", snap.trust());
    assert_eq!(
        snap.effective_policy("default").destructive_tools.value,
        Some(true),
        "the operator trusted this file, address included"
    );
}

// T-R4a-U16: gate precedence
#[test]
fn launch_beats_a_trusted_file_and_an_untrusted_false_beats_the_operator_true() {
    // launch =0 beats a trusted file's true
    let p = eff_with(
        "default",
        true,
        "",
        "write_tools_enabled = true\n",
        T::Trusted,
        LaunchPolicy {
            write_tools: Some(false),
            ..Default::default()
        },
    );
    assert_eq!(p.write_tools.value, Some(false));
    assert_eq!(p.write_tools.from, PartSource::Launch);

    // an untrusted false beats the operator file's true
    let p = eff(
        "write_tools_enabled = true\n",
        "write_tools_enabled = false\n",
        T::Untrusted,
    );
    assert_eq!(p.write_tools.value, Some(false));
    assert_eq!(p.write_tools.from, PartSource::WorkspaceFile);

    // the operator file's true is reported as the operator file's
    let (snap, _t, _p) = load(
        "write_tools_enabled = true\n",
        "namespace = \"USER\"\n",
        |_| {},
    );
    let declared = snap.declared_gates();
    assert_eq!(declared.write_tools_enabled, Some(true));
    let r = resolve_declared(
        &OperatorEnvGates::default(),
        declared,
        SystemMode::Live,
        "USER",
    );
    assert!(r.write_enabled);
    assert_eq!(r.write_source, GateSource::OperatorFile);
    assert_eq!(r.write_source.as_str(), "operator_file");
}

/// FR-037, FR-030b: with no operator value, an untrusted file's template is compared with the one the
/// target server's system mode implies, not with the strictest. A tightening applies; a loosening is
/// set aside.
#[test]
fn an_untrusted_template_is_compared_with_what_the_system_mode_implies() {
    let tpl = |v: &str| pol("mcpTemplate", &format!("{v:?}"));
    // (system template, workspace value, expected value, expected source, set-aside ids)
    #[allow(clippy::type_complexity)]
    let rows: Vec<(
        McpTemplate,
        &str,
        Option<McpTemplate>,
        PartSource,
        Vec<&str>,
    )> = vec![
        // A Development server: test and live tighten it; dev changes nothing and still applies.
        (
            McpTemplate::Dev,
            "test",
            Some(McpTemplate::Test),
            PartSource::WorkspaceFile,
            vec![],
        ),
        (
            McpTemplate::Dev,
            "live",
            Some(McpTemplate::Live),
            PartSource::WorkspaceFile,
            vec![],
        ),
        (
            McpTemplate::Dev,
            "dev",
            Some(McpTemplate::Dev),
            PartSource::WorkspaceFile,
            vec![],
        ),
        // A Test server: live tightens it; dev loosens it.
        (
            McpTemplate::Test,
            "live",
            Some(McpTemplate::Live),
            PartSource::WorkspaceFile,
            vec![],
        ),
        (
            McpTemplate::Test,
            "test",
            Some(McpTemplate::Test),
            PartSource::WorkspaceFile,
            vec![],
        ),
        (
            McpTemplate::Test,
            "dev",
            None,
            PartSource::SystemMode,
            vec!["workspace:policy.default.mcpTemplate"],
        ),
        // A Live server: nothing but live applies.
        (
            McpTemplate::Live,
            "live",
            Some(McpTemplate::Live),
            PartSource::WorkspaceFile,
            vec![],
        ),
        (
            McpTemplate::Live,
            "test",
            None,
            PartSource::SystemMode,
            vec!["workspace:policy.default.mcpTemplate"],
        ),
    ];
    assert!(rows.len() >= 8);
    for (system, ws, want, from, aside) in rows {
        for trust in [T::Untrusted, T::Lapsed] {
            let p = eff_on(
                "default",
                true,
                "",
                &tpl(ws),
                trust,
                LaunchPolicy::default(),
                Some(system.clone()),
            );
            let label = format!("system={system:?} ws={ws:?} {trust:?}");
            assert_eq!(p.mcp_template.value, want, "{label}");
            assert_eq!(p.mcp_template.from, from, "{label}");
            assert_eq!(ids(&p), aside, "{label}");
        }
    }

    // With the operator's own value, the system mode is not consulted.
    let p = eff_on(
        "default",
        true,
        &tpl("live"),
        &tpl("dev"),
        T::Untrusted,
        LaunchPolicy::default(),
        Some(McpTemplate::Dev),
    );
    assert_eq!(p.mcp_template.value, Some(McpTemplate::Live));
    assert_eq!(ids(&p), vec!["workspace:policy.default.mcpTemplate"]);

    // Without a probed mode the comparison is with live, as before.
    let p = eff_on(
        "default",
        true,
        "",
        &tpl("test"),
        T::Untrusted,
        LaunchPolicy::default(),
        None,
    );
    assert_eq!(p.mcp_template.value, None);

    // dataPolicy has no system-mode baseline: block.
    let p = eff_on(
        "default",
        true,
        "",
        &pol("dataPolicy", "\"allow\""),
        T::Untrusted,
        LaunchPolicy::default(),
        Some(McpTemplate::Dev),
    );
    assert_eq!(p.data_policy.value, DataPolicy::Block);
}

//! Spec 141 R2 section 3: which identity a call runs as, per target (T-R2-U01 to U03).
//!
//! The table is a pure function; the TOML and JSON sources are parsed from strings.

use iris_agentic_dev_core::iris::config_sources::{ConfigSnapshot, LoadInputs, CONFIG_FILE_NAME};
use iris_agentic_dev_core::iris::servers_config::ServersConfig;
use iris_agentic_dev_core::tools::exec::{
    decide_identity, needs_service_account, os_user_opt_in, IdentityDecision, IdentityInputs,
    IdentitySource, IdentityTarget, OptInSource,
};
use serde_json::json;

fn inputs<'a>(
    env: Option<&'a str>,
    withheld: bool,
    target: IdentityTarget,
    own: Option<&'a str>,
) -> IdentityInputs<'a> {
    IdentityInputs {
        env_account: env,
        withheld,
        target,
        own_account: own,
    }
}

fn service(user: &str, source: IdentitySource) -> IdentityDecision {
    IdentityDecision::Service {
        user: user.to_string(),
        source,
    }
}

// ── U01: the table ────────────────────────────────────────────────────────────

#[test]
fn the_default_connection_uses_the_env_account() {
    assert_eq!(
        decide_identity(&inputs(Some("svc"), false, IdentityTarget::Default, None)),
        service("svc", IdentitySource::ServiceAccountEnv)
    );
}

#[test]
fn with_no_env_account_the_default_runs_as_itself() {
    assert_eq!(
        decide_identity(&inputs(None, false, IdentityTarget::Default, None)),
        IdentityDecision::Primary
    );
}

#[test]
fn the_env_member_and_an_alias_of_the_default_use_the_env_account() {
    for t in [IdentityTarget::EnvMember, IdentityTarget::AliasOfDefault] {
        assert_eq!(
            decide_identity(&inputs(Some("svc"), false, t, None)),
            service("svc", IdentitySource::ServiceAccountEnv),
            "{t:?}"
        );
    }
}

#[test]
fn a_member_with_its_own_account_uses_it_and_not_the_env_account() {
    assert_eq!(
        decide_identity(&inputs(
            Some("svc"),
            false,
            IdentityTarget::Member,
            Some("svc2")
        )),
        service("svc2", IdentitySource::ServiceAccountServer)
    );
    assert_eq!(
        decide_identity(&inputs(None, false, IdentityTarget::Member, Some("svc2"))),
        service("svc2", IdentitySource::ServiceAccountServer),
        "a member's own account needs no env account"
    );
}

#[test]
fn a_member_with_no_account_is_refused_when_the_env_account_is_set() {
    assert_eq!(
        decide_identity(&inputs(Some("svc"), false, IdentityTarget::Member, None)),
        IdentityDecision::Refused {
            code: "SERVICE_ACCOUNT_UNCONFIGURED_FOR_SERVER"
        }
    );
}

#[test]
fn a_member_with_no_account_and_no_env_account_runs_as_itself() {
    assert_eq!(
        decide_identity(&inputs(None, false, IdentityTarget::Member, None)),
        IdentityDecision::Primary
    );
}

#[test]
fn a_withheld_env_account_is_refused_wherever_it_would_have_applied() {
    for t in [
        IdentityTarget::Default,
        IdentityTarget::EnvMember,
        IdentityTarget::AliasOfDefault,
    ] {
        assert_eq!(
            decide_identity(&inputs(Some("svc"), true, t, None)),
            IdentityDecision::Refused {
                code: "SERVICE_ACCOUNT_WITHHELD"
            },
            "{t:?}"
        );
    }
}

#[test]
fn a_member_with_its_own_account_is_not_held_back_by_a_withheld_env_account() {
    assert_eq!(
        decide_identity(&inputs(
            Some("svc"),
            true,
            IdentityTarget::Member,
            Some("svc2")
        )),
        service("svc2", IdentitySource::ServiceAccountServer)
    );
}

#[test]
fn the_nine_tools_need_the_service_account_and_reads_do_not() {
    let yes = [
        ("iris_execute", json!({})),
        ("iris_query", json!({"mode": "write"})),
        ("iris_query", json!({"force": true})),
        ("iris_global", json!({"action": "set"})),
        ("iris_global", json!({"action": "kill"})),
        ("iris_execute_method", json!({})),
        ("global_kill", json!({})),
        ("iris_test", json!({})),
        ("iris_coverage", json!({"mode": "run"})),
        ("iris_coverage", json!({"mode": "start"})),
        ("iris_ws_open", json!({})),
        ("iris_ws_exec", json!({})),
    ];
    for (tool, args) in &yes {
        assert!(needs_service_account(tool, args), "{tool} {args}");
    }
    let no = [
        ("iris_query", json!({})),
        ("iris_query", json!({"mode": "read"})),
        ("iris_global", json!({"action": "get"})),
        ("iris_coverage", json!({"mode": "report"})),
        ("iris_coverage", json!({"mode": "stop"})),
        ("iris_coverage", json!({"mode": "check"})),
        ("iris_doc", json!({})),
        ("iris_compile", json!({})),
    ];
    for (tool, args) in &no {
        assert!(!needs_service_account(tool, args), "{tool} {args}");
    }
}

#[test]
fn both_spellings_of_the_action_are_read() {
    assert!(needs_service_account(
        "iris_global",
        &json!({"mode": "set"})
    ));
    assert!(needs_service_account(
        "iris_coverage",
        &json!({"action": "run"})
    ));
}

// ── U02: where a member's account comes from ──────────────────────────────────

#[test]
fn servers_json_carries_the_service_account_and_container() {
    let cfg: ServersConfig = serde_json::from_str(
        r#"{"version":1,"servers":{"qa":{"host":"h","port":52773,"namespace":"USER",
        "username":"u","service_username":"iadsvc","service_password":"Pw141","container":"qa-iris"}}}"#,
    )
    .unwrap();
    let qa = &cfg.servers["qa"];
    assert_eq!(qa.service_username.as_deref(), Some("iadsvc"));
    assert_eq!(qa.service_password.as_deref(), Some("Pw141"));
    assert_eq!(qa.container.as_deref(), Some("qa-iris"));
    let back = serde_json::to_string(&cfg).unwrap();
    assert!(back.contains("service_username"), "{back}");
}

#[test]
fn a_servers_json_entry_without_them_still_parses() {
    let cfg: ServersConfig = serde_json::from_str(
        r#"{"version":1,"servers":{"qa":{"host":"h","port":1,"namespace":"USER","username":"u"}}}"#,
    )
    .unwrap();
    assert_eq!(cfg.servers["qa"].service_username, None);
    let back = serde_json::to_string(&cfg).unwrap();
    assert!(!back.contains("service_username"), "{back}");
}

fn snapshot(home: &str, project: Option<&str>) -> (ConfigSnapshot, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    let root = std::fs::canonicalize(dir.path()).unwrap();
    let h = root.join("home");
    let p = root.join("proj");
    std::fs::create_dir_all(&h).unwrap();
    std::fs::create_dir_all(&p).unwrap();
    std::fs::write(h.join(CONFIG_FILE_NAME), home).unwrap();
    if let Some(body) = project {
        std::fs::write(p.join(CONFIG_FILE_NAME), body).unwrap();
    }
    let snap = ConfigSnapshot::load(&LoadInputs {
        cwd: p,
        home: Some(h),
        ..Default::default()
    });
    (snap, dir)
}

#[test]
fn an_instance_block_in_the_operator_file_keeps_its_service_account_and_container() {
    let (snap, _d) = snapshot(
        r#"
[instance.qa]
host = "10.0.0.5"
service_username = "iadsvc"
container = "qa-iris"
"#,
        None,
    );
    let qa = snap
        .fleet
        .instance
        .get("qa")
        .expect("the instance is in the snapshot");
    assert_eq!(qa.service_username.as_deref(), Some("iadsvc"));
    assert_eq!(qa.container.as_deref(), Some("qa-iris"));
}

#[test]
fn an_untrusted_workspace_instance_loses_its_service_account() {
    let (snap, _d) = snapshot(
        "",
        Some(
            r#"
[instance.qa]
host = "10.0.0.5"
service_username = "_SYSTEM"
"#,
        ),
    );
    let kept = snap
        .fleet
        .instance
        .get("qa")
        .and_then(|i| i.service_username.clone());
    assert_eq!(
        kept, None,
        "a repository must not choose the account its code runs as"
    );
    assert!(
        snap.set_aside
            .iter()
            .any(|i| i.setting.contains("service_username")),
        "the drop is reported: {:?}",
        snap.set_aside
    );
}

// ── U03: the opt-in to run as the container's OS user ─────────────────────────

fn opt_in(snap: &ConfigSnapshot, server: &str, env: Option<&str>) -> Option<OptInSource> {
    os_user_opt_in(snap, server, env)
}

#[test]
fn the_env_var_turns_the_opt_in_on_for_every_server() {
    let (snap, _d) = snapshot("", None);
    assert_eq!(opt_in(&snap, "default", Some("1")), Some(OptInSource::Env));
    assert_eq!(opt_in(&snap, "qa", Some("true")), Some(OptInSource::Env));
    assert_eq!(opt_in(&snap, "default", Some("0")), None);
    assert_eq!(opt_in(&snap, "default", Some("")), None);
    assert_eq!(opt_in(&snap, "default", None), None);
}

#[test]
fn the_home_file_turns_it_on_for_every_server_or_for_one() {
    let (all, _d) = snapshot("exec_as_os_user = true\n", None);
    assert_eq!(opt_in(&all, "qa", None), Some(OptInSource::HomeFile));

    let (one, _d2) = snapshot("[policy.qa]\nexecAsOsUser = true\n", None);
    assert_eq!(opt_in(&one, "qa", None), Some(OptInSource::HomeFile));
    assert_eq!(opt_in(&one, "prod", None), None);
}

#[test]
fn a_workspace_file_never_turns_it_on_and_the_attempt_is_reported() {
    let (snap, _d) = snapshot(
        "",
        Some("exec_as_os_user = true\n\n[policy.qa]\nexecAsOsUser = true\n"),
    );
    assert_eq!(opt_in(&snap, "qa", None), None);
    assert_eq!(opt_in(&snap, "default", None), None);
    let ids: Vec<&str> = snap.set_aside.iter().map(|i| i.id.as_str()).collect();
    assert!(
        ids.contains(&"workspace:exec_as_os_user"),
        "the top-level key is reported: {ids:?}"
    );
    assert!(
        ids.iter().any(|i| i.contains("execAsOsUser")),
        "the per-server key is reported: {ids:?}"
    );
}

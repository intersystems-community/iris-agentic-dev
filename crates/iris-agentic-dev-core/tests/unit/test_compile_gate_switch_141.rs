//! Spec 141 FR-018: the compile-time gate has a switch, and the switch is a loosening setting.
//!
//! Off is accepted from launch settings, the operator file and a trusted file. An untrusted file's
//! `false` is set aside and the gate stays on. This file covers the setting; the constructs the gate
//! refuses are covered by the live tests of the compile-time-gate phase.

use iris_agentic_dev_core::iris::config_sources::{Change, TrustState};
use iris_agentic_dev_core::policy::effective::{
    effective_policy, Binding, LaunchPolicy, PartSource, PolicyInputs, PolicyLayer,
};
use iris_agentic_dev_core::testing::BEHAVIOR_ENV_VARS;
use std::path::Path;

fn compile_gate(
    server: &str,
    is_default: bool,
    op: &str,
    ws: &str,
    trust: TrustState,
    launch: LaunchPolicy,
) -> (bool, PartSource, Vec<String>) {
    let parse = |s: &str| {
        let t: toml::Table = s.parse().unwrap();
        PolicyLayer::from_table(&t, server, is_default)
    };
    let op_layer = (!op.is_empty()).then(|| parse(op));
    let ws_layer = (!ws.is_empty()).then(|| parse(ws));
    let p = effective_policy(&PolicyInputs {
        server,
        is_default,
        operator: op_layer.as_ref(),
        workspace: ws_layer.as_ref(),
        trust: &trust,
        binding: Binding::OperatorSide,
        launch,
        system_template: None,
        operator_file: Some(Path::new("/home/op/.iris-agentic-dev.toml")),
        workspace_file: Some(Path::new("/proj/.iris-agentic-dev.toml")),
    });
    (
        p.compile_gate.value,
        p.compile_gate.from,
        p.set_aside.iter().map(|i| i.id.clone()).collect(),
    )
}

fn lapsed() -> TrustState {
    TrustState::Lapsed {
        changed: vec![Change {
            setting: "x".into(),
            was: None,
            now: Some("1".into()),
        }],
    }
}

const OFF_TOP: &str = "compile_time_gate = false\n";
const OFF_POLICY: &str = "[policy.default]\ncompileTimeGate = false\n";

// T-R4a-U23
#[test]
fn an_untrusted_off_stays_on_and_leaves_an_item() {
    for (spelling, src, id) in [
        ("top level", OFF_TOP, "workspace:compile_time_gate"),
        (
            "policy",
            OFF_POLICY,
            "workspace:policy.default.compileTimeGate",
        ),
    ] {
        for trust in [TrustState::Untrusted, lapsed()] {
            let (on, from, aside) = compile_gate(
                "default",
                true,
                "",
                src,
                trust.clone(),
                LaunchPolicy::default(),
            );
            assert!(on, "{spelling}/{trust:?}: the gate must stay on");
            assert_eq!(from, PartSource::StrictDefault);
            assert_eq!(aside, vec![id.to_string()], "{spelling}/{trust:?}");
        }
    }
}

#[test]
fn a_trusted_off_turns_it_off() {
    for src in [OFF_TOP, OFF_POLICY] {
        let (on, from, aside) = compile_gate(
            "default",
            true,
            "",
            src,
            TrustState::Trusted,
            LaunchPolicy::default(),
        );
        assert!(!on);
        assert_eq!(from, PartSource::TrustedFile);
        assert!(aside.is_empty());
    }
}

#[test]
fn the_operator_file_turns_it_off_without_trust() {
    let (on, from, _) = compile_gate(
        "default",
        true,
        OFF_TOP,
        "",
        TrustState::NotNeeded,
        LaunchPolicy::default(),
    );
    assert!(!on);
    assert_eq!(from, PartSource::OperatorFile);

    // An untrusted `true` or silence cannot turn it back on past the operator... it is on already;
    // and an untrusted file cannot override the operator's off with its own off either way.
    let (on, from, _) = compile_gate(
        "default",
        true,
        OFF_TOP,
        "compile_time_gate = true\n",
        TrustState::Untrusted,
        LaunchPolicy::default(),
    );
    assert!(
        on,
        "a file that tightens applies: on is stricter than the operator's off"
    );
    assert_eq!(from, PartSource::WorkspaceFile);
}

#[test]
fn launch_settings_outrank_every_file() {
    // IAD_COMPILE_TIME_GATE=1 beats a trusted false.
    let (on, from, _) = compile_gate(
        "default",
        true,
        "",
        OFF_TOP,
        TrustState::Trusted,
        LaunchPolicy {
            compile_gate: Some(true),
            ..Default::default()
        },
    );
    assert!(on);
    assert_eq!(from, PartSource::Launch);

    // =1 holds on every server; =0 turns it off for the default server only.
    let (on, from, _) = compile_gate(
        "prod",
        false,
        "",
        "[policy.prod]\ncompileTimeGate = false\n",
        TrustState::Trusted,
        LaunchPolicy {
            compile_gate: Some(true),
            ..Default::default()
        },
    );
    assert!(on);
    assert_eq!(from, PartSource::Launch);

    let off = LaunchPolicy {
        compile_gate: Some(false),
        ..Default::default()
    };
    let (on, from, _) = compile_gate("default", true, "", "", TrustState::NotNeeded, off);
    assert!(!on);
    assert_eq!(from, PartSource::Launch);
    let (on, _, _) = compile_gate("prod", false, "", "", TrustState::NotNeeded, off);
    assert!(on, "=0 is for the default server");
}

#[test]
fn the_switch_is_per_server() {
    let (on, _, _) = compile_gate(
        "prod",
        false,
        "",
        "[policy.dev]\ncompileTimeGate = false\n",
        TrustState::Trusted,
        LaunchPolicy::default(),
    );
    assert!(
        on,
        "a policy for `dev` does not switch the gate off on `prod`"
    );
    // The top-level key is the default server's.
    let (on, _, _) = compile_gate(
        "prod",
        false,
        "",
        OFF_TOP,
        TrustState::Trusted,
        LaunchPolicy::default(),
    );
    assert!(on);
}

#[test]
fn the_environment_variable_is_registered_and_parsed() {
    assert!(
        BEHAVIOR_ENV_VARS.contains(&"IAD_COMPILE_TIME_GATE"),
        "a new behaviour variable belongs in BEHAVIOR_ENV_VARS (principle XII)"
    );
    for (raw, want) in [
        ("0", Some(false)),
        ("off", Some(false)),
        ("OFF", Some(false)),
        ("1", Some(true)),
        ("on", Some(true)),
        ("", None),
        ("maybe", None),
    ] {
        assert_eq!(
            LaunchPolicy::compile_gate_from_env_value(raw),
            want,
            "{raw:?}"
        );
    }
}

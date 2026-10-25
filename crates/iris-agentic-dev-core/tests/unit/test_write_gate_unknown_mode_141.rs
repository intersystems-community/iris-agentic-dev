//! Spec 141 R1 D6 and R5 C10: a pool member's write gate.
//!
//! `resolve_member` is the member's counterpart of `resolve_declared`. The default connection's
//! rules do not carry over: a launch setting or a top-level key that names no server cannot raise
//! a member, and a member whose `SystemMode` could not be read starts closed, whatever its
//! namespace is called.

use iris_agentic_dev_core::iris::connection::SystemMode;
use iris_agentic_dev_core::tools::write_gate::{
    member_needs_mode, resolve_member, DeclaredGates, GateSource, MemberDeclaration,
    OperatorEnvGates,
};

const NONE: OperatorEnvGates = OperatorEnvGates {
    write_tools_enabled: None,
    destructive_enabled: None,
    allow_prod: false,
};

fn decl() -> MemberDeclaration {
    MemberDeclaration::default()
}

#[test]
fn live_closes_a_member_and_development_and_test_open_it() {
    for (mode, open) in [
        (SystemMode::Live, false),
        (SystemMode::Development, true),
        (SystemMode::Test, true),
    ] {
        let g = resolve_member(&NONE, &decl(), mode, "USER");
        assert_eq!(g.write_enabled, open, "{mode:?}");
        assert_eq!(g.write_source, GateSource::InferredSystemMode, "{mode:?}");
        assert!(
            !g.destructive_enabled,
            "{mode:?}: destructive is never inferred"
        );
        assert_eq!(g.destructive_source, GateSource::InferredDefault);
    }
}

#[test]
fn a_member_whose_mode_is_unknown_starts_write_closed_whatever_its_namespace_says() {
    // R5 C10. The default connection infers from the namespace; a member whose mode the probe
    // could not read has nothing to infer from.
    for ns in ["USER", "PROD", "APPDATA"] {
        let g = resolve_member(&NONE, &decl(), SystemMode::Unknown, ns);
        assert!(!g.write_enabled, "namespace {ns}");
        assert_eq!(g.write_source, GateSource::UnprobedMember, "namespace {ns}");
    }
}

#[test]
fn a_member_with_no_system_mode_set_is_judged_by_the_namespace_like_the_default() {
    // A fresh instance answers the probe with an empty value: `Unset`. The probe worked, so the
    // member is not "unprobed"; the namespace decides, as it does on the default connection.
    let open = resolve_member(&NONE, &decl(), SystemMode::Unset, "USER");
    assert!(open.write_enabled);
    assert_eq!(open.write_source, GateSource::InferredNamespace);
    for ns in ["PROD", "production", "LIVE"] {
        let closed = resolve_member(&NONE, &decl(), SystemMode::Unset, ns);
        assert!(!closed.write_enabled, "{ns}");
        assert_eq!(closed.write_source, GateSource::InferredNamespace, "{ns}");
    }
}

#[test]
fn launch_env_zero_closes_every_member_and_env_one_raises_none() {
    let off = OperatorEnvGates {
        write_tools_enabled: Some(false),
        ..NONE
    };
    for mode in [
        SystemMode::Development,
        SystemMode::Test,
        SystemMode::Unknown,
    ] {
        let g = resolve_member(&off, &decl(), mode, "USER");
        assert!(!g.write_enabled, "{mode:?}");
        assert_eq!(g.write_source, GateSource::OperatorEnv);
    }
    // =0 beats a declaration on the member.
    let declared = MemberDeclaration {
        write_tools: Some(true),
        ..decl()
    };
    assert!(!resolve_member(&off, &declared, SystemMode::Development, "USER").write_enabled);

    // =1 and the legacy override raise only the default connection.
    let on = OperatorEnvGates {
        write_tools_enabled: Some(true),
        destructive_enabled: Some(true),
        allow_prod: true,
    };
    for mode in [SystemMode::Live, SystemMode::Unknown] {
        let g = resolve_member(&on, &decl(), mode, "USER");
        assert!(!g.write_enabled, "env =1 must not open a {mode:?} member");
        assert!(!g.destructive_enabled);
    }
}

#[test]
fn a_top_level_key_can_close_a_member_but_never_open_one() {
    let closed = MemberDeclaration {
        top_level: DeclaredGates {
            write_tools_enabled: Some(false),
            write_source: Some(GateSource::OperatorFile),
            ..Default::default()
        },
        ..decl()
    };
    let g = resolve_member(&NONE, &closed, SystemMode::Development, "USER");
    assert!(
        !g.write_enabled,
        "write_tools_enabled = false is the operator's kill switch for every server"
    );
    assert_eq!(g.write_source, GateSource::OperatorFile);

    let open = MemberDeclaration {
        top_level: DeclaredGates {
            write_tools_enabled: Some(true),
            destructive_tools_enabled: Some(true),
            ..Default::default()
        },
        ..decl()
    };
    for mode in [SystemMode::Live, SystemMode::Unknown] {
        let g = resolve_member(&NONE, &open, mode, "USER");
        assert!(
            !g.write_enabled,
            "write_tools_enabled = true must not open a {mode:?} member"
        );
        assert!(!g.destructive_enabled);
    }
}

#[test]
fn a_policy_declaration_opens_or_closes_a_member_whatever_its_mode() {
    for mode in [
        SystemMode::Live,
        SystemMode::Development,
        SystemMode::Test,
        SystemMode::Unknown,
    ] {
        let open = MemberDeclaration {
            write_tools: Some(true),
            ..decl()
        };
        let g = resolve_member(&NONE, &open, mode, "USER");
        assert!(g.write_enabled, "{mode:?}");
        assert_eq!(g.write_source, GateSource::PolicyDeclaration);

        let shut = MemberDeclaration {
            write_tools: Some(false),
            ..decl()
        };
        let g = resolve_member(&NONE, &shut, mode, "USER");
        assert!(!g.write_enabled, "{mode:?}");
        assert_eq!(g.write_source, GateSource::PolicyDeclaration);
    }
}

#[test]
fn the_destructive_tier_is_declared_per_member_and_is_a_subset_of_write() {
    let both = MemberDeclaration {
        write_tools: Some(true),
        destructive_tools: Some(true),
        ..decl()
    };
    let g = resolve_member(&NONE, &both, SystemMode::Development, "USER");
    assert!(g.write_enabled && g.destructive_enabled);
    assert_eq!(g.destructive_source, GateSource::PolicyDeclaration);

    // destructiveTools = true on a member whose writes are closed cannot open anything.
    let orphan = MemberDeclaration {
        destructive_tools: Some(true),
        ..decl()
    };
    let g = resolve_member(&NONE, &orphan, SystemMode::Live, "USER");
    assert!(!g.write_enabled);
    assert!(
        !g.destructive_enabled,
        "the destructive tier cannot outlive a closed write gate"
    );
    assert_eq!(g.destructive_source, GateSource::FailClosed);

    // The operator's =0 closes the destructive tier for every member.
    let off = OperatorEnvGates {
        destructive_enabled: Some(false),
        ..NONE
    };
    let g = resolve_member(&off, &both, SystemMode::Development, "USER");
    assert!(g.write_enabled);
    assert!(!g.destructive_enabled);
    assert_eq!(g.destructive_source, GateSource::OperatorEnv);
}

#[test]
fn a_member_is_probed_only_when_nothing_before_the_probe_decided() {
    assert!(member_needs_mode(&NONE, &decl()));
    assert!(!member_needs_mode(
        &OperatorEnvGates {
            write_tools_enabled: Some(false),
            ..NONE
        },
        &decl()
    ));
    assert!(!member_needs_mode(
        &NONE,
        &MemberDeclaration {
            write_tools: Some(true),
            ..decl()
        }
    ));
    assert!(!member_needs_mode(
        &NONE,
        &MemberDeclaration {
            top_level: DeclaredGates {
                write_tools_enabled: Some(false),
                ..Default::default()
            },
            ..decl()
        }
    ));
}

// Spec 085 write-gate integrity — Phase 2 foundational tests (T004, T005, T006).
//
// Every config in this file is produced by `toml::from_str` on a config **string**, never a
// `WorkspaceConfig` struct literal (FR-022). The struct-literal tests at
// `workspace_config.rs:1580-1628` are exactly why the #110 pattern shipped twice: a literal
// cannot catch a serde key that was silently dropped, and it cannot catch a default that only
// applies on the deserialize path.
//
// The operator's environment arrives as an `OperatorEnvGates` argument rather than through
// `std::env`, which is what makes the "operator already set the variable" branch reachable at
// all (FR-024). No test here mutates the process environment.

use iris_agentic_dev_core::iris::connection::SystemMode;
use iris_agentic_dev_core::iris::workspace_config::WorkspaceConfig;
use iris_agentic_dev_core::tools::write_gate::{
    resolve_gates, GateResolution, GateSource, OperatorEnvGates,
};

/// Parse a config from a TOML string. Panics with the parse error, because a test that silently
/// fell back to `Default` would assert nothing.
fn cfg(toml_src: &str) -> WorkspaceConfig {
    toml::from_str(toml_src).expect("config string must parse")
}

/// Operator set nothing — the state every existing test already covers.
fn no_env() -> OperatorEnvGates {
    OperatorEnvGates::default()
}

fn env_write(v: bool) -> OperatorEnvGates {
    OperatorEnvGates {
        write_tools_enabled: Some(v),
        ..Default::default()
    }
}

// ── T004: the precedence matrix ───────────────────────────────────────────────

/// The whole point of the feature: a config that says `false` resolves to `false`, on every
/// load, with `config_file` named as the decider. Under the old code this value was exported to
/// `IRIS_WRITE_TOOLS_ENABLED` only when that variable was absent, so the second load of a
/// process kept the first load's answer forever.
#[test]
fn config_false_resolves_false_and_reports_config_file() {
    let c = cfg("write_tools_enabled = false\n");
    let r = resolve_gates(&no_env(), Some(&c), SystemMode::Development, "USER");
    assert!(!r.write_enabled, "config says false, so writes are off");
    assert_eq!(r.write_source, GateSource::ConfigFile);
}

#[test]
fn config_true_resolves_true_and_reports_config_file() {
    let c = cfg("write_tools_enabled = true\n");
    let r = resolve_gates(&no_env(), Some(&c), SystemMode::Live, "PROD");
    assert!(r.write_enabled, "an explicit declaration beats inference");
    assert_eq!(r.write_source, GateSource::ConfigFile);
}

/// Documented precedence: an operator who exported the variable outranks the config file.
/// This is the branch no existing test reaches — all three tests in `workspace_config.rs` call
/// `env::remove_var` first, so they only ever exercise the clean-env side (FR-024).
#[test]
fn operator_env_outranks_config_in_both_directions() {
    let allow = cfg("write_tools_enabled = true\n");
    let deny = cfg("write_tools_enabled = false\n");

    let r = resolve_gates(
        &env_write(false),
        Some(&allow),
        SystemMode::Development,
        "USER",
    );
    assert!(!r.write_enabled, "operator said off, config said on");
    assert_eq!(r.write_source, GateSource::OperatorEnv);

    let r = resolve_gates(&env_write(true), Some(&deny), SystemMode::Live, "PROD");
    assert!(r.write_enabled, "operator said on, config said off");
    assert_eq!(r.write_source, GateSource::OperatorEnv);
}

/// `IRIS_ALLOW_PROD` sits below the config file. It used to win by accident whenever the config
/// value could not be exported, which is how a `false` config ended up serving writes.
#[test]
fn config_outranks_legacy_allow_prod() {
    let c = cfg("write_tools_enabled = false\n");
    let operator = OperatorEnvGates {
        allow_prod: true,
        ..Default::default()
    };
    let r = resolve_gates(&operator, Some(&c), SystemMode::Live, "PROD");
    assert!(!r.write_enabled, "config file outranks IRIS_ALLOW_PROD");
    assert_eq!(r.write_source, GateSource::ConfigFile);
}

#[test]
fn legacy_allow_prod_applies_when_nothing_is_declared() {
    let operator = OperatorEnvGates {
        allow_prod: true,
        ..Default::default()
    };
    let r = resolve_gates(&operator, None, SystemMode::Live, "PROD");
    assert!(r.write_enabled, "issue #26 override, unchanged");
    assert_eq!(r.write_source, GateSource::LegacyAllowProd);
}

/// The inference chain moves from `connection.rs:143-147` unchanged (FR-019). These four cases
/// pin today's answers so the move is provably behaviour-preserving.
#[test]
fn system_mode_inference_is_unchanged() {
    for (mode, expected) in [
        (SystemMode::Live, false),
        (SystemMode::Development, true),
        (SystemMode::Test, true),
    ] {
        let r = resolve_gates(&no_env(), None, mode, "USER");
        assert_eq!(r.write_enabled, expected, "SystemMode {mode:?}");
        assert_eq!(r.write_source, GateSource::InferredSystemMode);
    }
}

#[test]
fn namespace_inference_applies_only_when_system_mode_is_unknown() {
    for ns in ["USER", "MYAPP", "HSCUSTOM"] {
        let r = resolve_gates(&no_env(), None, SystemMode::Unknown, ns);
        assert!(r.write_enabled, "{ns} is not a production namespace");
        assert_eq!(r.write_source, GateSource::InferredNamespace);
    }
    for ns in ["PROD", "PRODUCTION", "LIVE", "PRD", "prod", "Production"] {
        let r = resolve_gates(&no_env(), None, SystemMode::Unknown, ns);
        assert!(!r.write_enabled, "{ns} is a production namespace");
        assert_eq!(r.write_source, GateSource::InferredNamespace);
    }
}

/// A declaration beats every inference, which is what makes the gate mean something on a
/// developer's `USER` namespace.
#[test]
fn declaration_beats_inference() {
    let c = cfg("write_tools_enabled = false\n");
    for mode in [
        SystemMode::Live,
        SystemMode::Development,
        SystemMode::Test,
        SystemMode::Unknown,
    ] {
        let r = resolve_gates(&no_env(), Some(&c), mode, "USER");
        assert!(!r.write_enabled, "declared false under SystemMode {mode:?}");
        assert_eq!(r.write_source, GateSource::ConfigFile);
    }
}

/// A config file that declares connection details but no gate must not be read as a
/// declaration — that is the serde silent-drop shape, and it has to fall through to inference.
#[test]
fn config_without_the_gate_key_falls_through_to_inference() {
    let c = cfg("container = \"iris-dev-iris\"\nnamespace = \"USER\"\n");
    let r = resolve_gates(&no_env(), Some(&c), SystemMode::Live, "USER");
    assert!(!r.write_enabled);
    assert_eq!(
        r.write_source,
        GateSource::InferredSystemMode,
        "an absent key is not a declaration"
    );
}

// ── T004 (destructive tier) ───────────────────────────────────────────────────

#[test]
fn destructive_tier_is_off_until_declared() {
    let c = cfg("write_tools_enabled = true\n");
    let r = resolve_gates(&no_env(), Some(&c), SystemMode::Development, "USER");
    assert!(!r.destructive_enabled, "never inferred, only declared");
    assert_eq!(
        r.destructive_source,
        GateSource::InferredDefault,
        "nothing failed — the documented default applied"
    );
}

#[test]
fn destructive_tier_honours_config_and_operator_env() {
    let c = cfg("write_tools_enabled = true\ndestructive_tools_enabled = true\n");
    let r = resolve_gates(&no_env(), Some(&c), SystemMode::Development, "USER");
    assert!(r.destructive_enabled);
    assert_eq!(r.destructive_source, GateSource::ConfigFile);

    let operator = OperatorEnvGates {
        destructive_enabled: Some(false),
        ..Default::default()
    };
    let r = resolve_gates(&operator, Some(&c), SystemMode::Development, "USER");
    assert!(!r.destructive_enabled, "operator env outranks the config");
    assert_eq!(r.destructive_source, GateSource::OperatorEnv);
}

// ── T005: fail-closed and the invariant ───────────────────────────────────────

#[test]
fn fail_closed_is_off_and_says_so() {
    let r = GateResolution::fail_closed();
    assert!(!r.write_enabled);
    assert!(!r.destructive_enabled);
    assert_eq!(r.write_source, GateSource::FailClosed);
    assert_eq!(r.destructive_source, GateSource::FailClosed);
}

/// Defect 3, as a resolver-level property. `destructive_tools_enabled = true` with
/// `write_tools_enabled = false` used to log `DESTRUCTIVE_REQUIRES_WRITES` and then start the
/// server with writes *enabled*, because the `return None` above the env export dropped the
/// caller into the permissive namespace heuristic. The declaration is now rejected at startup;
/// this asserts that even if one reaches the resolver, it closes rather than opens.
#[test]
fn destructive_true_with_writes_false_fails_closed() {
    let c = cfg("write_tools_enabled = false\ndestructive_tools_enabled = true\n");
    let r = resolve_gates(&no_env(), Some(&c), SystemMode::Unknown, "USER");
    assert!(!r.write_enabled, "USER must not re-open the gate");
    assert!(!r.destructive_enabled);
    assert_eq!(r.destructive_source, GateSource::FailClosed);
}

/// The data-model §2 invariant, over every input combination rather than a sampled few:
/// `destructive_enabled` is never true while `write_enabled` is false.
#[test]
fn destructive_never_outlives_the_write_gate() {
    let configs = [
        None,
        Some(cfg("")),
        Some(cfg("write_tools_enabled = true\n")),
        Some(cfg("write_tools_enabled = false\n")),
        Some(cfg("destructive_tools_enabled = true\n")),
        Some(cfg("destructive_tools_enabled = false\n")),
        Some(cfg(
            "write_tools_enabled = true\ndestructive_tools_enabled = true\n",
        )),
        Some(cfg(
            "write_tools_enabled = false\ndestructive_tools_enabled = true\n",
        )),
        Some(cfg(
            "write_tools_enabled = true\ndestructive_tools_enabled = false\n",
        )),
        Some(cfg(
            "write_tools_enabled = false\ndestructive_tools_enabled = false\n",
        )),
    ];
    let mut checked = 0usize;
    for write_env in [None, Some(true), Some(false)] {
        for destructive_env in [None, Some(true), Some(false)] {
            for allow_prod in [false, true] {
                let operator = OperatorEnvGates {
                    write_tools_enabled: write_env,
                    destructive_enabled: destructive_env,
                    allow_prod,
                };
                for c in &configs {
                    for mode in [
                        SystemMode::Live,
                        SystemMode::Development,
                        SystemMode::Test,
                        SystemMode::Unknown,
                    ] {
                        for ns in ["USER", "PROD", "MYAPP"] {
                            let r = resolve_gates(&operator, c.as_ref(), mode, ns);
                            // FR-018 read as an implication: destructive ⇒ write.
                            assert!(
                                !r.destructive_enabled || r.write_enabled,
                                "invariant broken: operator={operator:?} cfg={c:?} \
                                 mode={mode:?} ns={ns} -> {r:?}"
                            );
                            checked += 1;
                        }
                    }
                }
            }
        }
    }
    assert_eq!(checked, 3 * 3 * 2 * 10 * 4 * 3, "matrix size guard");
}

/// A gate is never reported without a source, and `FailClosed` never accompanies an open gate.
#[test]
fn an_open_gate_is_never_attributed_to_fail_closed() {
    let configs = [
        None,
        Some(cfg("write_tools_enabled = true\n")),
        Some(cfg("write_tools_enabled = false\n")),
    ];
    for c in &configs {
        for mode in [SystemMode::Live, SystemMode::Unknown] {
            let r = resolve_gates(&no_env(), c.as_ref(), mode, "USER");
            if r.write_enabled {
                assert_ne!(r.write_source, GateSource::FailClosed);
            }
            if r.destructive_enabled {
                assert_ne!(r.destructive_source, GateSource::FailClosed);
            }
        }
    }
}

// ── T006: the disconnected path ───────────────────────────────────────────────

/// `new_disconnected` used to re-derive the gate from `IRIS_WRITE_TOOLS_ENABLED` with
/// `unwrap_or(true)` — the opposite default from `from_iris` — so a server that could not reach
/// IRIS answered permissively. Both constructors now take the resolution (FR-012).
#[test]
fn disconnected_state_carries_the_resolution_it_was_given() {
    use iris_agentic_dev_core::tools::{ConnectionSource, ConnectionState};

    let c = cfg("write_tools_enabled = false\n");
    let gates = resolve_gates(&no_env(), Some(&c), SystemMode::Unknown, "USER");
    let state = ConnectionState::new_disconnected(ConnectionSource::ConfigFile, gates);
    assert!(
        !state.gates.write_enabled,
        "no connection must not mean permissive"
    );
    assert_eq!(state.gates.write_source, GateSource::ConfigFile);
    assert!(state.iris.is_none());
}

/// The complement, per Constitution IV: the new upstream gate check must not swallow an
/// unreachable server. Writes resolved **on**, no connection — the answer is `IRIS_UNREACHABLE`,
/// not a gate error. Otherwise "the gate blocked it" becomes indistinguishable from "IRIS is
/// down", which is the reporting failure this whole feature is about.
#[tokio::test]
async fn writes_on_but_no_connection_reports_iris_unreachable() {
    use iris_agentic_dev_core::tools::interop::{interop_lookup_manage_impl, LookupManageParams};

    let c = cfg("write_tools_enabled = true\n");
    let gates = resolve_gates(&no_env(), Some(&c), SystemMode::Development, "USER");
    assert!(gates.write_enabled, "precondition: the gate is open");

    let params: LookupManageParams = serde_json::from_value(serde_json::json!({
        "action": "set",
        "table": "IAD085",
        "key": "k",
        "value": "v",
        "namespace": "USER",
    }))
    .expect("params");

    let result = interop_lookup_manage_impl(None, params)
        .await
        .expect("Ok(CallToolResult)");
    let v: serde_json::Value = result
        .structured_content
        .clone()
        .expect("structured content");
    assert_eq!(v["error_code"], "IRIS_UNREACHABLE");
    assert_eq!(v["success"], false);
}

// ── T038: startup validation over every declared combination ──────────────────

/// The rejection is exactly one combination, and every other one is usable.
///
/// Enumerated rather than sampled, because both halves have been wrong in shipped code: 1.2.6
/// detected this combination and then started with writes *on*, and the fix has to not overshoot
/// into rejecting `destructive_tools_enabled = false` alongside `write_tools_enabled = false`,
/// which is a perfectly ordinary read-only setup.
///
/// Every config is parsed from a TOML string (FR-022). A struct literal here would assert that
/// `validate_gate_config` reads two fields; parsing asserts that those two fields survive serde,
/// which is the part that broke — `write_allowed_servers` was documented for two releases while
/// never being a field at all.
#[test]
fn only_destructive_on_with_writes_off_is_rejected() {
    use iris_agentic_dev_core::tools::write_gate::{validate_gate_config, GateConfigError};

    let decl = |v: Option<bool>, key: &str| match v {
        Some(b) => format!("{key} = {b}\n"),
        None => String::new(),
    };

    let mut rejected = 0usize;
    let mut accepted = 0usize;
    for write in [None, Some(true), Some(false)] {
        for destructive in [None, Some(true), Some(false)] {
            let src = format!(
                "{}{}",
                decl(write, "write_tools_enabled"),
                decl(destructive, "destructive_tools_enabled")
            );
            let c = cfg(&src);
            // Round-trip guard: if serde ever stops reading these keys, both fields come back
            // `None`, `validate_gate_config` returns `Ok` for all nine cases, and the loop below
            // would still "pass" — having tested nothing.
            assert_eq!(
                c.write_tools_enabled, write,
                "serde dropped write_tools_enabled from {src:?}"
            );
            assert_eq!(
                c.destructive_tools_enabled, destructive,
                "serde dropped destructive_tools_enabled from {src:?}"
            );

            let got = validate_gate_config(&c);
            if destructive == Some(true) && write == Some(false) {
                rejected += 1;
                assert_eq!(
                    got,
                    Err(GateConfigError::DestructiveRequiresWrites),
                    "{src:?} asks for the destructive tier with writes off — the tier is a subset \
                     of the write gate, so this can never take effect and must be refused"
                );
            } else {
                accepted += 1;
                assert_eq!(
                    got,
                    Ok(()),
                    "{src:?} is a usable configuration and must not be refused"
                );
            }
        }
    }
    assert_eq!(
        (rejected, accepted),
        (1, 8),
        "all nine combinations covered"
    );
}

/// The error carries the documented code, because that string is what the operator greps for and
/// what `docs/tools.md` promises. An error whose `Display` says one thing and whose `code()` says
/// another is the same class of defect as a payload that disagrees with its schema.
#[test]
fn the_rejection_names_the_documented_code() {
    use iris_agentic_dev_core::tools::write_gate::{
        validate_gate_config, ERR_DESTRUCTIVE_REQUIRES_WRITES,
    };

    let c = cfg("write_tools_enabled = false\ndestructive_tools_enabled = true\n");
    let err = validate_gate_config(&c).expect_err("must be rejected");
    assert_eq!(err.code(), ERR_DESTRUCTIVE_REQUIRES_WRITES);
    assert!(
        err.to_string().contains(ERR_DESTRUCTIVE_REQUIRES_WRITES),
        "the logged message has to contain the code an operator searches for: {err}"
    );
}

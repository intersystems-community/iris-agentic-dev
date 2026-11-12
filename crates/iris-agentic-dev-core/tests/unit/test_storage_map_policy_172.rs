//! Tests for allow_dynamic_map_globals policy field (spec 172 T057).

use iris_agentic_dev_core::iris::config_sources::TrustState;
use iris_agentic_dev_core::policy::effective::{
    effective_policy, Binding, EffectivePolicy, LaunchPolicy, PolicyInputs, PolicyLayer,
};
use std::path::Path;

#[derive(Clone, Copy, Debug)]
enum T {
    Trusted,
    Untrusted,
}

fn state(t: T) -> TrustState {
    match t {
        T::Trusted => TrustState::Trusted,
        T::Untrusted => TrustState::Untrusted,
    }
}

fn layer(toml_src: &str, server: &str, is_default: bool) -> PolicyLayer {
    let t: toml::Table = toml_src.parse().expect("test TOML parses");
    PolicyLayer::from_table(&t, server, is_default)
}

fn eff(op: &str, ws: &str, t: T) -> EffectivePolicy {
    let op_layer = (!op.is_empty()).then(|| layer(op, "default", true));
    let ws_layer = (!ws.is_empty()).then(|| layer(ws, "default", true));
    let st = state(t);
    effective_policy(&PolicyInputs {
        server: "default",
        is_default: true,
        operator: op_layer.as_ref(),
        workspace: ws_layer.as_ref(),
        trust: &st,
        binding: Binding::OperatorSide,
        launch: LaunchPolicy::default(),
        system_template: None,
        operator_file: Some(Path::new("/home/op/.iris-agentic-dev.toml")),
        workspace_file: Some(Path::new("/proj/.iris-agentic-dev.toml")),
    })
}

fn ids(p: &EffectivePolicy) -> Vec<&str> {
    p.set_aside.iter().map(|i| i.id.as_str()).collect()
}

#[test]
fn allow_dynamic_map_globals_in_operator_file() {
    // T057 (1): parse TOML with the key in operator file, assert effective value is true
    let op = "[policy.default]\nallowDynamicMapGlobals = true\n";
    let ws = "";
    let p = eff(op, ws, T::Untrusted);

    assert!(
        p.allow_dynamic_map_globals.value,
        "operator file sets allow_dynamic_map_globals"
    );
    assert!(p.set_aside.is_empty(), "operator file is never set aside");
}

#[test]
fn allow_dynamic_map_globals_set_aside_from_untrusted() {
    // T057 (2): same key in untrusted workspace file is set aside
    let op = "";
    let ws = "[policy.default]\nallowDynamicMapGlobals = true\n";
    let p = eff(op, ws, T::Untrusted);

    assert!(
        !p.allow_dynamic_map_globals.value,
        "untrusted workspace file cannot loosen"
    );
    assert_eq!(
        ids(&p),
        vec!["workspace:policy.default.allowDynamicMapGlobals"],
        "setting should be set aside"
    );
}

#[test]
fn allow_dynamic_map_globals_defaults_false() {
    // T057 (3): default is false
    let op = "";
    let ws = "";
    let p = eff(op, ws, T::Untrusted);

    assert!(!p.allow_dynamic_map_globals.value, "default is false");
}

#[test]
fn allow_dynamic_map_globals_trusted_workspace() {
    // Trusted workspace file can set it
    let op = "";
    let ws = "[policy.default]\nallowDynamicMapGlobals = true\n";
    let p = eff(op, ws, T::Trusted);

    assert!(
        p.allow_dynamic_map_globals.value,
        "trusted workspace file can loosen"
    );
    assert!(p.set_aside.is_empty(), "trusted workspace is not set aside");
}

// ── T088: values in compare/preview/propose answers follow the data policy (FR-021) ──────────

mod t088 {
    use iris_agentic_dev_core::tools::storage_map::sample::values_refusal;

    #[test]
    fn t088_each_policy_and_include_values() {
        for policy in ["allow", "block", "redact"] {
            // No values asked for: nothing to refuse, whatever the policy.
            assert!(
                values_refusal(false, false, policy, "s1").is_none(),
                "{policy}"
            );
            assert!(
                values_refusal(false, true, policy, "s1").is_none(),
                "{policy}"
            );
        }
        // Under allow, values need acknowledgePhi.
        let r = values_refusal(true, false, "allow", "s1").expect("allow without ack");
        assert_eq!(r["error_code"], "PHI_ACK_REQUIRED", "{r}");
        assert!(values_refusal(true, true, "allow", "s1").is_none());
        // Under block and redact, values are refused, acknowledged or not.
        for policy in ["block", "redact"] {
            for ack in [false, true] {
                let r = values_refusal(true, ack, policy, "s1").expect("refused");
                assert_eq!(
                    r["error_code"], "DATA_POLICY_BLOCKED",
                    "{policy} {ack}: {r}"
                );
                assert_eq!(r["server_name"], "s1", "{r}");
            }
        }
    }
}

//! Spec 141 R5 C16: the durable telemetry record goes to the local file; the IRIS sink is an
//! operator-only opt-in (`telemetry_sink = "iris"` in the operator file, or `IAD_TELEMETRY_SINK=iris`
//! in the launch settings).
//!
//! The cases parse TOML strings. A workspace file that sets the key has it set aside, trusted or
//! not, and turning it on while a server runs is a loosening that waits for the next start.

use iris_agentic_dev_core::iris::config_sources::{
    operator_only_items, ConfigSnapshot, LoadInputs, SetAsideKind, CONFIG_FILE_NAME,
};
use iris_agentic_dev_core::iris::operator_hold::hold_loosening;
use iris_agentic_dev_core::telemetry::{sink_setting, TelemetrySink};
use std::path::Path;

fn table(s: &str) -> toml::Table {
    s.parse().unwrap_or_else(|e| panic!("{e}: {s}"))
}

fn load(operator: Option<&str>, workspace: Option<&str>) -> ConfigSnapshot {
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path().join("home");
    let proj = home.join("proj");
    std::fs::create_dir_all(&proj).unwrap();
    if let Some(t) = operator {
        std::fs::write(home.join(CONFIG_FILE_NAME), t).unwrap();
    }
    if let Some(t) = workspace {
        std::fs::write(proj.join(CONFIG_FILE_NAME), t).unwrap();
    }
    ConfigSnapshot::load(&LoadInputs {
        cwd: std::fs::canonicalize(&proj).unwrap(),
        home: Some(std::fs::canonicalize(&home).unwrap()),
        ..Default::default()
    })
}

#[test]
fn the_default_is_the_local_file() {
    assert_eq!(
        load(None, None).operator.telemetry_sink,
        TelemetrySink::Local
    );
    assert_eq!(
        load(Some("write_tools_enabled = true\n"), None)
            .operator
            .telemetry_sink,
        TelemetrySink::Local,
        "a writable server does not make the IRIS sink the default"
    );
}

#[test]
fn the_operator_file_opts_in() {
    for body in [
        "telemetry_sink = \"iris\"\n",
        "telemetry_sink = \" IRIS \"\n",
    ] {
        assert_eq!(
            load(Some(body), None).operator.telemetry_sink,
            TelemetrySink::Iris,
            "{body:?}"
        );
    }
    assert_eq!(
        load(Some("telemetry_sink = \"local\"\n"), None)
            .operator
            .telemetry_sink,
        TelemetrySink::Local
    );
}

#[test]
fn an_unknown_value_stays_local_and_says_so() {
    let snap = load(Some("telemetry_sink = \"s3\"\n"), None);
    assert_eq!(snap.operator.telemetry_sink, TelemetrySink::Local);
    assert!(
        snap.config_errors
            .iter()
            .any(|e| e.contains("telemetry_sink")),
        "{:?}",
        snap.config_errors
    );
}

#[test]
fn the_launch_setting_opts_in_and_nothing_else_does() {
    let local = load(None, None).operator;
    assert_eq!(sink_setting(&local, None), TelemetrySink::Local);
    assert_eq!(sink_setting(&local, Some("")), TelemetrySink::Local);
    assert_eq!(sink_setting(&local, Some("local")), TelemetrySink::Local);
    assert_eq!(sink_setting(&local, Some("1")), TelemetrySink::Local);
    assert_eq!(sink_setting(&local, Some("iris")), TelemetrySink::Iris);
    assert_eq!(sink_setting(&local, Some(" IRIS ")), TelemetrySink::Iris);
    let iris = load(Some("telemetry_sink = \"iris\"\n"), None).operator;
    assert_eq!(sink_setting(&iris, None), TelemetrySink::Iris);
}

#[test]
fn a_workspace_file_cannot_set_it() {
    let t = table("telemetry_sink = \"iris\"\n");
    let items = operator_only_items(&t, Path::new("/proj/.iris-agentic-dev.toml"));
    assert_eq!(items.len(), 1, "{items:?}");
    assert_eq!(items[0].id, "workspace:telemetry_sink");
    assert_eq!(items[0].kind, SetAsideKind::OperatorOnly);
    assert!(items[0].setting.contains("iris"), "{}", items[0].setting);

    // Asking for the local file asks for nothing.
    let t = table("telemetry_sink = \"local\"\n");
    assert!(operator_only_items(&t, Path::new("/p/.iris-agentic-dev.toml")).is_empty());

    let snap = load(None, Some("telemetry_sink = \"iris\"\n"));
    assert_eq!(snap.operator.telemetry_sink, TelemetrySink::Local);
    assert!(
        snap.set_aside
            .iter()
            .any(|i| i.id == "workspace:telemetry_sink" && i.kind == SetAsideKind::OperatorOnly),
        "{:?}",
        snap.set_aside
    );
}

fn held(applied: &str, incoming: &str) -> (toml::Table, Vec<String>) {
    let (t, h) = hold_loosening(&table(applied), table(incoming));
    (t, h.into_iter().map(|h| h.id).collect())
}

#[test]
fn turning_it_on_during_a_session_waits_for_the_next_start() {
    for applied in ["namespace = \"USER\"\n", "telemetry_sink = \"local\"\n"] {
        let (kept, ids) = held(applied, "telemetry_sink = \"iris\"\n");
        assert_eq!(
            ids,
            vec!["operator:telemetry_sink".to_string()],
            "{applied:?}"
        );
        assert_ne!(
            kept.get("telemetry_sink").and_then(|v| v.as_str()),
            Some("iris"),
            "the server keeps running on what it started with: {kept:?}"
        );
    }
}

#[test]
fn turning_it_off_or_leaving_it_on_applies_at_once() {
    for (applied, incoming) in [
        (
            "telemetry_sink = \"iris\"\n",
            "telemetry_sink = \"local\"\n",
        ),
        ("telemetry_sink = \"iris\"\n", "namespace = \"USER\"\n"),
        ("telemetry_sink = \"iris\"\n", "telemetry_sink = \"iris\"\n"),
    ] {
        let (t, ids) = held(applied, incoming);
        assert!(ids.is_empty(), "{applied:?} -> {incoming:?} held {ids:?}");
        assert_eq!(t, table(incoming));
    }
}

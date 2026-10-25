//! Spec 141 T190: the skills name the `connection_source` values `check_config` returns.
//!
//! `iris-agentic-dev` listed `env_var`, `docker` and `server_manager`; `iris-windows-iis-setup`
//! told the reader to expect `http` and read `docker` as "the config file is not being picked up".
//! `check_config` returns `config_file`, `env_vars`, `iris_select_container` or `auto_discovered`
//! (T188), so an agent following either skill compared the answer to values that never occur. The
//! list of real values is the one `test_check_config_source_values_141` derives from the enum.

use crate::test_check_config_source_values_141::every_value_the_code_can_return;
use std::path::PathBuf;

fn skill(name: &str) -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../skills/skills")
        .join(name)
        .join("SKILL.md");
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("reading {}: {e}", path.display()))
}

/// Every quoted value a skill puts next to `connection_source`: the first quoted string after the
/// word, on the same line, whether it is written `connection_source: "x"`, `"connection_source":
/// "x"` or ``connection_source` is `"x"``.
fn values_named(text: &str) -> Vec<String> {
    let mut found = Vec::new();
    for line in text.lines() {
        let mut rest = line;
        while let Some(at) = rest.find("connection_source") {
            rest = &rest[at + "connection_source".len()..];
            let after_key = rest.trim_start_matches(['"', '`']);
            if let Some(value) = after_key.split('"').nth(1) {
                found.push(value.to_string());
            }
        }
    }
    found
}

#[test]
fn the_scan_reads_each_way_a_skill_writes_the_field() {
    let sample = "| `connected: true, connection_source: \"env_var\"` | x |\n\
                  - `\"connection_source\": \"http\"`\n\
                  If `connection_source` is `\"docker\"`, the file";
    assert_eq!(values_named(sample), vec!["env_var", "http", "docker"]);
}

#[test]
fn skills_name_only_connection_source_values_check_config_returns() {
    let real = every_value_the_code_can_return();
    for name in ["iris-agentic-dev", "iris-windows-iis-setup"] {
        let values = values_named(&skill(name));
        assert!(
            !values.is_empty(),
            "{name} no longer shows a connection_source value; this guard has nothing to check"
        );
        for value in &values {
            assert!(
                real.contains(value),
                "{name} names connection_source \"{value}\", which check_config never returns \
                 (it returns {real:?})"
            );
        }
    }
}

#[test]
fn the_setup_skill_lists_every_value_in_its_reading_table() {
    let text = skill("iris-agentic-dev");
    let named: std::collections::BTreeSet<String> = values_named(&text).into_iter().collect();
    assert_eq!(
        named,
        every_value_the_code_can_return(),
        "the table under \"Start with check_config\" must explain each value"
    );
}

#[test]
fn the_old_values_do_not_come_back() {
    for (name, old) in [
        ("iris-agentic-dev", "\"env_var\""),
        ("iris-agentic-dev", "\"docker\""),
        ("iris-agentic-dev", "\"server_manager\""),
        ("iris-windows-iis-setup", "\"http\""),
        ("iris-windows-iis-setup", "\"docker\""),
    ] {
        let near_field = skill(name)
            .lines()
            .filter(|l| l.contains("connection_source"))
            .any(|l| l.contains(old));
        assert!(
            !near_field,
            "{name} brings back the connection_source value {old}"
        );
    }
}

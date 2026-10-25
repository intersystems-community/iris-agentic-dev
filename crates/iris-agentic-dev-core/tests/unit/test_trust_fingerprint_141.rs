//! Spec 141 FR-030, FR-030a: what a trust entry fingerprints, how it is looked up, and what ends it.
//!
//! The expected lists are written out in full, never computed by the function under test.

use iris_agentic_dev_core::iris::config_sources::{
    loosening_settings, trust_entries, trust_state, Change, TrustEntry, TrustState, TRUST_FORMAT,
};
use std::path::Path;

fn table(s: &str) -> toml::Table {
    s.parse::<toml::Table>().expect("test TOML parses")
}

fn entry(path: &str, settings: &[&str]) -> TrustEntry {
    TrustEntry {
        path: path.to_string(),
        format: TRUST_FORMAT,
        settings: settings.iter().map(|s| s.to_string()).collect(),
    }
}

// T-R4a-U06
#[test]
fn loosening_settings_lists_every_row_of_the_table() {
    let t = table(
        r#"
write_tools_enabled = true
destructive_tools_enabled = true
compile_time_gate = false
tls_verify = false
ssh_host = "jump.example"
host = "localhost"
web_port = 52780
namespace = "USER"

[policy.default]
writeTools = true
destructiveTools = true
compileTimeGate = false
mcpTemplate = "Dev"
allow = ["Query", "compile", "query"]
dataPolicy = "Allow"
dataPolicyKillAllowlist = ["^Zed*", "^Abc"]

[policy.prod]
mcpTemplate = "test"

[instance.prod]
host = "prod.internal"
web_port = 443
scheme = "https"
role = "workspace"
service_username = "svc"
namespace = "PRODNS"
"#,
    );
    let expected: Vec<&str> = vec![
        r#"compile_time_gate = false"#,
        r#"connection = "http://localhost:52780""#,
        r#"connection.namespace = "USER""#,
        r#"destructive_tools_enabled = true"#,
        r#"instance.prod.address = "https://prod.internal:443""#,
        r#"instance.prod.namespace = "PRODNS""#,
        r#"instance.prod.role = "workbench""#,
        r#"instance.prod.service_username = "svc""#,
        r#"policy.default.allow = ["compile", "query"]"#,
        r#"policy.default.compileTimeGate = false"#,
        r#"policy.default.dataPolicy = "allow""#,
        r#"policy.default.dataPolicyKillAllowlist = ["^Abc", "^Zed*"]"#,
        r#"policy.default.destructiveTools = true"#,
        r#"policy.default.mcpTemplate = "dev""#,
        r#"policy.default.writeTools = true"#,
        r#"policy.prod.mcpTemplate = "test""#,
        r#"ssh_host = "jump.example""#,
        r#"tls_verify = false"#,
        r#"write_tools_enabled = true"#,
    ];
    assert_eq!(loosening_settings(&t), expected);
}

// T-R4a-U07
#[test]
fn a_file_that_only_tightens_has_no_loosening_settings() {
    let t = table(
        r#"
write_tools_enabled = false
destructive_tools_enabled = false
compile_time_gate = true
tls_verify = true
host = "prod.internal"
web_port = 443
namespace = "PROD"
disabled_tools = ["iris_doc"]

[policy.default]
writeTools = false
destructiveTools = false
compileTimeGate = true
mcpTemplate = "live"
allow = []
dataPolicy = "block"
globalBlocklist = ["^Secret*"]
dataPolicyKillAllowlist = []
irisAudit = true

[instance.prod]
host = "prod.internal"
role = "subject"
"#,
    );
    assert_eq!(loosening_settings(&t), Vec::<String>::new());
}

// T-R4a-U08
#[test]
fn canonical_form_ignores_case_order_duplicates_and_the_role_alias() {
    let a = table("[policy.default]\nallow = [\"Query\", \"compile\", \"query\"]\n");
    let b = table("[policy.default]\nallow = [\"compile\", \"query\"]\n");
    assert_eq!(loosening_settings(&a), loosening_settings(&b));
    assert_eq!(
        loosening_settings(&a),
        vec![r#"policy.default.allow = ["compile", "query"]"#]
    );

    let old = table("[instance.p]\nrole = \"workspace\"\n");
    let new = table("[instance.p]\nrole = \"workbench\"\n");
    assert_eq!(loosening_settings(&old), loosening_settings(&new));
    assert_eq!(
        loosening_settings(&old),
        vec![
            r#"instance.p.address = "http://localhost:52773""#,
            r#"instance.p.role = "workbench""#,
        ],
        "an `[instance.p]` block defines an address (the defaults), and the trust covers it"
    );
}

// T-R4a-U09
#[test]
fn trust_lookup_trusted_lapsed_untrusted_and_old_format() {
    let path = "/proj/.iris-agentic-dev.toml";
    let file = table("write_tools_enabled = true\n");
    let entries = vec![entry(path, &["write_tools_enabled = true"])];

    assert_eq!(
        trust_state(Path::new(path), &file, &entries),
        TrustState::Trusted
    );

    // One more loosening value: lapsed, and the change names the key.
    let more = table("write_tools_enabled = true\n[policy.default]\ndataPolicy = \"allow\"\n");
    assert_eq!(
        trust_state(Path::new(path), &more, &entries),
        TrustState::Lapsed {
            changed: vec![Change {
                setting: "policy.default.dataPolicy".to_string(),
                was: None,
                now: Some("\"allow\"".to_string()),
            }]
        }
    );

    // A value that was trusted and is now gone is a change too.
    let less = table("compile_time_gate = false\n");
    let TrustState::Lapsed { changed } = trust_state(Path::new(path), &less, &entries) else {
        panic!("a different set of loosening settings must lapse");
    };
    assert!(changed.contains(&Change {
        setting: "write_tools_enabled".to_string(),
        was: Some("true".to_string()),
        now: None,
    }));
    assert!(changed.contains(&Change {
        setting: "compile_time_gate".to_string(),
        was: None,
        now: Some("false".to_string()),
    }));

    // An entry for another path does not trust this file.
    let other = vec![entry(
        "/other/.iris-agentic-dev.toml",
        &["write_tools_enabled = true"],
    )];
    assert_eq!(
        trust_state(Path::new(path), &file, &other),
        TrustState::Untrusted
    );
    assert_eq!(
        trust_state(Path::new(path), &file, &[]),
        TrustState::Untrusted
    );

    // An entry written under an older canonical form lapses on purpose.
    let mut old = entry(path, &["write_tools_enabled = true"]);
    old.format = TRUST_FORMAT - 1;
    let TrustState::Lapsed { changed } = trust_state(Path::new(path), &file, &[old]) else {
        panic!("an older format must lapse");
    };
    assert_eq!(
        changed,
        vec![Change {
            setting: "format".to_string(),
            was: Some((TRUST_FORMAT - 1).to_string()),
            now: Some(TRUST_FORMAT.to_string()),
        }]
    );

    // No loosening settings: nothing to trust, whatever the entries say.
    let tame = table("namespace = \"USER\"\n");
    assert_eq!(
        trust_state(Path::new(path), &tame, &[]),
        TrustState::NotNeeded
    );
}

#[test]
fn trust_entries_are_read_from_the_operator_table() {
    let op = table(
        r#"
[[trust]]
path = "/a/.iris-agentic-dev.toml"
format = 1
settings = ['write_tools_enabled = true', 'connection = "http://localhost:52780"']
trusted_at = "2026-10-23T14:02:11Z"

[[trust]]
path = "/b/.iris-agentic-dev.toml"
format = 1
settings = []
"#,
    );
    assert_eq!(
        trust_entries(&op),
        vec![
            entry(
                "/a/.iris-agentic-dev.toml",
                &[
                    "write_tools_enabled = true",
                    r#"connection = "http://localhost:52780""#
                ]
            ),
            entry("/b/.iris-agentic-dev.toml", &[]),
        ]
    );
    assert!(trust_entries(&table("namespace = \"USER\"\n")).is_empty());
}

// T-R4a-U10
#[test]
fn trust_covers_the_address_the_settings_apply_to() {
    let path = Path::new("/proj/.iris-agentic-dev.toml");
    let trusted = [entry(
        "/proj/.iris-agentic-dev.toml",
        &[
            r#"connection = "http://localhost:52780""#,
            r#"connection.namespace = "USER""#,
            "write_tools_enabled = true",
        ],
    )];
    let base = "write_tools_enabled = true\nhost = \"localhost\"\nweb_port = 52780\nnamespace = \"USER\"\n";
    assert_eq!(
        trust_state(path, &table(base), &trusted),
        TrustState::Trusted
    );

    for (what, edited, key, was, now) in [
        (
            "host",
            base.replace("localhost", "prod.internal"),
            "connection",
            "\"http://localhost:52780\"",
            "\"http://prod.internal:52780\"",
        ),
        (
            "port",
            base.replace("52780", "52781"),
            "connection",
            "\"http://localhost:52780\"",
            "\"http://localhost:52781\"",
        ),
        (
            "namespace",
            base.replace("USER", "PROD"),
            "connection.namespace",
            "\"USER\"",
            "\"PROD\"",
        ),
    ] {
        let TrustState::Lapsed { changed } = trust_state(path, &table(&edited), &trusted) else {
            panic!("a changed {what} must end trust");
        };
        assert!(
            changed.contains(&Change {
                setting: key.to_string(),
                was: Some(was.to_string()),
                now: Some(now.to_string()),
            }),
            "{what}: {changed:?}"
        );
    }

    // A file with no loosening settings does not fingerprint its host.
    let tame = table("host = \"prod.internal\"\nweb_port = 443\n");
    assert_eq!(loosening_settings(&tame), Vec::<String>::new());
    assert_eq!(trust_state(path, &tame, &[]), TrustState::NotNeeded);

    // An edit to a key that is neither loosening nor an address leaves trust alone (FR-030a).
    let tool_list = format!("{base}disabled_tools = [\"iris_doc\"]\n");
    assert_eq!(
        trust_state(path, &table(&tool_list), &trusted),
        TrustState::Trusted
    );
}

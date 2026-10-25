//! Spec 141 FR-036, T-R4a-U25: the `trust` editor changes only the trust entry it was asked to.
//!
//! Operators keep comments, `[[servers]]` tables and policies in `~/.iris-agentic-dev.toml`. A
//! rewrite through a TOML round trip would drop the comments.

use iris_agentic_dev::cmd::trust::{edit_operator_file, revoke_entry};
use iris_agentic_dev_core::iris::config_sources::{trust_entries, TrustEntry, TRUST_FORMAT};

const OPERATOR: &str = r#"# Tom's operator file
# keep this comment exactly

[policy.default]
mcpTemplate = "live" # an inline comment

[[servers]]
name = "dev"
host = "localhost"

[[trust]]
# the entry for A
path = "/a/.iris-agentic-dev.toml"
format = 1
settings = ['write_tools_enabled = true']
trusted_at = "2026-10-01T00:00:00Z"

[[trust]]
path = "/b/.iris-agentic-dev.toml"
format = 1
settings = ['compile_time_gate = false']
trusted_at = "2026-10-02T00:00:00Z"

# trailing comment
"#;

fn entry(path: &str, settings: &[&str]) -> TrustEntry {
    TrustEntry {
        path: path.to_string(),
        format: TRUST_FORMAT,
        settings: settings.iter().map(|s| s.to_string()).collect(),
    }
}

fn parsed(src: &str) -> Vec<TrustEntry> {
    trust_entries(
        &src.parse::<toml::Table>()
            .expect("the edited file is valid TOML"),
    )
}

#[test]
fn replacing_an_entry_keeps_comments_and_every_other_byte() {
    let new = entry(
        "/a/.iris-agentic-dev.toml",
        &[
            "write_tools_enabled = true",
            r#"policy.default.dataPolicy = "allow""#,
        ],
    );
    let out = edit_operator_file(OPERATOR, &new, "2026-10-23T14:02:11Z").unwrap();

    // Everything before the first trust entry is byte-identical.
    let head_end = OPERATOR.find("[[trust]]").unwrap();
    assert!(
        out.starts_with(&OPERATOR[..head_end]),
        "head changed:\n{out}"
    );
    // The other entry survives byte for byte.
    let b_start = OPERATOR.find("[[trust]]\npath = \"/b/").unwrap();
    let b_end = OPERATOR.find("# trailing comment").unwrap();
    let b_block = &OPERATOR[b_start..b_end];
    assert!(out.contains(b_block), "entry B changed:\n{out}");
    for comment in [
        "# Tom's operator file",
        "# keep this comment exactly",
        "# an inline comment",
        "# trailing comment",
    ] {
        assert_eq!(out.matches(comment).count(), 1, "{comment}:\n{out}");
    }

    // One entry for A, with the new settings; B untouched.
    let entries = parsed(&out);
    assert_eq!(entries.len(), 2, "{out}");
    let a: Vec<_> = entries
        .iter()
        .filter(|e| e.path == "/a/.iris-agentic-dev.toml")
        .collect();
    assert_eq!(a.len(), 1);
    assert_eq!(a[0].settings, new.settings);
    assert_eq!(a[0].format, TRUST_FORMAT);
    assert!(
        out.contains("trusted_at = \"2026-10-23T14:02:11Z\""),
        "{out}"
    );
    assert!(
        !out.contains("2026-10-01T00:00:00Z"),
        "the old entry for A is gone:\n{out}"
    );
    assert!(
        entries.iter().any(|e| e.path == "/b/.iris-agentic-dev.toml"
            && e.settings == ["compile_time_gate = false"]),
        "{out}"
    );
}

#[test]
fn a_new_path_appends_and_an_empty_file_is_created() {
    let new = entry("/c/.iris-agentic-dev.toml", &["write_tools_enabled = true"]);
    let out = edit_operator_file(OPERATOR, &new, "2026-10-23T14:02:11Z").unwrap();
    // Every existing entry and table first; the document's closing comment stays in the file (a
    // TOML editor keeps it as the document's tail, after the new entry).
    let before_tail = &OPERATOR[..OPERATOR.find("# trailing comment").unwrap()];
    assert!(
        out.starts_with(before_tail.trim_end()),
        "existing text first:\n{out}"
    );
    assert_eq!(out.matches("# trailing comment").count(), 1, "{out}");
    assert_eq!(parsed(&out).len(), 3);

    let out = edit_operator_file("", &new, "2026-10-23T14:02:11Z").unwrap();
    assert_eq!(parsed(&out), vec![new.clone()]);
    assert!(out.contains("[[trust]]"));
}

#[test]
fn a_file_that_does_not_parse_is_refused_and_not_rewritten() {
    let new = entry("/c/.iris-agentic-dev.toml", &["write_tools_enabled = true"]);
    assert!(edit_operator_file("[[trust\nx", &new, "2026-10-23T14:02:11Z").is_err());
    assert!(revoke_entry("[[trust\nx", "/c/.iris-agentic-dev.toml").is_err());
}

#[test]
fn revoking_removes_only_that_entry() {
    let (out, found) = revoke_entry(OPERATOR, "/a/.iris-agentic-dev.toml").unwrap();
    assert!(found);
    let entries = parsed(&out);
    assert_eq!(entries.len(), 1, "{out}");
    assert_eq!(entries[0].path, "/b/.iris-agentic-dev.toml");
    assert!(out.contains("# keep this comment exactly"));
    assert!(out.contains("# trailing comment"));
    assert!(out.contains("[[servers]]"));

    let (same, found) = revoke_entry(OPERATOR, "/nope/.iris-agentic-dev.toml").unwrap();
    assert!(!found);
    assert_eq!(same, OPERATOR, "nothing to revoke: not a byte changes");
}

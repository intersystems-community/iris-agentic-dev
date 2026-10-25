//! Spec 131 US1: `iris_info what=sa_schema` says what it takes.
//!
//! `sa_schema` calls Atelier `/saschema/<url>`, the Studio Assist grammar for an XData namespace
//! URL. It was described as "SQL Analytics schema", and PR 142's skill told agents to pass it a cube
//! name, which gets a 404 and an empty result. A name that is not a URL is now refused before any
//! IRIS call, with text that says where cube discovery really lives.

use iris_agentic_dev_core::tools::info::{
    sa_schema_name_problem, sa_schema_path, sa_schema_url_problem, InfoParams, SA_SCHEMA_GUIDANCE,
};
use std::path::Path;

#[test]
fn a_cube_name_is_refused_with_the_guidance() {
    let msg = sa_schema_name_problem(Some("HoleFoods")).expect("a cube name is not a URL");
    assert!(
        msg.contains("HoleFoods"),
        "the error echoes the name: {msg}"
    );
    assert!(msg.contains(SA_SCHEMA_GUIDANCE), "{msg}");
}

#[test]
fn an_empty_or_missing_name_is_refused() {
    assert!(sa_schema_name_problem(None).is_some());
    assert!(sa_schema_name_problem(Some("")).is_some());
    assert!(sa_schema_name_problem(Some("   ")).is_some());
}

#[test]
fn a_class_or_table_name_is_refused() {
    for name in ["SQLUser.Orders", "%Library.Object", "USER"] {
        assert!(sa_schema_name_problem(Some(name)).is_some(), "{name}");
    }
}

#[test]
fn an_xdata_namespace_url_passes() {
    for name in [
        "http://www.intersystems.com/deepsee",
        "https://example.com/schema",
        " http://www.intersystems.com/deepsee ",
    ] {
        assert_eq!(sa_schema_name_problem(Some(name)), None, "{name}");
    }
}

#[test]
fn the_guidance_says_what_the_param_takes_and_where_cubes_are() {
    for needle in [
        "XData namespace URL",
        "http://www.intersystems.com/deepsee",
        "%DeepSee.Utils",
        "%GetCubeList",
        "%GetDimensionList",
        "iris_execute",
    ] {
        assert!(
            SA_SCHEMA_GUIDANCE.contains(needle),
            "guidance lacks {needle}: {SA_SCHEMA_GUIDANCE}"
        );
    }
}

/// The 129 rule: error and hint text never names a skill.
#[test]
fn the_guidance_names_no_skill() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../skills/skills");
    let mut names: Vec<String> = std::fs::read_dir(&dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| e.path().join("SKILL.md").exists())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    assert!(names.len() > 10);
    names.push("iris-mdx".into());
    for n in &names {
        assert!(!SA_SCHEMA_GUIDANCE.contains(n.as_str()), "names {n}");
    }
    assert!(!SA_SCHEMA_GUIDANCE.to_lowercase().contains("skill"));
}

#[test]
fn name_round_trips_as_a_url() {
    let p: InfoParams = serde_json::from_str(
        r#"{"what":"sa_schema","name":"http://www.intersystems.com/deepsee"}"#,
    )
    .unwrap();
    assert_eq!(
        p.name.as_deref(),
        Some("http://www.intersystems.com/deepsee")
    );
}

#[test]
fn the_name_doc_and_tool_description_drop_sql_analytics() {
    let info = include_str!("../../src/tools/info.rs");
    let mods = iris_agentic_dev_core::testing::tools_mod_source();
    for (file, src) in [("info.rs", info), ("mod.rs", mods)] {
        assert!(
            !src.contains("SQL Analytics"),
            "{file} still calls sa_schema SQL Analytics"
        );
    }
    assert!(
        info.contains("/// XData namespace URL for what=sa_schema"),
        "InfoParams.name doc must say it is a URL"
    );
    assert!(mods.contains("Studio Assist grammar"));
}

/// Atelier 404s `/saschema/http%3A%2F%2F...`: the slashes must stay raw. iad encoded the whole URL as
/// one segment, so `sa_schema` never returned a grammar for any name.
#[test]
fn the_url_keeps_its_slashes_in_the_path() {
    assert_eq!(
        sa_schema_path("http://www.intersystems.com/deepsee"),
        "/saschema/http%3A//www.intersystems.com/deepsee"
    );
    assert_eq!(
        sa_schema_path(" https://example.com/a b "),
        "/saschema/https%3A//example.com/a%20b"
    );
}

/// Spec 141 C5: a dot segment would make the HTTP client resolve the path off the Studio Assist
/// prefix (`/saschema/http://host/../..` reaches any path on the origin), so it is refused.
#[test]
fn sa_schema_refuses_dot_segments() {
    for name in [
        "http://x/../..",
        "http://x/./a",
        "https://x/a/..",
        "http://x/a/../b",
    ] {
        assert!(
            sa_schema_name_problem(Some(name)).is_some(),
            "a dot segment must be refused: {name}"
        );
    }
}

/// Spec 141 C5, second check: once the HTTP client has resolved the URL, its path must still start
/// with the Studio Assist prefix. The name check above catches a dot segment before the request;
/// this catches any path that leaves the prefix after resolution.
#[test]
fn sa_schema_url_keeps_the_prefix() {
    let prefix = "http://localhost:52780/api/atelier/v8/USER/saschema/";
    assert_eq!(
        sa_schema_url_problem(
            "http://localhost:52780/api/atelier/v8/USER/saschema/http%3A//www.intersystems.com/deepsee",
            prefix
        ),
        None
    );
    for url in [
        "http://localhost:52780/api/atelier/v8/USER/saschema/x/../../action/query",
        "http://localhost:52780/api/atelier/v8/USER/saschema/x/../../../",
    ] {
        assert!(
            sa_schema_url_problem(url, prefix).is_some(),
            "a URL that resolves off the prefix must be refused: {url}"
        );
    }
}

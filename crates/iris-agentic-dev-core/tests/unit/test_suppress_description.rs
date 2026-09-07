//! `IRIS_SUPPRESS_TOOL_DESCRIPTION` parsing (113 T036a).
//!
//! The flag exists to answer one question the rest of the suite cannot: is the advertised schema
//! sufficient on its own? Every other test lets the agent read the description, so a schema that
//! declares the wrong thing still passes as long as the prose explains it. Suppressing the
//! description leaves the schema as the only contract, which is what SC-007 measures.
//!
//! Parsing is tested from strings rather than a `Vec<String>` literal because the value arrives as
//! one env var: a caller writing `--suppress-tool-description a --suppress-tool-description b`
//! becomes `"a,b"`, and a caller setting the var by hand may write spaces, a trailing comma, or
//! both. Each of those has to mean the same thing.

use iris_agentic_dev_core::tools::parse_suppressed_descriptions;

#[test]
fn a_single_name_is_the_whole_set() {
    let set = parse_suppressed_descriptions("stream_inspect");
    assert_eq!(set.len(), 1);
    assert!(set.contains("stream_inspect"));
}

#[test]
fn commas_spaces_and_a_trailing_separator_all_mean_the_same_thing() {
    let canonical = parse_suppressed_descriptions("iris_admin,stream_inspect");
    for spelling in [
        "iris_admin, stream_inspect",
        " iris_admin , stream_inspect ",
        "iris_admin,stream_inspect,",
        "iris_admin stream_inspect",
        "iris_admin,,stream_inspect",
    ] {
        assert_eq!(
            parse_suppressed_descriptions(spelling),
            canonical,
            "`{spelling}` parsed to a different set than `iris_admin,stream_inspect`"
        );
    }
}

#[test]
fn an_empty_or_whitespace_value_suppresses_nothing() {
    // The distinction that matters: an unset var and a var set to "" must behave identically.
    // `empty-config-value` is in the Bug Class Registry because `build.rustc-wrapper = ""` was
    // read as "a wrapper named ''" and broke every coverage run.
    for spelling in ["", " ", ",", " , , "] {
        assert!(
            parse_suppressed_descriptions(spelling).is_empty(),
            "`{spelling}` should suppress nothing"
        );
    }
}

#[test]
fn the_wildcard_is_not_a_name() {
    // `*` would be a convenient "suppress everything", and it is deliberately absent: SC-007 is
    // measured per tool, and a run that blanked all 84 descriptions at once could not tell which
    // tool's schema carried the call. If it is ever wanted it should be an explicit flag.
    let set = parse_suppressed_descriptions("*");
    assert_eq!(set.len(), 1);
    assert!(
        set.contains("*"),
        "`*` is treated as a literal tool name, which matches nothing"
    );
}

#[test]
fn the_env_var_is_registered_as_behavior_changing() {
    // Principle XII: a var that changes what the server advertises and is not in
    // BEHAVIOR_ENV_VARS leaks from the developer's shell into every spawn test.
    assert!(
        iris_agentic_dev_core::testing::BEHAVIOR_ENV_VARS
            .contains(&"IRIS_SUPPRESS_TOOL_DESCRIPTION"),
        "IRIS_SUPPRESS_TOOL_DESCRIPTION must be in BEHAVIOR_ENV_VARS so clean_mcp_command strips it"
    );
}

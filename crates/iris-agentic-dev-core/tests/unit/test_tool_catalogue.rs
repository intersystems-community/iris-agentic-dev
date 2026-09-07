//! The catalogue the CLI serves is the listing MCP serves.
//!
//! `tool --list` and `tool <name> --schema` exist so a shell-only caller can discover the surface
//! without paying for `tools/list`. That only holds if the two answers come from the same place:
//! a second inventory would drift, and a caller reading the CLI would be told about a tool the
//! server does not have. So `tool_catalogue()` reads `tool_router.list_all()`, and the assertions
//! below compare it against the accessors that back the MCP path.
//!
//! No IRIS connection anywhere in this file — the catalogue is a property of the binary.

use iris_agentic_dev_core::tools::{
    normalize_schema_openapi3, summarize_description, IrisTools, Toolset,
};

fn tools() -> IrisTools {
    IrisTools::new_with_toolset(None, Toolset::Merged).expect("IrisTools::new with no connection")
}

/// The router's schema for `name`, put through the same rewrite `list_tools` applies on the way out.
///
/// `tool_input_schema` hands back what schemars reflected; a client gets that with nullable type
/// arrays rewritten to `anyOf`. The catalogue has to match the client's copy, not the raw one — a
/// `--schema` that prints `"type": ["string", "null"]` where `tools/list` sends `anyOf` is a second
/// contract, which is the whole thing this file exists to rule out. `capability_matrix` is the tool
/// that catches it if the normalization is dropped from either path.
fn client_schema(tools: &IrisTools, name: &str) -> serde_json::Value {
    let mut schema = tools.tool_input_schema(name).expect("registered tool");
    if let serde_json::Value::Object(map) = &mut schema {
        normalize_schema_openapi3(map);
    }
    schema
}

/// One entry per registered tool, no extras, and every field equal to what the MCP accessors return
/// for the same name. This is the single-source guard: if the catalogue is ever built from a second
/// list, one of these three comparisons fails.
#[test]
fn the_catalogue_is_the_router() {
    let tools = tools();
    let registered = tools.registered_tool_names();
    let catalogue = tools.tool_catalogue();

    assert!(
        registered.len() >= 80,
        "only {} tools registered; this test would be comparing a stub surface",
        registered.len()
    );
    assert_eq!(
        catalogue.len(),
        registered.len(),
        "catalogue holds {} entries for {} registered tools",
        catalogue.len(),
        registered.len()
    );

    let mut names: Vec<&str> = catalogue.iter().map(|e| e.name.as_str()).collect();
    let sorted = {
        let mut s = names.clone();
        s.sort_unstable();
        s
    };
    assert_eq!(
        names, sorted,
        "the catalogue must come out sorted by name, so `--list` output is stable between runs"
    );
    names.dedup();
    assert_eq!(names.len(), catalogue.len(), "duplicate names in catalogue");

    for entry in &catalogue {
        assert!(
            registered.contains(&entry.name),
            "catalogue advertises `{}`, which is not registered in this toolset",
            entry.name
        );
        assert_eq!(
            entry.description,
            tools.tool_description(&entry.name),
            "`{}`: catalogue description differs from the one tools/list serves",
            entry.name
        );
        assert_eq!(
            entry.input_schema,
            client_schema(&tools, &entry.name),
            "`{}`: catalogue schema differs from the one tools/list serves",
            entry.name
        );
    }
}

/// Every summary has to be short enough that the whole catalogue fits the 8 KB budget, and non-empty
/// so a caller reading `--list` learns something beyond the name.
#[test]
fn every_summary_is_short_and_says_something() {
    let tools = tools();
    let mut empty: Vec<String> = Vec::new();
    let mut long: Vec<String> = Vec::new();

    for entry in tools.tool_catalogue() {
        if entry.summary.trim().is_empty() {
            empty.push(entry.name.clone());
        }
        if entry.summary.chars().count() > 100 {
            long.push(format!("{} ({} chars)", entry.name, entry.summary.len()));
        }
    }

    assert!(
        empty.is_empty(),
        "these tools produce an empty one-line summary: {empty:?}"
    );
    assert!(
        long.is_empty(),
        "these summaries exceed 100 characters: {long:?}"
    );
}

#[test]
fn the_first_sentence_is_the_summary() {
    assert_eq!(
        summarize_description("Run a SQL query. Returns rows as JSON. Read-only."),
        "Run a SQL query."
    );
}

#[test]
fn a_newline_ends_the_summary_even_without_a_period() {
    assert_eq!(
        summarize_description("Inspect a stream\n\nLonger prose follows here."),
        "Inspect a stream"
    );
}

/// A period inside `e.g.` is not a sentence break. Without this the summary for any description that
/// starts with an example is cut mid-clause.
#[test]
fn an_abbreviation_is_not_a_sentence_break() {
    assert_eq!(
        summarize_description("Compile a class, e.g. My.Class. Errors come back structured."),
        "Compile a class, e.g. My.Class."
    );
}

#[test]
fn a_long_first_sentence_is_cut_on_a_word_boundary() {
    let long =
        "Query the interoperability message log for a production, returning header rows with \
                session identifiers and per-message status so a caller can follow one session end \
                to end.";
    let summary = summarize_description(long);

    assert!(
        summary.chars().count() <= 100,
        "summary is {} chars: {summary}",
        summary.chars().count()
    );
    assert!(
        summary.ends_with('…'),
        "a truncated summary must say so: {summary}"
    );
    let body = summary.trim_end_matches('…');
    assert!(
        long.starts_with(body),
        "truncation must keep a prefix of the description, got {body:?}"
    );
    assert!(
        !body.ends_with(' '),
        "the cut must land on a word boundary with no trailing space: {summary:?}"
    );
    assert!(
        long[body.len()..].starts_with(' '),
        "the cut split a word: {body:?}"
    );
}

/// A description with no sentence break at all still has to produce something under the limit.
#[test]
fn a_description_with_no_break_is_still_bounded() {
    let run_on = "a".repeat(400);
    let summary = summarize_description(&run_on);
    assert!(
        summary.chars().count() <= 100,
        "summary is {} chars",
        summary.chars().count()
    );
}

#[test]
fn an_empty_description_yields_an_empty_summary() {
    assert_eq!(summarize_description(""), "");
    assert_eq!(summarize_description("   \n  "), "");
}

/// Multi-byte input must not panic on a byte-index slice. Every tool description is ASCII today,
/// which is exactly why this would go unnoticed until one is not.
#[test]
fn multibyte_input_does_not_panic() {
    let wide = "サンプル".repeat(60);
    let summary = summarize_description(&wide);
    assert!(summary.chars().count() <= 100);
}

//! Storage safety helpers for `iris_doc`.
//!
//! `iris_doc` writes and compiles `Storage` blocks verbatim — IRIS alone
//! decides how a class's storage evolves on each compile (add gets the next
//! free ordinal, remove leaves a harmless dangling entry, rename requires
//! updating both the property and its `Storage` entry), matching the
//! Atelier API contract Studio and VS Code already rely on. This module
//! never second-guesses that: it only detects the one case a write can't be
//! allowed to pass through silently — dropping an existing `Storage` block
//! entirely — so `iris_doc` can require an explicit, user-confirmed opt-in
//! before permitting it. See `doc.rs`'s `allow_storage_regeneration` handling
//! for that check; this module supplies the structural signal it acts on.
//!
//! Everything else (leaving orphans alone on removal, updating a `Storage`
//! entry's name on rename) is agent guidance, not something this module
//! enforces — see `iris_doc`'s tool description.

/// Which storage generator produced a class's `Storage` block, per its own
/// `<Type>` tag. Used only to inform the agent what cleanup options apply
/// (e.g. `%KillExtent` exists on `Persistent`, not `Serial`) when a reset is
/// permitted — never to gate whether a write is allowed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StorageKind {
    Persistent,
    Serial,
    Unsupported,
}

/// Read the storage generator a class's `Storage` block declares itself as,
/// via its `<Type>` tag. `Unsupported` if there's no `Storage` block, or its
/// `<Type>` isn't one this module recognizes.
pub fn storage_kind(class_source: &str) -> StorageKind {
    let Some(span) = find_storage_block(class_source) else {
        return StorageKind::Unsupported;
    };
    if span.content.contains("<Type>%Storage.Persistent</Type>") {
        StorageKind::Persistent
    } else if span.content.contains("<Type>%Storage.Serial</Type>") {
        StorageKind::Serial
    } else {
        StorageKind::Unsupported
    }
}

/// The exact line span of a `Storage <Name> { ... }` block within a class's
/// source text, plus its inner content.
struct StorageBlockSpan {
    /// 0-based index of the first line strictly inside the block (after the
    /// opening `{`).
    inner_start: usize,
    /// 0-based index of the line holding the block's closing `}` (exclusive
    /// end of the inner content).
    inner_end: usize,
    /// The inner lines, joined by `\n` — everything strictly between the
    /// opening and closing braces.
    content: String,
}

/// Locate the `Storage <Name> { ... }` block in `source`, if present. Handles
/// both `Storage Default {` (brace on the header line) and `Storage Default`
/// followed by `{` on its own line — the opening-brace line, wherever it
/// falls, is never itself part of the recorded inner content.
fn find_storage_block(source: &str) -> Option<StorageBlockSpan> {
    let lines: Vec<&str> = source.lines().collect();
    let mut in_storage = false;
    let mut seen_open_brace = false;
    let mut brace_depth = 0i32;
    let mut inner_start = 0usize;

    for (idx, line) in lines.iter().enumerate() {
        if !in_storage {
            let mut parts = line.split_whitespace();
            if parts.next() == Some("Storage") && parts.next().is_some() {
                in_storage = true;
                if line.contains('{') {
                    seen_open_brace = true;
                    brace_depth += line.matches('{').count() as i32;
                    brace_depth -= line.matches('}').count() as i32;
                    inner_start = idx + 1;
                }
            }
            continue;
        }
        if !seen_open_brace {
            if line.contains('{') {
                seen_open_brace = true;
                brace_depth += line.matches('{').count() as i32;
                brace_depth -= line.matches('}').count() as i32;
                inner_start = idx + 1;
            }
            continue;
        }
        brace_depth += line.matches('{').count() as i32;
        brace_depth -= line.matches('}').count() as i32;
        if brace_depth <= 0 {
            let content = lines[inner_start..idx].join("\n");
            return Some(StorageBlockSpan {
                inner_start,
                inner_end: idx,
                content,
            });
        }
    }
    None
}

/// Whether `source` contains a `Storage <Name> { ... }` block at all — the
/// signal for whether a write would drop existing storage (present
/// server-side, missing from the submitted text).
pub fn has_storage_block(source: &str) -> bool {
    find_storage_block(source).is_some()
}

/// Names of every `Property <Name> As ...;` declared directly in the class
/// body (outside any `Storage` block, so a coincidental match inside
/// `Storage`'s own text — unlikely, but not impossible — can't cause a false
/// property). Case-sensitive, matching ObjectScript identifier semantics.
/// Used to report the pre-reset property list back to the agent when a
/// storage reset is permitted.
pub fn declared_properties(source: &str) -> Vec<String> {
    let storage_span = find_storage_block(source);
    source
        .lines()
        .enumerate()
        .filter(|(idx, _)| {
            storage_span
                .as_ref()
                .map(|s| *idx < s.inner_start || *idx >= s.inner_end)
                .unwrap_or(true)
        })
        .filter_map(|(_, line)| {
            let mut parts = line.split_whitespace();
            if parts.next() != Some("Property") {
                return None;
            }
            parts
                .next()
                .map(|name| name.trim_end_matches(';').to_string())
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_storage_class(properties_and_storage_lines: &[&str]) -> String {
        format!(
            "Class Test.Sample Extends %Persistent\n{{\n\n{}\n\n}}\n",
            properties_and_storage_lines.join("\n")
        )
    }

    // ── storage_kind ─────────────────────────────────────────────────────────

    #[test]
    fn test_storage_kind_persistent() {
        let src = sample_storage_class(&[
            "Property Name As %String;",
            "Storage Default",
            "{",
            "<Type>%Storage.Persistent</Type>",
            "}",
        ]);
        assert_eq!(storage_kind(&src), StorageKind::Persistent);
    }

    #[test]
    fn test_storage_kind_serial() {
        // Real shape from a compiled %SerialObject class: no %%CLASSNAME
        // entry (serial objects have no extent of their own), `<State>`
        // instead of `<DataLocation>`/`<IdLocation>` — neither matters here,
        // only `<Type>` does.
        let src = "Class Test.SerialProbe Extends %SerialObject\n{\nProperty Name As %String;\nStorage Default\n{\n<Data name=\"SerialProbeState\">\n<Value name=\"1\">\n<Value>Name</Value>\n</Value>\n</Data>\n<State>SerialProbeState</State>\n<Type>%Storage.Serial</Type>\n}\n}";
        assert_eq!(storage_kind(src), StorageKind::Serial);
    }

    #[test]
    fn test_storage_kind_sql_is_unsupported() {
        // Real shape from a compiled %Storage.SQL class (e.g. %FileMan.Field):
        // columns are keyed by name in a <SQLMap>, never by ordinal <Value>.
        let src = "Class Test.SqlProbe\n{\nStorage SQLStorage\n{\n<SQLMap name=\"Master\">\n<Data name=\"NAME\">\n<Delimiter>\"^\"</Delimiter>\n</Data>\n</SQLMap>\n<Type>%Storage.SQL</Type>\n}\n}";
        assert_eq!(storage_kind(src), StorageKind::Unsupported);
    }

    #[test]
    fn test_storage_kind_no_storage_block_is_unsupported() {
        let src = "Class Test.NoStorage {\nProperty Name As %String;\n}";
        assert_eq!(storage_kind(src), StorageKind::Unsupported);
    }

    // ── has_storage_block / declared_properties ────────────────────────────

    #[test]
    fn test_has_storage_block_true_when_present() {
        let src = sample_storage_class(&[
            "Property Name As %String;",
            "Storage Default",
            "{",
            "<Type>T</Type>",
            "}",
        ]);
        assert!(has_storage_block(&src));
    }

    #[test]
    fn test_has_storage_block_false_when_absent() {
        let src = sample_storage_class(&["Property Name As %String;"]);
        assert!(!has_storage_block(&src));
    }

    #[test]
    fn test_declared_properties_simple() {
        let src = sample_storage_class(&["Property Name As %String;", "Property Age As %Integer;"]);
        assert_eq!(declared_properties(&src), vec!["Name", "Age"]);
    }

    #[test]
    fn test_declared_properties_ignores_content_inside_storage() {
        // A line inside Storage that happens to start with the literal token
        // "Property" (contrived, but the point is the scan must be scoped
        // outside the block, not that this shape occurs naturally) must not
        // be picked up as a real declaration.
        let src = sample_storage_class(&[
            "Property Name As %String;",
            "Storage Default",
            "{",
            "Property FakeOne As %String;",
            "}",
        ]);
        assert_eq!(declared_properties(&src), vec!["Name"]);
    }

    #[test]
    fn test_declared_properties_none() {
        let src = "Class Foo Extends %Persistent {\n}\n";
        assert!(declared_properties(src).is_empty());
    }

    #[test]
    fn test_declared_properties_handles_maxlen_and_qualifiers() {
        let src = sample_storage_class(&[
            "Property Name As %String(MAXLEN = 80);",
            "Property Flag As %Boolean [ InitialExpression = 0 ];",
        ]);
        assert_eq!(declared_properties(&src), vec!["Name", "Flag"]);
    }

    // ── find_storage_block ──────────────────────────────────────────────────

    #[test]
    fn test_find_storage_block_single_line_header() {
        let src = "Class Foo {\nStorage Default\n{\n<Type>%Storage.Persistent</Type>\n}\n}";
        let span = find_storage_block(src).expect("should find block");
        assert_eq!(span.content, "<Type>%Storage.Persistent</Type>");
    }

    #[test]
    fn test_find_storage_block_brace_on_same_line() {
        let src = "Class Foo {\nStorage Default {\n<Type>%Storage.Persistent</Type>\n}\n}";
        let span = find_storage_block(src).expect("should find block");
        assert_eq!(span.content, "<Type>%Storage.Persistent</Type>");
    }

    #[test]
    fn test_find_storage_block_absent_returns_none() {
        let src = "Class Foo {\nProperty Name As %String;\n}";
        assert!(find_storage_block(src).is_none());
    }
}

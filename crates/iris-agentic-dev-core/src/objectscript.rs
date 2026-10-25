//! Safe embedding of user-supplied strings into generated ObjectScript source.
//!
//! ObjectScript string literals have exactly one escape: a quote is doubled
//! (`""`). Backslash is an ordinary character — C-style `\"` is a syntax error
//! (the `\` stays literal and the `"` terminates the string). A literal cannot
//! span source lines, and `build_exec_class` splits generated code on `\n`, so
//! control characters must never appear inside a literal; they are spliced in
//! via `$CHAR(n,...)` instead.

/// Render `s` as a single-line ObjectScript *expression* that evaluates to
/// exactly `s`: printable runs become quoted literals with `"` doubled,
/// control characters become `$CHAR(n,...)` splices.
///
/// `os_str_expr(r#"say "hi""#)` → `"say ""hi"""`
/// `os_str_expr("a\r\nb")` → `"a"_$CHAR(13,10)_"b"`
pub fn os_str_expr(s: &str) -> String {
    if s.is_empty() {
        return "\"\"".into();
    }
    let mut parts: Vec<String> = Vec::new();
    let mut lit = String::new();
    let mut ctl: Vec<u32> = Vec::new();
    let flush_lit = |lit: &mut String, parts: &mut Vec<String>| {
        if !lit.is_empty() {
            parts.push(format!("\"{}\"", lit.replace('"', "\"\"")));
            lit.clear();
        }
    };
    let flush_ctl = |ctl: &mut Vec<u32>, parts: &mut Vec<String>| {
        if !ctl.is_empty() {
            let codes: Vec<String> = ctl.iter().map(|c| c.to_string()).collect();
            parts.push(format!("$CHAR({})", codes.join(",")));
            ctl.clear();
        }
    };
    for ch in s.chars() {
        let code = ch as u32;
        if code < 0x20 || code == 0x7f {
            flush_lit(&mut lit, &mut parts);
            ctl.push(code);
        } else {
            flush_ctl(&mut ctl, &mut parts);
            lit.push(ch);
        }
    }
    flush_lit(&mut lit, &mut parts);
    flush_ctl(&mut ctl, &mut parts);
    parts.join("_")
}

/// Statements that write `payload` to the open stream held in `stream_var`,
/// chunked so no generated source line approaches the routine line-length
/// limit (worst case a chunk of quotes doubles, plus `$CHAR` overhead).
pub fn os_stream_write_stmts(stream_var: &str, payload: &str, chunk_chars: usize) -> Vec<String> {
    let chars: Vec<char> = payload.chars().collect();
    chars
        .chunks(chunk_chars.max(1))
        .map(|c| {
            let piece: String = c.iter().collect();
            format!("Do {}.Write({})", stream_var, os_str_expr(&piece))
        })
        .collect()
}

/// The text to put *between* the quotes of an existing literal so that `"<this>"` evaluates to `s`.
/// It closes the literal, concatenates `os_str_expr(s)`, and reopens it: `"`, `_`, expr, `_`, `"`.
/// For sites written as `\"{}\"` that cannot easily drop their own quotes.
pub fn os_lit_inner(s: &str) -> String {
    // Printable text needs only its quotes doubled, and then the output reads like hand-written code.
    if !s.chars().any(char::is_control) {
        return s.replace('"', "\"\"");
    }
    format!("\"_{}_\"", os_str_expr(s))
}

// ── Statements built from caller text ───────────────────────────────────────
//
// Each takes free text (a namespace, class, routine or skill name an agent sent) and splices it as
// `os_str_expr(..)`, unquoted, so a quote or a line break in the text stays inside one expression.

/// `EnableNamespace` for `ns` (iris_compile `force_writable`).
pub fn enable_namespace_code(ns: &str) -> String {
    format!(
        "do ##class(%Library.EnsembleMgr).EnableNamespace({},1)",
        os_str_expr(ns)
    )
}

/// Writes `1` or `0`: whether `ns` is a namespace (iris_test's pre-check).
pub fn namespace_exists_code(ns: &str) -> String {
    format!("write ##class(%SYS.Namespace).Exists({})", os_str_expr(ns))
}

/// Maps a routine and offset back to its class line (iris_debug).
pub fn source_line_code(routine: &str, offset: impl std::fmt::Display) -> String {
    format!(
        "Write ##class(%Studio.Debugger).SourceLine({},{})",
        os_str_expr(routine),
        offset
    )
}

/// Reads the registry entry for skill `name`.
pub fn skill_get_code(name: &str) -> String {
    format!("Write $Get(^SKILLS({}))", os_str_expr(name))
}

/// Deletes the registry entry for skill `name` and writes `OK`.
pub fn skill_forget_code(name: &str) -> String {
    format!("Kill ^SKILLS({}) Write \"OK\"", os_str_expr(name))
}

/// Exports `<class_name>.<ext>` (`ext` is `bpl` or `dtl`) to a stream and writes the XML.
pub fn export_class_xml_code(class_name: &str, ext: &str) -> String {
    let item = os_str_expr(&format!("{class_name}.{ext}"));
    format!(
        "Set stream = ##class(%Stream.GlobalCharacter).%New() \
         Do $system.OBJ.ExportToStream({item},stream,,\"c\") \
         Do stream.Rewind() \
         Write stream.Read(1000000)"
    )
}

// ── Names spliced into code ─────────────────────────────────────────────────
//
// A name lands in generated code bare (`^<global>`, `##class(<class>)`) or inside a literal. Free text
// can be escaped with `os_str_expr`; a name cannot, because it is not a value. So a name is checked
// against IRIS's own syntax for that kind of name and refused otherwise, before any call is made.
// `X) Kill ^Y Set q=$Get(^X` as a global name made a read-only `iris_global get` kill `^Y` (137).

use regex::Regex;
use std::sync::OnceLock;

fn refuse(code: &str, field: &str, value: &str, what: &str, syntax: &str) -> serde_json::Value {
    serde_json::json!({
        "success": false,
        "error_code": code,
        "message": format!("'{value}' is not {what}. {syntax}"),
        field: value,
    })
}

fn check(re: &'static OnceLock<Regex>, pattern: &str, value: &str) -> bool {
    re.get_or_init(|| Regex::new(pattern).expect("valid regex"))
        .is_match(value)
}

static GLOBAL_RE: OnceLock<Regex> = OnceLock::new();
static CLASS_RE: OnceLock<Regex> = OnceLock::new();
static ROUTINE_RE: OnceLock<Regex> = OnceLock::new();
static NAMESPACE_RE: OnceLock<Regex> = OnceLock::new();
static METHOD_RE: OnceLock<Regex> = OnceLock::new();
static SQL_ID_RE: OnceLock<Regex> = OnceLock::new();
static TIMESTAMP_RE: OnceLock<Regex> = OnceLock::new();

/// A global name without its `^`: an optional `%`, a letter, then letters and digits with single
/// periods between them. The namespace is a separate parameter, so extended references are refused.
pub fn validate_global_name(name: &str) -> Result<(), serde_json::Value> {
    if check(
        &GLOBAL_RE,
        r"^%?[A-Za-z][A-Za-z0-9]*(\.[A-Za-z0-9]+)*$",
        name,
    ) {
        return Ok(());
    }
    Err(refuse(
        "INVALID_GLOBAL_NAME",
        "global_name",
        name,
        "a global name",
        "A global name is an optional %, a letter, then letters and digits with single periods \
         between them (MyApp.Data, %SYS). Pass the namespace in 'namespace' and subscripts in \
         'subscripts', not in the name.",
    ))
}

/// A class or package name: an optional `%`, then identifiers (a letter, then letters and digits)
/// separated by single periods.
pub fn validate_class_name(name: &str) -> Result<(), serde_json::Value> {
    if check(
        &CLASS_RE,
        r"^%?[A-Za-z][A-Za-z0-9]*(\.[A-Za-z][A-Za-z0-9]*)*$",
        name,
    ) {
        return Ok(());
    }
    Err(refuse(
        "INVALID_CLASS_NAME",
        "class_name",
        name,
        "a class name",
        "A class name is an optional %, then identifiers separated by single periods \
         (MyApp.Patient, %Library.String).",
    ))
}

/// A routine or document name, with or without its extension. Generated routines have numeric
/// segments (`MyApp.Flow.1.int`), so a segment may start with a digit.
pub fn validate_routine_name(name: &str) -> Result<(), serde_json::Value> {
    if check(
        &ROUTINE_RE,
        r"^%?[A-Za-z][A-Za-z0-9]*(\.[A-Za-z0-9]+)*$",
        name,
    ) {
        return Ok(());
    }
    Err(refuse(
        "INVALID_ROUTINE_NAME",
        "routine",
        name,
        "a routine or document name",
        "A routine or document name is an optional %, a letter, then letters and digits with single \
         periods between them, optionally ending in its type (MyApp.Utils.mac, MyApp.Patient.cls).",
    ))
}

/// A namespace name: an optional `%`, a letter, then letters, digits, `_` and `-`.
pub fn validate_namespace(name: &str) -> Result<(), serde_json::Value> {
    if check(&NAMESPACE_RE, r"^%?[A-Za-z][A-Za-z0-9_-]*$", name) {
        return Ok(());
    }
    Err(refuse(
        "INVALID_NAMESPACE",
        "namespace",
        name,
        "a namespace name",
        "A namespace name is an optional %, a letter, then letters, digits, _ and - (USER, %SYS).",
    ))
}

/// A method or property name: an optional `%`, a letter, then letters and digits.
pub fn validate_method_name(name: &str) -> Result<(), serde_json::Value> {
    if check(&METHOD_RE, r"^%?[A-Za-z][A-Za-z0-9]*$", name) {
        return Ok(());
    }
    Err(refuse(
        "INVALID_METHOD_NAME",
        "method",
        name,
        "a method name",
        "A method name is an optional %, a letter, then letters and digits (OnRequest, %New).",
    ))
}

/// A SQL table reference: `Table` or `Schema.Table`, each part a plain identifier (an optional `%`,
/// a letter or `_`, then letters, digits and `_`) or a quoted one with no quote inside. More than two
/// parts pass too: `Pkg.Sub.Table` is a mistake IRIS answers with SQLCODE -30, and the
/// `deep_package_table` hint explains that answer, so the name has to reach IRIS (129).
pub fn validate_sql_identifier(name: &str) -> Result<(), serde_json::Value> {
    if check(
        &SQL_ID_RE,
        r#"^("[^"]+"|%?[A-Za-z_][A-Za-z0-9_]*)(\.("[^"]+"|%?[A-Za-z_][A-Za-z0-9_]*))*$"#,
        name,
    ) {
        return Ok(());
    }
    Err(refuse(
        "INVALID_SQL_IDENTIFIER",
        "table",
        name,
        "a SQL table name",
        "A SQL table name is Table or Schema.Table, each part an identifier or a \"quoted\" \
         identifier (Sample.Person, Ens_Rule.RuleSet).",
    ))
}

/// A date or date-time as IRIS's ODBC format writes it: `YYYY-MM-DD` or `YYYY-MM-DD HH:MM:SS[.f]`.
pub fn validate_timestamp(value: &str) -> Result<(), serde_json::Value> {
    if check(
        &TIMESTAMP_RE,
        r"^\d{4}-\d{2}-\d{2}( \d{2}:\d{2}:\d{2}(\.\d+)?)?$",
        value,
    ) {
        return Ok(());
    }
    Err(refuse(
        "INVALID_TIMESTAMP",
        "timestamp",
        value,
        "a timestamp",
        "A timestamp is YYYY-MM-DD or YYYY-MM-DD HH:MM:SS (2026-10-17 09:30:00).",
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_string_is_quoted() {
        assert_eq!(os_str_expr("GeneroSOAP"), "\"GeneroSOAP\"");
    }

    #[test]
    fn empty_string() {
        assert_eq!(os_str_expr(""), "\"\"");
    }

    #[test]
    fn quotes_are_doubled_not_backslashed() {
        assert_eq!(os_str_expr(r#"key="M""#), r#""key=""M""""#);
        assert!(!os_str_expr(r#"a"b"#).contains('\\'));
    }

    #[test]
    fn backslash_is_literal() {
        assert_eq!(os_str_expr(r"C:\tmp"), "\"C:\\tmp\"");
    }

    #[test]
    fn apostrophe_untouched() {
        assert_eq!(os_str_expr("it's"), "\"it's\"");
    }

    #[test]
    fn newlines_become_char_splices() {
        assert_eq!(os_str_expr("a\nb"), "\"a\"_$CHAR(10)_\"b\"");
        assert_eq!(os_str_expr("a\r\nb"), "\"a\"_$CHAR(13,10)_\"b\"");
        assert_eq!(os_str_expr("\n"), "$CHAR(10)");
    }

    #[test]
    fn unicode_passes_through() {
        assert_eq!(os_str_expr("señal"), "\"señal\"");
    }

    #[test]
    fn every_expr_line_has_balanced_quotes() {
        let samples = [
            "plain",
            r#"<entry table="G" key="M">1</entry>"#,
            "multi\nline\r\nwith\ttabs",
            "quote\"and'apostrophe",
        ];
        for s in samples {
            let expr = os_str_expr(s);
            assert!(!expr.contains('\n'), "expr must be single-line: {expr}");
            assert_eq!(
                expr.matches('"').count() % 2,
                0,
                "unbalanced quotes in {expr}"
            );
        }
    }

    #[test]
    fn write_stmts_chunk_and_cover_payload() {
        let payload = "x".repeat(1000);
        let stmts = os_stream_write_stmts("tStream", &payload, 400);
        assert_eq!(stmts.len(), 3);
        assert!(stmts.iter().all(|s| s.starts_with("Do tStream.Write(")));
        // chunking must never split on a non-char boundary
        let stmts = os_stream_write_stmts("tStream", &"ñ".repeat(401), 400);
        assert_eq!(stmts.len(), 2);
    }
}

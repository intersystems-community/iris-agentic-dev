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

//! Code-edit surface gate.
//!
//! Hard-blocks attempts to create/modify/delete class and routine *code* through the
//! arbitrary-execution tools (`iris_execute` ObjectScript, `iris_query` mode="write" SQL),
//! which otherwise bypass the `SYSTEM_BLOCKLIST` (that gate only fires on `iris_global`,
//! where a `global_name` param is present).
//!
//! Legitimate code editing must go through `iris_doc` (mode="put", SCM-gated) and
//! `iris_compile`; reading code goes through `iris_doc` (mode="get") or `iris_symbols`. This
//! gate is non-configurable and cannot be overridden — matching the treatment of
//! `^%Dictionary*` / `^oddDEF` / `^ROUTINE` in the system blocklist.
//!
//! It matches code-storage globals by name, so it stops reads of them as well as writes. That
//! is deliberate — the alternative is parsing ObjectScript well enough to tell a read from a
//! write through indirection, which this gate does not attempt.
//!
//! Editable surface (per InterSystems IRIS %Dictionary reference):
//! - `%Dictionary.*Definition` classes (ClassDefinition, MethodDefinition, PropertyDefinition,
//!   ParameterDefinition, IndexDefinition, ForeignKeyDefinition, ProjectionDefinition,
//!   QueryDefinition, TriggerDefinition, XDataDefinition, StorageDefinition, PackageDefinition,
//!   UDLTextDefinition, …). The read-only `%Dictionary.Compiled*` classes are NOT blocked.
//! - Code-management APIs: `$system.OBJ` / `%SYSTEM.OBJ` Load/Compile/Delete/Import,
//!   `%RoutineMgr`, `%Library.Routine`, `%Compiler.UDL.TextServices`.
//! - Direct writes to code-storage globals (`^oddDEF`, `^ROUTINE`, `^rMAC`, `^%Dictionary*`, …),
//!   detected by scanning global references against the shared `SYSTEM_BLOCKLIST`.

use crate::policy::patterns::{first_match, SYSTEM_BLOCKLIST};

const ERROR_CODE: &str = "CODE_EDIT_BLOCKED";

/// ObjectScript code-management API tokens. Matched case-insensitively as substrings
/// against a whitespace-free, uppercased copy of the code (ObjectScript is not
/// whitespace-sensitive within an expression, so `##class( %SYSTEM.OBJ )` normalizes to
/// `##CLASS(%SYSTEM.OBJ)`).
const OBJECTSCRIPT_API_TOKENS: &[&str] = &[
    // $system.OBJ / %SYSTEM.OBJ code load/compile/delete/import
    "$SYSTEM.OBJ.LOAD",
    "$SYSTEM.OBJ.COMPILE",
    "$SYSTEM.OBJ.DELETE",
    "$SYSTEM.OBJ.IMPORT",
    "$SYSTEM.OBJ.LOADSTREAM",
    "$SYSTEM.OBJ.MAKECLASSDEPLOYED",
    "%SYSTEM.OBJ.LOAD",
    "%SYSTEM.OBJ.COMPILE",
    "%SYSTEM.OBJ.DELETE",
    "%SYSTEM.OBJ.IMPORT",
    "%SYSTEM.OBJ.LOADSTREAM",
    "%SYSTEM.OBJ.MAKECLASSDEPLOYED",
    // Routine management
    "%ROUTINEMGR",
    "%LIBRARY.ROUTINE",
    // Class source (UDL) text services — SetTextFromString rewrites a class definition
    "%COMPILER.UDL.TEXTSERVICES",
];

/// SQL table/package tokens that identify a write against the code dictionary.
/// Matched case-insensitively as substrings against an uppercased copy of the SQL.
const SQL_CODE_TABLE_TOKENS: &[&str] = &["%DICTIONARY.", "%LIBRARY.ROUTINE"];

/// Gate: block ObjectScript that edits class/routine code.
///
/// Returns `Some(error_json)` when the code touches the editable-code surface, `None` otherwise.
pub fn check_objectscript_code_edit(code: &str, server_name: &str) -> Option<serde_json::Value> {
    // Normalize: drop ASCII whitespace, uppercase. This defeats spacing tricks like
    // `%Dictionary . ClassDefinition` and is safe because the tokens we match never
    // contain meaningful whitespace.
    let normalized: String = code
        .chars()
        .filter(|c| !c.is_whitespace())
        .flat_map(|c| c.to_uppercase())
        .collect();
    let flattened = flatten_punctuation(&normalized);

    // (a) Editable %Dictionary.*Definition classes (read-only Compiled* stays allowed).
    if let Some(matched) = first_dictionary_definition(&normalized) {
        return Some(error(code, &matched, server_name));
    }
    if let Some(matched) = first_dictionary_definition(&flattened) {
        return Some(error(code, &matched, server_name));
    }

    // (b) Code-management API tokens, matched against both the plain normalization and the
    //     punctuation-flattened one so the `##class(...)` and `$classmethod(...)` call forms
    //     are covered by the same token list.
    for token in OBJECTSCRIPT_API_TOKENS {
        if normalized.contains(token) || flattened.contains(token) {
            return Some(error(code, token, server_name));
        }
    }

    // (c) Direct writes to code-storage globals — scan `^global` references against the
    //     shared system blocklist (catches ^oddDEF, ^ROUTINE, ^rMAC, ^%Dictionary*, …).
    for gname in extract_globals(code) {
        if let Some(pattern) = first_match(&gname, SYSTEM_BLOCKLIST) {
            return Some(error(code, pattern, server_name));
        }
    }

    None
}

/// Gate: block UDL class source that uses compile-time code execution keywords.
///
/// `CodeMode = objectgenerator` / `expression` / `call` cause IRIS to run arbitrary code
/// at compile time, bypassing runtime privilege restrictions (e.g. a read-only service
/// account). This gate fires on the **assembled document content** (not individual edits),
/// so multi-call assembly tricks (split across insert_lines calls) are irrelevant — the
/// full content is always scanned before it reaches IRIS.
///
/// Only `.cls` documents are scanned; routines (`.mac`/`.inc`) don't support CodeMode.
///
/// Returns `Some(error_json)` when blocked, `None` when safe.
pub fn check_compile_time_code_mode(content: &str, doc_name: &str) -> Option<serde_json::Value> {
    // Only applies to class definitions.
    if !doc_name.to_lowercase().ends_with(".cls") {
        return None;
    }

    // Normalize: strip spaces/tabs within square-bracket annotations so that
    // `[ CodeMode = objectgenerator ]` and `[CodeMode=objectgenerator]` both match.
    // We don't strip ALL whitespace (unlike the execute gate) because class source
    // is line-oriented — we just need to normalize within `[...]` annotation blocks.
    //
    // Strategy: scan for `CODEMODE` followed (ignoring whitespace and `=`) by one of
    // the dangerous values. This catches all UDL forms:
    //   Method Foo() [ CodeMode = objectgenerator ]
    //   Method Foo() [CodeMode=objectgenerator]
    //   Method Foo() [ CodeMode = objectgenerator, ...]
    // The keyword MUST appear literally in UDL — there's no way to construct it
    // dynamically because UDL is declarative text, not executable code.

    let upper: String = content.to_uppercase();

    // Find all occurrences of CODEMODE in the uppercased content.
    let mut search = 0;
    while let Some(pos) = upper[search..].find("CODEMODE") {
        let after_keyword = search + pos + "CODEMODE".len();
        // Skip whitespace and `=` after CODEMODE
        let rest = &upper[after_keyword..];
        let trimmed = rest.trim_start();
        let trimmed = if let Some(after_eq) = trimmed.strip_prefix('=') {
            after_eq.trim_start()
        } else {
            search = after_keyword;
            continue;
        };

        // Check if the value is one of the dangerous modes.
        const DANGEROUS_MODES: &[&str] = &["OBJECTGENERATOR", "EXPRESSION", "CALL"];
        for mode in DANGEROUS_MODES {
            if trimmed.starts_with(mode) {
                // Verify it's a whole token (followed by non-alphanumeric or EOF)
                let after_mode = trimmed.strip_prefix(*mode).unwrap_or("");
                if after_mode.is_empty()
                    || !after_mode.starts_with(|c: char| c.is_ascii_alphanumeric() || c == '_')
                {
                    return Some(serde_json::json!({
                        "success": false,
                        "error_code": "COMPILE_TIME_EXEC_BLOCKED",
                        "code_edit_blocked": true,
                        "document": doc_name,
                        "matched": format!("CodeMode = {}", mode.to_lowercase()),
                        "message": format!(
                            "Document '{}' contains a compile-time code execution keyword \
                             (CodeMode = {}). This allows arbitrary code to run during \
                             compilation, bypassing runtime privilege restrictions. \
                             Only CodeMode = code (the default) is permitted.",
                            doc_name, mode.to_lowercase()
                        ),
                        "remediation": "Remove the CodeMode keyword or use CodeMode = code \
                                        (which is the default and can simply be omitted). \
                                        If you need generator logic, implement it as a \
                                        regular ClassMethod that is called explicitly.",
                    }));
                }
            }
        }

        search = after_keyword;
    }

    None
}

/// Gate: block write-mode SQL that edits the code dictionary.
///
/// Only meaningful for `iris_query` mode="write" (DML); read/SELECT introspection against
/// `%Dictionary.Compiled*` is unaffected. Returns `Some(error_json)` when blocked.
pub fn check_sql_code_edit(sql: &str, server_name: &str) -> Option<serde_json::Value> {
    // Whitespace-free, like the ObjectScript side. IRIS SQL accepts `%Dictionary . ClassDefinition`
    // and `"%Dictionary"."ClassDefinition"` for the same table, and matching the raw text meant
    // either spelling reached the dictionary with the gate reporting nothing.
    let normalized: String = sql
        .chars()
        .filter(|c| !c.is_whitespace())
        .flat_map(|c| c.to_uppercase())
        .collect();
    let flattened = flatten_punctuation(&normalized);
    for token in SQL_CODE_TABLE_TOKENS {
        if normalized.contains(token) || flattened.contains(token) {
            return Some(error(sql, token, server_name));
        }
    }
    None
}

/// Collapse call punctuation to dots so that every way of naming a class method reduces to the
/// dotted form the token list is written in.
///
/// The token list says `%SYSTEM.OBJ.COMPILE`, which matches `do $system.OBJ.Compile("X")` and
/// nothing else. Two other spellings reach the same method and both used to walk straight past
/// this gate:
///
/// - `##class(%SYSTEM.OBJ).Compile("X")` — the parenthesis sits where the dot is expected.
/// - `$classmethod("%SYSTEM.OBJ","Compile","X")` — quotes and a comma sit there instead.
///
/// Replacing `"`, `'`, `(`, `)`, and `,` with `.` and collapsing runs of dots turns all three
/// into `...%SYSTEM.OBJ.COMPILE...`, so one token list covers every call form, including ones
/// nobody has thought of yet. The cost is that a string literal containing a class and method
/// name side by side is a false block. That trade is right: a false block prints a legible
/// error, and a false permit is a class edit that no audit trail records.
fn flatten_punctuation(normalized: &str) -> String {
    let mut out = String::with_capacity(normalized.len());
    for c in normalized.chars() {
        let mapped = match c {
            '"' | '\'' | '(' | ')' | ',' => '.',
            other => other,
        };
        if mapped == '.' && out.ends_with('.') {
            continue;
        }
        out.push(mapped);
    }
    out
}

/// Find a `%DICTIONARY.<Name>` reference in `normalized` (whitespace-free, uppercased)
/// whose class name ends in `DEFINITION`. Returns the matched class token, e.g.
/// `%DICTIONARY.CLASSDEFINITION`.
fn first_dictionary_definition(normalized: &str) -> Option<String> {
    const PREFIX: &str = "%DICTIONARY.";
    let mut search = 0;
    while let Some(pos) = normalized[search..].find(PREFIX) {
        let start = search + pos;
        let name_start = start + PREFIX.len();
        // Read the class-name identifier (letters/digits — no '.'; the class name is a single segment).
        let name_end = normalized[name_start..]
            .find(|c: char| !(c.is_ascii_alphanumeric()))
            .map(|off| name_start + off)
            .unwrap_or(normalized.len());
        let class_name = &normalized[name_start..name_end];
        if class_name.ends_with("DEFINITION") {
            return Some(format!("{PREFIX}{class_name}"));
        }
        search = name_start;
    }
    None
}

/// ObjectScript commands whose argument is a routine reference, not a global reference.
/// Includes the single-letter abbreviations IRIS accepts.
const ROUTINE_CALL_COMMANDS: &[&str] = &["DO", "D", "GOTO", "G", "JOB", "J"];

/// Extract global references (`^name`, `^%name`, `^Pkg.Sub`) from ObjectScript source.
/// Returns names without the leading caret. Handles the `^["ns"]global` and `^|"ns"|global`
/// extended reference forms by skipping the namespace qualifier.
///
/// Routine references are skipped, because they share the caret syntax but name routines,
/// not globals — `$$run^SystemPerformance("test")` calls a routine, it does not touch a
/// global called `SystemPerformance`. Two forms:
/// - `label^routine` / `$$label^routine` — caret directly follows an identifier character.
/// - `Do ^routine` / `Goto ^routine` / `Job ^routine` — caret is the argument of a command
///   that only ever takes a routine.
fn extract_globals(code: &str) -> Vec<String> {
    let chars: Vec<char> = code.chars().collect();
    let n = chars.len();
    let mut out = Vec::new();
    let mut i = 0;
    while i < n {
        if chars[i] != '^' {
            i += 1;
            continue;
        }
        if is_routine_reference(&chars, i) {
            i += 1;
            continue;
        }
        let mut j = i + 1;
        // Skip an extended global reference qualifier: ^|...| or ^[...]
        if j < n && (chars[j] == '|' || chars[j] == '[') {
            let close = if chars[j] == '|' { '|' } else { ']' };
            j += 1;
            while j < n && chars[j] != close {
                j += 1;
            }
            j += 1; // skip closing delimiter
        }
        // Read the global name: leading % allowed, then alphanumerics and dots.
        let name_start = j;
        if j < n && chars[j] == '%' {
            j += 1;
        }
        while j < n && (chars[j].is_ascii_alphanumeric() || chars[j] == '.') {
            j += 1;
        }
        if j > name_start {
            out.push(chars[name_start..j].iter().collect());
        }
        i = j.max(i + 1);
    }
    out
}

/// Is the caret at `pos` naming a routine rather than a global?
fn is_routine_reference(chars: &[char], pos: usize) -> bool {
    // `label^routine` — the caret directly follows the label.
    let mut k = pos;
    if k > 0 {
        let prev = chars[k - 1];
        if prev.is_ascii_alphanumeric() || prev == '%' || prev == '.' {
            return true;
        }
    }

    // `Do ^routine` — walk back over whitespace, then read the preceding word.
    while k > 0 && chars[k - 1].is_whitespace() {
        k -= 1;
    }
    if k == pos || k == 0 {
        // No whitespace before the caret (so no command word), or start of input.
        return false;
    }
    let word_end = k;
    while k > 0 && (chars[k - 1].is_ascii_alphabetic()) {
        k -= 1;
    }
    if k == word_end {
        return false;
    }
    // The word must itself start a command — preceded by start of input, whitespace, or a
    // command separator. This rejects e.g. `xdo ^g` and `$$d ^g`.
    if k > 0 {
        let before = chars[k - 1];
        if !(before.is_whitespace() || before == '.' || before == ':') {
            return false;
        }
    }
    let word: String = chars[k..word_end]
        .iter()
        .flat_map(|c| c.to_uppercase())
        .collect();
    ROUTINE_CALL_COMMANDS.contains(&word.as_str())
}

fn error(_source: &str, matched: &str, server_name: &str) -> serde_json::Value {
    serde_json::json!({
        "success": false,
        "error_code": ERROR_CODE,
        "code_edit_blocked": true,
        "server_name": server_name,
        "matched": matched,
        "message": format!(
            "Reaching class or routine code through arbitrary execution is blocked (matched '{}') \
             for server '{}'. This covers %Dictionary.*Definition classes, $system.OBJ \
             Load/Compile/Delete, %RoutineMgr, and any reference to a code-storage global — \
             reads as well as writes, because the gate matches the global name and cannot tell \
             the two apart. Use the dedicated tools instead: they are auditable, SCM-gated, and \
             not blocked. The gate itself is non-configurable.",
            matched, server_name
        ),
        "remediation": "Read source with iris_doc (mode=\"get\") or iris_symbols; write it with \
                        iris_doc (mode=\"put\", which handles SCM checkout) and compile with \
                        iris_compile.",
    })
}

/// Test-only view of `extract_globals`. The tests that pin it live in `tests/unit/` rather than
/// an inline module, because assertion-message lines in a `#[cfg(test)] mod tests` never execute
/// on a passing run and so cap this file's measured coverage.
#[cfg(feature = "testing")]
pub fn extract_globals_for_tests(code: &str) -> Vec<String> {
    extract_globals(code)
}

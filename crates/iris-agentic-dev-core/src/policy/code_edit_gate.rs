//! Code-edit surface gate.
//!
//! Hard-blocks attempts to create/modify/delete class and routine *code* through the
//! arbitrary-execution tools (`iris_execute` ObjectScript, `iris_query` mode="write" SQL),
//! which otherwise bypass the `SYSTEM_BLOCKLIST` (that gate only fires on `iris_global`,
//! where a `global_name` param is present).
//!
//! Legitimate code editing must go through `iris_document` (mode="put", SCM-gated) and
//! `iris_compile`. This gate is non-configurable and cannot be overridden — matching the
//! treatment of `^%Dictionary*` / `^oddDEF` / `^ROUTINE` in the system blocklist.
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

    // (a) Editable %Dictionary.*Definition classes (read-only Compiled* stays allowed).
    if let Some(matched) = first_dictionary_definition(&normalized) {
        return Some(error(code, &matched, server_name));
    }

    // (b) Code-management API tokens.
    for token in OBJECTSCRIPT_API_TOKENS {
        if normalized.contains(token) {
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
pub fn check_compile_time_code_mode(
    content: &str,
    doc_name: &str,
) -> Option<serde_json::Value> {
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
        let trimmed = if trimmed.starts_with('=') {
            trimmed[1..].trim_start()
        } else {
            search = after_keyword;
            continue;
        };

        // Check if the value is one of the dangerous modes.
        const DANGEROUS_MODES: &[&str] = &["OBJECTGENERATOR", "EXPRESSION", "CALL"];
        for mode in DANGEROUS_MODES {
            if trimmed.starts_with(mode) {
                // Verify it's a whole token (followed by non-alphanumeric or EOF)
                let after_mode = &trimmed[mode.len()..];
                if after_mode.is_empty()
                    || !after_mode.starts_with(|c: char| c.is_ascii_alphanumeric() || c == '_')
                {
                    return Some(serde_json::json!({
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
    let upper = sql.to_uppercase();
    for token in SQL_CODE_TABLE_TOKENS {
        if upper.contains(token) {
            return Some(error(sql, token, server_name));
        }
    }
    None
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

/// Extract global references (`^name`, `^%name`, `^Pkg.Sub`) from ObjectScript source.
/// Returns names without the leading caret. Handles the `^["ns"]global` and `^|"ns"|global`
/// extended reference forms by skipping the namespace qualifier.
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

fn error(_source: &str, matched: &str, server_name: &str) -> serde_json::Value {
    serde_json::json!({
        "error_code": ERROR_CODE,
        "code_edit_blocked": true,
        "server_name": server_name,
        "matched": matched,
        "message": format!(
            "Editing class or routine code through arbitrary execution is blocked (matched '{}') \
             for server '{}'. This includes %Dictionary.*Definition classes, $system.OBJ \
             Load/Compile/Delete, %RoutineMgr, and direct writes to code-storage globals. \
             This protection is non-configurable.",
            matched, server_name
        ),
        "remediation": "Edit source with iris_document (mode=\"put\", which handles SCM checkout) \
                        and compile with iris_compile. These paths are auditable and SCM-gated.",
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── ObjectScript: %Dictionary.*Definition ────────────────────────────────
    #[test]
    fn blocks_class_definition_save() {
        let code = r#"set c=##class(%Dictionary.ClassDefinition).%OpenId("My.Class") do c.%Save()"#;
        assert!(check_objectscript_code_edit(code, "srv").is_some());
    }

    #[test]
    fn blocks_method_definition() {
        let code = "set m=##class(%Dictionary.MethodDefinition).%New()";
        assert!(check_objectscript_code_edit(code, "srv").is_some());
    }

    #[test]
    fn blocks_dictionary_definition_with_spacing() {
        let code = "##class( %Dictionary . ClassDefinition ).%DeleteId(\"X\")";
        assert!(check_objectscript_code_edit(code, "srv").is_some());
    }

    #[test]
    fn allows_compiled_class_introspection() {
        // Read-only Compiled* classes are NOT blocked.
        let code = r#"set r=##class(%Dictionary.CompiledClass).%OpenId("My.Class") write r.Name"#;
        assert!(check_objectscript_code_edit(code, "srv").is_none());
    }

    // ── ObjectScript: code-management APIs ────────────────────────────────────
    #[test]
    fn blocks_system_obj_compile() {
        assert!(
            check_objectscript_code_edit("do $system.OBJ.Compile(\"My.Class\")", "srv").is_some()
        );
    }

    #[test]
    fn blocks_system_obj_load() {
        assert!(
            check_objectscript_code_edit("do $System.OBJ.Load(\"/tmp/x.xml\",\"ck\")", "srv")
                .is_some()
        );
    }

    #[test]
    fn blocks_system_obj_delete() {
        assert!(
            check_objectscript_code_edit("do $SYSTEM.OBJ.Delete(\"My.Class\")", "srv").is_some()
        );
    }

    #[test]
    fn blocks_routine_mgr() {
        assert!(check_objectscript_code_edit("set r=##class(%RoutineMgr).%New()", "srv").is_some());
    }

    #[test]
    fn blocks_udl_text_services() {
        let code = "do ##class(%Compiler.UDL.TextServices).SetTextFromString(,,\"My.Class\",text)";
        assert!(check_objectscript_code_edit(code, "srv").is_some());
    }

    // ── ObjectScript: direct code-global writes ───────────────────────────────
    #[test]
    fn blocks_odddef_global_write() {
        assert!(check_objectscript_code_edit("set ^oddDEF(\"My.Class\")=1", "srv").is_some());
    }

    #[test]
    fn blocks_routine_global_write() {
        assert!(check_objectscript_code_edit("set ^ROUTINE(\"x\")=\"\"", "srv").is_some());
    }

    #[test]
    fn blocks_dictionary_global_write() {
        assert!(check_objectscript_code_edit("kill ^%Dictionary", "srv").is_some());
    }

    #[test]
    fn allows_ordinary_global_and_code() {
        assert!(check_objectscript_code_edit("write $ZVERSION,!", "srv").is_none());
        assert!(check_objectscript_code_edit(
            "set ^MyApp.Data(1)=\"ok\" write ^MyApp.Data(1)",
            "srv"
        )
        .is_none());
    }

    // ── SQL write gate ────────────────────────────────────────────────────────
    #[test]
    fn blocks_sql_update_dictionary() {
        let sql = "UPDATE %Dictionary.MethodDefinition SET Name='x' WHERE parent='My.Class'";
        assert!(check_sql_code_edit(sql, "srv").is_some());
    }

    #[test]
    fn blocks_sql_delete_dictionary() {
        assert!(check_sql_code_edit(
            "DELETE FROM %Dictionary.ClassDefinition WHERE ID='X'",
            "srv"
        )
        .is_some());
    }

    #[test]
    fn allows_sql_write_to_app_table() {
        assert!(
            check_sql_code_edit("UPDATE MyApp.Patient SET Name='x' WHERE ID=1", "srv").is_none()
        );
    }

    // ── extract_globals ───────────────────────────────────────────────────────
    #[test]
    fn extract_globals_basic() {
        let g = extract_globals("set ^foo(1)=2 set x=^bar");
        assert_eq!(g, vec!["foo".to_string(), "bar".to_string()]);
    }

    #[test]
    fn extract_globals_percent_and_dotted() {
        let g = extract_globals("write ^%Dictionary.x, ^Ens.Config");
        assert!(g.contains(&"%Dictionary.x".to_string()));
        assert!(g.contains(&"Ens.Config".to_string()));
    }

    #[test]
    fn extract_globals_extended_reference() {
        let g = extract_globals(r#"set ^["USER"]oddDEF(1)=2"#);
        assert!(g.contains(&"oddDEF".to_string()));
    }

    #[test]
    fn error_shape_has_code_and_remediation() {
        let e = check_objectscript_code_edit("do $system.OBJ.Compile(\"X\")", "srv").unwrap();
        assert_eq!(e["error_code"], "CODE_EDIT_BLOCKED");
        assert_eq!(e["code_edit_blocked"], true);
        assert!(e["remediation"].as_str().unwrap().contains("iris_document"));
    }

    // ── Compile-time code mode gate ──────────────────────────────────────────

    #[test]
    fn blocks_objectgenerator() {
        let cls = r#"Class My.Evil {
Method Hack() [ CodeMode = objectgenerator ]
{
  do ##class(%Dictionary.MethodDefinition).stuff()
}
}"#;
        let r = check_compile_time_code_mode(cls, "My.Evil.cls");
        assert!(r.is_some());
        assert_eq!(r.unwrap()["error_code"], "COMPILE_TIME_EXEC_BLOCKED");
    }

    #[test]
    fn blocks_expression_mode() {
        let cls = "Class My.Trick {\nMethod X() As %String [ CodeMode = expression ]\n{\n1+1\n}\n}";
        assert!(check_compile_time_code_mode(cls, "My.Trick.cls").is_some());
    }

    #[test]
    fn blocks_call_mode() {
        let cls = "Class My.Trick {\nMethod X() [ CodeMode = call ]\n{\nSomeRoutine\n}\n}";
        assert!(check_compile_time_code_mode(cls, "My.Trick.cls").is_some());
    }

    #[test]
    fn blocks_case_insensitive() {
        let cls = "Class My.X {\nMethod G() [ codemode = OBJECTGENERATOR ]\n{\n}\n}";
        assert!(check_compile_time_code_mode(cls, "My.X.cls").is_some());
    }

    #[test]
    fn blocks_no_spaces_around_equals() {
        let cls = "Class My.X {\nMethod G() [CodeMode=objectgenerator]\n{\n}\n}";
        assert!(check_compile_time_code_mode(cls, "My.X.cls").is_some());
    }

    #[test]
    fn blocks_extra_spaces() {
        let cls = "Class My.X {\nMethod G() [ CodeMode   =   objectgenerator , Final ]\n{\n}\n}";
        assert!(check_compile_time_code_mode(cls, "My.X.cls").is_some());
    }

    #[test]
    fn allows_normal_class() {
        let cls = "Class My.Normal {\nMethod Hello() As %String\n{\n  Quit \"hi\"\n}\n}";
        assert!(check_compile_time_code_mode(cls, "My.Normal.cls").is_none());
    }

    #[test]
    fn allows_codemode_code_explicit() {
        let cls = "Class My.X {\nMethod G() [ CodeMode = code ]\n{\n  Write 1\n}\n}";
        assert!(check_compile_time_code_mode(cls, "My.X.cls").is_none());
    }

    #[test]
    fn skips_non_cls_documents() {
        let content = "CodeMode = objectgenerator";
        assert!(check_compile_time_code_mode(content, "My.Routine.mac").is_none());
        assert!(check_compile_time_code_mode(content, "include.inc").is_none());
    }

    #[test]
    fn does_not_false_positive_on_codemode_in_comment() {
        // A comment mentioning "CodeMode" without `= objectgenerator` following it
        let cls = "Class My.X {\n/// This uses CodeMode but is safe\nMethod G()\n{\n}\n}";
        assert!(check_compile_time_code_mode(cls, "My.X.cls").is_none());
    }

    #[test]
    fn does_not_match_partial_word() {
        // "OBJECTGENERATORS" (with trailing S) should not match
        let cls = "Class My.X {\n/// CodeMode = objectgenerators is not a thing\nMethod G()\n{\n}\n}";
        assert!(check_compile_time_code_mode(cls, "My.X.cls").is_none());
    }

    #[test]
    fn blocks_assembled_from_inserts() {
        // Simulates what the agent might achieve across multiple insert_lines calls:
        // the final assembled content has the full keyword.
        let cls = "Class My.Sneaky {\nMethod Pwn() [ CodeMode = objectgenerator ]\n{\n  do badstuff\n}\n}";
        assert!(check_compile_time_code_mode(cls, "My.Sneaky.cls").is_some());
    }

    #[test]
    fn blocks_keyword_split_across_lines() {
        // Agent inserts "CodeMode =" on one line and "objectgenerator" on the next.
        // trim_start() handles the newline.
        let cls = "Class My.X {\nMethod G() [ CodeMode =\nobjectgenerator ]\n{\n}\n}";
        assert!(check_compile_time_code_mode(cls, "My.X.cls").is_some());
    }

    #[test]
    fn blocks_keyword_with_tabs_and_newlines() {
        let cls = "Class My.X {\nMethod G() [ CodeMode\t=\t\n\tobjectgenerator ]\n{\n}\n}";
        assert!(check_compile_time_code_mode(cls, "My.X.cls").is_some());
    }
}

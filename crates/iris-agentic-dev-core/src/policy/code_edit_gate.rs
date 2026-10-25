//! Code-edit surface gate: a best-effort lexical check, not a security boundary.
//!
//! Refuses text that looks like an attempt to create, change or delete class and routine *code*
//! through the arbitrary-execution tools (`iris_execute` ObjectScript, `iris_ws_exec`,
//! `iris_execute_method`, `iris_query` mode="write" SQL). Those tools sit outside the `iris_doc`
//! write path, which is where code edits belong: `iris_doc` (mode="put", SCM-gated) and
//! `iris_compile` are auditable, and reading code goes through `iris_doc` (mode="get") or
//! `iris_symbols`.
//!
//! The check is lexical. It reads the text of the call and matches names. Anything that builds the
//! name at run time gets past it: `$classmethod` with a class name concatenated from pieces,
//! `Xecute` of a built string, `@` name indirection, `$char` or `$reverse` encodings, and a name
//! split across two `iris_ws_exec` calls (the terminal session keeps its variables between them).
//! The boundary that holds is the service account: an IRIS user without `%Development` and without
//! write access to the routine database cannot edit code however the call is spelled. Set
//! `IRIS_SERVICE_USERNAME` so the arbitrary-execution tools run as that user.
//!
//! It matches code-storage globals by name, so it stops reads of them as well as writes. That is
//! deliberate: telling a read from a write through indirection means parsing ObjectScript, which
//! this gate does not attempt. Comments are blanked before scanning, because ObjectScript ignores
//! them. String literals are scanned, because `$ClassMethod("%SYSTEM.OBJ", ...)` and `@"^oddDEF"`
//! are only visible through their literals.
//!
//! What it matches:
//! - `%Dictionary.*Definition` classes. The read-only `%Dictionary.Compiled*` classes are not
//!   matched.
//! - Code-management APIs: `$system.OBJ` / `%SYSTEM.OBJ` Load, Compile, UnCompile, Delete, Import,
//!   `%RoutineMgr`, `%Routine`, `%Library.Routine`, `%Compiler.UDL.TextServices`, and the routine
//!   behind `$system.OBJ` (`^%apiOBJ`).
//! - The routine-buffer commands `ZLOAD`, `ZINSERT`, `ZREMOVE` and `ZSAVE` (and their short forms)
//!   at command position.
//! - Globals that hold code: `^oddDEF` and the other dictionary globals with the `odd` prefix,
//!   `^ROUTINE`, `^rOBJ`, `^rMAC`, `^rINC`, `^rINDEX` and their relatives, `^%Dictionary*`. This
//!   list is the gate's own. The shared `SYSTEM_BLOCKLIST` is a data policy for `iris_global`; it
//!   also names interop configuration, and reading that is not a code edit.

use crate::policy::patterns::first_match;
use regex::Regex;
use std::sync::OnceLock;

const ERROR_CODE: &str = "CODE_EDIT_BLOCKED";

/// ObjectScript code-management API tokens. Matched case-insensitively as substrings
/// against a whitespace-free, uppercased copy of the code (ObjectScript is not
/// whitespace-sensitive within an expression, so `##class( %SYSTEM.OBJ )` normalizes to
/// `##CLASS(%SYSTEM.OBJ)`).
const OBJECTSCRIPT_API_TOKENS: &[&str] = &[
    // $system.OBJ / %SYSTEM.OBJ code load/compile/delete/import
    "$SYSTEM.OBJ.LOAD",
    "$SYSTEM.OBJ.COMPILE",
    "$SYSTEM.OBJ.UNCOMPILE",
    "$SYSTEM.OBJ.DELETE",
    "$SYSTEM.OBJ.IMPORT",
    "$SYSTEM.OBJ.LOADSTREAM",
    "$SYSTEM.OBJ.MAKECLASSDEPLOYED",
    "%SYSTEM.OBJ.LOAD",
    "%SYSTEM.OBJ.COMPILE",
    "%SYSTEM.OBJ.UNCOMPILE",
    "%SYSTEM.OBJ.DELETE",
    "%SYSTEM.OBJ.IMPORT",
    "%SYSTEM.OBJ.LOADSTREAM",
    "%SYSTEM.OBJ.MAKECLASSDEPLOYED",
    // The routine that implements $system.OBJ. `Load^%apiOBJ`, `Compile^%apiOBJ` and
    // `Delete^%apiOBJ` exist and run on 2026.2, and `label^routine` is skipped by `extract_globals`.
    "^%APIOBJ",
    // Routine management. `%ROUTINE` also covers `%RoutineMgr`; `%Routine` is the short name of
    // `%Library.Routine`, and `%New("X.mac")`, `Write`, `Save` and `Compile` on it make a routine.
    "%ROUTINE",
    "%LIBRARY.ROUTINE",
    // Class source (UDL) text services — SetTextFromString rewrites a class definition
    "%COMPILER.UDL.TEXTSERVICES",
];

/// Globals that hold class and routine code. Matched with the same pattern rules as
/// `SYSTEM_BLOCKLIST` (a trailing `*` is a prefix, anything else is the whole name), but this list
/// is the code-edit gate's own, so it names code stores and system configuration, not interop data.
///
/// `^Ens.Config*`, `^Ens.Rule*`, `^Ens.MessageHeader*`, `^DeepSee*`, `^IRIS.Msg*` and
/// `^IRIS.Temp*` stay on `SYSTEM_BLOCKLIST` for `iris_global` and the data policy, and are not
/// here: reading them is not a code edit (EXEC-22).
///
/// Exact names for the `^odd*` and `^r*` families, not prefixes: a prefix hides application
/// globals that start the same way.
const CODE_STORAGE_GLOBALS: &[&str] = &[
    "^%Dictionary*",
    "^oddDEF",
    "^oddEXT",
    "^oddSQL",
    "^oddMAC",
    "^oddPKG",
    "^oddCOM",
    "^oddDEP",
    "^oddEXTR",
    "^oddMAP",
    "^oddMETA",
    "^oddPROC",
    "^oddPROJECT",
    "^oddPYTHON",
    "^oddStudioDocument",
    "^oddStudioMenu",
    "^oddTSQL",
    "^oddXML",
    "^ROUTINE",
    "^rOBJ",
    "^rMAP",
    "^rINDEX",
    "^rINCLUDE",
    "^rBACKUP",
    "^rMAC",
    "^rMACSAVE",
    "^rINC",
    "^rINCSAVE",
    "^rINT",
    "^rBAS",
    "^rINDEXCLASS",
    "^rINDEXEXT",
    "^rINDEXSQL",
    // System configuration and security, kept from the 1.6.3 list.
    "^%SYS*",
    "^%Library*",
    "^%SYSTEM*",
    "^ROLE",
    "^USER",
    "^SYS",
    "^SYS.*",
    "^SYSTEM",
    "^SYSTEM.*",
    "^IRIS.Sys.*",
    "^IRIS.SysLog*",
];

/// SQL table/package tokens that identify a write against the code dictionary.
/// Matched case-insensitively as substrings against an uppercased copy of the SQL.
const SQL_CODE_TABLE_TOKENS: &[&str] = &["%DICTIONARY.", "%LIBRARY.ROUTINE"];

/// Routine-buffer commands and their short forms. They write `^ROUTINE` and `^rOBJ` as a side
/// effect of the command, so `extract_globals` never sees a caret.
const ROUTINE_BUFFER_COMMANDS: &[&str] = &[
    "ZL", "ZLOAD", "ZI", "ZINSERT", "ZR", "ZREMOVE", "ZS", "ZSAVE",
];

/// Gate: block ObjectScript that edits class/routine code.
///
/// Returns `Some(error_json)` when the code touches the editable-code surface, `None` otherwise.
pub fn check_objectscript_code_edit(code: &str, server_name: &str) -> Option<serde_json::Value> {
    // Comments are documentation: blank them, keep string literals (see the module doc).
    let code = blank(code, Blank::COMMENTS_ONLY);
    let code = code.as_str();

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

    // (c) Routine-buffer commands, read from the text with its spaces: `$ZSTRIP` and a variable
    //     called `zsvar` must not match.
    if let Some(command) = first_routine_buffer_command(code) {
        return Some(error(code, &command, server_name));
    }

    // (d) Direct references to code-storage globals (^oddDEF, ^ROUTINE, ^rMAC, ^%Dictionary*, …).
    for gname in extract_globals(code) {
        if let Some(pattern) = first_match(&gname, CODE_STORAGE_GLOBALS) {
            return Some(error(code, pattern, server_name));
        }
    }

    None
}

/// The first routine-buffer command (`ZSAVE`, `ZINSERT`, …) in `code`, uppercased.
///
/// A command sits at the start of the text, after whitespace, after `{` or `}`, or at the start of
/// a string literal (the argument of `Xecute`), and ends at whitespace, a `:` (postconditional), a
/// quote or the end of the text. `$ZSTRIP`, `..zr()`, `obj.ZSave()` and `zsvar` fail one of those.
/// A variable named `zs` in `Write zs` is a false block; the error says the check is lexical.
fn first_routine_buffer_command(code: &str) -> Option<String> {
    let bytes = code.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if !bytes[i].is_ascii_alphanumeric() {
            i += 1;
            continue;
        }
        let start = i;
        while i < bytes.len() && bytes[i].is_ascii_alphanumeric() {
            i += 1;
        }
        let before_ok = start == 0
            || matches!(
                bytes[start - 1],
                b' ' | b'\t' | b'\r' | b'\n' | b'{' | b'}' | b'"'
            );
        let after_ok = i == bytes.len()
            || matches!(
                bytes[i],
                b' ' | b'\t' | b'\r' | b'\n' | b':' | b'"' | b'{' | b'}'
            );
        if before_ok && after_ok {
            let word = code[start..i].to_ascii_uppercase();
            if ROUTINE_BUFFER_COMMANDS.contains(&word.as_str()) {
                return Some(word);
            }
        }
    }
    None
}

/// How `blank` treats text IRIS ignores.
#[derive(Clone, Copy)]
struct Blank {
    /// Blank string literals too. The compile-time gate wants this (a keyword in a string is
    /// data); the code-edit block does not (the literal is where the class name is).
    strings: bool,
    /// The text is a UDL class: `;` ends a declaration at class level instead of starting a
    /// comment, and a member declaration at the start of a line ends any open comment.
    udl: bool,
}

impl Blank {
    const COMMENTS_AND_STRINGS_UDL: Blank = Blank {
        strings: true,
        udl: true,
    };
    const COMMENTS_AND_STRINGS: Blank = Blank {
        strings: true,
        udl: false,
    };
    const COMMENTS_ONLY: Blank = Blank {
        strings: false,
        udl: false,
    };
}

/// Words that start a class member declaration. A line that begins with one of these followed by
/// space is a UDL member header, which cannot occur inside a method body.
const MEMBER_KEYWORDS: &[&str] = &[
    "method",
    "classmethod",
    "clientmethod",
    "trigger",
    "query",
    "projection",
    "parameter",
    "xdata",
    "index",
    "property",
    "relationship",
    "foreignkey",
    "storage",
];

/// Does a UDL member declaration start the line beginning at `at`?
fn member_header_at(bytes: &[u8], at: usize) -> bool {
    let mut j = at;
    while j < bytes.len() && (bytes[j] == b' ' || bytes[j] == b'\t') {
        j += 1;
    }
    let start = j;
    while j < bytes.len() && bytes[j].is_ascii_alphabetic() {
        j += 1;
    }
    if j == start || j >= bytes.len() || !(bytes[j] == b' ' || bytes[j] == b'\t') {
        return false;
    }
    let word = &bytes[start..j];
    MEMBER_KEYWORDS
        .iter()
        .any(|k| k.as_bytes().eq_ignore_ascii_case(word))
}

/// Replaces the body of every comment (and, with `strings`, every string literal) with spaces,
/// keeping the document's length and line structure intact so byte offsets still map to the
/// original text.
///
/// The compile-time gate needs this because `CodeMode = objectgenerator` is meaningful only as a UDL
/// keyword. The same characters inside a comment are documentation and inside a string literal are
/// data, and refusing those (#135) blocks a class from documenting the very rule the gate enforces.
///
/// Comments and strings are consumed in one pass rather than stripped in stages, because they
/// interleave: `Set x = "a;b"` has no comment, and `/// don't` has no string. Handling them
/// separately lets either construct desynchronize the scan and hide a real generator keyword
/// further down the document.
///
/// A class is not all ObjectScript. A Python body, `&html<>`, `&js<>`, `&sql()` and an XData block
/// can hold a lone `"` or a `/*` that opens a string or comment IRIS never sees (FR-013). Two
/// rules keep that from swallowing what follows:
///
/// - A string literal ends at the end of its line. ObjectScript strings do not span lines, so an
///   open one at a newline was never a string: the opener is dropped and the line is read again as
///   code.
/// - In a UDL class, a block comment that is still open when a member declaration starts a line
///   was never a comment (a method header cannot be inside one): the opener is dropped and the
///   text is read again as code, so `///` doc comments inside the stretch still blank normally.
///
/// An unterminated block comment or string at the end of the document is left **intact** instead
/// of blanked to the end. Blanking it would let `/*` on its own line switch the gate off for
/// everything after it. UDL that unbalanced is not going to compile anyway, so scanning it raw
/// costs nothing but closes the hole.
fn blank(content: &str, mode: Blank) -> String {
    #[derive(PartialEq)]
    enum State {
        Code,
        LineComment,
        BlockComment,
        Str,
    }

    let bytes = content.as_bytes();
    let mut out: Vec<u8> = content.to_owned().into_bytes();
    let mut state = State::Code;
    // Where the currently open comment or string started, so it can be restored.
    let mut opened_at = 0usize;
    // Brace depth in code state. Class body is 1; a method body is 2 or more. `;` only starts a
    // comment where the text is ObjectScript, which in UDL is inside a member body.
    let mut depth = 0usize;
    let mut i = 0usize;

    // Blanks one byte unless it is a newline: line numbers have to survive.
    let blank_at = |out: &mut Vec<u8>, at: usize| {
        if out[at] != b'\n' {
            out[at] = b' ';
        }
    };

    while i < bytes.len() {
        let c = bytes[i];
        let next = bytes.get(i + 1).copied();
        match state {
            State::Code => {
                if c == b'/' && next == Some(b'/') {
                    // Covers `//` and the `///` doc form alike.
                    state = State::LineComment;
                    opened_at = i;
                } else if c == b'/' && next == Some(b'*') {
                    state = State::BlockComment;
                    opened_at = i;
                    blank_at(&mut out, i);
                    i += 1;
                    blank_at(&mut out, i);
                } else if (c == b';' && !(mode.udl && depth == 1))
                    || (c == b'#' && next == Some(b';'))
                {
                    state = State::LineComment;
                    opened_at = i;
                } else if c == b'"' {
                    state = State::Str;
                    opened_at = i;
                    if mode.strings {
                        blank_at(&mut out, i);
                    }
                } else if c == b'{' {
                    depth += 1;
                } else if c == b'}' {
                    depth = depth.saturating_sub(1);
                } else if c == b'\n' && mode.udl && member_header_at(bytes, i + 1) {
                    // A member declaration is at class level whatever the braces before it said.
                    depth = 1;
                }
            }
            State::LineComment => {
                if c == b'\n' {
                    state = State::Code;
                    if mode.udl && member_header_at(bytes, i + 1) {
                        depth = 1;
                    }
                } else {
                    blank_at(&mut out, i);
                }
            }
            State::BlockComment => {
                blank_at(&mut out, i);
                if c == b'*' && next == Some(b'/') {
                    i += 1;
                    blank_at(&mut out, i);
                    state = State::Code;
                } else if c == b'\n' && mode.udl && member_header_at(bytes, i + 1) {
                    out[opened_at..=i].copy_from_slice(&bytes[opened_at..=i]);
                    state = State::Code;
                    i = opened_at + 1;
                    continue;
                }
            }
            State::Str => {
                if c == b'\n' {
                    // Not a string after all. Read the rest of the line as code.
                    out[opened_at..=i].copy_from_slice(&bytes[opened_at..=i]);
                    state = State::Code;
                    i = opened_at + 1;
                    continue;
                }
                if mode.strings {
                    blank_at(&mut out, i);
                }
                if c == b'"' {
                    if next == Some(b'"') {
                        // `""` is the ObjectScript escape — still inside the literal.
                        i += 1;
                        if mode.strings {
                            blank_at(&mut out, i);
                        }
                    } else {
                        state = State::Code;
                    }
                }
            }
        }
        i += 1;
    }

    // A line comment always ends at the newline or at EOF, so only these two can be left open.
    if state == State::BlockComment || state == State::Str {
        out[opened_at..].copy_from_slice(&bytes[opened_at..]);
    }

    // Every replacement was a single ASCII byte swapped for another, so this cannot fail.
    String::from_utf8(out).unwrap_or_else(|_| content.to_string())
}

/// A compile-time construct found in a document.
struct Hit {
    offset: usize,
    matched: String,
    what: &'static str,
    remediation: &'static str,
}

const REMEDY_GENERATOR: &str =
    "Remove the CodeMode keyword from the member, or use CodeMode = code (the default, which can \
     simply be omitted). If you need generator logic, implement it as a regular ClassMethod that \
     is called explicitly. CodeMode = expression and CodeMode = call are accepted. Comments and \
     string literals are not scanned, so documenting the keyword is fine — only a UDL keyword \
     list is refused.";
const REMEDY_DIRECTIVE: &str =
    "Replace the directive with a ClassMethod that is called at run time, or with a #define whose \
     value is a literal. #if, #ifdef and #ifndef on a literal or a $$$macro are accepted. \
     Comments and string literals are not scanned, so documenting the directive is fine.";
const REMEDY_PARAMETER: &str =
    "Give the Parameter a literal value, or compute the value in a ClassMethod. A value in braces \
     is an expression IRIS evaluates while the class compiles.";
const REMEDY_PROJECTION: &str =
    "Remove the Projection member. IRIS calls the projection class's CreateProjection method \
     while the class compiles. Only %Projection.* system classes are accepted.";
const REMEDY_DDL: &str =
    "Write the body without the directive. A CREATE or ALTER statement compiles the class it \
     generates, and a preprocessor directive in its ObjectScript body runs during that compile, \
     before anything calls the procedure. Put the logic in a ClassMethod that the body calls, and \
     store the class with iris_doc.";

/// Gate: block documents that run code while IRIS compiles them.
///
/// IRIS runs these constructs during compilation, in the process that serves the compile request,
/// with that request's identity and not the service account's:
///
/// - `CodeMode = objectgenerator` and `CodeMode = generator` method generators, quoted or not.
///   `CodeMode = expression` and `CodeMode = call` run nothing at compile time and are accepted
///   (measured on 2026.2).
/// - the preprocessor directives `#execute`, `##expression`, `##function`, and `#if` / `#elseif`
///   with an operand that calls code. `##sql` is embedded SQL run at call time and is not matched.
/// - a braced `Parameter P = {expr}` value.
/// - a `Projection` member naming a class outside `%Projection.*`.
///
/// It checks `.cls` (UDL, or class XML put under a `.cls` name), `.mac` and `.inc`. A `.inc` is
/// never compiled alone: its directives run inside every class or routine that includes it, so a
/// directive in one is refused when it is written. `.int` is the preprocessor's output and holds
/// no compile-time code; it is not scanned.
///
/// This fires on the **assembled document content** (not individual edits), so multi-call
/// assembly tricks (split across insert_lines calls) are irrelevant — the full content is always
/// scanned before it reaches IRIS. `iris_compile` by name runs the same check on the stored source.
///
/// A construct counts only where IRIS acts on it: outside comments and string literals, and for
/// `CodeMode` inside a `[ ... ]` keyword list. Anchoring on brackets alone still refuses
/// `/// never write [ CodeMode = objectgenerator ]`, which is the most useful way to document the
/// rule; blanking comments alone accepts the keyword loose in a method body, where it means
/// nothing.
///
/// Returns `Some(error_json)` when blocked, `None` when safe.
pub fn check_compile_time_code_mode(content: &str, doc_name: &str) -> Option<serde_json::Value> {
    let lower = doc_name.to_lowercase();
    let hit = if lower.ends_with(".cls") {
        if looks_like_class_xml(content) {
            scan_class_xml(content)
        } else {
            scan_class(content)
        }
    } else if lower.ends_with(".mac") || lower.ends_with(".inc") {
        scan_routine(content)
    } else {
        None
    }?;
    Some(refusal(content, doc_name, &hit))
}

fn refusal(content: &str, doc_name: &str, hit: &Hit) -> serde_json::Value {
    let (line_no, line_text) = locate(content, hit.offset);
    serde_json::json!({
        "success": false,
        "error_code": "COMPILE_TIME_EXEC_BLOCKED",
        "code_edit_blocked": true,
        "document": doc_name,
        "matched": hit.matched,
        "line": line_no,
        "line_text": line_text,
        "message": format!(
            "Document '{}' contains {} ({}) on line {}: {}. IRIS runs this while the document \
             compiles, with the identity of the request that compiles it, which the service \
             account does not limit.",
            doc_name, hit.what, hit.matched, line_no, line_text
        ),
        "remediation": hit.remediation,
    })
}

/// Class XML begins with an XML declaration or an `<Export>` root.
fn looks_like_class_xml(content: &str) -> bool {
    let head = content.trim_start().to_ascii_lowercase();
    head.starts_with("<?xml") || head.starts_with("<export")
}

fn regex(slot: &'static OnceLock<Regex>, pattern: &str) -> &'static Regex {
    slot.get_or_init(|| Regex::new(pattern).expect("static pattern is valid"))
}

/// `#execute`, `##expression`, `##function`, and an unsafe `#if` / `#elseif`, in text where
/// comments and strings are already blanked. Earliest wins.
fn find_directive(scannable: &str) -> Option<Hit> {
    static SIMPLE: OnceLock<Regex> = OnceLock::new();
    static CONDITION: OnceLock<Regex> = OnceLock::new();
    let simple = regex(&SIMPLE, r"(?i)#execute\b|##(?:expression|function)\b")
        .find(scannable)
        .map(|m| Hit {
            offset: m.start(),
            matched: m.as_str().to_ascii_lowercase(),
            what: "a preprocessor directive that runs code at compile time",
            remediation: REMEDY_DIRECTIVE,
        });
    let condition = regex(&CONDITION, r"(?im)^[ \t]*(#(?:if|elseif))\b([^\n]*)")
        .captures_iter(scannable)
        .find(|caps| !condition_is_literal(&caps[2]))
        .map(|caps| Hit {
            offset: caps.get(1).map_or(0, |m| m.start()),
            matched: caps[1].to_ascii_lowercase(),
            what: "a preprocessor condition that calls code at compile time",
            remediation: REMEDY_DIRECTIVE,
        });
    [simple, condition]
        .into_iter()
        .flatten()
        .min_by_key(|h| h.offset)
}

/// Is a `#if` operand made only of literals, operators and `$$$macro` names? Anything that can
/// call code is not: `##class(...)`, `$$label^rtn`, `$Function`, `^global`, `@indirection`, `..method`.
fn condition_is_literal(operand: &str) -> bool {
    static MACRO: OnceLock<Regex> = OnceLock::new();
    let without_macros = regex(&MACRO, r"\$\$\$[A-Za-z0-9_%.]*").replace_all(operand, " ");
    !without_macros.contains(['#', '$', '^', '@']) && !without_macros.contains("..")
}

/// `CodeMode = objectgenerator|generator` in a UDL keyword list, quoted or not.
fn find_generator(scannable: &str, original: &str) -> Option<Hit> {
    let upper = scannable.to_ascii_uppercase();
    let bracket_depth_at = bracket_depths(&upper);
    let mut search = 0;
    while let Some(pos) = upper[search..].find("CODEMODE") {
        let keyword_at = search + pos;
        let after_keyword = keyword_at + "CODEMODE".len();
        search = after_keyword;
        if bracket_depth_at[keyword_at] == 0 {
            continue;
        }
        // Whitespace, `=`, whitespace, then the value. A quoted value is blanked in the scannable
        // copy, so the value is read from the original text, and the walk stops at a quote that
        // the scannable copy shows as space.
        let skip_blank = |mut p: usize| {
            while p < upper.len() && upper.as_bytes()[p].is_ascii_whitespace() {
                if original.as_bytes()[p] == b'"' {
                    break;
                }
                p += 1;
            }
            p
        };
        let at_eq = skip_blank(after_keyword);
        if upper.as_bytes().get(at_eq) != Some(&b'=') {
            continue;
        }
        let value_at = skip_blank(at_eq + 1);
        let tail = &original[value_at..];
        let value = match tail.strip_prefix('"') {
            Some(inner) => inner.split('"').next().unwrap_or("").trim(),
            None => tail
                .split(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
                .next()
                .unwrap_or(""),
        };
        let mode = value.to_ascii_lowercase();
        // `generator` is the older, non-object method generator; it runs at compile time too.
        if mode == "objectgenerator" || mode == "generator" {
            return Some(Hit {
                offset: keyword_at,
                matched: format!("CodeMode = {mode}"),
                what: "a method generator",
                remediation: REMEDY_GENERATOR,
            });
        }
    }
    None
}

/// `Parameter P ... = {` in blanked text.
fn find_braced_parameter(scannable: &str) -> Option<Hit> {
    static PARAMETER: OnceLock<Regex> = OnceLock::new();
    let re = regex(
        &PARAMETER,
        r"(?im)^[ \t]*(parameter)[ \t]+[%\w.]+(?:\s+as\s+[%\w.]+(?:\([^)]*\))?)?(?:\s*\[[^\]]*\])?\s*=\s*\{",
    );
    re.captures(scannable).map(|caps| Hit {
        offset: caps.get(1).map_or(0, |m| m.start()),
        matched: "Parameter with a braced value".to_string(),
        what: "a class parameter whose value is an expression evaluated at compile time",
        remediation: REMEDY_PARAMETER,
    })
}

/// `Projection P As Some.Class` where the class is not a `%Projection.*` system class.
fn find_projection(scannable: &str) -> Option<Hit> {
    static PROJECTION: OnceLock<Regex> = OnceLock::new();
    let re = regex(
        &PROJECTION,
        r"(?im)^[ \t]*(projection)[ \t]+[%\w.]+\s+as\s+([%\w.]+)",
    );
    re.captures_iter(scannable)
        .find(|caps| !caps[2].to_ascii_lowercase().starts_with("%projection."))
        .map(|caps| Hit {
            offset: caps.get(1).map_or(0, |m| m.start()),
            matched: format!("Projection as {}", &caps[2]),
            what: "a projection member that runs its class when this class compiles",
            remediation: REMEDY_PROJECTION,
        })
}

/// The `.inc` documents a class or routine names, in order of first mention, each once.
///
/// IRIS runs a compile-time directive in an `.inc` inside every class or routine that includes
/// it, so the scan of a document is not complete until the scan of its includes is. The names come
/// from `Include` and `IncludeGenerator` at the top of a class (alone, or a parenthesised list),
/// from `#include` anywhere in a class or routine, and from `<IncludeCode>` in class XML. A name
/// inside a comment or a string is not an include. Names are compared without regard to case, as
/// IRIS compares them.
pub fn included_documents(content: &str, doc_name: &str) -> Vec<String> {
    static HASH: OnceLock<Regex> = OnceLock::new();
    static UDL: OnceLock<Regex> = OnceLock::new();
    static XML: OnceLock<Regex> = OnceLock::new();
    let lower = doc_name.to_lowercase();
    let is_class = lower.ends_with(".cls");
    let mut lists: Vec<(usize, String)> = Vec::new();
    let mut collect = |re: &Regex, text: &str| {
        lists.extend(
            re.captures_iter(text)
                .map(|c| (c.get(0).map_or(0, |m| m.start()), c[1].to_string())),
        );
    };
    let hash = regex(&HASH, r"(?im)^[ \t]*#include[ \t]+([%\w.]+)");
    if is_class && looks_like_class_xml(content) {
        collect(
            regex(&XML, r"(?i)<Include(?:Code|Generator)?>([^<]*)</"),
            content,
        );
        collect(hash, content);
    } else if is_class {
        let scannable = blank(content, Blank::COMMENTS_AND_STRINGS_UDL);
        collect(
            regex(&UDL, r"(?im)^[ \t]*include(?:generator)?[ \t]+([^\n{]*)"),
            &scannable,
        );
        collect(hash, &scannable);
    } else if lower.ends_with(".mac") || lower.ends_with(".inc") {
        collect(hash, &blank(content, Blank::COMMENTS_AND_STRINGS));
    }
    lists.sort_by_key(|(at, _)| *at);
    let mut seen = std::collections::HashSet::new();
    let mut names = Vec::new();
    for (_, list) in lists {
        for name in list.split(|c: char| c == ',' || c == '(' || c == ')' || c.is_whitespace()) {
            let valid = name
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '%' | '_' | '.'));
            if !name.is_empty() && valid && seen.insert(name.to_ascii_lowercase()) {
                names.push(format!("{name}.inc"));
            }
        }
    }
    names
}

/// A compile-time directive in the ObjectScript of a `CREATE` or `ALTER` statement.
///
/// `CREATE PROCEDURE`, `FUNCTION`, `METHOD`, `QUERY` and `TRIGGER` with `LANGUAGE OBJECTSCRIPT`
/// generate a class method from the body and compile it, so a directive in the body runs while the
/// statement executes (measured: `##expression` in a `CREATE PROCEDURE` body sets its sentinel).
/// `CREATE TABLE` with `COMPUTECODE` does the same through the table's class. The scan starts at
/// the first `CREATE` or `ALTER` and reads the rest of the text as ObjectScript: SQL comments are
/// not skipped, because `--` is two unary minuses in ObjectScript and a directive after it runs.
/// A SQL comment that mentions a directive after a `CREATE` is a false block, which is the safe
/// side.
fn find_ddl_code(sql: &str) -> Option<Hit> {
    static STATEMENT: OnceLock<Regex> = OnceLock::new();
    let start = regex(&STATEMENT, r"(?i)\b(?:create|alter)\b")
        .find(sql)?
        .start();
    find_directive(&blank(&sql[start..], Blank::COMMENTS_AND_STRINGS)).map(|hit| Hit {
        offset: start + hit.offset,
        what: "a preprocessor directive in SQL that creates code",
        remediation: REMEDY_DDL,
        ..hit
    })
}

fn earliest(hits: impl IntoIterator<Item = Option<Hit>>) -> Option<Hit> {
    hits.into_iter().flatten().min_by_key(|h| h.offset)
}

/// A UDL class: generators, directives, braced parameters, projections.
fn scan_class(content: &str) -> Option<Hit> {
    let scannable = blank(content, Blank::COMMENTS_AND_STRINGS_UDL);
    earliest([
        find_generator(&scannable, content),
        find_directive(&scannable),
        find_braced_parameter(&scannable),
        find_projection(&scannable),
    ])
}

/// A `.mac` or `.inc`: directives only. Routines have no UDL keywords.
fn scan_routine(content: &str) -> Option<Hit> {
    find_directive(&blank(content, Blank::COMMENTS_AND_STRINGS))
}

/// Class XML put under a `.cls` name, which IRIS imports, stores as UDL and compiles. The same
/// constructs, in their XML spelling, plus the directives anywhere in the text. Documentation
/// that quotes a directive inside an XML export is a false block, which is the safe side.
fn scan_class_xml(content: &str) -> Option<Hit> {
    static GENERATOR: OnceLock<Regex> = OnceLock::new();
    static PARAMETER: OnceLock<Regex> = OnceLock::new();
    static PROJECTION: OnceLock<Regex> = OnceLock::new();
    let generator = regex(
        &GENERATOR,
        r"(?i)<CodeMode>\s*(objectgenerator|generator)\s*</CodeMode>",
    )
    .captures(content)
    .map(|caps| Hit {
        offset: caps.get(0).map_or(0, |m| m.start()),
        matched: format!("<CodeMode>{}", caps[1].to_ascii_lowercase()),
        what: "a method generator in class XML",
        remediation: REMEDY_GENERATOR,
    });
    let parameter = regex(&PARAMETER, r"(?i)<Default>\s*\{")
        .find(content)
        .map(|m| Hit {
            offset: m.start(),
            matched: "<Default> with a braced value".to_string(),
            what: "a class parameter whose value is an expression evaluated at compile time",
            remediation: REMEDY_PARAMETER,
        });
    let projection = regex(&PROJECTION, r"(?i)<Projection\b")
        .find(content)
        .map(|m| Hit {
            offset: m.start(),
            matched: "<Projection>".to_string(),
            what: "a projection member in class XML",
            remediation: REMEDY_PROJECTION,
        });
    earliest([generator, parameter, projection, find_directive(content)])
}

/// Square-bracket nesting depth at every byte offset, so a match can be tested for being inside
/// a UDL keyword list without re-scanning from the top each time.
fn bracket_depths(scannable: &str) -> Vec<i32> {
    let mut depths = Vec::with_capacity(scannable.len() + 1);
    let mut depth = 0i32;
    for b in scannable.bytes() {
        depths.push(depth);
        match b {
            b'[' => depth += 1,
            b']' => depth = (depth - 1).max(0),
            _ => {}
        }
    }
    depths.push(depth);
    depths
}

/// The 1-based line number containing `offset`, and that line with surrounding space trimmed.
fn locate(content: &str, offset: usize) -> (usize, String) {
    let head = &content[..offset.min(content.len())];
    let line_no = head.matches('\n').count() + 1;
    let start = head.rfind('\n').map(|i| i + 1).unwrap_or(0);
    let end = content[start..]
        .find('\n')
        .map(|i| start + i)
        .unwrap_or(content.len());
    (line_no, content[start..end].trim().to_string())
}

/// Gate: block write-mode SQL that edits the code dictionary, and DDL whose ObjectScript body
/// runs code while IRIS compiles it.
///
/// Only meaningful for `iris_query` mode="write" (DML) and a forced read; read/SELECT
/// introspection against `%Dictionary.Compiled*` is unaffected. Returns `Some(error_json)` when
/// blocked.
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
    find_ddl_code(sql).map(|hit| refusal(sql, "the SQL statement", &hit))
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
            "Reaching class or routine code through arbitrary execution is refused (matched '{}') \
             for server '{}'. This is a lexical check on the text of the call: it covers \
             %Dictionary.*Definition classes, $system.OBJ Load/Compile/UnCompile/Delete, \
             %RoutineMgr and %Routine, the routine-buffer commands (ZSAVE and relatives), and any \
             reference to a code-storage global — reads as well as writes, because it matches the \
             name and cannot tell the two apart. A call that builds the name at run time gets past \
             it; the boundary that holds is the service account (an IRIS user without \
             %Development and without write access to the routine database). Use the dedicated \
             tools instead: they are auditable and SCM-gated.",
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

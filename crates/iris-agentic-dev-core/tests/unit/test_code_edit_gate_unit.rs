//! Tests for `policy::code_edit_gate` — the ObjectScript, compile-time, and SQL code-edit
//! checks, plus the routine-vs-global caret disambiguation.
//!
//! Moved out of an inline `#[cfg(test)] mod tests`: assertion messages in such a module only
//! execute when a test fails, so eleven of this file's twelve uncovered lines were its own
//! failure messages. `extract_globals` is private, so the tests call the
//! `#[cfg(feature = "testing")]` accessor.

use iris_agentic_dev_core::policy::code_edit_gate::{
    check_compile_time_code_mode, check_objectscript_code_edit, check_sql_code_edit,
    extract_globals_for_tests,
};

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
    assert!(check_objectscript_code_edit("do $system.OBJ.Compile(\"My.Class\")", "srv").is_some());
}

#[test]
fn blocks_system_obj_load() {
    assert!(
        check_objectscript_code_edit("do $System.OBJ.Load(\"/tmp/x.xml\",\"ck\")", "srv").is_some()
    );
}

#[test]
fn blocks_system_obj_delete() {
    assert!(check_objectscript_code_edit("do $SYSTEM.OBJ.Delete(\"My.Class\")", "srv").is_some());
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

/// The token list is written in the dotted `$system.OBJ.Compile` form. `##class(%SYSTEM.OBJ)`
/// puts a parenthesis where the dot goes, so every one of these reached IRIS with the gate
/// reporting nothing — a complete bypass of the non-configurable code-edit block.
#[test]
fn blocks_hash_class_call_form() {
    for code in &[
        r#"do ##class(%SYSTEM.OBJ).Compile("My.Class","ck")"#,
        r#"do ##class(%SYSTEM.OBJ).Delete("My.Class")"#,
        r#"do ##class(%SYSTEM.OBJ).Load("/tmp/x.xml","ck")"#,
        r#"set sc=##class( %SYSTEM.OBJ ).Import("/tmp/x.xml")"#,
        r#"do ##class(%SYSTEM.OBJ).MakeClassDeployed("My.Class")"#,
    ] {
        assert!(
            check_objectscript_code_edit(code, "srv").is_some(),
            "##class() call form must be blocked: {code}"
        );
    }
}

/// `$classmethod` reaches the same methods through indirection: quotes and a comma sit where
/// the dot would be. Flattening call punctuation covers this with the same token list.
#[test]
fn blocks_classmethod_indirection() {
    for code in &[
        r#"do $classmethod("%SYSTEM.OBJ","Compile","My.Class")"#,
        r#"set sc=$CLASSMETHOD("%SYSTEM.OBJ","Delete","My.Class")"#,
        r#"do $classmethod("%Compiler.UDL.TextServices","SetTextFromString",,,"My.Class",t)"#,
        r#"set c=$classmethod("%Dictionary.ClassDefinition","%OpenId","My.Class")"#,
    ] {
        assert!(
            check_objectscript_code_edit(code, "srv").is_some(),
            "$classmethod indirection must be blocked: {code}"
        );
    }
}

/// Flattening must not start blocking ordinary code that happens to use parentheses.
#[test]
fn flattening_does_not_block_ordinary_calls() {
    for code in &[
        r#"write ##class(%SYSTEM.Version).GetVersion()"#,
        r#"set x=$classmethod("MyApp.Util","Format","abc")"#,
        r#"do ##class(%SYS.Journal.System).GetCurrentFile()"#,
        r#"write ##class(%Dictionary.CompiledClass).%OpenId("My.Class").Name"#,
    ] {
        assert!(
            check_objectscript_code_edit(code, "srv").is_none(),
            "ordinary call must stay permitted: {code}"
        );
    }
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
    assert!(
        check_objectscript_code_edit("set ^MyApp.Data(1)=\"ok\" write ^MyApp.Data(1)", "srv")
            .is_none()
    );
}

/// Recovering the last SystemPerformance run ID is a documented read of a diagnostic
/// data global. `^IRIS.Sys*` used to swallow `^IRIS.SystemPerformance` and report it as
/// a code edit, which made pbuttons history unreadable through `iris_execute`.
#[test]
fn allows_systemperformance_history_read() {
    let code = r#"set tLast=$order(^IRIS.SystemPerformance("history",""),-1) write tLast,!"#;
    assert!(
        check_objectscript_code_edit(code, "srv").is_none(),
        "reading pbuttons run history must not be gated as a code edit"
    );
}

#[test]
fn still_blocks_iris_sys_dot_config_write() {
    assert!(
        check_objectscript_code_edit("set ^IRIS.Sys.Config(\"x\")=1", "srv").is_some(),
        "^IRIS.Sys.* must stay blocked"
    );
}

/// `label^routine` and `$$label^routine` are routine references, not global references.
/// `^SystemPerformance` is a routine — the blocklist pattern `^SYS*` was matching it and
/// refusing every SystemPerformance call as a code edit.
#[test]
fn allows_routine_calls_that_look_like_blocked_globals() {
    for code in &[
        r#"set tRun=$$run^SystemPerformance("test")"#,
        "do run^SystemPerformance",
        r#"set tWait=$$waittime^SystemPerformance("20260904_154240_test")"#,
        "do ^SystemPerformance",
        "do ^%SS",
        "goto start^SYSTEMx",
    ] {
        assert!(
            check_objectscript_code_edit(code, "srv").is_none(),
            "routine reference must not be gated as a code-storage global write: {code}"
        );
    }
}

/// The routine-reference carve-out must not become a way to write code globals.
#[test]
fn still_blocks_code_global_writes_next_to_routine_calls() {
    assert!(
        check_objectscript_code_edit(r#"do run^SystemPerformance set ^oddDEF("X")=1"#, "srv")
            .is_some(),
        "a code-global write alongside a routine call must still be blocked"
    );
    assert!(
        check_objectscript_code_edit("kill ^ROUTINE", "srv").is_some(),
        "kill of a code global must stay blocked"
    );
}

#[test]
fn extract_globals_skips_routine_references() {
    assert_eq!(
        extract_globals_for_tests(r#"set x=$$run^SystemPerformance("test")"#),
        Vec::<String>::new()
    );
    assert_eq!(
        extract_globals_for_tests("do ^SystemPerformance"),
        Vec::<String>::new()
    );
    assert_eq!(
        extract_globals_for_tests("goto exit^MyRtn"),
        Vec::<String>::new()
    );
    // A real global read in the same line is still extracted.
    assert_eq!(
        extract_globals_for_tests(r#"do init^MyRtn write ^MyApp.Data(1)"#),
        vec!["MyApp.Data".to_string()]
    );
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

/// IRIS SQL accepts whitespace around the dot and quoted identifiers for the same table.
/// Matching the raw uppercased text let both spellings through.
#[test]
fn blocks_sql_dictionary_with_spacing_and_quotes() {
    for sql in &[
        "DELETE FROM %Dictionary . ClassDefinition WHERE ID='X'",
        "UPDATE \"%Dictionary\".\"MethodDefinition\" SET Name='x'",
        "DELETE FROM %Dictionary\n  .ClassDefinition",
    ] {
        assert!(
            check_sql_code_edit(sql, "srv").is_some(),
            "dictionary write must be blocked regardless of spelling: {sql}"
        );
    }
}

#[test]
fn allows_sql_write_to_app_table() {
    assert!(check_sql_code_edit("UPDATE MyApp.Patient SET Name='x' WHERE ID=1", "srv").is_none());
}

// ── extract_globals ───────────────────────────────────────────────────────
#[test]
fn extract_globals_basic() {
    let g = extract_globals_for_tests("set ^foo(1)=2 set x=^bar");
    assert_eq!(g, vec!["foo".to_string(), "bar".to_string()]);
}

#[test]
fn extract_globals_percent_and_dotted() {
    let g = extract_globals_for_tests("write ^%Dictionary.x, ^Ens.Config");
    assert!(g.contains(&"%Dictionary.x".to_string()));
    assert!(g.contains(&"Ens.Config".to_string()));
}

#[test]
fn extract_globals_extended_reference() {
    let g = extract_globals_for_tests(r#"set ^["USER"]oddDEF(1)=2"#);
    assert!(g.contains(&"oddDEF".to_string()));
}

#[test]
fn error_shape_has_code_and_remediation() {
    let e = check_objectscript_code_edit("do $system.OBJ.Compile(\"X\")", "srv").unwrap();
    assert_eq!(e["error_code"], "CODE_EDIT_BLOCKED");
    assert_eq!(e["code_edit_blocked"], true);
    assert!(e["remediation"].as_str().unwrap().contains("iris_doc"));
}

/// The gate matches any reference to a code-storage global, read included, so the message
/// must not claim the code was editing something — that sends the caller looking for a
/// write it never made, and the old remediation only named the write-side tools.
#[test]
fn error_message_covers_reads_not_just_edits() {
    let e = check_objectscript_code_edit(r#"write $Get(^ROUTINE("X",0,0))"#, "srv").unwrap();
    let msg = e["message"].as_str().unwrap();
    assert!(
        msg.contains("read"),
        "message must say reads are blocked too, got: {msg}"
    );
    let rem = e["remediation"].as_str().unwrap();
    assert!(
        rem.contains("mode=\"get\"") || rem.contains("iris_symbols"),
        "remediation must name a read path, got: {rem}"
    );
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
    let cls =
        "Class My.Sneaky {\nMethod Pwn() [ CodeMode = objectgenerator ]\n{\n  do badstuff\n}\n}";
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

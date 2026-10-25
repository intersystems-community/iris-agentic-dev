//! Spec 141 US3 (FR-019) and R5 C23 (EXEC-22): the code-edit block's own list of code-storage
//! globals, comments blanked before scanning and string literals not, plus the code-management
//! entry points the 1.6.3 list missed.

use iris_agentic_dev_core::policy::code_edit_gate::check_objectscript_code_edit;

fn blocked(code: &str) -> serde_json::Value {
    check_objectscript_code_edit(code, "srv").unwrap_or_else(|| panic!("should be blocked: {code}"))
}

/// Asserts the code-edit block refuses `code`.
macro_rules! assert_blocked {
    ($code:expr) => {
        assert!(
            check_objectscript_code_edit($code, "srv").is_some(),
            "should be blocked: {}",
            $code
        )
    };
}

/// Asserts the code-edit block lets `code` through.
macro_rules! assert_allowed {
    ($code:expr) => {
        if let Some(r) = check_objectscript_code_edit($code, "srv") {
            panic!("should pass: {}\n{r}", $code);
        }
    };
}

// ── R5 C23: the code-edit block's own list ───────────────────────────────

#[test]
fn reading_interop_configuration_is_not_a_code_edit() {
    assert_allowed!("Write $Data(^Ens.Config.ItemD)");
    assert_allowed!("Set tName = $Order(^Ens.Config.ProductionD(\"\"))");
    assert_allowed!("Write $Data(^Ens.Rule.Foo), $Data(^Ens.MessageHeader)");
    assert_allowed!("Write $Data(^IRIS.Temp.Foo), $Data(^DeepSee.Cache)");
}

#[test]
fn a_comment_naming_a_code_global_is_not_a_code_edit() {
    assert_allowed!("Write 1 ; Kill ^oddDEF would be refused");
    assert_allowed!("; Kill ^oddDEF");
    assert_allowed!("Write 1 // never touch ^ROUTINE");
    assert_allowed!("Write 1 /* ^rMAC and ^rINC are code storage */");
    assert_allowed!("/* %SYSTEM.OBJ.Load is not called */ Write 1");
    assert_allowed!("Write 1 ;; $system.OBJ.Delete(\"X\") is not called");
}

#[test]
fn code_storage_globals_are_still_refused() {
    for code in [
        "Kill ^oddDEF",
        "Set ^oddCOM(\"X\")=1",
        "Set ^oddPROC(\"X\")=1",
        "Set ^ROUTINE(\"X\")=1",
        "Set ^rOBJ(\"X\")=1",
        "Set ^rMAC(\"X\",0)=1",
        "Set ^rINC(\"X\",0)=1",
        "Set ^rINDEX(\"X\")=1",
        "Set ^rINDEXCLASS(\"X\")=1",
        "Set ^rMACSAVE(\"X\")=1",
        "Set ^oddStudioDocument(\"X\")=1",
        "Kill ^%Dictionary",
    ] {
        assert_blocked!(code);
    }
}

#[test]
fn string_literals_are_still_scanned() {
    // `@"^oddDEF"` and `$ClassMethod("%SYSTEM.OBJ", ...)` are only caught through their literals.
    assert_blocked!("Set x=@\"^oddDEF\"");
    assert_blocked!("Do $ClassMethod(\"%SYSTEM.OBJ\",\"Load\",\"x\")");
    assert_blocked!("Set g=\"^ROUTINE\" Kill @g");
}

#[test]
fn a_comment_marker_inside_a_string_does_not_hide_the_rest() {
    assert_blocked!("Write \"a;b\" Kill ^oddDEF");
    assert_blocked!("Write \"//\" Kill ^oddDEF");
    assert_blocked!("Write \"/*\" Kill ^oddDEF");
}

#[test]
fn the_system_configuration_globals_stay_blocked() {
    assert_blocked!("Set ^IRIS.Sys.Config(\"x\")=1");
    assert_blocked!("Set ^%SYS(\"x\")=1");
}

// ── FR-019: what the 1.6.3 list missed ───────────────────────────────────

#[test]
fn the_routine_class_alias_is_refused() {
    assert_blocked!("Set r=##class(%Routine).%New(\"X.mac\") Do r.Write(\" Quit\") Do r.Save()");
    assert_blocked!("Do ##class(%Routine).Delete(\"X.mac\")");
    assert_blocked!("Do $classmethod(\"%Routine\",\"Delete\",\"X.mac\")");
    assert_blocked!("Do ##class( %Routine ).Delete(\"X.mac\")");
}

#[test]
fn uncompile_is_refused() {
    assert_blocked!("Do $SYSTEM.OBJ.UnCompile(\"X\")");
    assert_blocked!("Do $system.OBJ.UnCompileAll()");
    assert_blocked!("Do ##class(%SYSTEM.OBJ).UnCompile(\"X\")");
    assert_blocked!("Do $classmethod(\"%SYSTEM.OBJ\",\"UnCompile\",\"X\")");
}

#[test]
fn routine_buffer_commands_are_refused() {
    for code in [
        "ZInsert \" Quit 1\" ZSave X",
        "ZLoad X",
        "ZRemove",
        "ZSave X",
        "zs X",
        "zl X",
        "zi \" Quit\"",
        "zr",
        "Set x=1 ZSave X",
        "Set x=1 Zs:x X",
        "If x { ZSave X }",
        "Xecute \"ZSave X\"",
        "Xecute \"ZInsert \"\" Quit 42\"\" ZSave IadZsProbe\"",
        "Do foo ZRemove",
    ] {
        assert_blocked!(code);
    }
}

#[test]
fn names_that_only_look_like_buffer_commands_pass() {
    for code in [
        "Write $ZSTRIP(x,\"<W\")",
        "Set zsvar=1",
        "Set x=$ZSEARCH(\"a\")",
        "Do ..zr()",
        "Write $ZS",
        "Set zload2=1",
        "Do obj.ZSaveIt()",
        "Write $ZINSTANCE",
    ] {
        assert_allowed!(code);
    }
}

#[test]
fn the_system_obj_routine_entry_points_are_refused() {
    assert_blocked!("Write $Text(Load^%apiOBJ)");
    assert_blocked!("Do Load^%apiOBJ(\"/tmp/x.xml\",\"-d\")");
    assert_blocked!("Set sc=$$Compile^%apiOBJ(\"X\")");
    assert_blocked!("Do Delete^%apiOBJ(\"X\")");
    assert_blocked!("Do ^%apiOBJ");
}

// ── FR-019: the message says what the block is ───────────────────────────

#[test]
fn the_refusal_says_it_is_a_lexical_check_and_names_the_boundary() {
    let r = blocked("Kill ^oddDEF");
    let msg = r["message"].as_str().unwrap().to_lowercase();
    assert!(
        msg.contains("lexical"),
        "message must call the block a lexical check: {msg}"
    );
    assert!(
        msg.contains("service account"),
        "message must name the service account as the boundary: {msg}"
    );
    assert!(
        !msg.contains("non-configurable") && !msg.contains("cannot be overridden"),
        "message must not promise a hard block: {msg}"
    );
    // The old wording is kept where it was a fact: reads are refused as well as writes.
    assert!(msg.contains("read"), "{msg}");
}

#[test]
fn the_module_documentation_makes_no_promise_the_code_does_not_keep() {
    let src = include_str!("../../src/policy/code_edit_gate.rs");
    let module_doc: String = src
        .lines()
        .take_while(|l| l.starts_with("//!"))
        .collect::<Vec<_>>()
        .join("\n")
        .to_lowercase();
    assert!(
        module_doc.contains("lexical"),
        "module doc must say the check is lexical"
    );
    assert!(
        module_doc.contains("service account"),
        "module doc must name the boundary"
    );
    assert!(
        module_doc.contains("indirection"),
        "module doc must admit what gets past it"
    );
    assert!(
        !module_doc.contains("hard-block") && !module_doc.contains("cannot be overridden"),
        "module doc must not claim a hard block"
    );
}

/// Every global the module doc says is covered must be covered. The 1.6.3 doc said `^rMAC` was
/// caught while the list lacked it (verify H5).
#[test]
fn every_global_the_module_doc_names_is_blocked() {
    let src = include_str!("../../src/policy/code_edit_gate.rs");
    let module_doc: String = src
        .lines()
        .take_while(|l| l.starts_with("//!"))
        .collect::<Vec<_>>()
        .join("\n");
    let mut named = Vec::new();
    for word in
        module_doc.split(|c: char| !(c.is_ascii_alphanumeric() || c == '^' || c == '%' || c == '.'))
    {
        if let Some(g) = word.strip_prefix('^') {
            let g = g.trim_end_matches('.');
            if !g.is_empty() {
                named.push(g.to_string());
            }
        }
    }
    assert!(
        named.len() >= 5,
        "the module doc should name several globals, found {named:?}"
    );
    for g in named {
        assert_blocked!(&format!("Set ^{g}(\"x\")=1"));
    }
}
